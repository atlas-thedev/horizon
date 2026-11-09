//! `ExtraModules`: Nether coordinates, moon phase, day / night cycle, totem
//! and item counters, playtime, reach display, combo counter and item
//! cooldowns, with the Java mod's ids, settings and looks.

use super::module::*;
use super::setting::{Kind, Setting};
use horizon_client::native_ui::canvas::clock_ms;
use horizon_client::native_ui::{Canvas, Image, font, icons, theme};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

pub fn all() -> Vec<Module> {
    vec![nether_coords(), moon_phase(), day_night(), totem_counter(), item_counter(), playtime(), reach(), combo(), cooldowns()]
}

fn with_hud(mut m: Module, f: impl FnOnce(&mut Hud)) -> Module {
    if let Some(h) = m.state.hud.as_mut() {
        f(h);
    }
    m
}

fn scale_of(m: &ModuleState) -> f32 {
    m.hud.as_ref().map_or(1.0, Hud::scale)
}

fn pad_of(m: &ModuleState) -> f32 {
    let s = scale_of(m);
    match m.hud.as_ref().map_or(STYLE_CARD, Hud::style) {
        STYLE_CARD => 5.0 * s,
        STYLE_BRACKETS => 4.0 * s,
        _ => s,
    }
}

fn two(v: i64) -> String {
    format!("{v:02}")
}

#[allow(clippy::too_many_arguments)]
fn text(m: &ModuleState, c: &mut Canvas, face: usize, size: f32, t: &str, x: f32, y: f32, argb: u32) {
    if let Some(h) = m.hud.as_ref() {
        h.text_face(c, face, size, t, x, y, argb);
    }
}

/// A frame (card or brackets) around a module that lays itself out.
trait Panel: Send {
    fn render(&mut self, m: &ModuleState, c: &mut Canvas, g: &GameView, x: f32, y: f32, draw: bool) -> [f32; 2];
    fn visible(&self, _m: &ModuleState, _g: &GameView) -> bool {
        true
    }
    fn needs_vitals(&self) -> bool {
        false
    }
}

struct Framed<T: Panel>(T);

impl<T: Panel> Behaviour for Framed<T> {
    fn measure(&mut self, m: &ModuleState, c: &mut Canvas, g: &GameView) -> Option<[f32; 2]> {
        let size = self.0.render(m, c, g, 0.0, 0.0, false);
        Some([size[0].max(1.0), size[1].max(1.0)])
    }
    fn paint(&mut self, m: &ModuleState, c: &mut Canvas, x: f32, y: f32, g: &GameView) -> bool {
        let sz = self.0.render(m, c, g, 0.0, 0.0, false);
        let (w, h) = (sz[0].max(1.0), sz[1].max(1.0));
        if let Some(hud) = m.hud.as_ref() {
            let s = hud.scale();
            if hud.style() == STYLE_CARD {
                hud.background(c, x, y, w, h, s);
            } else if hud.style() == STYLE_BRACKETS {
                hud.brackets(c, x, y, w, h, s);
            }
        }
        self.0.render(m, c, g, x, y, true);
        true
    }
    fn visible(&self, m: &ModuleState, g: &GameView) -> bool {
        self.0.visible(m, g)
    }
    fn needs_vitals(&self) -> bool {
        self.0.needs_vitals()
    }
}

/// The world's time of day in ticks (a whole day in 24 s for previews so the picture moves); -1 when unknown.
fn day_time(g: &GameView) -> i64 {
    if g.sample {
        (clock_ms() as i64) % 24000
    } else if g.in_world {
        g.day_time
    } else {
        -1
    }
}

fn dimension(g: &GameView) -> &str {
    if g.sample || g.dimension.is_empty() {
        "overworld"
    } else {
        g.dimension.trim_start_matches("minecraft:")
    }
}

// ── Nether coordinates ───────────────────────────────────────────────────

fn nether_coords() -> Module {
    Module::hud(
        "nether-coords",
        "Nether Coordinates",
        "Where a Nether portal here would lead: your position converted between the Overworld and the Nether (1 : 8).",
        theme::I_COMPASS,
        false,
        0.0,
        0.22,
        vec![Setting::bool("compact", "One line", false)],
        Box::new(NetherCoords),
    )
}

struct NetherCoords;

