//! Waypoints (the Java mod's `WaypointsModule`): named points per world, drawn over the game where they are (or at the
//! screen's edge, pointing the way), with a light beam you can see from far away. The add key drops one where you
//! stand; holding it drops one on the block you look at. Dying leaves a death point that goes away once you are back
//! there. The list lives on the Waypoints page of the Native menu; the Direction strip shows them too.
//!
//! Everything is drawn by the Native overlay from the camera's position, direction and field of view, and it only ever
//! shows things to you: nothing is sent and nothing in the game changes.
use super::module::*;
use super::setting::Setting;
use super::waypoints::{self, Camera, Point};
use horizon_client::native_ui::canvas::clock_ms;
use horizon_client::native_ui::{Canvas, font, theme};
use std::collections::HashSet;
use std::sync::Mutex;

pub const ID: &str = "waypoints";
/// GLFW N.
pub const KEY_N: i32 = 78;
const HOLD_MS: f64 = 400.0;
pub const LOOK_RANGE: f64 = 128.0;
const NOTICE_MS: f64 = 2600.0;

pub fn all() -> Vec<Module> {
    vec![Module::new(
        ID,
        "Waypoints",
        "Mark places and find your way back: markers over the world, beams, death points and a list per world.",
        VISUAL,
        theme::I_MAP_PIN,
        true,
        vec![
            Setting::key("add", "Add waypoint (hold: where you look)", KEY_N),
            Setting::num("size", "Marker size", 100.0, 60.0, 160.0, 10.0, "%"),
            Setting::num("range", "Show up to", 10000.0, 100.0, 10000.0, 100.0, " m"),
            Setting::bool("beam", "Light beam", true),
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

static ADD_KEY: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(KEY_N);

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
                s.remove(&w, p.id); // only the latest one matters
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
        // the add key: a tap adds where you stand, holding it marks the block you look at
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
        // the compass strip's marks
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

    fn overlay(&mut self, m: &ModuleState, c: &mut Canvas, g: &GameView, _now: f64, editing: bool) {
        if editing || g.sample {
            return;
        }
        let (w, h) = (c.width(), c.height());
        let k = m.num("size") / 100.0;
        let range = m.num("range");
        let nether = m.bool("nether");
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
        // far ones first, so the nearer markers sit on top
        items.sort_by(|a, b| b.2.total_cmp(&a.2));
        if m.bool("beam") {
            for (p, at, d) in &items {
                if *d > 6.0 {
                    beam(c, &cam, *at, p.color, w, h, *d);
                }
            }
        }
        for (p, at, d) in &items {
            let near = ((d - 1.5) / 3.0).clamp(0.0, 1.0) as f32; // fades out once you are there
            if near <= 0.01 {
                continue;
            }
            let converted = p.dim != self.dim;
            let q = cam.to_cam([at[0], at[1] + 1.1, at[2]]);
            let on = (q.depth > 0.05).then(|| cam.screen(q, w, h)).filter(|s| s[0] >= 0.0 && s[0] <= w && s[1] >= 0.0 && s[1] <= h);
            let icon = waypoints::ICONS[p.icon.min(waypoints::ICONS.len() - 1)];
            if let Some([sx, sy]) = on {
                let (dx, dy) = (sx - w / 2.0, sy - h / 2.0);
                let focus = dx * dx + dy * dy < (40.0 * k).powi(2);
                let mut label = format!("{}  \u{B7}  {}", p.name, waypoints::distance(*d));
                if converted {
                    label.push_str(&format!("  ({})", waypoints::dimension_name(&p.dim)));
                }
                let sub = focus.then(|| format!("{} {} {}", p.x, p.y, p.z));
                let a = near * if focus { 1.0 } else if converted { 0.6 } else { 0.85 };
                marker(c, sx, sy, k, p.color, icon, &label, sub.as_deref(), a);
            } else if m.bool("edge") {
                edge_arrow(c, w, h, k, p.color, icon, q.right, q.up, q.depth, near * if converted { 0.55 } else { 0.8 });
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

    fn preview(&mut self, m: &ModuleState, c: &mut Canvas, _g: &GameView, _now: f64, x: f32, y: f32, w: f32, h: f32) -> bool {
        c.fill(x, y, w, h, 0xFF0B0D12);
        c.gradient_v(x, y, w, h * 0.6, 0xFF2B4A6F, 0xFF101826);
        c.gradient_v(x, y + h * 0.6, w, h * 0.4, 0xFF1C2A1A, 0xFF0D130C);
        c.push_clip(x, y, w, h);
        let k = m.num("size") / 100.0 * 1.2;
        if m.bool("beam") {
            c.fill(x + w * 0.3 - 3.0, y, 6.0, h * 0.42, theme::alpha(0xFF60A5FA, 0.18));
            c.fill(x + w * 0.3 - 1.0, y, 2.0, h * 0.42, theme::alpha(0xFF60A5FA, 0.5));
            c.fill(x + w * 0.68 - 2.0, y, 4.0, h * 0.5, theme::alpha(0xFFF87171, 0.16));
            c.fill(x + w * 0.68 - 0.6, y, 1.2, h * 0.5, theme::alpha(0xFFF87171, 0.5));
        }
        marker(c, x + w * 0.3, y + h * 0.42, k, 0xFF60A5FA, theme::I_HOME, "Base  \u{B7}  243 m", Some("120 64 -300"), 1.0);
        marker(c, x + w * 0.68, y + h * 0.5, k * 0.85, 0xFFF87171, waypoints::ICONS[waypoints::DEATH_ICON], "Death  \u{B7}  1.2 km", None, 0.85);
        c.pop_clip();
        true
    }
}

/// A marker: the icon in a ring, the name and distance under it, and the coordinates when you look right at it.
pub fn marker(c: &mut Canvas, x: f32, y: f32, k: f32, color: u32, icon: u32, label: &str, sub: Option<&str>, alpha: f32) {
    c.push_alpha(alpha);
    let r = 10.0 * k;
    c.circle(x, y, r + 2.0 * k, 0x8C000000);
    c.circle(x, y, r, theme::alpha(color, 0.95));
    c.circle(x, y, r - 2.2 * k, 0xE6101114);
    c.icon(icon, 11.0 * k, x, y, color);
    let fs = 10.5 * k;
    let tw = c.text_width(font::SEMIBOLD, fs, label);
    let lh = c.line_height(font::SEMIBOLD, fs);
    let ly = y + r + 6.0 * k;
    c.round(x - tw / 2.0 - 6.0 * k, ly - 2.0 * k, tw + 12.0 * k, lh + 4.0 * k, 6.0 * k, 0x99000000);
    c.text(font::SEMIBOLD, fs, label, x - tw / 2.0, ly, 0xFFFFFFFF);
    if let Some(sub) = sub {
        let ss = 9.5 * k;
        let sw = c.text_width(font::MEDIUM, ss, sub);
        let sy = ly + lh + 6.0 * k;
        let sh = c.line_height(font::MEDIUM, ss);
        c.round(x - sw / 2.0 - 5.0 * k, sy - 2.0 * k, sw + 10.0 * k, sh + 4.0 * k, 5.0 * k, 0x80000000);
        c.text(font::MEDIUM, ss, sub, x - sw / 2.0, sy, 0xD9FFFFFF);
    }
    c.pop_alpha();
}

/// Off screen (or behind you): the icon at the screen's edge with a small arrow pointing towards the waypoint.
#[allow(clippy::too_many_arguments)]
fn edge_arrow(c: &mut Canvas, w: f32, h: f32, k: f32, color: u32, icon: u32, right: f64, up: f64, depth: f64, alpha: f32) {
    let (right, up) = if depth < 0.0 && right.abs() < 1e-6 && up.abs() < 1e-6 { (0.0, -1.0) } else { (right, up) };
    let ang = (-up).atan2(right);
    let (sin, cos) = ang.sin_cos();
    let m = f64::from(34.0 * k);
    let tx = if cos.abs() < 1e-6 { f64::MAX } else { (f64::from(w) / 2.0 - m) / cos.abs() };
    let ty = if sin.abs() < 1e-6 { f64::MAX } else { (f64::from(h) / 2.0 - m) / sin.abs() };
    let t = tx.min(ty);
    let x = (f64::from(w) / 2.0 + cos * t) as f32;
    let y = (f64::from(h) / 2.0 + sin * t) as f32;
    let (cos, sin) = (cos as f32, sin as f32);
    c.push_alpha(alpha);
    let r = 9.0 * k;
    c.circle(x, y, r + 2.0 * k, 0x8C000000);
    c.circle(x, y, r, theme::alpha(color, 0.95));
    c.circle(x, y, r - 2.0 * k, 0xE6101114);
    c.icon(icon, 10.0 * k, x, y, color);
    // the arrow tip, just outside the ring
    let (tip, base, half) = (r + 9.0 * k, r + 3.0 * k, 4.5 * k);
    let (px, py) = (x + cos * tip, y + sin * tip);
    let (bx, by) = (x + cos * base, y + sin * base);
    let (nx, ny) = (-sin * half, cos * half);
    c.free_quad(None, [px, py, bx + nx, by + ny, bx - nx, by - ny, bx - nx, by - ny], [0.0; 8], color);
    c.pop_alpha();
}

/// A column of light from the waypoint up to the build limit, clipped to what is in front of the camera.
fn beam(c: &mut Canvas, cam: &Camera, at: [f64; 3], color: u32, w: f32, h: f32, dist: f64) {
    let mut a = cam.to_cam(at);
    let mut b = cam.to_cam([at[0], at[1] + 256.0, at[2]]);
    let near = 0.2;
    if a.depth < near && b.depth < near {
        return;
    }
    let lerp = |p: waypoints::CamPoint, q: waypoints::CamPoint, t: f64| waypoints::CamPoint {
        depth: p.depth + (q.depth - p.depth) * t,
        right: p.right + (q.right - p.right) * t,
        up: p.up + (q.up - p.up) * t,
    };
    if a.depth < near {
        a = lerp(a, b, (near - a.depth) / (b.depth - a.depth));
    } else if b.depth < near {
        b = lerp(b, a, (near - b.depth) / (a.depth - b.depth));
    }
    let [ax, ay] = cam.screen(a, w, h);
    let [bx, by] = cam.screen(b, w, h);
    let (pa, pb) = (cam.px_per_block(a.depth, h), cam.px_per_block(b.depth, h));
    let fade = ((dist - 6.0) / 10.0).min(1.0) as f32;
    let (dx, dy) = (bx - ax, by - ay);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 0.5 {
        return;
    }
    let (nx, ny) = (-dy / len, dx / len);
    for (blocks, min, strength) in [(0.9f32, 3.0f32, 0.16f32), (0.3, 1.2, 0.5)] {
        let (wa, wb) = ((blocks * pa).max(min) / 2.0, (blocks * pb).max(min) / 2.0);
        c.free_quad(
            None,
            [ax + nx * wa, ay + ny * wa, bx + nx * wb, by + ny * wb, bx - nx * wb, by - ny * wb, ax - nx * wa, ay - ny * wa],
            [0.0; 8],
            theme::alpha(color, strength * fade),
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
