//! The voice audio engine, native-mod's `AudioEngine` + `RemoteStream` + `MicCleaner` in Rust:
//!
//! - the microphone (cpal) is resampled to 48 kHz mono, cleaned by WebRTC's audio processing (sonora: echo
//!   cancellation against what the speakers play, noise suppression, a high-pass filter), gated (push to talk / voice
//!   activation), encoded with Opus (opus-rs, pure Rust: 32 kbps, voice, in-band FEC) and sent in 20 ms frames;
//! - every speaker has a small jitter buffer in front of an Opus decoder (loss concealment and FEC); their voices are
//!   mixed with the voice volume and each player's own volume, with the ring tone and the speaker test chime, and
//!   played (cpal), and the mix is what the echo canceller removes from the microphone.
//!
//! One thread, "Native voice audio", owns the devices and the codecs; the cpal callbacks only move samples.

use super::voice;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

pub const RATE: u32 = 48_000;
/// 20 ms at 48 kHz: one Opus frame.
pub const FRAME: usize = 960;
/// 10 ms: one frame of the audio processing.
const APM_FRAME: usize = 480;
/// Flag of the last frame of a stretch of speech.
pub const END: u8 = 1;
/// Voice activation stays open this many frames after the voice drops under the level (300 ms).
const HANGOVER: u32 = 15;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// ── Opus ──────────────────────────────────────────────────────────────────

/// A 48 kHz mono Opus encoder set up like native-mod's (VoIP, 32 kbps, complexity 5, FEC for 10 % loss).
pub struct Encoder(opus_rs::OpusEncoder);

impl Encoder {
    pub fn new() -> Option<Encoder> {
        let mut e = opus_rs::OpusEncoder::new(RATE as i32, 1, opus_rs::Application::Voip).ok()?;
        e.bitrate_bps = 32_000;
        e.complexity = 5;
        e.use_inband_fec = true;
        e.packet_loss_perc = 10;
        e.set_signal(opus_rs::Signal::Voice);
        Some(Encoder(e))
    }

    /// One 20 ms frame into `out`; the packet's length.
    pub fn encode(&mut self, pcm: &[i16], out: &mut [u8]) -> Option<usize> {
        if pcm.len() != FRAME {
            return None;
        }
        self.0.encode_i16(pcm, FRAME, out).ok().filter(|n| *n > 0)
    }
}

pub struct Decoder {
    opus: opus_rs::OpusDecoder,
    out: Vec<f32>,
    /// The last good frame, faded out when a packet is missing and the codec cannot conceal it.
    last: [i16; FRAME],
}

impl Decoder {
    pub fn new() -> Option<Decoder> {
        Some(Decoder { opus: opus_rs::OpusDecoder::new(RATE as i32, 1).ok()?, out: vec![0.0; FRAME], last: [0; FRAME] })
    }

    /// 20 ms of audio from `packet` (`fec`: rebuild the frame before it from its redundancy), or, with no packet,
    /// a concealed frame. False when nothing usable came out.
    pub fn decode(&mut self, packet: Option<&[u8]>, fec: bool, pcm: &mut [i16; FRAME]) -> bool {
        let got = match (packet, fec) {
            (Some(p), true) => self.opus.decode_fec(p, FRAME, &mut self.out),
            (Some(p), false) => self.opus.decode(p, FRAME, &mut self.out),
            (None, _) => Err("lost"), // concealed below (an empty packet is not something to hand the decoder)
        };
        match got {
            Ok(n) if n > 0 => {
                for (o, v) in pcm.iter_mut().zip(&self.out) {
                    *o = (v.clamp(-1.0, 1.0) * 32767.0) as i16;
                }
                if packet.is_some() && !fec {
                    self.last = *pcm;
                }
                true
            }
            _ if packet.is_none() => {
                // concealment by hand: the last frame, quieter each time, instead of a click of silence
                for v in &mut self.last {
                    *v /= 2;
                }
                *pcm = self.last;
                true
            }
            _ => false,
        }
    }
}

// ── one speaker ───────────────────────────────────────────────────────────

const START_FRAMES: usize = 2;
const MAX_FRAMES: usize = 12;
const MAX_MISSES: u32 = 5;
const START_WAIT_MS: u128 = 60;

/// One speaker: a 40 ms jitter buffer in front of an Opus decoder, with loss concealment and FEC (`RemoteStream`).
pub struct Stream {
    buf: BTreeMap<u32, Vec<u8>>,
    decoder: Decoder,
    pcm: [i16; FRAME],
    next: u32,
    misses: u32,
    end: Option<u32>,
    playing: bool,
    first_at: Instant,
    pub level: f32,
    pub last_packet: Option<Instant>,
}

