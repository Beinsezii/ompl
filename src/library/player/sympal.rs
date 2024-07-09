#![warn(missing_docs)]

use super::{Player, PlayerMessage};
use crate::library::Track;
use crate::{bench, debug, info, log, LOG};

use std::fs::File;
use std::ops::Deref;
use std::panic;
use std::sync::{
    atomic::{AtomicBool, AtomicU32, AtomicU8, AtomicUsize, Ordering},
    mpsc::SyncSender,
    Arc, Mutex, RwLock,
};
use std::thread;
use std::time::{Duration, Instant};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    SampleFormat, SampleRate,
};
use dasp::{Sample, Signal};

use symphonia::core::{
    audio::SampleBuffer,
    io::MediaSourceStream,
    probe::{Hint, QueryDescriptor},
};

fn sleep1() {
    thread::sleep(Duration::from_millis(1))
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum DecoderState {
    Empty,
    Decoding,
    Complete,
    Error,
}

impl From<u8> for DecoderState {
    fn from(value: u8) -> Self {
        unsafe { std::mem::transmute(value) }
    }
}

impl Deref for DecoderState {
    type Target = u8;
    fn deref(&self) -> &Self::Target {
        unsafe { std::mem::transmute(self) }
    }
}

pub struct Backend {
    track: Mutex<Option<Arc<Track>>>,
    volume: Arc<AtomicU32>,
    channel: SyncSender<PlayerMessage>,
    join: Arc<AtomicBool>,
    streaming: Arc<AtomicBool>,
    decoder_state: Arc<AtomicU8>,
    // TODO: dynamic typing
    samples: Mutex<Arc<RwLock<Vec<i16>>>>,
    pos: Arc<AtomicUsize>,
    rate: Arc<AtomicU32>,
    channels: Arc<AtomicUsize>,
    device_rate: Arc<AtomicU32>,
    device_channels: Arc<AtomicU32>,
    device_format: Arc<AtomicU8>,
}

impl Backend {
    fn err<T: ToString>(&self, message: T) {
        self.channel
            .send(PlayerMessage::Error(message.to_string()))
            .expect(&format!("Sympal could not forward error {}", message.to_string()));
    }

    fn get_device(&self) -> Option<(cpal::Device, cpal::SupportedStreamConfig)> {
        // {{{
        #[cfg(feature = "jack")]
        let requested_id = Some(cpal::HostId::Jack);

        #[cfg(not(feature = "jack"))]
        let requested_id = None;

        let format = SampleFormat::I16;
        let rate = self.rate.load(Ordering::Relaxed);
        let channels = self.channels.load(Ordering::Relaxed) as u16;

        debug!("Sympal play acquire device");
        let device = if let Some(device) = if let Some(host) = requested_id.map(|id| cpal::host_from_id(id).ok()).flatten() {
            if host.default_output_device().is_some() {
                host.default_output_device()
            } else {
                cpal::default_host().default_output_device()
            }
        } else {
            cpal::default_host().default_output_device()
        } {
            device
        } else {
            self.err("Could not find a valid output device.");
            return None;
        };

        let mut state = 0b000u8;
        let relax_format = 0b001u8;
        let relax_rate = 0b010u8;
        let _relax_channels = 0b100u8;

        debug!("Sympal play acquire config");
        let config = if let Ok(mut configs) = device.supported_output_configs() {
            loop {
                if let Some(c) = configs.next() {
                    if c.channels() == channels
                        && (c.max_sample_rate().0 >= rate || state & relax_rate != 0)
                        && (c.min_sample_rate().0 <= rate || state & relax_rate != 0)
                        && (c.sample_format() == format || state & relax_format != 0)
                    {
                        break if state & relax_rate == 0 {
                            c.with_sample_rate(SampleRate(rate))
                        } else {
                            c.with_max_sample_rate()
                        };
                    }
                } else if state < 0b111 {
                    state += 1;
                    configs = device.supported_output_configs().expect("SYMPAL state config re-iter fail");
                } else {
                    self.err(&format!(
                        "Sympal: Could not find a valid configuration for device '{}'.\nRequested: {}b {}x {}Hz\nFound: \n{}",
                        device.name().unwrap_or(String::from("ERR")),
                        format,
                        channels,
                        rate,
                        device
                            .supported_output_configs()
                            .map(|configs| configs
                                .map(|c| format!(
                                    "{}b {}x {}-{}Hz",
                                    c.sample_format(),
                                    c.channels(),
                                    c.min_sample_rate().0,
                                    c.min_sample_rate().0,
                                ))
                                .collect::<Vec<String>>()
                                .join("\n"))
                            .unwrap_or("NONE".to_string())
                    ));
                    return None;
                }
            }
        } else {
            self.err(&format!(
                "Sympal: Audio device '{}' has no output configurations",
                device.name().unwrap_or(String::from("ERR"))
            ));
            self.stop();
            return None;
        };
        info!(
            "Symapl selected config {}b {}x {}hz for device {}",
            config.sample_format(),
            config.channels(),
            config.sample_rate().0,
            device.name().unwrap_or(String::from("NONE"))
        );
        // self.device_format.store(config.sample_format(), Ordering::SeqCst);
        self.device_rate.store(config.sample_rate().0, Ordering::SeqCst);
        self.device_channels.store(config.channels().into(), Ordering::SeqCst);
        self.device_format.store(config.sample_format() as u8, Ordering::SeqCst);
        Some((device, config))
    } // }}}
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
            streaming: Arc::new(AtomicBool::new(false)),
            decoder_state: Arc::new(AtomicU8::new(*DecoderState::Empty)),
            samples: Default::default(),
            pos: Arc::new(AtomicUsize::new(0)),
            rate: Arc::new(AtomicU32::new(0)),
            channels: Arc::new(AtomicUsize::new(0)),
            device_rate: Arc::new(AtomicU32::new(0)),
            device_channels: Arc::new(AtomicU32::new(0)),
            device_format: Arc::new(AtomicU8::new(0)),
        }
    }
    fn types(&self) -> Vec<String> {
        let mut result = [
            symphonia::default::formats::OggReader::query(),
            symphonia::default::formats::MkvReader::query(),
            symphonia::default::formats::MpaReader::query(),
            symphonia::default::formats::FlacReader::query(),
            symphonia::default::formats::WavReader::query(),
            symphonia::default::formats::AdtsReader::query(),
            symphonia::default::formats::IsoMp4Reader::query(),
        ]
        .iter()
        .map(|descriptors| {
            descriptors
                .iter()
                .map(|descriptor| descriptor.extensions.iter().map(|extension| extension.to_string()))
        })
        .flatten()
        .flatten()
        .collect::<Vec<String>>();
        result.sort();
        result.dedup();
        result
    }

    fn play(&self) {
        // {{{

        if self.track_get().is_none() {
            return;
        }
        // if already playing then just set pos to 0
        // so far no negative side-effects
        // prevents duplicate streams caused by join store/load by returning
        if !self.join.load(Ordering::Relaxed) {
            self.pos.store(0, Ordering::Relaxed);
            return;
        }

        let vol = self.volume.clone();
        let gain = self.track.lock().unwrap().as_ref().unwrap().gain();

        debug!("Sympal play await stream");
        while self.streaming.load(Ordering::Relaxed) {
            sleep1()
        }
        self.join.store(false, Ordering::Relaxed);
        loop {
            match self.decoder_state.load(Ordering::Relaxed).into() {
                DecoderState::Decoding | DecoderState::Complete => break,
                DecoderState::Error => {
                    self.join.store(true, Ordering::Relaxed);
                    return;
                }
                DecoderState::Empty => sleep1(),
            }
        }

        let Some((device, config)) = self.get_device() else {
            self.stop();
            return;
        };

        let join_thread = self.join.clone();
        let join_data = self.join.clone();
        let streaming = self.streaming.clone();
        let pos = self.pos.clone();
        let decoder_state = self.decoder_state.clone();
        let samples = self.samples.lock().unwrap().clone();
        let channels = self.channels.load(Ordering::Relaxed) as u32;
        let rate = self.rate.load(Ordering::Relaxed);
        let channel_str = self.channel.clone();
        let channel_err = self.channel.clone();
        let join_err = self.join.clone();
        let pos_err = self.pos.clone();
        let device_rate = self.device_rate.load(Ordering::SeqCst);
        let device_format: SampleFormat = unsafe { std::mem::transmute(self.device_format.load(Ordering::SeqCst)) };

        debug!("Sympal play spawn stream");
        thread::Builder::new()
            .name(String::from("SYMPAL Audio Stream"))
            .spawn(move || {
                // if play requested on last pos, reset.
                // basically if you manage to pause it after samples[] ends,
                // this restarts playback instead of playing nothing
                if decoder_state.load(Ordering::Relaxed) == *DecoderState::Complete && samples.read().unwrap().len() == pos.load(Ordering::Relaxed) {
                    pos.store(0, Ordering::Relaxed)
                }
                streaming.store(true, Ordering::Relaxed);
                let stream = device
                    .build_output_stream_raw(
                        &config.config(),
                        device_format,
                        move |ring_buffer: &mut cpal::Data, _: &cpal::OutputCallbackInfo| {
                            let amplitude = gain * f32::from_bits(vol.load(Ordering::Relaxed)).powi(3);
                            let mut start_pos = pos.load(Ordering::Relaxed);
                            start_pos -= start_pos % channels as usize;
                            let mut cur_pos = start_pos;
                            let samples = samples.read().unwrap();

                            macro_rules! typed_stream {
                                ($format:ty) => {{
                                    let samples_iter = samples.get(start_pos..).unwrap().iter().map(|s| {
                                        cur_pos += 1;
                                        s.to_sample::<$format>()
                                    });
                                    let ring_slice = ring_buffer.as_slice_mut::<$format>().unwrap();
                                    if rate == device_rate {
                                        ring_slice
                                            .iter_mut()
                                            .zip(samples_iter)
                                            .for_each(|(sink_sample, signal_sample)| *sink_sample = signal_sample.mul_amp(amplitude.into()));
                                    } else {
                                        let mut signal = dasp::signal::from_interleaved_samples_iter::<_, [_; 2]>(samples_iter);
                                        let a = signal.next();
                                        let b = signal.next();
                                        let interp = dasp::interpolate::linear::Linear::new(a, b);
                                        let frames = signal.from_hz_to_hz(interp, rate as f64, device_rate as f64);
                                        for (sink_sample, signal_sample) in ring_slice.iter_mut().zip(frames.into_interleaved_samples().into_iter()) {
                                            *sink_sample = signal_sample.mul_amp(amplitude.into());
                                        }
                                    }
                                }};
                            }

                            match ring_buffer.sample_format() {
                                SampleFormat::I8 => typed_stream!(i8),
                                SampleFormat::I16 => typed_stream!(i16),
                                SampleFormat::I32 => typed_stream!(i32),
                                SampleFormat::I64 => typed_stream!(i64),
                                SampleFormat::U8 => typed_stream!(u8),
                                SampleFormat::U16 => typed_stream!(u16),
                                SampleFormat::U32 => typed_stream!(u32),
                                SampleFormat::U64 => typed_stream!(u64),
                                SampleFormat::F32 => typed_stream!(f32),
                                SampleFormat::F64 => typed_stream!(f64),
                                _ => {
                                    channel_str
                                        .send(PlayerMessage::Error(format!(
                                            "Sympal invalid stream format '{}'",
                                            ring_buffer.sample_format()
                                        )))
                                        .unwrap();
                                    join_data.store(true, Ordering::Relaxed);
                                    pos.store(0, Ordering::Relaxed);
                                    return;
                                }
                            };

                            pos.store(cur_pos, Ordering::Relaxed);
                            if cur_pos >= samples.len() && !join_data.load(Ordering::Relaxed) {
                                join_data.store(true, Ordering::Relaxed);
                                channel_str.send(PlayerMessage::Request).unwrap();
                            }
                            if (start_pos as f32 / (rate * channels) as f32).floor()
                                < (cur_pos as f32 / (rate * channels) as f32).floor()
                                // only clock if seekable
                                && decoder_state.load(Ordering::Relaxed) == *DecoderState::Complete
                            {
                                channel_str.send(PlayerMessage::Clock).unwrap();
                            }
                        },
                        move |err| {
                            // TODO can some of these actually be handled?
                            join_err.store(true, Ordering::Relaxed);
                            channel_err
                                .send(PlayerMessage::Error(format!("SYMPAL Audio Stream Error:\n{}", err)))
                                .unwrap();
                            pos_err.store(0, Ordering::Relaxed);
                        },
                        None, // None=blocking, Some(Duration)=timeout
                    )
                    .expect("Could not initialize audio stream");
                stream.play().unwrap();
                while !join_thread.load(Ordering::Relaxed) {
                    sleep1()
                }
                streaming.store(false, Ordering::Relaxed);
            })
            .unwrap();
        debug!("Sympal play end");
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
        Some(self.decoder_state.load(Ordering::Relaxed) == *DecoderState::Complete)
    }
    fn times(&self) -> Option<(Duration, Duration)> {
        match self.seekable().unwrap() {
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
    fn seek(&self, time: Duration) {
        if self.seekable() == Some(true) {
            self.pos.store(
                (time.as_secs_f32() * self.rate.load(Ordering::Relaxed) as f32) as usize * self.channels.load(Ordering::Relaxed),
                Ordering::Release,
            );
            self.channel.send(PlayerMessage::Clock).unwrap();
        }
    }
    fn waveform(&self, count: usize) -> Option<Vec<f32>> {
        if self.seekable() == Some(true) {
            let samples = self.samples.lock().unwrap().clone();
            let read = samples.read().unwrap();

            // Interleaved channels + rate over 20KHz for 'resolution' right?
            let res_base = self.channels.load(Ordering::Relaxed) * (self.rate.load(Ordering::Relaxed) as usize / 20_000);
            // Cap sample amount @ 256 total. Causes timeH with Reptile to be about 1/2 of timeP
            // Seems appropriate.
            let res_adapt = (read.len() / res_base / count).min(256);

            let mut result = Vec::with_capacity(count);
            for chunk in read.chunks_exact(read.len() / count) {
                if chunk.len() >= res_base * res_adapt {
                    let padding = (chunk.len() - res_base * res_adapt) / 2;
                    result.push(
                        chunk[padding..res_base * res_adapt + padding]
                            .iter()
                            .map(|n| n.abs() as f32 / i16::MAX as f32)
                            .sum::<f32>()
                            / res_adapt as f32,
                        // maybe this should be base & adapt?
                        // Imma be real, I don't know what I'm doing but it seems to work okay-ish
                        // Maybe it should just hardcode @ 256 then half until
                        // chunk size is small enough
                        // TODO test this on a track that sweeps 0.0 -> 1.0
                    )
                }
            }
            Some(result)
        } else {
            None
        }
    }
    fn volume_set(&self, volume: f32) {
        self.volume.store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed)
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
                let mss = MediaSourceStream::new(Box::new(File::open(track.path().clone()).unwrap()), Default::default());

                let mut fr = symphonia::default::get_probe()
                    .format(
                        Hint::new().with_extension(track.path().extension().unwrap().to_str().unwrap()),
                        mss,
                        &Default::default(),
                        &Default::default(),
                    )
                    .expect("SYMPAL Probe result return ERR")
                    .format;

                let decoder = if let Some(decoder) = fr
                    .default_track()
                    .map(|t| symphonia::default::get_codecs().make(&t.codec_params, &Default::default()).ok())
                    .flatten()
                    .filter(|d| d.codec_params().channels.is_some())
                {
                    Mutex::new(decoder)
                } else {
                    let mut tracks = fr.tracks().into_iter();
                    loop {
                        match tracks.next() {
                            Some(track) => match symphonia::default::get_codecs().make(&track.codec_params, &Default::default()) {
                                Ok(decoder) => break Mutex::new(decoder),
                                Err(_e) => continue,
                            },
                            None => {
                                self.err(format!(
                                    "SYMPAL could not decode any tracks\nFile: {}{}",
                                    track.path().file_name().unwrap().to_string_lossy(),
                                    fr.tracks().into_iter().fold(String::new(), |acc, t| {
                                        acc + "\n"
                                            + &format!(
                                                "Track: {}\n  Channels: {:?}\n  Rate: {:?}\n  Format: {:?}",
                                                t.id, t.codec_params.channels, t.codec_params.sample_rate, t.codec_params.sample_format,
                                            )
                                    })
                                ));
                                std::mem::swap(guard, &mut None);
                                return None;
                            }
                        }
                    }
                };

                while self.streaming.load(Ordering::Relaxed) {
                    sleep1()
                }

                self.decoder_state.store(*DecoderState::Empty, Ordering::Relaxed);
                self.pos.store(0, Ordering::Relaxed);

                let channel = self.channel.clone();
                let channels = self.channels.clone();
                let decoder_state = self.decoder_state.clone();
                let rate = self.rate.clone();
                let samples = Arc::downgrade(&self.samples.lock().unwrap());

                thread::Builder::new()
                    .name(String::from("SYMPAL Decoder"))
                    .spawn(move || {
                        let begin = Instant::now();
                        let mut init = true;
                        while let Ok(packet) = fr.next_packet() {
                            if packet.dur() < 1 {
                                // 0 length packets are possible I guess
                                continue;
                            }
                            if let Some(samples) = samples.upgrade() {
                                let hook = panic::take_hook();
                                panic::set_hook(Box::new(|_| {}));
                                let packet_decode_result = panic::catch_unwind(|| {
                                    decoder
                                        .lock()
                                        .unwrap()
                                        .decode(&packet)
                                        .inspect_err(|e| {
                                            channel
                                                .send(PlayerMessage::Error(format!("Symphonia could not decode packet:\n{}", e)))
                                                .unwrap()
                                        })
                                        .ok()
                                        .map(|ab| {
                                            if init {
                                                let new_rate = ab.spec().rate;
                                                let new_channels = ab.spec().channels.count();
                                                rate.store(new_rate, Ordering::Relaxed);
                                                channels.store(new_channels, Ordering::Relaxed);

                                                // allocate 2 minutes' worth of initial space
                                                *samples.write().unwrap() = Vec::with_capacity(new_rate as usize * 120 * new_channels);
                                            }
                                            let mut sb = SampleBuffer::<i16>::new(packet.dur, *ab.spec());
                                            sb.copy_interleaved_ref(ab);
                                            samples.write().unwrap().extend_from_slice(sb.samples());
                                            decoder_state.store(*DecoderState::Decoding, Ordering::Relaxed)
                                        })
                                })
                                .inspect_err(|_e| {
                                    channel
                                        .send(PlayerMessage::Error("Symphonia panicked while decoding track".to_string()))
                                        .unwrap()
                                })
                                .ok()
                                .flatten();
                                panic::set_hook(hook);
                                if packet_decode_result == None {
                                    decoder_state.store(*DecoderState::Error, Ordering::Relaxed);
                                    return;
                                }
                                init = false;
                            } else {
                                return;
                            }
                        }
                        bench!("Track fully decoded in {}ms", (Instant::now() - begin).as_millis());
                        if let Some(samples) = samples.upgrade() {
                            decoder_state.store(*DecoderState::Complete, Ordering::Relaxed);
                            samples.write().unwrap().shrink_to_fit();
                            channel.send(PlayerMessage::Seekable).unwrap()
                        }
                    })
                    .unwrap();
            } else {
                self.decoder_state.store(*DecoderState::Empty, Ordering::Relaxed);
                self.pos.store(0, Ordering::Relaxed);
            }
        }
        track
    }
    // }}}
}