impl Behaviour for NetherCoords {
    fn visible(&self, _m: &ModuleState, g: &GameView) -> bool {
        g.sample || dimension(g) != "the_end"
    }
    fn lines(&mut self, m: &ModuleState, g: &GameView) -> Vec<Line> {
        let nether = !g.sample && dimension(g) == "the_nether";
        let f = if nether { 8.0 } else { 1.0 / 8.0 };
        let (x, z) = ((g.x * f).floor() as i64, (g.z * f).floor() as i64);
        let where_ = if nether { "Overworld" } else { "Nether" };
        if m.bool("compact") {
            return vec![Line::new(where_, format!("{x} {z}"))];
        }
        vec![Line::new(format!("{where_} X"), x.to_string()), Line::new(format!("{where_} Z"), z.to_string())]
    }
}

// ── moon phase ───────────────────────────────────────────────────────────

const PHASES: [&str; 8] = [
    "Full Moon", "Waning Gibbous", "Last Quarter", "Waning Crescent", "New Moon", "Waxing Crescent", "First Quarter", "Waxing Gibbous",
];

/// The moon drawn for one phase (0 = full ... 4 = new), lit like a sphere; cached per size.
fn moon(phase: usize, px: u32) -> Arc<Image> {
    static CACHE: OnceLock<Mutex<HashMap<(usize, u32), Arc<Image>>>> = OnceLock::new();
    let mut cache = CACHE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if let Some(img) = cache.get(&(phase, px)) {
        return img.clone();
    }
    if cache.len() > 96 {
        cache.clear();
    }
    let img = render_moon(phase, px);
    cache.insert((phase, px), img.clone());
    img
}

fn render_moon(phase: usize, px: u32) -> Arc<Image> {
    let n = px as usize;
    let mut argb = vec![0u32; n * n];
    let alpha = phase as f64 * std::f64::consts::PI / 4.0;
    // Light direction: from the right while waxing, from the left while waning.
    let (lx, lz) = (-alpha.sin(), alpha.cos());
    let r = px as f64 / 2.0;
    for j in 0..n {
        for i in 0..n {
            let nx = (i as f64 + 0.5 - r) / r;
            let ny = (j as f64 + 0.5 - r) / r;
            let d2 = nx * nx + ny * ny;
            let edge = ((1.0 - d2.sqrt()) * r + 0.5).clamp(0.0, 1.0);
            if edge <= 0.0 {
                continue;
            }
            let nz = (1.0 - d2).max(0.0).sqrt();
            let lit = nx * lx + nz * lz; // > 0 faces the light
            let k = (lit * 6.0 + 0.5).clamp(0.0, 1.0); // A soft terminator.
            let shade = (f64::from(0x2Au8) + f64::from(0xECu8 - 0x2A) * k).round();
            let (rr, gg) = (shade as u32, (shade * 0.985).round() as u32);
            let bb = ((shade * 1.02 + (1.0 - k) * 12.0).round() as u32).min(255);
            argb[j * n + i] = (((edge * 255.0).round() as u32) << 24) | (rr << 16) | (gg << 8) | bb;
        }
    }
    Image::new(px, px, argb)
}

fn moon_phase() -> Module {
    with_hud(
        Module::hud(
            "moon-phase",
            "Moon Phase",
            "The phase of the moon tonight, how many nights until the next full moon (more slimes in swamps, brighter nights) and the day count.",
            icons::MOON,
            false,
            0.0,
            0.3,
            vec![
                Setting::bool("name", "Show phase name", true),
                Setting::bool("next", "Show next full moon", true),
                Setting::bool("day", "Show day count", true),
            ],
            Box::new(Framed(MoonPhase)),
        ),
        |h| {
            h.drop_setting(LABELS);
            h.drop_setting(COLOR);
        },
    )
}

struct MoonPhase;

