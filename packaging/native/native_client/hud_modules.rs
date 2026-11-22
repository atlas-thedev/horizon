//! `HudModules`: the read-outs and graphical HUD modules (FPS, CPS,
//! Coordinates, Clock, Memory, Ping, Server IP, Speed, Keystrokes, Direction,
//! Native Watermark), with the Java mod's ids, defaults, places and look.

use super::module::*;
use super::setting::Setting;
use horizon_client::native_ui::{Canvas, font, theme};

pub fn all() -> Vec<Module> {
    vec![
        Module::hud("fps", "FPS", "Frames per second.", theme::I_GAUGE, true, 0.0, 0.0, vec![], Box::new(Fps)),
        Module::hud(
            "cps",
            "CPS",
            "Clicks per second.",
            theme::I_CLICK,
            false,
            0.0,
            0.07,
            vec![Setting::bool("right", "Show right clicks", true).fixed()],
            Box::new(Cps),
        ),
        keystrokes(),
        coordinates(),
        direction(),
        clock(),
        Module::hud(
            "memory",
            "Memory",
            "Java memory in use.",
            theme::I_MEMORY,
            false,
            1.0,
            0.07,
            vec![Setting::bool("detail", "Show amount", false).fixed()],
            Box::new(Memory),
        ),
        Module::hud("ping", "Ping", "Latency to the server.", theme::I_SIGNAL, false, 1.0, 0.14, vec![], Box::new(Ping)),
        server_ip(),
        Module::hud(
            "speed",
            "Speed",
            "How fast you are moving.",
            theme::I_ACTIVITY,
            false,
            0.0,
            0.28,
            vec![Setting::choice("unit", "Unit", 0, &["m/s", "km/h"])],
            Box::new(Speed),
        ),
        watermark(),
    ]
}

fn with_hud(mut m: Module, f: impl FnOnce(&mut Hud)) -> Module {
    if let Some(h) = m.state.hud.as_mut() {
        f(h);
    }
    m
}

// ── text read-outs ───────────────────────────────────────────────────────

struct Fps;
impl Behaviour for Fps {
    fn value_first(&self) -> bool {
        true
    }
    fn lines(&mut self, _m: &ModuleState, g: &GameView) -> Vec<Line> {
        vec![Line::new("FPS", g.fps.to_string())]
    }
}

struct Cps;
impl Behaviour for Cps {
    fn value_first(&self) -> bool {
        true
    }
    fn lines(&mut self, m: &ModuleState, g: &GameView) -> Vec<Line> {
        let v = if m.bool("right") {
            format!("{} | {}", g.cps_left, g.cps_right)
        } else {
            g.cps_left.to_string()
        };
        vec![Line::new("CPS", v)]
    }
}

fn clock() -> Module {
    with_hud(
        Module::hud(
            "clock",
            "Clock",
            "The time on your computer.",
            theme::I_CLOCK,
            false,
            1.0,
            0.0,
            vec![Setting::bool("24h", "24-hour clock", true), Setting::bool("seconds", "Show seconds", false).fixed()],
            Box::new(Clock),
        ),
        |h| h.drop_setting(LABELS),
    )
}

struct Clock;
impl Behaviour for Clock {
    fn lines(&mut self, m: &ModuleState, _g: &GameView) -> Vec<Line> {
        let (h, min, sec) = local_time();
        let h24 = m.bool("24h");
        let (h, suffix) = if h24 {
            (h, "")
        } else {
            (if h % 12 == 0 { 12 } else { h % 12 }, if h < 12 { " AM" } else { " PM" })
        };
        let hour = if h24 { format!("{h:02}") } else { h.to_string() };
        let secs = if m.bool("seconds") { format!(":{sec:02}") } else { String::new() };
        vec![Line::value(format!("{hour}:{min:02}{secs}{suffix}"))]
    }
}

