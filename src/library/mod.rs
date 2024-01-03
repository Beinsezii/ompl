use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, RwLock,
};
use std::thread;
use std::time::{Duration, Instant};

use lexical_sort::natural_lexical_cmp;
use rand::random;

use bus::{Bus, BusReader};
use std::sync::mpsc::sync_channel;
use std::sync::mpsc::Receiver;

mod player;
mod track;

pub use player::{Backend, Player};
pub use track::{find_tracks, get_taglist, get_taglist_sort, tagstring, Track};

use crate::{l1, l2, log, LOG_LEVEL};

use self::player::PlayerMessage;

// ## FILTER ## {{{

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct Filter {
    pub tag: String,
    pub items: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FilteredTracks {
    pub filter: Filter,
    pub tracks: Vec<Arc<Track>>,
}

// ## FILTER ## }}}

// ## THEME ## {{{

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub fg: Color,
    pub bg: Color,
    pub acc: Color,
}

impl ToString for Theme {
    fn to_string(&self) -> String {
        format!(
            "fg: {}\nbg: {}\nacc: {}",
            self.fg.to_string(),
            self.bg.to_string(),
            self.acc.to_string()
        )
    }
}

// ## THEME ## }}}

// ### FNs ### {{{

fn player_message_server(library: Arc<Library>, next_r: Receiver<PlayerMessage>) {
    l2!("PMS Start");
    let library_weak = Arc::downgrade(&library);
    drop(library);
    loop {
        let msg = next_r.recv();
        if let Some(library) = library_weak.upgrade() {
            match msg {
                Ok(msg) => match msg {
                    PlayerMessage::Request => match library.repeat_get() {
                        None => {
                            if library.get_queue().last() == library.track_get().as_ref()
                                && !library.shuffle_get()
                            {
                                library.bus.lock().unwrap().broadcast(LibEvt::Playback)
                            } else {
                                library.next()
                            }
                        }
                        Some(false) => library.play(),
                        Some(true) => library.next(),
                    },

                    PlayerMessage::Seekable | PlayerMessage::Clock => {
                        library.bus.lock().unwrap().broadcast(LibEvt::Playback)
                    }
                    PlayerMessage::Error(e) => {
                        library.bus.lock().unwrap().broadcast(LibEvt::Error(e))
                    }
                },
                Err(_) => break,
            }
        } else {
            break;
        }
    }
    l2!("PMS End");
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
    bus: Mutex<Bus<LibEvt>>,
    shuffle: AtomicBool,
    repeat: RwLock<Option<bool>>,
    hidden: AtomicBool,
    statusline: RwLock<String>,
    theme: RwLock<Theme>,
}

impl Library {
    // # new # {{{
    pub fn new(backend: Backend) -> Arc<Self> {
        let lib_now = Instant::now();

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
            .spawn(move || player_message_server(result_c, next_r))
            .unwrap();

        l1!(format!("Library built in {:?}", Instant::now() - lib_now));

        result
    }
    // # new # }}}

    pub fn get_receiver(&self) -> BusReader<LibEvt> {
        self.bus.lock().unwrap().add_rx()
    }

    // ## Player forwards ## {{{

    pub fn play(&self) {
        self.player.play();
        self.bus.lock().unwrap().broadcast(LibEvt::Playback);
    }
    pub fn pause(&self) {
        self.player.pause();
        self.bus.lock().unwrap().broadcast(LibEvt::Playback);
    }
    pub fn stop(&self) {
        self.player.stop();
        self.bus.lock().unwrap().broadcast(LibEvt::Playback);
    }
    pub fn play_pause(&self) {
        self.player.toggle();
        self.bus.lock().unwrap().broadcast(LibEvt::Playback);
    }
    pub fn volume_get(&self) -> f32 {
        self.player.volume_get()
    }
    pub fn volume_set(&self, volume: f32) {
        self.player.volume_set(volume);
        self.bus.lock().unwrap().broadcast(LibEvt::Playback);
    }
    pub fn volume_add(&self, amount: f32) {
        self.player.volume_add(amount);
        self.bus.lock().unwrap().broadcast(LibEvt::Playback);
    }

    pub fn track_get(&self) -> Option<Arc<Track>> {
        self.player.track_get()
    }

    pub fn play_track(&self, track: Option<Arc<Track>>) {
        // Check for moved tracks first.
        // Player could handle this but easier if library does
        if let Some(track) = track.as_ref() {
            if !track.path().exists() {
                self.bus.lock().unwrap().broadcast(LibEvt::Error(format!(
                    "Track no longer found at {}\nRemoving from library",
                    track.path().to_str().unwrap()
                )));

                let mut tracks = self.tracks.write().unwrap();
                if let Some(id) = tracks.iter().position(|t| t == track) {
                    tracks.remove(id);
                }
                drop(tracks);
                self.next();
                self.force_build_filters();
            }
        }
        if let Some(track) = self.player.play_track(track) {
            self.history.lock().unwrap().push(track)
        }
        self.bus.lock().unwrap().broadcast(LibEvt::Playback);
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

    pub fn seekable(&self) -> Option<bool> {
        self.player.seekable()
    }

    pub fn times(&self) -> Option<(Duration, Duration)> {
        self.player.times()
    }

    pub fn seek(&self, time: Duration) {
        self.player.seek(time)
    }

    pub fn seek_by(&self, secs: f32) {
        self.player.seek_by(secs)
    }

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
        self.bus.lock().unwrap().broadcast(LibEvt::Playback);
    }

