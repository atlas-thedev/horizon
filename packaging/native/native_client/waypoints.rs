//! The player's waypoints (the Java mod's `Waypoints`): named points per world (a server address or a singleplayer
//! save) and dimension, kept in config/native-waypoints.json in the mod's format. Also the maths the markers need:
//! where a point lands on the screen, and how a point from the Overworld maps into the Nether (1 : 8) and back.
use horizon_client::native_ui::theme;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

pub const OVERWORLD: &str = "overworld";
pub const NETHER: &str = "the_nether";
pub const END: &str = "the_end";
/// Marker colours offered on the page, in the order new waypoints take them.
pub const COLORS: [u32; 9] =
    [0xFF60A5FA, 0xFFF87171, 0xFF4ADE80, 0xFFFBBF24, 0xFFC084FC, 0xFF22D3EE, 0xFFFB923C, 0xFFF472B6, 0xFFFFFFFF];
/// Marker icons offered on the page; the last is the death point's.
pub const ICONS: [u32; 11] = [
    theme::I_MAP_PIN,
    theme::I_HOME,
    theme::I_STAR,
    theme::I_HEART,
    theme::I_SWORD,
    theme::I_SHIELD,
    theme::I_CROWN,
    theme::I_ZAP,
    theme::I_FLASK,
    theme::I_GLOBE,
    theme::I_X,
];
pub const DEATH_ICON: usize = ICONS.len() - 1;
pub const MAX_PER_WORLD: usize = 200;

/// One waypoint. Block coordinates; the marker sits on top of the block's centre.
#[derive(Clone, Debug, PartialEq)]
pub struct Point {
    /// Stable within a run (the page and the module refer to points by it).
    pub id: u64,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub dim: String,
    pub color: u32,
    pub icon: usize,
    pub visible: bool,
    pub death: bool,
    pub created: i64,
}

impl Point {
    pub fn new(name: &str, x: i32, y: i32, z: i32, dim: &str, color: u32, icon: usize) -> Self {
        Point {
            id: next_id(),
            name: name.to_owned(),
            x,
            y,
            z,
            dim: if dim.is_empty() { OVERWORLD.to_owned() } else { dim.to_owned() },
            color,
            icon: icon.min(ICONS.len() - 1),
            visible: true,
            death: false,
            created: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as i64),
        }
    }
}

fn next_id() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// Every world's waypoints, the most recently used world last.
#[derive(Default)]
pub struct Store {
    worlds: Vec<(String, Vec<Point>)>,
    file: Option<PathBuf>,
    loaded: bool,
    dirty: Option<std::time::Instant>,
    /// Bumped on every change, so views can tell their copy is stale.
    pub revision: u64,
}

/// Where the player is, as the module saw it last frame (for the page).
#[derive(Clone, Debug, Default)]
pub struct Here {
    pub world: Option<String>,
    pub label: Option<String>,
    pub dim: String,
    pub pos: [f64; 3],
}

pub fn store() -> MutexGuard<'static, Store> {
    static STORE: OnceLock<Mutex<Store>> = OnceLock::new();
    STORE.get_or_init(Default::default).lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn here() -> MutexGuard<'static, Here> {
    static HERE: OnceLock<Mutex<Here>> = OnceLock::new();
    HERE.get_or_init(Default::default).lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn key(world: &str) -> &str {
    if world.is_empty() { "unknown" } else { world }
}

impl Store {
    /// Keeps them in `game_dir`/config/native-waypoints.json (None: in memory only).
    pub fn init(&mut self, game_dir: Option<&std::path::Path>) {
        self.file = game_dir.map(|d| d.join("config").join("native-waypoints.json"));
        self.worlds.clear();
        self.loaded = false;
        self.load();
    }

    fn load(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        if let Some(text) = self.file.as_ref().and_then(|f| std::fs::read_to_string(f).ok()) {
            self.from_json(&text);
        }
    }

    /// The waypoints of a world, all dimensions, oldest first.
    pub fn of(&mut self, world: &str) -> Vec<Point> {
        self.load();
        let k = key(world);
        self.worlds.iter().find(|(w, _)| w == k).map(|(_, l)| l.clone()).unwrap_or_default()
    }

    /// Every world that has waypoints, the most recently used first.
    pub fn worlds(&mut self) -> Vec<String> {
        self.load();
        self.worlds.iter().rev().map(|(w, _)| w.clone()).collect()
    }

