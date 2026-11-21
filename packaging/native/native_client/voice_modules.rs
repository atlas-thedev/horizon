//! Voice in the Native UI (native-mod's `VoiceModules` and the voice parts of `RelayView`, `Toasts`, `UiRuntime`):
//!
//! - **Voice Chat**: the settings module (microphone and output device, mic volume, noise suppression, echo
//!   cancellation, voice volume, talk mode, keys) with a live microphone meter on its page;
//! - **Voice Overlay**: the movable "who is talking" faces, with a green ring while they talk;
//! - the call buttons in a Relay chat's header, the call strip under it (faces, timer, ping, per-player volume),
//!   and the incoming-call card over the game.

use super::avatars;
use super::module::{self, GameView, Module, ModuleState, SETTINGS};
use super::setting::Setting;
use super::ui::Ui;
use super::voice::{self, Member};
use super::voice_audio;
use horizon_client::native_ui::{Canvas, font, theme};

pub const PTT: usize = 0;
pub const ACTIVATION: usize = 1;
/// Push to talk's default: Z (B is the emote wheel).
pub const PTT_KEY: i32 = 90;

pub fn all() -> Vec<Module> {
    vec![voice_chat(), voice_overlay()]
}

// ── Voice Chat ────────────────────────────────────────────────────────────

fn voice_chat() -> Module {
    Module::new(
        "voice",
        "Voice Chat",
        "Talk to friends and groups in Relay calls. Only the people in your call can hear you.",
        SETTINGS,
        theme::I_MIC,
        true,
        vec![
            Setting::device("micdevice", "Microphone", voice_audio::inputs, "System default"),
            Setting::num("mic", "Microphone volume", 100.0, 0.0, 300.0, 5.0, "%"),
            Setting::bool("noise", "Noise suppression", true),
            Setting::bool("echo", "Echo cancellation", true),
            Setting::device("outdevice", "Output device", voice_audio::outputs, "System default"),
            Setting::num("volume", "Voice volume", 100.0, 0.0, 200.0, 5.0, "%"),
            Setting::choice("mode", "Talk mode", ACTIVATION, &["Push to talk", "Voice activation"]).shorts(&["PTT", "Voice"]),
            Setting::key("ptt", "Push-to-talk key", PTT_KEY),
            Setting::num("threshold", "Activation level", -42.0, -70.0, -10.0, 1.0, " dB"),
            Setting::key("mute", "Mute key", -1),
            Setting::key("deafen", "Deafen key", -1),
            Setting::bool("svc", "Simple Voice Chat (servers)", true),
            Setting::bool("mute-svc", "Auto-mute Simple Voice Chat in calls", true),
        ],
        None,
        Box::new(VoiceChat),
    )
}

fn device(m: &ModuleState, id: &str) -> String {
    match m.setting(id).map(|s| &s.kind) {
        Some(super::setting::Kind::Device { value, .. }) => value.clone(),
        _ => String::new(),
    }
}

fn key(m: &ModuleState, id: &str) -> i32 {
    m.setting(id).map_or(-1, Setting::as_key)
}

struct VoiceChat;

impl module::Behaviour for VoiceChat {
    fn frame(&mut self, m: &ModuleState, g: &GameView) {
        let ptt = key(m, "ptt");
        voice::configure(voice::Settings {
            push_to_talk: m.choice("mode") == PTT,
            // in a call the key also works while a screen (Relay chat, menus) is open: you are not in the world then
            ptt_held: (g.playing || voice::in_any_call()) && ptt >= 0 && g.keys_down.contains(&ptt),
            ptt_key: if ptt >= 0 { super::keys::name(ptt) } else { String::new() },
            threshold_db: m.num("threshold"),
            mic_gain: m.num("mic") / 100.0,
            out_volume: m.num("volume") / 100.0,
            mic_device: device(m, "micdevice"),
            out_device: device(m, "outdevice"),
            noise: m.bool("noise"),
            echo: m.bool("echo"),
        });
        voice::tick();
        super::svc::configure(m.bool("svc"), m.bool("mute-svc"));
        super::svc::tick();
    }

    fn hidden(&self, m: &ModuleState, id: &str) -> bool {
        id == "ptt" && m.choice("mode") != PTT
            || id == "threshold" && m.choice("mode") != ACTIVATION
            || id == "mute-svc" && !m.bool("svc")
    }

