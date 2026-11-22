//! Waypoints (the Java mod's `WaypointsModule`): named points per world, drawn over the game where they are (or at the
//! screen's edge, pointing the way with a distance pill), with a 3-layer glowing beacon column and ground anchor.
//! The add key drops one where you stand; holding it drops one on the block you look at. Dying leaves a death point
//! that goes away once you are back there. The list lives on the Waypoints page of the Native menu; the Direction
//! strip shows them too.
//!
//! Everything is drawn by the Native overlay from the camera's position, direction and field of view, and it only ever
//! shows things to you: nothing is sent and nothing in the game changes.

use super::markers_module::{draw_diamond, draw_triangle};
use super::mc_font;
use super::module::*;
use super::setting::Setting;
use super::waypoints::{self, Camera, Point};
use horizon_client::native_ui::canvas::clock_ms;
use horizon_client::native_ui::{Canvas, font, theme};
use std::collections::HashSet;
use std::sync::Mutex;

pub const ID: &str = "waypoints";
/// GLFW J.
pub const KEY_J: i32 = 74;
const HOLD_MS: f64 = 400.0;
pub const LOOK_RANGE: f64 = 128.0;
const NOTICE_MS: f64 = 2600.0;

pub fn all() -> Vec<Module> {
    vec![Module::new(
        ID,
        "Waypoints",
        "Mark places and find your way back: markers over the world, glowing beams, elevation deltas, death points and a list per world.",
        VISUAL,
        theme::I_MAP_PIN,
        true,
        vec![
            Setting::key("add", "Add waypoint (hold: where you look)", KEY_J),
            Setting::num("size", "Marker size", 100.0, 60.0, 160.0, 10.0, "%"),
            Setting::num("range", "Show up to", 10000.0, 100.0, 10000.0, 100.0, " m"),
            Setting::choice("font", "Font", 0, &["Minecraft", "Native"]),
            Setting::choice("coords", "Coordinates", 0, &["On Hover", "Always", "Off"]),
            Setting::bool("elevation", "Show height difference", true),
            Setting::bool("beam", "Light beam", true),
            Setting::num("beam_opacity", "Beam opacity", 70.0, 10.0, 100.0, 5.0, "%"),
            Setting::bool("edge", "Point to waypoints off screen", true),
            Setting::bool("deaths", "Death points", true),
            Setting::bool("nether", "Show Overworld points in the Nether (1:8)", true),
        ],
        None,
        Box::new(Waypoints { deaths: true, ..Default::default() }),
    )]
}

/// The add key is held: the frame should find the block you look at (`GameView::look_block`).
pub fn wants_look(modules: &super::modules::Modules, held: &HashSet<i32>) -> bool {
    modules
        .get(ID)
        .filter(|m| m.state.enabled)
        .and_then(|m| m.state.setting("add"))
        .is_some_and(|s| s.as_key() >= 0 && held.contains(&s.as_key()))
}

static ADD_KEY: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(KEY_J);

/// The add key's code, for the page's hint.
pub fn add_key() -> i32 {
    ADD_KEY.load(std::sync::atomic::Ordering::Relaxed)
}

/// What the Direction strip shows: the shown waypoints' headings and colours.
static COMPASS: Mutex<Vec<(f32, u32)>> = Mutex::new(Vec::new());

pub fn compass() -> Vec<(f32, u32)> {
    COMPASS.lock().map(|c| c.clone()).unwrap_or_default()
}

/// "overworld" from "minecraft:overworld".
pub fn dim_of(g: &GameView) -> String {
    let d = g.dimension.trim_start_matches("minecraft:");
    if d.is_empty() { waypoints::OVERWORLD.to_owned() } else { d.to_owned() }
}

#[derive(Default)]
struct Waypoints {
    pressed_at: Option<f64>,
    hold_fired: bool,
    was_down: bool,
    alive: bool,
    deaths: bool,
    notice: Option<(String, f64)>,
    shown: Vec<Point>,
    shown_world: Option<String>,
    shown_rev: u64,
    dim: String,
    world: Option<String>,
}

