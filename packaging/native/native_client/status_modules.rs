//! `StatusModules`: potion effects, armour and inventory. They draw the
//! game's own textures (effect icons, item icons), so they follow the
//! resource pack the player uses: the canvas asks the game to draw those
//! squares in place (`Canvas::host`).

use super::module::*;
use super::setting::Setting;
use super::status;
use horizon_client::native_ui::canvas::clock_ms;
use horizon_client::native_ui::{Canvas, font, theme};
use std::collections::HashMap;

const GOOD: u32 = 0xFF4ADE80;
const BAD: u32 = 0xFFF87171;

pub fn all() -> Vec<Module> {
    vec![effects(), armor(), inventory()]
}

/// A key for an effect id, shared by the module and the host that draws its icon.
pub fn effect_key(id: &str) -> u64 {
    id.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3))
}

/// The shared plumbing: one measure / draw routine and a card / text / brackets frame around it.
trait Boxed: Send {
    /// Lays out (`draw` false) or draws the module with its top-left corner at x, y; returns the size.
    fn render(&mut self, m: &ModuleState, c: &mut Canvas, g: &GameView, x: f32, y: f32, draw: bool) -> [f32; 2];
    fn visible(&self, _m: &ModuleState, _g: &GameView) -> bool {
        true
    }
    fn frame(&mut self, _m: &ModuleState, _g: &GameView) {}
    /// Space between the frame and the content.
    fn pad(&self, m: &ModuleState) -> f32 {
        let s = scale_of(m);
        match style_of(m) {
            STYLE_CARD => 5.0 * s,
            STYLE_BRACKETS => 4.0 * s,
            _ => s,
        }
    }
}

struct Framed<T: Boxed>(T);

impl<T: Boxed> Behaviour for Framed<T> {
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
    fn frame(&mut self, m: &ModuleState, g: &GameView) {
        self.0.frame(m, g);
    }
    fn needs_vitals(&self) -> bool {
        true
    }
}

fn scale_of(m: &ModuleState) -> f32 {
    m.hud.as_ref().map_or(1.0, Hud::scale)
}

fn style_of(m: &ModuleState) -> usize {
    m.hud.as_ref().map_or(STYLE_CARD, Hud::style)
}

#[allow(clippy::too_many_arguments)]
fn text(m: &ModuleState, c: &mut Canvas, face: usize, size: f32, t: &str, x: f32, y: f32, argb: u32) {
    if let Some(h) = m.hud.as_ref() {
        h.text_face(c, face, size, t, x, y, argb);
    }
}

fn pulse(now: f64, period: f64, lo: f32) -> u32 {
    (255.0 * (lo + (1.0 - lo) * (0.5 + 0.5 * (now / period).sin() as f32))).round() as u32
}

fn with_hud(mut m: Module, f: impl FnOnce(&mut Hud)) -> Module {
    if let Some(h) = m.state.hud.as_mut() {
        f(h);
    }
    m
}

/// An item's icon from the game's textures.
fn item(c: &mut Canvas, sl: &StackView, x: f32, y: f32, size: f32, tint: u32) {
    c.host(HOST_ITEM, sl.key, x, y, size, tint);
}

// ── potion effects ───────────────────────────────────────────────────────

fn effects() -> Module {
    with_hud(
        Module::hud(
            "effects",
            "Potion Effects",
            "Your active potion effects with the game's own icons, level and time left.",
            theme::I_FLASK,
            false,
            1.0,
            0.04,
            vec![
                Setting::choice("layout", "Layout", 0, &["List", "Row"]),
                Setting::bool("icons", "Show icons", true).fixed(),
                Setting::bool("names", "Show names", true).fixed(),
                Setting::bool("level", "Show level", true).fixed(),
                Setting::bool("time", "Show time left", true).fixed(),
                Setting::bool("bar", "Time bar", true).fixed(),
                Setting::bool("type", "Colour good / bad effects", true).fixed(),
                Setting::bool("beacon", "Show beacon effects", true).fixed(),
                Setting::bool("flash", "Flash when about to run out", true).fixed(),
                Setting::choice("sort", "Sort by", 0, &["Time left", "Level", "Name"]).fixed(),
                Setting::bool("hide", "Hide the vanilla effect icons", true).fixed(),
            ],
            Box::new(Framed(Effects { peak: HashMap::new(), generation: 0 })),
        ),
        |h| h.drop_setting(LABELS),
    )
}