    pub fn shuffle_toggle(&self) {
        self.shuffle_set(!self.shuffle_get())
    }

    /// None - No loop
    /// Some(false) - track loop
    /// Some(true) - full loop
    pub fn repeat_get(&self) -> Option<bool> {
        *self.repeat.read().unwrap()
    }

    /// None - No loop
    /// Some(false) - track loop
    /// Some(true) - full loop
    pub fn repeat_set(&self, repeat: Option<bool>) {
        *self.repeat.write().unwrap() = repeat;
        self.bus.lock().unwrap().broadcast(LibEvt::Playback);
    }

    /// Advances None -> Some(false) -> Some(true)
    pub fn repeat_toggle(&self) {
        let mut guard = self.repeat.write().unwrap();
        *guard = match *guard {
            None => Some(false),
            Some(false) => Some(true),
            Some(true) => None,
        };
        self.bus.lock().unwrap().broadcast(LibEvt::Playback);
    }

    pub fn hidden_get(&self) -> bool {
        self.hidden.load(Ordering::Relaxed)
    }

    pub fn hidden_set(&self, include_hidden: bool) {
        self.hidden.store(include_hidden, Ordering::Relaxed)
    }

    pub fn statusline_get(&self) -> String {
        self.statusline.read().unwrap().to_string()
    }

    pub fn statusline_set<T: ToString>(&self, statusline: T) {
        *self.statusline.write().unwrap() = statusline.to_string();
        self.bus.lock().unwrap().broadcast(LibEvt::Theme);
    }

    /// Parses tagstring from playing track and statusline
    pub fn statusline_get_format(&self) -> String {
        self.track_get()
            .map_or(String::from(""), |t| t.tagstring(self.statusline_get()))
    }

    pub fn theme_get(&self) -> Theme {
        *self.theme.read().unwrap()
    }

    pub fn theme_set(&self, theme: Theme) {
        *self.theme.write().unwrap() = theme;
        self.bus.lock().unwrap().broadcast(LibEvt::Theme);
    }

    // ## Other Settings ## }}}

    // ## Track Controls ## {{{