impl Waypoints {
    fn say(&mut self, text: String) {
        self.notice = Some((text, clock_ms()));
    }

    fn add(&mut self, x: i32, y: i32, z: i32) {
        let Some(w) = self.world.clone() else { return };
        let mut s = waypoints::store();
        let name = s.next_name(&w, "Waypoint");
        let color = s.next_color(&w);
        let ok = s.add(&w, Point::new(&name, x, y, z, &self.dim, color, 0));
        drop(s);
        self.say(if ok {
            format!("Added {name} at {x} {y} {z}")
        } else {
            format!("This world has {} waypoints already", waypoints::MAX_PER_WORLD)
        });
    }

    fn deaths(&mut self, g: &GameView) {
        let Some(w) = self.world.clone() else { return };
        let now = g.vitals.health > 0.0;
        if self.alive && !now {
            let mut s = waypoints::store();
            for p in s.of(&w).into_iter().filter(|p| p.death) {
                s.remove(&w, p.id);
            }
            let mut p = Point::new("Death", g.x.floor() as i32, g.y.floor() as i32, g.z.floor() as i32, &self.dim, 0xFFF87171, waypoints::DEATH_ICON);
            p.death = true;
            s.add(&w, p);
            drop(s);
            self.say("Death point saved".into());
        }
        self.alive = now;
        if now {
            let back: Vec<u64> = self
                .shown
                .iter()
                .filter(|p| {
                    p.death && p.dim == self.dim && {
                        let d = [f64::from(p.x) + 0.5 - g.x, f64::from(p.y) - g.y, f64::from(p.z) + 0.5 - g.z];
                        d[0] * d[0] + d[1] * d[1] + d[2] * d[2] < 16.0
                    }
                })
                .map(|p| p.id)
                .collect();
            if !back.is_empty() {
                let mut s = waypoints::store();
                for id in back {
                    s.remove(&w, id);
                }
            }
        }
    }

    fn notice_active(&self) -> bool {
        self.notice.as_ref().is_some_and(|(_, at)| clock_ms() - at < NOTICE_MS)
    }
}

impl Behaviour for Waypoints {
    fn needs_vitals(&self) -> bool {
        self.deaths
    }

    fn hidden(&self, m: &ModuleState, id: &str) -> bool {
        id == "beam_opacity" && !m.bool("beam")
    }

    fn frame(&mut self, m: &ModuleState, g: &GameView) {
        waypoints::store().tick();
        self.deaths = m.bool("deaths");
        if !g.in_world || g.sample {
            waypoints::here().world = None;
            self.pressed_at = None;
            self.was_down = false;
            self.alive = false;
            if let Ok(mut c) = COMPASS.lock() {
                c.clear();
            }
            return;
        }
        self.dim = dim_of(g);
        self.world = g.world.clone();
        {
            let mut h = waypoints::here();
            h.world = g.world.clone();
            h.label = g.world_label.clone();
            h.dim = self.dim.clone();
            h.pos = [g.x, g.y, g.z];
        }
        let rev = waypoints::store().revision;
        if self.world != self.shown_world || rev != self.shown_rev {
            self.shown = self.world.as_deref().map(|w| waypoints::store().of(w)).unwrap_or_default();
            self.shown_world = self.world.clone();
            self.shown_rev = rev;
        }
        if self.deaths {
            self.deaths(g);
        }
        let key = m.setting("add").map_or(-1, Setting::as_key);
        ADD_KEY.store(key, std::sync::atomic::Ordering::Relaxed);
        let down = g.playing && key >= 0 && g.is_down(key);
        let now = clock_ms();
        if down && !self.was_down {
            self.pressed_at = Some(now);
            self.hold_fired = false;
        }
        if down && !self.hold_fired && self.pressed_at.is_some_and(|t| now - t >= HOLD_MS) {
            self.hold_fired = true;
            match g.look_block {
                Some(b) => self.add(b[0], b[1] + 1, b[2]),
                None => self.say(format!("Nothing to mark within {} blocks", LOOK_RANGE as i32)),
            }
        }
        if !down && self.was_down && self.pressed_at.take().is_some() && !self.hold_fired && g.playing {
            self.add(g.x.floor() as i32, g.y.floor() as i32, g.z.floor() as i32);
        }
        if !g.playing {
            self.pressed_at = None;
        }
        self.was_down = down;
        let nether = m.bool("nether");
        let marks: Vec<(f32, u32)> = self
            .shown
            .iter()
            .filter(|p| p.visible)
            .filter_map(|p| {
                let at = waypoints::place_in(p, &self.dim, nether)?;
                let (dx, dz) = (at[0] - g.x, at[2] - g.z);
                (dx * dx + dz * dz >= 4.0).then(|| (waypoints::heading(g.x, g.z, at[0], at[2]), p.color))
            })
            .collect();
        if let Ok(mut c) = COMPASS.lock() {
            *c = marks;
        }
    }

