//! `OverlayModules`: Native versions of the vanilla HUD parts you can move and
//! resize, the scoreboard sidebar and the boss bars. In the Vanilla look the
//! game keeps drawing them, moved and scaled to the module's box; in the
//! Native look the game leaves them out and Native draws them as cards.

use super::module::*;
use super::setting::Setting;
use crate::native_ui::tweaks::{self, BarsView, Rich, SidebarView};
use crate::native_ui::{Canvas, font, theme};

pub fn all() -> Vec<Module> {
    vec![scoreboard(), boss_bars()]
}

const LOOKS: &[&str] = &["Vanilla", "Native"];

/// Native look: card padding, extra leading per row, the height of a blank server row, title-to-rows gap.
const PAD: f32 = 9.0;
const ROW_AIR: f32 = 3.5;
const GAP_ROW: f32 = 7.0;
const TITLE_GAP: f32 = 5.0;
/// Vanilla font px ~ our Medium at 10.5 px (most glyphs are 5 px + 1 px spacing).
const MC_FONT: f32 = 10.5;

fn rich_width(c: &mut Canvas, face: usize, size: f32, r: &Rich) -> f32 {
    r.0.iter().map(|(t, _)| c.text_width(face, size, t)).sum()
}

fn rich(text: &str, color: u32) -> Rich {
    Rich(vec![(text.to_owned(), color)])
}

/// Draws the runs of `r` (white ones in `base`); returns the x after them.
#[allow(clippy::too_many_arguments)]
fn draw_rich(m: &ModuleState, c: &mut Canvas, face: usize, size: f32, r: &Rich, mut x: f32, y: f32, colors: bool, base: u32) -> f32 {
    let Some(h) = m.hud.as_ref() else { return x };
    for (t, col) in &r.0 {
        let col = if !colors || (col & 0xFFFFFF) == 0xFFFFFF { base } else { *col };
        x = h.text_face(c, face, size, t, x, y, col);
    }
    x
}

fn with_hud(mut m: Module, f: impl FnOnce(&mut Hud)) -> Module {
    if let Some(h) = m.state.hud.as_mut() {
        f(h);
    }
    m
}

/// The offered Background setting (the shared one is not), defaulting to `def` percent.
fn offer_background(h: &mut Hud, def: f32) {
    if let Some(s) = h.get_mut(OPACITY) {
        *s = Setting::num(OPACITY, "Background", def, 0.0, 100.0, 5.0, "%");
    }
}

// ── scoreboard ───────────────────────────────────────────────────────────

fn scoreboard() -> Module {
    with_hud(
        Module::hud(
            "scoreboard",
            "Scoreboard",
            "The server sidebar. Toggle it off to hide it, or move and resize it.",
            theme::I_LIST,
            true,
            1.0,
            0.5,
            vec![
                Setting::choice("look", "Look", 0, LOOKS),
                Setting::bool("numbers", "Show score numbers", true).fixed(),
                Setting::bool("title", "Show title", true).fixed(),
                Setting::bool("colors", "Server colours", true).fixed(),
            ],
            Box::new(Scoreboard { live: None, demo: demo_sidebar(), size: [0.0; 2], moving: false }),
        ),
        |h| {
            h.drop_setting(LABELS);
            offer_background(h, 40.0);
        },
    )
}

fn demo_sidebar() -> SidebarView {
    let names = ["", "Players: 128", "Map: Lighthouse", "", "Kills: 7", "Coins: 2,450", "", "playnative.net"];
    let cols = [0u32, 0xFFFFFFFF, 0xFFFFFFFF, 0, 0xFF55FF55, 0xFFFFAA00, 0, 0xFFFFFF55];
    let n = names.len();
    SidebarView {
        title: rich("NATIVE", 0xFFFFFF55),
        rows: names
            .iter()
            .enumerate()
            .map(|(i, name)| (rich(if name.is_empty() { " " } else { name }, cols[i]), rich(&(n - i).to_string(), 0xFFFF5555)))
            .collect(),
        bounds: [0.0; 4],
        gui: [0.0; 2],
    }
}