impl Panel for MoonPhase {
    fn render(&mut self, m: &ModuleState, c: &mut Canvas, g: &GameView, x: f32, y: f32, draw: bool) -> [f32; 2] {
        let s = scale_of(m);
        let pad = pad_of(m);
        let t = if g.sample { ((clock_ms() as i64 / 2200) % 8) * 24000 + 18000 } else { day_time(g) };
        let days = if t < 0 { 0 } else { t / 24000 };
        let phase = (days % 8) as usize;
        let icon = 30.0 * s;
        let title = if t < 0 { "Unknown" } else { PHASES[phase] };
        let sub = (t >= 0 && m.bool("next")).then(|| {
            let to = (8 - phase as i64) % 8;
            if to == 0 {
                "Full moon tonight".to_owned()
            } else {
                format!("Full moon in {to} {}", if to == 1 { "night" } else { "nights" })
            }
        });
        let day_text = (t >= 0 && m.bool("day")).then(|| format!("Day {}", days + 1));
        let (fs, fs2) = (12.0 * s, 10.5 * s);
        let show_name = m.bool("name");
        let mut tw = if show_name { c.text_width(font::SEMIBOLD, fs, title) } else { 0.0 };
        if let Some(sub) = &sub {
            tw = tw.max(c.text_width(font::REGULAR, fs2, sub));
        }
        if let Some(d) = &day_text {
            tw = tw.max(c.text_width(font::REGULAR, fs2, d));
        }
        let (lh, lh2) = (c.line_height(font::SEMIBOLD, fs), c.line_height(font::REGULAR, fs2));
        let rows = usize::from(show_name) + usize::from(sub.is_some()) + usize::from(day_text.is_some());
        let text_h = if show_name { lh } else { 0.0 }
            + (usize::from(sub.is_some()) + usize::from(day_text.is_some())) as f32 * lh2
            + rows.saturating_sub(1) as f32 * 2.0 * s;
        let gap = if rows > 0 { 9.0 * s } else { 0.0 };
        let (w, h) = (pad * 2.0 + icon + gap + tw, pad * 2.0 + icon.max(text_h));
        if draw {
            let px = (c.px(icon)).max(8) as u32;
            let img = moon(if t < 0 { 4 } else { phase }, px);
            c.stamp(0x4D4F_4F4E_0000_0000 | (u64::from(px) << 8) | (if t < 0 { 4 } else { phase }) as u64, &img, x + pad, y + (h - icon) / 2.0, icon, icon, false, 0xFFFFFFFF);
            let tx = x + pad + icon + gap;
            let mut ty = y + (h - text_h) / 2.0;
            if show_name {
                text(m, c, font::SEMIBOLD, fs, title, tx, ty, 0xFFFFFFFF);
                ty += lh + 2.0 * s;
            }
            if let Some(sub) = &sub {
                text(m, c, font::REGULAR, fs2, sub, tx, ty, 0xFFB4B8C4);
                ty += lh2 + 2.0 * s;
            }
            if let Some(d) = &day_text {
                text(m, c, font::REGULAR, fs2, d, tx, ty, 0xFF8C90A0);
            }
        }
        [w, h]
    }
}

// ── day / night cycle ────────────────────────────────────────────────────

fn day_night() -> Module {
    with_hud(
        Module::hud(
            "day-night",
            "Day / Night Cycle",
            "Shows where the sun is: the in-game clock, day or night, and how long until it changes (monsters spawn from dusk).",
            theme::I_SUN,
            false,
            0.0,
            0.38,
            vec![
                Setting::bool("clock", "Show the in-game clock", true),
                Setting::bool("countdown", "Show time until the next change", true),
                Setting::bool("day", "Show day count", false),
            ],
            Box::new(Framed(DayNight)),
        ),
        |h| {
            h.drop_setting(LABELS);
            h.drop_setting(COLOR);
        },
    )
}

struct DayNight;

/// 0 day, 1 dusk, 2 night, 3 dawn.
fn phase_of(t: i64) -> usize {
    if t < 12000 {
        0
    } else if t < 13000 {
        1
    } else if t < 23000 {
        2
    } else {
        3
    }
}

