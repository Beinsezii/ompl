#![warn(missing_docs)]
use std::error::Error;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver};
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use bus::{Bus, BusReader};
use lexical_sort::natural_lexical_cmp;
use rand::random;

mod player;
mod track;

pub use player::{Backend, Player};
pub use track::{find_tracks, get_taglist, get_taglist_sort, tagstring, Track};

use crate::logging::*;

use self::player::PlayerMessage;

// ## FILTER ## {{{

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct Filter {
    pub tag: String,
    pub items: Vec<String>,
}

/// A Filter and its post-filtering tracks
#[derive(Clone, Debug, PartialEq)]
pub struct FilteredTracks {
    pub filter: Filter,
    pub tracks: Vec<Arc<Track>>,
}

// ## FILTER ## }}}

// ## THEME ## {{{

/// A theme color represented as 8bit sRGB or one of 16 terminal colors
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum Color {
    RGB([u8; 3]),
    Term(u8),
    None,
}

impl TryFrom<&str> for Color {
    type Error = String;
    fn try_from(value: &str) -> Result<Self, String> {
        let value = value.trim().to_lowercase();
        match value.as_str() {
            "none" => Ok(Color::None),
            "black" => Ok(Color::Term(0)),
            "red" => Ok(Color::Term(1)),
            "green" => Ok(Color::Term(2)),
            "yellow" => Ok(Color::Term(3)),
            "blue" => Ok(Color::Term(4)),
            "magenta" => Ok(Color::Term(5)),
            "cyan" => Ok(Color::Term(6)),
            "gray" => Ok(Color::Term(7)),
            "darkgray" => Ok(Color::Term(8)),
            "lightred" => Ok(Color::Term(9)),
            "lightgreen" => Ok(Color::Term(10)),
            "lightyellow" => Ok(Color::Term(11)),
            "lightblue" => Ok(Color::Term(12)),
            "lightmagenta" => Ok(Color::Term(13)),
            "lightcyan" => Ok(Color::Term(14)),
            "white" => Ok(Color::Term(15)),
            val => {
                if let Ok(col) = val.parse::<u8>() {
                    if col < 16 {
                        Ok(Color::Term(col))
                    } else {
                        Err(format!("Terminal color {} greater than 15", col))
                    }
                } else {
                    match colcon::str2space(val, colcon::Space::SRGB) {
                        Some(col) => Ok(Color::RGB(colcon::srgb_to_irgb(col))),
                        None => Err(format!("Unable to parse \"{}\" as a color", val)),
                    }
                }
            }
        }
    }
}

impl TryFrom<String> for Color {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_from(value.as_str())
    }
}

impl ToString for Color {
    fn to_string(&self) -> String {
        match self {
            Color::None => String::from("None"),
            Color::Term(v) => v.to_string(),
            Color::RGB(rgb) => colcon::irgb_to_hex(*rgb),
        }
    }
}

/// 3-tone theme
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub fg: Color,
    pub bg: Color,
    pub acc: Color,
}

impl ToString for Theme {
    fn to_string(&self) -> String {
        format!("fg: {}\nbg: {}\nacc: {}", self.fg.to_string(), self.bg.to_string(), self.acc.to_string())
    }
}

// ## THEME ## }}}

// ### FNs ### {{{

fn player_message_server(library: Arc<Library>, next_r: Receiver<PlayerMessage>) {
    debug!("PMS Start");
    let library_weak = Arc::downgrade(&library);
    drop(library);
    loop {
        let msg = next_r.recv();
        if let Some(library) = library_weak.upgrade() {
            match msg {
                Ok(msg) => match msg {
                    PlayerMessage::Request => match library.repeat_get() {
                        None => {
                            if library.get_queue().last() == library.track_get().as_ref() && !library.shuffle_get() {
                                library.broadcast(LibEvt::Playback)
                            } else {
                                library.next()
                            }
                        }
                        Some(false) => library.play(),
                        Some(true) => library.next(),
                    },

                    PlayerMessage::Seekable | PlayerMessage::Clock => library.broadcast(LibEvt::Playback),
                    PlayerMessage::Error(e) => library.broadcast(LibEvt::Error(e)),
                },
                Err(_) => break,
            }
        } else {
            break;
        }
    }
    debug!("PMS End");
}