impl Stream {
    pub fn new() -> Option<Stream> {
        Some(Stream {
            buf: BTreeMap::new(),
            decoder: Decoder::new()?,
            pcm: [0; FRAME],
            next: 0,
            misses: 0,
            end: None,
            playing: false,
            first_at: Instant::now(),
            level: 0.0,
            last_packet: None,
        })
    }

    pub fn push(&mut self, seq: u32, flags: u8, opus: Vec<u8>) {
        if self.playing && seq < self.next {
            return; // too late
        }
        if self.buf.is_empty() && !self.playing {
            self.first_at = Instant::now();
        }
        self.buf.insert(seq, opus);
        if flags & END != 0 {
            self.end = Some(seq);
        }
        self.last_packet = Some(Instant::now());
        while self.buf.len() > MAX_FRAMES {
            self.buf.pop_first(); // fell behind: catch up instead of piling up delay
            if let Some((&k, _)) = self.buf.first_key_value() {
                self.next = k;
            }
        }
    }

    /// The next 20 ms, or None while silent.
    pub fn pull(&mut self) -> Option<&[i16; FRAME]> {
        if !self.playing {
            let waiting = self.buf.len() < START_FRAMES && self.first_at.elapsed().as_millis() < START_WAIT_MS && self.end.is_none();
            if self.buf.is_empty() || waiting {
                return None;
            }
            self.playing = true;
            self.misses = 0;
            self.next = *self.buf.keys().next()?;
        }
        let mut pcm = [0i16; FRAME];
        let ok = if let Some(p) = self.buf.remove(&self.next) {
            self.misses = 0;
            self.decoder.decode(Some(&p), false, &mut pcm)
        } else {
            self.misses += 1;
            if self.misses > MAX_MISSES {
                self.stop();
                return None;
            }
            match self.buf.get(&(self.next + 1)) {
                Some(after) => {
                    let after = after.clone();
                    self.decoder.decode(Some(&after), true, &mut pcm)
                }
                None => self.decoder.decode(None, false, &mut pcm),
            }
        };
        if !ok {
            pcm = [0; FRAME];
        }
        if self.end.is_some_and(|e| self.next >= e) {
            self.stop();
        }
        self.next = self.next.wrapping_add(1);
        self.level = rms(&pcm);
        self.pcm = pcm;
        Some(&self.pcm)
    }

    fn stop(&mut self) {
        self.playing = false;
        self.first_at = Instant::now(); // ran dry: build the cushion up again before resuming
        self.end = None;
        self.level = 0.0;
        let next = self.next;
        self.buf.retain(|&k, _| k > next);
    }

    pub fn speaking(&self) -> bool {
        self.last_packet.is_some_and(|t| t.elapsed() < Duration::from_millis(250))
    }
}

pub fn rms(pcm: &[i16]) -> f32 {
    if pcm.is_empty() {
        return 0.0;
    }
    let sum: f64 = pcm.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
    ((sum / pcm.len() as f64).sqrt() / 32768.0) as f32
}

// ── shared with the UI and the network ────────────────────────────────────

/// What the UI reads: levels, who is talking, device problems.
#[derive(Default)]
pub struct Meter {
    pub self_level: f32,
    pub self_speaking: bool,
    pub mic_error: Option<String>,
    pub speaker_error: Option<String>,
    /// Remote sid -> (level, last packet).
    pub remote: HashMap<u32, (f32, Option<Instant>)>,
}

struct Engine {
    streams: Mutex<HashMap<u32, Stream>>,
    meter: Mutex<Meter>,
    running: AtomicBool,
    /// Samples to play once (the speaker test), 48 kHz mono.
    chime: Mutex<VecDeque<f32>>,
}

fn engine() -> &'static Engine {
    static E: OnceLock<Engine> = OnceLock::new();
    E.get_or_init(|| Engine {
        streams: Mutex::new(HashMap::new()),
        meter: Mutex::new(Meter::default()),
        running: AtomicBool::new(false),
        chime: Mutex::new(VecDeque::new()),
    })
}

/// A frame of somebody's voice came in.
pub fn push_audio(from: u32, seq: u32, flags: u8, opus: Vec<u8>) {
    let mut streams = lock(&engine().streams);
    if !streams.contains_key(&from) {
        let Some(s) = Stream::new() else { return };
        streams.insert(from, s);
    }
    if let Some(s) = streams.get_mut(&from) {
        s.push(seq, flags, opus);
    }
}

