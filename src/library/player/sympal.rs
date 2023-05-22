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
    Sample, SampleFormat, SampleRate,
};
use symphonia::core::{
    audio::SampleBuffer,
    io::MediaSourceStream,
    probe::{Hint, QueryDescriptor},
};

pub struct Backend {
    track: Mutex<Option<Arc<Track>>>,
    volume: Arc<AtomicU32>,
    channel: SyncSender<PlayerMessage>,
    join: Arc<AtomicBool>,
    first: Arc<AtomicBool>,
    last: Arc<AtomicBool>,
    // TODO: dynamic typing
    samples: Mutex<Arc<RwLock<Vec<i16>>>>,
    pos: Arc<AtomicUsize>,
    rate: Arc<AtomicU32>,
    channels: Arc<AtomicUsize>,
}

impl Player for Backend {
    fn new(sig: SyncSender<PlayerMessage>) -> Self
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
    #[rustfmt::skip]
    fn types(&self) -> Vec<String> {
        let mut result = [
            symphonia::default::formats::OggReader::query(),
            symphonia::default::formats::MkvReader::query(),
            symphonia::default::formats::MpaReader::query(),
            symphonia::default::formats::FlacReader::query(),
            symphonia::default::formats::WavReader::query(),
            symphonia::default::formats::AdtsReader::query(),
            // symphonia::default::formats::IsoMp4Reader::query(),
        ]
        .iter().map(|descriptors|
            descriptors.iter().map(|descriptor|
                descriptor.extensions.iter().map(|extension|
                    extension.to_string()
                )
            )
        )
        .flatten()
        .flatten()
        .collect::<Vec<String>>();
        result.sort();
        result.dedup();
        result
    }
    fn play(&self) {
        // {{{
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
                            && config.sample_format() == SampleFormat::I16
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

        let join_thread = self.join.clone();
        let join_data = self.join.clone();
        let pos = self.pos.clone();
        let first = self.first.clone();
        let samples = self.samples.lock().unwrap().clone();
        let channels = self.channels.load(Ordering::Relaxed) as u32;
        let rate = self.rate.load(Ordering::Relaxed);
        let channel_str = self.channel.clone();
        let channel_err = self.channel.clone();
        let join_err = self.join.clone();
        let pos_err = self.pos.clone();

        thread::Builder::new()
            .name(String::from("SYMPAL Audio Stream"))
            .spawn(move || {
                while !first.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(1))
                }
                let stream = device
                    .build_output_stream(
                        &config.config(),
                        move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                            let amplitude =
                                gain * f32::from_bits(vol.load(Ordering::Relaxed)).powi(3);
                            let p = pos.load(Ordering::Relaxed);
                            let mut n = 0;
                            for sample in data {
                                *sample = match samples.read().unwrap().get(n + p) {
                                    Some(s) => {
                                        n += 1;
                                        s.mul_amp(amplitude)
                                    }
                                    None => {
                                        // these acquire/release should make sure
                                        // no dupe messages right?
                                        if !join_data.load(Ordering::Acquire) {
                                            channel_str.send(PlayerMessage::Request).unwrap()
                                        };
                                        join_data.store(true, Ordering::Release);
                                        i16::EQUILIBRIUM
                                    }
                                };
                            }
                            pos.store(p + n, Ordering::Relaxed);
                            if (p as f32 / (rate * channels) as f32).floor()
                                < ((p + n) as f32 / (rate * channels) as f32).floor()
                            {
                                channel_str.send(PlayerMessage::Clock).unwrap();
                            }
                        },
                        move |err| {
                            // TODO can some of these actually be handled?
                            join_err.store(true, Ordering::Relaxed);
                            channel_err
                                .send(PlayerMessage::Error(format!(
                                    "SYMPAL Audio Stream Error:\n{}",
                                    err
                                )))
                                .unwrap();
                            pos_err.store(0, Ordering::Relaxed);
                        },
                        None, // None=blocking, Some(Duration)=timeout
                    )
                    .expect("Could not initialize audio stream");
                stream.play().unwrap();
                while !join_thread.load(Ordering::Relaxed) {
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
        // wonder if there should be a 1ms sleep here
        // since that's what the thread checks on?
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
    fn times(&self) -> Option<(Duration, Duration)> {
        match self.last.load(Ordering::Relaxed) {
            true => Some((
                Duration::from_secs_f64(
                    (self.pos.load(Ordering::Relaxed) as f64)
                        / (self.rate.load(Ordering::Relaxed) as f64)
                        / (self.channels.load(Ordering::Relaxed) as f64),
                ),
                Duration::from_secs_f64(
                    (self.samples.lock().unwrap().read().unwrap().len() as f64)
                        / (self.rate.load(Ordering::Relaxed) as f64)
                        / (self.channels.load(Ordering::Relaxed) as f64),
                ),
            )),
            false => None,
        }
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
                let channels = decoder
                    .codec_params()
                    .channels
                    .expect("SYMPAL Decoder has no channels")
                    .count();
                self.channels.store(channels, Ordering::Relaxed);

                // allocate 2 minutes' worth of initial space
                *self.samples.lock().unwrap() = Arc::new(RwLock::new(Vec::with_capacity(
                    rate as usize * 120 * channels,
                )));

                let (samples, first, last) = (
                    Arc::downgrade(&self.samples.lock().unwrap()),
                    self.first.clone(),
                    self.last.clone(),
                );

                self.first.store(false, Ordering::Relaxed);
                self.last.store(false, Ordering::Relaxed);
                self.pos.store(0, Ordering::Relaxed);
                let channel_dec = self.channel.clone();

                thread::Builder::new()
                    .name(String::from("SYMPAL Decoder"))
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
                                let mut sb = SampleBuffer::<i16>::new(packet.dur, *ab.spec());
                                sb.copy_interleaved_ref(ab);
                                samples.write().unwrap().extend_from_slice(sb.samples());
                                first.store(true, Ordering::Relaxed);
                            } else {
                                return;
                            }
                        }
                        if let Some(samples) = samples.upgrade() {
                            last.store(true, Ordering::Relaxed);
                            samples.write().unwrap().shrink_to_fit();
                            channel_dec.send(PlayerMessage::Seekable).unwrap()
                        }
                    })
                    .unwrap();
            }
        }
        track
    }
    // }}}
}