struct Effects {
    /// id -> (longest time seen, frame it was last seen): the bar shows time left against the full length.
    peak: HashMap<String, (i32, u64)>,
    generation: u64,
}

impl Effects {
    fn collect(m: &ModuleState, g: &GameView) -> Vec<EffectView> {
        let mut view: Vec<EffectView> = g
            .vitals
            .effects
            .iter()
            .filter(|e| !(e.ambient && !m.bool("beacon")))
            .cloned()
            .collect();
        let time = |e: &EffectView| if e.infinite { i32::MAX } else { e.ticks };
        match m.choice("sort") {
            1 => view.sort_by(|a, b| b.amp.cmp(&a.amp).then(time(a).cmp(&time(b)))),
            2 => view.sort_by_key(|e| status::effect_name(&e.id)),
            _ => view.sort_by_key(time),
        }
        view.truncate(12);
        view
    }

    fn fraction(&self, g: &GameView, e: &EffectView) -> f32 {
        if e.infinite {
            return 1.0;
        }
        let total = if g.sample { 20 * 600 } else { self.peak.get(&e.id).map_or(e.ticks, |p| p.0) };
        (e.ticks as f32 / total.max(1) as f32).clamp(0.0, 1.0)
    }

    fn title(m: &ModuleState, e: &EffectView) -> String {
        let n = if m.bool("names") { status::effect_name(&e.id) } else { String::new() };
        let l = if m.bool("level") && e.amp > 0 { status::roman(e.amp + 1) } else { String::new() };
        match (n.is_empty(), l.is_empty()) {
            (true, _) => l,
            (_, true) => n,
            _ => format!("{n} {l}"),
        }
    }

    fn icon(c: &mut Canvas, id: &str, x: f32, y: f32, size: f32, tint: u32) {
        c.host(HOST_EFFECT, effect_key(id), x, y, size, tint);
    }
}

impl Boxed for Effects {
    fn visible(&self, m: &ModuleState, g: &GameView) -> bool {
        g.sample || !Self::collect(m, g).is_empty()
    }

    fn frame(&mut self, _m: &ModuleState, g: &GameView) {
        self.generation += 1;
        for e in &g.vitals.effects {
            let entry = self.peak.entry(e.id.clone()).or_insert((e.ticks, self.generation));
            entry.0 = entry.0.max(e.ticks);
            entry.1 = self.generation;
        }
        let generation = self.generation;
        self.peak.retain(|_, p| p.1 == generation);
    }