/// Forgets the speakers that are not in `keep`.
pub fn keep_streams(keep: &[u32]) {
    lock(&engine().streams).retain(|sid, _| keep.contains(sid));
    lock(&engine().meter).remote.retain(|sid, _| keep.contains(sid));
}

pub fn with_meter<R>(f: impl FnOnce(&Meter) -> R) -> R {
    f(&lock(&engine().meter))
}

/// Starts the engine (idempotent): it opens the devices it needs and closes them when nothing needs them.
pub fn start() {
    if engine().running.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawned = std::thread::Builder::new().name("Native voice audio".into()).spawn(|| {
        run();
        engine().running.store(false, Ordering::SeqCst);
    });
    if spawned.is_err() {
        engine().running.store(false, Ordering::SeqCst);
    }
}

pub fn stop() {
    engine().running.store(false, Ordering::SeqCst);
}

/// Queues the speaker test: three rising notes.
pub fn play_chime(gain: f32) {
    let mut c = lock(&engine().chime);
    c.clear();
    c.extend(chime(RATE, gain));
    drop(c);
    start();
}

/// Three rising notes (C5, E5, G5) with a quick fade in and a slow fade out, -1..1 (`Voice.chime`).
pub fn chime(rate: u32, gain: f32) -> Vec<f32> {
    let freq = [523.25f64, 659.25, 783.99];
    let note = (rate * 28 / 100) as usize;
    let mut out = Vec::with_capacity(note * 3);
    for f in freq {
        for i in 0..note {
            let env = (i as f64 / (f64::from(rate) * 0.01)).min(1.0) * (1.0 - i as f64 / note as f64).powf(1.6);
            out.push(((2.0 * std::f64::consts::PI * f * i as f64 / f64::from(rate)).sin() * env * f64::from(gain)) as f32);
        }
    }
    out
}

/// Two beeps in a 3 s cycle (0 - 0.4 s and 0.6 - 1.0 s) with short fades so they do not click (`Voice.beepEnvelope`).
pub fn beep_envelope(sample: usize) -> f64 {
    let fade = 48.0 * 6.0;
    for start in [0usize, 28_800] {
        if sample >= start && sample - start < 19_200 {
            let rel = (sample - start) as f64;
            return (rel.min(19_200.0 - rel) / fade).min(1.0);
        }
    }
    0.0
}

// ── devices ───────────────────────────────────────────────────────────────

struct Devices {
    inputs: Vec<String>,
    outputs: Vec<String>,
    scanned: Option<Instant>,
    scanning: bool,
}

fn devices() -> &'static Mutex<Devices> {
    static D: OnceLock<Mutex<Devices>> = OnceLock::new();
    D.get_or_init(|| Mutex::new(Devices { inputs: Vec::new(), outputs: Vec::new(), scanned: None, scanning: false }))
}

/// Re-reads the devices in the background every few seconds (plugging a headset in shows up).
fn rescan() {
    {
        let mut d = lock(devices());
        if d.scanning || d.scanned.is_some_and(|t| t.elapsed() < Duration::from_secs(4)) {
            return;
        }
        d.scanning = true;
    }
    let _ = std::thread::Builder::new().name("Native voice devices".into()).spawn(|| {
        use cpal::traits::{DeviceTrait, HostTrait};
        let host = cpal::default_host();
        let mut inputs: Vec<String> = Vec::new();
        if let Ok(list) = host.input_devices() {
            for d in list {
                if let Ok(n) = d.name()
                    && !inputs.contains(&n)
                {
                    inputs.push(n);
                }
            }
        }
        let mut outputs: Vec<String> = Vec::new();
        if let Ok(list) = host.output_devices() {
            for d in list {
                if let Ok(n) = d.name()
                    && !outputs.contains(&n)
                {
                    outputs.push(n);
                }
            }
        }
        let mut d = lock(devices());
        d.inputs = inputs;
        d.outputs = outputs;
        d.scanned = Some(Instant::now());
        d.scanning = false;
    });
}

/// The microphones (for the Microphone setting).
pub fn inputs() -> Vec<String> {
    rescan();
    lock(devices()).inputs.clone()
}

/// The speakers and headsets (for the Output device setting).
pub fn outputs() -> Vec<String> {
    rescan();
    lock(devices()).outputs.clone()
}

