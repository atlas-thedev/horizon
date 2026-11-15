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
            threshold_db: m.num("threshold"),
            mic_gain: m.num("mic") / 100.0,
            out_volume: m.num("volume") / 100.0,
            mic_device: device(m, "micdevice"),
            out_device: device(m, "outdevice"),
            noise: m.bool("noise"),
            echo: m.bool("echo"),
        });
        voice::tick();
    }

    fn hidden(&self, m: &ModuleState, id: &str) -> bool {
        id == "ptt" && m.choice("mode") != PTT || id == "threshold" && m.choice("mode") != ACTIVATION
    }

    fn toggled(&mut self, on: bool) {
        if !on {
            voice::shutdown();
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

// ── Relay: the call buttons and the call strip ────────────────────────────

/// Call / mute / deafen / leave (or accept / decline while it rings) in a chat header, right to left from `right`.
/// Returns the left edge.
pub fn call_controls(ui: &mut Ui, c: &mut Canvas, key: &str, right: f32, y: f32) -> f32 {
    let size = 32.0;
    let mut x = right - size;
    if voice::ringing(key).is_some() && !voice::in_call(key) {
        if ui.icon_button(c, "relay:call-decline", x, y, size, theme::I_PHONE_OFF, Some("Decline")) {
            voice::decline(key);
        }
        x -= size + 6.0;
        // a halo that keeps rippling out of Accept while it rings
        for k in 0..2 {
            let ph = ((ui.now / 1100.0 + f64::from(k) / 2.0) % 1.0) as f32;
            c.round(x - ph * 7.0, y - ph * 7.0, size + ph * 14.0, size + ph * 14.0, 8.0 + ph * 7.0, theme::alpha(theme::ONLINE, 0.35 * (1.0 - ph)));
        }
        if ui.icon_button(c, "relay:call-accept", x, y, size, theme::I_PHONE, Some("Accept")) {
            voice::accept(key);
        }
        return x;
    }
    if voice::in_call(key) {
        if ui.icon_button(c, "relay:call-leave", x, y, size, theme::I_PHONE_OFF, Some("Leave call")) {
            voice::leave_call();
        }
        x -= size + 6.0;
        let deaf = voice::deafened();
        if ui.icon_button(c, "relay:call-deafen", x, y, size, if deaf { theme::I_HEADPHONES_OFF } else { theme::I_HEADPHONES }, Some(if deaf { "Undeafen" } else { "Deafen" })) {
            voice::toggle_deafen();
        }
        x -= size + 6.0;
        let mute = voice::muted();
        if ui.icon_button(c, "relay:call-mute", x, y, size, if mute { theme::I_MIC_OFF } else { theme::I_MIC }, Some(if mute { "Unmute" } else { "Mute" })) {
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
    if n > 0 {
        c.round(x - 2.0, y - 2.0, size + 4.0, size + 4.0, 9.0, theme::alpha(theme::ONLINE, 0.25));
    }
    if ui.icon_button(c, "relay:call", x, y, size, if busy { theme::I_LOADER } else { theme::I_PHONE }, Some(tip.as_str())) && !busy {
        voice::join_call(key);
    }
    x
}

/// The call strip under a chat's header: who is in the call (a click on a face shows their volume), how long it has
/// been going, the ping, or what is wrong. The height it took (0 when there is no call here).
#[derive(Default)]
pub struct CallStrip {
    open: Option<String>,
}

impl CallStrip {
    pub fn height(&self, key: &str) -> f32 {
        let call = voice::in_call(key) || voice::calling().as_deref() == Some(key);
        let live = !voice::call_members(key).is_empty();
        let note = voice::note().is_some() && !call;
        if !(call || live || note) {
            0.0
        } else if call && self.open.is_some() {
            90.0
        } else {
            50.0
        }
    }

    pub fn draw(&mut self, ui: &mut Ui, c: &mut Canvas, key: &str, x: f32, y: f32, w: f32) {
        let h = self.height(key);
        if h <= 0.0 {
            return;
        }
        c.fill(x, y, w, h, 0x0DFFFFFF);
        c.fill(x, y + h - 1.0, w, 1.0, theme::HAIRLINE);
        let mine = voice::in_call(key);
        let members = voice::call_members(key);
        let me = super::relay::me_id();
        let mut fx = x + 20.0;
        let cy = y + 25.0;
        for m in &members {
            let face = 26.0;
            let ring = if voice::speaking(m) { theme::ONLINE } else { 0x26FFFFFF };
            c.circle(fx + face / 2.0, cy, face / 2.0 + 2.0 + voice::level(m) * 2.0, ring);
            c.circle(fx + face / 2.0, cy, face / 2.0 + 0.5, 0xFF08090C);
            let skin = m.skin.clone().or_else(|| super::relay::friend(&m.user_id).and_then(|f| f.skin));
            avatars::draw_round(c, skin.as_deref(), fx, cy - face / 2.0, face);
            if m.muted || m.deafened {
                c.circle(fx + face - 3.0, cy + face / 2.0 - 3.0, 6.0, theme::DANGER);
                c.icon(if m.deafened { theme::I_HEADPHONES_OFF } else { theme::I_MIC_OFF }, 8.0, fx + face - 3.0, cy + face / 2.0 - 3.0, 0xFFFFFFFF);
            }
            let other = me.as_deref() != Some(m.user_id.as_str());
            if mine && other && ui.clicked(c, &format!("voice:face:{}", m.user_id), fx - 2.0, cy - face / 2.0 - 2.0, face + 4.0, face + 4.0) {
                self.open = if self.open.as_deref() == Some(m.user_id.as_str()) { None } else { Some(m.user_id.clone()) };
            }
            if ui.hover(c, fx, cy - face / 2.0, face, face) {
                ui.tip(m.shown());
            }
            fx += face + 8.0;
        }
        // the status on the right
        let status = if let Some(p) = voice::link_problem(key) {
            (p, theme::DANGER)
        } else if voice::calling().as_deref() == Some(key) {
            ("Calling...".to_owned(), theme::TEXT_SECONDARY)
        } else if let (true, Some(call)) = (mine, voice::call()) {
            let secs = ((super::relay::now_ms() - call.since) / 1000).max(0);
            let ping = if call.rtt >= 0 { format!(" · {} ms", call.rtt) } else { String::new() };
            (format!("In call · {}:{:02}{ping}", secs / 60, secs % 60), theme::ONLINE)
        } else if !members.is_empty() {
            (format!("{} in call", members.len()), theme::TEXT_SECONDARY)
        } else {
            (voice::note().unwrap_or_default(), theme::TEXT_MUTED)
        };
        let max = (x + w - 20.0 - fx - 10.0).max(60.0);
        let text = c.ellipsize(font::MEDIUM, 12.0, &status.0, max);
        let tw = c.text_width(font::MEDIUM, 12.0, &text);
        c.text(font::MEDIUM, 12.0, &text, x + w - 20.0 - tw, cy - c.line_height(font::MEDIUM, 12.0) / 2.0, status.1);
        // a friend's own volume
        let Some(open) = self.open.clone() else { return };
        let Some(m) = members.iter().find(|m| m.user_id == open).cloned() else {
            self.open = None;
            return;
        };
        if !mine {
            self.open = None;
            return;
        }
        volume_row(ui, c, &m, x + 20.0, y + 50.0, w - 40.0);
    }
}

/// "face  name  ----o----  120%": one player's own volume, 0 - 200 %.
fn volume_row(ui: &mut Ui, c: &mut Canvas, m: &Member, x: f32, y: f32, w: f32) {
    let cy = y + 16.0;
    let name = c.ellipsize(font::MEDIUM, 12.0, m.shown(), 120.0);
    c.text(font::MEDIUM, 12.0, &name, x, cy - c.line_height(font::MEDIUM, 12.0) / 2.0, theme::TEXT_STRONG);
    let v = voice::volume(&m.user_id);
    let pct = format!("{:.0}%", v * 100.0);
    let (sx, sw) = (x + 130.0, (w - 130.0 - 50.0).max(40.0));
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
    c.text(font::SEMIBOLD, 11.5, &pct, x + w - pw, cy - c.line_height(font::SEMIBOLD, 11.5) / 2.0, if v <= 0.0 { theme::DANGER } else { theme::TEXT_SECONDARY });
}

// ── the incoming-call card over the game ──────────────────────────────────

/// Height of the call card in the toasts column (0 when there is none).
pub fn card_height() -> f32 {
    if voice::incoming().is_some() || voice::calling().is_some() { 74.0 } else { 0.0 }
}

/// "Incoming voice call" (V answers, N declines) or "Calling..." (N cancels), top right, over the message cards.
pub fn draw_card(c: &mut Canvas, x: f32, y: f32, w: f32, now: i64) {
    let ring = voice::incoming();
    let calling = if ring.is_none() { voice::calling() } else { None };
    let (title, name, skin) = match (&ring, &calling) {
        (Some(r), _) => ("Incoming voice call", r.name.clone(), r.skin.clone()),
        (None, Some(key)) => {
            let f = key.strip_prefix("dm:").and_then(super::relay::friend);
            ("Calling...", f.as_ref().map_or("Friend".to_owned(), |f| f.display().to_owned()), f.and_then(|f| f.skin))
        }
        _ => return,
    };
    let h = 66.0;
    // a soft pulse while it rings
    let pulse = ((now as f64 / 600.0).sin() * 0.5 + 0.5) as f32;
    c.shadow(x, y + 3.0, w, h, 14.0, 14.0, 0x66000000);
    c.round(x, y, w, h, 14.0, 0xF00A0B0E);
    c.outline(x, y, w, h, 14.0, 1.0, theme::alpha(theme::ONLINE, 0.35 + 0.4 * pulse));
    avatars::draw(c, skin.as_deref(), x + 12.0, y + 12.0, 38.0, 10.0);
    let (tx, tw) = (x + 60.0, w - 72.0);
    c.text(font::SEMIBOLD, 12.0, title, tx, y + 9.0, theme::ONLINE);
    let shown = c.ellipsize(font::SEMIBOLD, 14.0, &name, tw);
    c.text(font::SEMIBOLD, 14.0, &shown, tx, y + 25.0, theme::TEXT_STRONG);
    let hint = if ring.is_some() { "V to answer · N to decline" } else { "N to cancel" };
    c.text(font::REGULAR, 10.5, hint, tx, y + 45.0, theme::TEXT_MUTED);
    c.icon(theme::I_PHONE, 16.0, x + w - 22.0, y + h / 2.0, theme::ONLINE);
}