struct Scoreboard {
    live: Option<SidebarView>,
    demo: SidebarView,
    size: [f32; 2],
    /// The game draws it (moved into our box this frame).
    moving: bool,
}

impl Scoreboard {
    fn vanilla(m: &ModuleState) -> bool {
        m.choice("look") == 0
    }

    fn data(&self, g: &GameView) -> &SidebarView {
        match (&self.live, g.sample) {
            (Some(v), false) if !v.rows.is_empty() => v,
            _ => &self.demo,
        }
    }

    /// Vanilla look: where the game draws it and the move / scale that puts it in our box.
    fn place(&mut self, m: &ModuleState, c: &Canvas, g: &GameView) -> bool {
        let Some(h) = m.hud.as_ref() else { return false };
        let ok = Self::vanilla(m) && !g.sample && g.in_world;
        let v = match (&self.live, ok) {
            (Some(v), true) if !v.rows.is_empty() && v.gui[0] > 0.0 => v,
            _ => {
                tweaks::set(|t| t.sidebar_move = None);
                return false;
            }
        };
        let (l, s) = (c.width() / v.gui[0], h.scale());
        let (vx, vy) = (v.bounds[0], v.bounds[1]);
        let (bw, bh) = ((v.bounds[2] - vx) * l * s, (v.bounds[3] - vy) * l * s);
        let (x, y) = (Hud::pos(h.fx, c.width(), bw), Hud::pos(h.fy, c.height(), bh));
        self.size = [bw, bh];
        tweaks::set(|t| t.sidebar_move = Some([x / l - vx * s, y / l - vy * s, s]));
        true
    }

    fn chip_width(c: &mut Canvas, fs: f32, score: &Rich, s: f32) -> f32 {
        (c.text_width(font::MEDIUM, fs, "00") + 12.0 * s).max(rich_width(c, font::MEDIUM, fs, score) + 12.0 * s)
    }

    fn title_height(c: &mut Canvas, fs: f32, s: f32) -> f32 {
        c.line_height(font::SEMIBOLD, fs) + 2.0 * PAD * s
    }

    /// Widest row in vanilla GUI px: name, a space and the red score, or the title.
    fn vanilla_width(m: &ModuleState, c: &mut Canvas, d: &SidebarView) -> f32 {
        let mut w = if m.bool("title") { rich_width(c, font::MEDIUM, MC_FONT, &d.title) } else { 0.0 };
        let space = if m.bool("numbers") { c.text_width(font::MEDIUM, MC_FONT, ": ") } else { 0.0 };
        for (name, score) in &d.rows {
            let row = rich_width(c, font::MEDIUM, MC_FONT, name) + if m.bool("numbers") { space + rich_width(c, font::MEDIUM, MC_FONT, score) } else { 0.0 };
            w = w.max(row);
        }
        w
    }
}

impl Behaviour for Scoreboard {
    fn frame(&mut self, m: &ModuleState, g: &GameView) {
        self.live = if g.in_world { tweaks::sidebar_view() } else { None };
        // The game leaves it out when we draw it ourselves; the editor's stand-in needs it back.
        tweaks::set(|t| t.hide_sidebar = !Self::vanilla(m));
    }

    fn toggled(&mut self, on: bool) {
        if !on {
            tweaks::set(|t| {
                t.hide_sidebar = false;
                t.sidebar_move = None;
            });
        }
    }

    fn visible(&self, _m: &ModuleState, g: &GameView) -> bool {
        g.sample || self.live.as_ref().is_some_and(|v| !v.rows.is_empty())
    }

    /// In the Vanilla look only the look itself is offered; the Native look has the rest.
    fn hidden(&self, m: &ModuleState, id: &str) -> bool {
        if Self::vanilla(m) {
            return id != "look" && id != SCALE;
        }
        id == OPACITY && m.hud.as_ref().map_or(STYLE_CARD, Hud::style) == STYLE_TEXT
    }

