use std::path::Path;
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, RwLock};
use std::thread;

use rand::random;
use rayon::prelude::*;
use walkdir::WalkDir;

mod player;
mod track;

pub use player::Player;
pub use track::Track;

pub static POLL_MS: u64 = 5;

#[derive(Clone, Debug)]
pub struct Status {
    playing: bool,
    // playtime. not implemented in player yet.
    // time: f32,
    track: Option<Track>,
    volume: f32,
}

pub type StatusSync = Arc<RwLock<Status>>;

impl Status {
    pub fn new(playing: bool, track: Option<Track>, volume: f32) -> Self {
        Self {
            playing,
            track,
            volume,
        }
    }
    pub fn new_sync(playing: bool, track: Option<Track>, volume: f32) -> StatusSync {
        Arc::new(RwLock::new(Self::new(playing, track, volume)))
    }
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(
            f,
            "{} {:?} {:.3}",
            match self.playing {
                true => "playing",
                false => "stopped",
            },
            self.track,
            self.volume
        )
    }
}

fn track_nexter(library: Arc<Library>, next_r: Receiver<()>) {
    loop {
        match next_r.try_recv() {
            Ok(_) => library.next(),
            Err(_) => (),
        }
        // prevent deadlock. Library will never drop cause thread holds it in scope, which prevents
        // player from dropping, too.
        // Simply check if there's only one ref.
        match Arc::strong_count(&library) {
            0 | 1 => break,
            _ => (),
        }
        thread::sleep(std::time::Duration::from_millis(POLL_MS))
    }
}

pub fn get_tracks<T: AsRef<Path>>(path: T) -> Vec<Track> {
    WalkDir::new(path)
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
                    for t in player::TYPES.into_iter() {
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
        .collect()
}

pub struct Library {
    pub tracks: Vec<Track>,
    player: Player,
    status: StatusSync,
}

impl Library {
    pub fn new<T: AsRef<Path>>(path: T) -> Arc<Self> {
        let mut tracks = get_tracks(path);

        // rayon cuts this down by about 3x on my 4-core machine.
        // *should* be good enough for most cases. Assuming you have a recent computer, it'd take
        // no more than a couple secs for a 10,000 track library. Could probably be optimized
        // further using a unique solution a la my gimp plugin PixelBuster v2.
        // Also, WalkDir hits pretty hard. Accounts for 1/3 of runtime after rayon.
        tracks.par_iter_mut().for_each(|track| track.load_meta());

        let status = Status::new_sync(false, tracks.get(0).cloned(), 0.5f32.powi(3));

        let (next_s, next_r) = mpsc::channel();
        let result = Arc::new(Self {
            player: Player::new(status.clone(), Some(next_s)),
            status,
            tracks,
        });

        let result_c = result.clone();

        thread::spawn(move || track_nexter(result_c, next_r));

        result
    }

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
        // matching on the read will hold the lock that pause/play need
        let playing = self.status.read().unwrap().playing;
        match playing {
            true => self.pause(),
            false => self.play(),
        }
    }
    pub fn next(&self) {
        self.stop();
        self.status.write().unwrap().track = self.get_random().cloned();
        self.play();
    }

    pub fn volume_get(&self) -> f32 {
        self.status.read().unwrap().volume.cbrt()
    }
    pub fn volume_set(&self, amount: f32) {
        self.status.write().unwrap().volume = 0.0f32.max(1.0f32.min(amount.powi(3)));
    }
    pub fn volume_add(&self, amount: f32) {
        self.volume_set(self.volume_get() + amount);
    }
    pub fn volume_sub(&self, amount: f32) {
        self.volume_set(self.volume_get() - amount);
    }

    pub fn get_random<'a>(&'a self) -> Option<&'a Track> {
        match self.tracks.len() {
            0 => None,
            1 => Some(&self.tracks[0]),
            _ => {
                loop {
                    let track = Some(&self.tracks[random::<usize>() % self.tracks.len()]);
                    if track != self.status.read().unwrap().track.as_ref() { break track }
                }
            }
        }
    }

    pub fn get_status(&self) -> StatusSync {
        self.status.clone()
    }
}
