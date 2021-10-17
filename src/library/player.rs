use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender, SyncSender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use rodio::{source::Done, Decoder, OutputStream, OutputStreamHandle, Source};

use super::StatusSync;
use super::POLL_MS;

static ORD: Ordering = Ordering::SeqCst;
pub static TYPES: &[&'static str] = &[".mp3", ".flac", ".ogg", ".wav"];

struct State {
    pause: AtomicBool,
    stop: AtomicBool,
    drop: AtomicBool,
}

fn track_ender(state: Arc<State>, active: Arc<AtomicUsize>, signal_next: Sender<()>) {
    while !state.drop.load(ORD) {
        if active.load(ORD) == 0 && !state.stop.load(ORD) {
            if let Err(_) = signal_next.send(()) {
                break;
            }
        }

        thread::sleep(Duration::from_millis(POLL_MS));
    }
}

fn stream(han_ch_s: Sender<OutputStreamHandle>, stm_ex_r: Receiver<()>) {
    let (_stream, handle) = OutputStream::try_default().unwrap();
    han_ch_s.send(handle).unwrap();
    stm_ex_r.recv().unwrap();
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

        Self {
            stm_ex_s,
            stream_handle,
            active,
            state,
            status,
        }
    }

    fn start(&self) {
        if let Some(track) = &self.status.read().unwrap().track {
            let state = self.state.clone();
            let active = self.active.clone();
            let status = self.status.clone();

            let src = Done::new(
                Decoder::new(track.get_reader())
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
        if !self.state.stop.load(ORD) {
            self.state.pause.store(true, ORD);
        }
        self.status.write().unwrap().playing = false;
    }
    pub fn play(&self) {
        if self.state.stop.load(ORD) {
            self.start();
        } else {
            self.status.write().unwrap().playing = true;
        }
        self.state.pause.store(false, ORD);
    }
    pub fn stop(&self) {
        self.state.stop.store(true, ORD);
        self.state.pause.store(false, ORD);
        // sometimes Done doesn't fire. Idk why.
        let mut breaker = 0;
        while self.active.load(ORD) != 0 && breaker < std::cmp::max(500 / POLL_MS, 10) {
            thread::sleep(Duration::from_millis(POLL_MS));
            breaker += 1;
        }
        self.status.write().unwrap().playing = false;
    }
}