impl Panel for DayNight {
    fn render(&mut self, m: &ModuleState, c: &mut Canvas, g: &GameView, x: f32, y: f32, draw: bool) -> [f32; 2] {
        const NAMES: [&str; 4] = ["Day", "Dusk", "Night", "Dawn"];
        const NEXT: [&str; 4] = ["Dusk in ", "Night in ", "Dawn in ", "Day in "];
        const ENDS: [i64; 4] = [12000, 13000, 23000, 24000];
        let s = scale_of(m);
        let pad = pad_of(m);
        let raw = day_time(g);
        let known = raw >= 0;
        let t = if known { raw % 24000 } else { 6000 };
        let ph = phase_of(t);
        let (bar_w, bar_h) = (124.0 * s, 6.0 * s);
        let (fs, fs2) = (12.0 * s, 10.5 * s);
        let (hours, minutes) = ((t / 1000 + 6) % 24, (t % 1000) * 60 / 1000);
        let head = format!(
            "{}{}",
            NAMES[ph],
            if m.bool("clock") && known { format!("  {}:{}", two(hours), two(minutes)) } else { String::new() }
        );
        let day_text = (m.bool("day") && known).then(|| format!("Day {}", raw / 24000 + 1));
        let secs = (ENDS[ph] - t) / 20;
        let sub = (m.bool("countdown") && known).then(|| format!("{}{}:{}", NEXT[ph], secs / 60, two(secs % 60)));
        let (lh, lh2) = (c.line_height(font::SEMIBOLD, fs), c.line_height(font::REGULAR, fs2));
        let head_w = c.text_width(font::SEMIBOLD, fs, &head)
            + day_text.as_ref().map_or(0.0, |d| c.text_width(font::REGULAR, fs2, d) + 10.0 * s);
        let sub_w = sub.as_ref().map_or(0.0, |t| c.text_width(font::REGULAR, fs2, t));
        let w = pad * 2.0 + bar_w.max(head_w.max(sub_w));
        let h = pad * 2.0 + lh + 5.0 * s + bar_h + 12.0 * s + if sub.is_some() { lh2 + 3.0 * s } else { 0.0 };
        if draw {
            let (ix, iy) = (x + pad, y + pad);
            text(m, c, font::SEMIBOLD, fs, &head, ix, iy, 0xFFFFFFFF);
            if let Some(d) = &day_text {
                let dw = c.text_width(font::REGULAR, fs2, d);
                text(m, c, font::REGULAR, fs2, d, x + w - pad - dw, iy + (lh - lh2) / 2.0, 0xFF9AA0B2);
            }
            let by = iy + lh + 7.0 * s;
            let bw = w - pad * 2.0;
            // The sky over a whole day: day, dusk, night, dawn (the day starts at sunrise).
            let seg = bw / 24000.0;
            c.round(ix, by, bw, bar_h, bar_h / 2.0, 0xFF1B2347);
            c.push_clip(ix, by, bw, bar_h);
            c.fill(ix, by, 12000.0 * seg, bar_h, 0xFF5AA9F0);
            c.gradient_h(ix + 12000.0 * seg, by, 1000.0 * seg, bar_h, 0xFF5AA9F0, 0xFFF2994A);
            c.fill(ix + 13000.0 * seg, by, 10000.0 * seg, bar_h, 0xFF1B2347);
            c.gradient_h(ix + 23000.0 * seg, by, 1000.0 * seg, bar_h, 0xFF1B2347, 0xFFF2994A);
            c.pop_clip();
            if known || g.sample {
                let (mx, my) = (ix + t as f32 * seg, by + bar_h / 2.0);
                let night = ph == 2;
                c.circle(mx, my, 6.0 * s, 0xAA000000);
                c.circle(mx, my, 4.6 * s, if night { 0xFFE6EAF5 } else { 0xFFFFD25A });
                if night {
                    c.circle(mx + 1.6 * s, my - 1.2 * s, 3.6 * s, 0xFF1B2347);
                }
            }
            if let Some(sub) = &sub {
                text(m, c, font::REGULAR, fs2, sub, ix, by + bar_h + 7.0 * s, 0xFFB4B8C4);
            }
        }
        [w, h]
    }
}

// ── counters ─────────────────────────────────────────────────────────────

/// How many of an item you carry (backpack, hotbar and off hand).
fn count(g: &GameView, id: &str) -> u32 {
    let v = &g.vitals;
    v.inv.iter().chain(std::iter::once(&v.offhand)).flatten().filter(|s| s.id == id).map(|s| s.count).sum()
}

/// "ender_pearl" -> "Ender Pearl".
fn pretty(id: &str) -> String {
    id.split('_')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut cs = p.chars();
            cs.next().map_or_else(String::new, |f| f.to_uppercase().collect::<String>() + cs.as_str())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

const ITEMS: [(&str, &str); 22] = [
    ("Ender Pearl", "ender_pearl"),
    ("Golden Apple", "golden_apple"),
    ("Enchanted Golden Apple", "enchanted_golden_apple"),
    ("Arrow", "arrow"),
    ("End Crystal", "end_crystal"),
    ("Obsidian", "obsidian"),
    ("Respawn Anchor", "respawn_anchor"),
    ("Glowstone", "glowstone"),
    ("Totem of Undying", "totem_of_undying"),
    ("Experience Bottle", "experience_bottle"),
    ("Firework Rocket", "firework_rocket"),
    ("Cooked Beef", "cooked_beef"),
    ("Cooked Porkchop", "cooked_porkchop"),
    ("Snowball", "snowball"),
    ("Fire Charge", "fire_charge"),
    ("Shield", "shield"),
    ("Water Bucket", "water_bucket"),
    ("Cobweb", "cobweb"),
    ("Oak Planks", "oak_planks"),
    ("Cobblestone", "cobblestone"),
    ("Diamond", "diamond"),
    ("Gold Ingot", "gold_ingot"),
];

fn item_names() -> Vec<String> {
    ITEMS.iter().map(|i| i.0.to_owned()).collect()
}

/// What a counter counts.
trait Counted: Send {
    /// The item counted right now (bare id); `None` when there is nothing to show (no item held).
    fn item(&self, m: &ModuleState, g: &GameView) -> Option<String>;
    fn item_name(&self, id: &str) -> String;
    fn sample(&self) -> u32;
    fn sample_item(&self, m: &ModuleState) -> String;
}

struct Counter<T: Counted>(T);

fn counter_module<T: Counted + 'static>(id: &'static str, name: &'static str, desc: &'static str, icon: u32, y: f32, mut settings: Vec<Setting>, t: T) -> Module {
    let mut all = vec![Setting::bool("hide", "Hide when you have none", false), Setting::bool("name", "Show the item name", false)];
    all.append(&mut settings);
    with_hud(Module::hud(id, name, desc, icon, false, 0.0, y, all, Box::new(Framed(Counter(t)))), |h| {
        h.drop_setting(LABELS);
        h.drop_setting(COLOR);
    })
}

