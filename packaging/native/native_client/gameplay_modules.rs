//! The modules that change how the game plays or looks rather than add a
//! read-out: Toggle Sprint, Zoom, Fullbright, Lag Free, Crosshair, Hitbox and
//! Critical Colours, Block Overlay, Low Fire, Damage Tint, Freelook, No Hurt
//! Cam, Motion Blur, Time Changer and Weather Changer. They hand what they
//! want to `native_ui::tweaks`, which the engine reads where
//! `tools/native/native_tweaks.py` hooked it.

use super::module::*;
use super::previews;
use super::setting::{self, Kind, Setting};
use horizon_client::native_ui::canvas::clock_ms;
use horizon_client::native_ui::tweaks::{self, TimeView, Zoom};
use horizon_client::native_ui::{Canvas, icons, theme};

pub fn all() -> Vec<Module> {
    vec![
        toggle_sprint(),
        zoom(),
        fullbright(),
        lag_free(),
        crosshair(),
        hitbox_colours(),
        critical_colours(),
    ]
}

/// The modules Java registers after the extra HUD modules.
pub fn late() -> Vec<Module> {
    vec![block_overlay(), low_fire(), damage_tint(), freelook(), no_hurt_cam(), motion_blur(), time_changer(), weather_changer()]
}

fn dt_seconds(last: &mut f64) -> f32 {
    let now = clock_ms();
    let dt = if *last == 0.0 { 0.016 } else { ((now - *last) / 1000.0).min(0.1) as f32 };
    *last = now;
    dt
}

fn with_hud(mut m: Module, f: impl FnOnce(&mut Hud)) -> Module {
    if let Some(h) = m.state.hud.as_mut() {
        f(h);
    }
    m
}

/// A preview with the "Vanilla" and the module's view side by side.
fn comparison(c: &mut Canvas, x: f32, y: f32, w: f32, h: f32, aspect: f32, mut draw: impl FnMut(&mut Canvas, &[f32; 4], bool)) {
    let views = previews::pair(x, y, w, h, aspect);
    for (i, v) in views.iter().enumerate() {
        c.push_clip(v[0], v[1], v[2], v[3]);
        draw(c, v, i == 1);
        c.pop_clip();
        c.outline(v[0], v[1], v[2], v[3], 6.0, 1.0, theme::HAIRLINE_STRONG);
        previews::caption(c, v, if i == 0 { "Vanilla" } else { "Native" }, i == 1);
    }
}

// ── Toggle Sprint ────────────────────────────────────────────────────────

fn toggle_sprint() -> Module {
    with_hud(
        Module::new(
            "togglesprint",
            "Toggle Sprint",
            "Press sprint once to keep sprinting (optionally sneak too).",
            MECHANIC,
            theme::I_FOOTPRINTS,
            false,
            vec![
                Setting::bool("sneak", "Toggle sneak too", false),
                Setting::bool("status", "Show status on HUD", true).fixed(),
                Setting::choice("font", "Font", 0, &["Native", "Minecraft"]),
            ],
            Some(Hud::new(0.0, 1.0)),
            Box::new(ToggleSprint),
        ),
        |h| h.drop_setting(LABELS),
    )
}

struct ToggleSprint;

impl Behaviour for ToggleSprint {
    fn frame(&mut self, m: &ModuleState, _g: &GameView) {
        let sneak = m.bool("sneak");
        tweaks::set(|t| {
            t.toggle_sprint = true;
            t.toggle_sneak = sneak;
        });
    }
    fn toggled(&mut self, on: bool) {
        if !on {
            tweaks::set(|t| {
                t.toggle_sprint = false;
                t.toggle_sneak = false;
            });
        }
    }
    fn visible(&self, m: &ModuleState, g: &GameView) -> bool {
        m.bool("status") && (g.sample || g.sprint_toggled || g.sneak_toggled)
    }
    fn lines(&mut self, _m: &ModuleState, g: &GameView) -> Vec<Line> {
        let v = if !g.sample && g.sneak_toggled { "Sneaking (Toggled)" } else { "Sprinting (Toggled)" };
        vec![Line::value(v)]
    }
}

// ── Zoom ─────────────────────────────────────────────────────────────────

fn zoom() -> Module {
    Module::plain(
        "zoom",
        "Zoom",
        "Hold a key to zoom in like a spyglass. Scroll to zoom further.",
        VISUAL,
        theme::I_ZOOM,
        vec![
            Setting::key("key", "Zoom key", 67),
            Setting::num("level", "Zoom", 4.0, 1.5, 12.0, 0.5, "x").fixed(),
            Setting::bool("smooth", "Smooth zoom", true).fixed(),
            Setting::bool("scroll", "Scroll to adjust", true).fixed(),
            Setting::bool("cinematic", "Cinematic camera", false).fixed(),
        ],
        Box::new(ZoomModule { current: 1.0, extra: 1.0, held: false, last: 0.0 }),
    )
}

struct ZoomModule {
    current: f32,
    extra: f32,
    held: bool,
    last: f64,
}

