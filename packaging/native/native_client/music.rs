//! Native Music: plays the songs in one music folder shared by every
//! instance (the launcher's `minecraft/music`), with playlists (its
//! sub-folders), shuffle, repeat, seeking and the sound bars. Audio runs on its
//! own thread straight to the speakers, so it keeps playing in menus, between
//! worlds and while the game is paused. UI calls only read snapshots or queue a
//! command.
#![allow(dead_code)]

use crate::native_ui::Image;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{Decoder, DecoderOptions};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, MetadataRevision, StandardTagKey};
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

pub const FORMATS: [&str; 4] = ["mp3", "ogg", "oga", "wav"];
pub const BANDS: usize = 32;
const SCOPE: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repeat {
    Off,
    All,
    One,
}

impl Repeat {
    pub fn from_index(i: i32) -> Repeat {
        match i {
            0 => Repeat::Off,
            2 => Repeat::One,
            _ => Repeat::All,
        }
    }
}

#[derive(Clone, Debug)]
struct Meta {
    title: String,
    artist: Option<String>,
    album: Option<String>,
    duration_ms: i64,
    error: Option<String>,
    read: bool,
    cover: Option<Arc<Vec<u8>>>,
}

/// One song file. Tags are filled in by the library thread.
pub struct Track {
    pub path: PathBuf,
    /// Path inside the music folder, with '/' (what is remembered between sessions).
    pub rel: String,
    /// Playlist (first sub-folder), or `None` for songs at the top.
    pub playlist: Option<String>,
    size: u64,
    modified: i64,
    meta: Mutex<Meta>,
}

impl Track {
    fn new(path: PathBuf, rel: String, playlist: Option<String>, size: u64, modified: i64) -> Track {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let (title, artist) = from_name(&name);
        Track {
            path,
            rel,
            playlist,
            size,
            modified,
            meta: Mutex::new(Meta { title, artist, album: None, duration_ms: -1, error: None, read: false, cover: None }),
        }
    }

    fn meta(&self) -> MutexGuard<'_, Meta> {
        self.meta.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn title(&self) -> String {
        self.meta().title.clone()
    }
    pub fn artist(&self) -> Option<String> {
        self.meta().artist.clone()
    }
    pub fn album(&self) -> Option<String> {
        self.meta().album.clone()
    }
    pub fn duration_ms(&self) -> i64 {
        self.meta().duration_ms
    }
    pub fn error(&self) -> Option<String> {
        self.meta().error.clone()
    }
    pub fn has_read(&self) -> bool {
        self.meta().read
    }
    pub fn has_cover(&self) -> bool {
        self.meta().cover.is_some()
    }

    /// "Artist", else the album, else "Unknown artist".
    pub fn subtitle(&self) -> String {
        let m = self.meta();
        match (&m.artist, &m.album) {
            (Some(a), _) if !a.is_empty() => a.clone(),
            (_, Some(al)) => al.clone(),
            _ => "Unknown artist".to_owned(),
        }
    }

    fn key(&self) -> String {
        format!("{}@{}", self.rel, self.modified)
    }
}

/// "Artist - Title.mp3" -> title and artist, for whatever the tags leave empty.
pub fn from_name(file_name: &str) -> (String, Option<String>) {
    let stem = match file_name.rfind('.') {
        Some(i) if i > 0 => &file_name[..i],
        _ => file_name,
    };
    let base = stem.replace('_', " ").trim().to_owned();
    match base.find(" - ") {
        Some(d) if d > 0 => {
            let artist = base[..d].trim().to_owned();
            let title = base[d + 3..].trim().to_owned();
            (if title.is_empty() { base.clone() } else { title }, Some(artist).filter(|a| !a.is_empty()))
        }
        _ => (base, None),
    }
}

/// "3:07", "1:02:45", "--:--" when unknown.
pub fn time(ms: i64) -> String {
    if ms < 0 {
        return "--:--".to_owned();
    }
    let s = ms / 1000;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

// ── shared state ─────────────────────────────────────────────────────────

enum Cmd {
    Play(Arc<Track>),
    Toggle,
    Next,
    Previous,
    Seek(i64),
    Restore(Arc<Track>, i64),
}

struct Shared {
    folder: Option<PathBuf>,
    library: Arc<Vec<Arc<Track>>>,
    playlists: Arc<Vec<String>>,
    playlist: Option<String>,
    current: Option<Arc<Track>>,
    playing: bool,
    position_ms: i64,
    seeking_to: Option<i64>,
    error: Option<(String, Instant)>,
    scanning: bool,
    scanned_once: bool,
    volume: f32,
    shuffle: bool,
    repeat: Repeat,
    duck: f32,
    commands: VecDeque<Cmd>,
    running: bool,
    rescan: bool,
    shuffled: Vec<Arc<Track>>,
    shuffled_from: Vec<String>,
    pending_restore: Option<(String, i64)>,
    save_soon: bool,
    rng: u64,
}

/// The speakers' side: what the audio callback plays.
struct Pipe {
    buf: VecDeque<f32>,
    gain: f32,
    target: f32,
    /// Frames the speakers took since the song was opened.
    played: u64,
    rate: u32,
    scope: Vec<f32>,
    scope_at: usize,
}

struct Core {
    s: Mutex<Shared>,
    wake: Condvar,
    pipe: Arc<Mutex<Pipe>>,
    spectrum: Mutex<Spectrum>,
    covers: Mutex<Covers>,
}

fn core() -> &'static Core {
    static CORE: OnceLock<Core> = OnceLock::new();
    CORE.get_or_init(|| Core {
        s: Mutex::new(Shared {
            folder: None,
            library: Arc::new(Vec::new()),
            playlists: Arc::new(Vec::new()),
            playlist: None,
            current: None,
            playing: false,
            position_ms: 0,
            seeking_to: None,
            error: None,
            scanning: false,
            scanned_once: false,
            volume: 0.7,
            shuffle: false,
            repeat: Repeat::All,
            duck: 1.0,
            commands: VecDeque::new(),
            running: false,
            rescan: true,
            shuffled: Vec::new(),
            shuffled_from: Vec::new(),
            pending_restore: None,
            save_soon: false,
            rng: 0x9E37_79B9_7F4A_7C15
                ^ std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64),
        }),
        wake: Condvar::new(),
        pipe: Arc::new(Mutex::new(Pipe { buf: VecDeque::new(), gain: 0.0, target: 0.0, played: 0, rate: 48_000, scope: vec![0.0; SCOPE], scope_at: 0 })),
        spectrum: Mutex::new(Spectrum::default()),
        covers: Mutex::new(Covers::default()),
    })
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn shared() -> MutexGuard<'static, Shared> {
    lock(&core().s)
}