    fn measure(&mut self, m: &ModuleState, c: &mut Canvas, g: &GameView) -> Option<[f32; 2]> {
        self.moving = self.place(m, c, g);
        if self.moving {
            return Some(self.size);
        }
        let hud = m.hud.as_ref()?;
        let d = self.data(g).clone();
        let s = hud.scale();
        let fs = 11.5 * s;
        if Self::vanilla(m) {
            // Vanilla look, no live sidebar (the editor's stand-in): the size the game will really draw it at.
            let l = 1.6 * s;
            let vw = Self::vanilla_width(m, c, &d);
            return Some([(vw + 4.0) * l, (d.rows.len() as f32 * 9.0 + if m.bool("title") { 10.0 } else { 1.0 }) * l]);
        }
        let lh = c.line_height(font::MEDIUM, fs) + ROW_AIR * s;
        let mut w = if m.bool("title") { rich_width(c, font::SEMIBOLD, fs, &d.title) + 8.0 * s } else { 0.0 };
        let mut body = 0.0;
        for (name, score) in &d.rows {
            if name.is_blank() {
                body += GAP_ROW * s;
                continue;
            }
            let mut row = rich_width(c, font::MEDIUM, fs, name);
            if m.bool("numbers") {
                row += 14.0 * s + Self::chip_width(c, fs, score, s);
            }
            w = w.max(row);
            body += lh;
        }
        let head = if m.bool("title") { Self::title_height(c, fs, s) + TITLE_GAP * s } else { PAD * s };
        Some([w + 2.0 * PAD * s, body + PAD * s + head])
    }

    fn paint(&mut self, m: &ModuleState, c: &mut Canvas, x: f32, y: f32, g: &GameView) -> bool {
        if self.moving {
            return true; // The game draws it, already moved into place.
        }
        let Some(hud) = m.hud.as_ref() else { return true };
        let Some(sz) = self.measure(m, c, g) else { return true };
        let d = self.data(g).clone();
        let s = hud.scale();
        let fs = 11.5 * s;
        let (colors, base) = (m.bool("colors"), hud.color());
        if Self::vanilla(m) {
            // The editor's stand-in for the vanilla sidebar: same box, rows and colours as the game's.
            let (l, row) = (1.6 * s, 9.0 * 1.6 * s);
            let vfs = 8.4 * l;
            let mut ty = y;
            if m.bool("title") {
                c.fill(x, y, sz[0], row + l, 0x66000000);
                let tw = rich_width(c, font::SEMIBOLD, vfs, &d.title);
                let mut tx = x + (sz[0] - tw) / 2.0;
                for (t, col) in &d.title.0 {
                    tx += c.text(font::SEMIBOLD, vfs, t, tx, ty + 1.2 * l, *col);
                }
                ty += row + l;
            }
            c.fill(x, ty, sz[0], d.rows.len() as f32 * row, 0x4D000000);
            for (name, score) in &d.rows {
                let mut tx = x + 2.0 * l;
                for (t, col) in &name.0 {
                    tx += c.text(font::MEDIUM, vfs, t, tx, ty + 0.6 * l, if colors { *col } else { base });
                }
                if m.bool("numbers") {
                    let nw = rich_width(c, font::MEDIUM, vfs, score);
                    let mut nx = x + sz[0] - 2.0 * l - nw;
                    for (t, _) in &score.0 {
                        nx += c.text(font::MEDIUM, vfs, t, nx, ty + 0.6 * l, 0xFFFF5555);
                    }
                }
                ty += row;
            }
            return true;
        }
        let lh = c.line_height(font::MEDIUM, fs) + ROW_AIR * s;
        let card = hud.style() != STYLE_TEXT;
        let a = hud.opacity() / 100.0;
        match hud.style() {
            STYLE_CARD => hud.background(c, x, y, sz[0], sz[1], s),
            STYLE_BRACKETS => hud.brackets(c, x, y, sz[0], sz[1], s),
            _ => {}
        }
        let pad = PAD * s;
        let mut ty = y + pad;
        if m.bool("title") {
            let th = Self::title_height(c, fs, s);
            if card && a > 0.01 {
                // A slightly brighter cap behind the title, with the card's own corner radius.
                c.push_clip(x, y, sz[0], th);
                c.round(x, y, sz[0], th + 6.0 * s, 6.0 * s, theme::alpha(0xFFFFFFFF, 0.05 * (a * 1.6).min(1.0)));
                c.pop_clip();
            }
            let tw = rich_width(c, font::SEMIBOLD, fs, &d.title);
            draw_rich(m, c, font::SEMIBOLD, fs, &d.title, x + (sz[0] - tw) / 2.0, y + pad, colors, base);
            c.fill(x + pad, y + th - 1.0, sz[0] - 2.0 * pad, 1.0, theme::alpha(base, 0.14));
            ty = y + th + TITLE_GAP * s;
        }
        for (name, score) in &d.rows {
            if name.is_blank() {
                // Blank server padding becomes a hairline, so the sidebar keeps its grouping without the holes.
                c.fill(x + pad, ty + GAP_ROW * s / 2.0, sz[0] - 2.0 * pad, 1.0, theme::alpha(base, 0.10));
                ty += GAP_ROW * s;
                continue;
            }
            draw_rich(m, c, font::MEDIUM, fs, name, x + pad, ty, colors, base);
            if m.bool("numbers") {
                let cw = Self::chip_width(c, fs, score, s);
                let nw = rich_width(c, font::MEDIUM, fs, score);
                let (cx, cy) = (x + sz[0] - pad - cw, ty - s);
                let chip_h = c.line_height(font::MEDIUM, fs) + 2.0 * s;
                if card && a > 0.01 {
                    c.round(cx, cy, cw, chip_h, chip_h / 2.0, theme::alpha(0xFFFFFFFF, 0.07));
                }
                draw_rich(m, c, font::MEDIUM, fs, score, cx + (cw - nw) / 2.0, ty, colors, base);
            }
            ty += lh;
        }
        true
    }
}