impl Behaviour for ZoomModule {
    fn frame(&mut self, m: &ModuleState, g: &GameView) {
        let key = m.setting("key").map_or(-1, Setting::as_key);
        let want = g.playing && key >= 0 && g.is_down(key);
        if want && !self.held {
            self.extra = 1.0;
        }
        self.held = want;
        let target = if self.held { (m.num("level") * self.extra).clamp(1.0, 50.0) } else { 1.0 };
        let dt = dt_seconds(&mut self.last);
        if m.bool("smooth") {
            self.current += (target - self.current) * (1.0 - (-14.0 * dt).exp());
            if (target - self.current).abs() < 0.002 {
                self.current = target;
            }
        } else {
            self.current = target;
        }
        let current = self.current;
        tweaks::set(|t| {
            t.zoom = (current > 1.001).then_some(Zoom { factor: current, smooth: false, turn: 1.0 / current });
        });
    }
    fn on_scroll(&mut self, m: &ModuleState, _g: &GameView, dy: f64) -> bool {
        if !self.held || !m.bool("scroll") || dy == 0.0 {
            return false;
        }
        self.extra = (self.extra * if dy > 0.0 { 1.25 } else { 0.8 }).clamp(0.4, 6.0);
        true
    }
    fn toggled(&mut self, on: bool) {
        if !on {
            self.held = false;
            self.current = 1.0;
            tweaks::set(|t| t.zoom = None);
        }
    }
    fn preview(&mut self, m: &ModuleState, c: &mut Canvas, _g: &GameView, now: f64, x: f32, y: f32, w: f32, h: f32) -> bool {
        let t = ((now / 1100.0).sin() as f32 + 1.0) / 2.0;
        let t = t * t * (3.0 - 2.0 * t);
        let z = 1.0 + (m.num("level") - 1.0) * t;
        // The real in-game screenshot, magnified towards the middle of the view like the game does.
        previews::backdrop(c, x, y, w, h, z, 640.0, 400.0);
        // The scope edge darkens as you zoom.
        let edge = ((z - 1.0) * 0.08).min(0.55);
        c.gradient_v(x, y, w, h * 0.18, theme::alpha(0xFF000000, edge), 0);
        c.gradient_v(x, y + h * 0.82, w, h * 0.18, 0, theme::alpha(0xFF000000, edge));
        previews::chip(c, &format!("{z:.1}x"), x + w / 2.0, y + 12.0, theme::TEXT_STRONG);
        let key = m.setting("key").map_or(-1, Setting::as_key);
        let hint = if key >= 0 {
            format!("Hold {} to zoom{}", super::keys::name(key), if m.bool("scroll") { ", scroll to adjust" } else { "" })
        } else {
            "Set a zoom key".to_owned()
        };
        previews::chip(c, &hint, x + w / 2.0, y + h - 34.0, theme::TEXT_SECONDARY);
        true
    }
}

// ── Fullbright ───────────────────────────────────────────────────────────

fn fullbright() -> Module {
    Module::plain(
        "fullbright",
        "Fullbright",
        "See in the dark: turns the brightness all the way up.",
        VISUAL,
        theme::I_SUN,
        vec![Setting::num("level", "Brightness", 16.0, 2.0, 16.0, 1.0, "").fixed()],
        Box::new(Fullbright),
    )
}

struct Fullbright;

impl Behaviour for Fullbright {
    fn frame(&mut self, m: &ModuleState, _g: &GameView) {
        let level = m.num("level");
        tweaks::set(|t| t.gamma = Some(level));
    }
    fn toggled(&mut self, on: bool) {
        if !on {
            tweaks::set(|t| t.gamma = None);
        }
    }
    fn preview(&mut self, _m: &ModuleState, c: &mut Canvas, _g: &GameView, now: f64, x: f32, y: f32, w: f32, h: f32) -> bool {
        let split = 0.5 + 0.32 * (now / 1600.0).sin() as f32;
        let sx = x + w * split;
        // The real in-game screenshot; the left side is how the same view looks at night without fullbright.
        previews::backdrop(c, x, y, w, h, 1.0, 640.0, 360.0);
        c.push_clip(x, y, sx - x, h);
        c.fill(x, y, w, h, 0xE8040612);
        c.pop_clip();
        c.fill(sx - 1.0, y, 2.0, h, 0xFFFFFFFF);
        c.circle(sx, y + h / 2.0, 9.0, 0xFFFFFFFF);
        c.icon(theme::I_SUN, 11.0, sx, y + h / 2.0, theme::SOLID_FG);
        previews::chip(c, "OFF", x + 44.0, y + h - 34.0, theme::TEXT_SECONDARY);
        previews::chip(c, "FULLBRIGHT", x + w - 64.0, y + h - 34.0, 0xFFFDE68A);
        true
    }
}

// ── Lag Free ─────────────────────────────────────────────────────────────

fn lag_free() -> Module {
    let mut m = Module::plain(
        "bgfps",
        "Lag Free",
        "Smooth FPS while you play, low FPS in the background, minimised or AFK.",
        SETTINGS,
        theme::I_ZAP,
        vec![
            Setting::num("unfocused", "FPS in the background", 30.0, 5.0, 120.0, 5.0, ""),
            Setting::num("minimized", "FPS when minimised", 5.0, 1.0, 30.0, 1.0, ""),
            Setting::bool("afk", "Lower FPS while AFK", true),
            Setting::num("afkAfter", "AFK after", 90.0, 15.0, 600.0, 15.0, "s"),
            Setting::num("afkFps", "FPS while AFK", 20.0, 5.0, 60.0, 5.0, ""),
            Setting::choice("particles", "Particles", 0, &["Game setting", "Decreased", "Minimal"]),
            Setting::num("entities", "Entity distance", 100.0, 50.0, 100.0, 5.0, "%"),
        ],
        Box::new(LagFree),
    );
    m.state.enabled = true;
    m.state.default_enabled = true;
    m
}

