//! Native's authenticated emote playback. Network work stays off the render thread.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use crate::native_emote_props::PropClip;
use std::time::{Duration, Instant};
const BONES: [&str; 6] = ["head", "body", "rightArm", "leftArm", "rightLeg", "leftLeg"];
const PARTS: [&str; 6] = [
    "head",
    "body",
    "right_arm",
    "left_arm",
    "right_leg",
    "left_leg",
];
#[derive(Clone)]
struct Clip {
    duration: f64,
    looping: bool,
    connected: bool,
    frames: Vec<(f64, [[f32; 6]; 6])>,
}
impl Clip {
    fn parse(v: &Value) -> Option<Self> {
        if v["format"] != 1 || v["rig"] != "minecraft-player" {
            return None;
        }
        let duration = v["durationMs"].as_f64()?;
        if !(1.0..=30000.0).contains(&duration) {
            return None;
        }
        let raw = v["frames"].as_array()?;
        if !(2..=1202).contains(&raw.len()) {
            return None;
        }
        let mut frames = Vec::new();
        let mut last = -1.0;
        for f in raw {
            let t = f["t"].as_f64()?;
            if !t.is_finite() || t <= last || t < 0.0 || t > duration {
                return None;
            }
            let mut pose = [[0.0; 6]; 6];
            for (i, bone) in BONES.iter().enumerate() {
                let values = f["pose"][bone].as_array()?;
                if values.len() != 6 {
                    return None;
                }
                for (j, value) in values.iter().enumerate() {
                    let n = value.as_f64()?;
                    if !n.is_finite() || n.abs() > if j < 3 { 100.0 } else { 256.0 } {
                        return None;
                    }
                    pose[i][j] = n as f32;
                }
            }
            frames.push((t, pose));
            last = t;
        }
        if frames.first()?.0 != 0.0 || last != duration {
            return None;
        }
        Some(Self {
            duration,
            looping: v["loop"].as_bool()?,
            connected: v["connected"].as_bool().unwrap_or(false),
            frames,
        })
    }
    fn sample(&self, elapsed: f64) -> [[f32; 6]; 6] {
        let t = if self.looping {
            elapsed.max(0.0) % self.duration
        } else {
            elapsed.clamp(0.0, self.duration)
        };
        let i = self.frames.partition_point(|f| f.0 <= t).saturating_sub(1);
        let a = &self.frames[i];
        let b = &self.frames[(i + 1).min(self.frames.len() - 1)];
        let k = ((t - a.0) / (b.0 - a.0).max(1.0)) as f32;
        let mut pose = std::array::from_fn(|bone| {
            std::array::from_fn(|axis| {
                let mut d = b.1[bone][axis] - a.1[bone][axis];
                if self.connected && axis < 3 {
                    d -= (d / std::f32::consts::TAU).round() * std::f32::consts::TAU;
                }
                a.1[bone][axis] + d * k
            })
        });
        if self.connected {
            connect_pose(&mut pose);
        }
        pose
    }
}
fn connect_pose(p: &mut [[f32; 6]; 6]) {
    fn rotate(r: [f32; 6], v: [f32; 3]) -> [f32; 3] {
        let (sx, cx) = r[0].sin_cos();
        let (sy, cy) = r[1].sin_cos();
        let (sz, cz) = r[2].sin_cos();
        let yy = cx * v[1] - sx * v[2];
        let zz = sx * v[1] + cx * v[2];
        let xx = cy * v[0] + sy * zz;
        [cz * xx - sz * yy, sz * xx + cz * yy, -sy * v[0] + cy * zz]
    }
    let hip: [f32; 3] = std::array::from_fn(
        |j| if j == 1 { 12.0 } else { 0.0 } + (p[4][j + 3] + p[5][j + 3]) * 0.5,
    );
    let bottom = rotate(p[1], [0.0, 12.0, 0.0]);
    let neck: [f32; 3] = std::array::from_fn(|j| hip[j] - bottom[j]);
    p[0][3..].copy_from_slice(&neck);
    p[1][3..].copy_from_slice(&neck);
    for b in 2..4 {
        let anchor = [if b == 2 { -5.0 } else { 5.0 }, 2.0, 0.0];
        let r = rotate(p[1], anchor);
        for j in 0..3 {
            p[b][j + 3] = neck[j] + r[j] - anchor[j];
        }
    }
    let mut side: [f32; 3] =
        std::array::from_fn(|j| if j == 0 { -3.8 } else { 0.0 } + p[4][j + 3] - p[5][j + 3]);
    let mut length = side.iter().map(|v| v * v).sum::<f32>().sqrt();
    if length < 0.00001 {
        side = [-3.8, 0.0, 0.0];
        length = 3.8;
    }
    for b in 4..6 {
        for j in 0..3 {
            let anchor = if j == 0 {
                if b == 4 { -1.9 } else { 1.9 }
            } else if j == 1 {
                12.0
            } else {
                0.0
            };
            p[b][j + 3] =
                hip[j] + if b == 4 { 1.0 } else { -1.0 } * side[j] * 1.9 / length - anchor;
        }
    }
}
#[derive(Clone)]
struct Playback {
    hash: String,
    uuid: Option<String>,
    server_start: i64,
    start: Instant,
    end: Instant,
}
#[derive(Default)]
struct State {
    scope: String,
    names: HashMap<u32, (String, Option<String>)>,
    clips: HashMap<String, Clip>,
    props: HashMap<String, Arc<PropClip>>,
    playing: HashMap<String, Playback>,
    commands: VecDeque<Value>,
    me: Value,
    canceled: HashMap<String, Instant>,
    position: Option<[f64; 3]>,
    local: String,
    fetching: HashSet<String>,
    failed_fetch: HashMap<String, Instant>,
    preview_times: HashMap<String, f64>,
    waiting: HashSet<String>,
    command_gen: u64,
    pending: bool,
}
// Gameplay always traverses the clip once. Retain the server event until it
// expires so polling the same event cannot restart a completed animation.
fn playback_visible(s: &State, name: &str, now: Instant) -> bool {
    let Some(p) = s.playing.get(name) else { return false };
    let Some(clip) = s.clips.get(&p.hash) else { return false };
    !s.waiting.contains(name) && now < p.end
        && now.saturating_duration_since(p.start).as_secs_f64() * 1000.0 < clip.duration
}
fn state() -> &'static Mutex<State> {
    static S: OnceLock<Mutex<State>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(State::default()))
}
fn commands_ready() -> &'static Condvar {
    static WAKE: OnceLock<Condvar> = OnceLock::new();
    WAKE.get_or_init(Condvar::new)
}
fn lock() -> std::sync::MutexGuard<'static, State> {
    state().lock().unwrap_or_else(|e| e.into_inner())
}
pub fn scope(address: &str) -> String {
    let address = address.trim().to_lowercase();
    let normalized = address.strip_suffix(":25565").unwrap_or(&address);
    format!("{:x}", Sha256::digest(format!("server:{normalized}")))
}
pub fn tick(
    scope: String,
    names: Vec<(u32, String, Option<String>)>,
    local: &str,
    position: [f64; 3],
    cancel: bool,
) {
    static START: std::sync::Once = std::sync::Once::new();
    START.call_once(|| {
        let _ = std::thread::Builder::new()
            .name("Native emote commands".into()).spawn(run_commands);
        let _ = std::thread::Builder::new()
            .name("Native emotes".into())
            .spawn(run);
    });
    let mut s = lock();
    if s.scope != scope {
        s.playing.clear();
        s.commands.clear();
        s.command_gen = s.command_gen.wrapping_add(1);
        s.pending = false;
        s.waiting.clear();
        s.position = None;
        s.scope = scope;
    }
    let moved = s.position.is_some_and(|p| {
        p.iter()
            .zip(position)
            .map(|(a, b)| (a - b) * (a - b))
            .sum::<f64>()
            > 0.0016
    });
    s.names = names
        .into_iter()
        .map(|(id, n, u)| (id, (n.to_lowercase(), u)))
        .collect();
    s.local = local.to_lowercase();
    let local_name = local.to_lowercase();
    let active = playback_visible(&s, &local_name, Instant::now()) || s.waiting.contains(&local_name);
    if (cancel || moved) && active {
        stop_locked(&mut s);
    }
    if !playback_visible(&s, &local_name, Instant::now()) && !s.waiting.contains(&local_name) {
        s.position = Some(position);
    }
}
fn stop_locked(s: &mut State) {
    s.playing.remove(&s.local);
    s.canceled.insert(s.local.clone(), Instant::now());
    let local = s.local.clone();
    s.waiting.remove(&local);
    s.command_gen = s.command_gen.wrapping_add(1);
    s.pending = false;
    s.commands.clear();
    s.commands.push_back(serde_json::json!({"stop":true}));
    commands_ready().notify_one();
}
pub fn stop() {
    stop_locked(&mut lock());
}
pub fn snapshot() -> Value {
    lock().me.clone()
}
pub fn play(page: usize, slot: usize) -> bool {
    let mut s = lock();
    if s.scope.is_empty()
        || s.commands.len() > 8
        || slot >= 8
        || page >= s.me["usablePages"].as_u64().unwrap_or(1) as usize
        || s.me["pages"][page][slot].as_str().is_none()
    {
        return false;
    }
    let id = s.me["pages"][page][slot].as_str().unwrap_or("");
    let hash = s.me["catalog"].as_array().and_then(|items| items.iter()
        .find(|item| item["id"].as_str() == Some(id)))
        .and_then(|item| item["hash"].as_str()).map(str::to_owned);
    if let Some(hash) = hash {
        let local = s.local.clone();
        if !local.is_empty() {
            let now = Instant::now();
            let duration = s.clips.get(&hash).map(|c| if c.looping { 30000.0 } else { c.duration });
            if duration.is_none() { s.waiting.insert(local.clone()); } else { s.waiting.remove(&local); }
            s.playing.insert(local, Playback { hash, uuid: None, server_start: 0,
                start: now, end: now + Duration::from_millis(duration.unwrap_or(30000.0) as u64) });
        }
    }
    let scope = s.scope.clone();
    s.command_gen = s.command_gen.wrapping_add(1);
    s.pending = true;
    s.commands.clear();
    s.commands.push_back(serde_json::json!({"page":page,"slot":slot,"scope":scope}));
    commands_ready().notify_one();
    true
}
fn identity_matches(id: u32, rendered: Option<&str>, authenticated: Option<&str>) -> bool {
    id == 0 || rendered.is_none_or(|u| u.chars().nth(12) == Some('3') || Some(u) == authenticated)
}
/// Render an active emote facing the player's interpolated look direction.
/// Only horizontal yaw is used: looking up or down must not tilt the whole model.
pub fn facing_yaw(previous_head: f32, head: f32, alpha: f32) -> f32 {
    previous_head + crate::mobs::living::wrap(head - previous_head) * alpha
}