fn send(cmd: Cmd) {
    shared().commands.push_back(cmd);
    core().wake.notify_all();
}

// ── folder ───────────────────────────────────────────────────────────────

/// The shared music folder. Launched by Native the instance lives in `<root>/instances/<id>`: songs go in
/// `<root>/music` next to the shared assets. Other launchers use the Native launcher's own folder, so every
/// instance on the PC still sees the same songs.
pub fn resolve_folder(game_dir: Option<&Path>, appdata: Option<PathBuf>, home: Option<PathBuf>) -> PathBuf {
    if let Some(parent) = game_dir.and_then(Path::parent)
        && parent.file_name().is_some_and(|n| n == "instances")
        && let Some(root) = parent.parent()
        && (root.join("versions").is_dir() || root.join("assets").is_dir() || root.join("music").is_dir())
    {
        return root.join("music");
    }
    let base = appdata.unwrap_or_else(|| {
        let home = home.unwrap_or_else(|| PathBuf::from("."));
        if cfg!(windows) {
            home.join("AppData").join("Roaming")
        } else if cfg!(target_os = "macos") {
            home.join("Library").join("Application Support")
        } else {
            home.join(".config")
        }
    });
    for name in ["Native", "Native Client", "native-client"] {
        if base.join(name).join("minecraft").is_dir() {
            return base.join(name).join("minecraft").join("music");
        }
    }
    base.join("Native").join("minecraft").join("music")
}

pub fn init(game_dir: Option<&Path>) {
    let mut s = shared();
    if s.folder.is_none() {
        let appdata = if cfg!(windows) {
            std::env::var_os("APPDATA").map(PathBuf::from)
        } else if cfg!(target_os = "macos") {
            None
        } else {
            std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from)
        };
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
        s.folder = Some(resolve_folder(game_dir.map(|d| d.canonicalize().unwrap_or_else(|_| d.to_path_buf())).as_deref(), appdata, home));
    }
}

pub fn folder() -> Option<PathBuf> {
    shared().folder.clone()
}

/// Opens the music folder in the file manager (creating it first).
pub fn open_folder() {
    let Some(dir) = folder() else { return };
    let _ = std::thread::Builder::new().name("Native music folder".into()).spawn(move || {
        let _ = std::fs::create_dir_all(&dir);
        let cmd = if cfg!(windows) {
            "explorer"
        } else if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        let _ = std::process::Command::new(cmd).arg(&dir).spawn();
        rescan_soon();
    });
}

// ── lifecycle ────────────────────────────────────────────────────────────

/// Starts the audio and library threads. Idempotent.
pub fn start() {
    {
        let mut s = shared();
        if s.running {
            return;
        }
        s.running = true;
    }
    let _ = std::thread::Builder::new().name("Native music".into()).spawn(audio_loop);
    let _ = std::thread::Builder::new().name("Native music library".into()).spawn(library_loop);
}

/// Stops playing and lets the speakers go (game closing).
pub fn stop() {
    save_state();
    let mut s = shared();
    s.running = false;
    s.playing = false;
    drop(s);
    core().wake.notify_all();
}

pub fn running() -> bool {
    shared().running
}

/// What the Music page's settings say, every frame.
pub fn configure(volume: f32, shuffle: bool, repeat: Repeat, duck: f32) {
    let mut s = shared();
    s.volume = volume.clamp(0.0, 1.0);
    s.shuffle = shuffle;
    s.repeat = repeat;
    s.duck = duck;
}

/// The volume now (the slider being dragged is heard at once).
pub fn set_volume(v: f32) {
    shared().volume = v.clamp(0.0, 1.0);
}

// ── what the UI reads ────────────────────────────────────────────────────

pub fn library() -> Arc<Vec<Arc<Track>>> {
    shared().library.clone()
}
pub fn playlists() -> Arc<Vec<String>> {
    shared().playlists.clone()
}
pub fn playlist() -> Option<String> {
    shared().playlist.clone()
}
pub fn set_playlist(name: Option<String>) {
    shared().playlist = name;
}

/// Songs of the chosen playlist (all songs when none is chosen).
pub fn visible() -> Vec<Arc<Track>> {
    let s = shared();
    visible_of(&s)
}

fn visible_of(s: &Shared) -> Vec<Arc<Track>> {
    match &s.playlist {
        None => s.library.as_ref().clone(),
        Some(p) => s.library.iter().filter(|t| t.playlist.as_deref() == Some(p)).cloned().collect(),
    }
}

pub fn current() -> Option<Arc<Track>> {
    shared().current.clone()
}
pub fn playing() -> bool {
    shared().playing
}
pub fn position() -> i64 {
    let s = shared();
    s.seeking_to.unwrap_or(s.position_ms)
}
pub fn duration() -> i64 {
    shared().current.as_ref().map_or(-1, |t| t.duration_ms())
}
pub fn scanning() -> bool {
    let s = shared();
    s.scanning || !s.scanned_once
}

/// A problem worth showing for a few seconds ("Couldn't play ..."), or `None`.
pub fn error() -> Option<String> {
    shared().error.as_ref().filter(|(_, at)| at.elapsed() < Duration::from_secs(8)).map(|(m, _)| m.clone())
}

fn fail(message: String) {
    shared().error = Some((message, Instant::now()));
}

// ── commands ─────────────────────────────────────────────────────────────

pub fn play(t: &Arc<Track>) {
    send(Cmd::Play(t.clone()));
}

pub fn toggle() {
    let first = {
        let s = shared();
        if s.current.is_none() { Some(order_of(&s).into_iter().next()) } else { None }
    };
    match first {
        Some(Some(t)) => play(&t),
        Some(None) => {}
        None => send(Cmd::Toggle),
    }
}

pub fn next() {
    send(Cmd::Next);
}

/// Back to the start of the song, or the previous song in the first three seconds.
pub fn previous() {
    send(Cmd::Previous);
}

pub fn seek(ms: i64) {
    let mut s = shared();
    if s.current.is_none() {
        return;
    }
    let ms = ms.max(0);
    s.seeking_to = Some(ms);
    s.commands.push_back(Cmd::Seek(ms));
    drop(s);
    core().wake.notify_all();
}