// ### FNs ### }}}

#[derive(Debug, Clone)]
pub enum LibEvt {
    /// Simple state change.
    /// Pause/Play/Seek/etc.
    Playback,
    /// Complex state change.
    /// Filters updated, etc.
    Update,
    /// Theme changed.
    Theme,
    /// Non-fatal error.
    Error(String),
}

pub struct Library {
    tracks: RwLock<Vec<Arc<Track>>>,
    history: Mutex<Vec<Arc<Track>>>,
    player: Box<dyn Player>,
    filtered_tree: RwLock<Vec<FilteredTracks>>,
    sorters: RwLock<Vec<String>>,
    /// Broadcaster for all receivers of library events
    bus: Mutex<Bus<LibEvt>>,
    shuffle: AtomicBool,
    /// None - No loop
    /// Some(false) - track loop
    /// Some(true) - full loop
    repeat: RwLock<Option<bool>>,
    /// Scan hidden files during append
    hidden: AtomicBool,
    /// Single line status for library
    statusline: RwLock<String>,
    theme: RwLock<Theme>,
}

impl Library {
    // # new # {{{
    pub fn new(backend: Backend) -> Result<Arc<Self>, Box<dyn Error>> {
        let bus = Mutex::new(Bus::<LibEvt>::new(99));

        let (next_s, next_r) = sync_channel(1);
        let result = Arc::new(Self {
            player: player::backend(backend, next_s),
            tracks: RwLock::new(Vec::new()),
            history: Mutex::new(Vec::new()),
            filtered_tree: RwLock::new(Vec::new()),
            sorters: RwLock::new(Vec::new()),
            bus,
            shuffle: AtomicBool::new(true),
            repeat: RwLock::new(Some(true)),
            hidden: AtomicBool::new(false),
            statusline: RwLock::new(String::from("title")),
            theme: RwLock::new(Theme {
                fg: Color::None,
                bg: Color::None,
                acc: Color::Term(3),
            }),
        });

        result.volume_set(0.5);

        let result_c = result.clone();

        thread::Builder::new()
            .name(String::from("LIBRARY Player Message Server"))
            .spawn(move || player_message_server(result_c, next_r))?;

        Ok(result)
    }
    // # new # }}}

    fn broadcast(&self, message: LibEvt) {
        if let Ok(mut bus) = self.bus.lock() {
            bus.broadcast(message)
        }
    }

    /// Receiver for all library events
    pub fn get_receiver(&self) -> Result<BusReader<LibEvt>, Box<dyn Error>> {
        Ok(self.bus.lock().map_err(|e| e.to_string())?.add_rx())
    }

    // ## Player forwards ## {{{

    pub fn play(&self) {
        self.player.play();
        self.broadcast(LibEvt::Playback);
    }
    pub fn pause(&self) {
        self.player.pause();
        self.broadcast(LibEvt::Playback);
    }
    pub fn stop(&self) {
        self.player.stop();
        self.broadcast(LibEvt::Playback);
    }
    /// Toggle play/pause. Typical media key control
    pub fn play_pause(&self) {
        self.player.toggle();
        self.broadcast(LibEvt::Playback);
    }
    /// 0.0 -> 1.0
    pub fn volume_get(&self) -> f32 {
        self.player.volume_get()
    }
    /// 0.0 -> 1.0
    pub fn volume_set(&self, volume: f32) {
        self.player.volume_set(volume);
        self.broadcast(LibEvt::Playback);
    }
    /// -1.0 -> 1.0
    pub fn volume_add(&self, amount: f32) {
        self.player.volume_add(amount);
        self.broadcast(LibEvt::Playback);
    }

    /// Currently playing/loaded track
    pub fn track_get(&self) -> Option<Arc<Track>> {
        self.player.track_get()
    }

