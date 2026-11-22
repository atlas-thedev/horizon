//! Markers mod (Lunar Client style, 100% parity with Java `MarkersModule.java`): drop real-time 3D world pings
//! on any block, mob, player, or waypoint and sync them live with teammates over Native Relay.
//!
//! Quick tap (<200 ms) of the Marker key drops a Normal Marker; holding it (>=200 ms) opens a 4-quadrant radial
//! selection wheel (Top=Normal diamond, Right=Danger warning triangle with `!`, Bottom=`!` blue-on-dark diamond,
//! Left=`?` amber-on-dark diamond) steered by yaw/pitch delta while held. Middle-clicking a marker you placed
//! removes it; Clear Markers removes all. Integrates directly with the Direction HUD strip and supports
//! customizable owner/description/distance/coordinate visibility, Compact Mode, custom colours, independent
//! self/other ding volumes (synthesized via `cpal`), and chat / Native client notifications.

use super::avatars;
use super::keys;
use super::mc_font;
use super::module::*;
use super::net;
use super::previews;
use super::relay;
use super::setting::Setting;
use super::waypoints::{self, Camera, Point};
use super::waypoints_module;
use horizon_client::native_ui::canvas::clock_ms;
use horizon_client::native_ui::{Canvas, font, theme};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

pub const ID: &str = "markers";
pub const NORMAL: usize = 0;
pub const DANGER: usize = 1;
pub const EXCLAMATION: usize = 2;
pub const QUESTION: usize = 3;
const KIND_NAMES: [&str; 4] = ["Marker", "Danger", "Objective", "Important"];
/// The picker's tile order, left to right.
const PICKER: [usize; 4] = [NORMAL, DANGER, EXCLAMATION, QUESTION];
/// Picker tiles: 4 of `TILE` px with `GAP` between them, centred under the crosshair.
const TILE: f32 = 58.0;
const GAP: f32 = 10.0;

/// The colour pins (packaging/native/icons/color) by kind: purple pin, yellow warning, green flag, blue star.
fn pin_art(kind: usize) -> Option<std::sync::Arc<horizon_client::native_ui::Image>> {
    use horizon_client::native_ui::{Image, assets};
    static ART: std::sync::OnceLock<[Option<std::sync::Arc<Image>>; 4]> = std::sync::OnceLock::new();
    ART.get_or_init(|| {
        [
            Image::decode(assets::ICON_MARKER_NORMAL),
            Image::decode(assets::ICON_MARKER_DANGER),
            Image::decode(assets::ICON_MARKER_FLAG),
            Image::decode(assets::ICON_MARKER_STAR),
        ]
    })
    .get(kind)
    .cloned()
    .flatten()
}
const HOLD_MS: f64 = 200.0;
pub const RANGE: f64 = 128.0;
const MAX_MARKERS: usize = 48;

#[derive(Clone, Debug)]
pub struct Marker {
    pub id: String,
    pub owner_id: Option<String>,
    pub owner: String,
    pub skin: Option<String>,
    pub kind: usize,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub dim: String,
    pub world: Option<String>,
    pub target_kind: String, // "block" | "entity" | "waypoint"
    pub target_name: String,
    pub target_icon: String, // bare item/block id e.g. "red_bed", "grass_block"
    pub own: bool,
    pub created_at: f64,
    pub expires_at: f64,
}

impl Marker {
    #[allow(clippy::too_many_arguments)]
    fn new(
        id: String,
        owner_id: Option<String>,
        owner: &str,
        skin: Option<String>,
        kind: usize,
        x: f64,
        y: f64,
        z: f64,
        dim: &str,
        world: Option<String>,
        target_kind: &str,
        target_name: &str,
        target_icon: &str,
        own: bool,
        duration_ms: f64,
    ) -> Self {
        let now = clock_ms();
        let dur = duration_ms.clamp(5000.0, 120_000.0);
        Marker {
            id,
            owner_id,
            owner: if owner.is_empty() { "Player".to_owned() } else { owner.to_owned() },
            skin,
            kind: kind.min(3),
            x,
            y,
            z,
            dim: if dim.is_empty() { waypoints::OVERWORLD.to_owned() } else { dim.to_owned() },
            world,
            target_kind: if target_kind.is_empty() { "block".to_owned() } else { target_kind.to_owned() },
            target_name: target_name.to_owned(),
            target_icon: target_icon.to_owned(),
            own,
            created_at: now,
            expires_at: now + dur,
        }
    }
}

#[derive(Clone)]
struct ConfigSnap {
    enabled: bool,
    duration: f32,
    share_party: bool,
    share_team: bool,
    share_friends: bool,
    self_vol: f32,
    other_vol: f32,
    chat_notice: bool,
    client_notice: bool,
    colors: [u32; 4],
    world: Option<String>,
    dim: String,
    team_name: Option<String>,
    player_name: String,
}

impl Default for ConfigSnap {
    fn default() -> Self {
        ConfigSnap {
            enabled: true,
            duration: 20.0,
            share_party: true,
            share_team: false,
            share_friends: false,
            self_vol: 75.0,
            other_vol: 75.0,
            chat_notice: false,
            client_notice: false,
            colors: [0xFFA6D8FF, 0xFFEA4B1F, 0xFF3B82F6, 0xFFF59E0B],
            world: None,
            dim: waypoints::OVERWORLD.to_owned(),
            team_name: None,
            player_name: "Player".to_owned(),
        }
    }
}

#[derive(Default)]
struct Shared {
    active: Vec<Marker>,
    cfg: ConfigSnap,
    chat_queue: Vec<String>,
}

fn shared() -> &'static Mutex<Shared> {
    static STATE: OnceLock<Mutex<Shared>> = OnceLock::new();
    STATE.get_or_init(Default::default)
}