pub fn rescan_soon() {
    shared().rescan = true;
}

// ── play order ───────────────────────────────────────────────────────────

fn next_random(s: &mut Shared) -> u64 {
    s.rng ^= s.rng << 13;
    s.rng ^= s.rng >> 7;
    s.rng ^= s.rng << 17;
    s.rng
}

/// The order songs play in: the playlist, or a shuffled copy that stays put until the playlist changes.
fn order_of(s: &Shared) -> Vec<Arc<Track>> {
    let v = visible_of(s);
    if !s.shuffle {
        return v;
    }
    let names: Vec<String> = v.iter().map(|t| t.rel.clone()).collect();
    if s.shuffled_from == names {
        return s.shuffled.clone();
    }
    v
}

fn ordered(s: &mut Shared) -> Vec<Arc<Track>> {
    let v = visible_of(s);
    if !s.shuffle {
        return v;
    }
    let names: Vec<String> = v.iter().map(|t| t.rel.clone()).collect();
    if s.shuffled_from != names {
        let mut list = v;
        for i in (1..list.len()).rev() {
            let j = (next_random(s) % (i as u64 + 1)) as usize;
            list.swap(i, j);
        }
        // The song playing now stays first, so "next" never repeats it.
        if let Some(cur) = &s.current
            && let Some(at) = list.iter().position(|t| Arc::ptr_eq(t, cur))
        {
            let c = list.remove(at);
            list.insert(0, c);
        }
        s.shuffled = list;
        s.shuffled_from = names;
    }
    s.shuffled.clone()
}

/// The song `step` away from the current one, or `None` at the end (repeat off).
fn step(order: &[Arc<Track>], cur: Option<&Arc<Track>>, step: i32, repeat: Repeat) -> Option<Arc<Track>> {
    if order.is_empty() {
        return None;
    }
    let i = cur.and_then(|c| order.iter().position(|t| Arc::ptr_eq(t, c))).map_or(-1, |i| i as i32);
    let mut n = i + step;
    let len = order.len() as i32;
    if n < 0 || n >= len {
        if repeat == Repeat::Off && cur.is_some() {
            return None;
        }
        n = n.rem_euclid(len);
    }
    order.get(n as usize).cloned()
}

// ── audio thread ─────────────────────────────────────────────────────────

/// One song being decoded.
struct Source {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    rate: u32,
    skip: u64,
    buf: Option<SampleBuffer<f32>>,
}

fn open_source(path: &Path, start_ms: i64) -> Result<Source, String> {
    let file = File::open(path).map_err(|e| format!("Can't open the file ({e})."))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(&ext.to_lowercase());
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions { enable_gapless: true, ..Default::default() }, &MetadataOptions::default())
        .map_err(|_| "Can't play this file.".to_owned())?;
    let mut format = probed.format;
    let track = format.default_track().ok_or_else(|| "No audio in this file.".to_owned())?;
    let (track_id, params) = (track.id, track.codec_params.clone());
    let mut decoder = symphonia::default::get_codecs().make(&params, &DecoderOptions::default()).map_err(|_| "This audio format isn't supported.".to_owned())?;
    let mut skip = 0;
    let rate = params.sample_rate.unwrap_or(44_100);
    if start_ms > 0 {
        let secs = start_ms as f64 / 1000.0;
        match format.seek(SeekMode::Accurate, SeekTo::Time { time: Time::from(secs), track_id: Some(track_id) }) {
            Ok(seeked) => {
                skip = seeked.required_ts.saturating_sub(seeked.actual_ts);
                decoder.reset();
            }
            Err(_) => skip = (secs * f64::from(rate)) as u64, // No seeking in this file: decode up to the place.
        }
    }
    Ok(Source { format, decoder, track_id, rate, skip, buf: None })
}

impl Source {
    /// Decodes a bit more, appended as interleaved stereo at the song's rate. False at the end.
    fn read(&mut self, out: &mut Vec<f32>) -> Result<bool, String> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(p) => p,
                Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(false),
                Err(SymError::ResetRequired) => return Ok(false),
                Err(e) => return Err(e.to_string()),
            };
            if packet.track_id() != self.track_id {
                continue;
            }
            let decoded = match self.decoder.decode(&packet) {
                Ok(d) => d,
                Err(SymError::DecodeError(_) | SymError::IoError(_)) => continue,
                Err(e) => return Err(e.to_string()),
            };
            let spec = *decoded.spec();
            self.rate = spec.rate;
            let ch = spec.channels.count().max(1);
            let need = decoded.capacity() as u64;
            if self.buf.as_ref().is_none_or(|b| (b.capacity() as u64) < need * ch as u64) {
                self.buf = Some(SampleBuffer::<f32>::new(need, spec));
            }
            let Some(sb) = self.buf.as_mut() else { continue };
            sb.copy_interleaved_ref(decoded);
            let samples = sb.samples();
            let frames = samples.len() / ch;
            let drop = (self.skip as usize).min(frames);
            self.skip -= drop as u64;
            out.reserve((frames - drop) * 2);
            for f in drop..frames {
                let l = samples[f * ch];
                let r = if ch > 1 { samples[f * ch + 1] } else { l };
                out.push(l);
                out.push(r);
            }
            if frames > drop {
                return Ok(true);
            }
        }
    }
}

/// Linear resampling of interleaved stereo.
struct Resampler {
    step: f64,
    pos: f64,
    last: [f32; 2],
    fresh: bool,
}

impl Resampler {
    fn new(from: u32, to: u32) -> Resampler {
        Resampler { step: f64::from(from) / f64::from(to.max(1)), pos: 0.0, last: [0.0; 2], fresh: true }
    }

    fn run(&mut self, input: &[f32], out: &mut VecDeque<f32>) {
        let n = input.len() / 2;
        if n == 0 {
            return;
        }
        if (self.step - 1.0).abs() < 1e-9 {
            out.extend(input.iter().copied());
            return;
        }
        if self.fresh {
            self.last = [input[0], input[1]];
            self.fresh = false;
        }
        let frame = |i: isize, last: [f32; 2]| -> [f32; 2] { if i < 0 { last } else { [input[i as usize * 2], input[i as usize * 2 + 1]] } };
        while self.pos + 1.0 <= (n - 1) as f64 + 1e-9 && (self.pos.floor() as isize) + 1 < n as isize {
            let i = self.pos.floor() as isize;
            let t = (self.pos - i as f64) as f32;
            let (a, b) = (frame(i, self.last), frame(i + 1, self.last));
            out.push_back(a[0] + (b[0] - a[0]) * t);
            out.push_back(a[1] + (b[1] - a[1]) * t);
            self.pos += self.step;
        }
        self.pos -= n as f64;
        self.last = frame(n as isize - 1, self.last);
    }
}