    fn toggled(&mut self, on: bool) {
        if !on {
            voice::shutdown();
            super::svc::on_disconnect();
        }
    }

    /// The mic test: the level bar (where voice activation opens marked on it) while you speak.
    fn preview(&mut self, m: &ModuleState, c: &mut Canvas, _g: &GameView, _now: f64, x: f32, y: f32, w: f32, h: f32) -> bool {
        voice::mic_test();
        let db = voice::mic_db() as f32;
        let activation = m.choice("mode") == ACTIVATION;
        let threshold = m.num("threshold");
        let hot = db >= if activation { threshold } else { -50.0 };
        let (bw, bh) = ((w - 48.0).min(320.0), 12.0);
        let (bx, by) = (x + (w - bw) / 2.0, y + h / 2.0);
        let title = "Mic test";
        c.text(font::SEMIBOLD, 13.0, title, bx, by - 42.0, theme::TEXT_STRONG);
        let hint = match voice::mic_error() {
            Some(e) => e,
            None if activation => "Speak: the bar turns green where voice activation opens.".to_owned(),
            None => "Speak: the bar shows what your microphone picks up.".to_owned(),
        };
        let hint = c.ellipsize(font::REGULAR, 11.5, &hint, bw);
        c.text(font::REGULAR, 11.5, &hint, bx, by - 22.0, if voice::mic_error().is_some() { theme::DANGER } else { theme::TEXT_MUTED });
        let frac = |v: f32| ((v + 70.0) / 70.0).clamp(0.0, 1.0);
        c.round(bx, by, bw, bh, bh / 2.0, 0x26FFFFFF);
        c.round(bx, by, bw * frac(db), bh, bh / 2.0, if hot { theme::ONLINE } else { 0xFF8B8D98 });
        if activation {
            let mx = bx + bw * frac(threshold);
            c.fill(mx - 1.0, by - 4.0, 2.0, bh + 8.0, 0xFFFFFFFF);
        }
        let level = if db <= -99.0 { "-∞ dB".to_owned() } else { format!("{db:.0} dB") };
        c.text(font::MEDIUM, 11.0, &level, bx, by + bh + 8.0, theme::TEXT_SECONDARY);
        if let Some(svc_status) = super::svc::status() {
            let col = if super::svc::active() { theme::ONLINE } else { theme::TEXT_MUTED };
            let txt = c.ellipsize(font::MEDIUM, 11.0, &svc_status, bw);
            c.text(font::MEDIUM, 11.0, &txt, bx, by + bh + 26.0, col);
        }
        true
    }
}

/// The mute / deafen keys of the Voice Chat module (-1 = none).
pub fn keys(modules: &super::modules::Modules) -> (i32, i32) {
    modules.get("voice").filter(|m| m.state.enabled).map_or((-1, -1), |m| (key(&m.state, "mute"), key(&m.state, "deafen")))
}

// ── Voice Overlay ─────────────────────────────────────────────────────────

struct Row {
    name: String,
    skin: Option<String>,
    speaking: bool,
    muted: bool,
    deafened: bool,
}

/// Everyone in your call, you first as "You"; a sample call in previews and the HUD editor when there is none.
fn rows(sample: bool) -> Vec<Row> {
    let me = super::relay::me_id();
    let mut out: Vec<Row> = Vec::new();
    if let Some(call) = voice::call() {
        for m in &call.members {
            let mine = me.as_deref() == Some(m.user_id.as_str());
            let row = Row {
                name: if mine { "You".to_owned() } else { m.shown().to_owned() },
                skin: m.skin.clone().or_else(|| super::relay::friend(&m.user_id).and_then(|f| f.skin)),
                speaking: voice::speaking(m),
                muted: if mine { voice::muted() } else { m.muted },
                deafened: if mine { voice::deafened() } else { m.deafened },
            };
            if mine { out.insert(0, row) } else { out.push(row) }
        }
    }
    if out.is_empty() && sample {
        out.push(Row { name: "You".into(), skin: super::relay::me_skin(), speaking: true, muted: false, deafened: false });
        out.push(Row { name: "Friend".into(), skin: None, speaking: false, muted: true, deafened: false });
    }
    out
}

