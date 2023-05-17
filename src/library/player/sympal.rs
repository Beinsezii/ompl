use super::{Player, PlayerMessage};
use crate::library::Track;

use std::{
    fs::File,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering, AtomicUsize},
        mpsc::SyncSender,
        Arc, Mutex, RwLock,
    },
    thread,
    time::{Duration, Instant},
};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Stream,
};
use symphonia::core::{
    audio::{AudioBuffer, RawSample, SampleBuffer},
    conv::ConvertibleSample,
    io::{MediaSourceStream, ReadBytes},
    probe::Hint,
};

pub struct Backend {
    track: Mutex<Option<Arc<Track>>>,
    volume: Arc<AtomicU32>,
    channel: Option<SyncSender<PlayerMessage>>,
    join: Arc<AtomicBool>,
    pos: Arc<AtomicUsize>,
}

trait AudioOutputSample:
    cpal::Sample + ConvertibleSample + RawSample + std::marker::Send + 'static
{
}

impl AudioOutputSample for f32 {}

impl Player for Backend {
    fn new(sig: Option<SyncSender<PlayerMessage>>) -> Self
    where
        Self: Sized,
    {
        Backend {
            track: Mutex::new(None),
            volume: Arc::new(AtomicU32::from(1.0f32.to_bits())),
            channel: sig,
            join: Arc::new(AtomicBool::new(false)),
            pos: Arc::new(AtomicUsize::new(0)),
        }
    }
    fn types(&self) -> Vec<String> {
        vec![String::from("mp3")]
    }
    fn play(&self) {
        let channel = if let Some(channel) = &self.channel {
            channel
        } else {
            return;
        };

        self.join.store(true, Ordering::Relaxed);

        let mss = MediaSourceStream::new(
            Box::new(
                File::open(self.track.lock().unwrap().as_ref().unwrap().path().clone()).unwrap(),
            ),
            Default::default(),
        );

        let mut fr = symphonia::default::get_probe()
            .format(
                Hint::new().with_extension("mp3"),
                mss,
                &Default::default(),
                &Default::default(),
            )
            .unwrap()
            .format;

        let tracks = fr.tracks();

        let mut decoder = symphonia::default::get_codecs()
            .make(&tracks[0].codec_params, &Default::default())
            .unwrap();

        let samples = Arc::new(RwLock::new(Vec::new()));
        let first = Arc::new(AtomicBool::new(false));
        let (tsamples, tfirst, tj) = (samples.clone(), first.clone(), self.join.clone());

        let vol = self.volume.clone();
        let gain = self.track.lock().unwrap().as_ref().unwrap().gain();

        let host = cpal::default_host();

        let device = host
            .default_output_device()
            .expect("No audio output device found");

        let config = device
            .default_output_config()
            .expect("Audio device has no supported configs");

        self.join.store(false, Ordering::Relaxed);

        thread::spawn( move ||
            while let Ok(packet) = fr.next_packet() {
                let ab = decoder.decode(&packet).unwrap();
                let mut sb = SampleBuffer::<f32>::new(packet.dur, *ab.spec());
                sb.copy_interleaved_ref(ab);
                tsamples.write().unwrap().extend_from_slice(sb.samples());
                tfirst.store(true, Ordering::Relaxed);
                if tj.load(Ordering::Relaxed) { break }
            }
        );

        let tj = self.join.clone();
        let uj = self.join.clone();
        let pos = self.pos.clone();
        pos.store(0, Ordering::Relaxed);

        thread::spawn(move || {
            while ! first.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(1))
            }
            let stream = device
                .build_output_stream(
                    &config.config(),
                    move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                        for sample in data {
                            let n = pos.load(Ordering::SeqCst);
                            *sample = gain
                                * f32::from_bits(vol.load(Ordering::Relaxed)).powi(3)
                                * match samples.read().unwrap().get(n) {
                                    Some(s) => s,
                                    None => {
                                        uj.store(true, Ordering::Relaxed);
                                        &0.0
                                    }
                                };
                            pos.store(n+1, Ordering::SeqCst);
                        }
                        // react to stream events and read or write stream data here.
                    },
                    move |err| {},
                    None, // None=blocking, Some(Duration)=timeout
                )
                .expect("Could not initialize audio stream");
            stream.play().unwrap();
            while !tj.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(1))
            }
        });
    }

    fn stop(&self) {}
    fn pause(&self) {}
    fn playing(&self) -> bool {
        false
    }
    fn paused(&self) -> bool {
        false
    }
    fn seekable(&self) -> Option<bool> {
        Some(false)
    }
    fn volume_set(&self, volume: f32) {
        self.volume
            .store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed)
    }
    fn volume_get(&self) -> f32 {
        f32::from_bits(self.volume.load(Ordering::Relaxed))
    }
    fn track_get(&self) -> Option<Arc<Track>> {
        self.track.lock().unwrap().clone()
    }
    fn track_set(&self, mut track: Option<Arc<Track>>) -> Option<Arc<Track>> {
        let guard: &mut Option<Arc<Track>> = &mut self.track.lock().unwrap();
        std::mem::swap(guard, &mut track);
        track
    }
}