    /// Set the currently loaded track and start playback
    pub fn play_track(&self, track: Option<Arc<Track>>) {
        // Check for moved tracks first.
        // Player could handle this but easier if library does
        if let Some(track) = track.as_ref() {
            if !track.path().exists() {
                self.broadcast(LibEvt::Error(format!(
                    "Track no longer found at {}\nRemoving from library",
                    track.path().to_str().unwrap_or("???")
                )));

                if let Ok(mut tracks) = self.tracks.write() {
                    if let Some(id) = tracks.iter().position(|t| t == track) {
                        tracks.remove(id);
                    }
                    self.force_build_filters();
                    self.next();
                }
                return;
            }
        }
        if let Some(track) = self.player.play_track(track) {
            if let Ok(mut history) = self.history.lock() {
                history.push(track)
            }
        }
        self.broadcast(LibEvt::Playback);
    }

    pub fn playing(&self) -> bool {
        self.player.playing()
    }
    pub fn paused(&self) -> bool {
        self.player.paused()
    }
    pub fn stopped(&self) -> bool {
        self.player.stopped()
    }

    /// Whether the player is ready to seek.
    /// `None` means the player does not support seeking.
    pub fn seekable(&self) -> Option<bool> {
        self.player.seekable()
    }

    /// Track position, Track duration
    pub fn times(&self) -> Option<(Duration, Duration)> {
        self.player.times()
    }

    /// Seek to this exact time
    pub fn seek(&self, time: Duration) {
        self.player.seek(time)
    }

    /// Seek by +-n seconds
    pub fn seek_by(&self, secs: f32) {
        self.player.seek_by(secs)
    }

    /// Generate a waveform preview of the current track
    pub fn waveform(&self, count: usize) -> Option<Vec<f32>> {
        self.player.waveform(count)
    }

    // ## Player Forwards ## }}}

    // ## Other Settings ## {{{

    pub fn shuffle_get(&self) -> bool {
        self.shuffle.load(Ordering::Relaxed)
    }

    pub fn shuffle_set(&self, shuffle: bool) {
        self.shuffle.store(shuffle, Ordering::Relaxed);
        self.broadcast(LibEvt::Playback);
    }

    pub fn shuffle_toggle(&self) {
        self.shuffle_set(!self.shuffle_get())
    }

    /// None - No loop
    /// Some(false) - track loop
    /// Some(true) - full loop
    pub fn repeat_get(&self) -> Option<bool> {
        *self.repeat.read().as_deref().unwrap_or(&Some(true))
    }

    /// None - No loop
    /// Some(false) - track loop
    /// Some(true) - full loop
    pub fn repeat_set(&self, repeat: Option<bool>) {
        if let Ok(mut guard) = self.repeat.write() {
            *guard = repeat;
            self.broadcast(LibEvt::Playback);
        }
    }

    /// Advances None -> Some(false) -> Some(true)
    pub fn repeat_toggle(&self) {
        let Ok(mut guard) = self.repeat.write() else { return };
        *guard = match *guard {
            None => Some(false),
            Some(false) => Some(true),
            Some(true) => None,
        };
        self.broadcast(LibEvt::Playback);
    }

    /// Whether append() scans hidden files
    pub fn hidden_get(&self) -> bool {
        self.hidden.load(Ordering::Relaxed)
    }

    /// Whether append() scans hidden files
    pub fn hidden_set(&self, include_hidden: bool) {
        self.hidden.store(include_hidden, Ordering::Relaxed)
    }

    /// Tagstring for library status
    pub fn statusline_get(&self) -> String {
        self.statusline.read().as_deref().unwrap_or(&String::from("???")).clone()
    }

    /// Tagstring for library status
    pub fn statusline_set<T: ToString>(&self, statusline: T) {
        if let Ok(mut guard) = self.statusline.write() {
            *guard = statusline.to_string();
            self.broadcast(LibEvt::Theme);
        }
    }

    /// Parses tagstring from playing track and statusline
    pub fn statusline_get_format(&self) -> String {
        self.track_get().map_or(String::from(""), |t| t.tagstring(self.statusline_get()))
    }

    pub fn theme_get(&self) -> Theme {
        *self.theme.read().expect("Library theme was not readable")
    }

    pub fn theme_set(&self, theme: Theme) {
        if let Ok(mut guard) = self.theme.write() {
            *guard = theme;
            self.broadcast(LibEvt::Theme);
        }
    }

    // ## Other Settings ## }}}

    // ## Track Controls ## {{{