impl<T: Counted> Panel for Counter<T> {
    fn needs_vitals(&self) -> bool {
        true
    }

    fn visible(&self, m: &ModuleState, g: &GameView) -> bool {
        if g.sample {
            return true;
        }
        self.0.item(m, g).is_some_and(|id| !m.bool("hide") || count(g, &id) > 0)
    }

    fn render(&mut self, m: &ModuleState, c: &mut Canvas, g: &GameView, x: f32, y: f32, draw: bool) -> [f32; 2] {
        let s = scale_of(m);
        let pad = pad_of(m);
        let id = if g.sample { Some(self.0.sample_item(m)) } else { self.0.item(m, g) };
        let n = if g.sample { self.0.sample() } else { id.as_deref().map_or(0, |id| count(g, id)) };
        let (ic, fs) = (16.0 * s, 13.0 * s);
        let num = n.to_string();
        let label = if m.bool("name") { id.as_deref().map(|i| self.0.item_name(i)) } else { None };
        let nw = c.text_width(font::BOLD, fs, &num);
        let lw = label.as_ref().map_or(0.0, |l| c.text_width(font::REGULAR, 10.5 * s, l));
        let text_w = nw.max(lw);
        let lh = c.line_height(font::BOLD, fs);
        let lh2 = if label.is_some() { c.line_height(font::REGULAR, 10.5 * s) } else { 0.0 };
        let text_h = lh + if label.is_some() { lh2 + s } else { 0.0 };
        let (w, h) = (pad * 2.0 + ic + 6.0 * s + text_w, pad * 2.0 + ic.max(text_h));
        if draw && let Some(id) = &id {
            c.push_alpha(if n == 0 { 0.45 } else { 1.0 });
            c.host(HOST_ITEM_ID, item_key(id), x + pad, y + (h - ic) / 2.0, ic, 0xFFFFFFFF);
            c.pop_alpha();
            let tx = x + pad + ic + 6.0 * s;
            let ty = y + (h - text_h) / 2.0;
            text(m, c, font::BOLD, fs, &num, tx, ty, if n == 0 { 0xFFF87171 } else { 0xFFFFFFFF });
            if let Some(l) = &label {
                text(m, c, font::REGULAR, 10.5 * s, l, tx, ty + lh + s, 0xFF9AA0B2);
            }
        }
        [w, h]
    }
}

fn totem_counter() -> Module {
    counter_module(
        "totem-counter",
        "Totem Counter",
        "How many Totems of Undying you carry, so you know before the next fight.",
        theme::I_HEART_PULSE,
        0.46,
        vec![],
        Totems,
    )
}

struct Totems;
impl Counted for Totems {
    fn item(&self, _m: &ModuleState, _g: &GameView) -> Option<String> {
        Some("totem_of_undying".to_owned())
    }
    fn item_name(&self, _id: &str) -> String {
        "Totems".to_owned()
    }
    fn sample(&self) -> u32 {
        3
    }
    fn sample_item(&self, _m: &ModuleState) -> String {
        "totem_of_undying".to_owned()
    }
}

fn item_counter() -> Module {
    counter_module(
        "item-counter",
        "Item Counter",
        "Counts any item you pick (pearls, golden apples, arrows, crystals...), or whatever you are holding.",
        theme::I_BACKPACK,
        0.54,
        vec![Setting::device("item", "Item to count", item_names, "The item in my hand")],
        Items,
    )
}

struct Items;