/// The named device, or the system default when the name is empty or it is not plugged in.
fn find_device(input: bool, name: &str) -> Option<cpal::Device> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    if !name.is_empty() {
        let list = if input { host.input_devices() } else { host.output_devices() };
        if let Ok(list) = list {
            for d in list {
                if d.name().is_ok_and(|n| n == name) {
                    return Some(d);
                }
            }
        }
    }
    if input { host.default_input_device() } else { host.default_output_device() }
}

/// Linear resampling of mono audio, kept across calls.
pub struct Resample {
    step: f64,
    pos: f64,
    last: f32,
}

impl Resample {
    pub fn new(from: u32, to: u32) -> Resample {
        Resample { step: f64::from(from) / f64::from(to.max(1)), pos: 0.0, last: 0.0 }
    }

    pub fn run(&mut self, input: &[f32], out: &mut VecDeque<f32>) {
        if (self.step - 1.0).abs() < 1e-9 {
            out.extend(input.iter().copied());
            return;
        }
        let n = input.len();
        // sample i of the input, -1 being the last one of the previous call
        let at = |i: isize, last: f32| if i < 0 { last } else { input[i as usize] };
        while n > 0 && self.pos < n as f64 - 1.0 + 1e-9 {
            let i = self.pos.floor() as isize - 1;
            let t = (self.pos - self.pos.floor()) as f32;
            let (a, b) = (at(i, self.last), at(i + 1, self.last));
            out.push_back(a + (b - a) * t);
            self.pos += self.step;
        }
        if n > 0 {
            self.pos -= n as f64;
            self.last = input[n - 1];
        }
    }
}

/// A stream's samples in any of cpal's formats as f32.
fn to_f32<T: Copy>(data: &[T]) -> Vec<f32>
where
    f32: cpal::FromSample<T>,
{
    data.iter().map(|s| <f32 as cpal::FromSample<T>>::from_sample_(*s)).collect()
}

/// -1..1 as a stream sample.
fn from_f32<T>(v: f32) -> T
where
    T: cpal::FromSample<f32>,
{
    <T as cpal::FromSample<f32>>::from_sample_(v.clamp(-1.0, 1.0))
}

/// The microphone: cpal pushes 48 kHz mono samples into `ring`.
fn open_input(name: &str, ring: Arc<Mutex<VecDeque<f32>>>) -> Result<cpal::Stream, String> {
    use cpal::SampleFormat;
    use cpal::traits::{DeviceTrait, StreamTrait};
    let device = find_device(true, name).ok_or("No microphone found.")?;
    let supported = device.default_input_config().map_err(|e| format!("The microphone can't be opened: {e}"))?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let (rate, channels) = (config.sample_rate.0, usize::from(config.channels.max(1)));
    let mut resample = Resample::new(rate, RATE);
    let mut push = move |samples: Vec<f32>| {
        let mono: Vec<f32> = samples.chunks(channels).map(|f| f.iter().sum::<f32>() / f.len() as f32).collect();
        let mut r = lock(&ring);
        resample.run(&mono, &mut r);
        let excess = r.len().saturating_sub(RATE as usize / 5); // never more than 200 ms behind
        r.drain(..excess);
    };
    let err = |e: cpal::StreamError| log::debug!("Native voice: microphone: {e}");
    let stream = match format {
        SampleFormat::F32 => device.build_input_stream(&config, move |d: &[f32], _| push(d.to_vec()), err, None),
        SampleFormat::I16 => device.build_input_stream(&config, move |d: &[i16], _| push(to_f32(d)), err, None),
        SampleFormat::U16 => device.build_input_stream(&config, move |d: &[u16], _| push(to_f32(d)), err, None),
        SampleFormat::I32 => device.build_input_stream(&config, move |d: &[i32], _| push(to_f32(d)), err, None),
        other => return Err(format!("The microphone's format ({other:?}) isn't supported.")),
    }
    .map_err(|e| format!("The microphone can't be opened: {e}"))?;
    stream.play().map_err(|e| format!("The microphone can't be opened: {e}"))?;
    Ok(stream)
}