    fn overlays(&self, _m: &ModuleState, g: &GameView) -> bool {
        g.hud_allowed && g.in_world && !g.sample && (!self.shown.is_empty() || self.notice_active())
    }

    fn overlay(&mut self, m: &ModuleState, c: &mut Canvas, g: &GameView, now: f64, editing: bool) {
        if editing || g.sample {
            return;
        }
        let (w, h) = (c.width(), c.height());
        let k = m.num("size") / 100.0;
        let range = m.num("range");
        let nether = m.bool("nether");
        let use_mc = m.choice("font") == 0;
        let coords_mode = m.choice("coords"); // 0 On Hover, 1 Always, 2 Off
        let show_elev = m.bool("elevation");
        let beam_opacity = (m.num("beam_opacity") / 100.0).clamp(0.1, 1.0);
        let cam = g.camera.unwrap_or(Camera { pos: [g.x, g.y + 1.62, g.z], yaw: g.yaw, pitch: g.pitch, fov: 70.0 });
        let mut items: Vec<(&Point, [f64; 3], f64)> = self
            .shown
            .iter()
            .filter(|p| p.visible)
            .filter_map(|p| {
                let at = waypoints::place_in(p, &self.dim, nether)?;
                let d = ((at[0] - g.x).powi(2) + (at[1] - g.y).powi(2) + (at[2] - g.z).powi(2)).sqrt();
                (range >= 10000.0 || d <= f64::from(range)).then_some((p, at, d))
            })
            .collect();
        // Far ones first, so nearer markers sit on top.
        items.sort_by(|a, b| b.2.total_cmp(&a.2));
        if m.bool("beam") {
            for (p, at, d) in &items {
                if *d > 5.0 {
                    beam(c, &cam, *at, p.color, w, h, *d, beam_opacity, now);
                }
            }
        }
        for (p, at, d) in &items {
            let near = ((d - 1.5) / 3.0).clamp(0.0, 1.0) as f32;
            if near <= 0.01 {
                continue;
            }
            let converted = p.dim != self.dim;
            let q = cam.to_cam([at[0], at[1] + 1.15, at[2]]);
            let on = (q.depth > 0.05).then(|| cam.screen(q, w, h)).filter(|s| s[0] >= 0.0 && s[0] <= w && s[1] >= 0.0 && s[1] <= h);
            let icon = waypoints::ICONS[p.icon.min(waypoints::ICONS.len() - 1)];
            let dist_compact = waypoints::distance(*d).replace(' ', "");
            if let Some([sx, sy]) = on {
                let (dx, dy_scr) = (sx - w / 2.0, sy - h / 2.0);
                let focus = dx * dx + dy_scr * dy_scr < (44.0 * k).powi(2);
                let mut label = format!("{}  \u{00B7}  {}", p.name, waypoints::distance(*d));
                if converted {
                    label.push_str(&format!(" ({})", waypoints::dimension_name(&p.dim)));
                }
                let dy = (at[1] - g.y).round() as i32;
                let elev = if show_elev && dy.abs() >= 4 {
                    Some((if dy > 0 { format!("\u{25B2} +{dy}m") } else { format!("\u{25BC} {dy}m") }, dy > 0))
                } else {
                    None
                };
                let want_coords = coords_mode == 1 || (coords_mode == 0 && focus);
                let sub = want_coords.then(|| format!("({}, {}, {})", p.x, p.y, p.z));
                let a = near * if focus { 1.0 } else if converted { 0.65 } else { 0.92 };
                marker(
                    c,
                    sx,
                    sy,
                    k,
                    p.color,
                    icon,
                    &label,
                    elev.as_ref().map(|(s, up)| (s.as_str(), *up)),
                    sub.as_deref(),
                    use_mc,
                    a,
                );
            } else if m.bool("edge") {
                edge_arrow(
                    c,
                    w,
                    h,
                    k,
                    p.color,
                    icon,
                    &dist_compact,
                    q.right,
                    q.up,
                    q.depth,
                    use_mc,
                    near * if converted { 0.58 } else { 0.86 },
                );
            }
        }
        if let Some((text, at)) = self.notice.clone() {
            let age = clock_ms() - at;
            if age > NOTICE_MS {
                self.notice = None;
            } else {
                notice(c, w, h, &text, (age / 150.0).min((NOTICE_MS - age) / 400.0).min(1.0) as f32);
            }
        }
    }

