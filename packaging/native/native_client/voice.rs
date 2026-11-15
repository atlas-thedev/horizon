//! Native Voice: friend and group calls through Relay, native-mod's `xyz.nativelaunch.voice.Voice` in Rust. You
//! are only ever heard by the people in the call you joined (there is no proximity voice).
//!
//! A call: `POST /v1/voice/join` hands out a seat (sid, key, the relay's address), then the audio goes over UDP
//! ([`super::voice_net`]) through [`super::voice_audio`]. Rosters, rings and ends come on the Relay event stream
//! (`voice:state`, `voice:ring`, `voice:ring-end`, `voice:declined`, `voice:ended`). The same server and packets as
//! the Java mod, so both editions talk in one call.
//!
//! Main-thread API for the UI; HTTP runs on one worker thread (in order), audio and the socket on their own.

use super::relay::{self, Notice};
use super::voice_audio as audio;
use super::voice_net::{self as vnet, Net};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, MutexGuard, OnceLock};

/// Silent this long after the link worked: take the call's seat again (the voice server restarted, or the network
/// changed). Tried every 5 s, given up after [`GIVE_UP_MS`].
pub const LOST_MS: i64 = 5_000;
pub const GIVE_UP_MS: i64 = 120_000;
/// A resumed private call whose friend does not come back within this is over.
pub const RESUME_WAIT_MS: i64 = 20_000;

fn now() -> i64 {
    relay::now_ms()
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Member {
    pub sid: u32,
    pub user_id: String,
    pub name: String,
    pub ign: Option<String>,
    pub skin: Option<String>,
    pub muted: bool,
    pub deafened: bool,
    pub connected: bool,
}

impl Member {
    fn parse(o: &Value) -> Option<Member> {
        Some(Member {
            sid: o.get("sid")?.as_u64()? as u32,
            user_id: str_of(o, "userId").unwrap_or_default(),
            name: str_of(o, "name").unwrap_or_default(),
            ign: str_of(o, "ign"),
            skin: str_of(o, "skin"),
            muted: bool_of(o, "muted"),
            deafened: bool_of(o, "deafened"),
            connected: bool_of(o, "connected"),
        })
    }

    /// The name to show: the Minecraft name the player joined with, else their Native name.
    pub fn shown(&self) -> &str {
        self.ign.as_deref().filter(|n| !n.is_empty()).unwrap_or(&self.name)
    }
}

fn members(list: Option<&Value>) -> Vec<Member> {
    list.and_then(Value::as_array).map_or_else(Vec::new, |a| a.iter().filter_map(Member::parse).collect())
}

/// The call you are in.
#[derive(Clone, Debug)]
pub struct Room {
    /// Conversation key ("dm:<friend>", "g:<group>").
    pub key: String,
    /// The server's room ("dm:<a>:<b>", "g:<group>").
    pub room: String,
    pub sid: u32,
    secret: [u8; 16],
    seq: u32,
    pub members: Vec<Member>,
    /// Round trip to the relay in ms, -1 before the first pong.
    pub rtt: i32,
    pub last_pong: i64,
    last_ping: i64,
    pub joined_at: i64,
    /// When the call began for you (kept across seats taken again): the call timer.
    pub since: i64,
    /// Someone else has been in this call: from then on, being the only one left means it is over (private calls).
    pub saw_others: bool,
    resumed: bool,
    /// Resumed as the same call going on (not ringing again): no "Calling...", the friend is expected back.
    carry_on: bool,
    resumed_at: i64,
    /// When the link was lost and not back yet, 0 while it is fine.
    lost_since: i64,
}

impl Room {
    /// The last moment the link was known to work: the last pong, or when this seat was taken again.
    fn alive_at(&self) -> i64 {
        self.last_pong.max(if self.resumed { self.resumed_at } else { 0 })
    }
}

/// Somebody is calling you: a card with accept / decline until it is answered, cancelled or times out.
#[derive(Clone, Debug, PartialEq)]
pub struct Ring {
    pub key: String,
    pub user_id: String,
    pub name: String,
    pub skin: Option<String>,
    pub at: i64,
    pub until: i64,
}

/// The Voice Chat module's settings, handed over every frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub push_to_talk: bool,
    pub ptt_held: bool,
    pub threshold_db: f32,
    pub mic_gain: f32,
    pub out_volume: f32,
    pub mic_device: String,
    pub out_device: String,
    pub noise: bool,
    pub echo: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            push_to_talk: false,
            ptt_held: false,
            threshold_db: -42.0,
            mic_gain: 1.0,
            out_volume: 1.0,
            mic_device: String::new(),
            out_device: String::new(),
            noise: true,
            echo: true,
        }
    }
}

