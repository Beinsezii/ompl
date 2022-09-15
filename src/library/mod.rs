use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, RwLock,
};
use std::thread;
use std::time::Instant;

use rand::random;
use rayon::prelude::*;

use bus::{Bus, BusReader};
use crossbeam::channel;
use crossbeam::channel::Receiver;

mod player;
mod track;

pub use player::{Player, TYPES};
pub use track::{get_taglist, get_taglist_sort, get_tracks, sort_by_tag, tagstring, Track};

use crate::{l1, l2, log, LOG_LEVEL};

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

// ### FNs ### {{{

fn track_nexter(library: Arc<Library>, next_r: Receiver<()>) {
    l2!("Track Nexter start");
    let library_weak = Arc::downgrade(&library);
    drop(library);
    loop {
        match next_r.recv() {
            Ok(_) => {
                if let Some(l) = library_weak.upgrade() {
                    l.next()
                }
            }
            Err(_) => break,
        }
    }
    l2!("Track Nexter end");
}

// ### FNs ### }}}

#[derive(Debug, Clone)]
pub enum LibEvt {
    Play,
    Pause,
    Stop,
    Shuffle,
    Volume,
    Update,
    Error(String),
}

pub struct Library {
    tracks: RwLock<Vec<Arc<Track>>>,
    history: Mutex<Vec<Arc<Track>>>,
    player: Player,
    filtered_tree: RwLock<Vec<FilteredTracks>>,
    sort_tagstrings: RwLock<Vec<String>>,
    bus: Mutex<Bus<LibEvt>>,
    shuffle: AtomicBool,
    hidden: AtomicBool,
}

impl Library {
    // # new # {{{
    pub fn new() -> Arc<Self> {
        let lib_now = Instant::now();

        let bus = Mutex::new(Bus::<LibEvt>::new(99));

        let (next_s, next_r) = channel::bounded(1);
        let result = Arc::new(Self {
            player: Player::new(None, Some(next_s)),
            tracks: RwLock::new(Vec::new()),
            history: Mutex::new(Vec::new()),
            filtered_tree: RwLock::new(Vec::new()),
            sort_tagstrings: RwLock::new(Vec::new()),
            bus,
            shuffle: AtomicBool::new(true),
            hidden: AtomicBool::new(false),
        });

        result.volume_set(0.5);

        let result_c = result.clone();

        thread::spawn(move || track_nexter(result_c, next_r));

        l1!(format!("Library built in {:?}", Instant::now() - lib_now));

        result
    }
    // # new # }}}

    // ## CONTROLS ## {{{
    pub fn play(&self) {
        self.player.play();
        self.bus.lock().unwrap().broadcast(LibEvt::Play);
    }
    pub fn pause(&self) {
        self.player.pause();
        self.bus.lock().unwrap().broadcast(LibEvt::Pause);
    }
    pub fn stop(&self) {
        self.player.hard_stop();
        self.bus.lock().unwrap().broadcast(LibEvt::Stop);
    }
    pub fn play_pause(&self) {
        match self.player.playing() {
            true => self.pause(),
            false => self.play(),
        }
    }
    pub fn next(&self) {
        if self.shuffle_get() {
            self.play_track(self.get_random())
        } else {
            self.play_track(self.get_sequential(false))
        };
    }
    pub fn previous(&self) {
        self.player.stop();
        if self.shuffle_get() {
            let track = self.history.lock().unwrap().pop();
            self.play_track(track);
            // remove twice cause it gets re-added.
            self.history.lock().unwrap().pop();
        } else {
            self.play_track(self.get_sequential(true))
        }
        self.play();
    }

    pub fn volume_get(&self) -> f32 {
        self.player.volume_get()
    }
    pub fn volume_set(&self, volume: f32) {
        self.player.volume_set(volume);
        self.bus.lock().unwrap().broadcast(LibEvt::Volume);
    }
    pub fn volume_add(&self, amount: f32) {
        self.volume_set(self.volume_get() + amount);
    }
    pub fn volume_sub(&self, amount: f32) {
        self.volume_set(self.volume_get() - amount);
    }

    pub fn shuffle_get(&self) -> bool {
        self.shuffle.load(Ordering::Relaxed)
    }

    pub fn shuffle_set(&self, shuffle: bool) {
        self.shuffle.store(shuffle, Ordering::Relaxed);
        self.bus.lock().unwrap().broadcast(LibEvt::Shuffle);
    }

    pub fn shuffle_toggle(&self) {
        self.shuffle_set(!self.shuffle_get())
    }

    pub fn hidden_get(&self) -> bool {
        self.hidden.load(Ordering::Relaxed)
    }

    pub fn hidden_set(&self, include_hidden: bool) {
        self.hidden.store(include_hidden, Ordering::Relaxed)
    }

