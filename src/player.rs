use rodio::{Decoder, OutputStream, OutputStreamHandle, Source};
use std::fs::File;
use std::io::BufReader;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Track {
    path: String,
}

static ORD: Ordering = Ordering::SeqCst;
static POLL_MS: u64 = 5;

impl Track {
    pub fn new(path: String) -> Self {
        Self { path }
    }
    pub fn get_reader(&self) -> BufReader<File> {
        BufReader::new(File::open(&self.path).unwrap())
    }
    pub fn get_decoder(&self) -> Result<Decoder<BufReader<File>>, rodio::decoder::DecoderError> {
        Decoder::new(self.get_reader())
    }
}

struct State {
    done: AtomicBool,
    pause: AtomicBool,
    stop: AtomicBool,
    volume: AtomicU8,
}

pub struct Player {
    #[allow(dead_code)]
    stream: OutputStream,

    stream_handle: OutputStreamHandle,
    state: Arc<State>,

    pub queue: Vec<Track>,
    pub index: usize,
}

impl Player {
    pub fn new() -> Self {
        let (stream, stream_handle) = OutputStream::try_default().unwrap();
        Self {
            stream,
            stream_handle,
            state: Arc::new(State {
                done: AtomicBool::new(true),
                pause: AtomicBool::new(false),
                stop: AtomicBool::new(true),
                volume: AtomicU8::new(5),
            }),
            index: 0,
            queue: Vec::new(),
        }
    }

    fn start(&mut self) {
        let state = self.state.clone();

        let src = self
            .get_track()
            .unwrap()
            .get_decoder()
            .unwrap()
            .amplify(1.0)
            .pausable(false)
            .stoppable()
            .periodic_access(Duration::from_millis(POLL_MS), move |src| {
                if state.stop.load(ORD) {
                    src.stop();
                    state.done.store(true, ORD);
                } else {
                    src.inner_mut()
                        .inner_mut()
                        .set_factor(state.volume.load(ORD) as f32 / 100.0);
                    src.inner_mut().set_paused(state.pause.load(ORD));
                }
            })
            .convert_samples();

        self.stream_handle.play_raw(src).unwrap();
        self.state.done.store(false, ORD);
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
        while !self.state.done.load(ORD) {
            std::thread::sleep(Duration::from_millis(POLL_MS))
        }
    }
    pub fn next(&mut self) {
        self.stop();
        self.index = (self.index + 1) % (self.queue.len());
        self.play();
    }

    // returns track from queue
    pub fn get_track(&self) -> Option<&Track> {
        self.queue.get(self.index)
    }
}
