use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use rodio::{OutputStream, OutputStreamHandle, Sink};

use crossbeam::channel;
use crossbeam::channel::{Receiver, Sender};

use super::track::Track;

use crate::{l1, l2, log, LOG_LEVEL};

pub const TYPES: &[&'static str] = &[".mp3", ".flac", ".ogg", ".wav"];

// ### BG TASKS ### {{{

fn track_ender(sink: Arc<RwLock<Option<Sink>>>, signal_next: Sender<()>) {
    l2!("Track Ender start");
    loop {
        if let Some(sink) = &*sink.read().unwrap() {
            if sink.empty() {
                if let Err(_) = signal_next.send(()) {
                    break;
                } else {
                    l2!("Next track!");
                }
            }
        }

        thread::sleep(Duration::from_millis(50));
    }
    l2!("Track Ender end");
}

fn stream(han_ch_s: Sender<OutputStreamHandle>, stm_ex_r: Receiver<()>) {
    l2!("Stream start");
    let (_stream, handle) = OutputStream::try_default().unwrap();
    han_ch_s.send(handle).unwrap();
    stm_ex_r.recv().unwrap();
    l2!("Stream end");
}

// ### BG TASKS ### }}}

pub struct Player {
    stream_handle: RwLock<Option<OutputStreamHandle>>,
    stm_ex_s: RwLock<Option<Sender<()>>>,
    volume_retained: RwLock<f32>,
    sink: Arc<RwLock<Option<Sink>>>,
    track: RwLock<Option<Arc<Track>>>,
}

impl Drop for Player {
    fn drop(&mut self) {
        self.hard_stop()
    }
}

impl Player {
    // # new # {{{
    pub fn new(track: Option<Arc<Track>>, signal_next: Option<Sender<()>>) -> Self {
        l2!("Constructing Player...");
        let now = Instant::now();

        let sink = Arc::new(RwLock::new(None));

        if let Some(sig) = signal_next {
            let thread_sink = sink.clone();
            thread::spawn(move || track_ender(thread_sink, sig));
        }

        let player = Self {
            stm_ex_s: RwLock::new(None),
            stream_handle: RwLock::new(None),
            volume_retained: RwLock::new(1.0f32),
            sink,
            track: RwLock::new(track),
        };

        l1!(format!("Player built in {:?}", Instant::now() - now));

        player
    }
    // # new # }}}

    // ## PLAYBACK ## {{{

    fn start(&self) {
        if self.stream_handle.read().unwrap().is_none() || self.stm_ex_s.read().unwrap().is_none() {
            let (han_ch_s, han_ch_r) = channel::bounded(1);
            let (stm_ex_s, stm_ex_r) = channel::bounded(1);
            thread::spawn(|| stream(han_ch_s, stm_ex_r));
            *self.stm_ex_s.write().unwrap() = Some(stm_ex_s);
            *self.stream_handle.write().unwrap() = Some(han_ch_r.recv().unwrap());
        }

        if let Some(reader) = self.track.read().unwrap().as_ref().map(|t| t.get_reader()) {
            match self
                .stream_handle
                .read()
                .unwrap()
                .as_ref()
                .unwrap()
                .play_once(reader)
            {
                Ok(sink) => {
                    sink.set_volume(*self.volume_retained.read().unwrap());
                    *self.sink.write().unwrap() = Some(sink);
                }
                Err(e) => panic!("{}", e),
            };
        }
    }

    pub fn pause(&self) {
        l2!("Pausing...");
        if let Some(sink) = &*self.sink.read().unwrap() {
            sink.pause()
        }
        l2!("Paused");
    }

    pub fn play(&self) {
        l2!("Starting playback...");
        if let Some(sink) = &*self.sink.read().unwrap() {
            if sink.is_paused() && !sink.empty() {
                sink.play();
                return;
            } else if !sink.empty() {
                // theoretically should be playing???
                return;
            }
        }
        self.start();
        l2!("Playing");
    }

    /// Clears playback buffer without removing the audio stream.
    /// Good for playing a different track.
    pub fn stop(&self) {
        l2!("Stopping...");
        *self.sink.write().unwrap() = None;
        l2!("Stopped");
    }

    /// Completely removes the audio stream
    /// Should be used when fully stopping playback to reduce idle load
    pub fn hard_stop(&self) {
        l2!("Hard Stopping...");
        if let Some(tx) = &*self.stm_ex_s.read().unwrap() {
            tx.send(()).unwrap();
        } else {
            return;
        }
        *self.sink.write().unwrap() = None;
        *self.stream_handle.write().unwrap() = None;
        *self.stm_ex_s.write().unwrap() = None;
        l2!("Hard Stopped");
    }
    // ## PLAYBACK ## }}}

    // ## GET/SET ## {{{

    pub fn volume_get(&self) -> f32 {
        self.volume_retained.read().unwrap().cbrt()
    }
    pub fn volume_set(&self, volume: f32) {
        let volume = 0.0f32.max(1.0f32.min(volume.powi(3)));
        if let Some(sink) = &*self.sink.read().unwrap() {
            sink.set_volume(volume)
        }
        *self.volume_retained.write().unwrap() = volume;
    }

    pub fn track_set(&self, track: Option<Arc<Track>>) {
        *self.track.write().unwrap() = track;
    }
    pub fn track_get(&self) -> Option<Arc<Track>> {
        self.track.read().unwrap().as_ref().cloned()
    }

    // ## GET/SET ## }}}

    // ## STATUS ## {{{

    pub fn playing(&self) -> bool {
        match &*self.sink.read().unwrap() {
            Some(sink) => !sink.empty() && !sink.is_paused(),
            None => false,
        }
    }

    pub fn paused(&self) -> bool {
        match &*self.sink.read().unwrap() {
            Some(sink) => sink.is_paused() && !sink.empty(),
            None => false,
        }
    }

    pub fn stopped(&self) -> bool {
        match &*self.sink.read().unwrap() {
            Some(sink) => sink.empty(),
            None => true,
        }
    }

    // ## STATUS ## }}}
}