#[derive(Default)]
struct Shared {
    call: Option<Room>,
    joining: Option<String>,
    note: Option<(String, i64)>,
    live: HashMap<String, Vec<Member>>,
    incoming: HashMap<String, Ring>,
    rejoining: bool,
    rejoin_at: i64,
    muted: bool,
    deafened: bool,
    mic_test_until: i64,
    settings: Settings,
    volumes: HashMap<String, f32>,
    volumes_loaded: bool,
    volumes_dirty: bool,
    net: Option<Net>,
    speaker_status: Option<String>,
    /// The Voice Chat module should be switched on (a call was accepted / started while it was off).
    enable_module: bool,
}

fn shared() -> &'static Mutex<Shared> {
    static S: OnceLock<Mutex<Shared>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(Shared::default()))
}

fn lock() -> MutexGuard<'static, Shared> {
    shared().lock().unwrap_or_else(|e| e.into_inner())
}

/// HTTP in order on one thread ("Native voice").
fn later(job: impl FnOnce() + Send + 'static) {
    static Q: OnceLock<Mutex<Sender<Box<dyn FnOnce() + Send>>>> = OnceLock::new();
    let q = Q.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Box<dyn FnOnce() + Send>>();
        let _ = std::thread::Builder::new().name("Native voice".into()).spawn(move || {
            for job in rx {
                job();
            }
        });
        Mutex::new(tx)
    });
    let _ = q.lock().unwrap_or_else(|e| e.into_inner()).send(Box::new(job));
}

fn post(path: &str, body: &Value) -> Result<Value, String> {
    let ticket = super::net::ticket().ok_or("Sign in to Native to use voice.")?;
    super::net::post_json(&format!("{}{path}", super::net::api()), Some(&ticket), body).map_err(http_text)
}

fn get(path: &str) -> Result<Value, String> {
    let ticket = super::net::ticket().ok_or("Sign in to Native to use voice.")?;
    super::net::get_json(&format!("{}{path}", super::net::api()), Some(&ticket)).map_err(http_text)
}

fn http_text(e: super::net::HttpError) -> String {
    match e {
        super::net::HttpError::Status(_, Some(m)) if !m.is_empty() => m,
        _ => "Voice is unavailable right now.".to_owned(),
    }
}

fn str_of(o: &Value, k: &str) -> Option<String> {
    o.get(k).and_then(Value::as_str).map(str::to_owned)
}

fn bool_of(o: &Value, k: &str) -> bool {
    o.get(k).and_then(Value::as_bool).unwrap_or(false)
}

fn unbase64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => return None,
        };
        acc = acc << 6 | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

fn notice(key: &str, title: &str, body: &str, name: &str, skin: Option<String>) {
    relay::push_notice(Notice { conv_key: Some(key.to_owned()), title: title.to_owned(), body: body.to_owned(), avatar_name: name.to_owned(), skin, at: now() });
}

/// "dm:a:b" -> "dm:<the other>", "g:id" -> "g:id"; None for anything else.
pub fn conv_key(room: &str, me: Option<&str>) -> Option<String> {
    if room.starts_with("g:") {
        return Some(room.to_owned());
    }
    let ids: Vec<&str> = room.strip_prefix("dm:")?.split(':').collect();
    let me = me?;
    if ids.len() != 2 {
        return None;
    }
    Some(format!("dm:{}", if ids[0] == me { ids[1] } else { ids[0] }))
}

/// True when somebody other than you is in the roster.
pub fn has_others(members: &[Member], me: Option<&str>) -> bool {
    members.iter().any(|m| me != Some(m.user_id.as_str()))
}

/// A private call is over when someone else was in it and you are now the only one left. Group calls carry on, and a
/// call nobody has joined yet is still ringing, not over.
pub fn call_over(key: Option<&str>, members: &[Member], me: Option<&str>, saw_others: bool) -> bool {
    key.is_some_and(|k| k.starts_with("dm:")) && saw_others && !has_others(members, me)
}

/// The link worked (`alive_at` is not 0) and has been silent for [`LOST_MS`]: try again every 5 s.
pub fn should_rejoin(alive_at: i64, now: i64, busy: bool, last_try: i64) -> bool {
    !busy && alive_at != 0 && now - alive_at > LOST_MS && now - last_try > LOST_MS
}

fn me() -> Option<String> {
    relay::me_id()
}

// ── per-player volume (config/native-voice.json, by Native user id, shared with the Java mod) ──

fn volumes_file() -> Option<std::path::PathBuf> {
    super::game_dir().map(|d| d.join("config").join("native-voice.json"))
}

