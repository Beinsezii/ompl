use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};
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

#[derive(Clone, Debug, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy)]
pub enum LibEvt {
    Play,
    Pause,
    Stop,
    Volume,
    Filter,
}

pub struct Library {
    pub tracks: Vec<Arc<Track>>,
    history: Mutex<Vec<Arc<Track>>>,
    player: Player,
    filtered_tree: RwLock<Vec<FilteredTracks>>,
    bus: Mutex<Bus<LibEvt>>,
}

impl Library {
    // # new # {{{
    pub fn new<T: AsRef<Path>>(path: T, initial_filters: Option<Vec<Filter>>) -> Arc<Self> {
        l2!("Constructing library...");
        let lib_now = Instant::now();
        let mut tracks: Vec<Track> = get_tracks(path);

        let bus = Mutex::new(Bus::<LibEvt>::new(99));

        l2!("Fetching metadata...");
        let met_now = Instant::now();
        // rayon cuts this down by about 3x on my 4-core machine.
        // *should* be good enough for most cases. Assuming you have a recent computer, it'd take
        // no more than a couple secs for a 10,000 track library. Could probably be optimized
        // further using a unique solution a la my gimp plugin PixelBuster v2.
        // Also, WalkDir hits pretty hard. Accounts for 1/3 of runtime after rayon.
        tracks.par_iter_mut().for_each(|track| track.load_meta());
        l1!(format!("Metadata loaded in {:?}", Instant::now() - met_now));

        let tracks: Vec<Arc<Track>> = tracks.into_iter().map(|t| Arc::new(t)).collect();

        let (next_s, next_r) = channel::bounded(1);
        let result = Arc::new(Self {
            player: Player::new(None, Some(next_s)),
            tracks,
            history: Mutex::new(Vec::new()),
            filtered_tree: RwLock::new(Vec::new()),
            bus,
        });

        if let Some(f) = initial_filters {
            result.set_filters(f)
        }
        result.player.track_set(result.get_random());
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
        self.play_track(self.get_random());
    }
    pub fn previous(&self) {
        self.player.stop();
        self.player.track_set(self.history.lock().unwrap().pop());
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

    pub fn play_track(&self, track: Option<Arc<Track>>) {
        if let Some(track) = self.player.track_get() {
            self.history.lock().unwrap().push(track)
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

            let iter = if i == 0 {
                self.tracks.iter()
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
        self.bus.lock().unwrap().broadcast(LibEvt::Filter);
        l1!(format!("Filters updated in {:?}", Instant::now() - now));
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

    pub fn get_filter_tree(&self) -> Vec<FilteredTracks> {
        self.filtered_tree.read().unwrap().clone()
    }

    pub fn get_tracks(&self) -> Vec<Arc<Track>> {
        self.tracks.clone()
    }

    pub fn get_queue(&self) -> Vec<Arc<Track>> {
        let mut ptr: &Vec<Arc<Track>> = &self.tracks;
        let tree = self.filtered_tree.read().unwrap();
        for ft in tree.iter().rev() {
            if !ft.tracks.is_empty() {
                ptr = &ft.tracks;
                break;
            }
        }
        ptr.clone()
    }

    pub fn get_queue_sort<T: AsRef<str>>(&self, tagstring: T) -> Vec<Arc<Track>> {
        let mut result = self.get_queue();
        sort_by_tag(tagstring, &mut result);
        result
    }

    /// Fetch all tags from filtered queue. Will map 1:1 with get_queue()
    pub fn get_taglist<T: Into<String>>(&self, tagstring: T) -> Vec<String> {
        get_taglist(tagstring, &self.get_queue())
    }

    /// Sorts *and* dedupes. Will NOT map 1:1 with get_queue_sort() if there are multiple tracks
    /// with the same tag value.
    pub fn get_taglist_sort<T: Into<String>>(&self, tagstring: T) -> Vec<String> {
        get_taglist(tagstring, &self.get_queue())
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