    fn preview(&mut self, m: &ModuleState, c: &mut Canvas, _g: &GameView, now: f64, x: f32, y: f32, w: f32, h: f32) -> bool {
        c.fill(x, y, w, h, 0xFF0B0D12);
        c.gradient_v(x, y, w, h * 0.58, 0xFF233B59, 0xFF0F1724);
        c.gradient_v(x, y + h * 0.58, w, h * 0.42, 0xFF1A281A, 0xFF0B110B);
        c.push_clip(x, y, w, h);
        let k = (m.num("size") / 100.0 * 1.15).clamp(0.7, 1.5);
        let use_mc = m.choice("font") == 0;
        let show_elev = m.bool("elevation");
        let show_coords = m.choice("coords") != 2;
        let op = (m.num("beam_opacity") / 100.0).clamp(0.1, 1.0);
        if m.bool("beam") {
            let pulse = ((now / 2400.0) % 1.0) as f32;
            for (bx, by, col, scale) in [(x + w * 0.30, y + h * 0.52, 0xFF60A5FA, 1.0f32), (x + w * 0.70, y + h * 0.56, 0xFFF87171, 0.78)] {
                let bh = by - y;
                c.fill(bx - 5.5 * scale, y, 11.0 * scale, bh, theme::alpha(col, 0.12 * op));
                c.fill(bx - 2.2 * scale, y, 4.4 * scale, bh, theme::alpha(col, 0.36 * op));
                c.fill(bx - 0.9 * scale, y, 1.8 * scale, bh, theme::alpha(theme::mix(col, 0xFFFFFFFF, 0.65), 0.72 * op));
                let py = by - pulse * bh;
                c.gradient_v(bx - 2.4 * scale, py - 18.0, 4.8 * scale, 22.0, 0x00FFFFFF, theme::alpha(0xFFFFFFFF, 0.55 * op));
                draw_diamond(c, bx, by + 2.0, 6.0 * scale, theme::alpha(col, 0.25 * op));
                draw_diamond(c, bx, by + 2.0, 3.0 * scale, theme::alpha(col, 0.85 * op));
            }
        }
        marker(
            c,
            x + w * 0.30,
            y + h * 0.44,
            k,
            0xFF60A5FA,
            theme::I_HOME,
            "Base  \u{00B7}  243 m",
            show_elev.then_some(("\u{25B2} +12m", true)),
            show_coords.then_some("(120, 64, -300)"),
            use_mc,
            1.0,
        );
        marker(
            c,
            x + w * 0.70,
            y + h * 0.49,
            k * 0.88,
            0xFFF87171,
            waypoints::ICONS[waypoints::DEATH_ICON],
            "Death  \u{00B7}  1.2 km",
            show_elev.then_some(("\u{25BC} -8m", false)),
            None,
            use_mc,
            0.9,
        );
        c.pop_clip();
        true
    }
}