fn load_volumes(s: &mut Shared) {
    if s.volumes_loaded {
        return;
    }
    s.volumes_loaded = true;
    let Some(text) = volumes_file().and_then(|f| std::fs::read_to_string(f).ok()) else { return };
    if let Ok(v) = serde_json::from_str::<Value>(&text)
        && let Some(o) = v.get("volumes").and_then(Value::as_object)
    {
        for (id, v) in o {
            if let Some(f) = v.as_f64() {
                s.volumes.insert(id.clone(), (f as f32).clamp(0.0, 2.0));
            }
        }
    }
}

/// Volume for one player, 0 - 2 (1 = as sent).
pub fn volume(user_id: &str) -> f32 {
    let mut s = lock();
    load_volumes(&mut s);
    s.volumes.get(user_id).copied().unwrap_or(1.0)
}

pub fn set_volume(user_id: &str, v: f32) {
    let mut s = lock();
    load_volumes(&mut s);
    let v = v.clamp(0.0, 2.0);
    if (v - 1.0).abs() < 0.001 {
        s.volumes.remove(user_id);
    } else {
        s.volumes.insert(user_id.to_owned(), v);
    }
    if !s.volumes_dirty {
        s.volumes_dirty = true;
        later(|| {
            std::thread::sleep(std::time::Duration::from_millis(800));
            let map = {
                let mut s = lock();
                s.volumes_dirty = false;
                s.volumes.clone()
            };
            if let Some(f) = volumes_file() {
                let _ = f.parent().map(std::fs::create_dir_all);
                let body = json!({ "volumes": map });
                if let Err(e) = std::fs::write(&f, body.to_string()) {
                    log::debug!("Native voice: volumes not saved: {e}");
                }
            }
        });
    }
}

/// Every speaker's volume by their seat in your call, for the mixer.
pub fn volumes_by_sid() -> HashMap<u32, f32> {
    let mut s = lock();
    load_volumes(&mut s);
    let Some(call) = &s.call else { return HashMap::new() };
    call.members.iter().map(|m| (m.sid, s.volumes.get(&m.user_id).copied().unwrap_or(1.0))).collect()
}

// ── state for the UI ──────────────────────────────────────────────────────

pub fn call() -> Option<Room> {
    lock().call.clone()
}

pub fn in_any_call() -> bool {
    lock().call.is_some()
}

pub fn in_call(key: &str) -> bool {
    lock().call.as_ref().is_some_and(|c| c.key == key)
}

pub fn joining(key: &str) -> bool {
    lock().joining.as_deref() == Some(key)
}

/// Members of a call in a conversation (yours or someone else's), empty when none.
pub fn call_members(key: &str) -> Vec<Member> {
    let s = lock();
    match &s.call {
        Some(c) if c.key == key => c.members.clone(),
        _ => s.live.get(key).cloned().unwrap_or_default(),
    }
}

/// The last problem or "declined" note, fading after ten seconds.
pub fn note() -> Option<String> {
    let mut s = lock();
    if s.note.as_ref().is_some_and(|(_, until)| now() > *until) {
        s.note = None;
    }
    s.note.as_ref().map(|(t, _)| t.clone())
}

fn set_note(s: &mut Shared, text: &str) {
    s.note = Some((text.to_owned(), now() + 10_000));
}

/// Why the call has no audio right now, or None when it is fine / still connecting.
pub fn link_problem(key: &str) -> Option<String> {
    let s = lock();
    let r = s.call.as_ref().filter(|c| c.key == key)?;
    let t = now();
    if s.rejoining || r.lost_since != 0 || r.last_pong != 0 && t - r.last_pong > LOST_MS {
        return Some("Reconnecting to the call...".to_owned());
    }
    (r.last_pong == 0 && t - r.joined_at > 6000).then(|| "Can't reach the voice server. A firewall or VPN may be blocking UDP.".to_owned())
}

pub fn muted() -> bool {
    lock().muted
}

pub fn deafened() -> bool {
    lock().deafened
}

pub fn mic_error() -> Option<String> {
    audio::with_meter(|m| m.mic_error.clone())
}

pub fn speaker_error() -> Option<String> {
    audio::with_meter(|m| m.speaker_error.clone())
}

pub fn self_speaking() -> bool {
    audio::with_meter(|m| m.self_speaking)
}

/// Microphone level in dBFS (after the mic volume and the cleaning), -100 when silent / closed.
pub fn mic_db() -> f64 {
    let l = audio::with_meter(|m| m.self_level);
    if l <= 1e-5 { -100.0 } else { 20.0 * f64::from(l).log10() }
}