pub fn all() -> Vec<Module> {
    vec![Module::new(
        ID,
        "Markers",
        "Allows you and your team to mark things in the world, like in various FPS games.",
        VISUAL,
        theme::I_MAP_PIN,
        true,
        vec![
            // Keybinds & controls
            Setting::key("key", "Marker keybind", -1),
            Setting::key("clearKey", "Clear markers keybind", -1),
            Setting::choice("placeAt", "Place markers at", 0, &["Where I look", "Where I stand"]),
            Setting::bool("middleRemove", "Middle-click to remove", true),
            // Scale, duration & animation
            Setting::num("scale", "Scale", 1.0, 0.5, 2.0, 0.05, "x"),
            Setting::num("duration", "Visible duration", 20.0, 5.0, 120.0, 5.0, " s"),
            Setting::bool("animate", "Animate marker", true),
            // Visibility (Always / On Hover / Off)
            Setting::choice("showOwner", "Show owner", 0, &["Always", "On Hover", "Off"]),
            Setting::choice("showDesc", "Show description", 1, &["Always", "On Hover", "Off"]),
            Setting::choice("showDist", "Show distance", 1, &["Always", "On Hover", "Off"]),
            Setting::choice("showCoords", "Show coordinates", 1, &["Always", "On Hover", "Off"]),
            // Display styling
            Setting::choice("ownerDisplay", "Owner display", 0, &["Head", "Name", "Both"]),
            Setting::text("ownerSuffix", "Owner suffix", "'s Marker", "'s Marker", 24),
            Setting::choice("descDisplay", "Description display", 0, &["Icon", "Name", "Both"]),
            Setting::bool("compact", "Compact mode", false),
            Setting::bool("background", "Background", false),
            Setting::bool("shadow", "Text shadow", true),
            // Sharing
            Setting::bool("shareParty", "Share with party & server teams", true),
            Setting::bool("shareTeam", "Share with vanilla scoreboard teams", false),
            Setting::bool("shareFriends", "Share with Native friends", false),
            // Sounds & notifications
            Setting::num("selfVol", "Ding volume (self)", 75.0, 0.0, 100.0, 5.0, "%"),
            Setting::num("otherVol", "Ding volume (others)", 75.0, 0.0, 100.0, 5.0, "%"),
            Setting::bool("chatNotice", "Chat notification", false),
            Setting::bool("clientNotice", "Native client notification", false),
            // Variant colours
            Setting::color("pinMarker", "Marker colour", 0xFFB07CFF),
            Setting::color("pinDanger", "Danger colour", 0xFFFBBF24),
            Setting::color("pinObjective", "Objective colour", 0xFF34D399),
            Setting::color("pinImportant", "Important colour", 0xFF60A5FA),
        ],
        None,
        Box::new(Markers::default()),
    )]
}

/// True when the Marker key is currently held so `mod.rs` performs an entity/block target raycast.
pub fn wants_target(modules: &super::modules::Modules, held: &HashSet<i32>) -> bool {
    modules
        .get(ID)
        .filter(|m| m.state.enabled)
        .and_then(|m| m.state.setting("key"))
        .is_some_and(|s| s.as_key() >= 0 && held.contains(&s.as_key()))
}

/// The Marker & Clear keybind codes when the module is enabled (so `mod.rs` can consume keyboard presses).
pub fn keybinds(modules: &super::modules::Modules) -> (i32, i32) {
    let Some(m) = modules.get(ID).filter(|m| m.state.enabled) else {
        return (-1, -1);
    };
    let k = m.state.setting("key").map_or(-1, Setting::as_key);
    let c = m.state.setting("clearKey").map_or(-1, Setting::as_key);
    (k, c)
}

/// Drains client-side chat notifications produced by markers (when `chatNotice` is enabled).
pub fn take_chat_notices() -> Vec<String> {
    let mut s = shared().lock().unwrap_or_else(|e| e.into_inner());
    std::mem::take(&mut s.chat_queue)
}

/// Places a waypoint as a live Normal Marker so teammates can see it immediately.
pub fn ping_waypoint(p: &Point) {
    place_at(
        NORMAL,
        f64::from(p.x) + 0.5,
        f64::from(p.y) + 0.5,
        f64::from(p.z) + 0.5,
        "waypoint",
        &p.name,
        "compass",
    );
}

/// Exports active markers for the Direction HUD strip: `(heading, kind, color)`.
pub fn compass(g: &GameView) -> Vec<(f32, usize, u32)> {
    let s = shared().lock().unwrap_or_else(|e| e.into_inner());
    if !s.cfg.enabled {
        return Vec::new();
    }
    if g.sample {
        return vec![
            (240.0, EXCLAMATION, s.cfg.colors[EXCLAMATION]),
            (270.0, NORMAL, s.cfg.colors[NORMAL]),
            (305.0, DANGER, s.cfg.colors[DANGER]),
        ];
    }
    let now = clock_ms();
    let mut out = Vec::new();
    for mk in &s.active {
        if now >= mk.expires_at {
            continue;
        }
        if let (Some(w), Some(mw)) = (&s.cfg.world, &mk.world)
            && w != mw
        {
            continue;
        }
        if s.cfg.dim != mk.dim {
            continue;
        }
        let (dx, dz) = (mk.x - g.x, mk.z - g.z);
        if dx * dx + dz * dz < 1.0 {
            continue;
        }
        out.push((waypoints::heading(g.x, g.z, mk.x, mk.z), mk.kind, s.cfg.colors[mk.kind.min(3)]));
    }
    out
}