impl Items {
    fn chosen(m: &ModuleState) -> Option<&'static str> {
        let value = m.setting("item").and_then(|s| match &s.kind {
            Kind::Device { value, .. } => Some(value.as_str()),
            _ => None,
        })?;
        ITEMS.iter().find(|i| i.0 == value).map(|i| i.1)
    }
}

impl Counted for Items {
    fn item(&self, m: &ModuleState, g: &GameView) -> Option<String> {
        Self::chosen(m).map(str::to_owned).or_else(|| g.vitals.hand.as_ref().map(|h| h.id.clone()))
    }
    fn item_name(&self, id: &str) -> String {
        ITEMS.iter().find(|i| i.1 == id).map_or_else(|| pretty(id), |i| i.0.to_owned())
    }
    fn sample(&self) -> u32 {
        16
    }
    fn sample_item(&self, m: &ModuleState) -> String {
        Self::chosen(m).unwrap_or("ender_pearl").to_owned()
    }
}

// ── combo counter ────────────────────────────────────────────────────────

/// The counting rules, apart from the game: a hit adds one (at most one per damage-immunity window),
/// damage taken or silence ends it.
#[derive(Default)]
struct ComboTracker {
    combo: u32,
    best: u32,
    last_hit_at: f64,
    last_count_at: f64,
}

impl ComboTracker {
    const IMMUNITY_MS: f64 = 450.0;

    /// An attack landed on something in reach.
    fn hit(&mut self, now: f64) {
        if self.combo > 0 && now - self.last_count_at < Self::IMMUNITY_MS {
            return; // Inside the target's damage immunity: this swing did no damage.
        }
        self.combo += 1;
        self.best = self.best.max(self.combo);
        self.last_count_at = now;
        self.last_hit_at = now;
    }

    fn damaged(&mut self) {
        self.combo = 0;
    }

    /// No hit for `timeout_ms`: the combo is over.
    fn tick(&mut self, now: f64, timeout_ms: f64) {
        if self.combo > 0 && now - self.last_hit_at > timeout_ms {
            self.combo = 0;
        }
    }
}

fn combo() -> Module {
    Module::hud(
        "combo",
        "Combo Counter",
        "Counts the hits you land in a row. It ends when you take damage or stop hitting for a few seconds.",
        theme::I_SWORD,
        false,
        0.0,
        0.46,
        vec![
            Setting::num("timeout", "Combo ends after", 3.0, 1.0, 10.0, 1.0, "s"),
            Setting::bool("damage", "End the combo when I take damage", true),
            Setting::bool("best", "Show my best combo", true),
        ],
        Box::new(Combo { tracker: ComboTracker::default(), last_health: -1.0, was_down: false }),
    )
}

struct Combo {
    tracker: ComboTracker,
    last_health: f32,
    was_down: bool,
}

impl Behaviour for Combo {
    fn needs_vitals(&self) -> bool {
        true
    }
    fn frame(&mut self, m: &ModuleState, g: &GameView) {
        let now = clock_ms();
        let hp = g.vitals.health + g.vitals.absorption;
        if m.bool("damage") && self.last_health >= 0.0 && hp < self.last_health - 0.01 {
            self.tracker.damaged();
        }
        self.last_health = hp;
        let down = g.down[ATTACK];
        if down && !self.was_down && g.reach.is_some_and(|d| d > 0.0 && d < 8.0) {
            self.tracker.hit(now);
        }
        self.was_down = down;
        self.tracker.tick(now, f64::from(m.num("timeout")) * 1000.0);
    }
    fn lines(&mut self, m: &ModuleState, g: &GameView) -> Vec<Line> {
        let (c, b) = if g.sample { (7, 12) } else { (self.tracker.combo, self.tracker.best) };
        let mut lines = vec![Line::new("Combo", c.to_string())];
        if m.bool("best") {
            lines.push(Line::new("Best", b.max(c).to_string()));
        }
        lines
    }
}

// ── cooldowns ────────────────────────────────────────────────────────────

fn cooldowns() -> Module {
    with_hud(
        Module::hud(
            "cooldowns",
            "Item Cooldowns",
            "A bar that drains for every item on cooldown: ender pearls, shields, chorus fruit and more.",
            theme::I_TIMER,
            false,
            0.0,
            0.6,
            vec![Setting::bool("names", "Show item names", false)],
            Box::new(Framed(Cooldowns)),
        ),
        |h| {
            h.drop_setting(LABELS);
            h.drop_setting(COLOR);
        },
    )
}

struct Cooldowns;