/// The speakers: cpal pulls 48 kHz mono samples from `ring`, resampled and copied to every channel.
fn open_output(name: &str, ring: Arc<Mutex<VecDeque<f32>>>) -> Result<cpal::Stream, String> {
    use cpal::SampleFormat;
    use cpal::traits::{DeviceTrait, StreamTrait};
    let device = find_device(false, name).ok_or("No speakers found.")?;
    let supported = device.default_output_config().map_err(|e| format!("The speakers can't be opened: {e}"))?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let (rate, channels) = (config.sample_rate.0, usize::from(config.channels.max(1)));
    let mut resample = Resample::new(RATE, rate);
    let mut pending: VecDeque<f32> = VecDeque::new();
    let mut fill = move |frames: usize| -> Vec<f32> {
        {
            let mut r = lock(&ring);
            while pending.len() < frames && !r.is_empty() {
                let n = r.len().min(480);
                let take: Vec<f32> = r.drain(..n).collect();
                resample.run(&take, &mut pending);
            }
        }
        let mut out = Vec::with_capacity(frames * channels);
        for _ in 0..frames {
            let v = pending.pop_front().unwrap_or(0.0);
            out.extend(std::iter::repeat_n(v, channels));
        }
        out
    };
    let err = |e: cpal::StreamError| log::debug!("Native voice: speakers: {e}");
    let stream = match format {
        SampleFormat::F32 => device.build_output_stream(
            &config,
            move |d: &mut [f32], _| {
                let samples = fill(d.len() / channels);
                d.copy_from_slice(&samples);
            },
            err,
            None,
        ),
        SampleFormat::I16 => device.build_output_stream(
            &config,
            move |d: &mut [i16], _| {
                let samples = fill(d.len() / channels);
                for (o, v) in d.iter_mut().zip(samples) {
                    *o = from_f32::<i16>(v);
                }
            },
            err,
            None,
        ),
        SampleFormat::U16 => device.build_output_stream(
            &config,
            move |d: &mut [u16], _| {
                let samples = fill(d.len() / channels);
                for (o, v) in d.iter_mut().zip(samples) {
                    *o = from_f32::<u16>(v);
                }
            },
            err,
            None,
        ),
        other => return Err(format!("The speakers' format ({other:?}) isn't supported.")),
    }
    .map_err(|e| format!("The speakers can't be opened: {e}"))?;
    stream.play().map_err(|e| format!("The speakers can't be opened: {e}"))?;
    Ok(stream)
}

// ── cleaning ──────────────────────────────────────────────────────────────

/// WebRTC's audio processing for the microphone: the high-pass filter always, echo cancellation and noise
/// suppression as the player chose.
fn processor(noise: bool, echo: bool) -> sonora::AudioProcessing {
    use sonora::config::{EchoCanceller, HighPassFilter, NoiseSuppression, NoiseSuppressionLevel};
    let config = sonora::Config {
        high_pass_filter: Some(HighPassFilter::default()),
        echo_canceller: echo.then(EchoCanceller::default),
        noise_suppression: noise.then(|| NoiseSuppression { level: NoiseSuppressionLevel::High, ..Default::default() }),
        ..Default::default()
    };
    sonora::AudioProcessing::builder()
        .config(config)
        .capture_config(sonora::StreamConfig::new(RATE, 1))
        .render_config(sonora::StreamConfig::new(RATE, 1))
        .build()
}

// ── the loop ──────────────────────────────────────────────────────────────

/// What is on, read from `voice` once per 10 ms.
pub struct Wants {
    pub mic: bool,
    pub speakers: bool,
    pub ring: bool,
    pub mic_device: String,
    pub out_device: String,
    pub mic_gain: f32,
    pub out_volume: f32,
    pub push_to_talk: bool,
    pub ptt_held: bool,
    pub threshold_db: f32,
    pub muted: bool,
    pub deafened: bool,
    pub noise: bool,
    pub echo: bool,
    pub in_call: bool,
}

