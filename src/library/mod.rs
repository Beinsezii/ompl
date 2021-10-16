use std::path::Path;
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::thread;

use rand::random;
use rayon::prelude::*;
use walkdir::WalkDir;

mod player;
mod track;

pub use player::Player;
pub use track::Track;

pub static POLL_MS: u64 = 5;

fn track_nexter(library: Arc<Mutex<Library>>, next_r: Receiver<()>) {
    loop {
        match next_r.try_recv() {
            Ok(_) => library.lock().unwrap().next(),
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
}

impl Library {
    pub fn new<T: AsRef<Path>>(path: T) -> Arc<Mutex<Self>> {
        let mut tracks = get_tracks(path);

        // rayon cuts this down by about 3x on my 4-core machine.
        // *should* be good enough for most cases. Assuming you have a recent computer, it'd take
        // no more than a couple secs for a 10,000 track library. Could probably be optimized
        // further using a unique solution a la my gimp plugin PixelBuster v2.
        // Also, WalkDir hits pretty hard. Accounts for 1/3 of runtime after rayon.
        tracks.par_iter_mut().for_each(|track| track.load_meta());

        let (next_s, next_r) = mpsc::channel();
        let result = Arc::new(Mutex::new(Self {
            player: Player::new(tracks.get(0).cloned(), Some(next_s)),
            tracks,
        }));

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
        self.player.play_pause();
    }
    pub fn next(&mut self) {
        self.player.next(self.get_random().cloned())
    }

    pub fn get_random<'a>(&'a self) -> Option<&'a Track> {
        self.tracks.get(random::<usize>() % self.tracks.len())
    }
}