// ── boss bars ────────────────────────────────────────────────────────────

const BAR_COLORS: [u32; 7] = [0xFFEC4899, 0xFF38BDF8, 0xFFEF4444, 0xFF22C55E, 0xFFEAB308, 0xFFA855F7, 0xFFE5E5E5];

fn boss_bars() -> Module {
    with_hud(
        Module::hud(
            "bossbar",
            "Boss Bar",
            "Boss and event bars. Move them, resize them, restyle the bar.",
            theme::I_ACTIVITY,
            true,
            0.5,
            0.0,
            vec![
                Setting::choice("look", "Look", 0, LOOKS),
                Setting::bool("name", "Show name", true).fixed(),
                Setting::bool("bar", "Show bar", true).fixed(),
                Setting::bool("percent", "Show percent", false).fixed(),
                Setting::num("width", "Bar width", 182.0, 100.0, 320.0, 2.0, "px").fixed(),
            ],
            Box::new(BossBars { live: BarsView::default(), demo: demo_bars(), shown: Vec::new(), size: [0.0; 2], moving: false, last: None }),
        ),
        |h| {
            h.drop_setting(LABELS);
            if let Some(s) = h.get_mut(STYLE) {
                *s = s.clone().initial(STYLE_TEXT);
            }
            offer_background(h, 55.0);
        },
    )
}

fn demo_bars() -> BarsView {
    BarsView {
        bars: vec![tweaks::BarView { name: rich("Ender Dragon", 0xFFFFFFFF), progress: 0.62, color: 0, name_len: 12 }],
        bounds: [0.0; 4],
        gui: [0.0; 2],
    }
}

struct BossBars {
    live: BarsView,
    demo: BarsView,
    /// The progress shown per bar (it eases toward the real one), and whose it is.
    shown: Vec<(String, f32)>,
    size: [f32; 2],
    moving: bool,
    last: Option<std::time::Instant>,
}

impl BossBars {
    fn vanilla(m: &ModuleState) -> bool {
        m.choice("look") == 0
    }

    fn data(&self, g: &GameView) -> &BarsView {
        if g.sample || self.live.bars.is_empty() { &self.demo } else { &self.live }
    }