struct LagFree;

impl Behaviour for LagFree {
    fn hidden(&self, m: &ModuleState, id: &str) -> bool {
        (id == "afkAfter" || id == "afkFps") && !m.bool("afk")
    }
    fn frame(&mut self, m: &ModuleState, g: &GameView) {
        let cap = if !g.focused {
            Some(m.num("unfocused").round() as u32)
        } else if m.bool("afk") && g.idle_ms > f64::from(m.num("afkAfter")) * 1000.0 {
            Some(m.num("afkFps").round() as u32)
        } else {
            None
        };
        tweaks::set(|t| t.background_fps = cap);
    }
    fn toggled(&mut self, on: bool) {
        if !on {
            tweaks::set(|t| t.background_fps = None);
        }
    }
}

// ── Crosshair ────────────────────────────────────────────────────────────

const CUSTOM: usize = 5;

fn crosshair() -> Module {
    Module::plain(
        "crosshair",
        "Crosshair",
        "Replace the vanilla crosshair with your own: shape, size, gap, colour and outline.",
        VISUAL,
        theme::I_CROSSHAIR,
        vec![
            Setting::choice("shape", "Shape", 0, &["Cross", "T", "Dot", "Circle", "Square", "Custom"]),
            Setting::num("size", "Size", 7.0, 1.0, 30.0, 1.0, "px"),
            Setting::num("pixel", "Pixel size", 2.0, 1.0, 6.0, 1.0, "px"),
            Setting::num("thickness", "Thickness", 2.0, 1.0, 8.0, 1.0, "px").fixed(),
            Setting::num("gap", "Gap", 3.0, 0.0, 20.0, 1.0, "px").fixed(),
            Setting::bool("dot", "Centre dot", false).fixed(),
            Setting::color("color", "Colour", 0xFFFFFFFF),
            Setting::num("opacity", "Opacity", 100.0, 10.0, 100.0, 5.0, "%").fixed(),
            Setting::bool("outline", "Outline", true).fixed(),
            Setting::color("outlineColor", "Outline colour", 0xFF000000).fixed(),
            Setting::bool("dynamic", "Open up while moving", false).fixed(),
            Setting::bool("onTarget", "Change colour over a mob or player", true).fixed(),
            Setting::color("targetColor", "Colour over a target", 0xFFEF4444).fixed(),
            Setting::bool("hide", "Hide the vanilla crosshair", true).fixed(),
            Setting::pixels("art", "Draw your crosshair", 15, setting::plus(15, 4, 1)),
        ],
        Box::new(Crosshair { spread: 0.0, last: 0.0 }),
    )
}

struct Crosshair {
    spread: f32,
    last: f64,
}

impl Crosshair {
    #[allow(clippy::too_many_arguments)]
    fn draw(m: &ModuleState, c: &mut Canvas, cx: f32, cy: f32, k: f32, base: u32, extra_gap: f32) {
        let sc = c.scale;
        let a = ((((base >> 24) as f32) / 255.0) * m.num("opacity") / 100.0 * 255.0).round() as u32;
        let col = (a << 24) | (base & 0xFFFFFF);
        let oc = theme::alpha(m.color("outlineColor"), a as f32 / 255.0);
        let t = ((m.num("thickness") * k * sc * 0.6).round() as i32).max(1);
        let l = ((m.num("size") * k * sc * 0.75).round() as i32).max(1);
        let g = (((m.num("gap") + extra_gap) * k * sc * 0.75).round() as i32).max(0);
        let o = if m.bool("outline") { ((sc * 0.7 * k.min(1.5)).round() as i32).max(1) } else { 0 };
        let (px, py) = (c.px(cx), c.px(cy));
        let half = t / 2;
        let shape = m.choice("shape");
        if shape == CUSTOM {
            Self::custom(m, c, px, py, k, col, oc, o);
            return;
        }
        if shape == 2 {
            // Dot.
            let d = t.max(l);
            if o > 0 {
                rect(c, px - d / 2 - o, py - d / 2 - o, d + 2 * o, d + 2 * o, oc);
            }
            rect(c, px - d / 2, py - d / 2, d, d, col);
            return;
        }
        if shape == 3 || shape == 4 {
            // Circle / square ring.
            let d = 2 * (l + g);
            let (x0, y0, w) = ((px - d / 2) as f32 / sc, (py - d / 2) as f32 / sc, d as f32 / sc);
            let r = if shape == 3 { w / 2.0 } else { 0.0 };
            let of = o as f32 / sc;
            if o > 0 {
                c.outline(x0 - of, y0 - of, w + 2.0 * of, w + 2.0 * of, if shape == 3 { r + of } else { 0.0 }, (t + 2 * o) as f32 / sc, oc);
            }
            c.outline(x0, y0, w, w, r, t as f32 / sc, col);
            if m.bool("dot") {
                centre_dot(c, px, py, t, o, col, oc);
            }
            return;
        }
        // Arms: left, right, top, bottom (T has no top arm).
        let arms = [
            [px - half - g - l, py - half, l, t],
            [px - half + t + g, py - half, l, t],
            [px - half, py - half - g - l, t, l],
            [px - half, py - half + t + g, t, l],
        ];
        for (i, a) in arms.iter().enumerate() {
            if shape == 1 && i == 2 {
                continue;
            }
            if o > 0 {
                rect(c, a[0] - o, a[1] - o, a[2] + 2 * o, a[3] + 2 * o, oc);
            }
        }
        for (i, a) in arms.iter().enumerate() {
            if shape == 1 && i == 2 {
                continue;
            }
            rect(c, a[0], a[1], a[2], a[3], col);
        }
        if m.bool("dot") {
            centre_dot(c, px, py, t, o, col, oc);
        }
    }

