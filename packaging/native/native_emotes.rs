//! Native's authenticated emote playback. Network work stays off the render thread.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};
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
    start: Instant,
    end: Instant,
}
#[derive(Default)]
struct State {
    scope: String,
    names: HashMap<u32, (String, Option<String>)>,
    clips: HashMap<String, Clip>,
    playing: HashMap<String, Playback>,
    commands: VecDeque<Value>,
    me: Value,
    canceled: HashMap<String, Instant>,
    position: Option<[f64; 3]>,
    local: String,
}
fn state() -> &'static Mutex<State> {
    static S: OnceLock<Mutex<State>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(State::default()))
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
            .name("Native emotes".into())
            .spawn(run);
    });
    let mut s = lock();
    if s.scope != scope {
        s.playing.clear();
        s.commands.clear();
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
    if (cancel || moved) && s.playing.contains_key(&local.to_lowercase()) {
        stop_locked(&mut s);
    }
    if !s.playing.contains_key(&local.to_lowercase()) {
        s.position = Some(position);
    }
}
fn stop_locked(s: &mut State) {
    s.playing.remove(&s.local);
    s.canceled.insert(s.local.clone(), Instant::now());
    s.commands.push_back(serde_json::json!({"stop":true}));
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
    let scope = s.scope.clone();
    s.commands
        .push_back(serde_json::json!({"page":page,"slot":slot,"scope":scope}));
    true
}
fn identity_matches(id: u32, rendered: Option<&str>, authenticated: Option<&str>) -> bool {
    id == 0 || rendered.is_none_or(|u| u.chars().nth(12) == Some('3') || Some(u) == authenticated)
}
/// True only while a validated, loaded emote actually poses this rendered player.
pub fn is_playing(id: u32) -> bool {
    let s = lock();
    let Some((name, uuid)) = s.names.get(&id) else {
        return false;
    };
    s.playing.get(name).is_some_and(|p| {
        p.end > Instant::now()
            && s.clips.contains_key(&p.hash)
            && identity_matches(id, uuid.as_deref(), p.uuid.as_deref())
    })
}
pub fn apply(id: u32, rest: [Option<[f32; 9]>; 6], p: &mut crate::mobs::anim::Poses) {
    let s = lock();
    let Some((name, uuid)) = s.names.get(&id) else {
        return;
    };
    let Some(active) = s.playing.get(name) else {
        return;
    };
    if Instant::now() >= active.end {
        return;
    }
    // The local model is trusted from our own ticket; nearby premium players must match UUIDs.
    if !identity_matches(id, uuid.as_deref(), active.uuid.as_deref()) {
        return;
    }
    let Some(clip) = s.clips.get(&active.hash) else {
        return;
    };
    let pose = clip.sample(active.start.elapsed().as_secs_f64() * 1000.0);
    for i in 0..6 {
        if let Some(mut v) = rest[i] {
            for j in 0..3 {
                v[j] += pose[i][j + 3];
                v[j + 3] = pose[i][j];
            }
            *p.get(PARTS[i]) = v;
        }
    }
    for (outer, inner) in [
        ("hat", "head"),
        ("jacket", "body"),
        ("right_sleeve", "right_arm"),
        ("left_sleeve", "left_arm"),
        ("right_pants", "right_leg"),
        ("left_pants", "left_leg"),
    ] {
        if p.has(outer) && p.has(inner) {
            let v = *p.get(inner);
            *p.get(outer) = v;
        }
    }
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
            s.playing
                .retain(|name, p| present.contains(name) || p.start > requested);
        }
    }
    for e in events {
        let Some(hash) = e["hash"].as_str().or_else(|| e["clipHash"].as_str()) else {
            continue;
        };
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        if !lock().clips.contains_key(hash) {
            use std::io::Read;
            if let Ok(res) = agent.get(&format!("{api}/v1/emotes/clips/{hash}")).call() {
                let mut raw = Vec::new();
                if res
                    .into_reader()
                    .take(2 * 1024 * 1024 + 1)
                    .read_to_end(&mut raw)
                    .is_ok()
                    && raw.len() <= 2 * 1024 * 1024
                    && format!("{:x}", Sha256::digest(&raw)) == hash
                    && let Ok(v) = serde_json::from_slice::<Value>(&raw)
                    && let Some(clip) = Clip::parse(&v)
                {
                    let mut s = lock();
                    if s.clips.len() >= 256 {
                        s.clips.clear();
                    }
                    s.clips.insert(hash.into(), clip);
                }
            }
        }
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
            || remaining <= 0.0
            || s.canceled.get(&name).is_some_and(|t| {
                *t >= requested
                    || Instant::now().duration_since(*t).as_secs_f64() * 1000.0 < elapsed
            })
        {
            continue;
        }
        s.playing.insert(
            name,
            Playback {
                hash: hash.into(),
                uuid: e["minecraftUuid"]
                    .as_str()
                    .map(|u| u.replace('-', "").to_lowercase()),
                start: Instant::now() - Duration::from_secs_f64(elapsed / 1000.0),
                end: Instant::now() + Duration::from_secs_f64(remaining / 1000.0),
            },
        );
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
                s.playing.clear();
                previous = ticket.clone();
                refresh = Instant::now() - Duration::from_secs(30);
            }
            if refresh.elapsed() > Duration::from_secs(15) {
                if let Some(v) = request(&agent, &api, &ticket, "/v1/mod/emotes/me", None) {
                    lock().me = v;
                }
                refresh = Instant::now();
            }
            let (scope, names, commands) = {
                let mut s = lock();
                (
                    s.scope.clone(),
                    s.names.values().map(|(n, _)| n.clone()).collect::<Vec<_>>(),
                    s.commands.drain(..).collect::<Vec<_>>(),
                )
            };
            for command in commands {
                let when = Instant::now();
                let stop = command["stop"] == true;
                if let Some(v) = request(
                    &agent,
                    &api,
                    &ticket,
                    if stop {
                        "/v1/mod/emotes/stop"
                    } else {
                        "/v1/mod/emotes/play"
                    },
                    Some(&command),
                ) {
                    if !stop {
                        adopt(&v, &agent, &api, &ticket, &scope, when);
                    }
                }
            }
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
            s.commands.clear();
        }
        std::thread::sleep(Duration::from_millis(750));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
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