    fn place(&mut self, m: &ModuleState, c: &Canvas, g: &GameView) -> bool {
        let Some(h) = m.hud.as_ref() else { return false };
        if !Self::vanilla(m) || g.sample || !g.in_world || self.live.bars.is_empty() || self.live.gui[0] <= 0.0 {
            tweaks::set(|t| t.boss_move = None);
            return false;
        }
        let v = &self.live;
        let (l, s) = (c.width() / v.gui[0], h.scale());
        let (vx, vy) = (v.bounds[0], v.bounds[1]);
        let (bw, bh) = ((v.bounds[2] - vx) * l * s, (v.bounds[3] - vy) * l * s);
        let (x, y) = (Hud::pos(h.fx, c.width(), bw), Hud::pos(h.fy, c.height(), bh));
        self.size = [bw, bh];
        tweaks::set(|t| t.boss_move = Some([x / l - vx * s, y / l - vy * s, s]));
        true
    }

    fn row_h(m: &ModuleState, c: &mut Canvas, s: f32) -> f32 {
        let mut h = 0.0;
        if m.bool("name") {
            h += c.line_height(font::SEMIBOLD, 11.5 * s) + 6.0 * s;
        }
        if m.bool("bar") {
            h += 6.0 * s;
        }
        h + 6.0 * s
    }

    fn preview_row_h(c: &mut Canvas, l: f32) -> f32 {
        c.line_height(font::SEMIBOLD, 8.4 * l) + 3.0 * l + 5.0 * l + 5.0 * l
    }
}

impl Behaviour for BossBars {
    fn frame(&mut self, m: &ModuleState, g: &GameView) {
        self.live = if g.in_world { tweaks::bars_view() } else { BarsView::default() };
        tweaks::set(|t| t.hide_bossbars = !Self::vanilla(m));
    }

    fn toggled(&mut self, on: bool) {
        if !on {
            tweaks::set(|t| {
                t.hide_bossbars = false;
                t.boss_move = None;
            });
        }
    }

    fn visible(&self, _m: &ModuleState, g: &GameView) -> bool {
        g.sample || !self.live.bars.is_empty()
    }

    fn hidden(&self, m: &ModuleState, id: &str) -> bool {
        if Self::vanilla(m) {
            return id != "look" && id != SCALE;
        }
        id == OPACITY && m.hud.as_ref().map_or(STYLE_CARD, Hud::style) != STYLE_CARD
    }

    fn measure(&mut self, m: &ModuleState, c: &mut Canvas, g: &GameView) -> Option<[f32; 2]> {
        self.moving = self.place(m, c, g);
        if self.moving {
            return Some(self.size);
        }
        let hud = m.hud.as_ref()?;
        let d = self.data(g).clone();
        let s = hud.scale();
        let n = d.bars.len().max(1) as f32;
        if Self::vanilla(m) {
            let l = 1.6 * s;
            return Some([182.0 * l, n * Self::preview_row_h(c, l) - 5.0 * l]);
        }
        let mut w = m.num("width") * s;
        if m.bool("name") {
            for b in &d.bars {
                w = w.max(rich_width(c, font::SEMIBOLD, 11.5 * s, &b.name) + if m.bool("percent") { 40.0 * s } else { 0.0 });
            }
        }
        let card = hud.style() == STYLE_CARD;
        Some([w + if card { 16.0 * s } else { 0.0 }, n * Self::row_h(m, c, s) - 6.0 * s + if card { 12.0 * s } else { 0.0 }])
    }

