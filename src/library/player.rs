use rodio::{Decoder, OutputStream, OutputStreamHandle, Source};
use std::fs::File;
use std::io::BufReader;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::track::Track;

static ORD: Ordering = Ordering::SeqCst;
static POLL_MS: u64 = 5;

struct State {
    active: AtomicUsize,
    pause: AtomicBool,
    stop: AtomicBool,
    volume: AtomicU8,
}

fn get_decoder(track: &Track) -> Result<Decoder<BufReader<File>>, rodio::decoder::DecoderError> {
    Decoder::new(track.get_reader())
}

pub struct Player {
    #[allow(dead_code)]
    stream: OutputStream,
    stream_handle: OutputStreamHandle,

    state: Arc<State>,

    track: Option<Track>,
}

impl Player {
    pub fn new(track: Option<Track>) -> Self {
        let (stream, stream_handle) = OutputStream::try_default().unwrap();
        Self {
            stream,
            stream_handle,
            state: Arc::new(State {
                active: AtomicUsize::new(0),
                pause: AtomicBool::new(false),
                stop: AtomicBool::new(true),
                volume: AtomicU8::new(5),
            }),
            track,
        }
    }

    fn start(&mut self) {
        if let Some(track) = self.get_track() {
            let state = self.state.clone();

            let src = get_decoder(track)
                .unwrap()
                .amplify(1.0)
                .pausable(false)
                .stoppable()
                .periodic_access(Duration::from_millis(POLL_MS), move |src| {
                    if state.stop.load(ORD) {
                        src.stop();
                        state.active.store(0, ORD);
                    } else {
                        src.inner_mut()
                            .inner_mut()
                            .set_factor(std::cmp::min(state.volume.load(ORD), 100) as f32 / 100.0);
                        src.inner_mut().set_paused(state.pause.load(ORD));
                    }
                })
                .convert_samples();

            self.stream_handle.play_raw(src).unwrap();
            self.state.active.store(1, ORD);
        }
    }

    pub fn pause(&self) {
        if !self.state.stop.load(ORD) {
            self.state.pause.store(true, ORD);
        }
    }
    pub fn play(&mut self) {
        if self.state.stop.load(ORD) {
            self.start();
        }
        self.state.stop.store(false, ORD);
        self.state.pause.store(false, ORD);
    }
    pub fn stop(&mut self) {
        self.state.stop.store(true, ORD);
        self.state.pause.store(false, ORD);
        while self.state.active.load(ORD) != 0 {
            std::thread::sleep(Duration::from_millis(POLL_MS))
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