/// The speakers: a cpal stream pulling from the pipe.
struct Output {
    _stream: cpal::Stream,
    rate: u32,
}

fn open_output(pipe: Arc<Mutex<Pipe>>) -> Result<Output, String> {
    use cpal::SampleFormat;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or_else(|| "No sound device for music.".to_owned())?;
    let supported = device.default_output_config().map_err(|e| format!("No sound device for music: {e}"))?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let (rate, channels) = (config.sample_rate.0, config.channels as usize);
    lock(&pipe).rate = rate;
    let err = |e: cpal::StreamError| log::warn!("Native music: {e}");
    let stream = match format {
        SampleFormat::F32 => {
            let p = pipe;
            device.build_output_stream(&config, move |d: &mut [f32], _| fill(d, channels, &p), err, None)
        }
        SampleFormat::I16 => {
            let p = pipe;
            device.build_output_stream(
                &config,
                move |d: &mut [i16], _| {
                    let mut tmp = vec![0.0f32; d.len()];
                    fill(&mut tmp, channels, &p);
                    for (o, v) in d.iter_mut().zip(tmp) {
                        *o = (v.clamp(-1.0, 1.0) * 32767.0) as i16;
                    }
                },
                err,
                None,
            )
        }
        SampleFormat::U16 => {
            let p = pipe;
            device.build_output_stream(
                &config,
                move |d: &mut [u16], _| {
                    let mut tmp = vec![0.0f32; d.len()];
                    fill(&mut tmp, channels, &p);
                    for (o, v) in d.iter_mut().zip(tmp) {
                        *o = ((v.clamp(-1.0, 1.0) * 0.5 + 0.5) * 65535.0) as u16;
                    }
                },
                err,
                None,
            )
        }
        other => return Err(format!("The sound device's format ({other:?}) isn't supported.")),
    }
    .map_err(|e| format!("No sound device for music: {e}"))?;
    stream.play().map_err(|e| format!("No sound device for music: {e}"))?;
    Ok(Output { _stream: stream, rate })
}

/// The audio callback: the pipe's frames at the pipe's gain, which ramps toward its target.
fn fill(data: &mut [f32], channels: usize, pipe: &Mutex<Pipe>) {
    let Ok(mut p) = pipe.lock() else {
        data.fill(0.0);
        return;
    };
    let step = 1.0 / (p.rate.max(1) as f32 * 0.03);
    for frame in data.chunks_mut(channels.max(1)) {
        // Silent and asked to stay so (paused): the queue keeps its place.
        if p.gain <= 0.0005 && p.target <= 0.0005 {
            frame.fill(0.0);
            continue;
        }
        let (Some(l), Some(r)) = (p.buf.pop_front(), p.buf.pop_front()) else {
            frame.fill(0.0);
            continue;
        };
        if p.gain < p.target {
            p.gain = (p.gain + step).min(p.target);
        } else if p.gain > p.target {
            p.gain = (p.gain - step).max(p.target);
        }
        let g = p.gain;
        let (l, r) = (l * g, r * g);
        match frame.len() {
            1 => frame[0] = (l + r) / 2.0,
            _ => {
                frame[0] = l;
                frame[1] = r;
                for extra in frame.iter_mut().skip(2) {
                    *extra = 0.0;
                }
            }
        }
        p.played += 1;
        let at = p.scope_at;
        p.scope[at] = (l + r) / 2.0;
        p.scope_at = (at + 1) % SCOPE;
    }
}

struct Audio {
    source: Option<Source>,
    resampler: Option<Resampler>,
    output: Option<Output>,
    open_at_ms: i64,
    eof: bool,
    idle_since: Instant,
    last_save: Instant,
    scratch: Vec<f32>,
}

fn audio_loop() {
    let core = core();
    let mut a = Audio { source: None, resampler: None, output: None, open_at_ms: 0, eof: false, idle_since: Instant::now(), last_save: Instant::now(), scratch: Vec::new() };
    load_state();
    loop {
        let (playing, volume, duck, running, cmds, save) = {
            let mut s = shared();
            let cmds: Vec<Cmd> = s.commands.drain(..).collect();
            let save = (s.save_soon && a.last_save.elapsed() > Duration::from_millis(1500)) || a.last_save.elapsed() > Duration::from_secs(15);
            if save {
                s.save_soon = false;
            }
            (s.playing, s.volume, s.duck, s.running, cmds, save)
        };
        if !running {
            break;
        }
        for c in cmds {
            run_command(&mut a, c);
        }
        if save {
            a.last_save = Instant::now();
            save_state();
        }
        let playing = playing && shared().playing;
        lock(&core.pipe).target = if playing { volume * duck } else { 0.0 };
        let silent = a.source.is_none() || (!playing && lock(&core.pipe).gain <= 0.0005);
        if silent {
            if a.output.is_some() && a.idle_since.elapsed() > Duration::from_secs(30) {
                a.output = None; // Paused for a while: other apps get the sound device back.
            }
            if a.source.is_some() {
                position_update(&a);
            } else {
                a.idle_since = Instant::now();
            }
            lock(&core.spectrum).decay(0.05);
            wait(&core.wake, 50);
            continue;
        }
        a.idle_since = Instant::now();
        if a.output.is_none() {
            match open_output(core.pipe.clone()) {
                Ok(o) => a.output = Some(o),
                Err(e) => {
                    fail(e);
                    shared().playing = false;
                    continue;
                }
            }
        }
        let queued = lock(&core.pipe).buf.len() / 2;
        let rate = a.output.as_ref().map_or(48_000, |o| o.rate) as usize;
        if !a.eof && queued < rate * 35 / 100 {
            let mut scratch = std::mem::take(&mut a.scratch);
            scratch.clear();
            let result = a.source.as_mut().map(|s| s.read(&mut scratch));
            match result {
                Some(Ok(true)) => {
                    let src_rate = a.source.as_ref().map_or(rate as u32, |s| s.rate);
                    let r = a.resampler.get_or_insert_with(|| Resampler::new(src_rate, rate as u32));
                    let mut p = lock(&core.pipe);
                    r.run(&scratch, &mut p.buf);
                }
                Some(Ok(false)) | None => a.eof = true,
                Some(Err(e)) => {
                    if let Some(t) = shared().current.clone() {
                        t.meta().error = Some(e.clone());
                    }
                    fail(format!("Couldn't play: {e}"));
                    a.eof = true;
                }
            }
            a.scratch = scratch;
        } else if a.eof && queued == 0 {
            advance(&mut a, 1, true);
            continue;
        } else {
            wait(&core.wake, 8);
        }
        position_update(&a);
    }
    a.output = None;
    a.source = None;
    shared().playing = false;
}

