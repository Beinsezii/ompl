use std::path::Path;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Instant;
use std::cmp::Ordering;

use rand::random;
use rayon::prelude::*;
use walkdir::WalkDir;

use crossbeam::channel;
use crossbeam::channel::Receiver;

mod player;
mod track;

pub use player::{Player, TYPES};
pub use track::Track;

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

pub fn get_tracks<T: AsRef<Path>>(path: T) -> Vec<Track> {
    l2!("Finding tracks...");
    let now = Instant::now();

    let tracks: Vec<Track> = WalkDir::new(path)
        .max_depth(10)
        .into_iter()
        .filter_entry(|e| {
            e.file_name()
                .to_str()
                .map(|s| !s.starts_with("."))
                .unwrap_or(false)
        })
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_str()
                .map(|s| {
                    let mut res = false;
                    for t in TYPES.into_iter() {
                        if s.ends_with(t) {
                            res = true;
                            break;
                        }
                    }
                    res
                })
                .unwrap_or(false)
        })
        .map(|e| Track::new(e.path()))
        .collect();

    l1!(format!(
        "Found {} tracks in {:?}",
        tracks.len(),
        Instant::now() - now
    ));
    tracks
}

pub fn get_all_tag(tag: &str, tracks: &Vec<Arc<Track>>) -> Vec<String> {
    tracks
        .iter()
        .filter_map(|t| t.tags().get(tag).cloned())
        .collect::<Vec<String>>()
}

pub fn get_all_tag_sort(tag: &str, tracks: &Vec<Arc<Track>>) -> Vec<String> {
    let mut result = get_all_tag(tag, tracks);
    result.sort();
    result.dedup();
    result
}

pub fn sort_by_tag(tag: &str, tracks: &mut Vec<Arc<Track>>) {
    tracks.sort_by(|a, b| {
        let a = a.tags().get(tag);
        let b = b.tags().get(tag);
        if a.is_none() && b.is_none() {
            Ordering::Equal
        } else {
            match a {
                Some(a) => match b {
                    Some(b) => a.cmp(b),
                    None => Ordering::Greater,
                },
                None => Ordering::Less,
            }
        }
    } )
}

// ### FNs ### }}}

pub struct Library {
    pub tracks: Vec<Arc<Track>>,
    player: Player,
    filtered_tree: RwLock<Vec<FilteredTracks>>,
}

impl Library {
    // # new # {{{
    pub fn new<T: AsRef<Path>>(path: T, initial_filters: Option<Vec<Filter>>) -> Arc<Self> {
        l2!("Constructing library...");
        let lib_now = Instant::now();
        let mut tracks: Vec<Track> = get_tracks(path);

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
            filtered_tree: RwLock::new(Vec::new()),
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
        self.player.play()
    }
    pub fn pause(&self) {
        self.player.pause()
    }
    pub fn stop(&self) {
        self.player.stop()
    }
    pub fn play_pause(&self) {
        match self.player.active() {
            true => self.pause(),
            false => self.play(),
        }
    }
    pub fn next(&self) {
        self.play_track(self.get_random());
    }

    pub fn volume_get(&self) -> f32 {
        self.player.volume_get()
    }
    pub fn volume_set(&self, volume: f32) {
        self.player.volume_set(volume);
    }
    pub fn volume_add(&self, amount: f32) {
        self.volume_set(self.volume_get() + amount);
    }
    pub fn volume_sub(&self, amount: f32) {
        self.volume_set(self.volume_get() - amount);
    }

    pub fn play_track(&self, track: Option<Arc<Track>>) {
        self.stop();
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
                    let tags = t.tags();
                    if let Some(val) = tags.get(&f.tag.to_ascii_lowercase()) {
                        if f.items.contains(val) {
                            tracks_f.push(t.clone())
                        }
                    }
                }
                tracks_f
            } else {
                iter.map(|t| t.clone()).collect()
            };
            filtered_tree.push(FilteredTracks { filter: f, tracks })
        }

        *self.filtered_tree.write().unwrap() = filtered_tree;
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
        let mut tracks = self.tracks.clone();

        for ft in self.filtered_tree.read().unwrap().iter().rev() {
            if !ft.tracks.is_empty() {
                tracks = ft.tracks.clone();
                break;
            }
        }
        tracks
    }

    // ## GET/SET ## }}}
}