/// True while this member's voice is coming in (you: while you are being sent).
pub fn speaking(m: &Member) -> bool {
    if me().as_deref() == Some(m.user_id.as_str()) {
        return self_speaking();
    }
    audio::with_meter(|meter| meter.remote.get(&m.sid).is_some_and(|(_, at)| at.is_some_and(|t| t.elapsed().as_millis() < 250)))
}

/// How loud this member is right now, 0 - 1: the talking ring.
pub fn level(m: &Member) -> f32 {
    if me().as_deref() == Some(m.user_id.as_str()) {
        return audio::with_meter(|meter| if meter.self_speaking { (meter.self_level * 6.0).min(1.0) } else { 0.0 });
    }
    if !speaking(m) {
        return 0.0;
    }
    audio::with_meter(|meter| meter.remote.get(&m.sid).map_or(0.0, |(l, _)| (l * 6.0).min(1.0)))
}

/// The mic test is on screen: keep the microphone open (and the engine running) for a moment.
pub fn mic_test() {
    lock().mic_test_until = now() + 600;
    audio::start();
}

/// Plays a short chime through the chosen output device.
pub fn test_speakers() {
    let gain = {
        let mut s = lock();
        s.speaker_status = Some("Played: did you hear it?".to_owned());
        s.settings.out_volume.clamp(0.25, 1.0) * 0.4
    };
    audio::play_chime(gain);
}

/// "Played: did you hear it?" or a device problem; None before the first test.
pub fn speaker_status() -> Option<String> {
    speaker_error().or_else(|| lock().speaker_status.clone())
}

/// The module's settings, every frame.
pub fn configure(settings: Settings) {
    let mut s = lock();
    if s.settings != settings {
        s.settings = settings;
    }
}

/// True once after a call was accepted or started while the Voice Chat module was off: switch it on.
pub fn take_enable_request() -> bool {
    std::mem::take(&mut lock().enable_module)
}

// ── for the audio engine and the socket ───────────────────────────────────

/// What the audio engine should run right now.
pub fn wants() -> audio::Wants {
    let mut s = lock();
    let t = now();
    s.incoming.retain(|_, r| t <= r.until);
    let in_call = s.call.is_some();
    let st = &s.settings;
    audio::Wants {
        mic: in_call && !s.muted && !s.deafened || t < s.mic_test_until,
        speakers: in_call,
        ring: !s.incoming.is_empty(),
        mic_device: st.mic_device.clone(),
        out_device: st.out_device.clone(),
        mic_gain: st.mic_gain,
        out_volume: st.out_volume,
        push_to_talk: st.push_to_talk,
        ptt_held: st.ptt_held,
        threshold_db: st.threshold_db,
        muted: s.muted,
        deafened: s.deafened,
        noise: st.noise,
        echo: st.echo,
        in_call,
    }
}

fn send(s: &mut Shared, kind: u8, plain: &[u8]) {
    let Shared { call, net, .. } = s;
    if let (Some(r), Some(n)) = (call.as_mut(), net.as_ref()) {
        r.seq = r.seq.wrapping_add(1);
        n.send(kind, r.sid, r.seq, &r.secret, plain);
    }
}

/// One encoded frame of your voice to your call ([flags][opus]).
pub fn send_audio(packet: &[u8]) {
    send(&mut lock(), vnet::T_AUDIO, packet);
}

fn ping(s: &mut Shared) {
    let t = now();
    if let Some(r) = s.call.as_mut() {
        r.last_ping = t;
    }
    send(s, vnet::T_PING, &t.to_be_bytes());
}

/// A packet from the relay (the socket's thread).
fn on_packet(kind: u8, sid: u32, _seq: u32, raw: &[u8]) {
    let audio_frame = {
        let mut s = lock();
        let Some(r) = s.call.as_mut().filter(|r| r.sid == sid) else { return };
        let Some(plain) = vnet::open(raw, &r.secret, 1) else { return };
        if kind == vnet::T_PING && plain.len() >= 8 {
            let sent = i64::from_be_bytes(plain[..8].try_into().unwrap_or([0; 8]));
            r.rtt = (now() - sent).clamp(0, 99_999) as i32;
            r.last_pong = now();
            None
        } else if kind == vnet::T_AUDIO && plain.len() >= 9 {
            let from = u32::from_be_bytes([plain[0], plain[1], plain[2], plain[3]]);
            let seq = u32::from_be_bytes([plain[5], plain[6], plain[7], plain[8]]);
            Some((from, seq, plain[4], plain[9..].to_vec()))
        } else {
            None
        }
    };
    if let Some((from, seq, flags, opus)) = audio_frame {
        audio::push_audio(from, seq, flags, opus);
    }
}

fn keep_streams(s: &Shared) -> Vec<u32> {
    s.call.as_ref().map_or_else(Vec::new, |c| c.members.iter().map(|m| m.sid).collect())
}