impl Panel for Cooldowns {
    fn visible(&self, _m: &ModuleState, g: &GameView) -> bool {
        g.sample || !g.cooldowns.is_empty()
    }

    fn render(&mut self, m: &ModuleState, c: &mut Canvas, g: &GameView, x: f32, y: f32, draw: bool) -> [f32; 2] {
        let s = scale_of(m);
        let pad = pad_of(m);
        let sample = [("ender_pearl".to_owned(), 0.6 + 0.15 * (clock_ms() / 600.0).sin() as f32), ("shield".to_owned(), 0.25 + 0.15 * (clock_ms() / 600.0).sin() as f32)];
        let rows: &[(String, f32)] = if g.sample { &sample } else { &g.cooldowns };
        let n = rows.len();
        let (ic, bar_w, bar_h) = (16.0 * s, 54.0 * s, 4.0 * s);
        let (row_h, gap) = (ic + 4.0 * s, 3.0 * s);
        let fs = 10.5 * s;
        let names = m.bool("names");
        let name_w = if names { rows.iter().map(|r| c.text_width(font::REGULAR, fs, &pretty(&r.0))).fold(0.0, f32::max) } else { 0.0 };
        let w = pad * 2.0 + ic + 6.0 * s + bar_w.max(name_w);
        let h = pad * 2.0 + n.max(1) as f32 * row_h + n.saturating_sub(1) as f32 * gap;
        if draw {
            for (i, (id, f)) in rows.iter().enumerate() {
                let ry = y + pad + i as f32 * (row_h + gap);
                c.host(HOST_ITEM_ID, item_key(id), x + pad, ry + (row_h - ic) / 2.0, ic, 0xFFFFFFFF);
                let bx = x + pad + ic + 6.0 * s;
                let by = ry + if names { row_h - bar_h - s } else { (row_h - bar_h) / 2.0 };
                if names {
                    text(m, c, font::REGULAR, fs, &pretty(id), bx, ry, 0xFFC4C8D4);
                }
                c.round(bx, by, bar_w, bar_h, bar_h / 2.0, 0x33FFFFFF);
                let f = f.clamp(0.0, 1.0);
                let color = if f > 0.66 { 0xFFF87171 } else if f > 0.33 { 0xFFFBBF24 } else { 0xFF4ADE80 };
                c.round(bx, by, (bar_w * f).max(bar_h), bar_h, bar_h / 2.0, color);
            }
        }
        [w, h]
    }
}

// ── playtime ─────────────────────────────────────────────────────────────

/// Milliseconds played: this session, and in total (kept in `config/native-playtime.json`).
struct Played {
    total: Option<f64>,
    session: f64,
    last: f64,
    saved_at: f64,
    dirty: bool,
}

fn played() -> &'static Mutex<Played> {
    static P: OnceLock<Mutex<Played>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(Played { total: None, session: 0.0, last: 0.0, saved_at: 0.0, dirty: false }))
}

fn playtime_file() -> Option<std::path::PathBuf> {
    super::game_dir().map(|d| d.join("config").join("native-playtime.json"))
}

/// Counts the time spent in a world, once a frame (so a freeze or a pause of the window never adds a jump).
pub fn tick(in_world: bool) {
    let now = clock_ms();
    let mut p = played().lock().unwrap_or_else(|e| e.into_inner());
    if p.total.is_none() {
        p.total = Some(
            playtime_file()
                .and_then(|f| std::fs::read_to_string(f).ok())
                .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
                .and_then(|v| v.get("totalMs").and_then(serde_json::Value::as_f64))
                .unwrap_or(0.0)
                .max(0.0),
        );
        p.saved_at = now;
    }
    if p.last != 0.0 && in_world {
        let dt = (now - p.last).min(1000.0);
        if dt > 0.0 {
            p.session += dt;
            p.total = p.total.map(|t| t + dt);
            p.dirty = true;
        }
    }
    p.last = now;
    if p.dirty && now - p.saved_at > 30_000.0 {
        p.dirty = false;
        p.saved_at = now;
        if let Some(f) = playtime_file() {
            if let Some(dir) = f.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let body = serde_json::json!({ "totalMs": p.total.unwrap_or(0.0) as i64 }).to_string();
            let _ = std::fs::write(f, body);
        }
    }
}

fn hms(ms: f64) -> String {
    let sec = (ms / 1000.0) as i64;
    let (h, m, s) = (sec / 3600, sec % 3600 / 60, sec % 60);
    if h > 0 { format!("{h}:{}:{}", two(m), two(s)) } else { format!("{m}:{}", two(s)) }
}

