//! Waypoints (the Java mod's `WaypointsModule`): named points per world, drawn over the game as clean Lunar-style
//! bordered tags (`Castle [10m]`) with a subtle 3D ground circle beneath them (plus optional beam, coords, elevation
//! delta, and off-screen edge arrows). Pressing the add key opens the floating "Create A Waypoint" modal at your
//! position (holding it opens it at the block you look at). Dying leaves a death point that goes away once you return.

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
        "Mark places and find your way back: clean in-world waypoint tags, ground rings, optional beams, death points and a list per world.",
        VISUAL,
        theme::I_MAP_PIN,
        true,
        vec![
            Setting::key("add", "Create waypoint (hold: where you look)", KEY_J),
            Setting::num("size", "Marker size", 100.0, 60.0, 160.0, 10.0, "%"),
            Setting::num("range", "Show up to", 10000.0, 100.0, 10000.0, 100.0, " m"),
            Setting::choice("font", "Font", 0, &["Minecraft", "Native"]),
            Setting::choice("coords", "Coordinates", 2, &["On Hover", "Always", "Off"]),
            Setting::bool("elevation", "Show height difference", false),
            Setting::bool("ground_ring", "Ground ring", true),
            Setting::bool("beam", "Light beam", false),
            Setting::num("beam_opacity", "Beam opacity", 70.0, 10.0, 100.0, 5.0, "%"),
            Setting::bool("edge", "Point to waypoints off screen", true),
            Setting::bool("deaths", "Death points", true),
            Setting::bool("nether", "Show Overworld points in the Nether (1:8)", true),
        ],
        None,
        Box::new(Waypoints { deaths: true, ..Default::default() }),
    )]
}

/// Pending request to open the floating "Create A Waypoint" modal over the live world.
static OPEN_CREATE: Mutex<Option<([f64; 3], String)>> = Mutex::new(None);

fn request_open_create(pos: [f64; 3], dim: &str) {
    if let Ok(mut slot) = OPEN_CREATE.lock() {
        *slot = Some((pos, dim.to_owned()));
    }
}