    /// The drawn crosshair: each grid cell is a square of whole screen pixels, centred on the middle cell.
    #[allow(clippy::too_many_arguments)]
    fn custom(m: &ModuleState, c: &mut Canvas, px: i32, py: i32, k: f32, col: u32, oc: u32, o: i32) {
        let Some(art) = m.setting("art") else { return };
        let Kind::Pixels { size, grid, .. } = &art.kind else { return };
        let (n, mid) = (*size as i32, *size as i32 / 2);
        let p = ((m.num("pixel") * k * c.scale * 0.6).round() as i32).max(1);
        let (x0, y0) = (px - mid * p - p / 2, py - mid * p - p / 2);
        for pass in (if o > 0 { 0 } else { 1 })..2 {
            for j in 0..n {
                for i in 0..n {
                    if !grid[(j * n + i) as usize] {
                        continue;
                    }
                    if pass == 0 {
                        rect(c, x0 + i * p - o, y0 + j * p - o, p + 2 * o, p + 2 * o, oc);
                    } else {
                        rect(c, x0 + i * p, y0 + j * p, p, p, col);
                    }
                }
            }
        }
    }
}

fn centre_dot(c: &mut Canvas, px: i32, py: i32, t: i32, o: i32, col: u32, oc: u32) {
    let d = t.max(2);
    if o > 0 {
        rect(c, px - d / 2 - o, py - d / 2 - o, d + 2 * o, d + 2 * o, oc);
    }
    rect(c, px - d / 2, py - d / 2, d, d, col);
}

/// A rectangle in whole framebuffer pixels (so the crosshair stays crisp at any UI scale).
fn rect(c: &mut Canvas, x: i32, y: i32, w: i32, h: i32, argb: u32) {
    let sc = c.scale;
    c.fill(x as f32 / sc, y as f32 / sc, w as f32 / sc, h as f32 / sc, argb);
}

impl Behaviour for Crosshair {
    fn hidden(&self, m: &ModuleState, id: &str) -> bool {
        let custom = m.choice("shape") == CUSTOM;
        (id == "size" && custom) || ((id == "pixel" || id == "art") && !custom)
    }
    fn frame(&mut self, m: &ModuleState, g: &GameView) {
        let hide = m.bool("hide") && g.hud_allowed;
        tweaks::set(|t| t.crosshair = hide);
    }
    fn toggled(&mut self, on: bool) {
        if !on {
            tweaks::set(|t| t.crosshair = false);
        }
    }
    fn overlays(&self, _m: &ModuleState, g: &GameView) -> bool {
        g.hud_allowed
    }
    fn overlay(&mut self, m: &ModuleState, c: &mut Canvas, g: &GameView, now: f64, editing: bool) {
        let dt = (now - self.last).clamp(0.0, 100.0) as f32;
        self.last = now;
        let want = if m.bool("dynamic") && !g.sample { (g.speed * 1.4).min(8.0) } else { 0.0 };
        self.spread += (want - self.spread) * (dt / 90.0).min(1.0);
        let col = if m.bool("onTarget") && g.target_living && !g.sample && !editing { m.color("targetColor") } else { m.color("color") };
        let (cx, cy) = (c.width() / 2.0, c.height() / 2.0);
        Self::draw(m, c, cx, cy, 1.0, col, self.spread);
    }
    fn preview(&mut self, m: &ModuleState, c: &mut Canvas, _g: &GameView, _now: f64, x: f32, y: f32, w: f32, h: f32) -> bool {
        // The real in-game screenshot behind it, so a white / black crosshair can be judged on a real scene.
        previews::backdrop(c, x, y, w, h, 1.6, 640.0, 450.0);
        c.push_clip(x, y, w, h);
        Self::draw(m, c, x + w / 2.0, y + h / 2.0, 2.2, m.color("color"), 0.0);
        c.pop_clip();
        true
    }
}

// ── Hitbox and critical colours ──────────────────────────────────────────

fn hitbox_colours() -> Module {
    Module::plain(
        "hitbox-colours",
        "Hitbox Colours",
        "Choose the F3+B body and blue look-direction arrow colours. The red eye-height line stays vanilla.",
        VISUAL,
        theme::I_CROSSHAIR,
        vec![
            Setting::color("body", "Hitbox colour", 0xFFFFFFFF),
            Setting::color("direction", "Look-direction colour", 0xFF0000FF),
        ],
        Box::new(HitboxColours),
    )
}

struct HitboxColours;