    /// Get a random track from the filtered queue
    pub fn get_random(&self) -> Option<Arc<Track>> {
        debug!("Getting random track...");
        let tracks = self.get_queue();
        match tracks.len() {
            0 => None,
            1 => Some(tracks[0].clone()),
            _ => loop {
                let track = Some(&tracks[random::<usize>() % tracks.len()]);
                if track != self.track_get().as_ref() {
                    break track.cloned();
                }
            },
        }
    }

    /// Get the next track from the filtered queue. Does not respect `repeat`
    pub fn get_sequential(&self, reverse: bool) -> Option<Arc<Track>> {
        let mut tracks = self.get_queue();
        if reverse {
            tracks = tracks.into_iter().rev().collect()
        }
        let mut i = 0;
        if let Some(track) = self.track_get() {
            for (n, t) in tracks.iter().enumerate() {
                if t == &track {
                    i = n + 1
                }
            }
        }
        if i >= tracks.len() {
            i = 0
        }

        tracks.get(i).cloned()
    }

    /// Play the next track, either shuffled or sequential
    pub fn next(&self) {
        if self.shuffle_get() {
            self.play_track(self.get_random())
        } else {
            self.play_track(self.get_sequential(false))
        };
    }

    /// If shuffle, pop the previous track from history and play it
    /// Else get the prior sequential track
    pub fn previous(&self) {
        if self.shuffle_get() {
            let Ok(mut guard) = self.history.lock() else { return };
            let track = guard.pop();
            drop(guard);
            self.play_track(track);
            // remove twice cause it gets re-added.
            if let Ok(mut history) = self.history.lock() {
                history.pop();
            }
        } else {
            self.play_track(self.get_sequential(true))
        }
    }

    // ## Track Controls ## }}}

    // ## Library Paths Control ## {{{

    /// Get compatible file extensions for the player
    pub fn types(&self) -> Vec<String> {
        self.player.types()
    }

    /// Scan path for compatible file extensions and load tracks into library
    pub fn append_library<T: AsRef<Path>>(&self, path: T) {
        let begin = Instant::now();

        let mut new_tracks: Vec<Track> = find_tracks(path, &self.player.types(), self.hidden_get());
        let mut count = new_tracks.len();

        let now = Instant::now();
        thread::scope(|scope| {
            // 50 is a completely arbitrary value that seems to perform well enough
            // Basically tradeoff between thread spawn overhead and IO calls.
            // I dont want an entire async runtime for loading metadata so here it is
            for chunk in new_tracks.chunks_mut(50) {
                scope.spawn(|| chunk.iter_mut().for_each(|track| track.load_meta()));
            }
        });

        bench!("Probed meta for {} tracks in {:?}", count, now.elapsed());
        let now = Instant::now();

        if let Ok(mut tracks) = self.tracks.write() {
            new_tracks.into_iter().map(|t| Arc::new(t)).for_each(|t| tracks.push(t));
            let len = tracks.len();

            // pushed vals in front
            tracks.reverse();
            // Stable sort should prioritize new tracks due to reverse()
            tracks.sort_by(|a, b| a.path().cmp(b.path()));
            // Therefore the dedupe will use the newly probed tracks
            tracks.dedup_by(|a, b| a.path() == b.path());

            if len > tracks.len() {
                info!("Removed {} duplicate tracks during append", len - tracks.len());
                count -= len - tracks.len()
            }
        }

        bench!("Loaded {} tracks into library in {:?}", count, now.elapsed());

        self.sort();

        if self.player.track_get().is_none() {
            self.player.track_set(if self.shuffle_get() {
                self.get_random()
            } else {
                self.get_sequential(false)
            });
        }

        bench!("Finished appending {} tracks in total {:?}", count, begin.elapsed())
    }

    /// Drop all tracks from the library
    pub fn purge(&self) {
        if let Ok(mut tracks) = self.tracks.write() {
            *tracks = Vec::new();
        }
        self.force_build_filters();
        self.broadcast(LibEvt::Update);
    }

    // ## Library Paths Control ## }}}

    // ## Filters Control ## {{{