pub fn take_open_create() -> Option<([f64; 3], String)> {
    OPEN_CREATE.lock().ok().and_then(|mut slot| slot.take())
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

    fn deaths(&mut self, g: &GameView) {
        let Some(w) = self.world.clone() else { return };
        let now = g.vitals.health > 0.0;
        if self.alive && !now {
            let mut s = waypoints::store();
            for p in s.of(&w).into_iter().filter(|p| p.death) {
                s.remove(&w, p.id);
            }
            let mut p = Point::new(
                "Death",
                g.x.floor() as i32,
                g.y.floor() as i32,
                g.z.floor() as i32,
                &self.dim,
                0xFFF87171,
                waypoints::DEATH_ICON,
            );
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
                Some(b) => request_open_create([f64::from(b[0]) + 0.5, f64::from(b[1] + 1), f64::from(b[2]) + 0.5], &self.dim),
                None => self.say(format!("Nothing to mark within {} blocks", LOOK_RANGE as i32)),
            }
        }
        if !down && self.was_down && self.pressed_at.take().is_some() && !self.hold_fired && g.playing {
            request_open_create([g.x, g.y, g.z], &self.dim);
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
                (dx * dx + dz * dz >= 4.0).then(|| (waypoints::heading(g.x, g.z, at[0], at[2]), waypoints::live_color(p, now)))
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
        let show_ring = m.bool("ground_ring");
        let beam_opacity = (m.num("beam_opacity") / 100.0).clamp(0.1, 1.0);
        let cam = g.camera.unwrap_or(Camera { pos: [g.x, g.y + 1.62, g.z], yaw: g.yaw, pitch: g.pitch, fov: 70.0 });
        let mut items: Vec<(&Point, [f64; 3], f64, u32)> = self
            .shown
            .iter()
            .filter(|p| p.visible)
            .filter_map(|p| {
                let at = waypoints::place_in(p, &self.dim, nether)?;
                let d = ((at[0] - g.x).powi(2) + (at[1] - g.y).powi(2) + (at[2] - g.z).powi(2)).sqrt();
                (range >= 10000.0 || d <= f64::from(range)).then_some((p, at, d, waypoints::live_color(p, now)))
            })
            .collect();
        // Far ones first, so nearer markers sit on top.
        items.sort_by(|a, b| b.2.total_cmp(&a.2));
        if m.bool("beam") {
            for (_, at, d, col) in &items {
                if *d > 5.0 {
                    beam(c, &cam, *at, *col, w, h, *d, beam_opacity, now);
                }
            }
        }
        if show_ring {
            for (_, at, d, col) in &items {
                if *d > 1.5 && *d < 120.0 {
                    ground_circle(c, &cam, *at, *col, w, h, *d);
                }
            }
        }
        for (p, at, d, col) in &items {
            let near = ((d - 1.2) / 2.6).clamp(0.0, 1.0) as f32;
            if near <= 0.01 {
                continue;
            }
            let converted = p.dim != self.dim;
            let q = cam.to_cam([at[0], at[1] + 1.15, at[2]]);
            let on = (q.depth > 0.05).then(|| cam.screen(q, w, h)).filter(|s| s[0] >= 0.0 && s[0] <= w && s[1] >= 0.0 && s[1] <= h);
            let icon = waypoints::ICONS[p.icon.min(waypoints::ICONS.len() - 1)];
            let dist_compact = waypoints::compact_distance(*d);
            if let Some([sx, sy]) = on {
                let (dx, dy_scr) = (sx - w / 2.0, sy - h / 2.0);
                let focus = dx * dx + dy_scr * dy_scr < (48.0 * k).powi(2);
                let mut label = format!("{} [{}]", p.name, dist_compact);
                if converted {
                    label.push_str(&format!(" ({})", waypoints::dimension_name(&p.dim)));
                }
                let dy = (at[1] - g.y).round() as i32;
                let elev = if show_elev && dy.abs() >= 4 {
                    Some((if dy > 0 { format!("\u{25B2}+{dy}m") } else { format!("\u{25BC}{dy}m") }, dy > 0))
                } else {
                    None
                };
                let want_coords = coords_mode == 1 || (coords_mode == 0 && focus);
                let sub = want_coords.then(|| format!("({}, {}, {})", p.x, p.y, p.z));
                let a = near * if focus { 1.0 } else if converted { 0.68 } else { 0.96 };
                marker(
                    c,
                    sx,
                    sy,
                    k,
                    *col,
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
                    *col,
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
        let k = (m.num("size") / 100.0 * 1.12).clamp(0.7, 1.45);
        let use_mc = m.choice("font") == 0;
        let show_elev = m.bool("elevation");
        let show_coords = m.choice("coords") != 2;
        let show_ring = m.bool("ground_ring");
        let op = (m.num("beam_opacity") / 100.0).clamp(0.1, 1.0);
        if m.bool("beam") {
            let pulse = ((now / 2400.0) % 1.0) as f32;
            for (bx, by, col, scale) in [(x + w * 0.32, y + h * 0.66, 0xFF22C55E, 1.0f32), (x + w * 0.72, y + h * 0.62, 0xFFF87171, 0.78)] {
                let bh = by - y;
                c.fill(bx - 5.5 * scale, y, 11.0 * scale, bh, theme::alpha(col, 0.12 * op));
                c.fill(bx - 2.2 * scale, y, 4.4 * scale, bh, theme::alpha(col, 0.36 * op));
                c.fill(bx - 0.9 * scale, y, 1.8 * scale, bh, theme::alpha(theme::mix(col, 0xFFFFFFFF, 0.65), 0.72 * op));
                let py = by - pulse * bh;
                c.gradient_v(bx - 2.4 * scale, py - 18.0, 4.8 * scale, 22.0, 0x00FFFFFF, theme::alpha(0xFFFFFFFF, 0.55 * op));
            }
        }
        if show_ring {
            preview_ring(c, x + w * 0.32, y + h * 0.68, 16.0, 5.5, 0xFF22C55E);
            preview_ring(c, x + w * 0.72, y + h * 0.64, 12.0, 4.2, 0xFFF87171);
        }
        marker(
            c,
            x + w * 0.32,
            y + h * 0.44,
            k,
            0xFF22C55E,
            "Castle [10m]",
            show_elev.then_some(("\u{25B2}+12m", true)),
            show_coords.then_some("(18, 105, 154)"),
            use_mc,
            1.0,
        );
        marker(
            c,
            x + w * 0.72,
            y + h * 0.47,
            k * 0.9,
            0xFFF87171,
            "Death [1.2km]",
            show_elev.then_some(("\u{25BC}-8m", false)),
            None,
            use_mc,
            0.94,
        );
        c.pop_clip();
        true
    }
}

/// Clean Lunar-style waypoint tag: dark translucent color-tinted box with a crisp 1px colored border
/// and centered white text (`Castle [10m]`). No floating diamond badge above the text.
#[allow(clippy::too_many_arguments)]
pub fn marker(
    c: &mut Canvas,
    x: f32,
    y: f32,
    k: f32,
    color: u32,
    label: &str,
    elev: Option<(&str, bool)>,
    sub: Option<&str>,
    use_mc_font: bool,
    alpha: f32,
) {
    let raw_a = ((color >> 24) & 0xFF) as f32;
    let col_alpha = if raw_a <= 0.5 { 1.0 } else { (raw_a / 255.0).clamp(0.15, 1.0) };
    let opaque = 0xFF000000 | (color & 0x00FF_FFFF);
    let fill = theme::alpha(theme::mix(0xFF0A0B0F, opaque, 0.22), 0.72 * col_alpha);
    let border = theme::alpha(opaque, 0.94 * col_alpha);
    let rad = 2.8 * k;

    c.push_alpha(alpha);
    let next_y = if use_mc_font {
        let px = mc_font::snap_px(c, 9.5 * k);
        let lh = mc_font::height(px);
        let lw = mc_font::width(c, label, px, false);
        let ew = elev.map_or(0.0, |(e, _)| mc_font::width(c, e, px, false) + 5.0 * k);
        let total = lw + ew;
        let pad_x = 6.5 * k;
        let pad_y = 3.5 * k;
        let bx = x - total / 2.0 - pad_x;
        let by = y - lh / 2.0 - pad_y;
        let bw = total + pad_x * 2.0;
        let bh = lh + pad_y * 2.0;
        c.round(bx, by, bw, bh, rad, fill);
        c.outline(bx, by, bw, bh, rad, 1.15, border);
        let tx = x - total / 2.0;
        let ty = y - lh / 2.0;
        mc_font::draw(c, label, tx, ty, px, 0xFFFFFFFF, true, false);
        if let Some((e, up)) = elev {
            let ecol = if up { 0xFF4ADE80 } else { 0xFFFBBF24 };
            mc_font::draw(c, e, tx + lw + 5.0 * k, ty, px, ecol, true, false);
        }
        by + bh + 4.0 * k
    } else {
        let fs = 10.5 * k;
        let lh = c.line_height(font::SEMIBOLD, fs);
        let lw = c.text_width(font::SEMIBOLD, fs, label);
        let ew = elev.map_or(0.0, |(e, _)| c.text_width(font::SEMIBOLD, fs * 0.9, e) + 5.0 * k);
        let total = lw + ew;
        let pad_x = 6.5 * k;
        let pad_y = 3.2 * k;
        let bx = x - total / 2.0 - pad_x;
        let by = y - lh / 2.0 - pad_y;
        let bw = total + pad_x * 2.0;
        let bh = lh + pad_y * 2.0;
        c.round(bx, by, bw, bh, rad, fill);
        c.outline(bx, by, bw, bh, rad, 1.15, border);
        let tx = x - total / 2.0;
        let ty = y - lh / 2.0;
        c.text(font::SEMIBOLD, fs, label, tx, ty, 0xFFFFFFFF);
        if let Some((e, up)) = elev {
            let ecol = if up { 0xFF4ADE80 } else { 0xFFFBBF24 };
            let elh = c.line_height(font::SEMIBOLD, fs * 0.9);
            c.text(font::SEMIBOLD, fs * 0.9, e, tx + lw + 5.0 * k, ty + (lh - elh) / 2.0, ecol);
        }
        by + bh + 4.0 * k
    };

    if let Some(sub) = sub {
        if use_mc_font {
            let px = mc_font::snap_px(c, 8.2 * k);
            let sh = mc_font::height(px);
            let sw = mc_font::width(c, sub, px, false);
            c.round(x - sw / 2.0 - 5.0 * k, next_y - 1.5 * k, sw + 10.0 * k, sh + 3.0 * k, 2.5 * k, 0x980A0B0E);
            mc_font::draw(c, sub, x - sw / 2.0, next_y, px, 0xFFD4D4D8, true, false);
        } else {
            let ss = 9.2 * k;
            let sw = c.text_width(font::MEDIUM, ss, sub);
            let sh = c.line_height(font::MEDIUM, ss);
            c.round(x - sw / 2.0 - 5.0 * k, next_y - 1.5 * k, sw + 10.0 * k, sh + 3.0 * k, 2.5 * k, 0x980A0B0E);
            c.text(font::MEDIUM, ss, sub, x - sw / 2.0, next_y, 0xFFD4D4D8);
        }
    }
    c.pop_alpha();
}

/// Subtle 3D colored ring projected flat on the ground directly beneath the waypoint.
fn ground_circle(c: &mut Canvas, cam: &Camera, at: [f64; 3], color: u32, w: f32, h: f32, dist: f64) {
    let raw_a = ((color >> 24) & 0xFF) as f32;
    let col_alpha = if raw_a <= 0.5 { 1.0 } else { (raw_a / 255.0).clamp(0.15, 1.0) };
    let opaque = 0xFF000000 | (color & 0x00FF_FFFF);
    let fade = (((dist - 1.5) / 3.0).clamp(0.0, 1.0) * (1.0 - ((dist - 40.0) / 80.0).clamp(0.0, 0.85))) as f32;
    let a = fade * 0.72 * col_alpha;
    if a <= 0.02 {
        return;
    }
    const SEGS: usize = 20;
    let radius = 0.58;
    let y = at[1] + 0.04;
    let mut pts = [[0.0f32; 2]; SEGS];
    for (i, slot) in pts.iter_mut().enumerate() {
        let ang = (i as f64) * std::f64::consts::TAU / (SEGS as f64);
        let q = cam.to_cam([at[0] + ang.cos() * radius, y, at[2] + ang.sin() * radius]);
        if q.depth <= 0.12 {
            return;
        }
        *slot = cam.screen(q, w, h);
    }
    let thickness = (cam.px_per_block(dist, h) * 0.055).clamp(1.1, 2.4);
    let stroke = theme::alpha(opaque, a);
    for i in 0..SEGS {
        let p0 = pts[i];
        let p1 = pts[(i + 1) % SEGS];
        let (dx, dy) = (p1[0] - p0[0], p1[1] - p0[1]);
        let len = (dx * dx + dy * dy).sqrt();
        if len < 0.2 {
            continue;
        }
        let (nx, ny) = (-dy / len * (thickness * 0.5), dx / len * (thickness * 0.5));
        c.free_quad(
            None,
            [p0[0] + nx, p0[1] + ny, p1[0] + nx, p1[1] + ny, p1[0] - nx, p1[1] - ny, p0[0] - nx, p0[1] - ny],
            [0.0; 8],
            stroke,
        );
    }
}

fn preview_ring(c: &mut Canvas, cx: f32, cy: f32, rx: f32, ry: f32, color: u32) {
    const SEGS: usize = 18;
    let stroke = theme::alpha(color, 0.75);
    for i in 0..SEGS {
        let a0 = (i as f32) * std::f32::consts::TAU / (SEGS as f32);
        let a1 = ((i + 1) as f32) * std::f32::consts::TAU / (SEGS as f32);
        let (x0, y0) = (cx + a0.cos() * rx, cy + a0.sin() * ry);
        let (x1, y1) = (cx + a1.cos() * rx, cy + a1.sin() * ry);
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len = (dx * dx + dy * dy).sqrt().max(0.1);
        let (nx, ny) = (-dy / len * 0.85, dx / len * 0.85);
        c.free_quad(None, [x0 + nx, y0 + ny, x1 + nx, y1 + ny, x1 - nx, y1 - ny, x0 - nx, y0 - ny], [0.0; 8], stroke);
    }
}

/// The HUD modules' rectangles (x, y, w, h) as painted last frame, so the edge arrows can keep clear of them.
static HUD_RECTS: std::sync::Mutex<Vec<[f32; 4]>> = std::sync::Mutex::new(Vec::new());

/// Called by the HUD pass each frame with where every HUD module was painted.
pub fn set_hud_rects(rects: Vec<[f32; 4]>) {
    if let Ok(mut r) = HUD_RECTS.lock() {
        *r = rects;
    }
}

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

#[allow(clippy::too_many_arguments)]
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
    let opaque = 0xFF000000 | (color & 0x00FF_FFFF);
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
    let (x, y) = avoid_hud(x, y, r + 14.0 * k, w, h);
    c.push_alpha(alpha);
    draw_diamond(c, x, y + 1.0 * k, r + 1.0 * k, 0x88000000);
    draw_diamond(c, x, y, r, theme::alpha(opaque, 0.95));
    draw_diamond(c, x, y, r - 2.2 * k, 0xEE0D0F14);
    c.icon(icon, 9.5 * k, x, y, opaque);

    let (tip, base, half) = (r + 9.0 * k, r + 2.5 * k, 4.6 * k);
    let (px_tip, py_tip) = (x + cos * tip, y + sin * tip);
    let (bx, by) = (x + cos * base, y + sin * base);
    let (nx, ny) = (-sin * half, cos * half);
    draw_triangle(c, px_tip, py_tip + 0.8 * k, bx + nx, by + ny + 0.8 * k, bx - nx, by - ny + 0.8 * k, 0x70000000);
    draw_triangle(c, px_tip, py_tip, bx + nx, by + ny, bx - nx, by - ny, opaque);

    let pill_y = if y > h - 54.0 * k { y - r - 15.0 * k } else { y + r + 4.0 * k };
    if use_mc_font {
        let px = mc_font::snap_px(c, 8.0 * k);
        let dw = mc_font::width(c, dist, px, false);
        let dh = mc_font::height(px);
        let pill_x = x.clamp(dw / 2.0 + 8.0 * k, w - dw / 2.0 - 8.0 * k);
        c.round(pill_x - dw / 2.0 - 4.0 * k, pill_y - 1.5 * k, dw + 8.0 * k, dh + 3.0 * k, 3.0 * k, 0xA00A0B0E);
        mc_font::draw(c, dist, pill_x - dw / 2.0, pill_y, px, 0xFFE4E4E7, true, false);
    } else {
        let fs = 8.5 * k;
        let dw = c.text_width(font::SEMIBOLD, fs, dist);
        let dh = c.line_height(font::SEMIBOLD, fs);
        let pill_x = x.clamp(dw / 2.0 + 8.0 * k, w - dw / 2.0 - 8.0 * k);
        c.round(pill_x - dw / 2.0 - 4.0 * k, pill_y - 1.5 * k, dw + 8.0 * k, dh + 3.0 * k, 3.0 * k, 0xA00A0B0E);
        c.text(font::SEMIBOLD, fs, dist, pill_x - dw / 2.0, pill_y, 0xFFE4E4E7);
    }
    c.pop_alpha();
}

/// Optional 3-layer glowing beacon column + animated upward pulse shimmer.
#[allow(clippy::too_many_arguments)]
fn beam(c: &mut Canvas, cam: &Camera, at: [f64; 3], color: u32, w: f32, h: f32, dist: f64, opacity: f32, now: f64) {
    let opaque = 0xFF000000 | (color & 0x00FF_FFFF);
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
    let core_col = theme::mix(opaque, 0xFFFFFFFF, 0.65);

    for (blocks, min_px, col, strength) in [
        (1.15f32, 5.5f32, opaque, 0.12f32),
        (0.42, 2.2, opaque, 0.36),
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