    pub fn get_random(&self) -> Option<Arc<Track>> {
        l2!("Getting random track...");
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

    pub fn next(&self) {
        if self.shuffle_get() {
            self.play_track(self.get_random())
        } else {
            self.play_track(self.get_sequential(false))
        };
    }

    pub fn previous(&self) {
        if self.shuffle_get() {
            let track = self.history.lock().unwrap().pop();
            self.play_track(track);
            // remove twice cause it gets re-added.
            self.history.lock().unwrap().pop();
        } else {
            self.play_track(self.get_sequential(true))
        }
    }

    // ## Track Controls ## }}}

    // ## Library Paths Control ## {{{

    pub fn types(&self) -> Vec<String> {
        self.player.types()
    }

    pub fn append_library<T: AsRef<Path>>(&self, path: T) {
        let mut new_tracks: Vec<Track> = find_tracks(path, &self.player.types(), self.hidden_get());

        thread::scope(|scope| {
            // 50 is a completely arbitrary value that seems to perform well enough
            // Basically tradeoff between thread spawn overhead and IO calls.
            // I dont want an entire async runtime for loading metadata so here it is
            for chunk in new_tracks.chunks_mut(50) {
                scope.spawn(|| chunk.iter_mut().for_each(|track| track.load_meta()));
            }
        });

        let mut tracks = self.tracks.write().unwrap();
        new_tracks.into_iter().map(|t| Arc::new(t)).for_each(|t| {
            if !tracks.contains(&t) {
                tracks.push(t)
            }
        });
        drop(tracks);

        self.sort();

        if self.player.track_get().is_none() {
            self.player.track_set(if self.shuffle_get() {
                self.get_random()
            } else {
                self.get_sequential(false)
            });
        }
    }

    pub fn purge(&self) {
        *self.tracks.write().unwrap() = Vec::new();
        self.force_build_filters();
        self.bus.lock().unwrap().broadcast(LibEvt::Update);
    }

    // ## Library Paths Control ## }}}

    // ## Filters Control ## {{{

    /// rebuilds whole filter tree without caching
    fn force_build_filters(&self) {
        let filters = self.get_filters();
        *self.filtered_tree.write().unwrap() = vec![];
        self.set_filters(filters);
    }

    pub fn filter_count(&self) -> usize {
        self.filtered_tree.read().unwrap().len()
    }

    pub fn get_filter_tree(&self) -> Vec<FilteredTracks> {
        self.filtered_tree.read().unwrap().clone()
    }

    pub fn get_filters(&self) -> Vec<Filter> {
        self.filtered_tree
            .read()
            .unwrap()
            .iter()
            .map(|ft| ft.filter.clone())
            .collect::<Vec<Filter>>()
    }

    pub fn set_filters(&self, filters: Vec<Filter>) {
        l2!("Updating filters...");
        let now = Instant::now();
        let mut cache = true;
        let mut filtered_tree = Vec::<FilteredTracks>::new();

        for (i, f) in filters.into_iter().enumerate() {
            // if filters continuously match existing, don't rebuild.
            if let Some(ft) = self.filtered_tree.read().unwrap().get(i) {
                if ft.filter == f && cache {
                    filtered_tree.push(ft.clone());
                    continue;
                } else {
                    cache = false
                }
            }

            let itracks = self.tracks.read().unwrap();
            let iter = if i == 0 {
                itracks.iter()
            } else {
                filtered_tree[i - 1].tracks.iter()
            };

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

        *self.filtered_tree.write().unwrap() = filtered_tree;
        self.bus.lock().unwrap().broadcast(LibEvt::Update);
        l1!(format!("Filters updated in {:?}", Instant::now() - now));
    }

    pub fn get_filter(&self, pos: usize) -> Option<Filter> {
        self.filtered_tree
            .read()
            .unwrap()
            .get(pos)
            .map(|f| f.filter.clone())
    }

    pub fn set_filter(&self, index: usize, filter: Filter) {
        let mut filters = self.get_filters();
        if let Some(fm) = filters.get_mut(index) {
            *fm = filter
        } else {
            filters.push(filter)
        }
        self.set_filters(filters)
    }

    pub fn remove_filter(&self, pos: usize) {
        let mut filters = self.get_filters();
        if pos < filters.len() {
            filters.remove(pos);
        }
        self.set_filters(filters);
    }

    pub fn insert_filter(&self, filter: Filter, pos: usize) {
        let mut filters = self.get_filters();
        let len = filters.len();
        filters.insert(pos.min(len), filter);
        self.set_filters(filters);
    }

    pub fn get_filter_items(&self, pos: usize) -> Option<Vec<String>> {
        self.filtered_tree
            .read()
            .unwrap()
            .get(pos)
            .map(|f| f.filter.items.clone())
    }

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

    fn sort(&self) {
        self.tracks.write().unwrap().sort_by(|a, b| {
            let mut result = std::cmp::Ordering::Equal;
            for ts in self.sorters.read().unwrap().iter() {
                result = result.then(natural_lexical_cmp(&a.tagstring(ts), &b.tagstring(ts)))
            }
            result
        });
        self.force_build_filters()
    }

    pub fn sort_count(&self) -> usize {
        self.sorters.read().unwrap().len()
    }

    pub fn get_sorters(&self) -> Vec<String> {
        self.sorters.read().unwrap().clone()
    }

    pub fn set_sorters(&self, tagstrings: Vec<String>) {
        *self.sorters.write().unwrap() = tagstrings;
        self.sort();
    }

    pub fn get_sorter(&self, index: usize) -> Option<String> {
        self.sorters.read().unwrap().get(index).cloned()
    }

    pub fn set_sorter(&self, index: usize, tagstring: String) {
        let mut tagstrings = self.sorters.write().unwrap();
        if let Some(ts) = tagstrings.get_mut(index) {
            *ts = tagstring
        } else {
            tagstrings.push(tagstring)
        }
        drop(tagstrings);
        self.sort();
    }

    pub fn remove_sorter(&self, index: usize) {
        let mut sorters = self.sorters.write().unwrap();
        if index < sorters.len() {
            sorters.remove(index);
        }
        drop(sorters);
        self.sort()
    }

    pub fn insert_sorter(&self, tagstring: String, pos: usize) {
        {
            let mut sts = self.sorters.write().unwrap();
            let len = sts.len();
            sts.insert(pos.min(len), tagstring);
        }
        self.sort();
    }

    // ## Sorters Control ## }}}

    // ## Tracklist Control ## {{{

    pub fn get_tracks(&self) -> Vec<Arc<Track>> {
        self.tracks.read().unwrap().clone()
    }

    /// Will get from last non-empty FilteredTracks
    pub fn get_queue(&self) -> Vec<Arc<Track>> {
        let mut ptr: &Vec<Arc<Track>> = &self.tracks.read().unwrap();
        let tree = self.filtered_tree.read().unwrap();
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

    // /// Sorts *and* dedupes. Will NOT map 1:1 with get_queue_sorter() if there are multiple tracks
    // /// with the same tag value.
    // pub fn get_taglist_sort<T: AsRef<str>>(&self, tagstring: T) -> Vec<String> {
    //     get_taglist_sort(tagstring, &self.get_queue())
    // }

    // ## Tracklist Control ## }}}
}
