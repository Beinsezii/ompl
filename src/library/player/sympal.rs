#![warn(missing_docs)]

use super::{Player, PlayerMessage};
use crate::library::Track;

use std::{
    fs::File,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
        mpsc::SyncSender,
        Arc, Mutex, RwLock,
    },
    thread,
    time::Duration,
};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    SampleFormat, SampleRate,
};
use symphonia::core::{audio::SampleBuffer, io::MediaSourceStream, probe::Hint};

pub struct Backend {
    track: Mutex<Option<Arc<Track>>>,
    volume: Arc<AtomicU32>,
    channel: Option<SyncSender<PlayerMessage>>,
    join: Arc<AtomicBool>,
    first: Arc<AtomicBool>,
    last: Arc<AtomicBool>,
    // TODO: i16 probably better.
    // Wonder if Windows has as extensive of format support?
    samples: Mutex<Arc<RwLock<Vec<f32>>>>,
    pos: Arc<AtomicUsize>,
    rate: Arc<AtomicU32>,
    channels: Arc<AtomicUsize>,
}

impl Player for Backend {
    fn new(sig: Option<SyncSender<PlayerMessage>>) -> Self
    where
        Self: Sized,
    {
        Backend {
            track: Mutex::new(None),
            volume: Arc::new(AtomicU32::from(1.0f32.to_bits())),
            channel: sig,
            join: Arc::new(AtomicBool::new(true)),
            first: Arc::new(AtomicBool::new(false)),
            last: Arc::new(AtomicBool::new(false)),
            samples: Default::default(),
            pos: Arc::new(AtomicUsize::new(0)),
            rate: Arc::new(AtomicU32::new(0)),
            channels: Arc::new(AtomicUsize::new(0)),
        }
    }
    fn types(&self) -> Vec<String> {
        vec![
            String::from("flac"),
            String::from("m4a"),
            String::from("mp3"),
            String::from("ogg"),
            String::from("wav"),
        ]
    }
    fn play(&self) {
        // {{{
        let channel = if let Some(channel) = &self.channel {
            channel.clone()
        } else {
            return;
        };

        self.join.store(true, Ordering::Relaxed);

        let vol = self.volume.clone();
        let gain = self.track.lock().unwrap().as_ref().unwrap().gain();

        let host = cpal::default_host();

        let device = host
            .default_output_device()
            .expect("No audio output device found");

        let rate = self.rate.load(Ordering::Relaxed);
        let channels = self.channels.load(Ordering::Relaxed);

        let config = if let Ok(mut configs) = device.supported_output_configs() {
            loop {
                match configs.next() {
                    Some(config) => {
                        if config.channels() as usize == channels
                            && config.max_sample_rate().0 >= rate
                            && config.min_sample_rate().0 <= rate
                            && config.sample_format() == SampleFormat::F32
                        {
                            break config.with_sample_rate(SampleRate(rate));
                        }
                    }
                    None => panic!("No valid config for audio device found!"),
                }
            }
        } else {
            panic!("No configs for audio device found!")
        };

        self.join.store(false, Ordering::Relaxed);

        let tj = self.join.clone();
        let uj = self.join.clone();
        let pos = self.pos.clone();
        let first = self.first.clone();
        let samples = self.samples.lock().unwrap().clone();
        let channel_err = channel.clone();

        thread::Builder::new()
            .name(String::from("Audio Stream"))
            .spawn(move || {
                while !first.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(1))
                }
                let stream = device
                    .build_output_stream(
                        &config.config(),
                        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                            for sample in data {
                                // Wonder how necessary these locks are.
                                // IDK much about atomics but this seems to make sense to ensure
                                // the same pos isn't loaded twice, right?
                                let n = pos.load(Ordering::Acquire);
                                *sample = gain
                                    * f32::from_bits(vol.load(Ordering::Relaxed)).powi(3)
                                    * match samples.read().unwrap().get(n) {
                                        Some(s) => s,
                                        None => {
                                            if !uj.load(Ordering::Acquire) {
                                                channel.send(PlayerMessage::Request).unwrap()
                                            };
                                            uj.store(true, Ordering::Release);
                                            &0.0
                                        }
                                    };
                                pos.store(n + 1, Ordering::Release);
                            }
                            // react to stream events and read or write stream data here.
                        },
                        move |err| {
                            channel_err
                                .send(PlayerMessage::Error(err.to_string()))
                                .unwrap();
                        },
                        None, // None=blocking, Some(Duration)=timeout
                    )
                    .expect("Could not initialize audio stream");
                stream.play().unwrap();
                while !tj.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(1))
                }
            })
            .unwrap();
    }
    // }}}

    fn stop(&self) {
        self.pause();
        self.pos.store(0, Ordering::Relaxed);
    }
    fn pause(&self) {
        self.join.store(true, Ordering::Relaxed);
    }
    fn playing(&self) -> bool {
        !self.join.load(Ordering::Relaxed)
    }
    fn paused(&self) -> bool {
        self.pos.load(Ordering::Relaxed) != 0 && self.join.load(Ordering::Relaxed)
    }
    fn seekable(&self) -> Option<bool> {
        Some(self.last.load(Ordering::Relaxed))
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
        // {{{
        self.join.store(true, Ordering::Relaxed);
        let guard: &mut Option<Arc<Track>> = &mut self.track.lock().unwrap();
        if *guard == track {
            self.stop()
        } else {
            std::mem::swap(guard, &mut track);

            if let Some(track) = guard.as_ref() {
                let mss = MediaSourceStream::new(
                    Box::new(File::open(track.path().clone()).unwrap()),
                    Default::default(),
                );

                let mut fr = symphonia::default::get_probe()
                    .format(
                        Hint::new()
                            .with_extension(track.path().extension().unwrap().to_str().unwrap()),
                        mss,
                        &Default::default(),
                        &Default::default(),
                    )
                    .expect("SYMPAL Probe result return ERR")
                    .format;

                let audio_tracks = fr.tracks();

                let mut decoder = symphonia::default::get_codecs()
                    .make(&audio_tracks[0].codec_params, &Default::default())
                    .expect("SYMAPL Decoder return ERR");

                let rate = decoder
                    .codec_params()
                    .sample_rate
                    .expect("SYMPAL Decoder has no sample rate");
                self.rate.store(rate, Ordering::Relaxed);
                self.channels.store(
                    decoder
                        .codec_params()
                        .channels
                        .expect("SYMPAL Decoder has no channels")
                        .count(),
                    Ordering::Relaxed,
                );

                *self.samples.lock().unwrap() =
                    Arc::new(RwLock::new(Vec::with_capacity(rate as usize * 120)));

                let (samples, first, last) = (
                    Arc::downgrade(&self.samples.lock().unwrap()),
                    self.first.clone(),
                    self.last.clone(),
                );

                // Paranoia, making 100% sure any other decode threads drop first.
                // Probably not necessary.
                // Felt cute, might remove later.
                thread::sleep(Duration::from_millis(1));

                self.first.store(false, Ordering::Relaxed);
                self.last.store(false, Ordering::Relaxed);
                self.pos.store(0, Ordering::Relaxed);

                thread::Builder::new()
                    .name(String::from("Decoder"))
                    .spawn(move || {
                        while let Ok(packet) = fr.next_packet() {
                            if packet.dur() < 1 {
                                // 0 length packets are possible I guess
                                continue;
                            }
                            if let Some(samples) = samples.upgrade() {
                                let ab = match decoder.decode(&packet) {
                                    Ok(ab) => ab,
                                    // Symphonia docs say these errs should just discard packet
                                    Err(symphonia::core::errors::Error::DecodeError(..))
                                    | Err(symphonia::core::errors::Error::IoError(..)) => continue,
                                    Err(e) => std::panic::panic_any(e),
                                };
                                let mut sb = SampleBuffer::<f32>::new(packet.dur, *ab.spec());
                                sb.copy_interleaved_ref(ab);
                                samples.write().unwrap().extend_from_slice(sb.samples());
                                first.store(true, Ordering::Relaxed);
                            } else {
                                return;
                            }
                        }
                        last.store(true, Ordering::Relaxed);
                    })
                    .unwrap();
            }
        }
        track
    }
    // }}}
}