    /// False when the world is full.
    pub fn add(&mut self, world: &str, p: Point) -> bool {
        self.load();
        let k = key(world).to_owned();
        let i = self.worlds.iter().position(|(w, _)| *w == k);
        let mut list = i.map(|i| self.worlds.remove(i).1).unwrap_or_default();
        let ok = list.len() < MAX_PER_WORLD;
        if ok {
            list.push(p);
        }
        self.worlds.push((k, list)); // most recently used last
        if ok {
            self.changed();
        }
        ok
    }

    pub fn remove(&mut self, world: &str, id: u64) {
        self.load();
        let k = key(world);
        if let Some(i) = self.worlds.iter().position(|(w, _)| w == k) {
            let before = self.worlds[i].1.len();
            self.worlds[i].1.retain(|p| p.id != id);
            if self.worlds[i].1.len() != before {
                if self.worlds[i].1.is_empty() {
                    self.worlds.remove(i);
                }
                self.changed();
            }
        }
    }

    /// Changes one point in place; false when it is gone.
    pub fn edit(&mut self, world: &str, id: u64, f: impl FnOnce(&mut Point)) -> bool {
        self.load();
        let k = key(world);
        let Some(p) = self.worlds.iter_mut().find(|(w, _)| w == k).and_then(|(_, l)| l.iter_mut().find(|p| p.id == id)) else {
            return false;
        };
        f(p);
        self.changed();
        true
    }

    fn changed(&mut self) {
        self.revision += 1;
        self.dirty = Some(std::time::Instant::now());
    }

    /// Writes pending changes a moment after the last one (called every frame).
    pub fn tick(&mut self) {
        let Some(at) = self.dirty else { return };
        if at.elapsed() < std::time::Duration::from_millis(800) {
            return;
        }
        self.dirty = None;
        let Some(file) = self.file.clone() else { return };
        let text = self.to_json();
        let tmp = file.with_extension("json.tmp");
        let result = file
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|_| std::fs::write(&tmp, text))
            .and_then(|_| std::fs::rename(&tmp, &file));
        if let Err(e) = result {
            log::warn!("Native: waypoints could not be saved: {e}");
        }
    }

    /// A name not used yet in this world: "Waypoint 3".
    pub fn next_name(&mut self, world: &str, base: &str) -> String {
        let list = self.of(world);
        (1..)
            .map(|n| format!("{base} {n}"))
            .find(|name| !list.iter().any(|p| p.name.eq_ignore_ascii_case(name)))
            .unwrap_or_else(|| base.to_owned())
    }

    /// The colour a new waypoint takes: the first one the world does not use yet.
    pub fn next_color(&mut self, world: &str) -> u32 {
        let list = self.of(world);
        COLORS
            .iter()
            .copied()
            .find(|c| !list.iter().any(|p| p.color == *c))
            .unwrap_or(COLORS[list.len() % COLORS.len()])
    }

    pub fn to_json(&self) -> String {
        let mut worlds = serde_json::Map::new();
        for (w, list) in &self.worlds {
            let arr: Vec<Value> = list
                .iter()
                .map(|p| {
                    let mut o = json!({
                        "name": p.name, "x": p.x, "y": p.y, "z": p.z, "dim": p.dim,
                        "color": format!("#{:06X}", p.color & 0xFFFFFF), "icon": p.icon,
                        "visible": p.visible, "created": p.created,
                    });
                    if p.death {
                        o["death"] = json!(true);
                    }
                    o
                })
                .collect();
            worlds.insert(w.clone(), Value::Array(arr));
        }
        serde_json::to_string_pretty(&json!({ "version": 1, "worlds": worlds })).unwrap_or_default()
    }

    pub fn from_json(&mut self, text: &str) {
        self.worlds.clear();
        let Ok(root) = serde_json::from_str::<Value>(text) else { return };
        let Some(ws) = root["worlds"].as_object() else { return };
        for (w, arr) in ws {
            let list: Vec<Point> = arr
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|o| {
                    let int = |k: &str| o[k].as_i64().map(|v| v as i32);
                    let color = o["color"]
                        .as_str()
                        .and_then(|c| u32::from_str_radix(c.trim_start_matches('#'), 16).ok())
                        .unwrap_or(0x60A5FA);
                    let mut p = Point::new(
                        o["name"].as_str().unwrap_or("Waypoint"),
                        int("x")?,
                        int("y")?,
                        int("z")?,
                        o["dim"].as_str().unwrap_or(OVERWORLD),
                        0xFF000000 | color,
                        o["icon"].as_u64().unwrap_or(0) as usize,
                    );
                    p.visible = o["visible"].as_bool().unwrap_or(true);
                    p.death = o["death"].as_bool().unwrap_or(false);
                    if let Some(t) = o["created"].as_i64() {
                        p.created = t;
                    }
                    Some(p)
                })
                .collect();
            if !list.is_empty() {
                self.worlds.push((w.clone(), list));
            }
        }
    }
}