    fn render(&mut self, m: &ModuleState, c: &mut Canvas, g: &GameView, x: f32, y: f32, draw: bool) -> [f32; 2] {
        let s = scale_of(m);
        let pad = self.pad(m);
        let view = Self::collect(m, g);
        let n = view.len();
        if n == 0 {
            return [1.0, 1.0];
        }
        let now = clock_ms();
        let color = m.hud.as_ref().map_or(0xFFFFFFFF, Hud::color);
        let (ic, gap) = (18.0 * s, 3.0 * s);
        let (show_icon, bar_on, time_on) = (m.bool("icons"), m.bool("bar"), m.bool("time"));
        let (nf, tf) = (11.5 * s, 10.5 * s);
        let lh1 = c.line_height(font::SEMIBOLD, nf);
        let lh2 = c.line_height(font::MEDIUM, tf);
        let urgent = |e: &EffectView| m.bool("flash") && !e.infinite && !e.ambient && status::seconds(e.ticks) <= 10;
        let accent = |e: &EffectView| {
            if m.bool("type") {
                if status::harmful(&e.id) { BAD } else { GOOD }
            } else {
                color
            }
        };
        if m.choice("layout") == 0 {
            let name_line = m.bool("names") || m.bool("level");
            let text_h = match (name_line, time_on) {
                (true, true) => lh1 + lh2,
                (true, false) => lh1,
                (false, true) => lh2,
                _ => 0.0,
            };
            let content = (if show_icon { ic } else { 0.0 }).max(text_h);
            let row_h = content + if bar_on { 4.0 * s } else { 0.0 };
            let mut inner: f32 = 0.0;
            for e in &view {
                let t1 = if name_line { c.text_width(font::SEMIBOLD, nf, &Self::title(m, e)) } else { 0.0 };
                let t2 = if time_on {
                    c.text_width(font::MEDIUM, tf, &if e.infinite { "Infinite".to_owned() } else { status::time(e.ticks) })
                } else {
                    0.0
                };
                let tw = t1.max(t2);
                inner = inner.max((if show_icon { ic + if tw > 0.0 { 6.0 * s } else { 0.0 } } else { 0.0 }) + tw);
            }
            inner = inner.max(48.0 * s);
            if draw {
                let mut ry = y + pad;
                for e in &view {
                    let urgent = urgent(e);
                    let a = if urgent { pulse(now, 140.0, 0.45) } else { 255 };
                    let tint = a << 24 | 0xFFFFFF;
                    let accent = accent(e);
                    let mut tx = x + pad;
                    if show_icon {
                        Self::icon(c, &e.id, tx, ry + (content - ic) / 2.0, ic, tint);
                        tx += ic + 6.0 * s;
                    }
                    let mut ty = ry + (content - text_h) / 2.0;
                    if name_line {
                        text(m, c, font::SEMIBOLD, nf, &Self::title(m, e), tx, ty, theme::alpha(color, a as f32 / 255.0));
                        ty += lh1;
                    }
                    if time_on {
                        let tc = if urgent { BAD } else { theme::alpha(color, 0.72) };
                        let t = if e.infinite { "Infinite".to_owned() } else { status::time(e.ticks) };
                        text(m, c, font::MEDIUM, tf, &t, tx, ty, theme::alpha(tc, a as f32 / 255.0));
                    }
                    if bar_on {
                        let by = ry + content + 1.5 * s;
                        let bh = (2.0 * s).max(1.5);
                        c.round(x + pad, by, inner, bh, bh / 2.0, 0x33FFFFFF);
                        c.round(x + pad, by, (inner * self.fraction(g, e)).max(bh), bh, bh / 2.0, theme::alpha(accent, a as f32 / 255.0));
                    }
                    ry += row_h + gap;
                }
            }
            return [inner + 2.0 * pad + 4.0 * s, n as f32 * row_h + (n - 1) as f32 * gap + 2.0 * pad];
        }
        // Row of tiles: icon (or the name when icons are off), level badge, time below.
        let mut total = 0.0;
        let mut widths = Vec::with_capacity(n);
        for (i, e) in view.iter().enumerate() {
            let top = if show_icon {
                ic
            } else {
                let t = Self::title(m, e);
                c.text_width(font::SEMIBOLD, nf, &if t.is_empty() { status::effect_name(&e.id) } else { t })
            };
            let tw = if time_on {
                c.text_width(font::MEDIUM, tf, &if e.infinite { "Inf".to_owned() } else { status::time(e.ticks) })
            } else {
                0.0
            };
            let w = top.max(tw) + 4.0 * s;
            widths.push(w);
            total += w + if i > 0 { gap } else { 0.0 };
        }
        let tile_h = (if show_icon { ic } else { lh1 }) + if time_on { lh2 + s } else { 0.0 } + if bar_on { 4.0 * s } else { 0.0 };
        if draw {
            let mut tx = x + pad;
            for (i, e) in view.iter().enumerate() {
                let urgent = urgent(e);
                let a = if urgent { pulse(now, 140.0, 0.45) } else { 255 };
                let af = a as f32 / 255.0;
                let accent = accent(e);
                let cx = tx + widths[i] / 2.0;
                let mut ty = y + pad;
                if show_icon {
                    Self::icon(c, &e.id, cx - ic / 2.0, ty, ic, a << 24 | 0xFFFFFF);
                    if m.bool("level") && e.amp > 0 {
                        let l = status::roman(e.amp + 1);
                        let lf = 8.5 * s;
                        let lw = c.text_width(font::SEMIBOLD, lf, &l);
                        text(m, c, font::SEMIBOLD, lf, &l, cx + ic / 2.0 - lw, ty - s, theme::alpha(0xFFFFFFFF, af));
                    }
                    ty += ic;
                } else {
                    let t = Self::title(m, e);
                    let t = if t.is_empty() { status::effect_name(&e.id) } else { t };
                    let tw = c.text_width(font::SEMIBOLD, nf, &t);
                    text(m, c, font::SEMIBOLD, nf, &t, cx - tw / 2.0, ty, theme::alpha(color, af));
                    ty += lh1;
                }
                if time_on {
                    let t = if e.infinite { "Inf".to_owned() } else { status::time(e.ticks) };
                    let tw = c.text_width(font::MEDIUM, tf, &t);
                    text(m, c, font::MEDIUM, tf, &t, cx - tw / 2.0, ty + s, theme::alpha(if urgent { BAD } else { theme::alpha(color, 0.72) }, af));
                    ty += lh2 + s;
                }
                if bar_on {
                    let bh = (2.0 * s).max(1.5);
                    c.round(tx + s, ty + 1.5 * s, widths[i] - 2.0 * s, bh, bh / 2.0, 0x33FFFFFF);
                    c.round(
                        tx + s,
                        ty + 1.5 * s,
                        ((widths[i] - 2.0 * s) * self.fraction(g, e)).max(bh),
                        bh,
                        bh / 2.0,
                        theme::alpha(accent, af),
                    );
                }
                tx += widths[i] + gap;
            }
        }
        [total + 2.0 * pad + 4.0 * s, tile_h + 2.0 * pad]
    }
}

