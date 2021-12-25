use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use rodio::{OutputStream, OutputStreamHandle, Sink};

use crossbeam::channel;
use crossbeam::channel::{Receiver, Sender};

use super::track::Track;

use crate::{l1, l2, log, LOG_LEVEL};

const POLL_MS: u64 = 5;
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

        thread::sleep(Duration::from_millis(POLL_MS));
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
    stream_handle: OutputStreamHandle,
    stm_ex_s: Sender<()>,
    volume_retained: RwLock<f32>,
    sink: Arc<RwLock<Option<Sink>>>,
    track: RwLock<Option<Arc<Track>>>,
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stm_ex_s.send(()).unwrap();
    }
}

impl Player {
    // # new # {{{
    pub fn new(track: Option<Arc<Track>>, signal_next: Option<Sender<()>>) -> Self {
        l2!("Constructing Player...");
        let now = Instant::now();

        let (han_ch_s, han_ch_r) = channel::bounded(1);
        let (stm_ex_s, stm_ex_r) = channel::bounded(1);
        thread::spawn(|| stream(han_ch_s, stm_ex_r));
        let stream_handle = han_ch_r.recv().unwrap();

        let sink = Arc::new(RwLock::new(None));

        if let Some(sig) = signal_next {
            let thread_sink = sink.clone();
            thread::spawn(move || track_ender(thread_sink, sig));
        }

        let player = Self {
            stm_ex_s,
            stream_handle,
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
        if let Some(reader) = self.track.read().unwrap().as_ref().map(|t| t.get_reader()) {
            match self.stream_handle.play_once(reader) {
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

    pub fn stop(&self) {
        l2!("Stopping...");
        *self.sink.write().unwrap() = None;
        l2!("Stopped");
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