    /// rebuilds whole filter tree without caching
    fn force_build_filters(&self) {
        let filters = self.get_filters();
        if let Ok(mut ft) = self.filtered_tree.write() {
            *ft = Vec::new();
        }
        self.set_filters(filters);
    }

    /// Amount of filters
    pub fn filter_count(&self) -> usize {
        self.filtered_tree.read().map(|v| v.len()).unwrap_or(0)
    }

    /// All FilteredTracks. Cloned
    pub fn get_filter_tree(&self) -> Vec<FilteredTracks> {
        self.filtered_tree.read().as_deref().cloned().unwrap_or(Vec::new())
    }

    /// All Filters. Cloned
    pub fn get_filters(&self) -> Vec<Filter> {
        self.filtered_tree
            .read()
            .as_deref()
            .unwrap_or(&Vec::new())
            .iter()
            .map(|ft| ft.filter.clone())
            .collect::<Vec<Filter>>()
    }

    /// Replace all Filters, rebuilding the FilteredTracks
    pub fn set_filters(&self, filters: Vec<Filter>) {
        debug!("Updating filters...");
        let now = Instant::now();
        let mut cache = true;
        let mut filtered_tree = Vec::<FilteredTracks>::new();

        for (i, f) in filters.into_iter().enumerate() {
            // if filters continuously match existing, don't rebuild.
            if let Some(ft) = self.filtered_tree.read().as_ref().ok().map(|v| v.get(i)).flatten() {
                if ft.filter == f && cache {
                    filtered_tree.push(ft.clone());
                    continue;
                } else {
                    cache = false
                }
            }

            let Ok(itracks) = self.tracks.read() else { break };
            let iter = if i == 0 { itracks.iter() } else { filtered_tree[i - 1].tracks.iter() };

            let tracks = if !f.items.is_empty() {
                let mut tracks_f = Vec::new();
                for t in iter {
                    if f.items.contains(&tagstring::parse(&f.tag, &t.tags())) {
                        tracks_f.push(t.clone())
                    }
                }
                tracks_f
            } else {
                iter.map(|t| t.clone()).collect()
            };
            filtered_tree.push(FilteredTracks { filter: f, tracks })
        }

        if let Ok(mut ft) = self.filtered_tree.write() {
            *ft = filtered_tree;
            self.broadcast(LibEvt::Update);
        };
        bench!("Filters updated in {:?}", now.elapsed())
    }

    /// Get clone of Nth Filter
    pub fn get_filter(&self, pos: usize) -> Option<Filter> {
        self.filtered_tree.read().ok().map(|v| v.get(pos).map(|f| f.filter.clone())).flatten()
    }

    /// Set Nth Filter and rebuild FilteredTracks
    pub fn set_filter(&self, index: usize, filter: Filter) {
        let mut filters = self.get_filters();
        if let Some(fm) = filters.get_mut(index) {
            *fm = filter
        } else {
            filters.push(filter)
        }
        self.set_filters(filters)
    }

    /// Delete Nth filter and rebuild FilteredTracks
    pub fn remove_filter(&self, pos: usize) {
        let mut filters = self.get_filters();
        if !filters.is_empty() {
            filters.remove(pos.min(filters.len().saturating_sub(1)));
        }
        self.set_filters(filters);
    }

    /// Insert filter to position and rebuild FilteredTracks
    pub fn insert_filter(&self, filter: Filter, pos: usize) {
        let mut filters = self.get_filters();
        let len = filters.len();
        filters.insert(pos.min(len), filter);
        self.set_filters(filters);
    }

    /// Get clone of Nth FilteredTracks items
    pub fn get_filter_items(&self, pos: usize) -> Option<Vec<String>> {
        self.filtered_tree
            .read()
            .ok()
            .map(|v| v.get(pos).map(|f| f.filter.items.clone()))
            .flatten()
    }

    /// Set Nth FilteredTracks items and rebuild FilteredTracks
    pub fn set_filter_items(&self, pos: usize, items: Vec<String>) {
        let mut filters = self.get_filters();
        if let Some(f) = filters.get_mut(pos) {
            f.items = items;
            self.set_filters(filters)
        }
    }