    pub fn play_track(&self, track: Option<Arc<Track>>) {
        if let Some(prev_track) = self.player.track_get() {
            self.history.lock().unwrap().push(prev_track)
        }
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
                return;
            }
        }

        self.player.stop();
        self.player.track_set(track);
        self.play();
    }

    pub fn track_get(&self) -> Option<Arc<Track>> {
        self.player.track_get()
    }

    // ## CONTROLS ## }}}

    // # set_filters # {{{
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

    pub fn set_filter(&self, index: usize, filter: Filter) {
        let mut filters = self.get_filters();
        if let Some(fm) = filters.get_mut(index) {
            *fm = filter
        } else {
            filters.push(filter)
        }
        self.set_filters(filters)
    }

    fn force_build_filters(&self) {
        let filters = self.get_filters();
        *self.filtered_tree.write().unwrap() = vec![];
        self.set_filters(filters);
    }
    // # set_filters # }}}

    // ## GET/SET ## {{{

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

    pub fn append_library<T: AsRef<Path>>(&self, path: T) {
        let mut new_tracks: Vec<Track> = get_tracks(path, self.hidden_get());

        new_tracks
            .par_iter_mut()
            .for_each(|track| track.load_meta());

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

    pub fn filter_count(&self) -> usize {
        self.filtered_tree.read().unwrap().len()
    }

    pub fn get_filter_tree(&self) -> Vec<FilteredTracks> {
        self.filtered_tree.read().unwrap().clone()
    }

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

    pub fn get_filtered_tracks(&self, pos: usize) -> Option<FilteredTracks> {
        self.filtered_tree.read().unwrap().get(pos).cloned()
    }

    pub fn get_filter(&self, pos: usize) -> Option<Filter> {
        self.filtered_tree
            .read()
            .unwrap()
            .get(pos)
            .map(|f| f.filter.clone())
    }

    pub fn get_filters(&self) -> Vec<Filter> {
        self.filtered_tree
            .read()
            .unwrap()
            .iter()
            .map(|ft| ft.filter.clone())
            .collect::<Vec<Filter>>()
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

    fn sort(&self) {
        self.tracks.write().unwrap().sort_by(|a, b| {
            let mut result = std::cmp::Ordering::Equal;
            for ts in self.sort_tagstrings.read().unwrap().iter() {
                result = result.then(a.tagstring(ts).cmp(&b.tagstring(ts)))
            }
            result
        });
        self.force_build_filters()
    }

    pub fn set_sort_tagstrings(&self, tagstrings: Vec<String>) {
        *self.sort_tagstrings.write().unwrap() = tagstrings;
        self.sort();
    }

    pub fn set_sort(&self, index: usize, tagstring: String) {
        let mut tagstrings = self.sort_tagstrings.write().unwrap();
        if let Some(ts) = tagstrings.get_mut(index) {
            *ts = tagstring
        } else {
            tagstrings.push(tagstring)
        }
        drop(tagstrings);
        self.sort();
    }

    pub fn get_sort_tagstrings(&self) -> Vec<String> {
        self.sort_tagstrings.read().unwrap().clone()
    }

    pub fn get_sort(&self, index: usize) -> Option<String> {
        self.sort_tagstrings.read().unwrap().get(index).cloned()
    }

    pub fn insert_sort_tagstring(&self, tagstring: String, pos: usize) {
        {
            let mut sts = self.sort_tagstrings.write().unwrap();
            let len = sts.len();
            sts.insert(pos.min(len), tagstring);
        }
        self.sort();
    }

    pub fn remove_sort_tagstring(&self, pos: usize) {
        {
            let mut sts = self.sort_tagstrings.write().unwrap();
            if pos < sts.len() {
                sts.remove(pos);
            }
        }
        self.sort();
    }

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
    pub fn get_taglist<T: Into<String>>(&self, tagstring: T) -> Vec<String> {
        get_taglist(tagstring, &self.get_queue())
    }

    /// Sorts *and* dedupes. Will NOT map 1:1 with get_queue_sort() if there are multiple tracks
    /// with the same tag value.
    pub fn get_taglist_sort<T: Into<String>>(&self, tagstring: T) -> Vec<String> {
        get_taglist_sort(tagstring, &self.get_queue())
    }

    pub fn get_receiver(&self) -> BusReader<LibEvt> {
        self.bus.lock().unwrap().add_rx()
    }

    // ## GET/SET ## }}}

    // ## Status ## {{{

    pub fn playing(&self) -> bool {
        self.player.playing()
    }
    pub fn paused(&self) -> bool {
        self.player.paused()
    }
    pub fn stopped(&self) -> bool {
        self.player.stopped()
    }

    // ## Status ## }}}
}