/// True only while a validated, loaded emote actually poses this rendered player.
pub fn is_playing(id: u32) -> bool {
    let s = lock();
    let Some((name, uuid)) = s.names.get(&id) else {
        return false;
    };
    s.playing.get(name).is_some_and(|p| {
        playback_visible(&s, name, Instant::now())
            && identity_matches(id, uuid.as_deref(), p.uuid.as_deref())
    })
}
/// Shared validated prop clip and the same elapsed clock used by the body pose.
pub fn prop_frame(id: u32) -> Option<(String, Arc<PropClip>, u64)> {
    let s = lock();
    let (name, uuid) = s.names.get(&id)?;
    let active = s.playing.get(name)?;
    let now = Instant::now();
    if !playback_visible(&s, name, now) ||
        !identity_matches(id, uuid.as_deref(), active.uuid.as_deref()) { return None; }
    let props = s.props.get(&active.hash)?.clone();
    Some((active.hash.clone(), props, now.saturating_duration_since(active.start).as_millis().min(u64::MAX as u128) as u64))
}
fn parsed_props(value: &Value, clip: &Clip) -> Option<Option<Arc<PropClip>>> {
    match value.get("props") {
        None => Some(None),
        Some(v) => {
            let mut props=PropClip::parse(v, clip.duration as u32, clip.looping)?;
            // PNG decoding is bounded by the validated dimensions and runs on the asset worker.
            props.decode_textures(|png| crate::hud::decode_bytes(png).ok())?;
            Some(Some(Arc::new(props)))
        },
    }
}
pub fn apply(id: u32, rest: [Option<[f32; 9]>; 6], p: &mut crate::mobs::anim::Poses) {
    let s = lock();
    let Some((name, uuid)) = s.names.get(&id) else {
        return;
    };
    let Some(active) = s.playing.get(name) else {
        return;
    };
    let now = Instant::now();
    if !playback_visible(&s, name, now) {
        return;
    }
    // The local model is trusted from our own ticket; nearby premium players must match UUIDs.
    if !identity_matches(id, uuid.as_deref(), active.uuid.as_deref()) {
        return;
    }
    let Some(clip) = s.clips.get(&active.hash) else {
        return;
    };
    let pose = clip.sample(now.saturating_duration_since(active.start).as_secs_f64() * 1000.0);
    for i in 0..6 {
        if let Some(mut v) = rest[i] {
            for j in 0..3 {
                v[j] += pose[i][j + 3];
                v[j + 3] = pose[i][j];
            }
            *p.get(PARTS[i]) = v;
        }
    }
    // 26.x skin layers are children of their body parts. Keep their local
    // poses: copying the parent here applies its transform a second time.
}