/// Sleek colored diamond badge + icon + crisp label (Minecraft or Native font) + elevation delta + optional coords pill.
#[allow(clippy::too_many_arguments)]
pub fn marker(
    c: &mut Canvas,
    x: f32,
    y: f32,
    k: f32,
    color: u32,
    icon: u32,
    label: &str,
    elev: Option<(&str, bool)>,
    sub: Option<&str>,
    use_mc_font: bool,
    alpha: f32,
) {
    c.push_alpha(alpha);
    let r = 12.5 * k;
    // Layered diamond badge: soft outer halo, crisp drop shadow, colored rim, obsidian core, colored icon.
    draw_diamond(c, x, y, r + 3.2 * k, theme::alpha(color, 0.20));
    draw_diamond(c, x, y + 1.2 * k, r + 1.0 * k, 0x88000000);
    draw_diamond(c, x, y, r, theme::alpha(color, 0.96));
    draw_diamond(c, x, y, r - 2.3 * k, 0xEE0D0F14);
    c.icon(icon, 10.5 * k, x, y, color);

    let ly = y + r + 5.5 * k;
    let (lh, next_y) = if use_mc_font {
        let px = mc_font::snap_px(c, 9.5 * k);
        let lh = mc_font::height(px);
        let lw = mc_font::width(c, label, px, false);
        let ew = elev.map_or(0.0, |(e, _)| mc_font::width(c, e, px, false) + 5.0 * k);
        let total = lw + ew;
        let rx = x - total / 2.0;
        c.round(rx - 6.0 * k, ly - 3.0 * k, total + 12.0 * k, lh + 6.0 * k, 5.0 * k, 0xA80B0C10);
        c.outline(rx - 6.0 * k, ly - 3.0 * k, total + 12.0 * k, lh + 6.0 * k, 5.0 * k, 1.0, theme::alpha(color, 0.35));
        mc_font::draw(c, label, rx, ly, px, 0xFFFFFFFF, true, false);
        if let Some((e, up)) = elev {
            let ecol = if up { 0xFF4ADE80 } else { 0xFFFBBF24 };
            mc_font::draw(c, e, rx + lw + 5.0 * k, ly, px, ecol, true, false);
        }
        (lh, ly + lh + 6.5 * k)
    } else {
        let fs = 10.5 * k;
        let lh = c.line_height(font::SEMIBOLD, fs);
        let lw = c.text_width(font::SEMIBOLD, fs, label);
        let ew = elev.map_or(0.0, |(e, _)| c.text_width(font::SEMIBOLD, fs * 0.9, e) + 5.0 * k);
        let total = lw + ew;
        let rx = x - total / 2.0;
        c.round(rx - 6.0 * k, ly - 2.5 * k, total + 12.0 * k, lh + 5.0 * k, 5.5 * k, 0xA80B0C10);
        c.outline(rx - 6.0 * k, ly - 2.5 * k, total + 12.0 * k, lh + 5.0 * k, 5.5 * k, 1.0, theme::alpha(color, 0.35));
        c.text(font::SEMIBOLD, fs, label, rx, ly, 0xFFFFFFFF);
        if let Some((e, up)) = elev {
            let ecol = if up { 0xFF4ADE80 } else { 0xFFFBBF24 };
            let elh = c.line_height(font::SEMIBOLD, fs * 0.9);
            c.text(font::SEMIBOLD, fs * 0.9, e, rx + lw + 5.0 * k, ly + (lh - elh) / 2.0, ecol);
        }
        (lh, ly + lh + 6.0 * k)
    };
    let _ = lh;

    if let Some(sub) = sub {
        if use_mc_font {
            let px = mc_font::snap_px(c, 8.5 * k);
            let sh = mc_font::height(px);
            let sw = mc_font::width(c, sub, px, false);
            c.round(x - sw / 2.0 - 5.0 * k, next_y - 2.0 * k, sw + 10.0 * k, sh + 4.0 * k, 4.5 * k, 0x900A0B0E);
            mc_font::draw(c, sub, x - sw / 2.0, next_y, px, 0xFFD4D4D8, true, false);
        } else {
            let ss = 9.5 * k;
            let sw = c.text_width(font::MEDIUM, ss, sub);
            let sh = c.line_height(font::MEDIUM, ss);
            c.round(x - sw / 2.0 - 5.0 * k, next_y - 2.0 * k, sw + 10.0 * k, sh + 4.0 * k, 4.5 * k, 0x900A0B0E);
            c.text(font::MEDIUM, ss, sub, x - sw / 2.0, next_y, 0xFFD4D4D8);
        }
    }
    c.pop_alpha();
}