/// Called by `relay.rs` when a teammate places a marker via SSE (`marker:place`).
pub fn on_remote_place(payload: &Value) {
    let mk = payload.get("marker").filter(|v| v.is_object()).unwrap_or(payload);
    let relation = payload.get("relation").and_then(Value::as_str).unwrap_or("");
    let cfg = {
        let s = shared().lock().unwrap_or_else(|e| e.into_inner());
        if !s.cfg.enabled {
            return;
        }
        s.cfg.clone()
    };
    if relation == "friend" && !cfg.share_friends {
        return;
    }
    if relation == "party" && !cfg.share_party {
        return;
    }
    if relation == "team" {
        if !cfg.share_team {
            return;
        }
        if let Some(r_team) = mk.get("team").and_then(Value::as_str).filter(|t| !t.is_empty())
            && let Some(my_team) = cfg.team_name.as_deref()
            && r_team != my_team
        {
            return;
        }
    }
    if let Some(r_scope) = mk.get("scope").and_then(Value::as_str).filter(|s| !s.is_empty())
        && let Some(my_scope) = scope_of(cfg.world.as_deref())
        && !r_scope.eq_ignore_ascii_case(&my_scope)
    {
        return;
    }
    let Some(x) = mk.get("x").and_then(Value::as_f64) else { return };
    let Some(y) = mk.get("y").and_then(Value::as_f64) else { return };
    let Some(z) = mk.get("z").and_then(Value::as_f64) else { return };
    let id = mk
        .get("id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map_or_else(next_marker_id, str::to_owned);
    let owner_id = mk.get("ownerId").and_then(Value::as_str).map(str::to_owned);
    let owner = mk.get("owner").and_then(Value::as_str).unwrap_or("Player");
    let skin = mk.get("skin").and_then(Value::as_str).map(str::to_owned);
    let kind = mk.get("kind").and_then(Value::as_u64).unwrap_or(NORMAL as u64) as usize;
    let dim = mk.get("dim").and_then(Value::as_str).unwrap_or(waypoints::OVERWORLD);
    let t_kind = mk.get("targetKind").and_then(Value::as_str).unwrap_or("block");
    let t_name = mk.get("targetName").and_then(Value::as_str).unwrap_or("");
    let t_icon = mk.get("targetIcon").and_then(Value::as_str).unwrap_or("");
    let dur_ms = mk
        .get("duration")
        .and_then(Value::as_f64)
        .map_or(f64::from(cfg.duration) * 1000.0, |s| s * 1000.0);

    let marker = Marker::new(
        id.clone(),
        owner_id,
        owner,
        skin,
        kind,
        x,
        y,
        z,
        dim,
        cfg.world.clone(),
        t_kind,
        t_name,
        t_icon,
        false,
        dur_ms,
    );
    {
        let mut s = shared().lock().unwrap_or_else(|e| e.into_inner());
        s.active.retain(|m| m.id != id);
        s.active.push(marker.clone());
        while s.active.len() > MAX_MARKERS {
            s.active.remove(0);
        }
    }
    play_ding(marker.kind, cfg.other_vol / 100.0);
    notify_placed(&cfg, &marker);
}

/// Called by `relay.rs` when a teammate removes a marker or clears their markers (`marker:remove`).
pub fn on_remote_remove(payload: &Value) {
    let owner_id = payload.get("ownerId").and_then(Value::as_str);
    let id = payload.get("id").and_then(Value::as_str).filter(|s| !s.is_empty());
    let mut s = shared().lock().unwrap_or_else(|e| e.into_inner());
    s.active.retain(|mk| {
        if let Some(id) = id {
            mk.id != id
        } else if let Some(oid) = owner_id {
            mk.owner_id.as_deref() != Some(oid)
        } else {
            true
        }
    });
}

fn next_marker_id() -> String {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0u128, |d| d.as_nanos());
    format!("{:012x}{:04x}", (t as u64) & 0xFFFF_FFFF_FFFF, n & 0xFFFF)
}

fn scope_of(world: Option<&str>) -> Option<String> {
    let w = world?;
    let rest = w.strip_prefix("server:")?.trim().to_ascii_lowercase();
    let clean = rest.strip_suffix(":25565").unwrap_or(&rest);
    (!clean.is_empty()).then(|| clean.to_owned())
}

fn place_at(kind: usize, x: f64, y: f64, z: f64, target_kind: &str, target_name: &str, target_icon: &str) {
    let cfg = shared().lock().unwrap_or_else(|e| e.into_inner()).cfg.clone();
    let me_name = if !cfg.player_name.is_empty() {
        cfg.player_name.clone()
    } else {
        relay::snapshot().me_name.unwrap_or_else(|| "Player".to_owned())
    };
    let me_id = relay::me_id();
    let skin = avatars::own(&me_name).or_else(relay::me_skin);
    let id = next_marker_id();
    let mk = Marker::new(
        id,
        me_id,
        &me_name,
        skin,
        kind,
        x,
        y,
        z,
        &cfg.dim,
        cfg.world.clone(),
        target_kind,
        target_name,
        target_icon,
        true,
        f64::from(cfg.duration) * 1000.0,
    );
    {
        let mut s = shared().lock().unwrap_or_else(|e| e.into_inner());
        s.active.push(mk.clone());
        while s.active.len() > MAX_MARKERS {
            s.active.remove(0);
        }
    }
    play_ding(mk.kind, cfg.self_vol / 100.0);
    notify_placed(&cfg, &mk);
    broadcast_place(&cfg, &mk);
}

fn clear_all() {
    let cfg = {
        let mut s = shared().lock().unwrap_or_else(|e| e.into_inner());
        s.active.clear();
        s.cfg.clone()
    };
    broadcast_remove(&cfg, None);
}

fn remove_own(id: &str) {
    let (removed, cfg) = {
        let mut s = shared().lock().unwrap_or_else(|e| e.into_inner());
        let before = s.active.len();
        s.active.retain(|m| m.id != id);
        (s.active.len() != before, s.cfg.clone())
    };
    if removed {
        broadcast_remove(&cfg, Some(id));
    }
}

fn broadcast_place(cfg: &ConfigSnap, mk: &Marker) {
    if !cfg.share_party && !cfg.share_team && !cfg.share_friends {
        return;
    }
    let Some(scope) = scope_of(cfg.world.as_deref()) else { return };
    let Some(ticket) = net::ticket() else { return };
    let mut body = json!({
        "id": mk.id,
        "owner": mk.owner,
        "kind": mk.kind,
        "x": mk.x,
        "y": mk.y,
        "z": mk.z,
        "dim": mk.dim,
        "scope": scope,
        "targetKind": mk.target_kind,
        "targetName": mk.target_name,
        "targetIcon": mk.target_icon,
        "duration": cfg.duration.round() as i64,
        "shareParty": cfg.share_party,
        "shareTeam": cfg.share_team,
        "shareFriends": cfg.share_friends,
    });
    if let Some(team) = cfg.team_name.as_deref().filter(|t| !t.is_empty()) {
        body["team"] = Value::String(team.to_owned());
    }
    let url = format!("{}/v1/mod/markers", net::api());
    let _ = std::thread::Builder::new().name("Native marker post".into()).spawn(move || {
        let _ = net::post_json(&url, Some(&ticket), &body);
    });
}

fn broadcast_remove(cfg: &ConfigSnap, id: Option<&str>) {
    let Some(scope) = scope_of(cfg.world.as_deref()) else { return };
    let Some(ticket) = net::ticket() else { return };
    let mut body = json!({ "scope": scope });
    if let Some(id) = id {
        body["id"] = Value::String(id.to_owned());
    }
    let url = format!("{}/v1/mod/markers/remove", net::api());
    let _ = std::thread::Builder::new().name("Native marker remove".into()).spawn(move || {
        let _ = net::post_json(&url, Some(&ticket), &body);
    });
}