fn span(ms: f64) -> String {
    let min = (ms / 60000.0) as i64;
    let h = min / 60;
    if h >= 100 {
        format!("{h}h")
    } else if h > 0 {
        format!("{h}h {}m", two(min % 60))
    } else {
        format!("{min}m")
    }
}

fn playtime() -> Module {
    Module::hud(
        "playtime",
        "Playtime",
        "How long you have played: this session and your total time in Minecraft with Native.",
        theme::I_TIMER,
        false,
        0.0,
        0.3,
        vec![Setting::choice("mode", "Show", 2, &["Session", "Total", "Both"])],
        Box::new(Playtime),
    )
}

struct Playtime;

impl Behaviour for Playtime {
    fn lines(&mut self, m: &ModuleState, g: &GameView) -> Vec<Line> {
        let (ses, tot) = if g.sample {
            (5_025_000.0, 142_380_000.0)
        } else {
            let p = played().lock().unwrap_or_else(|e| e.into_inner());
            (p.session, p.total.unwrap_or(0.0).max(0.0))
        };
        let mode = m.choice("mode");
        let mut lines = Vec::new();
        if mode != 1 {
            lines.push(Line::new("Session", hms(ses)));
        }
        if mode != 0 {
            lines.push(Line::new("Total", span(tot)));
        }
        lines
    }
}

// ── reach display ────────────────────────────────────────────────────────

fn reach() -> Module {
    Module::hud(
        "reach",
        "Reach Display",
        "Shows how far away the player you just hit was (in blocks, from your eyes to where you hit).",
        theme::I_CROSSHAIR,
        false,
        0.0,
        0.38,
        vec![Setting::choice("decimals", "Decimals", 1, &["1", "2", "3"]), Setting::num("hold", "Keep the last hit for", 4.0, 1.0, 15.0, 1.0, "s")],
        Box::new(Reach { last: None, last_at: 0.0, was_down: false }),
    )
}

struct Reach {
    last: Option<f64>,
    last_at: f64,
    was_down: bool,
}

impl Behaviour for Reach {
    fn frame(&mut self, _m: &ModuleState, g: &GameView) {
        let down = g.down[ATTACK];
        if down && !self.was_down
            && let Some(d) = g.reach.filter(|d| *d > 0.0 && *d < 8.0)
        {
            self.last = Some(d);
            self.last_at = clock_ms();
        }
        self.was_down = down;
    }
    fn lines(&mut self, m: &ModuleState, g: &GameView) -> Vec<Line> {
        let d = if g.sample { Some(2.94) } else { self.last };
        let fresh = g.sample || d.is_some() && clock_ms() - self.last_at < f64::from(m.num("hold")) * 1000.0;
        let places = m.choice("decimals") + 1;
        let v = match d {
            Some(d) if fresh => format!("{d:.places$} blocks"),
            _ => "-".to_owned(),
        };
        vec![Line::new("Reach", v)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_extra_modules_lay_out_on_a_sample_game() {
        let mut c = horizon_client::native_ui::canvas();
        c.begin(1280, 760, 1.0);
        let g = GameView::sample();
        for mut m in all() {
            let size = m.measure(&mut c, &g);
            assert!(size[0] > 1.0 && size[1] > 1.0, "{} is empty", m.state.id);
            m.paint(&mut c, 10.0, 10.0, &g);
        }
    }

    #[test]
    fn the_combo_ends_on_damage_and_counts_one_hit_per_immunity_window() {
        let mut t = ComboTracker::default();
        t.hit(1000.0);
        t.hit(1200.0); // Inside the 450 ms immunity: no damage was done.
        assert_eq!(t.combo, 1);
        t.hit(1600.0);
        assert_eq!((t.combo, t.best), (2, 2));
        t.damaged();
        assert_eq!(t.combo, 0);
        t.hit(5000.0);
        t.tick(9000.0, 3000.0);
        assert_eq!((t.combo, t.best), (0, 2));
    }

    #[test]
    fn the_sky_has_four_phases_and_the_moon_eight() {
        assert_eq!([phase_of(0), phase_of(12500), phase_of(18000), phase_of(23500)], [0, 1, 2, 3]);
        assert_eq!(PHASES.len(), 8);
        assert_eq!(pretty("ender_pearl"), "Ender Pearl");
        assert_eq!(hms(125_000.0), "2:05");
        assert_eq!(span(3_660_000.0), "1h 01m");
        let img = moon(0, 16);
        assert_eq!((img.width, img.height), (16, 16));
    }
}