/// Off screen (or behind you): diamond badge + icon + directional arrow + compact distance pill (`243m`) at screen edge.
#[allow(clippy::too_many_arguments)]
/// The HUD modules' rectangles (x, y, w, h) as painted last frame, so the edge arrows can keep clear of them.
static HUD_RECTS: std::sync::Mutex<Vec<[f32; 4]>> = std::sync::Mutex::new(Vec::new());

/// Called by the HUD pass each frame with where every HUD module was painted.
pub fn set_hud_rects(rects: Vec<[f32; 4]>) {
    if let Ok(mut r) = HUD_RECTS.lock() {
        *r = rects;
    }
}

/// Where a marker of half-size `r` at (x, y) on the screen's edge should go to stay clear of the HUD: slid along
/// that edge (up or down on a side, left or right on the top or bottom) to the nearest free spot.
fn avoid_hud(mut x: f32, mut y: f32, r: f32, w: f32, h: f32) -> (f32, f32) {
    let Ok(rects) = HUD_RECTS.lock() else { return (x, y) };
    let side = x <= r * 2.0 || x >= w - r * 2.0;
    for _ in 0..4 {
        let Some(&[rx, ry, rw, rh]) =
            rects.iter().find(|&&[rx, ry, rw, rh]| x + r > rx && x - r < rx + rw && y + r > ry && y - r < ry + rh)
        else {
            break;
        };
        if side {
            let (above, below) = (ry - r - 2.0, ry + rh + r + 2.0);
            let (ok_above, ok_below) = (above >= r, below <= h - r);
            y = if ok_above && (!ok_below || y - above <= below - y) { above } else if ok_below { below } else { y };
        } else {
            let (left, right) = (rx - r - 2.0, rx + rw + r + 2.0);
            let (ok_left, ok_right) = (left >= r, right <= w - r);
            x = if ok_left && (!ok_right || x - left <= right - x) { left } else if ok_right { right } else { x };
        }
    }
    (x, y)
}