fn notify_placed(cfg: &ConfigSnap, mk: &Marker) {
    let kind_name = KIND_NAMES[mk.kind.min(3)];
    let target = if mk.target_name.is_empty() { String::new() } else { format!(" ({})", mk.target_name) };
    let coords = format!("({:.1}, {:.1}, {:.1})", mk.x, mk.y, mk.z);
    if cfg.chat_notice {
        let line = format!("\u{00A7}b[Markers] \u{00A7}f{} \u{00A7}7placed a \u{00A7}f{} Marker \u{00A7}7at {coords}{target}", mk.owner, kind_name);
        shared().lock().unwrap_or_else(|e| e.into_inner()).chat_queue.push(line);
    }
    if cfg.client_notice {
        relay::push_notice(relay::Notice {
            conv_key: None,
            title: format!("{} \u{00B7} {} Marker", mk.owner, kind_name),
            body: format!("{coords}{target}"),
            avatar_name: mk.owner.clone(),
            skin: mk.skin.clone(),
            at: relay::now_ms(),
        });
    }
}

// ── Audio ding synthesizer (cpal non-blocking thread) ─────────────────────

/// Synthesizes mono f32 PCM for a marker ping (Normal/!/`?`: high crisp ding; Danger: lower 440Hz -> 330Hz tone).
pub fn synth_ding(kind: usize, rate: u32, gain: f32) -> Vec<f32> {
    let danger = kind == DANGER;
    let duration = if danger { 0.24 } else { 0.16 };
    let samples = (f64::from(rate) * duration) as usize;
    let mut out = Vec::with_capacity(samples);
    let tau = std::f64::consts::TAU;
    for i in 0..samples {
        let t = i as f64 / f64::from(rate);
        let s = if danger {
            let f = if t < 0.11 { 440.0 } else { 330.0 };
            let lt = if t < 0.11 { t } else { t - 0.11 };
            let env = (lt / 0.006).min(1.0) * (-lt * 20.0).exp();
            (0.75 * (tau * f * lt).sin() + 0.25 * (tau * f * 1.5 * lt).sin()) * env
        } else {
            let f1 = if kind == QUESTION {
                1175.0
            } else if kind == EXCLAMATION {
                1480.0
            } else {
                1318.5
            };
            let env = (t / 0.004).min(1.0) * (-t * 26.0).exp();
            (0.7 * (tau * f1 * t).sin() + 0.3 * (tau * f1 * 1.5 * t).sin()) * env
        };
        out.push((s as f32 * gain).clamp(-1.0, 1.0));
    }
    out
}

fn play_ding(kind: usize, vol: f32) {
    if vol <= 0.01 {
        return;
    }
    let gain = vol.clamp(0.0, 1.0) * 0.35;
    let _ = std::thread::Builder::new().name("Native marker ding".into()).spawn(move || {
        use cpal::SampleFormat;
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let host = cpal::default_host();
        let Some(device) = host.default_output_device() else { return };
        let Ok(supported) = device.default_output_config() else { return };
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let rate = config.sample_rate.0.max(8000);
        let channels = usize::from(config.channels.max(1));
        let pcm = synth_ding(kind, rate, gain);
        let dur_ms = (pcm.len() as u64 * 1000 / u64::from(rate)) + 80;
        let mut cursor = 0usize;
        let err = |_: cpal::StreamError| {};
        let stream = match format {
            SampleFormat::F32 => device.build_output_stream(
                &config,
                move |d: &mut [f32], _| {
                    for frame in d.chunks_exact_mut(channels) {
                        let v = pcm.get(cursor).copied().unwrap_or(0.0);
                        cursor = cursor.saturating_add(1);
                        for s in frame {
                            *s = v;
                        }
                    }
                },
                err,
                None,
            ),
            SampleFormat::I16 => device.build_output_stream(
                &config,
                move |d: &mut [i16], _| {
                    for frame in d.chunks_exact_mut(channels) {
                        let v = pcm.get(cursor).copied().unwrap_or(0.0);
                        cursor = cursor.saturating_add(1);
                        let q = <i16 as cpal::FromSample<f32>>::from_sample_(v);
                        for s in frame {
                            *s = q;
                        }
                    }
                },
                err,
                None,
            ),
            SampleFormat::U16 => device.build_output_stream(
                &config,
                move |d: &mut [u16], _| {
                    for frame in d.chunks_exact_mut(channels) {
                        let v = pcm.get(cursor).copied().unwrap_or(0.0);
                        cursor = cursor.saturating_add(1);
                        let q = <u16 as cpal::FromSample<f32>>::from_sample_(v);
                        for s in frame {
                            *s = q;
                        }
                    }
                },
                err,
                None,
            ),
            _ => return,
        };
        if let Ok(stream) = stream
            && stream.play().is_ok()
        {
            std::thread::sleep(std::time::Duration::from_millis(dur_ms));
        }
    });
}

// ── Behaviour ─────────────────────────────────────────────────────────────

#[derive(Default)]
struct Markers {
    key_held: bool,
    clear_was_down: bool,
    middle_was_down: bool,
    press_at: f64,
    wheel_open: bool,
    wheel_start_yaw: f32,
    wheel_start_pitch: f32,
    wheel_dx: f32,
    wheel_dy: f32,
    wheel_choice: i32, // -1 = center/cancel, 0..3 = quadrants
    pending_target: Option<MarkerTarget>,
    hovered_own_id: Option<String>,
}

impl Markers {
    fn sync_config(m: &ModuleState, g: &GameView) {
        let mut s = shared().lock().unwrap_or_else(|e| e.into_inner());
        s.cfg.enabled = m.enabled;
        s.cfg.duration = m.num("duration");
        s.cfg.share_party = m.bool("shareParty");
        s.cfg.share_team = m.bool("shareTeam");
        s.cfg.share_friends = m.bool("shareFriends");
        s.cfg.self_vol = m.num("selfVol");
        s.cfg.other_vol = m.num("otherVol");
        s.cfg.chat_notice = m.bool("chatNotice");
        s.cfg.client_notice = m.bool("clientNotice");
        s.cfg.colors = [m.color("pinMarker"), m.color("pinDanger"), m.color("pinObjective"), m.color("pinImportant")];
        if g.in_world && !g.sample {
            s.cfg.world = g.world.clone();
            s.cfg.dim = waypoints_module::dim_of(g);
            s.cfg.team_name = g.scoreboard_team.clone();
            if !g.player_name.is_empty() {
                s.cfg.player_name = g.player_name.clone();
            }
        }
        let now = clock_ms();
        s.active.retain(|mk| now < mk.expires_at);
    }