// ── armour status (Armor Stats) ──────────────────────────────────────────

fn armor() -> Module {
    with_hud(
        Module::hud(
            "armor",
            "Armor Stats",
            "Displays your equipped armor pieces and their durability bars.",
            theme::I_SHIELD,
            false,
            0.0,
            0.7,
            vec![
                Setting::choice("layout", "Orientation", 0, &["Vertical", "Horizontal"]).shorts(&["Vertical", "Horizontal"]),
                Setting::bool("durability", "Show durability", true).fixed(),
                Setting::bool("empty", "Show empty slots", false).fixed(),
            ],
            Box::new(Framed(Armor)),
        ),
        |h| h.drop_setting(LABELS),
    )
}

struct Armor;

impl Boxed for Armor {
    fn visible(&self, m: &ModuleState, g: &GameView) -> bool {
        g.sample || g.vitals.armor.iter().any(Option::is_some) || m.bool("empty")
    }

    fn render(&mut self, m: &ModuleState, c: &mut Canvas, g: &GameView, x: f32, y: f32, draw: bool) -> [f32; 2] {
        let s = scale_of(m);
        let pad = self.pad(m);
        let item_size = 16.0 * s;
        let spacing = 4.0 * s;
        let bar_h = 3.0 * s; // 2px bar + 1px gap
        let durability = m.bool("durability");
        let slot_h = if durability { item_size + bar_h } else { item_size };
        let show_empty = m.bool("empty");
        let visible = g.vitals.armor.iter().filter(|a| a.is_some() || show_empty).count();
        if visible == 0 {
            return [1.0, 1.0];
        }
        let vertical = m.choice("layout") == 0;
        let (w, h) = if vertical {
            (item_size + pad * 2.0, visible as f32 * slot_h + (visible - 1) as f32 * spacing + pad * 2.0)
        } else {
            (visible as f32 * item_size + (visible - 1) as f32 * spacing + pad * 2.0, slot_h + pad * 2.0)
        };
        if draw {
            let (mut ox, mut oy) = (0.0, 0.0);
            for sl in &g.vitals.armor {
                if sl.is_none() && !show_empty {
                    continue;
                }
                let ix = if vertical { x + pad } else { x + pad + ox };
                let iy = if vertical { y + pad + oy } else { y + pad };
                if let Some(sl) = sl {
                    item(c, sl, ix, iy, item_size, 0xFFFFFFFF);
                    if durability && sl.max > 0 && sl.damage > 0 {
                        let pct = sl.durability();
                        let (bx, by, bw) = (ix + 2.0 * s, iy + item_size + s, 13.0 * s);
                        let bh = (2.0 * s).max(1.0);
                        c.fill(bx, by, bw, bh, 0xFF000000);
                        c.fill(bx, by, (bw * pct).round().max(1.0), bh, status::wear(pct));
                    }
                } else {
                    // Empty slot placeholder.
                    c.round(ix + item_size * 0.1, iy + item_size * 0.1, item_size * 0.8, item_size * 0.8, 3.0 * s, 0x14FFFFFF);
                }
                if vertical {
                    oy += slot_h + spacing;
                } else {
                    ox += item_size + spacing;
                }
            }
        }
        [w, h]
    }
}