// ── actions ───────────────────────────────────────────────────────────────

/// Takes a seat in a call (`resume`: the same call carrying on after the link dropped: no new ring).
fn join(key: &str, resume: bool) -> Result<Room, String> {
    me().ok_or("Sign in to Native to use voice.")?;
    let (muted, deafened) = {
        let s = lock();
        (s.muted, s.deafened)
    };
    let mut body = json!({ "room": key, "muted": muted, "deafened": deafened });
    if let Some(skin) = relay::me_skin().filter(|k| k.len() == 64 || k.starts_with("mj:")) {
        body["skin"] = json!(skin);
    }
    if resume {
        body["resume"] = json!(true);
    }
    let res = post("/v1/voice/join", &body)?;
    let secret: [u8; 16] = str_of(&res, "key").and_then(|k| unbase64(&k)).and_then(|k| k.try_into().ok()).ok_or("Voice is unavailable right now.")?;
    let host = str_of(&res, "host").ok_or("Voice is unavailable right now.")?;
    let port = res.get("port").and_then(Value::as_u64).ok_or("Voice is unavailable right now.")? as u16;
    let t = now();
    let room = Room {
        key: key.to_owned(),
        room: str_of(&res, "room").unwrap_or_default(),
        sid: res.get("sid").and_then(Value::as_u64).ok_or("Voice is unavailable right now.")? as u32,
        secret,
        seq: 0,
        members: members(res.get("members")),
        rtt: -1,
        last_pong: 0,
        last_ping: 0,
        joined_at: t,
        since: t,
        saw_others: false,
        resumed: false,
        carry_on: false,
        resumed_at: 0,
        lost_since: 0,
    };
    let same = lock().net.as_ref().is_some_and(|n| n.same_server(&host, port));
    if !same {
        let net = Net::open(&host, port, on_packet).map_err(|e| format!("Can't reach the voice server: {e}"))?;
        lock().net = Some(net);
    }
    audio::start();
    Ok(room)
}

/// Leaves a seat: a goodbye packet and `POST /v1/voice/leave`.
fn bye(r: &Room) {
    {
        let s = lock();
        if let Some(n) = s.net.as_ref() {
            let mut seq = r.seq;
            seq = seq.wrapping_add(1);
            n.send(vnet::T_BYE, r.sid, seq, &r.secret, &[]);
        }
    }
    if let Err(e) = post("/v1/voice/leave", &json!({ "sid": r.sid })) {
        log::debug!("Native voice: leave: {e}");
    }
    let keep = keep_streams(&lock());
    audio::keep_streams(&keep);
}

pub fn join_call(key: &str) {
    {
        let mut s = lock();
        if s.joining.is_some() {
            return;
        }
        s.incoming.remove(key);
        s.joining = Some(key.to_owned());
        s.note = None;
        s.enable_module = true; // calls need the Voice Chat module running (keep-alive, push to talk)
    }
    let key = key.to_owned();
    later(move || {
        let result = join(&key, false);
        let old = {
            let mut s = lock();
            s.joining = None;
            match result {
                Ok(mut r) => {
                    r.last_ping = now();
                    s.live.remove(&key);
                    let new_sid = r.sid;
                    let old = s.call.replace(r).filter(|o| o.sid != new_sid);
                    ping(&mut s);
                    old
                }
                Err(e) => {
                    s.note = Some((e, i64::MAX)); // stays until the next try
                    None
                }
            }
        };
        if let Some(old) = old {
            bye(&old);
        }
    });
}

pub fn leave_call() {
    if let Some(r) = lock().call.take() {
        later(move || bye(&r));
    }
}

pub fn toggle_mute() {
    let mut s = lock();
    s.muted = !s.muted;
    if !s.muted && s.deafened {
        s.deafened = false;
    }
    push_state(&s);
}

pub fn toggle_deafen() {
    let mut s = lock();
    s.deafened = !s.deafened;
    push_state(&s);
}

fn push_state(s: &Shared) {
    let Some(r) = &s.call else { return };
    let body = json!({ "sid": r.sid, "muted": s.muted, "deafened": s.deafened });
    later(move || {
        if let Err(e) = post("/v1/voice/state", &body) {
            log::debug!("Native voice: state: {e}");
        }
    });
}

/// Leaves everything (the module was switched off / the game is closing).
pub fn shutdown() {
    leave_call();
    later(|| {
        lock().net = None;
        audio::stop();
        audio::keep_streams(&[]);
    });
}