fn wait(cv: &Condvar, ms: u64) {
    let guard = shared();
    if guard.commands.is_empty() {
        let _ = cv.wait_timeout(guard, Duration::from_millis(ms));
    }
}

fn position_update(a: &Audio) {
    let core = core();
    let (played, rate) = {
        let p = lock(&core.pipe);
        (p.played, p.rate)
    };
    let mut s = shared();
    if s.seeking_to.is_none() {
        s.position_ms = a.open_at_ms + (played * 1000 / u64::from(rate.max(1))) as i64;
    }
}

fn run_command(a: &mut Audio, c: Cmd) {
    match c {
        Cmd::Play(t) => open(a, &t, 0, true),
        Cmd::Toggle => {
            let (cur, pos) = {
                let s = shared();
                (s.current.clone(), s.position_ms)
            };
            if a.source.is_none() {
                if let Some(t) = cur {
                    open(a, &t, pos, true);
                }
            } else {
                let mut s = shared();
                s.playing = !s.playing;
                s.save_soon = true;
            }
        }
        Cmd::Next => advance(a, 1, false),
        Cmd::Previous => {
            let (cur, pos, playing) = {
                let s = shared();
                (s.current.clone(), s.position_ms, s.playing)
            };
            match cur {
                Some(t) if pos > 3000 => open(a, &t, 0, playing),
                _ => advance(a, -1, false),
            }
        }
        Cmd::Seek(ms) => {
            let (cur, playing) = {
                let s = shared();
                (s.current.clone(), s.playing)
            };
            if let Some(t) = cur {
                open(a, &t, ms, playing);
            }
            shared().seeking_to = None;
        }
        Cmd::Restore(t, at) => {
            let mut s = shared();
            if s.current.is_none() {
                s.current = Some(t.clone());
                s.position_ms = at;
                drop(s);
                if !t.has_read() {
                    read_info(&t);
                }
            }
        }
    }
}

/// Runs on the audio thread: switches to `t` at `ms`.
fn open(a: &mut Audio, t: &Arc<Track>, ms: i64, play: bool) {
    a.source = None;
    a.resampler = None;
    a.eof = false;
    {
        let mut p = lock(&core().pipe);
        p.buf.clear();
        p.gain = 0.0; // Fades in: no click when jumping into the middle of a waveform.
        p.played = 0;
    }
    match open_source(&t.path, ms) {
        Ok(src) => {
            a.source = Some(src);
            t.meta().error = None;
        }
        Err(e) => {
            t.meta().error = Some(e.clone());
            fail(format!("Couldn't play {}: {e}", t.title()));
            let mut s = shared();
            s.current = Some(t.clone());
            s.playing = false;
            s.position_ms = ms;
            return;
        }
    }
    a.open_at_ms = ms;
    {
        let mut s = shared();
        s.current = Some(t.clone());
        s.position_ms = ms;
        s.playing = play;
        s.save_soon = true;
    }
    if !t.has_read() {
        read_info(t);
    }
}

fn advance(a: &mut Audio, dir: i32, ended: bool) {
    let (cur, repeat, order) = {
        let mut s = shared();
        let order = ordered(&mut s);
        (s.current.clone(), s.repeat, order)
    };
    if ended && repeat == Repeat::One {
        if let Some(c) = cur {
            open(a, &c, 0, true);
            return;
        }
    }
    let mut cur = cur;
    // Skip songs that can't be played, at most once around the list.
    for _ in 0..order.len().max(1) {
        let Some(next) = step(&order, cur.as_ref(), dir, if ended { repeat } else { Repeat::All }) else {
            // The end of the playlist with repeat off: stop on the last song, back at its start.
            a.source = None;
            a.resampler = None;
            let mut s = shared();
            s.playing = false;
            s.position_ms = 0;
            return;
        };
        if next.error().is_none() || !ended {
            open(a, &next, 0, true);
            if a.source.is_some() && shared().current.as_ref().is_some_and(|c| Arc::ptr_eq(c, &next)) {
                return;
            }
        }
        cur = Some(next);
    }
    shared().playing = false;
}

// ── library ──────────────────────────────────────────────────────────────

fn playable(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| FORMATS.contains(&e.to_lowercase().as_str()))
}

/// Changes whenever a song is added, removed, renamed or replaced (cheap: listing only).
fn signature(dir: &Path) -> String {
    let mut out = String::new();
    if !dir.is_dir() {
        return out;
    }
    let mut dirs = vec![dir.to_path_buf()];
    if let Ok(top) = std::fs::read_dir(dir) {
        dirs.extend(top.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
    }
    for d in dirs {
        let m = std::fs::metadata(&d).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis());
        out.push_str(&format!("{}:{m};", d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
    }
    out
}

fn track_of(old: &HashMap<String, Arc<Track>>, root: &Path, file: &Path, list: Option<&str>) -> Arc<Track> {
    let rel = file.strip_prefix(root).unwrap_or(file).to_string_lossy().replace('\\', "/");
    let meta = std::fs::metadata(file).ok();
    let size = meta.as_ref().map_or(0, std::fs::Metadata::len);
    let modified = meta.and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as i64);
    if let Some(prev) = old.get(&rel)
        && prev.size == size
        && prev.modified == modified
    {
        return prev.clone();
    }
    Arc::new(Track::new(file.to_path_buf(), rel, list.map(str::to_owned), size, modified))
}

fn scan(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    let old: HashMap<String, Arc<Track>> = shared().library.iter().map(|t| (t.rel.clone(), t.clone())).collect();
    let mut found: Vec<Arc<Track>> = Vec::new();
    let mut lists: Vec<String> = Vec::new();
    if let Ok(top) = std::fs::read_dir(dir) {
        for e in top.flatten() {
            let p = e.path();
            if p.is_dir() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                if let Ok(sub) = std::fs::read_dir(&p) {
                    for q in sub.flatten().map(|q| q.path()) {
                        if q.is_file() && playable(&q) {
                            found.push(track_of(&old, dir, &q, Some(&name)));
                            if !lists.contains(&name) {
                                lists.push(name.clone());
                            }
                        }
                    }
                }
            } else if p.is_file() && playable(&p) {
                found.push(track_of(&old, dir, &p, None));
            }
        }
    }
    found.sort_by(|a, b| {
        let key = |t: &Arc<Track>| (t.playlist.clone().unwrap_or_default().to_lowercase(), t.rel.to_lowercase());
        key(a).cmp(&key(b))
    });
    lists.sort_by_key(|l| l.to_lowercase());
    let restore = {
        let mut s = shared();
        s.library = Arc::new(found);
        if s.playlist.as_ref().is_some_and(|p| !lists.contains(p)) {
            s.playlist = None;
        }
        s.playlists = Arc::new(lists);
        s.pending_restore.take()
    };
    if let Some((rel, at)) = restore {
        let t = shared().library.iter().find(|t| t.rel == rel).cloned();
        if let Some(t) = t {
            send(Cmd::Restore(t, at));
        }
    }
}