fn voice_overlay() -> Module {
    let mut m = Module::hud(
        "voice-overlay",
        "Voice Overlay",
        "Faces of everyone in your voice call, with a green ring while they talk.",
        theme::I_HEADPHONES,
        true,
        0.0,
        0.3,
        vec![Setting::bool("names", "Show names", true), Setting::choice("layout", "Layout", 0, &["Vertical", "Horizontal"])],
        Box::new(VoiceOverlay),
    );
    if let Some(h) = m.state.hud.as_mut() {
        h.drop_setting(module::STYLE);
        h.drop_setting(module::LABELS);
    }
    m
}

struct VoiceOverlay;

impl VoiceOverlay {
    fn size(m: &ModuleState, c: &mut Canvas, rows: &[Row]) -> [f32; 2] {
        let s = m.hud.as_ref().map_or(1.0, |h| h.scale());
        let (face, gap, pad) = (22.0 * s, 6.0 * s, 6.0 * s);
        let n = rows.len() as f32;
        if m.choice("layout") == 1 {
            return [pad * 2.0 + n * face + (n - 1.0).max(0.0) * gap, pad * 2.0 + face];
        }
        let text = if m.bool("names") {
            rows.iter().map(|r| c.text_width(font::SEMIBOLD, 12.0 * s, &r.name)).fold(0.0, f32::max) + 8.0 * s
        } else {
            0.0
        };
        [pad * 2.0 + face + text, pad * 2.0 + n * face + (n - 1.0).max(0.0) * gap]
    }
}

impl module::Behaviour for VoiceOverlay {
    fn visible(&self, _m: &ModuleState, g: &GameView) -> bool {
        g.sample || voice::in_any_call()
    }

    fn measure(&mut self, m: &ModuleState, c: &mut Canvas, _g: &GameView) -> Option<[f32; 2]> {
        let rows = rows(true);
        Some(Self::size(m, c, &rows))
    }

    fn paint(&mut self, m: &ModuleState, c: &mut Canvas, x: f32, y: f32, _g: &GameView) -> bool {
        let rows = rows(true);
        let size = Self::size(m, c, &rows);
        let Some(hud) = m.hud.as_ref() else { return false };
        let s = hud.scale();
        let (face, gap, pad) = (22.0 * s, 6.0 * s, 6.0 * s);
        hud.background(c, x, y, size[0], size[1], s);
        let horizontal = m.choice("layout") == 1;
        for (i, r) in rows.iter().enumerate() {
            let fx = if horizontal { x + pad + i as f32 * (face + gap) } else { x + pad };
            let fy = if horizontal { y + pad } else { y + pad + i as f32 * (face + gap) };
            let (cx, cy) = (fx + face / 2.0, fy + face / 2.0);
            if r.speaking {
                c.circle(cx, cy, face / 2.0 + 4.5 * s, theme::alpha(theme::ONLINE, 0.22));
            }
            c.circle(cx, cy, face / 2.0 + 2.0 * s, if r.speaking { theme::ONLINE } else { 0x40FFFFFF });
            c.circle(cx, cy, face / 2.0 + 0.5 * s, 0xFF08090C);
            avatars::draw_round(c, r.skin.as_deref(), fx, fy, face);
            if r.muted || r.deafened {
                let (bx, by, br) = (fx + face - 3.0 * s, fy + face - 3.0 * s, 5.5 * s);
                c.circle(bx, by, br, theme::DANGER);
                c.icon(if r.deafened { theme::I_HEADPHONES_OFF } else { theme::I_MIC_OFF }, 7.5 * s, bx, by, 0xFFFFFFFF);
            }
            if m.bool("names") && !horizontal {
                let ty = cy - c.line_height(font::SEMIBOLD, 12.0 * s) / 2.0;
                let color = if r.speaking { 0xFFFFFFFF } else { theme::alpha(hud.color(), 0.7) };
                hud.text(c, s, &r.name, fx + face + 8.0 * s, ty, color);
            }
        }
        true
    }
}

// ── Relay: the call buttons and the call stage ────────────────────────────

/// Header icon button; with a tint it is a filled pill (green join/accept, red leave/decline/muted).
fn voice_button(ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, icon: u32, tip: &str, tint: u32) -> bool {
    let size = 32.0;
    if tint == 0 {
        return ui.icon_button(c, id, x, y, size, icon, Some(tip));
    }
    let over = ui.hover(c, x, y, size, size);
    let click = ui.clicked(c, id, x, y, size, size);
    c.round(x, y, size, size, 8.0, if over { theme::mix(tint, 0xFFFFFFFF, 0.15) } else { tint });
    c.icon(icon, 16.0, x + 16.0, y + 16.0, 0xFFFFFFFF);
    if over {
        ui.tip(tip);
    }
    click
}

