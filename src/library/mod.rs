mod player;
mod track;
use rand::random;
use std::cell::RefCell;
use std::path::Path;
use walkdir::WalkDir;

pub use player::Player;
pub use track::Track;

pub struct Library {
    pub songs: Vec<Track>,
    player: RefCell<Player>,
}

impl Library {
    pub fn new<T: AsRef<Path>>(path: T) -> Self {
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

        Self {
            player: RefCell::new(Player::new(songs.get(0).cloned())),
            songs,
        }
    }

    pub fn play(&self) {
        self.player.borrow_mut().play()
    }
    pub fn pause(&self) {
        self.player.borrow_mut().pause()
    }
    pub fn stop(&self) {
        self.player.borrow_mut().stop()
    }
    pub fn next(&self) {
        self.player.borrow_mut().next(self.get_random().cloned())
    }

    pub fn get_random<'a>(&'a self) -> Option<&'a Track> {
        self.songs.get(random::<usize>() % self.songs.len())
    }
}