impl Behaviour for HitboxColours {
    fn frame(&mut self, m: &ModuleState, _g: &GameView) {
        let (body, dir) = (m.color("body"), m.color("direction"));
        tweaks::set(|t| t.hitbox = Some((body, dir)));
    }
    fn toggled(&mut self, on: bool) {
        if !on {
            tweaks::set(|t| t.hitbox = None);
        }
    }
}

fn critical_colours() -> Module {
    Module::plain(
        "critical-colours",
        "Critical Colours",
        "Custom colours for critical-hit sparks, bow critical trails and enchanted-hit particles. Keeps vanilla counts and movement.",
        VISUAL,
        theme::I_SPARKLES,
        vec![
            Setting::color("normal", "Critical-hit colour", 0xFFFFC857),
            Setting::color("enchanted", "Enchanted-hit colour", 0xFF9D7BFF),
        ],
        Box::new(CriticalColours),
    )
}

struct CriticalColours;

impl Behaviour for CriticalColours {
    fn frame(&mut self, m: &ModuleState, _g: &GameView) {
        let (normal, magic) = (m.color("normal"), m.color("enchanted"));
        tweaks::set(|t| t.critical = Some((normal, magic)));
    }
    fn toggled(&mut self, on: bool) {
        if !on {
            tweaks::set(|t| t.critical = None);
        }
    }
}

// ── Block Overlay ────────────────────────────────────────────────────────

fn block_overlay() -> Module {
    Module::plain(
        "block-overlay",
        "Block Overlay",
        "Colour the outline around the block you are looking at, or let it cycle through the rainbow.",
        VISUAL,
        theme::I_LAYERS,
        vec![
            Setting::color("color", "Outline colour", 0xFFFFFFFF),
            Setting::num("opacity", "Strength", 100.0, 10.0, 100.0, 5.0, "%"),
            Setting::bool("rainbow", "Rainbow", false),
            Setting::num("speed", "Rainbow speed", 3.0, 1.0, 10.0, 1.0, ""),
        ],
        Box::new(BlockOverlay),
    )
}

struct BlockOverlay;

/// `BlockOverlay.hue`: a fully saturated colour around the wheel.
fn hue(h: f32, alpha: u32) -> u32 {
    let h = (h - h.floor()) * 6.0;
    let f = h - h.floor();
    let q = 1.0 - f;
    let (r, g, b) = match h as u32 {
        0 => (1.0, f, 0.0),
        1 => (q, 1.0, 0.0),
        2 => (0.0, 1.0, f),
        3 => (0.0, q, 1.0),
        4 => (f, 0.0, 1.0),
        _ => (1.0, 0.0, q),
    };
    (alpha << 24) | (((r * 255.0).round() as u32) << 16) | (((g * 255.0).round() as u32) << 8) | (b * 255.0).round() as u32
}

impl Behaviour for BlockOverlay {
    fn hidden(&self, m: &ModuleState, id: &str) -> bool {
        (id == "speed" && !m.bool("rainbow")) || (id == "color" && m.bool("rainbow"))
    }
    fn has_preview(&self) -> bool {
        false
    }
    fn frame(&mut self, m: &ModuleState, _g: &GameView) {
        let alpha = ((m.num("opacity") / 100.0) * 255.0).round() as u32;
        let argb = if m.bool("rainbow") {
            hue(((clock_ms() % 100_000.0) as f32) / (12_000.0 / m.num("speed")), alpha)
        } else {
            (alpha << 24) | (m.color("color") & 0xFFFFFF)
        };
        tweaks::set(|t| t.outline = Some((argb, 1.0)));
    }
    fn toggled(&mut self, on: bool) {
        if !on {
            tweaks::set(|t| t.outline = None);
        }
    }
}

// ── Low Fire ─────────────────────────────────────────────────────────────

fn low_fire() -> Module {
    Module::plain(
        "low-fire",
        "Low Fire",
        "Lowers the fire on your screen while you burn, so you can still see and fight.",
        VISUAL,
        icons::FLAME,
        vec![Setting::num("height", "Lower the fire by", 60.0, 10.0, 100.0, 5.0, "%")],
        Box::new(LowFire),
    )
}

struct LowFire;

impl Behaviour for LowFire {
    fn frame(&mut self, m: &ModuleState, _g: &GameView) {
        let drop = m.num("height") / 100.0 * 0.45;
        tweaks::set(|t| t.fire_lower = Some(drop));
    }
    fn toggled(&mut self, on: bool) {
        if !on {
            tweaks::set(|t| t.fire_lower = None);
        }
    }
    fn preview(&mut self, m: &ModuleState, c: &mut Canvas, _g: &GameView, now: f64, x: f32, y: f32, w: f32, h: f32) -> bool {
        let lower = m.num("height") / 100.0 * 0.45;
        comparison(c, x, y, w, h, 1.2, |c, v, lowered| {
            let [vx, top, half, vh] = *v;
            previews::backdrop(c, vx, top, half, vh, 1.0, 640.0, 360.0);
            // The game draws two 1 x 1 quads half a block in front of the camera, 0.3 below it and 0.24 to either side;
            // the view is 70 degrees tall, so one game unit is vh / 0.7 on screen.
            let ppu = vh / 0.7;
            let d = if lowered { lower } else { 0.0 };
            for q in 0..2 {
                let qx = vx + half / 2.0 + if q == 0 { -0.24 } else { 0.24 } * ppu - ppu / 2.0;
                let qy = top + vh / 2.0 - (0.2 - d) * ppu;
                flames(c, qx, qy, ppu, now);
            }
            let (cx, cy) = (vx + half / 2.0, top + vh / 2.0);
            c.fill(cx - 7.0, cy - 1.0, 14.0, 2.0, 0xE0FFFFFF);
            c.fill(cx - 1.0, cy - 7.0, 2.0, 14.0, 0xE0FFFFFF);
        });
        true
    }
}