    fn color_for(m: &ModuleState, kind: usize) -> u32 {
        match kind {
            DANGER => m.color("pinDanger"),
            EXCLAMATION => m.color("pinObjective"),
            QUESTION => m.color("pinImportant"),
            _ => m.color("pinMarker"),
        }
    }

    fn place_pending(&mut self, kind: usize) {
        let Some(t) = self.pending_target.take() else { return };
        place_at(kind, t.pos[0], t.pos[1], t.pos[2], &t.kind, &t.name, &t.icon);
    }
}

impl Behaviour for Markers {
    fn toggled(&mut self, on: bool) {
        let mut s = shared().lock().unwrap_or_else(|e| e.into_inner());
        s.cfg.enabled = on;
        if !on {
            self.key_held = false;
            self.wheel_open = false;
            self.hovered_own_id = None;
        }
    }

    fn frame(&mut self, m: &ModuleState, g: &GameView) {
        Self::sync_config(m, g);
        if !g.in_world || g.sample {
            self.key_held = false;
            self.wheel_open = false;
            self.hovered_own_id = None;
            self.clear_was_down = false;
            self.middle_was_down = false;
            return;
        }
        let now = clock_ms();
        let clear_code = m.setting("clearKey").map_or(-1, Setting::as_key);
        let clear_down = g.playing && clear_code >= 0 && g.is_down(clear_code);
        if clear_down && !self.clear_was_down {
            clear_all();
        }
        self.clear_was_down = clear_down;

        let marker_code = m.setting("key").map_or(-1, Setting::as_key);
        let down = g.playing && marker_code >= 0 && g.is_down(marker_code);
        if !g.playing {
            self.key_held = false;
            self.wheel_open = false;
            self.pending_target = None;
        } else if down && !self.key_held {
            self.key_held = true;
            self.press_at = now;
            self.wheel_open = false;
            self.wheel_choice = -1;
            // Snapshot target immediately on key press so holding to choose a wheel icon marks what you aimed at.
            let stand = m.choice("placeAt") == 1;
            let feet = MarkerTarget {
                pos: [g.x.floor() + 0.5, g.y.floor() + 0.1, g.z.floor() + 0.5],
                kind: "block".to_owned(),
                name: "Position".to_owned(),
                icon: String::new(),
            };
            self.pending_target = if stand { Some(feet) } else { g.marker_target.clone() }.or_else(|| {
                g.look_block
                    .map(|b| MarkerTarget {
                        pos: [f64::from(b[0]) + 0.5, f64::from(b[1]) + 1.0, f64::from(b[2]) + 0.5],
                        kind: "block".to_owned(),
                        name: "Block".to_owned(),
                        icon: "grass_block".to_owned(),
                    })
                    .or_else(|| {
                        Some(MarkerTarget {
                            pos: [g.x, g.y + 0.5, g.z],
                            kind: "block".to_owned(),
                            name: "Position".to_owned(),
                            icon: String::new(),
                        })
                    })
            });
        } else if down && self.key_held {
            if !self.wheel_open && now - self.press_at >= HOLD_MS {
                self.wheel_open = true;
                self.wheel_start_yaw = g.yaw;
                self.wheel_start_pitch = g.pitch;
                self.wheel_dx = 0.0;
                self.wheel_dy = 0.0;
                self.wheel_choice = -1;
            }
            if self.wheel_open {
                let dyaw = wrap_deg(g.yaw - self.wheel_start_yaw);
                let dpitch = g.pitch - self.wheel_start_pitch;
                self.wheel_dx = (dyaw * 3.8).clamp(-100.0, 100.0);
                self.wheel_dy = (dpitch * 3.8).clamp(-100.0, 100.0);
                self.wheel_choice = tile_for(self.wheel_dx);
            }
        } else if !down && self.key_held {
            let was_wheel = self.wheel_open;
            let choice = self.wheel_choice;
            self.key_held = false;
            self.wheel_open = false;
            if !was_wheel {
                self.place_pending(NORMAL);
            } else if choice >= 0 {
                self.place_pending(choice as usize);
            } else {
                self.pending_target = None;
            }
        }

        // Middle-click single removal (mouse button 2).
        if g.playing && m.bool("middleRemove") {
            let mid_down = g.is_down(2);
            if mid_down && !self.middle_was_down
                && let Some(id) = self.hovered_own_id.as_deref()
            {
                remove_own(id);
            }
            self.middle_was_down = mid_down;
        } else {
            self.middle_was_down = false;
        }
    }

    fn overlays(&self, _m: &ModuleState, g: &GameView) -> bool {
        if !g.hud_allowed || !g.in_world || g.sample {
            return false;
        }
        if self.wheel_open {
            return true;
        }
        !shared().lock().unwrap_or_else(|e| e.into_inner()).active.is_empty()
    }