    /// Gets Filters paired with FilteredTracks from the *previous filtration*
    /// First tracks will be unfiltered, second will be after Filters[0], etc.
    /// Intended for visual modification of Filters, where you pick from a list
    /// of tracks from the previous filtration to add to the filter
    pub fn get_filter_tree_display(&self) -> (Vec<Filter>, Vec<Vec<Arc<Track>>>) {
        let mut data = vec![self.get_tracks()];
        let mut tags = Vec::new();

        for ft in self.get_filter_tree().into_iter() {
            tags.push(ft.filter);
            data.push(ft.tracks);
        }

        data.pop();

        (tags, data)
    }

    // ## Filters Control ## }}}

    // ## Sorters Control ## {{{

    /// Sort unfiltered tracks based on sorter tagstrings
    fn sort(&self) {
        let now = Instant::now();
        if let Ok(mut tracks) = self.tracks.write() {
            tracks.sort_by(|a, b| {
                let mut result = std::cmp::Ordering::Equal;
                for ts in self.sorters.read().as_deref().unwrap_or(&Vec::new()).iter() {
                    result = result.then(natural_lexical_cmp(&a.tagstring(ts), &b.tagstring(ts)))
                }
                result
            });
            bench!("Sorted {} tracks in {:?}", tracks.len(), now.elapsed());
        }
        self.force_build_filters()
    }

    /// Amount of sorter tagstrings
    pub fn sort_count(&self) -> usize {
        self.sorters.read().map(|v| v.len()).unwrap_or(0)
    }

    /// Get clone of sorter tagstrings
    pub fn get_sorters(&self) -> Vec<String> {
        self.sorters.read().as_deref().cloned().unwrap_or(Vec::new())
    }

    /// Set all sorter tagstrings and re-sort library
    pub fn set_sorters(&self, tagstrings: Vec<String>) {
        if let Ok(mut sorters) = self.sorters.write() {
            *sorters = tagstrings
        }
        self.sort();
    }

    /// Get Nth sorter tagstring
    pub fn get_sorter(&self, index: usize) -> Option<String> {
        self.sorters.read().ok().map(|v| v.get(index).cloned()).flatten()
    }

    /// Set Nth sorter tagstring and re-sort library
    pub fn set_sorter(&self, index: usize, tagstring: String) {
        if let Ok(mut tagstrings) = self.sorters.write() {
            if let Some(ts) = tagstrings.get_mut(index) {
                *ts = tagstring
            } else {
                tagstrings.push(tagstring)
            }
        }
        self.sort();
    }

    /// Remove Nth sorter tagstring and re-sort library
    pub fn remove_sorter(&self, index: usize) {
        let mut sorters = self.get_sorters();
        if !sorters.is_empty() {
            sorters.remove(index.min(sorters.len().saturating_sub(1)));
        }
        self.set_sorters(sorters)
    }

    /// Add sorter tagstring to position and re-sort library
    pub fn insert_sorter(&self, tagstring: String, pos: usize) {
        if let Ok(mut sts) = self.sorters.write() {
            let len = sts.len();
            sts.insert(pos.min(len), tagstring);
        }
        self.sort();
    }

    // ## Sorters Control ## }}}

    // ## Tracklist Control ## {{{

    /// Get cloned references to all tracks
    pub fn get_tracks(&self) -> Vec<Arc<Track>> {
        self.tracks.read().as_deref().cloned().unwrap_or(Vec::new())
    }

    /// Get cloned references to last non-empty FilteredTracks
    pub fn get_queue(&self) -> Vec<Arc<Track>> {
        let Ok(tguard) = self.tracks.read() else {
            return Vec::new();
        };
        let mut ptr: &Vec<Arc<Track>> = &tguard;
        let Ok(fguard) = self.filtered_tree.read() else { return Vec::new() };
        let tree = &fguard;
        for ft in tree.iter().rev() {
            if !ft.tracks.is_empty() {
                ptr = &ft.tracks;
                break;
            }
        }
        ptr.clone()
    }

    /// Fetch all tags from filtered queue. Will map 1:1 with get_queue()
    pub fn get_taglist<T: AsRef<str>>(&self, tagstring: T) -> Vec<String> {
        get_taglist(tagstring, &self.get_queue())
    }

    // ## Tracklist Control ## }}}
}