/// Call / mute / deafen / leave (or accept / decline while it rings) in a chat header, right to left from `right`.
/// Returns the left edge.
pub fn call_controls(ui: &mut Ui, c: &mut Canvas, key: &str, right: f32, y: f32) -> f32 {
    let size = 32.0;
    let mut x = right - size;
    if voice::ringing(key).is_some() && !voice::in_call(key) {
        if voice_button(ui, c, "relay:call-decline", x, y, theme::I_PHONE_OFF, "Decline", theme::DANGER) {
            voice::decline(key);
        }
        x -= size + 6.0;
        // a halo that keeps rippling out of Accept while it rings
        for k in 0..2 {
            let ph = ((ui.now / 1100.0 + f64::from(k) / 2.0) % 1.0) as f32;
            c.round(x - ph * 7.0, y - ph * 7.0, size + ph * 14.0, size + ph * 14.0, 8.0 + ph * 7.0, theme::alpha(theme::ONLINE, 0.35 * (1.0 - ph)));
        }
        if voice_button(ui, c, "relay:call-accept", x, y, theme::I_PHONE, "Accept", theme::ONLINE) {
            voice::accept(key);
        }
        return x;
    }
    if voice::in_call(key) {
        if voice_button(ui, c, "relay:call-leave", x, y, theme::I_PHONE_OFF, "Leave call", theme::DANGER) {
            voice::leave_call();
        }
        x -= size + 6.0;
        let deaf = voice::deafened();
        if voice_button(ui, c, "relay:call-deafen", x, y, if deaf { theme::I_HEADPHONES_OFF } else { theme::I_HEADPHONES }, if deaf { "Undeafen" } else { "Deafen" }, if deaf { theme::DANGER } else { 0 }) {
            voice::toggle_deafen();
        }
        x -= size + 6.0;
        let mute = voice::muted();
        if voice_button(ui, c, "relay:call-mute", x, y, if mute { theme::I_MIC_OFF } else { theme::I_MIC }, if mute { "Unmute" } else { "Mute" }, if mute { theme::DANGER } else { 0 }) {
            voice::toggle_mute();
        }
        return x;
    }
    let n = voice::call_members(key).len();
    let busy = voice::joining(key);
    let tip = if busy {
        "Connecting...".to_owned()
    } else if n > 0 {
        format!("Join call ({n} in call)")
    } else {
        "Start voice call".to_owned()
    };
    if voice_button(ui, c, "relay:call", x, y, if busy { theme::I_LOADER } else { theme::I_PHONE }, &tip, if n > 0 { theme::ONLINE } else { 0 }) && !busy {
        voice::join_call(key);
    }
    x
}

/// The animated call stage under a chat's header (faces, audio-reactive glow, mute badges, call timer, live ping pill,
/// and per-player volume slider), or a compact live banner when others are in a call.
#[derive(Default)]
pub struct CallStrip {
    open: Option<String>,
    key: Option<String>,
    h: f32,
}