/// Where a point of `p.dim` is in `here` (x, y, z), or None when it cannot be shown there: the Overworld and the
/// Nether map onto each other (1 : 8) when `cross_nether` is on.
pub fn place_in(p: &Point, here: &str, cross_nether: bool) -> Option<[f64; 3]> {
    if p.dim == here {
        return Some([f64::from(p.x) + 0.5, f64::from(p.y), f64::from(p.z) + 0.5]);
    }
    if !cross_nether {
        return None;
    }
    if p.dim == OVERWORLD && here == NETHER {
        return Some([(f64::from(p.x) / 8.0).floor() + 0.5, f64::from(p.y), (f64::from(p.z) / 8.0).floor() + 0.5]);
    }
    if p.dim == NETHER && here == OVERWORLD {
        return Some([f64::from(p.x) * 8.0 + 4.0, f64::from(p.y), f64::from(p.z) * 8.0 + 4.0]);
    }
    None
}

/// A point in camera space: how far along the view, to the right and up.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CamPoint {
    pub depth: f64,
    pub right: f64,
    pub up: f64,
}

/// The camera a frame was drawn with: position, the game's yaw / pitch (degrees; yaw 0 faces +Z, pitch > 0 looks
/// down) and the vertical field of view in degrees.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Camera {
    pub pos: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub fov: f32,
}

impl Camera {
    pub fn to_cam(&self, p: [f64; 3]) -> CamPoint {
        let (sy, cy) = f64::from(self.yaw).to_radians().sin_cos();
        let (sp, cp) = f64::from(self.pitch).to_radians().sin_cos();
        let f = [-sy * cp, -sp, cy * cp];
        let (rx, rz) = (-cy, -sy);
        let u = [-rz * f[1], rz * f[0] - rx * f[2], rx * f[1]]; // right x forward
        let d = [p[0] - self.pos[0], p[1] - self.pos[1], p[2] - self.pos[2]];
        CamPoint {
            depth: d[0] * f[0] + d[1] * f[1] + d[2] * f[2],
            right: d[0] * rx + d[2] * rz,
            up: d[0] * u[0] + d[1] * u[1] + d[2] * u[2],
        }
    }

    /// Screen position of a camera-space point in front of the camera (depth > 0), on a w x h screen.
    pub fn screen(&self, q: CamPoint, w: f32, h: f32) -> [f32; 2] {
        let t = (f64::from(self.fov.clamp(1.0, 179.0)).to_radians() / 2.0).tan();
        let aspect = if h <= 0.0 { 1.0 } else { f64::from(w) / f64::from(h) };
        [
            (f64::from(w) / 2.0 + q.right / q.depth / (t * aspect) * f64::from(w) / 2.0) as f32,
            (f64::from(h) / 2.0 - q.up / q.depth / t * f64::from(h) / 2.0) as f32,
        ]
    }

    /// Screen pixels one block covers at `depth`.
    pub fn px_per_block(&self, depth: f64, h: f32) -> f32 {
        let t = (f64::from(self.fov.clamp(1.0, 179.0)).to_radians() / 2.0).tan();
        (f64::from(h) / 2.0 / t / depth.max(0.01)) as f32
    }

    /// Where a world point lands on a w x h screen; None when it is behind the camera.
    pub fn project(&self, p: [f64; 3], w: f32, h: f32) -> Option<[f32; 2]> {
        let q = self.to_cam(p);
        (q.depth > 0.05).then(|| self.screen(q, w, h))
    }
}

/// The compass heading (0 = north, 90 = east) from (x, z) towards (tx, tz).
pub fn heading(x: f64, z: f64, tx: f64, tz: f64) -> f32 {
    let a = (tx - x).atan2(-(tz - z)).to_degrees();
    (a.rem_euclid(360.0)) as f32
}