    fn overlay(&mut self, m: &ModuleState, c: &mut Canvas, g: &GameView, now: f64, editing: bool) {
        if editing || g.sample {
            return;
        }
        let (w, h) = (c.width(), c.height());
        let (snap, cur_world, cur_dim) = {
            let s = shared().lock().unwrap_or_else(|e| e.into_inner());
            (s.active.clone(), s.cfg.world.clone(), s.cfg.dim.clone())
        };
        let cam = g.camera.unwrap_or(Camera { pos: [g.x, g.y + 1.62, g.z], yaw: g.yaw, pitch: g.pitch, fov: 70.0 });
        let mut new_hover_own: Option<String> = None;
        let mut best_hover_d2 = f32::MAX;

        if !snap.is_empty() {
            let mut ordered: Vec<(usize, f64)> = snap
                .iter()
                .enumerate()
                .map(|(i, mk)| {
                    let d = ((mk.x - g.x).powi(2) + (mk.y - g.y).powi(2) + (mk.z - g.z).powi(2)).sqrt();
                    (i, d)
                })
                .collect();
            ordered.sort_by(|a, b| b.1.total_cmp(&a.1));

            let k = m.num("scale");
            let anim_on = m.bool("animate");
            for (idx, dist) in ordered {
                let mk = &snap[idx];
                if let (Some(w0), Some(mw)) = (&cur_world, &mk.world)
                    && w0 != mw
                {
                    continue;
                }
                if cur_dim != mk.dim {
                    continue;
                }
                let Some([sx, sy]) = cam.project([mk.x, mk.y + 0.35, mk.z], w, h) else {
                    continue;
                };
                if sx < -80.0 || sx > w + 80.0 || sy < -80.0 || sy > h + 80.0 {
                    continue;
                }
                let (dcx, dcy) = (sx - w / 2.0, sy - h / 2.0);
                let d2 = dcx * dcx + dcy * dcy;
                let hovered = d2 < (46.0 * k).powi(2);
                if hovered && mk.own && d2 < best_hover_d2 {
                    best_hover_d2 = d2;
                    new_hover_own = Some(mk.id.clone());
                }
                let age = (now - mk.created_at).max(0.0);
                let left = (mk.expires_at - now).max(0.0);
                let pop = (age / 140.0).min(1.0) as f32;
                let fade_out = (left / 400.0).clamp(0.0, 1.0) as f32;
                let bob = if anim_on { ((now / 360.0 + idx as f64).sin() as f32) * 2.5 * k } else { 0.0 };
                draw_world_marker(
                    m,
                    c,
                    mk,
                    sx,
                    sy + bob,
                    k * (0.82 + 0.18 * ease_out(pop)),
                    dist,
                    hovered,
                    fade_out,
                    false,
                );
            }
        }
        self.hovered_own_id = new_hover_own;
        if self.wheel_open {
            draw_wheel(m, c, w / 2.0, h / 2.0, self.wheel_choice, self.wheel_dx, self.wheel_dy);
        }
    }

    fn preview(&mut self, m: &ModuleState, c: &mut Canvas, _g: &GameView, now: f64, x: f32, y: f32, w: f32, h: f32) -> bool {
        previews::backdrop(c, x, y, w, h, 1.25, 640.0, 400.0);
        c.push_clip(x, y, w, h);
        let k = m.num("scale").min(1.45);
        let bob = if m.bool("animate") { ((now / 360.0).sin() as f32) * 2.5 * k } else { 0.0 };
        let sample = Marker::new(
            "preview".into(),
            Some("me".into()),
            "Player",
            avatars::own("Player"),
            NORMAL,
            -27.3,
            77.0,
            33.2,
            waypoints::OVERWORLD,
            None,
            "block",
            "Grass Block",
            "grass_block",
            true,
            60_000.0,
        );
        draw_world_marker(m, c, &sample, x + w * 0.5, y + h * 0.66 + bob, k, 8.4, true, 1.0, true);
        let key_code = m.setting("key").map_or(-1, Setting::as_key);
        let hint = if key_code >= 0 {
            format!("Tap {} to ping \u{00B7} Hold for wheel", keys::name(key_code))
        } else {
            "Bind a Marker keybind below to ping".to_owned()
        };
        previews::chip(c, &hint, x + w / 2.0, y + h - 30.0, theme::TEXT_SECONDARY);
        c.pop_clip();
        true
    }
}

// ── World marker & wheel drawing ──────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn draw_world_marker(
    m: &ModuleState,
    c: &mut Canvas,
    mk: &Marker,
    cx: f32,
    cy: f32,
    k: f32,
    dist_blocks: f64,
    hovered: bool,
    alpha: f32,
    in_preview: bool,
) {
    if alpha <= 0.01 {
        return;
    }
    c.push_alpha(alpha);
    let px = mc_font::snap_px(c, 9.5 * k);
    let line_h = mc_font::height(px);
    let sh = m.bool("shadow");
    let bg = m.bool("background");
    let cmp = m.bool("compact");

    let want_owner = should_show(m.choice("showOwner"), hovered);
    let want_desc = should_show(m.choice("showDesc"), hovered) && (!mk.target_icon.is_empty() || !mk.target_name.is_empty());
    let want_dist = should_show(m.choice("showDist"), hovered);
    let want_coords = should_show(m.choice("showCoords"), hovered);

    // Marker glyph at (cx, cy); info lines stack cleanly right above it.
    let icon_r = 11.5 * k;
    draw_marker_glyph(c, mk.kind, cx, cy, icon_r, Markers::color_for(m, mk.kind));

    let mut stack_y = cy - icon_r - 5.0 * k;
    let coords_text = want_coords.then(|| format!("({:.1}, {:.1}, {:.1})", mk.x, mk.y, mk.z));
    let dist_text = want_dist.then(|| format!("({dist_blocks:.1}m)"));

    // Row 3 (bottom-most text row): Coordinates
    if let Some(ct) = coords_text.as_deref() {
        let cw = mc_font::width(c, ct, px, false);
        stack_y -= line_h;
        if bg {
            c.round(cx - cw / 2.0 - 4.0 * k, stack_y - 2.0 * k, cw + 8.0 * k, line_h + 4.0 * k, 4.0 * k, 0x99000000);
        }
        mc_font::draw(c, ct, cx - cw / 2.0, stack_y, px, 0xFFE4E4E7, sh, false);
        stack_y -= 4.0 * k;
    }

    // Row 2: Target description (item/block icon +/or name) + Distance
    if want_desc || dist_text.is_some() {
        let dd = m.choice("descDisplay");
        let show_icon = want_desc && (cmp || dd == 0 || dd == 2) && !mk.target_icon.is_empty();
        let show_name = want_desc && !cmp && (dd == 1 || dd == 2) && !mk.target_name.is_empty();
        let item_sz = 11.0 * k;
        let mut txt = String::new();
        if show_name {
            txt.push_str(&mk.target_name);
        }
        if let Some(dt) = dist_text.as_deref() {
            if !txt.is_empty() {
                txt.push(' ');
            }
            txt.push_str(dt);
        }
        let tw = if txt.is_empty() { 0.0 } else { mc_font::width(c, &txt, px, false) };
        let icon_w = if show_icon { item_sz + if tw > 0.0 { 3.5 * k } else { 0.0 } } else { 0.0 };
        let total_w = icon_w + tw;
        let row_h = line_h.max(if icon_w > 0.0 { item_sz } else { 0.0 });
        stack_y -= row_h;
        let mut rx = cx - total_w / 2.0;
        if bg && total_w > 0.0 {
            c.round(rx - 4.0 * k, stack_y - 2.0 * k, total_w + 8.0 * k, row_h + 4.0 * k, 4.0 * k, 0x99000000);
        }
        if icon_w > 0.0 {
            let iy = stack_y + (row_h - item_sz) / 2.0;
            if !in_preview && mk.target_kind == "block" {
                c.host(HOST_ITEM_ID, item_key(&mk.target_icon), rx, iy, item_sz, 0xFFFFFFFF);
            } else {
                draw_block_fallback_icon(c, rx, iy, item_sz, &mk.target_icon);
            }
            rx += icon_w;
        }
        if tw > 0.0 {
            mc_font::draw(c, &txt, rx, stack_y + (row_h - line_h) / 2.0, px, 0xFFD4D4D8, sh, false);
        }
        stack_y -= 4.0 * k;
    }

    // Row 1 (top): Owner head +/or name + customizable suffix
    if want_owner {
        let od = m.choice("ownerDisplay");
        let head = cmp || od == 0 || od == 2;
        let name = !cmp && (od == 1 || od == 2);
        let suffix = if cmp { "" } else { m.setting("ownerSuffix").map_or("", Setting::as_text) };
        let text = if name { format!("{}{suffix}", mk.owner) } else { suffix.to_owned() };
        let head_sz = 10.5 * k;
        let tw = if text.is_empty() { 0.0 } else { mc_font::width(c, &text, px, false) };
        let gap = if head && tw > 0.0 { if name { 3.5 * k } else { 1.5 * k } } else { 0.0 };
        let total_w = if head { head_sz } else { 0.0 } + gap + tw;
        let row_h = line_h.max(if head { head_sz } else { 0.0 });
        stack_y -= row_h;
        let mut rx = cx - total_w / 2.0;
        if bg && total_w > 0.0 {
            c.round(rx - 4.0 * k, stack_y - 2.0 * k, total_w + 8.0 * k, row_h + 4.0 * k, 4.0 * k, 0x99000000);
        }
        if head {
            avatars::draw(c, mk.skin.as_deref(), rx, stack_y + (row_h - head_sz) / 2.0, head_sz, 1.5 * k);
            rx += head_sz + gap;
        }
        if tw > 0.0 {
            mc_font::draw(c, &text, rx, stack_y + (row_h - line_h) / 2.0, px, 0xFFFFFFFF, sh, false);
        }
    }
    c.pop_alpha();
}