fn record_playback(s: &mut State, name: String, next: Playback) {
    if s.playing.get(&name).is_some_and(|old| {
        old.server_start > next.server_start
            || (old.server_start == next.server_start && old.hash == next.hash && old.uuid == next.uuid)
    }) {
        return;
    }
    let mut next = next;
    if let Some(old) = s.playing.get(&name)
        && old.server_start == 0 && old.hash == next.hash
    {
        next.start = old.start;
        if !s.waiting.contains(&name)
            && let Some(clip) = s.clips.get(&next.hash)
            && !clip.looping
        { next.end = old.start + Duration::from_millis(clip.duration as u64); }
    }
    s.playing.insert(name, next);
}
fn request(
    agent: &ureq::Agent,
    api: &str,
    ticket: &str,
    path: &str,
    body: Option<&Value>,
) -> Option<Value> {
    use std::io::Read;
    let url = format!("{api}{path}");
    let req = if body.is_some() {
        agent.post(&url)
    } else {
        agent.get(&url)
    }
    .set("Authorization", &format!("Bearer {ticket}"));
    let res = if let Some(v) = body {
        req.set("Content-Type", "application/json")
            .send_string(&v.to_string())
    } else {
        req.call()
    }
    .ok()?;
    let mut bytes = Vec::new();
    res.into_reader()
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 2 * 1024 * 1024 {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}
fn cache_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("NATIVE_GAME_DIR").map(std::path::PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .map(|dir| dir.join(".native").join("emotes"))
}
fn load_clip(api: &str, hash: &str) {
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) { return; }
    { let mut s = lock(); if s.clips.contains_key(hash) || s.fetching.len() >= 4 || s.failed_fetch.get(hash).is_some_and(|t| t.elapsed() < Duration::from_secs(20)) || !s.fetching.insert(hash.to_owned()) { return; } }
    let api = api.to_owned(); let hash = hash.to_owned(); let requested = hash.clone();
    let spawned = std::thread::Builder::new().name("Native emote asset".into()).spawn(move || {
        use std::io::Read;
        let path = cache_dir().map(|p| p.join("clips").join(format!("{hash}.json")));
        let valid = |raw: &[u8]| raw.len() <= 2 * 1024 * 1024 && format!("{:x}", Sha256::digest(raw)) == hash;
        let mut raw = path.as_ref().and_then(|p| std::fs::metadata(p).ok().filter(|m| m.len() <= 2 * 1024 * 1024).and_then(|_| std::fs::read(p).ok())).filter(|b| valid(b));
        if raw.is_none() {
            let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(15)).build();
            if let Ok(res) = agent.get(&format!("{api}/v1/emotes/clips/{hash}")).call() {
                let mut bytes = Vec::new();
                if res.into_reader().take(2 * 1024 * 1024 + 1).read_to_end(&mut bytes).is_ok() && valid(&bytes) { raw = Some(bytes); }
            }
        }
        let mut loaded = false;
        if let Some(raw) = raw && let Ok(value) = serde_json::from_slice::<Value>(&raw) && let Some(clip) = Clip::parse(&value) && let Some(props) = parsed_props(&value, &clip) {
            if let Some(path) = path && let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent); let _ = std::fs::write(path, raw);
            }
            let mut preview_time = clip.frames.iter().max_by(|a, b| {
                let score = |p: &[[f32; 6]; 6]| p.iter().flatten().map(|n| n*n).sum::<f32>();
                score(&a.1).total_cmp(&score(&b.1))
            }).map(|frame| frame.0.min(clip.duration - 0.001)).unwrap_or(0.0);
            if let Some(props)=&props {preview_time=props.preview_time(preview_time.max(0.0) as u64) as f64;}
            let mut s = lock();
            let names: Vec<String> = s.waiting.iter().filter(|name| s.playing.get(*name).is_some_and(|p| p.hash == hash)).cloned().collect();
            let now = Instant::now();
            for name in names {
                s.waiting.remove(&name);
                if let Some(p) = s.playing.get_mut(&name) { p.start = now; p.end = now + Duration::from_millis(if clip.looping { 30000 } else { clip.duration as u64 }); }
            }
            if s.clips.len() >= 256 { if let Some(key) = s.clips.keys().find(|key| !s.playing.values().any(|p| &p.hash == *key)).cloned() { s.clips.remove(&key); s.props.remove(&key); s.preview_times.remove(&key); } }
            // Keep decoded prop textures bounded. Evict inactive entries first; if every
            // slot is active, preserve body playback and omit the newest props rather than
            // allowing an unbounded crowd of uploads to exhaust memory.
            const PROP_BUDGET: usize = 64 * 1024 * 1024;
            if let Some(props) = props {
                let bytes=props.decoded_bytes();
                while s.props.values().map(|p| p.decoded_bytes()).sum::<usize>() + bytes > PROP_BUDGET {
                    let Some(old)=s.props.keys().find(|key| !s.playing.values().any(|p| &p.hash==*key && p.end>now)).cloned() else { break; };
                    s.props.remove(&old); s.clips.remove(&old); s.preview_times.remove(&old);
                }
                if s.props.values().map(|p| p.decoded_bytes()).sum::<usize>() + bytes <= PROP_BUDGET {
                    s.props.insert(hash.clone(),props);
                }
            } else { s.props.remove(&hash); }
            s.preview_times.insert(hash.clone(), preview_time);
            s.clips.insert(hash.clone(), clip);
            loaded = true;
        }
        let mut s = lock(); s.fetching.remove(&hash);
        s.failed_fetch.retain(|_, t| t.elapsed() < Duration::from_secs(20));
        if loaded { s.failed_fetch.remove(&hash); } else { s.failed_fetch.insert(hash, Instant::now()); }
    });
    if spawned.is_err() { lock().fetching.remove(&requested); }
}
fn cached_me(ticket: &str) -> Option<Value> {
    let path = cache_dir()?.join("me.json");
    if std::fs::metadata(&path).ok()?.len() > 2 * 1024 * 1024 { return None; }
    let wrapper: Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    if wrapper["ticketFingerprint"].as_str()? != format!("{:x}", Sha256::digest(ticket.as_bytes())) { return None; }
    wrapper.get("me").cloned()
}
fn save_me(ticket: &str, me: &Value) {
    if let Some(dir) = cache_dir() {
        let _ = std::fs::create_dir_all(&dir);
        let wrapper = serde_json::json!({"ticketFingerprint": format!("{:x}", Sha256::digest(ticket.as_bytes())), "me": me});
        if let Ok(bytes) = serde_json::to_vec(&wrapper) { let _ = std::fs::write(dir.join("me.json"), bytes); }
    }
}
pub fn local_name() -> String { lock().local.clone() }
// Hover previews repeat for every clip, regardless of its gameplay loop flag.
// Static previews keep their selected poster time. Both body and props call
// this function, so they always sample the same point in the animation.
fn preview_time(s: &State, hash: &str, elapsed: Option<f64>) -> Option<f64> {
    let clip = s.clips.get(hash)?;
    let time = match elapsed {
        Some(t) if t.is_finite() => t.max(0.0) % clip.duration,
        Some(_) => return None,
        None => *s.preview_times.get(hash).unwrap_or(&0.0),
    };
    time.is_finite().then_some(time.max(0.0))
}
pub fn preview_pose(hash: &str, elapsed: Option<f64>) -> Option<[[f32; 6]; 6]> {
    let s = lock();
    Some(s.clips.get(hash)?.sample(preview_time(&s, hash, elapsed)?))
}
/// The wheel samples props at exactly the same time as `preview_pose`.
pub fn preview_props(hash:&str,elapsed:Option<f64>)->Option<(String,Arc<PropClip>,u64)> {
    let s=lock();let props=s.props.get(hash)?.clone();
    let time=preview_time(&s,hash,elapsed)?;
    Some((hash.to_owned(),props,time as u64))
}
fn prefetch(api: &str, me: &Value) {
    if let (Some(pages), Some(items)) = (me["pages"].as_array(), me["catalog"].as_array()) {
        for id in pages.iter().take(me["usablePages"].as_u64().unwrap_or(1) as usize).filter_map(Value::as_array).flatten().filter_map(Value::as_str) {
            if let Some(hash) = items.iter().find(|item| item["id"].as_str() == Some(id)).and_then(|i| i["hash"].as_str()) { load_clip(api, hash); }
        }
    }
}