// ── inventory ────────────────────────────────────────────────────────────

fn inventory() -> Module {
    with_hud(
        Module::hud(
            "inventory",
            "Inventory",
            "Your backpack on screen without opening it, with the game's item icons and counts (custom items too).",
            theme::I_BACKPACK,
            false,
            1.0,
            0.8,
            vec![
                Setting::choice("layout", "Layout", 0, &["Horizontal", "Vertical"]),
                Setting::bool("background", "Background", true),
                Setting::bool("hotbar", "Show the hotbar row", false).fixed(),
                Setting::bool("slots", "Slot backgrounds", true).fixed(),
                Setting::bool("counts", "Item counts", true).fixed(),
                Setting::bool("bars", "Durability bars", true).fixed(),
                Setting::bool("selected", "Highlight the selected slot", true).fixed(),
                Setting::bool("hide", "Hide when the inventory is empty", false).fixed(),
            ],
            Box::new(Inventory),
        ),
        |h| {
            h.drop_setting(LABELS);
            h.drop_setting(COLOR);
        },
    )
}

struct Inventory;

impl Inventory {
    fn pad(m: &ModuleState) -> f32 {
        let s = scale_of(m);
        match style_of(m) {
            STYLE_CARD if m.bool("background") => 6.0 * s,
            STYLE_BRACKETS => 4.0 * s,
            _ => 2.0 * s,
        }
    }