/// A few flickering tongues of fire in a square (the game's animated fire sprite is the engine's to draw).
fn flames(c: &mut Canvas, x: f32, y: f32, size: f32, now: f64) {
    let tongues = 7;
    for i in 0..tongues {
        let f = i as f32 / (tongues - 1) as f32;
        let flick = ((now / 90.0 + f64::from(i) * 1.7).sin() as f32 * 0.5 + 0.5) * 0.35;
        let hgt = size * (0.55 + flick + 0.25 * (1.0 - (f - 0.5).abs() * 2.0));
        let w = size / tongues as f32 * 1.6;
        let tx = x + f * (size - w);
        c.round_gradient_h(tx, y + size - hgt, w, hgt, w / 2.0, 0xE6F59E0B, 0xE6EF4444);
    }
}

// ── Damage Tint ──────────────────────────────────────────────────────────

fn damage_tint() -> Module {
    Module::plain(
        "damage-tint",
        "Damage Tint",
        "Change the colour and strength of the flash when you or anything else takes damage. Only the look: no effect on gameplay.",
        VISUAL,
        theme::I_PALETTE,
        vec![Setting::color("color", "Tint colour", 0xFFFF0000), Setting::num("opacity", "Strength", 70.0, 0.0, 100.0, 5.0, "%")],
        Box::new(DamageTint),
    )
}

struct DamageTint;