/// Coordinates in a line of text ("120 64 -300", "x: 120, y: 64, z: -300", "120, -300"), or None. Two numbers are
/// x and z (y is then None: keep your own height).
pub fn parse_coords(text: &str) -> Option<(i32, Option<i32>, i32)> {
    let s = text.trim();
    if s.is_empty() || s.len() > 200 {
        return None;
    }
    let lower = s.to_ascii_lowercase();
    // labelled: x: 1 y = 2 z 3
    let mut labelled: [Option<i32>; 3] = [None; 3];
    let bytes = lower.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        let axis = match b {
            b'x' => 0,
            b'y' => 1,
            b'z' => 2,
            _ => continue,
        };
        if i > 0 && bytes[i - 1].is_ascii_alphanumeric() {
            continue;
        }
        let rest = lower[i + 1..].trim_start();
        let Some(rest) = rest.strip_prefix(':').or_else(|| rest.strip_prefix('=')) else { continue };
        if let Some(v) = numbers(rest).first() {
            labelled[axis] = Some(*v);
        }
    }
    if let (Some(x), Some(z)) = (labelled[0], labelled[2]) {
        return Some((x, labelled[1], z));
    }
    let nums = numbers(&lower);
    match nums.as_slice() {
        [x, y, z] => Some((*x, Some(*y), *z)),
        [x, z] => Some((*x, None, *z)),
        _ => None,
    }
}