/// Draws the 4-quadrant Lunar-style radial marker selection wheel.
fn draw_wheel(m: &ModuleState, c: &mut Canvas, cx: f32, cy: f32, selected: i32, dx: f32, _dy: f32) {
    // The marker picker: four tiles with the colour pins, the one under the pointer lifted and lit.
    let total = 4.0 * TILE + 3.0 * GAP;
    let (x0, y0) = (cx - total / 2.0, cy + 34.0);
    c.shadow(x0 - 10.0, y0 - 10.0, total + 20.0, TILE + 20.0, 16.0, 22.0, 0x88000000);
    c.round(x0 - 10.0, y0 - 10.0, total + 20.0, TILE + 20.0, 16.0, 0xD80B0C10);
    c.outline(x0 - 10.0, y0 - 10.0, total + 20.0, TILE + 20.0, 16.0, 1.0, 0x26FFFFFF);
    for (i, &kind) in PICKER.iter().enumerate() {
        let on = selected == kind as i32;
        let (tx, ty) = (x0 + i as f32 * (TILE + GAP), y0 - if on { 4.0 } else { 0.0 });
        let col = Markers::color_for(m, kind);
        c.round(tx, ty, TILE, TILE, 13.0, if on { 0xFF16181F } else { 0xFF0F1015 });
        c.outline(tx, ty, TILE, TILE, 13.0, if on { 1.6 } else { 1.0 }, if on { theme::alpha(col, 0.95) } else { 0x33FFFFFF });
        if on {
            c.shadow(tx, ty, TILE, TILE, 13.0, 18.0, theme::alpha(col, 0.45));
        }
        draw_marker_glyph(c, kind, tx + TILE / 2.0, ty + TILE / 2.0, if on { 20.5 } else { 17.5 }, col);
    }
    // The pointer: where turning has moved it along the row.
    let px = (cx + dx * 1.9).clamp(x0, x0 + total);
    let py = y0 + TILE + 6.0;
    c.free_quad(None, [px, py, px - 5.0, py + 7.0, px + 5.0, py + 7.0, px + 5.0, py + 7.0], [0.0; 8], 0xE6FFFFFF);
    let label = if (0..4).contains(&selected) { KIND_NAMES[selected as usize].to_owned() } else { "Turn to pick  ·  release to cancel".to_owned() };
    let tw = c.text_width(font::SEMIBOLD, 11.5, &label);
    let lh = c.line_height(font::SEMIBOLD, 11.5);
    let ly = y0 + TILE + 20.0;
    c.round(cx - tw / 2.0 - 12.0, ly, tw + 24.0, 24.0, 12.0, 0xD80B0C10);
    let col = if (0..4).contains(&selected) { Markers::color_for(m, selected as usize) } else { theme::TEXT_MUTED };
    c.text(font::SEMIBOLD, 11.5, &label, cx - tw / 2.0, ly + (24.0 - lh) / 2.0, col);
}

/// Draws a crisp marker glyph (Normal diamond, Danger warning triangle with `!`, `!` diamond, `?` diamond).
pub fn draw_marker_glyph(c: &mut Canvas, kind: usize, cx: f32, cy: f32, r: f32, color: u32) {
    // A marker's colour pin centred on (cx, cy), `r` its half height; the drawn glyph when the artwork is missing.
    match pin_art(kind) {
        Some(img) => {
            let s = r * 2.3;
            c.image(&img, cx - s / 2.0, cy - s / 2.0, s, s, [0.0, 0.0, 1.0, 1.0], 0xFFFFFFFF);
        }
        None => draw_vector_glyph(c, kind, cx, cy, r, color),
    }
}