fn adopt(
    v: &Value,
    agent: &ureq::Agent,
    api: &str,
    _ticket: &str,
    scope: &str,
    requested: Instant,
) {
    let server = v["serverTime"].as_f64().unwrap_or(0.0);
    let events: Vec<Value> = v["events"]
        .as_array()
        .cloned()
        .unwrap_or_else(|| v.get("event").cloned().into_iter().collect());
    if v["events"].is_array() {
        let present: Vec<_> = events
            .iter()
            .filter_map(|e| e["name"].as_str().map(str::to_lowercase))
            .collect();
        let mut s = lock();
        if s.scope == scope {
            let local = s.local.clone(); let pending = s.pending;
            s.playing.retain(|name, p| present.contains(name) || p.start > requested || (pending && *name == local));
        }
    }
    for e in events {
        let Some(hash) = e["hash"].as_str().or_else(|| e["clipHash"].as_str()) else {
            continue;
        };
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        load_clip(api, hash);
        let Some(name) = e["name"].as_str() else {
            continue;
        };
        let name = name.to_lowercase();
        let elapsed = (server - e["startedAt"].as_f64().unwrap_or(server))
            .max(0.0)
            .min(30000.0);
        let remaining = (e["expiresAt"].as_f64().unwrap_or(0.0) - server).clamp(0.0, 30000.0);
        let mut s = lock();
        if s.scope != scope
            || (v["events"].is_array() && s.pending && name == s.local)
            || remaining <= 0.0
            || s.canceled.get(&name).is_some_and(|t| {
                *t >= requested
                    || Instant::now().duration_since(*t).as_secs_f64() * 1000.0 < elapsed
            })
        {
            continue;
        }
        record_playback(
            &mut s,
            name,
            Playback {
                hash: hash.into(),
                uuid: e["minecraftUuid"]
                    .as_str()
                    .map(|u| u.replace('-', "").to_lowercase()),
                server_start: e["startedAt"].as_i64().unwrap_or(0),
                start: Instant::now() - Duration::from_secs_f64(elapsed / 1000.0),
                end: Instant::now() + Duration::from_secs_f64(remaining / 1000.0),
            },
        );
    }
}
fn run_commands() {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(6)).build();
    loop {
        let (command, scope, generation) = {
            let mut s = lock();
            while s.commands.is_empty() { s = commands_ready().wait(s).unwrap_or_else(|e| e.into_inner()); }
            (s.commands.pop_front().unwrap(), s.scope.clone(), s.command_gen)
        };
        let Some((api, ticket)) = crate::discord_presence::native_emote_auth() else {
            let mut s = lock(); if s.command_gen == generation { s.pending = false; } continue;
        };
        let when = Instant::now(); let stop = command["stop"] == true;
        let result = request(&agent, &api, &ticket, if stop { "/v1/mod/emotes/stop" } else { "/v1/mod/emotes/play" }, Some(&command));
        let current = { let s = lock(); s.command_gen == generation && s.scope == scope }
            && crate::discord_presence::native_emote_auth().is_some_and(|(_, current)| current == ticket);
        if current && !stop {
            if let Some(v) = result { adopt(&v, &agent, &api, &ticket, &scope, when); }
            let mut s = lock(); if s.command_gen == generation { s.pending = false; }
        }
    }
}

