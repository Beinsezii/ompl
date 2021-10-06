mod player;
mod track;
use rand::random;
use std::path::Path;
use walkdir::WalkDir;

use std::sync::{Arc, Mutex};
use std::sync::mpsc::{SyncSender, Sender, Receiver};

pub use player::Player;
pub use track::Track;

fn track_nexter(library: Arc<Mutex<Library>>, next_r: Receiver<()>) {
    loop {
        match next_r.recv() {
            Ok(_) => library.lock().unwrap().next(),
            Err(_) => break,
        }
    }
}

pub struct Library {
    pub songs: Vec<Track>,
    player: Player,
}

impl Library {
    pub fn new<T: AsRef<Path>>(path: T) -> Arc<Mutex<Self>> {
        let songs: Vec<Track> = WalkDir::new(path)
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
                    .map(|s| s.ends_with(".mp3") || s.ends_with(".flac"))
                    .unwrap_or(false)
            })
            .map(|e| Track::new(e.path()))
            .collect();

        let (next_s, next_r) = std::sync::mpsc::channel();
        let result = Arc::new(Mutex::new(Self {
            player: Player::new(songs.get(0).cloned(), next_s),
            songs,
        }));

        let result_c = result.clone();

        std::thread::spawn(move || track_nexter(result_c, next_r));

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
        self.songs.get(random::<usize>() % self.songs.len())
    }
}