fn draw_vector_glyph(c: &mut Canvas, kind: usize, cx: f32, cy: f32, r: f32, color: u32) {
    if kind == DANGER {
        let top_y = cy - r * 0.95;
        let bot_y = cy + r * 0.80;
        let half_w = r * 1.05;
        draw_triangle(c, cx, top_y + 1.2, cx + half_w + 1.0, bot_y + 1.2, cx - half_w - 1.0, bot_y + 1.2, 0x80000000);
        draw_triangle(c, cx, top_y, cx + half_w, bot_y, cx - half_w, bot_y, color);
        let bw = (r * 0.22).max(1.5);
        c.fill(cx - bw / 2.0, cy - r * 0.34, bw, r * 0.56, 0xFF101216);
        c.fill(cx - bw / 2.0, cy + r * 0.34, bw, bw, 0xFF101216);
        return;
    }
    if kind == EXCLAMATION || kind == QUESTION {
        draw_diamond(c, cx, cy + 1.0, r + 1.2, 0x88000000);
        draw_diamond(c, cx, cy, r, 0xEE111318);
        let px = (r * 0.145).max(1.0);
        let sym = if kind == EXCLAMATION { "!" } else { "?" };
        let sw = mc_font::width(c, sym, px, true);
        let sh = mc_font::height(px);
        mc_font::draw(c, sym, cx - sw / 2.0 + px * 0.3, cy - sh / 2.0, px, color, false, true);
        return;
    }
    // Normal: clean colored diamond with drop shadow.
    draw_diamond(c, cx, cy + 1.1, r + 0.8, 0x70000000);
    draw_diamond(c, cx, cy, r, color);
}

pub fn draw_diamond(c: &mut Canvas, cx: f32, cy: f32, r: f32, argb: u32) {
    c.free_quad(None, [cx, cy - r, cx + r, cy, cx, cy + r, cx - r, cy], [0.0; 8], argb);
}

#[allow(clippy::too_many_arguments)]
pub fn draw_triangle(c: &mut Canvas, x0: f32, y0: f32, x1: f32, y1: f32, x2: f32, y2: f32, argb: u32) {
    c.free_quad(None, [x0, y0, x1, y1, x2, y2, x2, y2], [0.0; 8], argb);
}

fn draw_annulus(c: &mut Canvas, cx: f32, cy: f32, inner: f32, outer: f32, color: u32) {
    draw_sector(c, cx, cy, inner, outer, 0.0, std::f64::consts::TAU, color);
}

#[allow(clippy::too_many_arguments)]
fn draw_sector(c: &mut Canvas, cx: f32, cy: f32, inner: f32, outer: f32, a: f64, b: f64, color: u32) {
    let steps = (((b - a).abs() / (std::f64::consts::PI / 20.0)).ceil() as usize).max(12);
    for n in 0..steps {
        let x = a + (b - a) * (n as f64) / (steps as f64);
        let y = a + (b - a) * ((n + 1) as f64) / (steps as f64);
        let (sx, cx0) = x.sin_cos();
        let (sy, cy0) = y.sin_cos();
        c.free_quad(
            None,
            [
                cx + (cx0 as f32) * inner,
                cy + (sx as f32) * inner,
                cx + (cx0 as f32) * outer,
                cy + (sx as f32) * outer,
                cx + (cy0 as f32) * outer,
                cy + (sy as f32) * outer,
                cx + (cy0 as f32) * inner,
                cy + (sy as f32) * inner,
            ],
            [0.0; 8],
            color,
        );
    }
}

/// Clean pixel-art miniature block fallback (used in settings preview / when a block texture isn't available).
fn draw_block_fallback_icon(c: &mut Canvas, x: f32, y: f32, s: f32, id: &str) {
    let (top, side) = if id.contains("bed") {
        (0xFFD93838, 0xFFE5E5E5)
    } else if id.contains("diamond") {
        (0xFF4CE8E0, 0xFF28B5B0)
    } else if id.contains("chest") {
        (0xFFC28E3A, 0xFF8C6223)
    } else if id.contains("compass") {
        (0xFF9CA3AF, 0xFF4B5563)
    } else {
        (0xFF5EAA3C, 0xFF7B5638)
    };
    c.round(x, y, s, s, 1.5, side);
    c.fill(x, y, s, s * 0.36, top);
    c.outline(x, y, s, s, 1.5, 0.8, 0x80000000);
}

fn should_show(mode: usize, hovered: bool) -> bool {
    mode == 0 || (mode == 1 && hovered)
}

/// The picker tile under the pointer (`dx` px right of the centre), or -1 in the middle gap (release cancels).
fn tile_for(dx: f32) -> i32 {
    let x = dx * 1.9 + 2.0 * TILE + 1.5 * GAP;
    (0..4)
        .find(|&i| {
            let l = i as f32 * (TILE + GAP);
            x >= l && x <= l + TILE
        })
        .map_or(-1, |i| PICKER[i] as i32)
}

fn wrap_deg(d: f32) -> f32 {
    ((d + 180.0).rem_euclid(360.0)) - 180.0
}

fn ease_out(t: f32) -> f32 {
    let u = 1.0 - t;
    1.0 - u * u * u
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_quadrants_and_ding_synth_match_java() {
        // The pointer starts in the middle gap (release cancels); turning walks it over the tiles.
        assert_eq!(tile_for(0.0), -1);
        assert_eq!(tile_for(-20.0), DANGER as i32);
        assert_eq!(tile_for(-55.0), NORMAL as i32);
        assert_eq!(tile_for(20.0), EXCLAMATION as i32);
        assert_eq!(tile_for(55.0), QUESTION as i32);
        assert_eq!(tile_for(500.0), -1);

        let normal = synth_ding(NORMAL, 44_100, 0.3);
        let danger = synth_ding(DANGER, 44_100, 0.3);
        assert!(danger.len() > normal.len());
        assert!(normal.iter().any(|s| s.abs() > 0.05));
        assert!(danger.iter().any(|s| s.abs() > 0.05));
    }

    #[test]
    fn remote_events_respect_scope_and_remove() {
        let mut s = shared().lock().unwrap_or_else(|e| e.into_inner());
        s.active.clear();
        s.cfg.enabled = true;
        s.cfg.share_party = true;
        s.cfg.world = Some("server:play.example.net".into());
        s.cfg.dim = waypoints::OVERWORLD.into();
        s.cfg.other_vol = 0.0; // silent in unit test
        drop(s);

        on_remote_place(&json!({
            "relation": "party",
            "marker": {
                "id": "m1",
                "ownerId": "u1",
                "owner": "Alex",
                "kind": 1,
                "x": 10.5,
                "y": 65.0,
                "z": -4.5,
                "dim": "overworld",
                "scope": "play.example.net",
                "targetKind": "block",
                "targetName": "Red Bed",
                "targetIcon": "red_bed",
                "duration": 20
            }
        }));
        assert_eq!(shared().lock().unwrap().active.len(), 1);
        on_remote_remove(&json!({ "id": "m1" }));
        assert!(shared().lock().unwrap().active.is_empty());
    }
}