impl CallStrip {
    pub fn draw(&mut self, ui: &mut Ui, c: &mut Canvas, key: &str, x: f32, y: f32, w: f32) -> f32 {
        let in_call = voice::in_call(key);
        let members = voice::call_members(key);
        let me = super::relay::me_id();
        let mut picked: Option<Member> = None;
        if in_call && let Some(open) = self.open.as_deref() {
            picked = members
                .iter()
                .find(|m| m.user_id == open && me.as_deref() != Some(m.user_id.as_str()))
                .cloned();
        }
        if picked.is_none() {
            self.open = None;
        }
        let note = if !in_call { voice::note() } else { None };
        let live_banner = !in_call && (!members.is_empty() || note.is_some());
        let target = if in_call {
            if picked.is_some() { 170.0 } else { 128.0 }
        } else if live_banner {
            48.0
        } else {
            0.0
        };
        if self.key.as_deref() != Some(key) {
            self.key = Some(key.to_owned());
            self.h = 0.0; // a newly opened chat grows its stage in
        }
        let dt = ui.dt.min(0.1);
        self.h += (target - self.h) * (dt * 12.0).min(1.0);
        if (target - self.h).abs() < 0.5 {
            self.h = target;
        }
        if self.h < 1.0 {
            return 0.0;
        }
        let h = self.h;
        c.push_clip(x, y, w, h);
        c.push_alpha((h / (if target == 0.0 { 48.0 } else { target }).max(1.0) * 1.4).min(1.0));

        if !in_call {
            if !members.is_empty() {
                c.gradient_v(x, y, w, h, 0x1A22C55E, 0x0622C55E);
                let (mut fx, cy, face) = (x + 20.0, y + h / 2.0, 24.0);
                for m in members.iter().take(4) {
                    let skin = m.skin.clone().or_else(|| super::relay::friend(&m.user_id).and_then(|f| f.skin));
                    c.circle(fx + face / 2.0, cy, face / 2.0 + 1.5, theme::alpha(theme::ONLINE, 0.55));
                    avatars::draw_round(c, skin.as_deref(), fx, cy - face / 2.0, face);
                    fx += face + 6.0;
                }
                let label = format!("{} in voice call", members.len());
                let lh = c.line_height(font::SEMIBOLD, 12.0);
                c.text(font::SEMIBOLD, 12.0, &label, fx + 6.0, cy - lh / 2.0, theme::ONLINE);
                let busy = voice::joining(key);
                let (bw, bh) = (72.0, 28.0);
                let (bx, by) = (x + w - 20.0 - bw, cy - bh / 2.0);
                let over = ui.hover(c, bx, by, bw, bh);
                c.round(bx, by, bw, bh, 8.0, if over { theme::mix(theme::ONLINE, 0xFFFFFFFF, 0.15) } else { theme::ONLINE });
                let btxt = if busy { "..." } else { "Join" };
                let btw = c.text_width(font::SEMIBOLD, 12.0, btxt);
                c.text(font::SEMIBOLD, 12.0, btxt, bx + (bw - btw) / 2.0, by + (bh - lh) / 2.0, theme::SOLID_FG);
                if ui.clicked(c, "relay:banner-join", bx, by, bw, bh) && !busy {
                    voice::join_call(key);
                }
            } else if let Some(note_txt) = note {
                c.fill(x, y, w, h, 0x0DFFFFFF);
                let cy = y + h / 2.0;
                c.icon(theme::I_PHONE_OFF, 14.0, x + 28.0, cy, theme::TEXT_MUTED);
                let txt = c.ellipsize(font::MEDIUM, 12.0, &note_txt, (w - 70.0).max(40.0));
                let lh = c.line_height(font::MEDIUM, 12.0);
                c.text(font::MEDIUM, 12.0, &txt, x + 44.0, cy - lh / 2.0, theme::TEXT_SECONDARY);
            }
            c.pop_alpha();
            c.pop_clip();
            c.fill(x, y + h - 1.0, w, 1.0, theme::HAIRLINE);
            return h;
        }

        let link = voice::link_problem(key);
        let tint = if link.is_some() { theme::DANGER } else { theme::ONLINE };
        c.gradient_v(x, y, w, h, theme::alpha(tint, 0.15), theme::alpha(tint, 0.03));

        // top line: live dot + status + timer + ping pill
        let calling = voice::calling().as_deref() == Some(key);
        let pulse = (0.5 + 0.5 * (ui.now / 320.0).sin()) as f32;
        c.circle(x + 24.0, y + 18.0, 4.0 + pulse * 2.5, theme::alpha(tint, 0.25));
        c.circle(x + 24.0, y + 18.0, 3.5, tint);
        let head = if let Some(p) = &link {
            p.as_str()
        } else if calling {
            "Ringing..."
        } else if members.len() <= 1 {
            "Waiting for others"
        } else {
            "Voice connected"
        };
        let mut tx = x + 35.0;
        let hlh = c.line_height(font::SEMIBOLD, 11.5);
        let hw = c.text(font::SEMIBOLD, 11.5, head, tx, y + 18.0 - hlh / 2.0, tint);
        tx += hw + 10.0;
        if let Some(room) = voice::call() {
            let secs = ((super::relay::now_ms() - room.since) / 1000).max(0);
            let time = if secs >= 3600 {
                format!("{}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60)
            } else {
                format!("{}:{:02}", secs / 60, secs % 60)
            };
            let tlh = c.line_height(font::MEDIUM, 11.0);
            let tw2 = c.text(font::MEDIUM, 11.0, &time, tx, y + 18.0 - tlh / 2.0, theme::TEXT_MUTED);
            tx += tw2 + 8.0;
            if room.rtt >= 0 {
                let ping_txt = format!("{} ms", room.rtt);
                let p_col = if room.rtt < 80 { theme::ONLINE } else if room.rtt < 180 { theme::IDLE } else { theme::DANGER };
                let pw = c.text_width(font::SEMIBOLD, 10.0, &ping_txt) + 12.0;
                c.round(tx, y + 10.0, pw, 16.0, 8.0, theme::alpha(p_col, 0.15));
                let plh = c.line_height(font::SEMIBOLD, 10.0);
                c.text(font::SEMIBOLD, 10.0, &ping_txt, tx + 6.0, y + 18.0 - plh / 2.0, p_col);
            }
        }
        if members.len() > 1 {
            let hint = "Click a face to change their volume";
            let hw2 = c.text_width(font::REGULAR, 10.5, hint);
            let lh2 = c.line_height(font::REGULAR, 10.5);
            c.text(font::REGULAR, 10.5, hint, x + w - 20.0 - hw2, y + 18.0 - lh2 / 2.0, theme::TEXT_MUTED);
        }

        // faces
        struct Tile {
            name: String,
            skin: Option<String>,
            user_id: Option<String>,
            member: Option<Member>,
            is_self: bool,
        }
        let mut tiles: Vec<Tile> = Vec::with_capacity(members.len() + 1);
        for m in &members {
            let is_self = me.as_deref() == Some(m.user_id.as_str());
            let skin = m.skin.clone().or_else(|| super::relay::friend(&m.user_id).and_then(|f| f.skin));
            tiles.push(Tile {
                name: if is_self { "You".to_owned() } else { m.shown().to_owned() },
                skin,
                user_id: if is_self { None } else { Some(m.user_id.clone()) },
                member: Some(m.clone()),
                is_self,
            });
        }
        if calling && let Some(fid) = key.strip_prefix("dm:") {
            let f = super::relay::friend(fid);
            tiles.push(Tile {
                name: f.as_ref().map_or_else(|| "Friend".to_owned(), |f| f.display().to_owned()),
                skin: f.and_then(|f| f.skin),
                user_id: None,
                member: None,
                is_self: false,
            });
        }
        let max_tiles = (((w - 40.0) / 80.0) as usize).max(1);
        let n = tiles.len().min(max_tiles);
        let (face, slot) = (44.0f32, 80.0f32);
        let row0 = x + (w - n as f32 * slot) / 2.0;
        let fy = y + 36.0;
        for (i, t) in tiles.iter().take(n).enumerate() {
            let cx = row0 + i as f32 * slot + slot / 2.0;
            let cy = fy + face / 2.0;
            let r = face / 2.0;
            let waiting = t.member.is_none();
            let lvl = t.member.as_ref().map_or(0.0, voice::level);
            let talking = t.member.as_ref().is_some_and(|m| lvl > 0.0 || voice::speaking(m));
            if waiting {
                for k in 0..3 {
                    let ph = ((ui.now / 1500.0 + f64::from(k) / 3.0) % 1.0) as f32;
                    let ex = r + 2.0 + ph * 16.0;
                    c.outline(cx - ex, cy - ex, ex * 2.0, ex * 2.0, ex, 1.5, theme::alpha(theme::ONLINE, 0.55 * (1.0 - ph)));
                }
            } else if talking {
                let glow = 0.35 + 0.65 * lvl.max(0.3);
                c.circle(cx, cy, r + 4.0 + glow * 5.0, theme::alpha(theme::ONLINE, 0.22 * glow));
            }
            let sel = t.user_id.as_ref().is_some_and(|id| self.open.as_deref() == Some(id.as_str()));
            let over = t.user_id.is_some() && ui.hover(c, cx - r - 4.0, cy - r - 4.0, face + 8.0, face + 8.0);
            let ring_col = if talking {
                theme::ONLINE
            } else if sel {
                0xFF60A5FA
            } else if over {
                0x66FFFFFF
            } else {
                0x26FFFFFF
            };
            c.circle(cx, cy, r + 2.5, ring_col);
            c.circle(cx, cy, r + 0.5, 0xFF08090C);
            c.push_alpha(if waiting { 0.55 } else { 1.0 });
            avatars::draw_round(c, t.skin.as_deref(), cx - r, cy - r, face);
            c.pop_alpha();
            if let Some(m) = &t.member {
                let muted = if t.is_self { voice::muted() } else { m.muted };
                let deaf = if t.is_self { voice::deafened() } else { m.deafened };
                if muted || deaf {
                    c.circle(cx + r - 4.0, cy + r - 4.0, 8.0, 0xFF08090C);
                    c.circle(cx + r - 4.0, cy + r - 4.0, 6.5, theme::DANGER);
                    c.icon(if deaf { theme::I_HEADPHONES_OFF } else { theme::I_MIC_OFF }, 8.0, cx + r - 4.0, cy + r - 4.0, 0xFFFFFFFF);
                }
            }
            let label = c.ellipsize(font::MEDIUM, 11.0, &t.name, slot - 8.0);
            let lw = c.text_width(font::MEDIUM, 11.0, &label);
            c.text(font::MEDIUM, 11.0, &label, cx - lw / 2.0, fy + face + 7.0, if talking { theme::TEXT_STRONG } else { theme::TEXT_SECONDARY });
            if let Some(uid) = &t.user_id {
                if ui.clicked(c, &format!("relay:stage:{uid}"), cx - r - 4.0, cy - r - 4.0, face + 8.0, face + 8.0) {
                    self.open = if sel { None } else { Some(uid.clone()) };
                }
                if over {
                    ui.tip(if sel { "Hide volume".to_owned() } else { format!("Volume for {}", t.name) }.as_str());
                }
            }
        }
        if tiles.len() > n {
            let more = format!("+{}", tiles.len() - n);
            let mlh = c.line_height(font::SEMIBOLD, 12.0);
            c.text(font::SEMIBOLD, 12.0, &more, row0 + n as f32 * slot + 4.0, fy + face / 2.0 - mlh / 2.0, theme::TEXT_MUTED);
        }

        // one volume slider, for the face that was clicked
        if let Some(m) = picked {
            let rw = (w - 40.0).min(460.0);
            volume_row(ui, c, &m, x + (w - rw) / 2.0, y + 124.0, rw);
        }
        c.pop_alpha();
        c.pop_clip();
        c.fill(x, y + h - 1.0, w, 1.0, theme::HAIRLINE);
        h
    }
}

/// "face  name  ----o----  120%": one player's own volume, 0 - 200 %.
fn volume_row(ui: &mut Ui, c: &mut Canvas, m: &Member, x: f32, y: f32, w: f32) {
    let (face, cy) = (20.0, y + 15.0);
    let talking = voice::speaking(m);
    let skin = m.skin.clone().or_else(|| super::relay::friend(&m.user_id).and_then(|f| f.skin));
    c.circle(x + face / 2.0, cy, face / 2.0 + 2.0, if talking { theme::ONLINE } else { 0x26FFFFFF });
    c.circle(x + face / 2.0, cy, face / 2.0 + 0.5, 0xFF08090C);
    avatars::draw_round(c, skin.as_deref(), x, cy - face / 2.0, face);
    let name_w = (w * 0.32).min(110.0);
    let name = c.ellipsize(font::MEDIUM, 12.0, m.shown(), name_w);
    let lh = c.line_height(font::MEDIUM, 12.0);
    c.text(font::MEDIUM, 12.0, &name, x + face + 8.0, cy - lh / 2.0, theme::TEXT_STRONG);
    let v = voice::volume(&m.user_id);
    let pct = format!("{:.0}%", v * 100.0);
    let sx = x + face + 8.0 + name_w + 12.0;
    let sw = (x + w - 40.0 - 10.0 - sx).max(40.0);
    let id = format!("voice:vol:{}", m.user_id);
    c.round(sx, cy - 2.0, sw, 4.0, 2.0, 0x33FFFFFF);
    c.round(sx, cy - 2.0, sw * v / 2.0, 4.0, 2.0, theme::ACCENT);
    c.circle(sx + sw * v / 2.0, cy, 6.0, 0xFFFFFFFF);
    let _ = ui.clicked(c, &id, sx - 6.0, cy - 9.0, sw + 12.0, 18.0);
    if ui.is_pressing(&id) {
        let t = ((ui.mx - sx) / sw).clamp(0.0, 1.0);
        let snapped = ((t * 2.0 * 20.0).round() / 20.0).clamp(0.0, 2.0); // steps of 5 %
        voice::set_volume(&m.user_id, snapped);
    }
    let pw = c.text_width(font::SEMIBOLD, 11.5, &pct);
    let lh = c.line_height(font::SEMIBOLD, 11.5);
    c.text(font::SEMIBOLD, 11.5, &pct, x + w - pw, cy - lh / 2.0, if v <= 0.0 { theme::DANGER } else { theme::TEXT_SECONDARY });
}

// ── the incoming-call card over the game ──────────────────────────────────

/// Height of the call card in the toasts column (0 when there is none).
pub fn card_height() -> f32 {
    if voice::incoming().is_some() || voice::calling().is_some() { 84.0 } else { 0.0 }
}

/// "Incoming voice call" (V accepts, N declines) or "Calling..." (N cancels), top right, over the message cards.
pub fn draw_card(c: &mut Canvas, x: f32, y: f32, w: f32, now: i64) {
    let ring = voice::incoming();
    let calling = if ring.is_none() { voice::calling() } else { None };
    let (head, name, skin) = match (&ring, &calling) {
        (Some(r), _) => ("Incoming voice call", r.name.clone(), r.skin.clone()),
        (None, Some(key)) => {
            let f = key.strip_prefix("dm:").and_then(super::relay::friend);
            ("Calling...", f.as_ref().map_or("your friend".to_owned(), |f| f.display().to_owned()), f.and_then(|f| f.skin))
        }
        _ => return,
    };
    let h = 76.0;
    let pulse = ((now as f64 / 260.0).sin() * 0.5 + 0.5) as f32;
    let accent = if ring.is_some() { theme::ONLINE } else { 0xFF60A5FA };
    c.shadow(x, y + 3.0, w, h, 14.0, 16.0, 0x70000000);
    c.round(x, y, w, h, 14.0, 0xF40A0B0E);
    c.outline(x, y, w, h, 14.0, 1.0, theme::alpha(accent, 0.35 + 0.35 * pulse));
    let (ax, ay) = (x + 14.0, y + 14.0);
    if ring.is_some() {
        c.circle(ax + 24.0, ay + 24.0, 27.0 + 2.0 * pulse, theme::alpha(accent, 0.18 + 0.2 * pulse));
    }
    avatars::draw(c, skin.as_deref(), ax, ay, 48.0, 12.0);
    let (tx, tw) = (x + 76.0, w - 118.0);
    c.text(font::SEMIBOLD, 10.5, head, tx, y + 12.0, accent);
    let shown = c.ellipsize(font::SEMIBOLD, 14.0, &name, tw);
    c.text(font::SEMIBOLD, 14.0, &shown, tx, y + 27.0, theme::TEXT_STRONG);
    let hint = if ring.is_some() { "V  accept  ·  N  decline" } else { "N  cancel call" };
    c.text(font::REGULAR, 10.5, hint, tx, y + 50.0, theme::TEXT_SECONDARY);
    c.circle(x + w - 24.0, y + 26.0, 13.0, theme::alpha(accent, 0.16));
    c.icon(theme::I_PHONE, 14.0, x + w - 24.0, y + 26.0, accent);
    if let Some(r) = ring {
        let span = (r.until - r.at).max(1) as f32;
        let left = ((r.until - now) as f32 / span).clamp(0.0, 1.0);
        c.fill(x + 14.0, y + h - 3.0, (w - 28.0) * left, 1.5, theme::alpha(accent, 0.65));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::ui::Event;

    #[test]
    fn idle_call_stage_and_card_are_collapsed() {
        let mut c = horizon_client::native_ui::canvas();
        c.begin(400, 400, 1.0);
        let mut ui = Ui::default();
        let mut strip = CallStrip::default();
        assert_eq!(strip.draw(&mut ui, &mut c, "dm:idle", 0.0, 0.0, 400.0), 0.0);
        assert_eq!(card_height(), 0.0);
    }

    #[test]
    fn filled_voice_button_triggers_click_on_release() {
        let mut c = horizon_client::native_ui::canvas();
        c.begin(200, 200, 1.0);
        let mut ui = Ui::default();
        ui.begin(&[Event::Move { x: 16.0, y: 16.0 }, Event::Button { button: 0, pressed: true }], 1.0);
        assert!(!voice_button(&mut ui, &mut c, "t:accept", 0.0, 0.0, theme::I_PHONE, "Accept", theme::ONLINE));
        ui.end();
        ui.begin(&[Event::Button { button: 0, pressed: false }], 1.0);
        assert!(voice_button(&mut ui, &mut c, "t:accept", 0.0, 0.0, theme::I_PHONE, "Accept", theme::ONLINE));
        ui.end();
    }
}
