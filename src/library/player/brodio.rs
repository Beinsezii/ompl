use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use rodio::{OutputStream, OutputStreamHandle, Sink};

use std::sync::mpsc::sync_channel;
use std::sync::mpsc::{Receiver, SyncSender};

use super::{Player, PlayerMessage};
use crate::{l1, l2, library::Track, log, LOG_LEVEL};

// ### BG TASKS ### {{{

fn track_ender(sink: Arc<RwLock<Option<Sink>>>, signal_next: SyncSender<PlayerMessage>) {
    l2!("Track Ender start");
    loop {
        if let Some(sink) = &*sink.read().unwrap() {
            if sink.empty() {
                if let Err(_) = signal_next.send(PlayerMessage::Request) {
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

fn stream(han_ch_s: SyncSender<OutputStreamHandle>, stm_ex_r: Receiver<()>) {
    l2!("Stream start");
    let (_stream, handle) = OutputStream::try_default().unwrap();
    han_ch_s.send(handle).unwrap();
    stm_ex_r.recv().unwrap();
    l2!("Stream end");
}

// ### BG TASKS ### }}}

pub struct Backend {
    stream_handle: RwLock<Option<OutputStreamHandle>>,
    stm_ex_s: RwLock<Option<SyncSender<()>>>,
    volume_retained: RwLock<f32>,
    sink: Arc<RwLock<Option<Sink>>>,
    track: RwLock<Option<Arc<Track>>>,
}

impl Drop for Backend {
    fn drop(&mut self) {
        self.stop()
    }
}

impl Player for Backend {
    fn new(sig_end: Option<SyncSender<PlayerMessage>>) -> Self {
        l2!("Constructing Backend...");
        let now = Instant::now();

        let sink = Arc::new(RwLock::new(None));

        if let Some(sig) = sig_end {
            let thread_sink = sink.clone();
            thread::spawn(move || track_ender(thread_sink, sig));
        }

        let player = Self {
            stm_ex_s: RwLock::new(None),
            stream_handle: RwLock::new(None),
            volume_retained: RwLock::new(1.0f32),
            sink,
            track: RwLock::new(None),
        };

        l1!(format!("Backend built in {:?}", Instant::now() - now));

        player
    }

    fn seekable(&self) -> Option<bool> {
        None
    }

    fn types(&self) -> Vec<String> {
        vec![
            String::from(".mp3"),
            String::from(".flac"),
            String::from(".ogg"),
            String::from(".wav"),
        ]
    }

    fn track_get(&self) -> Option<Arc<Track>> {
        self.track.read().unwrap().as_ref().cloned()
    }

    fn track_set(&self, mut track: Option<Arc<Track>>) -> Option<Arc<Track>> {
        let guard: &mut Option<Arc<Track>> = &mut self.track.write().unwrap();
        std::mem::swap(guard, &mut track);
        *self.sink.write().unwrap() = None;
        track
    }

    fn volume_get(&self) -> f32 {
        self.volume_retained.read().unwrap().cbrt()
    }

    fn volume_set(&self, volume: f32) {
        let volume = 0.0f32.max(1.0f32.min(volume.powi(3)));
        if let Some(sink) = &*self.sink.read().unwrap() {
            sink.set_volume(
                volume
                    * self
                        .track
                        .read()
                        .unwrap()
                        .as_ref()
                        .map(|t| t.gain())
                        .unwrap_or(1.0),
            )
        }
        *self.volume_retained.write().unwrap() = volume;
    }

    fn pause(&self) {
        l2!("Pausing...");
        if let Some(sink) = &*self.sink.read().unwrap() {
            sink.pause()
        }
        l2!("Paused");
    }

    fn play(&self) {
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
        if self.stream_handle.read().unwrap().is_none() || self.stm_ex_s.read().unwrap().is_none() {
            let (han_ch_s, han_ch_r) = sync_channel(1);
            let (stm_ex_s, stm_ex_r) = sync_channel(1);
            thread::spawn(|| stream(han_ch_s, stm_ex_r));
            *self.stm_ex_s.write().unwrap() = Some(stm_ex_s);
            *self.stream_handle.write().unwrap() = Some(han_ch_r.recv().unwrap());
        }

        if let Some(track) = self.track.read().unwrap().as_ref() {
            match self
                .stream_handle
                .read()
                .unwrap()
                .as_ref()
                .unwrap()
                .play_once(track.get_reader())
            {
                Ok(sink) => {
                    sink.set_volume(*self.volume_retained.read().unwrap() * track.gain());
                    *self.sink.write().unwrap() = Some(sink);
                }
                Err(e) => panic!("{}", e),
            };
        }
        l2!("Playing");
    }

    fn stop(&self) {
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

    fn playing(&self) -> bool {
        match &*self.sink.read().unwrap() {
            Some(sink) => !sink.empty() && !sink.is_paused(),
            None => false,
        }
    }

    fn paused(&self) -> bool {
        match &*self.sink.read().unwrap() {
            Some(sink) => sink.is_paused() && !sink.empty(),
            None => false,
        }
    }

    fn stopped(&self) -> bool {
        match &*self.sink.read().unwrap() {
            Some(sink) => sink.empty(),
            None => true,
        }
    }
}