/// Every frame while the Voice Chat module is on: the keep-alive, taking the seat again after the link dropped,
/// ending a resumed private call nobody came back to.
pub fn tick() {
    let t = now();
    let mut guard = lock();
    let s: &mut Shared = &mut guard;
    if s.call.as_ref().is_some_and(|r| t - r.last_ping > 2000) {
        ping(s);
    }
    let (busy, last_try) = (s.rejoining, s.rejoin_at);
    let Some(r) = s.call.as_mut() else { return };
    if r.lost_since != 0 && r.last_pong > r.resumed_at {
        r.lost_since = 0; // the new seat answers: recovered
    }
    let lost = r.lost_since != 0 && t - r.lost_since > GIVE_UP_MS;
    let resume_over = r.carry_on && r.key.starts_with("dm:") && !r.saw_others && t - r.resumed_at > RESUME_WAIT_MS;
    let retake = should_rejoin(r.alive_at(), t, busy, last_try).then(|| r.clone());
    if lost {
        s.call = None;
        set_note(s, "Lost the voice call.");
        return;
    }
    if resume_over {
        drop(guard);
        ended_by_peer(None);
        return;
    }
    if let Some(old) = retake {
        s.rejoining = true;
        s.rejoin_at = t;
        drop(guard);
        rejoin(old);
    }
}

/// Takes the same call's seat again: the server forgot the old one (it restarted) or the network changed.
fn rejoin(old: Room) {
    later(move || {
        // a private call nobody answered yet rings again; one that was going (or a group call) just carries on
        let resume = old.saw_others || old.key.starts_with("g:");
        let result = join(&old.key, resume);
        let leftover = {
            let mut s = lock();
            s.rejoining = false;
            match result {
                Ok(mut r) => {
                    r.since = old.since;
                    r.resumed = true;
                    r.carry_on = resume;
                    r.resumed_at = now();
                    r.lost_since = if old.lost_since != 0 { old.lost_since } else { old.last_pong };
                    if s.call.as_ref().is_some_and(|c| c.sid == old.sid) {
                        s.call = Some(r);
                        ping(&mut s);
                        None
                    } else {
                        Some(r) // left (or switched) while reconnecting
                    }
                }
                Err(e) => {
                    log::debug!("Native voice: rejoin: {e}"); // the API is still restarting: tick() tries again
                    if let Some(c) = s.call.as_mut().filter(|c| c.sid == old.sid && c.lost_since == 0) {
                        c.lost_since = c.last_pong;
                    }
                    None
                }
            }
        };
        if let Some(r) = leftover {
            bye(&r);
        }
        let keep = keep_streams(&lock());
        audio::keep_streams(&keep);
    });
}

// ── private calls: ringing, accept, decline ───────────────────────────────

/// The newest call that is ringing, or None.
pub fn incoming() -> Option<Ring> {
    let t = now();
    let mut s = lock();
    s.incoming.retain(|_, r| t <= r.until);
    s.incoming.values().max_by_key(|r| r.at).cloned()
}

/// The ring for one conversation, or None.
pub fn ringing(key: &str) -> Option<Ring> {
    let t = now();
    lock().incoming.get(key).filter(|r| t <= r.until).cloned()
}

/// Conversation key of your private call that nobody has joined yet ("Calling..."), or None.
pub fn calling() -> Option<String> {
    let me = me();
    let s = lock();
    let r = s.call.as_ref()?;
    if !r.key.starts_with("dm:") || r.carry_on || has_others(&r.members, me.as_deref()) {
        return None;
    }
    Some(r.key.clone())
}

/// Picks up: joins the call (and switches Voice Chat on if it was off).
pub fn accept(key: &str) {
    {
        let mut s = lock();
        s.enable_module = true;
        s.incoming.remove(key);
    }
    join_call(key);
}

/// Says no: the caller's call ends too (the server tells their game).
pub fn decline(key: &str) {
    lock().incoming.remove(key);
    let body = json!({ "room": key });
    later(move || {
        if let Err(e) = post("/v1/voice/decline", &body) {
            log::debug!("Native voice: decline: {e}");
        }
    });
}

fn add_incoming(key: Option<String>, user_id: Option<String>, name: Option<String>, skin: Option<String>, timeout_ms: i64) {
    let Some(key) = key.filter(|k| k.starts_with("dm:")) else { return };
    let friend = relay::friend(&key[3..]);
    let (name, skin) = match friend {
        Some(f) => (f.display().to_owned(), f.skin.clone()),
        None => (name.unwrap_or_else(|| "Someone".to_owned()), skin),
    };
    {
        let mut s = lock();
        if s.call.as_ref().is_some_and(|c| c.key == key) {
            return;
        }
        let at = now();
        let until = at + timeout_ms.clamp(5000, 120_000);
        s.incoming.insert(key.clone(), Ring { key, user_id: user_id.unwrap_or_default(), name, skin, at, until });
    }
    audio::start(); // the ring tone
}