/// Hours, minutes and seconds on this computer's clock (local time).
fn local_time() -> (u32, u32, u32) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let local = now + local_offset_seconds();
    let day = local.rem_euclid(86_400);
    ((day / 3600) as u32, (day % 3600 / 60) as u32, (day % 60) as u32)
}

/// The local time zone's offset from UTC, from `TZ`-free sources: the
/// `NATIVE_UTC_OFFSET` the launcher may set (seconds), else UTC.
fn local_offset_seconds() -> i64 {
    std::env::var("NATIVE_UTC_OFFSET")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

struct Memory;
impl Behaviour for Memory {
    fn visible(&self, _m: &ModuleState, g: &GameView) -> bool {
        g.mem_max > 0
    }
    fn lines(&mut self, m: &ModuleState, g: &GameView) -> Vec<Line> {
        let max = g.mem_max.max(1);
        let pct = g.mem_used * 100 / max;
        let detail = if m.bool("detail") {
            format!(" {:.1}/{:.1} GB", g.mem_used as f64 / 1073741824.0, max as f64 / 1073741824.0)
        } else {
            String::new()
        };
        vec![Line::new("Mem", format!("{pct}%{detail}"))]
    }
}

struct Ping;
impl Behaviour for Ping {
    fn visible(&self, _m: &ModuleState, g: &GameView) -> bool {
        g.sample || g.ping >= 0 && g.server.is_some()
    }
    fn value_first(&self) -> bool {
        true
    }
    fn lines(&mut self, _m: &ModuleState, g: &GameView) -> Vec<Line> {
        vec![Line::new("ms", g.ping.max(0).to_string())]
    }
}

fn server_ip() -> Module {
    with_hud(
        Module::hud("server", "Server IP", "The server you are playing on.", theme::I_SERVER, false, 1.0, 0.21, vec![], Box::new(ServerIp)),
        |h| h.drop_setting(LABELS),
    )
}

struct ServerIp;
impl Behaviour for ServerIp {
    fn visible(&self, _m: &ModuleState, g: &GameView) -> bool {
        g.server.as_deref().is_some_and(|s| !s.is_empty())
    }
    fn lines(&mut self, _m: &ModuleState, g: &GameView) -> Vec<Line> {
        vec![Line::value(g.server.clone().unwrap_or_default())]
    }
}

struct Speed;
impl Behaviour for Speed {
    fn value_first(&self) -> bool {
        true
    }
    fn lines(&mut self, m: &ModuleState, g: &GameView) -> Vec<Line> {
        let kmh = m.choice("unit") == 1;
        let s = if kmh { g.speed * 3.6 } else { g.speed };
        vec![Line::new(if kmh { "km/h" } else { "m/s" }, format!("{s:.1}"))]
    }
}

// ── coordinates ──────────────────────────────────────────────────────────

fn coordinates() -> Module {
    let mut m = Module::hud(
        "coords",
        "Coordinates",
        "Your position, the way you are facing and the biome you are in.",
        theme::I_MAP_PIN,
        false,
        0.0,
        0.14,
        vec![
            Setting::bool("facing", "Show direction", true),
            Setting::bool("biome", "Show biome", true),
            Setting::choice("font", "Font", 1, &["Native", "Minecraft"]),
            Setting::bool("compact", "One line", false).fixed(),
            Setting::choice("decimals", "Decimals", 0, &["0", "1", "2"]).fixed(),
        ],
        Box::new(Coordinates),
    );
    // Like the classic clients: text on the game, no card.
    if let Some(s) = m.state.hud.as_mut().and_then(|h| h.get_mut(STYLE)) {
        *s = s.clone().initial(STYLE_TEXT);
    }
    m
}

struct Coordinates;

/// Facing order of `GameView::facing` (S, SW, W, NW, N, NE, E, SE): the sign of X / Z walking that way.
const DX: [&str; 8] = ["", "-", "-", "-", "", "+", "+", "+"];
const DZ: [&str; 8] = ["+", "+", "", "-", "-", "-", "", "+"];

impl Coordinates {
    fn n(m: &ModuleState, v: f64) -> String {
        match m.choice("decimals") {
            1 => format!("{v:.1}"),
            2 => format!("{v:.2}"),
            _ => (v.floor() as i64).to_string(),
        }
    }
    fn rows(m: &ModuleState, g: &GameView) -> ([String; 3], [&'static str; 3]) {
        let f = (((g.yaw % 360.0) + 360.0) % 360.0 / 45.0).round() as usize & 7;
        let facing = m.bool("facing");
        (
            [
                format!("X: {}", Self::n(m, g.x)),
                format!("Y: {}", Self::n(m, g.y)),
                format!("Z: {}", Self::n(m, g.z)),
            ],
            if facing { [DX[f], g.facing(), DZ[f]] } else { ["", "", ""] },
        )
    }
}

impl Behaviour for Coordinates {
    fn lines(&mut self, m: &ModuleState, g: &GameView) -> Vec<Line> {
        let facing = if m.bool("facing") { format!("  {}", g.facing()) } else { String::new() };
        vec![Line::new("XYZ", format!("{} {} {}{facing}", Self::n(m, g.x), Self::n(m, g.y), Self::n(m, g.z)))]
    }
    fn measure(&mut self, m: &ModuleState, c: &mut Canvas, g: &GameView) -> Option<[f32; 2]> {
        if m.bool("compact") {
            return None;
        }
        let hud = m.hud.as_ref()?;
        let s = hud.scale();
        let (left, _) = Self::rows(m, g);
        let lw = left.iter().map(|t| hud.text_w(c, s, t)).fold(0.0, f32::max);
        let rw = if m.bool("facing") { hud.text_w(c, s, "NW") } else { 0.0 };
        let w = lw + if rw > 0.0 { 22.0 * s + rw } else { 0.0 };
        let card = hud.style() == STYLE_CARD;
        let lh = hud.line_h(c, s);
        let n = 3.0;
        Some([
            w + if card { 16.0 * s } else { 2.0 * s },
            n * lh + (n - 1.0) * 2.0 * s + if card { 10.0 * s } else { 2.0 * s },
        ])
    }
    fn paint(&mut self, m: &ModuleState, c: &mut Canvas, x: f32, y: f32, g: &GameView) -> bool {
        if m.bool("compact") {
            return false;
        }
        let Some(size) = self.measure(m, c, g) else {
            return false;
        };
        let Some(hud) = m.hud.as_ref() else {
            return false;
        };
        let s = hud.scale();
        let (left, right) = Self::rows(m, g);
        let style = hud.style();
        let card = style == STYLE_CARD;
        if card {
            hud.background(c, x, y, size[0], size[1], s);
        } else if style == STYLE_BRACKETS {
            hud.brackets(c, x - 3.0 * s, y - 2.0 * s, size[0] + 6.0 * s, size[1] + 4.0 * s, s);
        }
        let (pad, lh) = (if card { 8.0 * s } else { s }, hud.line_h(c, s));
        let (tx, mut ty, right0) = (x + pad, y + if card { 5.0 * s } else { s }, x + size[0] - pad);
        let col = hud.color();
        for i in 0..3 {
            hud.text(c, s, &left[i], tx, ty, col);
            if !right[i].is_empty() {
                // The signs sit centred under / over the compass letters, like the classic clients.
                let mid = right0 - hud.text_w(c, s, "NW") / 2.0;
                let w = hud.text_w(c, s, right[i]);
                hud.text(c, s, right[i], mid - w / 2.0, ty, col);
            }
            ty += lh + 2.0 * s;
        }
        true
    }
}

// ── keystrokes ───────────────────────────────────────────────────────────

fn keystrokes() -> Module {
    with_hud(
        Module::hud(
            "keystrokes",
            "Keystrokes",
            "Shows the movement keys and mouse buttons you press.",
            theme::I_KEYBOARD,
            true,
            0.0,
            0.5,
            vec![
                Setting::bool("mouse", "Mouse buttons", true).fixed(),
                Setting::bool("space", "Space bar", true).fixed(),
                Setting::bool("cps", "CPS on mouse", true).fixed(),
                Setting::color("pressed", "Pressed colour", 0xFFFFFFFF),
            ],
            Box::new(Keystrokes { anim: [0.0; 9], last: None }),
        ),
        |h| h.drop_setting(SHADOW),
    )
}

struct Keystrokes {
    anim: [f32; 9],
    last: Option<std::time::Instant>,
}

impl Keystrokes {
    /// Text on a pressed key: dark on a light pressed colour, white on a dark one.
    fn on_pressed(m: &ModuleState) -> u32 {
        let p = m.color("pressed");
        let lum = (0.2126 * ((p >> 16) & 255) as f32 + 0.7152 * ((p >> 8) & 255) as f32 + 0.0722 * (p & 255) as f32) / 255.0;
        if lum > 0.55 { theme::SOLID_FG } else { 0xFFFFFFFF }
    }

    fn label(g: &GameView, i: usize, def: &str) -> String {
        match &g.keys[i] {
            Some(n) if n.chars().count() <= 5 => n.clone(),
            _ => def.to_owned(),
        }
    }

    /// Card: filled keys. Text: just the letters (a pressed key lights up). Brackets: outlined keys.
    #[allow(clippy::too_many_arguments)]
    fn draw_box(m: &ModuleState, c: &mut Canvas, s: f32, x: f32, y: f32, w: f32, h: f32, t: f32) {
        let Some(hud) = m.hud.as_ref() else { return };
        let style = hud.style();
        let a = if style == STYLE_CARD { hud.opacity() / 100.0 } else { 0.0 };
        let bg = theme::mix(theme::alpha(0xFF08090C, a), theme::alpha(m.color("pressed"), 0.92), t);
        c.round(x, y, w, h, 5.0 * s, bg);
        if style == STYLE_CARD {
            c.outline(x, y, w, h, 5.0 * s, 1.0, theme::alpha(theme::HAIRLINE_STRONG, a.max(0.3)));
        } else if style == STYLE_BRACKETS {
            c.outline(x, y, w, h, 5.0 * s, 1.0, theme::alpha(hud.color(), (0.7 * (1.0 - t)).max(0.35)));
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn key(m: &ModuleState, c: &mut Canvas, s: f32, x: f32, y: f32, w: f32, h: f32, name: &str, sub: Option<&str>, t: f32) {
        Self::draw_box(m, c, s, x, y, w, h, t);
        let Some(hud) = m.hud.as_ref() else { return };
        let fg = theme::mix(hud.color(), Self::on_pressed(m), t);
        let fs = if name.chars().count() > 2 { 9.5 } else { 12.0 } * s;
        let tw = c.text_width(font::SEMIBOLD, fs, name);
        let lh = c.line_height(font::SEMIBOLD, fs);
        // No shadow on keys: on a light key it reads as dirt.
        match sub {
            None => {
                c.text(font::SEMIBOLD, fs, name, x + (w - tw) / 2.0, y + (h - lh) / 2.0, fg);
            }
            Some(sub) => {
                let ss = 8.0 * s;
                let sl = c.line_height(font::MEDIUM, ss);
                let top = y + (h - lh - sl) / 2.0;
                c.text(font::SEMIBOLD, fs, name, x + (w - tw) / 2.0, top, fg);
                let sw = c.text_width(font::MEDIUM, ss, sub);
                c.text(font::MEDIUM, ss, sub, x + (w - sw) / 2.0, top + lh, theme::alpha(fg, 0.7));
            }
        }
    }
}

impl Behaviour for Keystrokes {
    fn measure(&mut self, m: &ModuleState, _c: &mut Canvas, _g: &GameView) -> Option<[f32; 2]> {
        let s = m.hud.as_ref()?.scale();
        let (k, gap) = (26.0 * s, 2.0 * s);
        let mut h = 2.0 * k + gap;
        if m.bool("mouse") {
            h += gap + k;
        }
        if m.bool("space") {
            h += gap + 12.0 * s;
        }
        Some([3.0 * k + 2.0 * gap, h])
    }
    fn paint(&mut self, m: &ModuleState, c: &mut Canvas, x: f32, y: f32, g: &GameView) -> bool {
        let now = std::time::Instant::now();
        let dt = self.last.map_or(0.016, |l| now.duration_since(l).as_secs_f32().min(0.1));
        self.last = Some(now);
        let kk = 1.0 - (-18.0 * dt).exp();
        for (i, a) in self.anim.iter_mut().enumerate() {
            *a += ((if g.down[i] { 1.0 } else { 0.0 }) - *a) * kk;
        }
        let Some(hud) = m.hud.as_ref() else { return true };
        let s = hud.scale();
        let (k, gap) = (26.0 * s, 2.0 * s);
        let anim = self.anim;
        Self::key(m, c, s, x + k + gap, y, k, k, &Self::label(g, FORWARD, "W"), None, anim[FORWARD]);
        let mut ry = y + k + gap;
        Self::key(m, c, s, x, ry, k, k, &Self::label(g, LEFT, "A"), None, anim[LEFT]);
        Self::key(m, c, s, x + k + gap, ry, k, k, &Self::label(g, BACK, "S"), None, anim[BACK]);
        Self::key(m, c, s, x + 2.0 * (k + gap), ry, k, k, &Self::label(g, RIGHT, "D"), None, anim[RIGHT]);
        ry += k + gap;
        if m.bool("mouse") {
            let half = (3.0 * k + gap) / 2.0;
            let unit = if hud.labels() { " CPS" } else { "" };
            let (l, r) = (format!("{}{unit}", g.cps_left), format!("{}{unit}", g.cps_right));
            let cps = m.bool("cps");
            Self::key(m, c, s, x, ry, half, k, "LMB", cps.then_some(l.as_str()), anim[ATTACK]);
            Self::key(m, c, s, x + half + gap, ry, half, k, "RMB", cps.then_some(r.as_str()), anim[USE]);
            ry += k + gap;
        }
        if m.bool("space") {
            let (w, h) = (3.0 * k + 2.0 * gap, 12.0 * s);
            Self::draw_box(m, c, s, x, ry, w, h, anim[JUMP]);
            let bw = w * 0.35;
            let bar = theme::mix(theme::alpha(hud.color(), 0.8), Self::on_pressed(m), anim[JUMP]);
            c.round(x + (w - bw) / 2.0, ry + h / 2.0 - s, bw, 2.0 * s, s, bar);
        }
        true
    }
}

// ── direction ────────────────────────────────────────────────────────────

fn direction() -> Module {
    let mut m = with_hud(
        Module::hud(
            "direction",
            "Direction",
            "A compass strip that turns with you.",
            theme::I_COMPASS,
            false,
            0.5,
            0.0,
            vec![
                Setting::bool("degrees", "Show degrees", true).fixed(),
                Setting::bool("waypoints", "Show waypoints", true),
                Setting::bool("markers", "Show markers", true),
            ],
            Box::new(Direction),
        ),
        |h| h.drop_setting(LABELS),
    );
    if let Some(s) = m.state.hud.as_mut().and_then(|h| h.get_mut(STYLE)) {
        *s = s.clone().initial(STYLE_TEXT);
    }
    m
}

struct Direction;
const NAMES: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];

impl Behaviour for Direction {
    fn measure(&mut self, m: &ModuleState, _c: &mut Canvas, _g: &GameView) -> Option<[f32; 2]> {
        let s = m.hud.as_ref()?.scale();
        let top = if m.bool("degrees") { 19.0 } else { 8.0 };
        let bot = if m.bool("waypoints") || m.bool("markers") { 13.0 } else { 2.0 };
        Some([256.0 * s, (top + 24.0 + bot) * s])
    }
    fn paint(&mut self, m: &ModuleState, c: &mut Canvas, x: f32, y: f32, g: &GameView) -> bool {
        let Some(hud) = m.hud.as_ref() else { return true };
        let s = hud.scale();
        let show_deg = m.bool("degrees");
        let show_icons = m.bool("waypoints") || m.bool("markers");
        let w = 256.0 * s;
        let top_h = if show_deg { 19.0 * s } else { 8.0 * s };
        let strip_h = 24.0 * s;
        let total_h = top_h + strip_h + if show_icons { 13.0 * s } else { 2.0 * s };
        match hud.style() {
            STYLE_CARD => hud.background(c, x, y, w, total_h, s),
            STYLE_BRACKETS => hud.brackets(c, x, y, w, total_h, s),
            _ => {}
        }
        // In-game heading: 0 = N, 90 = E, 180 = S, 270 = W (matching vanilla & Waypoints::heading).
        let heading = (((g.yaw + 180.0) % 360.0) + 360.0) % 360.0;
        let span = 132.0; // shows ~9 15-degree intervals (e.g. 30, SW, 60, 75, W, 105, 120, NW, 150)
        let ppd = w / span;
        let cx = x + w / 2.0;
        let px = super::mc_font::snap_px(c, 9.0 * s);
        let lh = super::mc_font::height(px);
        let sh = hud.shadow();

        // Top center: current heading number + crisp white downward triangle ▼
        let tri_y = if show_deg {
            let deg_num = (heading.round() as i32).rem_euclid(360);
            let deg_str = deg_num.to_string();
            let dw = super::mc_font::width(c, &deg_str, px, false);
            super::mc_font::draw(c, &deg_str, cx - dw / 2.0, y + 1.0 * s, px, hud.color(), sh, false);
            y + lh + 3.0 * s
        } else {
            y + 2.0 * s
        };
        let tri_w = 4.2 * s;
        let tri_h = 4.2 * s;
        super::markers_module::draw_triangle(
            c,
            cx - tri_w,
            tri_y + 0.8 * s,
            cx + tri_w,
            tri_y + 0.8 * s,
            cx,
            tri_y + tri_h + 0.8 * s,
            0x70000000,
        );
        super::markers_module::draw_triangle(c, cx - tri_w, tri_y, cx + tri_w, tri_y, cx, tri_y + tri_h, hud.color());

        // Compass tick lines & mc_font heading labels
        let tick_y = y + top_h + 1.0 * s;
        let tick_h = 8.0 * s;
        let label_y = tick_y + tick_h + 3.0 * s;
        c.push_clip(x + 2.0 * s, tick_y - 2.0 * s, w - 4.0 * s, total_h - top_h + 4.0 * s);
        for a in (0..360).step_by(15) {
            let mut off = a as f32 - heading;
            off = ((off + 540.0) % 360.0) - 180.0;
            if off.abs() > span / 2.0 + 10.0 {
                continue;
            }
            let tx = cx + off * ppd;
            let edge = (1.0 - (off.abs() / (span / 2.0)).clamp(0.0, 1.0)).powf(0.55);
            let fade = (edge * 1.15).clamp(0.08, 1.0);
            let tick_w = (1.3 * s).max(1.0);
            let is_card = a % 45 == 0;
            // Vertical tick mark with subtle drop shadow
            c.fill(tx - tick_w / 2.0 + s * 0.6, tick_y + s * 0.6, tick_w, tick_h, theme::alpha(0xFF000000, 0.45 * fade));
            c.fill(tx - tick_w / 2.0, tick_y, tick_w, tick_h, theme::alpha(hud.color(), (if is_card { 0.95 } else { 0.82 }) * fade));

            let label_owned;
            let label = if is_card {
                NAMES[a / 45]
            } else {
                label_owned = a.to_string();
                &label_owned
            };
            let col = if a == 0 {
                theme::alpha(0xFFFFFFFF, fade)
            } else {
                theme::alpha(hud.color(), (if is_card { 0.98 } else { 0.86 }) * fade)
            };
            let lw = super::mc_font::width(c, label, px, false);
            super::mc_font::draw(c, label, tx - lw / 2.0, label_y, px, col, sh, false);
        }

        // Waypoint diamonds & Marker glyphs neatly beneath the heading strip (clamped to edge when off-strip).
        if show_icons {
            let icon_y = label_y + lh + 6.5 * s;
            let lim = span / 2.0 - 5.0;
            if m.bool("waypoints") && !g.sample {
                for (mark, color) in super::waypoints_module::compass() {
                    let off = ((mark - heading + 540.0) % 360.0) - 180.0;
                    let out = off.abs() > lim;
                    let mx = cx + off.clamp(-lim, lim) * ppd;
                    let r = if out { 3.4 } else { 4.6 } * s;
                    let a = if out { 0.6 } else { 1.0 };
                    super::markers_module::draw_diamond(c, mx, icon_y + 0.8 * s, r + 0.6 * s, theme::alpha(0x88000000, a));
                    super::markers_module::draw_diamond(c, mx, icon_y, r, theme::alpha(color, a));
                }
            }
            if m.bool("markers") {
                for (mark, kind, color) in super::markers_module::compass(g) {
                    let off = ((mark - heading + 540.0) % 360.0) - 180.0;
                    let out = off.abs() > lim;
                    let mx = cx + off.clamp(-lim, lim) * ppd;
                    let r = if out { 4.2 } else { 5.4 } * s;
                    c.push_alpha(if out { 0.62 } else { 1.0 });
                    super::markers_module::draw_marker_glyph(c, kind, mx, icon_y, r, color);
                    c.pop_alpha();
                }
            }
        }
        c.pop_clip();
        true
    }
}

// ── watermark ────────────────────────────────────────────────────────────

fn watermark() -> Module {
    with_hud(
        Module::hud(
            "watermark",
            "Native Watermark",
            "A subtle Native logo. Move or resize it in the HUD editor; hide it with F1 or switch it off.",
            theme::I_SPARKLES,
            true,
            1.0,
            0.0,
            vec![
                Setting::bool("text", "Show Native Client text", true),
                Setting::num("strength", "Opacity", 55.0, 10.0, 100.0, 5.0, "%"),
            ],
            Box::new(Watermark),
        ),
        |h| h.drop_setting(STYLE),
    )
}

struct Watermark;
impl Behaviour for Watermark {
    fn measure(&mut self, m: &ModuleState, c: &mut Canvas, _g: &GameView) -> Option<[f32; 2]> {
        let k = m.hud.as_ref()?.scale();
        // Measured at the drawn size (glyphs do not scale linearly), plus room for the shadow.
        let w = if m.bool("text") {
            24.0 * k + c.text_width(font::SEMIBOLD, 10.0 * k, "Native Client") + 4.0 * k
        } else {
            18.0 * k
        };
        Some([w, 18.0 * k])
    }
    fn paint(&mut self, m: &ModuleState, c: &mut Canvas, x: f32, y: f32, _g: &GameView) -> bool {
        let Some(hud) = m.hud.as_ref() else { return true };
        let k = hud.scale();
        let d = 18.0 * k;
        let strength = m.num("strength") / 100.0;
        let tint = theme::alpha(hud.color() | 0xFF000000, strength);
        let watermark = horizon_client::native_ui::icons::for_module("watermark", theme::I_SPARKLES);
        c.icon(watermark, d, x + d / 2.0, y + d / 2.0, tint);
        if m.bool("text") {
            let fs = 10.0 * k;
            let ty = y + (d - c.line_height(font::SEMIBOLD, fs)) / 2.0;
            c.text(font::SEMIBOLD, fs, "Native Client", x + 24.0 * k + k, ty + k, theme::alpha(0xFF000000, strength));
            c.text(font::SEMIBOLD, fs, "Native Client", x + 24.0 * k, ty, tint);
        }
        true
    }
}