fn edge_arrow(
    c: &mut Canvas,
    w: f32,
    h: f32,
    k: f32,
    color: u32,
    icon: u32,
    dist: &str,
    right: f64,
    up: f64,
    depth: f64,
    use_mc_font: bool,
    alpha: f32,
) {
    let (right, up) = if depth < 0.0 && right.abs() < 1e-6 && up.abs() < 1e-6 { (0.0, -1.0) } else { (right, up) };
    let ang = (-up).atan2(right);
    let (sin, cos) = ang.sin_cos();
    let m = f64::from(40.0 * k);
    let tx = if cos.abs() < 1e-6 { f64::MAX } else { (f64::from(w) / 2.0 - m) / cos.abs() };
    let ty = if sin.abs() < 1e-6 { f64::MAX } else { (f64::from(h) / 2.0 - m) / sin.abs() };
    let t = tx.min(ty);
    let x = (f64::from(w) / 2.0 + cos * t) as f32;
    let y = (f64::from(h) / 2.0 + sin * t) as f32;
    let (cos, sin) = (cos as f32, sin as f32);
    let r = 11.0 * k;
    // Keep clear of the HUD (Keystrokes, Coordinates ...) at that edge, the arrow and distance tag included.
    let (x, y) = avoid_hud(x, y, r + 14.0 * k, w, h);
    c.push_alpha(alpha);
    draw_diamond(c, x, y + 1.0 * k, r + 1.0 * k, 0x88000000);
    draw_diamond(c, x, y, r, theme::alpha(color, 0.95));
    draw_diamond(c, x, y, r - 2.2 * k, 0xEE0D0F14);
    c.icon(icon, 9.5 * k, x, y, color);

    // Directional arrow tip pointing outward toward the off-screen waypoint.
    let (tip, base, half) = (r + 9.0 * k, r + 2.5 * k, 4.6 * k);
    let (px_tip, py_tip) = (x + cos * tip, y + sin * tip);
    let (bx, by) = (x + cos * base, y + sin * base);
    let (nx, ny) = (-sin * half, cos * half);
    draw_triangle(c, px_tip, py_tip + 0.8 * k, bx + nx, by + ny + 0.8 * k, bx - nx, by - ny + 0.8 * k, 0x70000000);
    draw_triangle(c, px_tip, py_tip, bx + nx, by + ny, bx - nx, by - ny, color);

    // Compact distance pill tucked slightly below/inward so it never clips off the screen edge.
    let pill_y = if y > h - 54.0 * k { y - r - 15.0 * k } else { y + r + 4.0 * k };
    if use_mc_font {
        let px = mc_font::snap_px(c, 8.0 * k);
        let dw = mc_font::width(c, dist, px, false);
        let dh = mc_font::height(px);
        let pill_x = x.clamp(dw / 2.0 + 8.0 * k, w - dw / 2.0 - 8.0 * k);
        c.round(pill_x - dw / 2.0 - 4.0 * k, pill_y - 1.5 * k, dw + 8.0 * k, dh + 3.0 * k, 4.0 * k, 0xA00A0B0E);
        mc_font::draw(c, dist, pill_x - dw / 2.0, pill_y, px, 0xFFE4E4E7, true, false);
    } else {
        let fs = 8.5 * k;
        let dw = c.text_width(font::SEMIBOLD, fs, dist);
        let dh = c.line_height(font::SEMIBOLD, fs);
        let pill_x = x.clamp(dw / 2.0 + 8.0 * k, w - dw / 2.0 - 8.0 * k);
        c.round(pill_x - dw / 2.0 - 4.0 * k, pill_y - 1.5 * k, dw + 8.0 * k, dh + 3.0 * k, 4.0 * k, 0xA00A0B0E);
        c.text(font::SEMIBOLD, fs, dist, pill_x - dw / 2.0, pill_y, 0xFFE4E4E7);
    }
    c.pop_alpha();
}