/// The other person left (or the server ended your side): you are taken out of the call too.
fn ended_by_peer(room: Option<&str>) {
    let ended = {
        let mut s = lock();
        let matches = s.call.as_ref().is_some_and(|c| room.is_none_or(|r| r == c.room));
        if !matches {
            return;
        }
        s.call.take()
    };
    let Some(ended) = ended else { return };
    let friend = ended.key.strip_prefix("dm:").and_then(relay::friend);
    let who = friend.as_ref().map_or("They".to_owned(), |f| f.display().to_owned());
    let text = if ended.key.starts_with("dm:") { format!("{who} left the call") } else { "The call ended".to_owned() };
    set_note(&mut lock(), &text);
    notice(&ended.key, "Call ended", &text, friend.as_ref().map_or("", |f| f.name.as_str()), friend.as_ref().and_then(|f| f.skin.clone()));
    later(move || bye(&ended));
}

// ── Relay events ──────────────────────────────────────────────────────────

/// A voice event from the mod event stream (relay.rs).
pub fn on_event(kind: &str, p: &Value) {
    match kind {
        "voice:state" => on_state(p),
        "voice:ring" => {
            let from = p.get("from");
            add_incoming(
                str_of(p, "room").and_then(|r| conv_key(&r, me().as_deref())),
                from.and_then(|f| str_of(f, "userId")),
                from.and_then(|f| str_of(f, "name")),
                from.and_then(|f| str_of(f, "skin")),
                p.get("timeoutMs").and_then(Value::as_i64).unwrap_or(45_000),
            );
        }
        "voice:ring-end" => {
            let Some(key) = str_of(p, "room").and_then(|r| conv_key(&r, me().as_deref())) else { return };
            let ring = lock().incoming.remove(&key);
            let reason = str_of(p, "reason").unwrap_or_default();
            if let Some(r) = ring
                && matches!(reason.as_str(), "cancelled" | "missed" | "no-answer")
            {
                notice(&key, "Missed call", &format!("{} tried to call you", r.name), &r.name, r.skin);
            }
        }
        "voice:declined" => on_declined(p),
        "voice:ended" => ended_by_peer(str_of(p, "room").as_deref()),
        _ => {}
    }
}

fn on_state(p: &Value) {
    let Some(room) = str_of(p, "room") else { return };
    let list = members(p.get("members"));
    let me = me();
    let key = conv_key(&room, me.as_deref());
    let mut ended = false;
    let mut ring_group: Option<(String, Option<String>)> = None;
    {
        let mut s = lock();
        let mine = s.call.as_ref().is_some_and(|c| c.room == room);
        if mine {
            let left = p.get("left").filter(|l| l.is_object());
            let reason = left.and_then(|l| str_of(l, "reason")).unwrap_or_default();
            let me_left = left.is_some_and(|l| str_of(l, "userId") == me) && reason != "left" && reason != "switched";
            if let Some(c) = s.call.as_mut() {
                c.members = list.clone();
                if me_left || call_over(Some(&c.key), &list, me.as_deref(), c.saw_others) {
                    ended = true;
                } else if has_others(&list, me.as_deref()) {
                    c.saw_others = true;
                }
            }
        } else if let Some(k) = key.clone() {
            if list.is_empty() {
                s.live.remove(&k);
            } else {
                s.live.insert(k.clone(), list.clone());
            }
            if let Some(started) = p.get("started").filter(|v| v.is_object())
                && str_of(started, "userId") != me
                && k.starts_with("g:")
            {
                ring_group = Some((k, str_of(started, "name"))); // private calls ring through voice:ring
            }
        }
    }
    if ended {
        ended_by_peer(Some(&room));
    }
    if let Some((k, who)) = ring_group {
        let title = relay::group(&k[2..]).map_or("Group call".to_owned(), |g| g.name);
        let body = match &who {
            Some(w) => format!("{w} started a voice call"),
            None => "Started a voice call".to_owned(),
        };
        notice(&k, &title, &body, who.as_deref().unwrap_or(""), None);
    }
    let keep = keep_streams(&lock());
    audio::keep_streams(&keep);
}