fn library_loop() {
    let mut signature_seen = String::new();
    loop {
        let (dir, rescan, running) = {
            let s = shared();
            (s.folder.clone(), s.rescan, s.running)
        };
        if !running {
            return;
        }
        if let Some(dir) = dir {
            let sig = signature(&dir);
            if rescan || sig != signature_seen {
                signature_seen = sig;
                {
                    let mut s = shared();
                    s.rescan = false;
                    s.scanning = true;
                }
                scan(&dir);
                let mut s = shared();
                s.scanning = false;
                s.scanned_once = true;
            }
            let unread: Vec<Arc<Track>> = shared().library.iter().filter(|t| !t.has_read()).cloned().collect();
            for t in unread {
                if !shared().running {
                    return;
                }
                read_info(&t);
            }
        }
        std::thread::sleep(Duration::from_secs(3));
    }
}

// ── tags, durations and covers ───────────────────────────────────────────

#[derive(Default)]
struct Info {
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    duration_ms: i64,
    cover: Option<Vec<u8>>,
}

fn take_tags(info: &mut Info, rev: &MetadataRevision) {
    for tag in rev.tags() {
        let value = tag.value.to_string().trim().to_owned();
        if value.is_empty() {
            continue;
        }
        match tag.std_key {
            Some(StandardTagKey::TrackTitle) if info.title.is_none() => info.title = Some(value),
            Some(StandardTagKey::Artist) if info.artist.is_none() => info.artist = Some(value),
            Some(StandardTagKey::AlbumArtist) if info.artist.is_none() => info.artist = Some(value),
            Some(StandardTagKey::Album) if info.album.is_none() => info.album = Some(value),
            _ => {}
        }
    }
    if info.cover.is_none()
        && let Some(v) = rev.visuals().first()
    {
        info.cover = Some(v.data.to_vec());
    }
}

fn probe_info(path: &Path) -> Result<Info, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(&ext.to_lowercase());
    }
    let mut probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|_| "Can't read this file.".to_owned())?;
    let mut info = Info { duration_ms: -1, ..Info::default() };
    if let Some(m) = probed.metadata.get()
        && let Some(rev) = m.current()
    {
        take_tags(&mut info, rev);
    }
    {
        let md = probed.format.metadata();
        if let Some(rev) = md.current() {
            take_tags(&mut info, rev);
        }
    }
    let Some(track) = probed.format.default_track() else { return Ok(info) };
    let (id, params) = (track.id, track.codec_params.clone());
    if let (Some(n), Some(rate)) = (params.n_frames, params.sample_rate) {
        info.duration_ms = (n * 1000 / u64::from(rate.max(1))) as i64;
    } else if let Some(rate) = params.sample_rate {
        // No length in the header: add up the packets (no decoding, so it is quick).
        let mut frames = 0u64;
        while let Ok(p) = probed.format.next_packet() {
            if p.track_id() == id {
                frames += p.dur();
            }
        }
        if frames > 0 {
            info.duration_ms = (frames * 1000 / u64::from(rate.max(1))) as i64;
        }
    }
    Ok(info)
}

fn read_info(t: &Arc<Track>) {
    let result = probe_info(&t.path);
    let mut m = t.meta();
    m.read = true;
    match result {
        Ok(info) => {
            if let Some(title) = info.title {
                m.title = title;
            }
            if let Some(artist) = info.artist {
                m.artist = Some(artist);
            }
            m.album = info.album;
            if info.duration_ms > 0 {
                m.duration_ms = info.duration_ms;
            }
            m.cover = info.cover.filter(|c| c.len() <= 8 * 1024 * 1024).map(Arc::new);
        }
        Err(e) => {
            if m.error.is_none() {
                m.error = Some(e);
            }
        }
    }
}

// ── covers ───────────────────────────────────────────────────────────────

#[derive(Default)]
struct Covers {
    ready: HashMap<String, (Option<Arc<Image>>, u32)>,
    order: VecDeque<String>,
    decoding: HashSet<String>,
}

fn hash_key(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)) | 1 << 62
}

/// A stable id for stamping the cover of `t`.
pub fn cover_key(t: &Track) -> u64 {
    hash_key(&format!("cover:{}", t.key()))
}

/// The cover picture (scaled to at most 256 px, corners rounded), or `None` while loading / when there is none.
pub fn cover(t: &Arc<Track>) -> Option<Arc<Image>> {
    let bytes = {
        let m = t.meta();
        if !m.read {
            return None;
        }
        m.cover.clone()?
    };
    let key = t.key();
    {
        let mut c = lock(&core().covers);
        if let Some((img, _)) = c.ready.get(&key) {
            return img.clone();
        }
        if !c.decoding.insert(key.clone()) {
            return None;
        }
    }
    let _ = std::thread::Builder::new().name("Native music cover".into()).spawn(move || {
        let done = Image::decode(&bytes).map(|full| {
            let small = scaled_to(&full, 256);
            let accent = accent_of(&small);
            (rounded(&small, 0.09), accent)
        });
        let mut c = lock(&core().covers);
        c.decoding.remove(&key);
        while c.order.len() >= 48 {
            if let Some(old) = c.order.pop_front() {
                c.ready.remove(&old);
            }
        }
        c.order.push_back(key.clone());
        c.ready.insert(key, match done {
            Some((img, accent)) => (Some(img), accent),
            None => (None, 0),
        });
    });
    None
}

