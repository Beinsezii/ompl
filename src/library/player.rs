use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender, SyncSender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use rodio::{source::Done, Decoder, OutputStream, OutputStreamHandle, Source};

use super::StatusSync;

use crate::{l1, l2, l4, log, LOG_LEVEL};

static PLAYBACK_DEBUG: AtomicUsize = AtomicUsize::new(0);
const POLL_MS: u64 = 5;
const ORD: Ordering = Ordering::SeqCst;
pub const TYPES: &[&'static str] = &[".mp3", ".flac", ".ogg", ".wav"];

struct State {
    pause: AtomicBool,
    stop: AtomicBool,
    drop: AtomicBool,
}

fn track_ender(state: Arc<State>, active: Arc<AtomicUsize>, signal_next: Sender<()>) {
    l2!("Track Ender start");
    while !state.drop.load(ORD) {
        if active.load(ORD) == 0 && !state.stop.load(ORD) {
            if let Err(_) = signal_next.send(()) {
                break;
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

pub struct Player {
    stream_handle: OutputStreamHandle,
    stm_ex_s: SyncSender<()>,

    active: Arc<AtomicUsize>,
    state: Arc<State>,

    status: StatusSync,
}

impl Drop for Player {
    fn drop(&mut self) {
        self.state.drop.store(true, ORD);
        self.stm_ex_s.send(()).unwrap();
    }
}

impl Player {
    pub fn new(status: StatusSync, signal_next: Option<Sender<()>>) -> Self {
        l2!("Constructing Player...");
        let now = Instant::now();

        let (han_ch_s, han_ch_r) = mpsc::channel();
        let (stm_ex_s, stm_ex_r) = mpsc::sync_channel(0);
        thread::spawn(|| stream(han_ch_s, stm_ex_r));
        let stream_handle = han_ch_r.recv().unwrap();

        let active = Arc::new(AtomicUsize::new(0));
        let state = Arc::new(State {
            pause: AtomicBool::new(false),
            stop: AtomicBool::new(true),
            drop: AtomicBool::new(false),
        });

        if let Some(sig) = signal_next {
            let thread_state = state.clone();
            let thread_active = active.clone();
            thread::spawn(move || track_ender(thread_state, thread_active, sig));
        }

        let player = Self {
            stm_ex_s,
            stream_handle,
            active,
            state,
            status,
        };

        l1!(format!("Player built in {:?}", Instant::now() - now));

        player
    }

    fn start(&self) {
        let reader = self
            .status
            .read()
            .unwrap()
            .track
            .as_ref()
            .map(|t| t.get_reader());
        if let Some(reader) = reader {
            PLAYBACK_DEBUG.store(0, ORD);
            let state = self.state.clone();
            let active = self.active.clone();
            let status = self.status.clone();
            let title = status
                .read()
                .unwrap()
                .track
                .as_ref()
                .unwrap()
                .tags()
                .get("title")
                .unwrap_or(&"???".to_string())
                .clone();
            l2!(format!("Starting track \"{}\"...", &title));

            let src = Done::new(
                Decoder::new(reader)
                    .unwrap()
                    .amplify(1.0)
                    .pausable(false)
                    .stoppable()
                    .periodic_access(Duration::from_millis(POLL_MS), move |src| {
                        if state.stop.load(ORD) {
                            src.stop();
                        } else {
                            src.inner_mut()
                                .inner_mut()
                                .set_factor(status.read().unwrap().volume);
                            src.inner_mut().set_paused(state.pause.load(ORD));
                        }
                        let iters = PLAYBACK_DEBUG.fetch_add(1, ORD);
                        if iters > 100 {
                            l4!("100 playback iterations!!!");
                            PLAYBACK_DEBUG.store(0, ORD);
                        }
                    })
                    .convert_samples(),
                active,
            );

            self.stream_handle.play_raw(src).unwrap();
            self.state.stop.store(false, ORD);
            self.active.store(1, ORD);
            self.status.write().unwrap().playing = true;
        }
    }

    pub fn pause(&self) {
        l2!("Pausing...");
        if !self.state.stop.load(ORD) {
            self.state.pause.store(true, ORD);
        }
        self.status.write().unwrap().playing = false;
        l2!("Paused");
    }
    pub fn play(&self) {
        l2!("Starting playback...");
        if self.state.stop.load(ORD) {
            self.start();
        } else {
            self.status.write().unwrap().playing = true;
        }
        self.state.pause.store(false, ORD);
        l2!("Playing");
    }
    pub fn stop(&self) {
        l2!("Stopping...");
        self.state.stop.store(true, ORD);
        self.state.pause.store(false, ORD);
        // sometimes Done doesn't fire. Idk why.
        let mut breaker = 0;
        while self.active.load(ORD) != 0 {
            thread::sleep(Duration::from_millis(POLL_MS));
            if breaker > 500 / POLL_MS {
                l2!("Harsh stop!");
                break;
            }
            breaker += 1;
        }
        self.status.write().unwrap().playing = false;
        l2!("Stopped");
    }
}