fn run() {
    let cap: Arc<Mutex<VecDeque<f32>>> = Arc::new(Mutex::new(VecDeque::new()));
    let play: Arc<Mutex<VecDeque<f32>>> = Arc::new(Mutex::new(VecDeque::new()));
    let mut mic: Option<(cpal::Stream, String)> = None;
    let mut out: Option<(cpal::Stream, String)> = None;
    let mut mic_retry = Instant::now();
    let mut out_retry = Instant::now();
    let Some(mut encoder) = Encoder::new() else {
        lock(&engine().meter).mic_error = Some("The voice encoder could not start.".into());
        return;
    };
    let mut apm_flags = (true, true);
    let mut apm = processor(true, true);
    let mut frame: Vec<i16> = Vec::with_capacity(FRAME);
    let mut packet = [0u8; 1 + 1275];
    let (mut hang, mut open) = (0u32, false);
    let mut next_mix = Instant::now();
    let mut ring_pos = 0usize;
    let mut idle_since: Option<Instant> = None;
    while engine().running.load(Ordering::SeqCst) {
        let w = voice::wants();
        let chiming = !lock(&engine().chime).is_empty();
        // Nothing to do for a while: the thread ends (and the devices close).
        if !w.mic && !w.speakers && !w.ring && !chiming {
            mic = None;
            out = None;
            let since = *idle_since.get_or_insert_with(Instant::now);
            if since.elapsed() > Duration::from_secs(5) {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
            continue;
        }
        idle_since = None;

        // devices
        if mic.as_ref().is_some_and(|(_, n)| *n != w.mic_device) || !w.mic {
            mic = None;
            lock(&cap).clear();
        }
        if w.mic && mic.is_none() && Instant::now() >= mic_retry {
            match open_input(&w.mic_device, cap.clone()) {
                Ok(s) => {
                    mic = Some((s, w.mic_device.clone()));
                    lock(&engine().meter).mic_error = None;
                }
                Err(e) => {
                    lock(&engine().meter).mic_error = Some(e);
                    mic_retry = Instant::now() + Duration::from_secs(2);
                }
            }
        }
        let want_out = w.speakers || w.ring || chiming;
        if out.as_ref().is_some_and(|(_, n)| *n != w.out_device) || !want_out {
            out = None;
            lock(&play).clear();
        }
        if want_out && out.is_none() && Instant::now() >= out_retry {
            match open_output(&w.out_device, play.clone()) {
                Ok(s) => {
                    out = Some((s, w.out_device.clone()));
                    lock(&engine().meter).speaker_error = None;
                }
                Err(e) => {
                    lock(&engine().meter).speaker_error = Some(e);
                    out_retry = Instant::now() + Duration::from_secs(2);
                }
            }
        }
        if apm_flags != (w.noise, w.echo) {
            apm_flags = (w.noise, w.echo);
            apm = processor(w.noise, w.echo);
        }

        // speakers: 20 ms at a time on the clock, up to 60 ms ahead
        let now = Instant::now();
        if now > next_mix + Duration::from_millis(200) {
            next_mix = now; // the thread was held up: do not burst to catch up
        }
        while next_mix <= now + Duration::from_millis(40) {
            next_mix += Duration::from_millis(20);
            let mut mix = [0f32; FRAME];
            if w.in_call {
                // read before taking the audio locks: voice never waits on them while holding its own
                let volumes = voice::volumes_by_sid();
                let mut streams = lock(&engine().streams);
                let mut meter = lock(&engine().meter);
                for (sid, s) in streams.iter_mut() {
                    let pulled = s.pull().copied();
                    meter.remote.insert(*sid, (s.level, s.last_packet));
                    let Some(pcm) = pulled else { continue };
                    if w.deafened {
                        continue;
                    }
                    let g = w.out_volume * volumes.get(sid).copied().unwrap_or(1.0) / 32768.0;
                    for (m, v) in mix.iter_mut().zip(pcm.iter()) {
                        *m += f32::from(*v) * g;
                    }
                }
            }
            if w.ring && !w.deafened {
                let g = w.out_volume.min(1.0) * 0.3;
                for m in &mut mix {
                    let at = ring_pos % (RATE as usize * 3);
                    let env = beep_envelope(at);
                    if env > 0.0 {
                        let t = at as f64 / f64::from(RATE);
                        let v = ((2.0 * std::f64::consts::PI * 440.0 * t).sin() + (2.0 * std::f64::consts::PI * 480.0 * t).sin()) * 0.5 * env;
                        *m += v as f32 * g;
                    }
                    ring_pos += 1;
                }
            } else {
                ring_pos = 0;
            }
            {
                let mut c = lock(&engine().chime);
                for m in &mut mix {
                    match c.pop_front() {
                        Some(v) => *m += v,
                        None => break,
                    }
                }
            }
            for m in &mut mix {
                *m = m.clamp(-1.0, 1.0);
            }
            // what plays is what the echo canceller takes out of the microphone
            for half in mix.chunks(APM_FRAME) {
                let mut sink = [0f32; APM_FRAME];
                let _ = apm.process_render_f32(&[half], &mut [&mut sink[..]]);
            }
            if out.is_some() {
                let mut p = lock(&play);
                p.extend(mix.iter().copied());
                let excess = p.len().saturating_sub(RATE as usize / 8); // at most 125 ms queued
                p.drain(..excess);
            }
        }

        // microphone: every 10 ms that came in
        loop {
            let chunk: Vec<f32> = {
                let mut c = lock(&cap);
                if c.len() < APM_FRAME {
                    break;
                }
                c.drain(..APM_FRAME).collect()
            };
            let gained: Vec<f32> = chunk.iter().map(|v| (v * w.mic_gain).clamp(-1.0, 1.0)).collect();
            let mut clean = [0f32; APM_FRAME];
            if apm.process_capture_f32(&[&gained[..]], &mut [&mut clean[..]]).is_err() {
                clean.copy_from_slice(&gained);
            }
            frame.extend(clean.iter().map(|v| (v * 32767.0) as i16));
            if frame.len() < FRAME {
                continue;
            }
            let level = rms(&frame);
            let db = if level <= 1e-9 { -100.0 } else { 20.0 * level.log10() };
            let want = if w.muted || w.deafened {
                false
            } else if w.push_to_talk {
                w.ptt_held
            } else if db >= w.threshold_db {
                hang = HANGOVER;
                true
            } else if hang > 0 {
                hang -= 1;
                true
            } else {
                false
            };
            {
                let mut m = lock(&engine().meter);
                m.self_level = level;
                m.self_speaking = want;
            }
            if w.in_call && (want || open)
                && let Some(n) = encoder.encode(&frame, &mut packet[1..])
            {
                packet[0] = if want { 0 } else { END };
                voice::send_audio(&packet[..n + 1]);
            }
            open = want;
            frame.clear();
        }
        if mic.is_none() {
            let mut m = lock(&engine().meter);
            m.self_level = 0.0;
            m.self_speaking = false;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(at: usize) -> [i16; FRAME] {
        let mut s = [0i16; FRAME];
        for (i, v) in s.iter_mut().enumerate() {
            *v = ((2.0 * std::f64::consts::PI * 300.0 * (at + i) as f64 / 48_000.0).sin() * 8000.0) as i16;
        }
        s
    }

    #[test]
    fn the_java_mods_opus_decodes_here() {
        // a 300 Hz tone encoded by native-mod's Opus (Concentus, the same settings): what a Java player sends
        let packets = [
            "7881a824c35581009c0000090802ff12c9b8cab70571ae98c77bad48f41301b9d717128d30bc8a25cdf363e23f7fd08b10ee9c27349c0b79971d306673a812784db330de9cefe138b7512c0b4653bc80dfb4d9fa76d695cf056572c557f8abecd79a88cbe265774db771d93778259df63f9313e4fc1a27b35f",
            "78c2e324c3549a45ad8aecfed2161dd7fe9efeadcf58e052bf84028d0d450145ed3234e8ff6a21a5c6e0702a52f9bc9e7a8a8b2c09c54745935235aa6159a53efba57aff016de01d2946692aa316375ca38dd86fb23368b685554044909498",
            "78eb4112d1d618a17c93031a4d68d91d4452cbac38b035e39cc405eb7431a5efab801fde41f0fc5ef9369baee6d3cc4a6fbddadbdd3d98df3386a45c1e96d1ab3584a30f0fcdba338d527e0c1ba4",
            "78e51b370e7e05f4680a5d647958d2fb67d436289939fe318b2208959a0b5417351e7bfd61a6c9fa3dedb98d7c0fc2bd4181935cc82c10afc204836d7e0b4c046c434ecd7ef99f82e7296959",
            "78e663970f7cb9b23eb21506675c08ef8521dc56e73136caee00ace8fdbd75c3aa512ba8e5200ecddb13f607409ad99e868f367f28a9d1fb30b17fe4b0fd1ae89bf402edbd35a8530efc69e7aece57",
            "78e663970f7cb9b23d7f841ce573e7ce8e54e8adf122ddc1f3297b650f808a4d6888079a5b4c84f305d5e2921cc59465bf79ab901a448a103271db8ecabfdcc38944fa86b87a2c232ae2d25c904d7358",
        ];
        let unhex = |s: &str| (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect::<Vec<u8>>();
        let mut d = Decoder::new().unwrap();
        let mut pcm = [0i16; FRAME];
        for (i, p) in packets.iter().enumerate() {
            assert!(d.decode(Some(&unhex(p)), false, &mut pcm), "packet {i} decodes");
            if i >= 2 {
                let level = rms(&pcm);
                assert!(level > 0.08 && level < 0.3, "frame {i} at the tone's level: {level}");
                let crossings = pcm.windows(2).filter(|w| (w[0] < 0) != (w[1] < 0)).count();
                assert!((9..=15).contains(&crossings), "frame {i} is the 300 Hz tone: {crossings} zero crossings");
            }
        }
    }

    #[test]
    fn opus_carries_a_voice_through() {
        // one frame sent and one played every 20 ms, as in a call
        let mut enc = Encoder::new().expect("encoder");
        let mut packet = [0u8; 1275];
        let mut s = Stream::new().expect("decoder");
        let mut loudest = 0.0f32;
        for f in 0..20u32 {
            let n = enc.encode(&tone(f as usize * FRAME), &mut packet).expect("a packet");
            assert!(n > 10 && n < 300, "about 32 kbps: {n} bytes");
            s.push(f, 0, packet[..n].to_vec());
            if let Some(pcm) = s.pull()
                && f >= 5
            {
                loudest = loudest.max(rms(pcm));
            }
        }
        assert!(loudest > 0.1, "the tone comes out at its level: {loudest}");
    }

    #[test]
    fn a_lost_frame_is_concealed_and_the_end_stops_playback() {
        let mut enc = Encoder::new().unwrap();
        let mut packet = [0u8; 1275];
        let mut s = Stream::new().unwrap();
        let mut played = 0;
        for f in 0..20u32 {
            let n = enc.encode(&tone(f as usize * FRAME), &mut packet).unwrap();
            if f != 7 {
                s.push(f, if f == 19 { END } else { 0 }, packet[..n].to_vec());
            }
            if s.pull().is_some() {
                played += 1;
            }
        }
        while s.pull().is_some() {
            played += 1;
        }
        assert_eq!(played, 20, "every frame plays, the lost one concealed");
        assert!(s.pull().is_none(), "silent after the end of speech");
    }

    #[test]
    fn falling_behind_drops_old_audio() {
        let mut enc = Encoder::new().unwrap();
        let mut packet = [0u8; 1275];
        let mut s = Stream::new().unwrap();
        for f in 0..60u32 {
            let n = enc.encode(&tone(f as usize * FRAME), &mut packet).unwrap();
            s.push(f, 0, packet[..n].to_vec());
        }
        let mut played = 0;
        while s.pull().is_some() {
            played += 1;
        }
        assert!(played <= MAX_FRAMES + MAX_MISSES as usize + 1, "only the capped buffer plays: {played}");
    }

    #[test]
    fn resampling_keeps_the_length_in_proportion() {
        let mut r = Resample::new(44_100, 48_000);
        let mut out = VecDeque::new();
        for _ in 0..100 {
            r.run(&[0.5f32; 441], &mut out);
        }
        let want = 44_100.0 * 48_000.0 / 44_100.0;
        assert!((out.len() as f64 - want).abs() < 10.0, "{} samples for one second", out.len());
        assert!(out.iter().skip(2).all(|v| (v - 0.5).abs() < 1e-4), "a steady level stays steady");
    }

    #[test]
    fn the_ring_is_two_beeps_every_three_seconds() {
        assert_eq!(beep_envelope(0), 0.0, "starts from silence");
        assert!((beep_envelope(9600) - 1.0).abs() < 1e-9, "full in the middle of the first beep");
        assert_eq!(beep_envelope(24_000), 0.0, "the gap between the beeps");
        assert!(beep_envelope(38_400) > 0.99, "the second beep");
        assert_eq!(beep_envelope(100_000), 0.0, "quiet for the rest of the cycle");
    }

    #[test]
    fn echo_cancellation_removes_what_the_speakers_played() {
        // the microphone hears only the speakers (a 400 Hz tone, half as loud): the processed mic is much quieter
        let mut apm = processor(false, true);
        let mut before = 0.0f32;
        let mut after = 0.0f32;
        for f in 0..400usize {
            let mut far = [0f32; APM_FRAME];
            for (i, v) in far.iter_mut().enumerate() {
                *v = ((2.0 * std::f64::consts::PI * 400.0 * (f * APM_FRAME + i) as f64 / 48_000.0).sin() * 0.3) as f32;
            }
            let mut sink = [0f32; APM_FRAME];
            apm.process_render_f32(&[&far[..]], &mut [&mut sink[..]]).unwrap();
            let near: Vec<f32> = far.iter().map(|v| v * 0.5).collect();
            let mut out = [0f32; APM_FRAME];
            apm.process_capture_f32(&[&near[..]], &mut [&mut out[..]]).unwrap();
            if f >= 300 {
                before += near.iter().map(|v| v * v).sum::<f32>();
                after += out.iter().map(|v| v * v).sum::<f32>();
            }
        }
        assert!(after < before * 0.3, "at least 5 dB of the echo is gone: {}", after / before);
    }
}