/// A colour from the cover for the player's glow, or `fallback`.
pub fn accent(t: &Track, fallback: u32) -> u32 {
    lock(&core().covers).ready.get(&t.key()).filter(|(img, _)| img.is_some()).map_or(fallback, |(_, a)| *a)
}

fn scaled_to(src: &Arc<Image>, max: u32) -> Arc<Image> {
    let (w, h) = (src.width, src.height);
    if w <= max && h <= max {
        return src.clone();
    }
    let k = w.max(h) as f32 / max as f32;
    let (nw, nh) = (((w as f32 / k).round() as u32).max(1), ((h as f32 / k).round() as u32).max(1));
    src.scaled(nw, nh)
}

/// Rounded corners baked into the pixels (radius as a share of the size), anti-aliased.
fn rounded(src: &Arc<Image>, frac: f32) -> Arc<Image> {
    let (w, h) = (src.width as usize, src.height as usize);
    let r = w.min(h) as f32 * frac;
    let mut px = src.argb.clone();
    for y in 0..h {
        for x in 0..w {
            let dx = (r - x as f32 - 0.5).max(x as f32 + 0.5 - (w as f32 - r)).max(0.0);
            let dy = (r - y as f32 - 0.5).max(y as f32 + 0.5 - (h as f32 - r)).max(0.0);
            if dx <= 0.0 || dy <= 0.0 {
                continue;
            }
            let cover = (r - (dx * dx + dy * dy).sqrt() + 0.5).clamp(0.0, 1.0);
            let c = px[y * w + x];
            px[y * w + x] = (((c >> 24) as f32 * cover).round() as u32) << 24 | (c & 0xFFFFFF);
        }
    }
    Image::new(w as u32, h as u32, px)
}

fn hsv(r: f32, g: f32, b: f32) -> [f32; 3] {
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let d = max - min;
    let mut h = 0.0;
    if d > 0.0 {
        h = if max == r { ((g - b) / d) % 6.0 } else if max == g { (b - r) / d + 2.0 } else { (r - g) / d + 4.0 };
        h /= 6.0;
        if h < 0.0 {
            h += 1.0;
        }
    }
    [h, if max == 0.0 { 0.0 } else { d / max }, max]
}

fn rgb(h: f32, s: f32, v: f32) -> u32 {
    let c = v * s;
    let x = c * (1.0 - ((h * 6.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h * 6.0) as i32 % 6 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    0xFF00_0000 | ((((r + m) * 255.0).round() as u32) << 16) | ((((g + m) * 255.0).round() as u32) << 8) | ((b + m) * 255.0).round() as u32
}

/// The cover's most colourful average, brightened enough to glow on a dark card.
fn accent_of(img: &Image) -> u32 {
    let (mut r, mut g, mut b, mut wsum) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for c in &img.argb {
        let (cr, cg, cb) = (((c >> 16) & 0xFF) as f64, ((c >> 8) & 0xFF) as f64, (c & 0xFF) as f64);
        let (max, min) = (cr.max(cg).max(cb), cr.min(cg).min(cb));
        // Colourful, bright pixels count most.
        let w = 0.05 + (max - min) / 255.0 * (max / 255.0);
        r += cr * w;
        g += cg * w;
        b += cb * w;
        wsum += w;
    }
    if wsum <= 0.0 {
        return 0xFFFFFFFF;
    }
    let [h, s, v] = hsv((r / wsum / 255.0) as f32, (g / wsum / 255.0) as f32, (b / wsum / 255.0) as f32);
    rgb(h, (s * 1.25 + 0.08).min(1.0), (v * 1.2).clamp(0.55, 0.85))
}

// ── the sound bars ───────────────────────────────────────────────────────

struct Spectrum {
    bands: [f32; BANDS],
    last: Option<Instant>,
}

impl Default for Spectrum {
    fn default() -> Self {
        Spectrum { bands: [0.0; BANDS], last: None }
    }
}

/// In-place radix-2 FFT.
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f32::consts::PI / len as f32;
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * cr - im[b] * ci, re[b] * ci + im[b] * cr);
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let ncr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = ncr;
            }
        }
        len <<= 1;
    }
}

impl Spectrum {
    /// Bands from the last `SCOPE` samples that reached the speakers; quick to rise, slow to fall.
    fn feed(&mut self, ring: &[f32], at: usize, rate: u32, dt: f32) {
        let mut re = vec![0.0f32; SCOPE];
        let mut im = vec![0.0f32; SCOPE];
        for i in 0..SCOPE {
            let w = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (SCOPE - 1) as f32).cos();
            re[i] = ring[(at + i) % SCOPE] * w;
        }
        fft(&mut re, &mut im);
        let rate = rate.max(8000) as f64;
        let (lo, hi) = (40.0f64, 16_000.0f64.min(rate / 2.0));
        let mut edges = [0usize; BANDS + 1];
        for (b, e) in edges.iter_mut().enumerate() {
            let f = lo * (hi / lo).powf(b as f64 / BANDS as f64);
            *e = ((f * SCOPE as f64 / rate).round() as usize).clamp(1, SCOPE / 2 - 1);
        }
        for b in 1..=BANDS {
            if edges[b] <= edges[b - 1] {
                edges[b] = (edges[b - 1] + 1).min(SCOPE / 2);
            }
        }
        for b in 0..BANDS {
            let mut peak = 0.0f32;
            for k in edges[b]..edges[b + 1].max(edges[b] + 1) {
                peak = peak.max(re[k] * re[k] + im[k] * im[k]);
            }
            // dB, 0 at a loud tone, a gentle lift for the highs (they carry less energy).
            let db = 10.0 * (f64::from(peak) / (SCOPE as f64 * SCOPE as f64 / 16.0) + 1e-12).log10() + b as f64 * 0.45;
            let target = ((db + 62.0) / 58.0).clamp(0.0, 1.0) as f32;
            let cur = self.bands[b];
            self.bands[b] = if target > cur { cur + (target - cur) * (dt * 28.0).min(1.0) } else { target.max(cur - dt * 1.6) };
        }
    }

    fn decay(&mut self, dt: f32) {
        for b in &mut self.bands {
            *b = (*b - dt * 1.6).max(0.0);
        }
    }
}