/// The whole numbers of a text (decimals floored), up to four; a number glued to letters is not one.
fn numbers(s: &str) -> Vec<i32> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() && out.len() < 4 {
        let start = i;
        let sign = b[i] == b'-' || b[i] == b'+';
        let digits_at = if sign { i + 1 } else { i };
        if digits_at < b.len() && b[digits_at].is_ascii_digit() && (start == 0 || !(b[start - 1].is_ascii_alphanumeric() || b[start - 1] == b'.')) {
            let mut j = digits_at;
            while j < b.len() && (b[j].is_ascii_digit() || b[j] == b'.') {
                j += 1;
            }
            if j < b.len() && b[j].is_ascii_alphabetic() {
                i = j;
                continue;
            }
            if let Ok(v) = s[start..j].parse::<f64>() {
                if v.abs() < 1e8 {
                    out.push(v.floor() as i32);
                }
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

/// "Base 120 64 -300", what Copy puts on the clipboard.
pub fn describe(p: &Point) -> String {
    format!("{} {} {} {}", p.name, p.x, p.y, p.z)
}

/// "1.2 km" / "243 m".
pub fn distance(blocks: f64) -> String {
    if blocks >= 10000.0 {
        format!("{:.0} km", blocks / 1000.0)
    } else if blocks >= 1000.0 {
        format!("{:.1} km", blocks / 1000.0)
    } else {
        format!("{} m", blocks.round() as i64)
    }
}

/// "Overworld", "Nether", "End" or the id's own path.
pub fn dimension_name(dim: &str) -> String {
    match dim {
        "" | OVERWORLD => "Overworld".into(),
        NETHER => "Nether".into(),
        END => "End".into(),
        d => {
            let d = d.replace('_', " ");
            let mut c = d.chars();
            c.next().map_or(d.clone(), |f| f.to_uppercase().collect::<String>() + c.as_str())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam(yaw: f32, pitch: f32, fov: f32) -> Camera {
        Camera { pos: [0.0, 64.0, 0.0], yaw, pitch, fov }
    }

    #[test]
    fn a_point_straight_ahead_is_in_the_middle() {
        let s = cam(0.0, 0.0, 70.0).project([0.0, 64.0, 10.0], 800.0, 600.0).unwrap();
        assert!((s[0] - 400.0).abs() < 1e-3 && (s[1] - 300.0).abs() < 1e-3);
    }

    #[test]
    fn sides_and_height_land_where_they_should() {
        assert!(cam(0.0, 0.0, 70.0).project([5.0, 64.0, 10.0], 800.0, 600.0).unwrap()[0] < 400.0, "east is left facing south");
        assert!(cam(0.0, 0.0, 70.0).project([0.0, 70.0, 10.0], 800.0, 600.0).unwrap()[1] < 300.0);
        assert!(cam(180.0, 0.0, 70.0).project([5.0, 64.0, -10.0], 800.0, 600.0).unwrap()[0] > 400.0);
        let down = cam(0.0, 90.0, 70.0).project([0.0, 54.0, 0.0], 800.0, 600.0).unwrap();
        assert!((down[0] - 400.0).abs() < 1e-2 && (down[1] - 300.0).abs() < 1e-2);
        // vertical fov 90: 45 degrees up is the top edge
        let top = Camera { pos: [0.0; 3], yaw: 0.0, pitch: 0.0, fov: 90.0 }.project([0.0, 10.0, 10.0], 800.0, 600.0).unwrap();
        assert!(top[1].abs() < 1e-3);
        let behind = cam(0.0, 0.0, 70.0).to_cam([-3.0, 64.0, -10.0]);
        assert!(behind.depth < 0.0 && behind.right > 0.0);
        assert!(cam(0.0, 0.0, 70.0).project([0.0, 64.0, -10.0], 800.0, 600.0).is_none());
    }

    #[test]
    fn nether_and_overworld_map_onto_each_other() {
        let home = Point::new("Home", 800, 70, -1601, OVERWORLD, 0, 0);
        assert_eq!(place_in(&home, NETHER, true), Some([100.5, 70.0, -200.5]));
        assert_eq!(place_in(&home, NETHER, false), None);
        assert_eq!(place_in(&home, END, true), None);
        let portal = Point::new("Portal", 10, 40, -3, NETHER, 0, 0);
        assert_eq!(place_in(&portal, OVERWORLD, true), Some([84.0, 40.0, -20.0]));
    }

    #[test]
    fn headings_follow_the_compass() {
        assert!((heading(0.0, 0.0, 0.0, -10.0) - 0.0).abs() < 1e-3);
        assert!((heading(0.0, 0.0, 10.0, 0.0) - 90.0).abs() < 1e-3);
        assert!((heading(0.0, 0.0, 0.0, 10.0) - 180.0).abs() < 1e-3);
        assert!((heading(0.0, 0.0, -10.0, 0.0) - 270.0).abs() < 1e-3);
    }

    #[test]
    fn coordinates_are_read_from_chat_like_text() {
        assert_eq!(parse_coords("120 64 -300"), Some((120, Some(64), -300)));
        assert_eq!(parse_coords("Base 120, 64, -300"), Some((120, Some(64), -300)));
        assert_eq!(parse_coords("x: -5.7 y: 70 z: 12.2"), Some((-6, Some(70), 12)));
        assert_eq!(parse_coords("120 -300"), Some((120, None, -300)));
        assert_eq!(parse_coords("X=1 Z=2"), Some((1, None, 2)));
        assert_eq!(parse_coords("hello there"), None);
        assert_eq!(parse_coords("1 2 3 4 5"), None);
    }

    #[test]
    fn waypoints_are_kept_per_world_and_survive_saving() {
        let mut s = Store::default();
        s.init(None);
        assert!(s.add("server:play.example.net", Point::new("Base", 1, 2, 3, OVERWORLD, 0xFF60A5FA, 1)));
        let mut d = Point::new("Death", -4, 5, 6, NETHER, 0xFFF87171, DEATH_ICON);
        d.death = true;
        d.visible = false;
        s.add("world:New World", d);
        assert_eq!(s.next_name("world:New World", "Waypoint"), "Waypoint 1");
        assert_eq!(s.next_color("world:New World"), 0xFF60A5FA);
        assert_eq!(s.next_color("server:play.example.net"), COLORS[1]);
        let text = s.to_json();
        let mut back = Store::default();
        back.init(None);
        back.from_json(&text);
        let l = back.of("world:New World");
        assert_eq!(l.len(), 1);
        assert!(l[0].death && !l[0].visible && l[0].dim == NETHER && l[0].color == 0xFFF87171);
        assert_eq!(back.of("server:play.example.net")[0].icon, 1);
        assert!(back.of("server:other").is_empty());
        let id = l[0].id;
        back.remove("world:New World", id);
        assert_eq!(back.worlds(), vec!["server:play.example.net".to_owned()]);
        let base = back.of("server:play.example.net")[0].id;
        assert!(back.edit("server:play.example.net", base, |p| p.name = "Home".into()));
        assert_eq!(back.of("server:play.example.net")[0].name, "Home");
    }

    #[test]
    fn the_mods_file_reads_the_same() {
        let mut s = Store::default();
        s.from_json(r##"{"version":1,"worlds":{"world:abc":[{"name":"Base","x":1,"y":2,"z":3,"dim":"overworld","color":"#4ADE80","icon":2,"visible":true,"created":5}]}}"##);
        let p = &s.of("world:abc")[0];
        assert_eq!((p.x, p.y, p.z, p.color, p.icon, p.created), (1, 2, 3, 0xFF4ADE80, 2, 5));
    }

    #[test]
    fn distances_read_nicely() {
        assert_eq!(distance(243.4), "243 m");
        assert_eq!(distance(1234.0), "1.2 km");
        assert_eq!(distance(12345.0), "12 km");
    }
}