fn on_declined(p: &Value) {
    let Some(room) = str_of(p, "room") else { return };
    let ended = {
        let mut s = lock();
        if !s.call.as_ref().is_some_and(|c| c.room == room) {
            return;
        }
        s.call.take()
    };
    let Some(c) = ended else { return };
    let friend = c.key.strip_prefix("dm:").and_then(relay::friend);
    let by = p.get("by").and_then(|b| str_of(b, "name")).filter(|n| !n.is_empty());
    let who = friend.as_ref().map(|f| f.display().to_owned()).or(by).unwrap_or_else(|| "They".to_owned());
    let unanswered = str_of(p, "reason").as_deref() == Some("no-answer");
    let (title, line) = if unanswered { ("No answer", format!("{who} did not answer")) } else { ("Call declined", format!("{who} declined your call")) };
    set_note(&mut lock(), &line);
    notice(&c.key, title, &line, &who, friend.and_then(|f| f.skin));
    later(move || bye(&c));
}

/// After (re)connecting: which calls are already running in your conversations.
pub fn refresh_calls() {
    later(|| {
        let res = match get("/v1/voice/calls") {
            Ok(v) => v,
            Err(e) => {
                log::debug!("Native voice: calls: {e}");
                return;
            }
        };
        let me = me();
        let mut rings = Vec::new();
        {
            let mut s = lock();
            s.live.clear();
            for o in res.get("calls").and_then(Value::as_array).into_iter().flatten() {
                let Some(key) = str_of(o, "room").and_then(|r| conv_key(&r, me.as_deref())) else { continue };
                let mine = s.call.as_ref().is_some_and(|c| c.key == key);
                let list = members(o.get("members"));
                if !list.is_empty() && !mine {
                    s.live.insert(key.clone(), list);
                }
                if let Some(r) = o.get("ringing").filter(|v| v.is_object())
                    && !mine
                {
                    rings.push((key, str_of(r, "userId"), str_of(r, "name"), r.get("timeoutMs").and_then(Value::as_i64).unwrap_or(45_000)));
                }
            }
        }
        for (key, uid, name, timeout) in rings {
            add_incoming(Some(key), uid, name, None, timeout);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roster(ids: &[&str]) -> Vec<Member> {
        ids.iter().enumerate().map(|(i, id)| Member { sid: i as u32 + 1, user_id: (*id).to_owned(), ..Default::default() }).collect()
    }

    #[test]
    fn a_private_call_is_over_when_you_are_left_alone_after_someone_was_there() {
        let me = Some("me");
        assert!(call_over(Some("dm:bob"), &roster(&["me"]), me, true), "Bob left: the call is over");
        assert!(!call_over(Some("dm:bob"), &roster(&["me"]), me, false), "nobody joined yet: still ringing");
        assert!(!call_over(Some("dm:bob"), &roster(&["me", "bob"]), me, true), "both still there");
        assert!(!call_over(Some("g:squad"), &roster(&["me"]), me, true), "a group call carries on");
        assert!(call_over(Some("dm:bob"), &roster(&[]), me, true), "an empty roster after someone was there");
    }

    #[test]
    fn a_dropped_link_is_taken_again_only_after_it_worked() {
        let t = 100_000;
        assert!(should_rejoin(t - 6_000, t, false, 0), "silent for 6 s after working");
        assert!(!should_rejoin(t - 2_000, t, false, 0), "a short gap is a late pong");
        assert!(!should_rejoin(0, t, false, 0), "it never worked (UDP blocked): no loop");
        assert!(!should_rejoin(t - 6_000, t, true, 0), "already trying");
        assert!(!should_rejoin(t - 6_000, t, false, t - 1_000), "tried a second ago");
    }

    #[test]
    fn rooms_read_as_conversations() {
        assert_eq!(conv_key("dm:1:2", Some("1")).as_deref(), Some("dm:2"));
        assert_eq!(conv_key("dm:1:2", Some("2")).as_deref(), Some("dm:1"));
        assert_eq!(conv_key("g:squad", None).as_deref(), Some("g:squad"));
        assert_eq!(conv_key("dm:1:2", None), None, "a private room needs to know who you are");
        assert_eq!(conv_key("srv:mc.example.net", Some("1")), None, "no proximity rooms");
    }

    #[test]
    fn the_seat_key_reads_from_base64() {
        // what server/voice.js sends: crypto.randomBytes(16).toString('base64')
        assert_eq!(unbase64("AAECAwQFBgcICQoLDA0ODw=="), Some((0u8..16).collect::<Vec<u8>>()));
        assert_eq!(unbase64("not base64!"), None);
    }

    #[test]
    fn a_roster_entry_reads_like_the_relay_sends_it() {
        let v = json!({ "sid": 3_000_000_000u64, "userId": "7", "name": "Bob", "ign": "BobMC", "skin": null, "muted": true, "deafened": false, "connected": true });
        let m = Member::parse(&v).unwrap();
        assert_eq!(m.sid, 3_000_000_000, "sids are unsigned 32-bit");
        assert_eq!(m.shown(), "BobMC");
        assert!(m.muted && m.connected && !m.deafened);
    }
}
