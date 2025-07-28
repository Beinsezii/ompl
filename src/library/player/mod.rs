#![warn(missing_docs)]

// I figured out this cool name for symphonia-cpal 'sympal' and was like "wow thats way cooler than
// something like 'backend-symphonia.rs' so I just went with it only to later realize that if I
// have both a local module and a dependancy called 'rodio' its confusing as fuck so naturally I
// have to rename my rodio backend module with 'b' for backend + rodio which gives me 'brodio' and
// now I'm mildly disgusted with myself, bro.
#[cfg(feature = "backend-rodio")]
mod brodio;

#[cfg(feature = "backend-sympal")]
mod sympal;

use std::sync::Arc;
use std::sync::mpsc::SyncSender;
use std::time::Duration;

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::library::Track;

#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize, ValueEnum)]
pub enum Backend {
    /// Pick from enabled backends, prioritizing Rodio on Windows, otherwise Sympal
    Default,
    /// Advanced backend with extra features
    #[cfg(feature = "backend-sympal")]
    Sympal,
    /// Safe backend with maximum compatibility and lower memory usage
    #[cfg(feature = "backend-rodio")]
    Rodio,
}

/// Panics if no backends are enabled at compile time.
pub fn backend(backend: Backend, buffer: Option<u32>, signal: SyncSender<PlayerMessage>) -> Box<dyn Player> {
    // {{{
    #[allow(unreachable_code)]
    match backend {
        Backend::Default => {
            #[cfg(feature = "backend-sympal")]
            return Box::new(sympal::Backend::new(buffer, signal));
            #[cfg(feature = "backend-rodio")]
            return Box::new(brodio::Backend::new(buffer, signal));
            panic!("No backends enabled during compile!")
        }
        #[cfg(feature = "backend-sympal")]
        Backend::Sympal => Box::new(sympal::Backend::new(buffer, signal)),
        #[cfg(feature = "backend-rodio")]
        Backend::Rodio => Box::new(brodio::Backend::new(buffer, signal)),
    }
    // }}}
}

/// Messages from Player -> Library
/// Library should forward to other modules if necessary
pub enum PlayerMessage {
    /// Request a new track
    Request,
    /// seekable() will now return true
    Seekable,
    /// ONE SECOND HAS PASSED
    Clock,
    /// Other non-fatal error
    Error(String),
}

pub trait Player: Send + Sync {
    // ### REQUIRED FNS {{{

    /// Constructs new player with optional sender that fires when
    /// the current playing track ends or playback is otherwise interrupted.
    fn new(buffer: Option<u32>, sig: SyncSender<PlayerMessage>) -> Self
    where
        Self: Sized;

    // Really this could be something like a const but you cant have that in a trait ig
    //
    /// Returns filetypes playable by Player
    /// ex: `["mp3", "ogg"]`
    fn types(&self) -> Vec<String>;
    /// Whether the file can seek playback.
    /// Expect to change @ runtime.
    /// None means player as a whole cannot seek.
    fn seekable(&self) -> Option<bool>;

    /// Returns current and total time
    fn times(&self) -> Option<(Duration, Duration)>;

    /// Seek to specified time
    fn seek(&self, time: Duration);

    /// Sample whole buffer to generate a waveform
    fn waveform(&self, count: usize) -> Option<Vec<f32>>;

    /// Set player volume. Multiplier, 1.0 == unchanged
    fn volume_set(&self, volume: f32);
    /// Get player volume. Multiplier, 1.0 == unchanged
    fn volume_get(&self) -> f32;

    /// Set player track. Should stop playback.
    /// Returns previously set track.
    fn track_set(&self, track: Option<Arc<Track>>) -> Option<Arc<Track>>;
    /// Get currently set track.
    fn track_get(&self) -> Option<Arc<Track>>;

    /// Play currently set track.
    fn play(&self);
    /// Stop playback. Should drop audio device.
    fn stop(&self);
    /// Pause playback. Should drop audio device.
    fn pause(&self);

    /// Whether player is playing.
    fn playing(&self) -> bool;
    /// Whether player is paused.
    fn paused(&self) -> bool;

    // ### REQUIRED FNS }}}

    // ### PROVIDED FNS ### {{{

    fn volume_add(&self, amount: f32) {
        let current = self.volume_get();
        self.volume_set(current + amount)
    }

    /// Set new track && play immediately.
    /// Returns old track.
    fn play_track(&self, track: Option<Arc<Track>>) -> Option<Arc<Track>> {
        let track = self.track_set(track);
        self.play();
        track
    }

    /// Toggle between play/pause.
    fn toggle(&self) {
        if self.playing() { self.pause() } else { self.play() }
    }

    /// Whether player is completeley stopped.
    fn stopped(&self) -> bool {
        (!self.playing()) && (!self.paused())
    }

    /// Advance seek by seconds, positive or negative
    fn seek_by(&self, secs: f32) {
        if let Some(true) = self.seekable() {
            if let Some((current, total)) = self.times() {
                self.seek(Duration::from_secs_f32((current.as_secs_f32() + secs).max(0.0)).min(total))
            }
        }
    }

    // ### PROVIDED FNS ### }}}
}