fn run() {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(6))
        .build();
    let mut refresh = Instant::now() - Duration::from_secs(30);
    let mut previous = String::new();
    loop {
        if let Some((api, ticket)) = crate::discord_presence::native_emote_auth() {
            if ticket != previous {
                let mut s = lock();
                s.me = Value::Null;
                s.playing.clear(); s.waiting.clear(); s.commands.clear(); s.pending = false;
                s.command_gen = s.command_gen.wrapping_add(1);
                previous = ticket.clone();
                refresh = Instant::now() - Duration::from_secs(30);
                drop(s);
                if let Some(me) = cached_me(&ticket) { prefetch(&api, &me); lock().me = me; }
            }
            if refresh.elapsed() > Duration::from_secs(15) {
                if let Some(v) = request(&agent, &api, &ticket, "/v1/mod/emotes/me", None) {
                    if crate::discord_presence::native_emote_auth().is_some_and(|(_, current)| current == ticket) {
                        save_me(&ticket, &v); prefetch(&api, &v); lock().me = v;
                    }
                }
                refresh = Instant::now();
            }
            let current_me = { lock().me.clone() }; prefetch(&api, &current_me);
            let (scope, names) = { let s = lock(); (s.scope.clone(), s.names.values().map(|(n, _)| n.clone()).collect::<Vec<_>>()) };
            if !scope.is_empty() && !names.is_empty() {
                let when = Instant::now();
                if let Some(v) = request(
                    &agent,
                    &api,
                    &ticket,
                    &format!(
                        "/v1/mod/emotes/events?scope={scope}&players={}",
                        names
                            .into_iter()
                            .filter(|n| n.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_'))
                            .take(64)
                            .collect::<Vec<_>>()
                            .join(",")
                    ),
                    None,
                ) {
                    adopt(&v, &agent, &api, &ticket, &scope, when);
                }
            }
            let mut s = lock();
            s.playing.retain(|_, p| p.end > Instant::now());
            s.canceled
                .retain(|_, t| t.elapsed() < Duration::from_secs(35));
        } else {
            let mut s = lock();
            s.me = Value::Null;
            s.playing.clear();
            s.commands.clear(); s.waiting.clear(); s.pending = false;
            s.command_gen = s.command_gen.wrapping_add(1); previous.clear();
        }
        std::thread::sleep(Duration::from_millis(750));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn polling_preserves_the_clock_and_ignores_older_events() {
        let mut s = State::default();
        let now = Instant::now();
        let playback = |server_start, start| Playback {
            hash: "timing_probe".into(), uuid: None, server_start,
            start, end: now + Duration::from_secs(5),
        };
        record_playback(&mut s, "probe".into(), playback(1000, now));
        record_playback(&mut s, "probe".into(), playback(1000, now - Duration::from_millis(100)));
        assert_eq!(s.playing["probe"].start, now);
        record_playback(&mut s, "probe".into(), playback(900, now - Duration::from_millis(200)));
        assert_eq!(s.playing["probe"].server_start, 1000);
        record_playback(&mut s, "probe".into(), playback(2000, now));
        assert_eq!(s.playing["probe"].server_start, 2000);
    }
    #[test]
    fn facing_follows_horizontal_look_using_shortest_interpolated_turn() {
        assert_eq!(facing_yaw(10.0, 70.0, 0.0), 10.0);
        assert_eq!(facing_yaw(10.0, 70.0, 0.5), 40.0);
        assert_eq!(facing_yaw(10.0, 70.0, 1.0), 70.0);
        assert_eq!(facing_yaw(179.0, -179.0, 0.5), 180.0);
        assert_eq!(facing_yaw(-179.0, 179.0, 0.5), -180.0);
    }
    #[test]
    fn gameplay_finishes_once_but_hover_previews_repeat_every_clip() {
        let mut s = State::default();
        let now = Instant::now();
        let make_clip = |looping| Clip { duration:1000.0, looping, connected:false,
            frames:vec![(0.0,[[0.0;6];6]),(1000.0,[[1.0;6];6])] };
        for looping in [false,true] {
            let hash=if looping {"looping"} else {"once"};
            s.clips.insert(hash.into(),make_clip(looping));
            s.preview_times.insert(hash.into(),600.0);
            s.playing.insert(hash.into(),Playback {hash:hash.into(),uuid:None,server_start:1,
                start:now-Duration::from_millis(999),end:now+Duration::from_secs(30)});
            assert!(playback_visible(&s,hash,now));
            assert!(!playback_visible(&s,hash,now+Duration::from_millis(1)));
            assert!(s.playing.contains_key(hash),"keep event identity after playback");
            assert_eq!(preview_time(&s,hash,Some(1250.0)),Some(250.0));
            assert_eq!(preview_time(&s,hash,None),Some(600.0));
            assert!(preview_time(&s,hash,Some(f64::NAN)).is_none());
            assert!((s.clips[hash].sample(preview_time(&s,hash,Some(1250.0)).unwrap())[0][0]-0.25).abs()<1e-6);
        }
    }
    #[test]
    fn nested_skin_layers_keep_their_local_pose() {
        use crate::mobs::{Part, anim::Poses};
        let id = 700008;
        let name = "nested_skin_probe".to_string();
        let hash = "nested_skin_clip".to_string();
        let mut sleeve_pose = [0.0; 9];
        sleeve_pose[0] = 0.25;
        sleeve_pose[6..].fill(1.0);
        let model = Part {
            name: "root".into(), pose: [0.0; 9], polygons: vec![], children: vec![Part {
                name: "right_arm".into(), pose: [0.0,2.0,0.0,0.0,0.0,0.0,1.0,1.0,1.0], polygons: vec![],
                children: vec![Part {name:"right_sleeve".into(),pose:sleeve_pose,polygons:vec![],children:vec![]}],
            }],
        };
        let mut p = Poses::default();
        p.reset(&model);
        let rest = PARTS.map(|n| p.has(n).then(|| *p.get(n)));
        let mut pose = [[0.0; 6]; 6];
        pose[2][0] = 1.5;
        pose[2][4] = 4.0;
        let now = Instant::now();
        {
            let mut s = lock();
            s.names.insert(id, (name.clone(), None));
            s.playing.insert(name.clone(), Playback {hash:hash.clone(),uuid:None,server_start:0,start:now,end:now+Duration::from_secs(5)});
            s.clips.insert(hash.clone(), Clip {duration:1000.0,looping:true,connected:false,frames:vec![(0.0,pose),(1000.0,pose)]});
        }
        apply(id, rest, &mut p);
        assert_eq!(p.get("right_arm")[3], 1.5);
        assert_eq!(p.get("right_arm")[1], 6.0);
        assert_eq!(*p.get("right_sleeve"), sleeve_pose);
        let mut s = lock();
        s.names.remove(&id); s.playing.remove(&name); s.clips.remove(&hash);
    }
    #[test]
    fn interpolation_loop_and_validation() {
        let pose = |n: f64| {
            BONES
                .into_iter()
                .map(|b| (b.to_string(), serde_json::json!([n, 0, 0, 0, 0, 0])))
                .collect::<serde_json::Map<_, _>>()
        };
        let mut v = serde_json::json!({"format":1,"rig":"minecraft-player","durationMs":1000,"loop":true,"frames":[{"t":0,"pose":pose(0.0)},{"t":1000,"pose":pose(2.0)}]});
        let c = Clip::parse(&v).unwrap();
        assert!((c.sample(1250.0)[0][0] - 0.5).abs() < 1e-6);
        v["frames"][1]["t"] = serde_json::json!(0);
        assert!(Clip::parse(&v).is_none());
    }
    #[test]
    fn identity_cannot_impersonate_a_nearby_premium_player() {
        let premium = "123456781234423412341234567890ab";
        let offline = "123456781234323412341234567890ab";
        assert!(!identity_matches(4, Some(premium), None));
        assert!(identity_matches(4, Some(premium), Some(premium)));
        assert!(identity_matches(4, Some(offline), None));
        assert!(identity_matches(0, Some(premium), None));
    }
    #[test]
    fn hands_are_hidden_only_for_loaded_active_emotes() {
        let id = 700007;
        let name = "emote_cosmetic_probe".to_string();
        let hash = "cosmetic_probe_clip".to_string();
        let now = Instant::now();
        {
            let mut s = lock();
            s.names.insert(id, (name.clone(), None));
            s.playing.insert(
                name.clone(),
                Playback {
                    hash: hash.clone(),
                    uuid: None,
                    server_start: 0,
                    start: now,
                    end: now + Duration::from_secs(10),
                },
            );
        }
        assert!(
            !is_playing(id),
            "an unloaded clip must keep hand items visible"
        );
        lock().clips.insert(
            hash.clone(),
            Clip {
                duration: 1000.0,
                looping: true,
                connected: true,
                frames: vec![(0.0, [[0.0; 6]; 6]), (1000.0, [[0.0; 6]; 6])],
            },
        );
        assert!(is_playing(id));
        lock().playing.get_mut(&name).unwrap().end = now - Duration::from_secs(1);
        assert!(
            !is_playing(id),
            "hand items return as soon as playback expires"
        );
        let mut s = lock();
        s.names.remove(&id);
        s.playing.remove(&name);
        s.clips.remove(&hash);
    }
    #[test]
    fn connected_rig_keeps_neck_and_shoulders_attached() {
        let mut pose = [[0.0; 6]; 6];
        pose[1][0] = std::f32::consts::FRAC_PI_2;
        connect_pose(&mut pose);
        assert!((pose[1][4] - 12.0).abs() < 0.0001);
        assert!((pose[1][5] + 12.0).abs() < 0.0001);
        assert_eq!(&pose[0][3..], &pose[1][3..]);
        assert!((pose[2][4] - 10.0).abs() < 0.0001);
        assert!((pose[2][5] + 10.0).abs() < 0.0001);
    }
    #[test]
    fn scope_normalizes_default_port() {
        assert_eq!(scope("Example.org:25565"), scope("example.org"));
        assert_ne!(scope("example.org:25566"), scope("example.org"));
    }
}