/// The sound bars into `out` (any length: bands are resampled).
pub fn bars(out: &mut [f32]) {
    let core = core();
    let (ring, at, rate, active) = {
        let p = lock(&core.pipe);
        (p.scope.clone(), p.scope_at, p.rate, p.gain > 0.0005)
    };
    let mut sp = lock(&core.spectrum);
    let dt = sp.last.map_or(0.016, |t| t.elapsed().as_secs_f32().clamp(0.001, 0.1));
    sp.last = Some(Instant::now());
    if active {
        sp.feed(&ring, at, rate, dt);
    } else {
        sp.decay(dt);
    }
    let n = out.len();
    for (i, o) in out.iter_mut().enumerate() {
        let pos = if n == 1 { 0.0 } else { i as f32 * (BANDS - 1) as f32 / (n - 1) as f32 };
        let (a, f) = (pos.floor() as usize, pos.fract());
        let b = (a + 1).min(BANDS - 1);
        *o = sp.bands[a] * (1.0 - f) + sp.bands[b] * f;
    }
}

// ── remembered between sessions (in the shared folder) ───────────────────

fn state_file() -> Option<PathBuf> {
    shared().folder.as_ref().map(|d| d.join(".native-player.json"))
}

fn load_state() {
    let Some(f) = state_file() else { return };
    let Ok(text) = std::fs::read_to_string(f) else { return };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { return };
    let mut s = shared();
    if let Some(t) = v.get("track").and_then(|t| t.as_str()) {
        s.pending_restore = Some((t.to_owned(), v.get("position").and_then(serde_json::Value::as_i64).unwrap_or(0)));
    }
    if let Some(p) = v.get("playlist").and_then(|p| p.as_str()) {
        s.playlist = Some(p.to_owned());
    }
}

fn save_state() {
    let Some(f) = state_file() else { return };
    let (track, position, playlist) = {
        let s = shared();
        (s.current.as_ref().map(|t| t.rel.clone()), s.position_ms, s.playlist.clone())
    };
    let Some(track) = track else { return };
    let v = serde_json::json!({ "track": track, "position": position, "playlist": playlist });
    if let Some(dir) = f.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = f.with_extension("json.tmp");
    if std::fs::write(&tmp, v.to_string()).is_ok() {
        let _ = std::fs::rename(&tmp, &f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(rel: &str) -> Arc<Track> {
        Arc::new(Track::new(PathBuf::from(rel), rel.to_owned(), None, 1, 1))
    }

    #[test]
    fn names_say_artist_and_title() {
        assert_eq!(from_name("Daft Punk - One More Time.mp3"), ("One More Time".to_owned(), Some("Daft Punk".to_owned())));
        assert_eq!(from_name("my_song.ogg"), ("my song".to_owned(), None));
        assert_eq!(from_name(" - lonely.wav").1, None);
    }

    #[test]
    fn times_read_like_a_player() {
        assert_eq!(time(-1), "--:--");
        assert_eq!(time(187_000), "3:07");
        assert_eq!(time(3_765_000), "1:02:45");
    }

    #[test]
    fn stepping_wraps_with_repeat_and_stops_without() {
        let order = vec![track("a.mp3"), track("b.mp3"), track("c.mp3")];
        assert!(Arc::ptr_eq(&step(&order, Some(&order[0]), 1, Repeat::All).unwrap(), &order[1]));
        assert!(Arc::ptr_eq(&step(&order, Some(&order[2]), 1, Repeat::All).unwrap(), &order[0]));
        assert!(Arc::ptr_eq(&step(&order, Some(&order[0]), -1, Repeat::All).unwrap(), &order[2]));
        assert!(step(&order, Some(&order[2]), 1, Repeat::Off).is_none());
        assert!(Arc::ptr_eq(&step(&order, None, 1, Repeat::Off).unwrap(), &order[0]));
        assert!(step(&[], None, 1, Repeat::All).is_none());
    }

    #[test]
    fn the_folder_is_the_launchers_shared_one() {
        let root = std::env::temp_dir().join(format!("native-music-root-{}", std::process::id()));
        let game = root.join("instances").join("one");
        let _ = std::fs::create_dir_all(&game);
        let _ = std::fs::create_dir_all(root.join("versions"));
        assert_eq!(resolve_folder(Some(&game), None, None), root.join("music"));
        let other = resolve_folder(Some(Path::new("/somewhere/else")), Some(PathBuf::from("/appdata")), None);
        assert_eq!(other, PathBuf::from("/appdata").join("Native").join("minecraft").join("music"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_fft_finds_a_tone() {
        let n = 1024;
        let mut re: Vec<f32> = (0..n).map(|i| (2.0 * std::f32::consts::PI * 64.0 * i as f32 / n as f32).sin()).collect();
        let mut im = vec![0.0; n];
        fft(&mut re, &mut im);
        let mag = |k: usize| re[k] * re[k] + im[k] * im[k];
        assert!(mag(64) > 1000.0 * mag(10));
    }

    #[test]
    fn the_resampler_keeps_the_length_in_proportion() {
        let mut r = Resampler::new(24_000, 48_000);
        let input: Vec<f32> = (0..2000).map(|i| (i / 2) as f32).collect();
        let mut out = VecDeque::new();
        r.run(&input, &mut out);
        let frames = out.len() / 2;
        assert!((1990..=2000).contains(&frames), "{frames}");
        let mut same = Resampler::new(48_000, 48_000);
        let mut out = VecDeque::new();
        same.run(&input, &mut out);
        assert_eq!(out.len(), input.len());
    }

    #[test]
    fn rounded_corners_clear_the_corner_pixel_only() {
        let img = Image::new(20, 20, vec![0xFFFFFFFF; 400]);
        let r = rounded(&img, 0.3);
        assert_eq!(r.argb[0] >> 24, 0);
        assert_eq!(r.argb[10 * 20 + 10], 0xFFFFFFFF);
    }

    #[test]
    fn a_cover_colour_is_bright_enough_to_glow() {
        let img = Image::new(4, 4, vec![0xFF203060; 16]);
        let a = accent_of(&img);
        let v = hsv(((a >> 16) & 255) as f32 / 255.0, ((a >> 8) & 255) as f32 / 255.0, (a & 255) as f32 / 255.0)[2];
        assert!(v >= 0.5, "{v}");
    }
}