    fn render(m: &ModuleState, c: &mut Canvas, g: &GameView, x: f32, y: f32, draw: bool) -> [f32; 2] {
        let s = scale_of(m);
        let pad = Self::pad(m);
        let (cell, gap) = (20.0 * s, 2.0 * s);
        let step = cell + gap;
        let (ic, sep) = (16.0 * s, 6.0 * s);
        let hotbar = m.bool("hotbar");
        let lines = 3 + usize::from(hotbar);
        let vertical = m.choice("layout") == 1;
        let along = 9.0 * step - gap;
        let across = lines as f32 * step - gap + if hotbar { sep - gap } else { 0.0 };
        let (w, h) = (if vertical { across } else { along } + 2.0 * pad, if vertical { along } else { across } + 2.0 * pad);
        if draw {
            let (cr, fs) = (4.0 * s, 10.0 * s);
            let flh = c.line_height(font::BOLD, fs);
            let (background, slots) = (m.bool("background"), m.bool("slots"));
            if hotbar {
                if !vertical {
                    let div_y = y + pad + 3.0 * step - gap + sep / 2.0;
                    c.fill(x + pad + 2.0 * s, div_y, along - 4.0 * s, 1.0, 0x1AFFFFFF);
                } else {
                    let div_x = x + pad + 3.0 * step - gap + sep / 2.0;
                    c.fill(div_x, y + pad + 2.0 * s, 1.0, along - 4.0 * s, 0x1AFFFFFF);
                }
            }
            for r in 0..lines {
                let hot = r == 3;
                let off = r as f32 * step + if hot { sep - gap } else { 0.0 };
                for col in 0..9 {
                    let sl = g.vitals.inv.get(if hot { col } else { 9 + r * 9 + col }).and_then(Option::as_ref);
                    let sx = x + pad + if vertical { off } else { col as f32 * step };
                    let sy = y + pad + if vertical { col as f32 * step } else { off };
                    if slots && background {
                        c.round(sx, sy, cell, cell, cr, 0x40000000);
                        c.outline(sx, sy, cell, cell, cr, 1.0, 0x14FFFFFF);
                        c.fill(sx + cr, sy + 1.0, cell - 2.0 * cr, (s * 0.75).max(1.0), 0x0AFFFFFF);
                    }
                    if hot && m.bool("selected") && col == g.vitals.selected {
                        c.round(sx, sy, cell, cell, cr, 0x1AFFFFFF);
                        c.outline(sx - 0.5 * s, sy - 0.5 * s, cell + s, cell + s, cr + 0.5 * s, (1.5 * s).max(1.0), 0xCCFFFFFF);
                    }
                    let Some(sl) = sl else { continue };
                    let (ix, iy) = (sx + (cell - ic) / 2.0, sy + (cell - ic) / 2.0);
                    item(c, sl, ix, iy, ic, 0xFFFFFFFF);
                    if m.bool("bars") && sl.max > 0 && sl.damage > 0 {
                        let pct = sl.durability();
                        let (bx, by, bw) = (sx + 2.5 * s, sy + cell - 3.0 * s, cell - 5.0 * s);
                        let bh = (2.0 * s).max(1.5);
                        c.fill(bx, by, bw, bh, 0xCC000000);
                        c.fill(bx, by, (bw * pct).round().max(1.0), bh, status::wear(pct));
                    }
                    if m.bool("counts") && sl.count > 1 {
                        let t = sl.count.to_string();
                        let tw = c.text_width(font::BOLD, fs, &t);
                        let (tx, ty) = (sx + cell - tw - 1.5 * s, sy + cell - flh + s);
                        c.text(font::BOLD, fs, &t, tx + 1.0, ty + 1.0, 0xDD000000);
                        c.text(font::BOLD, fs, &t, tx, ty, 0xFFFFFFFF);
                    }
                }
            }
        }
        [w, h]
    }
}

impl Behaviour for Inventory {
    fn measure(&mut self, m: &ModuleState, c: &mut Canvas, g: &GameView) -> Option<[f32; 2]> {
        let size = Self::render(m, c, g, 0.0, 0.0, false);
        Some([size[0].max(1.0), size[1].max(1.0)])
    }

    fn paint(&mut self, m: &ModuleState, c: &mut Canvas, x: f32, y: f32, g: &GameView) -> bool {
        let sz = Self::render(m, c, g, 0.0, 0.0, false);
        if let Some(hud) = m.hud.as_ref() {
            let s = hud.scale();
            let (w, h) = (sz[0].max(1.0), sz[1].max(1.0));
            if hud.style() == STYLE_CARD && m.bool("background") {
                hud.background(c, x, y, w, h, s);
            } else if hud.style() == STYLE_BRACKETS {
                hud.brackets(c, x, y, w, h, s);
            }
        }
        Self::render(m, c, g, x, y, true);
        true
    }

    fn needs_vitals(&self) -> bool {
        true
    }

    fn visible(&self, m: &ModuleState, g: &GameView) -> bool {
        if g.sample || !m.bool("hide") {
            return true;
        }
        let from = if m.bool("hotbar") { 0 } else { 9 };
        g.vitals.inv.iter().skip(from).any(Option::is_some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_status_modules_lay_out_on_a_sample_game() {
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
    fn effects_show_the_shortest_time_first() {
        let m = &effects().state;
        let g = GameView::sample();
        let view = Effects::collect(m, &g);
        assert_eq!(view[0].id, "regeneration");
        assert_eq!(view.len(), 5);
    }
}
