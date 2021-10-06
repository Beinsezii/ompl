use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender, SyncSender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use rodio::{source::Done, Decoder, OutputStream, OutputStreamHandle, Source};

use super::track::Track;
use super::POLL_MS;

static ORD: Ordering = Ordering::SeqCst;

struct State {
    pause: AtomicBool,
    stop: AtomicBool,
    volume: AtomicU8,
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

    track: Option<Track>,
}

impl Drop for Player {
    fn drop(&mut self) {
        self.state.drop.store(true, ORD);
        self.stm_ex_s.send(()).unwrap();
    }
}

impl Player {
    pub fn new(track: Option<Track>, signal_next: Option<Sender<()>>) -> Self {
        let (han_ch_s, han_ch_r) = mpsc::channel();
        let (stm_ex_s, stm_ex_r) = mpsc::sync_channel(0);
        thread::spawn(|| stream(han_ch_s, stm_ex_r));
        let stream_handle = han_ch_r.recv().unwrap();

        let active = Arc::new(AtomicUsize::new(0));
        let state = Arc::new(State {
            pause: AtomicBool::new(false),
            stop: AtomicBool::new(true),
            volume: AtomicU8::new(5),
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
            track,
        }
    }

    fn start(&self) {
        if let Some(track) = self.get_track() {
            let state = self.state.clone();
            let active = self.active.clone();

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
                            src.inner_mut().inner_mut().set_factor(
                                std::cmp::min(state.volume.load(ORD), 100) as f32 / 100.0,
                            );
                            src.inner_mut().set_paused(state.pause.load(ORD));
                        }
                    })
                    .convert_samples(),
                active,
            );

            self.stream_handle.play_raw(src).unwrap();
            self.active.store(1, ORD);
        }
    }

    pub fn pause(&self) {
        if !self.state.stop.load(ORD) {
            self.state.pause.store(true, ORD);
        }
    }
    pub fn play(&self) {
        if self.state.stop.load(ORD) {
            self.start();
        }
        self.state.stop.store(false, ORD);
        self.state.pause.store(false, ORD);
    }
    pub fn stop(&self) {
        self.state.stop.store(true, ORD);
        self.state.pause.store(false, ORD);
        while self.active.load(ORD) != 0 {
            thread::sleep(Duration::from_millis(POLL_MS))
        }
    }
    pub fn play_pause(&self) {
        if !self.state.pause.load(ORD) && !self.state.stop.load(ORD) {
            self.pause();
        } else {
            self.play();
        }
    }
    pub fn next(&mut self, next: Option<Track>) {
        self.stop();
        self.track = next;
        self.play();
    }

    // returns track from
    pub fn get_track(&self) -> Option<&Track> {
        self.track.as_ref()
    }
}
