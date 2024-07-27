#![warn(missing_docs)]

use super::{Player, PlayerMessage};
use crate::library::Track;
use crate::logging::*;
use crate::try_block;

use std::error::Error;
use std::fs::File;
use std::mem::{swap, transmute};
use std::ops::Deref;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, AtomicUsize, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex, RwLock};
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
    thread::sleep(Duration::from_nanos(1))
}

macro_rules! wait_on {
    ($cond:expr) => {
        while $cond {
            sleep1()
        }
    };
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum DecoderState {
    Empty,
    Init,
    Decoding,
    Complete,
    Error,
}

impl From<u8> for DecoderState {
    fn from(value: u8) -> Self {
        unsafe { transmute(value) }
    }
}

impl Deref for DecoderState {
    type Target = u8;
    fn deref(&self) -> &Self::Target {
        unsafe { transmute(self) }
    }
}

pub struct Backend {
    track: Mutex<Option<Arc<Track>>>,
    volume: Arc<AtomicU32>,
    channel: SyncSender<PlayerMessage>,
    join_stream: Arc<AtomicBool>,
    join_decode: Arc<AtomicBool>,
    streaming: Arc<AtomicBool>,
    decoder_state: Arc<AtomicU8>,
    // TODO: dynamic typing
    samples: Arc<RwLock<Vec<i16>>>,
    pos: Arc<AtomicUsize>,
    rate: Arc<AtomicU32>,
    channels: Arc<AtomicUsize>,
    device_rate: Arc<AtomicU32>,
    device_channels: Arc<AtomicU32>,
    device_format: Arc<AtomicU8>,
}

impl Backend {
    fn get_device(&self) -> Result<(cpal::Device, cpal::SupportedStreamConfig), Box<dyn Error>> {
        // {{{
        #[cfg(feature = "jack")]
        let requested_id = Some(cpal::HostId::Jack);

        #[cfg(not(feature = "jack"))]
        let requested_id = None;

        let format = SampleFormat::I16;
        let rate = self.rate.load(Ordering::Relaxed);
        let channels = self.channels.load(Ordering::Relaxed) as u16;

        debug!("Sympal play acquire device");

        let device = match requested_id {
            Some(id) => cpal::host_from_id(id)?.default_output_device(),
            None => cpal::default_host().default_output_device(),
        }
        .ok_or("Could not find a output device.")?;

        let mut state = 0b000u8;
        let relax_format = 0b001u8;
        let relax_rate = 0b010u8;
        let _relax_channels = 0b100u8;

        debug!("Sympal play acquire config");
        let mut configs = device.supported_output_configs()?;
        let config = {
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
                    configs = device.supported_output_configs()?;
                } else {
                    return Err(format!(
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
                                    c.max_sample_rate().0,
                                ))
                                .collect::<Vec<String>>()
                                .join("\n"))
                            .unwrap_or("NONE".to_string())
                    )
                    .into());
                }
            }
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
        Ok((device, config))
    } // }}}

    fn play_internal<'a>(&'a self) -> Result<(), Box<dyn Error + 'a>> {
        // {{{

        let gain = if let Some(track) = self.track_get() {
            track.gain()
        } else {
            return Ok(());
        };

        // if already playing then just set pos to 0
        // so far no negative side-effects
        // prevents duplicate streams caused by join store/load by returning
        if !self.join_stream.load(Ordering::Relaxed) {
            self.pos.store(0, Ordering::Relaxed);
            return Ok(());
        }

        debug!("Sympal play await stream");
        wait_on!(self.streaming.load(Ordering::Relaxed));
        self.join_stream.store(false, Ordering::Relaxed);
        loop {
            match self.decoder_state.load(Ordering::Relaxed).into() {
                DecoderState::Decoding | DecoderState::Complete => break,
                DecoderState::Error => {
                    self.join_stream.store(true, Ordering::Relaxed);
                    return Ok(());
                }
                DecoderState::Empty | DecoderState::Init => sleep1(),
            }
        }

        let vol = self.volume.clone();
        let (device, config) = self.get_device()?;
        let join_thread = self.join_stream.clone();
        let join_data = self.join_stream.clone();
        let streaming = self.streaming.clone();
        let pos = self.pos.clone();
        let decoder_state = self.decoder_state.clone();
        let samples = self.samples.clone();
        let channels = self.channels.load(Ordering::Relaxed) as u32;
        let rate = self.rate.load(Ordering::Relaxed);
        let channel_str = self.channel.clone();
        let channel_thread = self.channel.clone();
        let channel_err = self.channel.clone();
        let join_err = self.join_stream.clone();
        let pos_err = self.pos.clone();
        let device_rate = self.device_rate.load(Ordering::SeqCst);
        let device_format: SampleFormat = unsafe { transmute(self.device_format.load(Ordering::SeqCst)) };

        // if play requested on last pos, reset.
        // basically if you manage to pause it after samples[] ends,
        // this restarts playback instead of playing nothing
        if self.decoder_state.load(Ordering::Relaxed) == *DecoderState::Complete && self.samples.read()?.len() == self.pos.load(Ordering::Relaxed) {
            self.pos.store(0, Ordering::Relaxed)
        }

        debug!("Sympal play spawn stream");
        thread::Builder::new().name(String::from("SYMPAL Audio Stream")).spawn(move || {
            streaming.store(true, Ordering::Relaxed);
            let stream = device.build_output_stream_raw(
                &config.config(),
                device_format,
                move |ring_buffer: &mut cpal::Data, _: &cpal::OutputCallbackInfo| {
                    let result = try_block!({
                        let amplitude = gain * f32::from_bits(vol.load(Ordering::Relaxed)).powi(3);
                        let mut start_pos = pos.load(Ordering::Relaxed);
                        start_pos -= start_pos % channels as usize;
                        let mut cur_pos = start_pos;
                        let samples = samples.read()?;

                        macro_rules! typed_stream {
                            ($format:ty) => {{
                                let samples_iter = samples.get(start_pos..).ok_or("Sample pos out of bounds")?.iter().map(|s| {
                                    cur_pos += 1;
                                    s.to_sample::<$format>()
                                });
                                let ring_slice = ring_buffer.as_slice_mut::<$format>().ok_or("Ring buffer has no slice")?;
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
                            // SampleFormat is non-exhaustive
                            _ => {
                                join_data.store(true, Ordering::Relaxed);
                                pos.store(0, Ordering::Relaxed);
                                return Err(format!("Sympal unsupported stream format '{}'", ring_buffer.sample_format()).into());
                            }
                        };

                        pos.store(cur_pos, Ordering::Relaxed);
                        if cur_pos >= samples.len() && !join_data.load(Ordering::Relaxed) {
                            join_data.store(true, Ordering::Relaxed);
                            channel_str.send(PlayerMessage::Request)?;
                        }
                        // only clock if seekable
                        if (start_pos as f32 / (rate * channels) as f32).floor() < (cur_pos as f32 / (rate * channels) as f32).floor()
                            && decoder_state.load(Ordering::Relaxed) == *DecoderState::Complete
                        {
                            channel_str.send(PlayerMessage::Clock)?;
                        }
                        Ok(())
                    });
                    match result {
                        Ok(()) => (),
                        Err(e) => {
                            let _ = channel_str.send(PlayerMessage::Error(format!("Error occured while decoding the track:\n  {}", e)));
                        }
                    };
                },
                move |err| {
                    // TODO can some of these actually be handled?
                    join_err.store(true, Ordering::Relaxed);
                    let _ = channel_err.send(PlayerMessage::Error(format!("SYMPAL Audio Stream Error:\n{}", err)));
                    pos_err.store(0, Ordering::Relaxed);
                },
                None, // None=blocking, Some(Duration)=timeout
            );
            if let Ok(stream) = stream {
                let Ok(()) = stream.play() else {
                    let _ = channel_thread.send(PlayerMessage::Error("CPAL Audio stream could not play".into()));
                    streaming.store(false, Ordering::Relaxed);
                    join_thread.store(true, Ordering::Relaxed);
                    return;
                };
                wait_on!(!join_thread.load(Ordering::Relaxed));
                streaming.store(false, Ordering::Relaxed);
            }
        })?;
        debug!("Sympal play end");
        Ok(())
    }
    // }}}

    fn track_set_internal<'a>(&'a self, mut track: Option<Arc<Track>>) -> Result<Option<Arc<Track>>, Box<dyn Error + 'a>> {
        // {{{
        self.join_stream.store(true, Ordering::Relaxed);
        self.join_decode.store(true, Ordering::Relaxed);
        let guard: &mut Option<Arc<Track>> = &mut *self.track.lock()?;
        if *guard == track {
            self.stop();
            return Ok(track);
        }
        swap(guard, &mut track);

        if let Some(track) = guard.as_ref() {
            let mss = MediaSourceStream::new(Box::new(File::open(track.path().clone())?), Default::default());

            let mut fr = symphonia::default::get_probe()
                .format(
                    Hint::new().with_extension(
                        track
                            .path()
                            .extension()
                            .ok_or("Could not find track file extension")?
                            .to_str()
                            .ok_or("Extension characters are invalid")?,
                    ),
                    mss,
                    &Default::default(),
                    &Default::default(),
                )?
                .format;

            let decoder = if let Some(decoder) = fr
                .default_track()
                .map(|t| symphonia::default::get_codecs().make(&t.codec_params, &Default::default()).ok())
                .flatten()
                .filter(|d| d.codec_params().channels.is_some())
            {
                decoder
            } else {
                let mut tracks = fr.tracks().into_iter();
                loop {
                    match tracks.next() {
                        Some(track) => match symphonia::default::get_codecs().make(&track.codec_params, &Default::default()) {
                            Ok(decoder) => break decoder,
                            Err(_e) => continue,
                        },
                        None => {
                            let err = format!(
                                "SYMPAL could not decode any tracks\nFile: {}{}",
                                track.path().file_name().ok_or("Track has no file name")?.to_string_lossy(),
                                fr.tracks().into_iter().fold(String::new(), |acc, t| {
                                    acc + "\n"
                                        + &format!(
                                            "Track: {}\n  Channels: {:?}\n  Rate: {:?}\n  Format: {:?}",
                                            t.id, t.codec_params.channels, t.codec_params.sample_rate, t.codec_params.sample_format,
                                        )
                                })
                            );
                            swap(guard, &mut None);
                            return Err(err.into());
                        }
                    }
                }
            };

            wait_on!(self.streaming.load(Ordering::Relaxed));
            wait_on!(match self.decoder_state.load(Ordering::Relaxed).into() {
                DecoderState::Decoding | DecoderState::Init => true,
                _ => false,
            });

            self.decoder_state.store(*DecoderState::Empty, Ordering::Relaxed);
            self.join_decode.store(false, Ordering::Relaxed);
            self.pos.store(0, Ordering::Relaxed);

            let channel = self.channel.clone();
            let channel_er = self.channel.clone();
            let channels = self.channels.clone();
            let decoder_state = self.decoder_state.clone();
            let decoder_state_er = self.decoder_state.clone();
            let join_decode = self.join_decode.clone();
            let rate = self.rate.clone();
            let samples = self.samples.clone();

            thread::Builder::new().name(String::from("SYMPAL Decoder")).spawn(move || {
                let result = try_block!({
                    let mut decoder = decoder; // assign in closure for FnOnce()
                    let begin = Instant::now();
                    decoder_state.store(*DecoderState::Init, Ordering::Relaxed);
                    while let Ok(packet) = fr.next_packet() {
                        // A new decoder is waiting to start
                        if join_decode.load(Ordering::Relaxed) {
                            decoder_state.store(*DecoderState::Empty, Ordering::Relaxed);
                            break;
                        }
                        // 0 length packets are possible I guess
                        if packet.dur() < 1 {
                            continue;
                        }
                        let ab = match decoder.decode(&packet) {
                            Err(symphonia::core::errors::Error::IoError(_)) => continue,
                            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
                            r => r?,
                        };

                        // Set channels and rate on first packet
                        // Also allocate 2 minutes of sample storage
                        if decoder_state.load(Ordering::Relaxed) == *DecoderState::Init {
                            let new_rate = ab.spec().rate;
                            let new_channels = ab.spec().channels.count();
                            rate.store(new_rate, Ordering::Relaxed);
                            channels.store(new_channels, Ordering::Relaxed);

                            *samples.write()? = Vec::with_capacity(new_rate as usize * 120 * new_channels);
                            decoder_state.store(*DecoderState::Decoding, Ordering::Relaxed)
                        }

                        let mut sb = SampleBuffer::<i16>::new(packet.dur, *ab.spec());
                        // avoid panic
                        if sb.capacity() < ab.frames() {
                            return Err("SampleBuffer capacity was lower than AudioBuffer frame count!".into());
                        };
                        sb.copy_interleaved_ref(ab);
                        samples.write()?.append(&mut sb.samples_mut().to_vec());
                    }
                    bench!("Track fully decoded in {}ms", begin.elapsed().as_millis());
                    decoder_state.store(*DecoderState::Complete, Ordering::Relaxed);
                    samples.write()?.shrink_to_fit();
                    channel.send(PlayerMessage::Seekable)?;
                    Ok(())
                });
                match result {
                    Ok(()) => (),
                    Err(e) => {
                        decoder_state_er.store(*DecoderState::Error, Ordering::Relaxed);
                        let _ = channel_er.send(PlayerMessage::Error(format!("Error occured while decoding the track:\n  {}", e)));
                    }
                };
            })?;
        } else {
            self.decoder_state.store(*DecoderState::Empty, Ordering::Relaxed);
            self.pos.store(0, Ordering::Relaxed);
        }
        Ok(track)
    }
    // }}}
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
            join_stream: Arc::new(AtomicBool::new(true)),
            join_decode: Arc::new(AtomicBool::new(true)),
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
        if let Err(e) = self.play_internal() {
            self.stop();
            let _ = self.channel.send(PlayerMessage::Error(
                (format!("Error occured when attempting to play the stream:\n  {}", e)).to_string(),
            ));
        }
    }
    // }}}

    fn stop(&self) {
        self.pause();
        self.pos.store(0, Ordering::Relaxed);
    }
    fn pause(&self) {
        self.join_stream.store(true, Ordering::Relaxed);
    }
    fn playing(&self) -> bool {
        !self.join_stream.load(Ordering::Relaxed)
    }
    fn paused(&self) -> bool {
        self.pos.load(Ordering::Relaxed) != 0 && self.join_stream.load(Ordering::Relaxed)
    }
    fn seekable(&self) -> Option<bool> {
        Some(self.decoder_state.load(Ordering::Relaxed) == *DecoderState::Complete)
    }
    fn times(&self) -> Option<(Duration, Duration)> {
        if let Ok(samples) = self.samples.read() {
            // unwrap as it's hardcoded to Some
            match self.seekable().unwrap() {
                true => Some((
                    Duration::from_secs_f64(
                        (self.pos.load(Ordering::Relaxed) as f64)
                            / (self.rate.load(Ordering::Relaxed) as f64)
                            / (self.channels.load(Ordering::Relaxed) as f64),
                    ),
                    Duration::from_secs_f64(
                        (samples.len() as f64) / (self.rate.load(Ordering::Relaxed) as f64) / (self.channels.load(Ordering::Relaxed) as f64),
                    ),
                )),
                false => None,
            }
        } else {
            None
        }
    }
    fn seek(&self, time: Duration) {
        if self.seekable() == Some(true) {
            self.pos.store(
                (time.as_secs_f32() * self.rate.load(Ordering::Relaxed) as f32) as usize * self.channels.load(Ordering::Relaxed),
                Ordering::Release,
            );
            let _ = self.channel.send(PlayerMessage::Clock);
        }
    }
    fn waveform(&self, count: usize) -> Option<Vec<f32>> {
        if self.seekable() == Some(true) {
            let Ok(reader) = self.samples.read() else { return None };

            // Interleaved channels + rate over 20KHz for 'resolution' right?
            let sample_size = self.channels.load(Ordering::Relaxed) * (self.rate.load(Ordering::Relaxed) as usize / 20_000);
            // Cap samples per item to 256 for performance
            let subsamples = (reader.len() / sample_size / count).clamp(1, 256);

            Some(
                reader
                    .chunks_exact((reader.len() / count).max(1))
                    .map(|chunk| {
                        chunk
                            .iter()
                            .step_by((chunk.len().div_ceil(subsamples)).max(1))
                            .map(|n| (*n as f32).abs() / i16::MAX as f32)
                            .sum::<f32>()
                            / subsamples as f32
                    })
                    .collect(),
            )
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
        self.track.lock().ok()?.clone()
    }
    fn track_set(&self, track: Option<Arc<Track>>) -> Option<Arc<Track>> {
        self.track_set_internal(track).map_or_else(
            |e| {
                let _ = self.channel.send(PlayerMessage::Error(
                    (format!("Error occured when attempting to set the track:\n  {}", e)).to_string(),
                ));
                None
            },
            |b| b,
        )
    }
}