impl Behaviour for DamageTint {
    fn frame(&mut self, m: &ModuleState, _g: &GameView) {
        let rgb = m.color("color");
        let alpha = (m.num("opacity") / 100.0 * 255.0).round() as u32;
        // The overlay texel as RGBA bytes: its alpha is what of the entity's own colour stays.
        let texel = u32::from_le_bytes([(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, (255 - alpha.min(255)) as u8]);
        tweaks::set(|t| t.hurt_texel = Some(texel));
    }
    fn toggled(&mut self, on: bool) {
        if !on {
            tweaks::set(|t| t.hurt_texel = None);
        }
    }
    fn preview(&mut self, m: &ModuleState, c: &mut Canvas, _g: &GameView, now: f64, x: f32, y: f32, w: f32, h: f32) -> bool {
        previews::backdrop(c, x, y, w, h, 1.25, 640.0, 470.0);
        let hurt = now % 2000.0 < 600.0; // The game's hurt flash lasts half a second.
        let overlay = if hurt { theme::alpha(0xFF000000 | (m.color("color") & 0xFFFFFF), m.num("opacity") / 100.0) } else { 0 };
        let figure = (h * 0.6).min(w * 0.8).max(60.0);
        let (cx, base) = (x + w / 2.0, y + h * 0.84 - 16.0);
        // A figure standing on the ground, flashing when hit.
        let body = |c: &mut Canvas, tint: u32| {
            let k = figure / 8.0;
            c.round(cx - 2.0 * k, base - 8.0 * k, 4.0 * k, 4.0 * k, k * 0.4, theme::mix(0xFF9CB88A, tint, if tint >> 24 > 0 { 0.6 } else { 0.0 }));
            c.round(cx - 2.0 * k, base - 4.0 * k, 4.0 * k, 3.0 * k, k * 0.4, theme::mix(0xFF4A6B9A, tint, if tint >> 24 > 0 { 0.6 } else { 0.0 }));
            c.round(cx - 2.0 * k, base - k, 1.8 * k, k, k * 0.3, theme::mix(0xFF35496B, tint, if tint >> 24 > 0 { 0.6 } else { 0.0 }));
            c.round(cx + 0.2 * k, base - k, 1.8 * k, k, k * 0.3, theme::mix(0xFF35496B, tint, if tint >> 24 > 0 { 0.6 } else { 0.0 }));
        };
        body(c, overlay);
        previews::chip(c, if hurt { "Taking damage" } else { "Not hurt" }, x + w / 2.0, y + h - 34.0, if hurt { theme::TEXT_STRONG } else { theme::TEXT_SECONDARY });
        true
    }
}

// ── Freelook ─────────────────────────────────────────────────────────────

fn freelook() -> Module {
    Module::plain(
        "freelook",
        "Freelook",
        "Hold a key and move the mouse to look all around your player without turning it: check behind you while you keep running.",
        SETTINGS,
        theme::I_EYE,
        vec![
            Setting::key("key", "Freelook key", 342),
            Setting::bool("toggle", "Toggle instead of hold", false),
            Setting::bool("third", "Third person while looking around", true),
            Setting::bool("smooth", "Glide back when I let go", true),
            Setting::num("sensitivity", "Look speed", 100.0, 25.0, 200.0, 5.0, "%"),
        ],
        Box::new(Freelook { active: false, returning: false, toggled: false, was_down: false, last: 0.0 }),
    )
}

struct Freelook {
    active: bool,
    returning: bool,
    toggled: bool,
    was_down: bool,
    last: f64,
}

impl Behaviour for Freelook {
    fn frame(&mut self, m: &ModuleState, g: &GameView) {
        let dt = dt_seconds(&mut self.last);
        let key = m.setting("key").map_or(-1, Setting::as_key);
        let down = key >= 0 && g.playing && g.is_down(key);
        if m.bool("toggle") && down && !self.was_down {
            self.toggled = !self.toggled;
        }
        self.was_down = down;
        let want = g.playing && if m.bool("toggle") { self.toggled } else { down };
        if !g.playing && self.toggled {
            self.toggled = false;
        }
        let sens = m.num("sensitivity") / 100.0;
        tweaks::set(|t| t.freelook_sens = sens);
        if want && !self.active {
            self.active = true;
            self.returning = false;
            tweaks::set(|t| t.freelook = Some((0.0, 0.0)));
        } else if !want && self.active {
            self.active = false;
            if m.bool("smooth") {
                self.returning = true;
            } else {
                tweaks::set(|t| t.freelook = None);
            }
        }
        if self.returning {
            let k = (-14.0 * dt).exp();
            tweaks::set(|t| {
                if let Some((y, p)) = t.freelook {
                    let (y, p) = (y * k, p * k);
                    t.freelook = if y.abs() < 0.2 && p.abs() < 0.2 { None } else { Some((y, p)) };
                } else {
                    self.returning = false;
                }
            });
            if tweaks::get().freelook.is_none() {
                self.returning = false;
            }
        }
    }
    fn toggled(&mut self, on: bool) {
        if !on {
            self.active = false;
            self.returning = false;
            self.toggled = false;
            tweaks::set(|t| t.freelook = None);
        }
    }
}

// ── No Hurt Cam ──────────────────────────────────────────────────────────

fn no_hurt_cam() -> Module {
    Module::plain(
        "no-hurt-cam",
        "No Hurt Cam",
        "Stops the camera from jolting when you take damage, so you keep your aim in a fight.",
        VISUAL,
        theme::I_ZAP,
        vec![],
        Box::new(NoHurtCam),
    )
}

struct NoHurtCam;

impl Behaviour for NoHurtCam {
    fn frame(&mut self, _m: &ModuleState, _g: &GameView) {
        tweaks::set(|t| t.no_hurt_cam = true);
    }
    fn toggled(&mut self, on: bool) {
        if !on {
            tweaks::set(|t| t.no_hurt_cam = false);
        }
    }
    fn preview(&mut self, _m: &ModuleState, c: &mut Canvas, _g: &GameView, now: f64, x: f32, y: f32, w: f32, h: f32) -> bool {
        let phase = (now % 1800.0) / 1800.0;
        let shake = if phase < 0.35 { ((now / 40.0).sin() * 7.0 * (1.0 - phase / 0.35)) as f32 } else { 0.0 };
        comparison(c, x, y, w, h, 0.7, |c, v, steady| {
            let [vx, top, half, vh] = *v;
            let (dx, dy) = if steady { (0.0, 0.0) } else { (shake, shake * 0.6) };
            // The real in-game screenshot; vanilla jolts it when you are hit, this module keeps it still.
            previews::backdrop(c, vx + dx - 10.0, top + dy - 10.0, half + 20.0, vh + 20.0, 1.0, 640.0, 360.0);
            if !steady && phase < 0.35 {
                c.fill(vx, top, half, vh, theme::alpha(0xFFFF2020, 0.25 * (1.0 - phase as f32 / 0.35)));
            }
        });
        true
    }
}

// ── Motion Blur ──────────────────────────────────────────────────────────

fn motion_blur() -> Module {
    Module::plain(
        "motion-blur",
        "Motion Blur",
        "Smooth trails when you turn or move, like a camera. The HUD and crosshair stay sharp.",
        VISUAL,
        theme::I_ZAP,
        vec![Setting::num("strength", "Strength", 50.0, 10.0, 90.0, 5.0, "%")],
        Box::new(MotionBlur),
    )
}

struct MotionBlur;

impl Behaviour for MotionBlur {
    fn frame(&mut self, m: &ModuleState, g: &GameView) {
        let keep = if g.in_world { Some(m.num("strength") / 100.0) } else { None };
        tweaks::set(|t| t.motion_blur = keep);
    }
    fn toggled(&mut self, on: bool) {
        if !on {
            tweaks::set(|t| t.motion_blur = None);
        }
    }
    fn preview(&mut self, m: &ModuleState, c: &mut Canvas, _g: &GameView, now: f64, x: f32, y: f32, w: f32, h: f32) -> bool {
        let k = m.num("strength") / 100.0;
        let frames = 14;
        let t0 = now / 1000.0;
        c.push_clip(x, y, w, h);
        // Oldest first, fully covering; every newer frame laid over it at (1 - k): the same sum the game does.
        for i in (0..frames).rev() {
            let t = t0 - f64::from(i) / 60.0;
            let pan = 640.0 + (t * 1.7).sin() as f32 * 330.0;
            c.push_alpha(if i == frames - 1 { 1.0 } else { 1.0 - k });
            previews::backdrop(c, x, y, w, h, 1.35, pan, 380.0);
            c.pop_alpha();
        }
        // The crosshair stays sharp, like the HUD.
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        c.fill(cx - 7.0, cy - 1.0, 14.0, 2.0, 0xE0FFFFFF);
        c.fill(cx - 1.0, cy - 7.0, 2.0, 14.0, 0xE0FFFFFF);
        c.pop_clip();
        previews::chip(c, "Turning the camera", x + w / 2.0, y + h - 34.0, theme::TEXT_SECONDARY);
        true
    }
}

// ── Time Changer ─────────────────────────────────────────────────────────

const TIME_NAMES: [&str; 6] = ["Sunrise", "Noon", "Sunset", "Night", "Midnight", "Custom"];
const TIME_TICKS: [i64; 6] = [0, 6000, 12000, 14000, 18000, -1];

fn time_changer() -> Module {
    Module::plain(
        "time-changer",
        "Time Changer",
        "See the world at sunrise, noon, sunset or midnight whenever you like. Only on your screen: the server's time does not change.",
        SETTINGS,
        theme::I_CLOCK,
        vec![
            Setting::choice("preset", "Time", 1, &TIME_NAMES).shorts(&["Dawn", "Noon", "Dusk", "Night", "Midn.", "Custom"]),
            Setting::num("custom", "Time of day", 6000.0, 0.0, 23750.0, 250.0, " ticks"),
            Setting::bool("flow", "Let time keep passing", false),
        ],
        Box::new(TimeChanger { target: 6000, flow: false, dirty: true, offset: 0 }),
    )
}

/// `TimeChanger.chosen`.
fn chosen_time(preset: usize, custom: f32) -> i64 {
    let t = TIME_TICKS[preset.min(TIME_TICKS.len() - 1)];
    if t >= 0 { t } else { (custom as i64).rem_euclid(24000) }
}

struct TimeChanger {
    target: i64,
    flow: bool,
    dirty: bool,
    offset: i64,
}

impl Behaviour for TimeChanger {
    fn hidden(&self, m: &ModuleState, id: &str) -> bool {
        id == "custom" && m.choice("preset") != TIME_NAMES.len() - 1
    }
    fn frame(&mut self, m: &ModuleState, g: &GameView) {
        let t = chosen_time(m.choice("preset"), m.num("custom"));
        let flow = m.bool("flow");
        if t != self.target || flow != self.flow {
            self.dirty = true;
        }
        self.target = t;
        self.flow = flow;
        if flow {
            if self.dirty && g.in_world {
                self.offset = t - g.day_time.rem_euclid(24000);
                self.dirty = false;
            }
            let offset = self.offset;
            tweaks::set(|tw| tw.time = Some(TimeView::Flow(offset)));
        } else {
            tweaks::set(|tw| tw.time = Some(TimeView::Fixed(t)));
        }
    }
    fn toggled(&mut self, on: bool) {
        self.dirty = true;
        if !on {
            tweaks::set(|t| t.time = None);
        }
    }
}

// ── Weather Changer ──────────────────────────────────────────────────────

fn weather_changer() -> Module {
    Module::plain(
        "weather-changer",
        "Weather Changer",
        "Pick clear skies, rain or a thunderstorm for your own screen. The server's weather does not change.",
        SETTINGS,
        theme::I_ZOOM,
        vec![Setting::choice("weather", "Weather", 0, &["Clear", "Rain", "Thunder"])],
        Box::new(WeatherChanger),
    )
}

struct WeatherChanger;

impl Behaviour for WeatherChanger {
    fn frame(&mut self, m: &ModuleState, _g: &GameView) {
        let levels = match m.choice("weather") {
            0 => (0.0, 0.0),
            1 => (1.0, 0.0),
            _ => (1.0, 1.0),
        };
        tweaks::set(|t| t.weather = Some(levels));
    }
    fn toggled(&mut self, on: bool) {
        if !on {
            tweaks::set(|t| t.weather = None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gameplay_modules_exist_with_the_java_ids() {
        let ids: Vec<&str> = all().iter().chain(late().iter()).map(|m| m.state.id).collect();
        for id in [
            "togglesprint", "zoom", "fullbright", "bgfps", "crosshair", "hitbox-colours", "critical-colours", "block-overlay", "low-fire",
            "damage-tint", "freelook", "no-hurt-cam", "motion-blur", "time-changer", "weather-changer",
        ] {
            assert!(ids.contains(&id), "{id} is missing");
        }
    }

    #[test]
    fn the_time_presets_and_the_rainbow_read_like_the_java_mod() {
        assert_eq!(chosen_time(1, 0.0), 6000);
        assert_eq!(chosen_time(5, 25_000.0), 1000);
        assert_eq!(hue(0.0, 255), 0xFFFF0000);
        assert_eq!(hue(1.0 / 3.0, 255) & 0xFFFFFF, 0x00FF00);
    }

    #[test]
    fn the_previews_and_the_crosshair_draw() {
        let mut c = horizon_client::native_ui::canvas();
        c.begin(1280, 760, 1.0);
        let g = GameView::sample();
        for mut m in all().into_iter().chain(late()) {
            m.behaviour.preview(&m.state, &mut c, &g, 1234.0, 10.0, 10.0, 400.0, 300.0);
            m.behaviour.overlay(&m.state, &mut c, &g, 1234.0, false);
            // Every module takes its settings in the page without panicking.
            let _ = m.shown_settings();
        }
    }
}