/// 3-layer glowing beacon column (wide outer glow, vibrant mid column, bright white-tinted inner core) +
/// animated upward pulse shimmer + ground diamond anchor.
#[allow(clippy::too_many_arguments)]
fn beam(c: &mut Canvas, cam: &Camera, at: [f64; 3], color: u32, w: f32, h: f32, dist: f64, opacity: f32, now: f64) {
    let near = 0.2;
    let lerp = |p: waypoints::CamPoint, q: waypoints::CamPoint, t: f64| waypoints::CamPoint {
        depth: p.depth + (q.depth - p.depth) * t,
        right: p.right + (q.right - p.right) * t,
        up: p.up + (q.up - p.up) * t,
    };
    let clip_seg = |p0: [f64; 3], p1: [f64; 3]| -> Option<(waypoints::CamPoint, waypoints::CamPoint)> {
        let mut a = cam.to_cam(p0);
        let mut b = cam.to_cam(p1);
        if a.depth < near && b.depth < near {
            return None;
        }
        if a.depth < near {
            a = lerp(a, b, (near - a.depth) / (b.depth - a.depth));
        } else if b.depth < near {
            b = lerp(b, a, (near - b.depth) / (a.depth - b.depth));
        }
        Some((a, b))
    };

    let Some((a, b)) = clip_seg(at, [at[0], at[1] + 256.0, at[2]]) else {
        return;
    };
    let [ax, ay] = cam.screen(a, w, h);
    let [bx, by] = cam.screen(b, w, h);
    let (pa, pb) = (cam.px_per_block(a.depth, h), cam.px_per_block(b.depth, h));
    let fade = ((dist - 5.0) / 10.0).clamp(0.0, 1.0) as f32 * opacity;
    let (dx, dy) = (bx - ax, by - ay);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 0.5 {
        return;
    }
    let (nx, ny) = (-dy / len, dx / len);
    let core_col = theme::mix(color, 0xFFFFFFFF, 0.65);

    // 3-layer glowing beacon column: outer glow, vibrant mid, white-tinted core.
    for (blocks, min_px, col, strength) in [
        (1.15f32, 5.5f32, color, 0.12f32),
        (0.42, 2.2, color, 0.36),
        (0.14, 1.0, core_col, 0.72),
    ] {
        let (wa, wb) = ((blocks * pa).max(min_px) / 2.0, (blocks * pb).max(min_px) / 2.0);
        c.free_quad(
            None,
            [ax + nx * wa, ay + ny * wa, bx + nx * wb, by + ny * wb, bx - nx * wb, by - ny * wb, ax - nx * wa, ay - ny * wa],
            [0.0; 8],
            theme::alpha(col, strength * fade),
        );
    }

    // Animated upward pulse shimmer traveling along the beacon every 2.4 s.
    let phase = ((now / 2400.0) % 1.0) as f64;
    let y0 = at[1] + phase * 96.0;
    let y1 = y0 + 10.0;
    if let Some((sa, sb)) = clip_seg([at[0], y0, at[2]], [at[0], y1, at[2]]) {
        let [sx0, sy0] = cam.screen(sa, w, h);
        let [sx1, sy1] = cam.screen(sb, w, h);
        let (spa, spb) = (cam.px_per_block(sa.depth, h), cam.px_per_block(sb.depth, h));
        let (wa, wb) = ((0.52 * spa).max(2.6) / 2.0, (0.52 * spb).max(2.6) / 2.0);
        let pulse_alpha = ((1.0 - phase as f32) * 0.45 * fade).clamp(0.0, 0.65);
        c.free_quad(
            None,
            [
                sx0 + nx * wa,
                sy0 + ny * wa,
                sx1 + nx * wb,
                sy1 + ny * wb,
                sx1 - nx * wb,
                sy1 - ny * wb,
                sx0 - nx * wa,
                sy0 - ny * wa,
            ],
            [0.0; 8],
            theme::alpha(0xFFFFFFFF, pulse_alpha),
        );
    }

    // Ground diamond anchor at the base of the beam.
    let gq = cam.to_cam([at[0], at[1] + 0.08, at[2]]);
    if gq.depth > near {
        let [gx, gy] = cam.screen(gq, w, h);
        if gx >= -24.0 && gx <= w + 24.0 && gy >= -24.0 && gy <= h + 24.0 {
            let r = (cam.px_per_block(gq.depth, h) * 0.48).clamp(3.0, 14.0);
            draw_diamond(c, gx, gy, r * 1.45, theme::alpha(color, 0.24 * fade));
            draw_diamond(c, gx, gy, r * 0.78, theme::alpha(color, 0.78 * fade));
            draw_diamond(c, gx, gy, r * 0.32, theme::alpha(0xFFFFFFFF, 0.9 * fade));
        }
    }
}

fn notice(c: &mut Canvas, w: f32, h: f32, text: &str, a: f32) {
    let fs = 12.0;
    let tw = c.text_width(font::SEMIBOLD, fs, text);
    let lh = c.line_height(font::SEMIBOLD, fs);
    let (x, y) = ((w - tw) / 2.0, h * 0.7);
    c.push_alpha(a);
    c.round(x - 26.0, y - 7.0, tw + 38.0, lh + 14.0, 9.0, 0xB3000000);
    c.icon(theme::I_MAP_PIN, 12.0, x - 12.0, y + lh / 2.0, theme::ACCENT);
    c.text(font::SEMIBOLD, fs, text, x + 2.0, y, 0xFFFFFFFF);
    c.pop_alpha();
}
