use rodio::{source::Done, Decoder, OutputStream, OutputStreamHandle, Source};
use std::fs::File;
use std::io::BufReader;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use std::sync::mpsc::{Receiver, Sender, SyncSender};

use super::track::Track;

static ORD: Ordering = Ordering::SeqCst;
static POLL_MS: u64 = 5;

struct State {
    pause: AtomicBool,
    stop: AtomicBool,
    volume: AtomicU8,
    drop: AtomicBool,
}

fn get_decoder(track: &Track) -> Result<Decoder<BufReader<File>>, rodio::decoder::DecoderError> {
    Decoder::new(track.get_reader())
}

fn track_ender(state: Arc<State>, active: Arc<AtomicUsize>, signal_next: Sender<()>) {
    let mut cur = (active.load(ORD), state.stop.load(ORD));
    let mut prev = cur.clone();
    loop {
        if state.drop.load(ORD) {
            break;
        };

        cur = (active.load(ORD), state.stop.load(ORD));
        if cur != prev {
            println!("ACT: {} STOP: {}", cur.0, cur.1);
            if cur.0 == 0 && !cur.1 {
                println!("next?");
                if let Err(_) = signal_next.send(()) {
                    break;
                }
            }
        }
        prev = cur;
        std::thread::sleep(Duration::from_millis(POLL_MS));
    }
}

fn stream(hanchan_s: Sender<OutputStreamHandle>, sterm_r: Receiver<()>) {
    let (_stream, handle) = OutputStream::try_default().unwrap();
    hanchan_s.send(handle).unwrap();
    sterm_r.recv().unwrap();
}

pub struct Player {
    stream_handle: OutputStreamHandle,
    stmterm_s: SyncSender<()>,

    active: Arc<AtomicUsize>,
    state: Arc<State>,

    track: Option<Track>,
}

impl Drop for Player {
    fn drop(&mut self) {
        self.state.drop.store(true, ORD);
        self.stmterm_s.send(()).unwrap();
    }
}

impl Player {
    pub fn new(track: Option<Track>, signal_next: Sender<()>) -> Self {
        let (hanchan_s, hanchan_r) = std::sync::mpsc::channel();
        let (stmterm_s, stmterm_r) = std::sync::mpsc::sync_channel(0);
        std::thread::spawn(|| stream(hanchan_s, stmterm_r));
        let stream_handle = hanchan_r.recv().unwrap();

        let active = Arc::new(AtomicUsize::new(0));
        let state = Arc::new(State {
            pause: AtomicBool::new(false),
            stop: AtomicBool::new(true),
            volume: AtomicU8::new(5),
            drop: AtomicBool::new(false),
        });

        let thread_state = state.clone();
        let thread_active = active.clone();
        std::thread::spawn(move || track_ender(thread_state, thread_active, signal_next));

        Self {
            stmterm_s,
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
                get_decoder(track)
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
            std::thread::sleep(Duration::from_millis(POLL_MS))
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