    fn paint(&mut self, m: &ModuleState, c: &mut Canvas, x: f32, y: f32, g: &GameView) -> bool {
        if self.moving {
            return true;
        }
        let Some(hud) = m.hud.as_ref() else { return true };
        let Some(sz) = self.measure(m, c, g) else { return true };
        let d = self.data(g).clone();
        let (s, base) = (hud.scale(), hud.color());
        let fs = 11.5 * s;
        if Self::vanilla(m) {
            // The editor's stand-in: the UI font reserves its measured line box plus a visible gap.
            let (l, vfs) = (1.6 * s, 8.4 * 1.6 * s);
            let mut ty = y;
            for b in &d.bars {
                let nw = rich_width(c, font::SEMIBOLD, vfs, &b.name);
                let mut tx = x + (sz[0] - nw) / 2.0;
                for (t, col) in &b.name.0 {
                    tx += c.text(font::SEMIBOLD, vfs, t, tx, ty, if (col & 0xFFFFFF) == 0xFFFFFF { base } else { *col });
                }
                let col = BAR_COLORS[b.color.min(BAR_COLORS.len() - 1)];
                let by = ty + c.line_height(font::SEMIBOLD, vfs) + 3.0 * l;
                c.fill(x, by, sz[0], 5.0 * l, theme::mix(col, 0xFF000000, 0.6));
                c.fill(x, by, sz[0] * b.progress.clamp(0.0, 1.0), 5.0 * l, col);
                ty += Self::preview_row_h(c, l);
            }
            return true;
        }
        let card = hud.style() == STYLE_CARD;
        match hud.style() {
            STYLE_CARD => hud.background(c, x, y, sz[0], sz[1], s),
            STYLE_BRACKETS => hud.brackets(c, x, y, sz[0], sz[1], s),
            _ => {}
        }
        let now = std::time::Instant::now();
        let dt = self.last.map_or(0.016, |t| now.duration_since(t).as_secs_f32().min(0.1));
        self.last = Some(now);
        let k = 1.0 - (-10.0 * dt).exp();
        let (ix, iw) = (x + if card { 8.0 * s } else { 0.0 }, sz[0] - if card { 16.0 * s } else { 0.0 });
        let mut ty = y + if card { 6.0 * s } else { 0.0 };
        self.shown.resize(d.bars.len(), (String::new(), 0.0));
        for (i, b) in d.bars.iter().enumerate() {
            let id: String = b.name.0.iter().map(|(t, _)| t.as_str()).collect();
            if self.shown[i].0 != id {
                self.shown[i] = (id, b.progress);
            }
            self.shown[i].1 += (b.progress - self.shown[i].1) * k;
            if m.bool("name") {
                let nw = rich_width(c, font::SEMIBOLD, fs, &b.name);
                let pc = m.bool("percent").then(|| format!("  {}%", (b.progress * 100.0).round() as i32));
                let pw = pc.as_ref().map_or(0.0, |p| c.text_width(font::MEDIUM, fs, p));
                let mut tx = ix + (iw - nw - pw) / 2.0;
                tx = draw_rich(m, c, font::SEMIBOLD, fs, &b.name, tx, ty, true, base);
                if let Some(p) = pc {
                    hud.text_face(c, font::MEDIUM, fs, &p, tx, ty, theme::alpha(base, 0.62));
                }
                ty += c.line_height(font::SEMIBOLD, fs) + 6.0 * s;
            }
            if m.bool("bar") {
                let col = BAR_COLORS[b.color.min(BAR_COLORS.len() - 1)];
                let bh = 5.0 * s;
                c.round(ix, ty, iw, bh, bh / 2.0, 0x66000000);
                c.outline(ix, ty, iw, bh, bh / 2.0, 1.0, theme::HAIRLINE);
                let p = self.shown[i].1.clamp(0.0, 1.0);
                if p > 0.005 {
                    c.round(ix, ty, bh.max(iw * p), bh, bh / 2.0, col);
                }
                ty += 6.0 * s;
            }
            ty += 6.0 * s;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_demo_sidebar_has_blank_spacer_rows() {
        let d = demo_sidebar();
        assert_eq!(d.rows.len(), 8);
        assert!(d.rows[0].0.is_blank() && d.rows[3].0.is_blank() && !d.rows[1].0.is_blank());
    }

    #[test]
    fn both_modules_start_on_the_vanilla_look() {
        for m in all() {
            assert_eq!(m.state.choice("look"), 0, "{}", m.state.id);
            assert!(m.state.enabled, "{}", m.state.id);
        }
    }

    #[test]
    fn the_native_look_asks_the_game_to_leave_its_own_out() {
        let mut m = scoreboard();
        m.state.settings.iter_mut().find(|s| s.id == "look").unwrap().set_choice(1);
        let g = GameView::default();
        m.behaviour.frame(&m.state, &g);
        assert!(tweaks::get().hide_sidebar);
        m.behaviour.toggled(false);
        assert!(!tweaks::get().hide_sidebar);
    }
}
