//! `HudEditor`: the screen "Edit HUD" opens: the game dimmed, every HUD module
//! in place with a name tag, a gear (its settings) and a minus (take it off the
//! screen), and a hub in the middle with the way into the rest of Native. Drag
//! a module to move it (it snaps to the edges, the middle and other modules),
//! drag a corner or scroll over it to resize, right-click for settings.
#![allow(dead_code)]

use super::menu::{self, Action, TAB_COSMETICS, TAB_MODS, TAB_MUSIC, TAB_SETTINGS, clamp01};
use super::module::{GameView, Hud, MARGIN, SCALE};
use super::modules::Modules;
use super::setting::Setting;
use super::textfield::TextField;
use super::ui::Ui;
use super::widgets::{ROW, Widgets};
use horizon_client::native_ui::{Canvas, Image, assets, font, icons, theme};
use std::sync::Arc;

const SNAP: f32 = 6.0;
const BLUE: u32 = 0xFF3B82F6;
const RED: u32 = 0xFFEF4444;
const LOGO_KEY: u64 = 0x4D45_4E55_4855_4221;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Part {
    Keybind,
    Options,
    Appearance,
}

pub struct HudEditor {
    drag: Option<usize>,
    popover: Option<usize>,
    sizing: Option<usize>,
    /// Corner being dragged (0 top-left, 1 top-right, 2 bottom-right, 3 bottom-left).
    size_corner: usize,
    size_start: f32,
    anchor: [f32; 2],
    start_size: [f32; 2],
    grab: [f32; 2],
    press: [f32; 2],
    pop: [f32; 4],
    shown: Vec<usize>,
    rects: Vec<[f32; 4]>,
    /// Per shown module: {tag x, tag y, tag w, gear x, minus x}.
    tags: Vec<[f32; 5]>,
    guide_x: Option<f32>,
    guide_y: Option<f32>,
    sample: GameView,
    shown_at: f64,
    backdrop: Option<Arc<Image>>,
    logo: Option<Arc<Image>>,
    drawer_open: bool,
    drawer_search: TextField,
    drawer_filter: usize,
    drawer_key: String,
    drawer_scroll: f32,
    drawer_pos: [f32; 2],
    grab_d: [f32; 2],
    drawer_moved: bool,
    drawer_rect: [f32; 4],
    hub_rect: [f32; 4],
    notice_rect: [f32; 4],
    notice: Option<String>,
    removed: Option<usize>,
    notice_at: f64,
    notice_frame_at: f64,
    tile_lift: f32,
    widgets: Widgets,
}

impl Default for HudEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl HudEditor {
    pub fn new() -> Self {
        let mut drawer_search = TextField::new("hud:drawer:search");
        drawer_search.bare = true;
        drawer_search.max_length = 64;
        HudEditor {
            drag: None,
            popover: None,
            sizing: None,
            size_corner: 0,
            size_start: 1.0,
            anchor: [0.0; 2],
            start_size: [1.0; 2],
            grab: [0.0; 2],
            press: [0.0; 2],
            pop: [0.0; 4],
            shown: Vec::new(),
            rects: Vec::new(),
            tags: Vec::new(),
            guide_x: None,
            guide_y: None,
            sample: GameView::sample(),
            shown_at: 0.0,
            backdrop: Image::decode(assets::HUD_BACKGROUND),
            logo: Image::decode(assets::LOGO),
            drawer_open: false,
            drawer_search,
            drawer_filter: 0,
            drawer_key: String::new(),
            drawer_scroll: 0.0,
            drawer_pos: [0.0; 2],
            grab_d: [0.0; 2],
            drawer_moved: false,
            drawer_rect: [0.0; 4],
            hub_rect: [0.0; 4],
            notice_rect: [0.0; 4],
            notice: None,
            removed: None,
            notice_at: 0.0,
            notice_frame_at: 0.0,
            tile_lift: 0.0,
            widgets: Widgets::default(),
        }
    }

    /// The editor opens: the intro plays again and nothing is picked up.
    pub fn opened(&mut self, now: f64) {
        self.shown_at = now;
        self.popover = None;
        self.drag = None;
        self.sizing = None;
        self.drawer_open = false;
        self.notice = None;
        self.removed = None;
    }

    pub fn escape(&mut self, ui: &mut Ui) -> bool {
        if self.widgets.escape() {
            return true;
        }
        if self.drawer_search.focused(ui) {
            ui.focus = None;
            self.drawer_search.clear();
            return true;
        }
        if self.popover.is_some() {
            self.popover = None;
            return true;
        }
        if self.drawer_open {
            self.drawer_open = false;
            return true;
        }
        false
    }

    fn inside(r: &[f32; 4], c: &Canvas, ui: &Ui) -> bool {
        r[2] > 0.0 && ui.hover(c, r[0], r[1], r[2], r[3])
    }

    fn say(&mut self, ui: &Ui, text: String, undo: Option<usize>) {
        self.notice = Some(text);
        self.removed = undo;
        self.notice_at = ui.now;
        self.notice_frame_at = ui.now;
    }

    fn game_for<'a>(mods: &Modules, mi: usize, live: &'a GameView, sample: &'a GameView, in_world: bool) -> &'a GameView {
        let m = &mods.all[mi];
        if in_world && m.behaviour.visible(&m.state, live) { live } else { sample }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(&mut self, ui: &mut Ui, c: &mut Canvas, mods: &mut Modules, live: &GameView, in_world: bool) -> Action {
        let mut action = Action::None;
        let (w_all, h_all) = (c.width(), c.height());
        let sample = self.sample.clone();
        let inn = clamp01(((ui.now - self.shown_at) / 220.0) as f32);
        if !in_world {
            if let Some(bg) = self.backdrop.clone() {
                c.image_cover(&bg, 0.0, 0.0, w_all, h_all, 1.02, 0.5, 0.5, theme::alpha(0xFFFFFFFF, inn));
            }
        }
        c.fill(0.0, 0.0, w_all, h_all, theme::alpha(if in_world { 0x80000000 } else { 0x59000000 }, inn));
        c.gradient_v(0.0, 0.0, w_all, h_all * 0.25, theme::alpha(0x66000000, inn), 0);
        c.gradient_v(0.0, h_all * 0.7, w_all, h_all * 0.3, 0, theme::alpha(0x73000000, inn));
        if self.drag.is_some() || self.sizing.is_some() {
            let step = 20.0;
            let mut gx = step;
            while gx < w_all {
                c.fill(gx, 0.0, 1.0, h_all, 0x0CFFFFFF);
                gx += step;
            }
            let mut gy = step;
            while gy < h_all {
                c.fill(0.0, gy, w_all, 1.0, 0x0CFFFFFF);
                gy += step;
            }
            c.fill(w_all / 2.0, 0.0, 1.0, h_all, 0x1AFFFFFF);
            c.fill(0.0, h_all / 2.0, w_all, 1.0, 0x1AFFFFFF);
        }

        // Where the hub and the drawer sit (they block the modules under them).
        let (hw, hh) = (280.0, 172.0);
        self.hub_rect = [(w_all - hw) / 2.0, (h_all - hh) / 2.0 - 18.0 + (1.0 - inn) * 14.0, hw, hh];
        let dt = ui.anim_on("hud:drawer", self.drawer_open, 14.0);
        let dw = (w_all - 32.0).min(300.0f32.max(370.0f32.min(w_all * 0.36)));
        if dt > 0.01 {
            let dh = h_all - 32.0;
            if self.drawer_moved {
                self.drawer_pos[0] = self.drawer_pos[0].clamp(0.0, (w_all - dw).max(0.0));
                self.drawer_pos[1] = self.drawer_pos[1].clamp(0.0, (h_all - dh).max(0.0));
            }
            let base_x = if self.drawer_moved { self.drawer_pos[0] } else { w_all - dw - 16.0 };
            self.drawer_rect = [
                base_x + (1.0 - dt) * (dw + 30.0),
                if self.drawer_moved { self.drawer_pos[1] } else { 16.0 },
                dw,
                dh,
            ];
        } else {
            self.drawer_rect[2] = 0.0;
        }
        let hr = self.hub_rect;
        let over_hub = ui.hover(c, hr[0] - 10.0, hr[1] - 10.0, hr[2] + 20.0, hr[3] + 20.0);
        let over_drawer = Self::inside(&self.drawer_rect, c, ui);
        let over_notice = self.notice.is_some() && Self::inside(&self.notice_rect, c, ui);

        // Modules and their boxes.
        self.shown.clear();
        self.rects.clear();
        self.tags.clear();
        for mi in 0..mods.all.len() {
            if !mods.all[mi].state.enabled || mods.all[mi].state.hud.is_none() {
                continue;
            }
            let g = Self::game_for(mods, mi, live, &sample, in_world);
            let sz = mods.all[mi].measure(c, g);
            let hud = mods.all[mi].state.hud.as_ref().unwrap();
            self.shown.push(mi);
            self.rects.push([Hud::pos(hud.fx, w_all, sz[0]), Hud::pos(hud.fy, h_all, sz[1]), sz[0], sz[1]]);
        }
        for i in 0..self.shown.len() {
            let r = self.rects[i];
            let name = mods.all[self.shown[i]].state.name;
            let lw = c.text_width(font::SEMIBOLD, 10.5, name) + 16.0;
            let row_w = lw + 4.0 + 18.0 + 4.0 + 18.0;
            let lx = (r[0] - 3.0).min(w_all - row_w - 2.0).max(2.0);
            let ly = if r[1] - 3.0 - 22.0 < 2.0 { r[1] + r[3] + 7.0 } else { r[1] - 3.0 - 22.0 };
            self.tags.push([lx, ly, lw, lx + lw + 4.0, lx + lw + 26.0]);
        }
        let over_pop = self.popover.is_some() && ui.hover(c, self.pop[0], self.pop[1], self.pop[2], self.pop[3]);
        let blocked = over_hub || over_drawer || over_notice || over_pop;

        // The gear / minus badges first: they sit over the tag row, outside the box.
        let mut tag_hover: Option<usize> = None;
        if !blocked && self.drag.is_none() && self.sizing.is_none() {
            for i in (0..self.shown.len()).rev() {
                let t = self.tags[i];
                if ui.hover(c, t[0], t[1], t[4] + 18.0 - t[0], 18.0) {
                    tag_hover = Some(i);
                    ui.cursor_hand = true;
                    let mi = self.shown[i];
                    if ui.pressed && ui.hover(c, t[3], t[1], 18.0, 18.0) {
                        // The gear goes straight to this module's page in the Native menu (right-click keeps the quick card).
                        mods.save();
                        action = Action::OpenMod(mods.all[mi].state.id.to_owned());
                        ui.pressed = false;
                    } else if ui.pressed && ui.hover(c, t[4], t[1], 18.0, 18.0) {
                        mods.all[mi].set_enabled(false);
                        mods.changed();
                        let name = mods.all[mi].state.name;
                        self.say(ui, format!("{name} hidden"), Some(mi));
                        if self.popover == Some(mi) {
                            self.popover = None;
                        }
                        ui.pressed = false;
                    }
                    break;
                }
            }
        }

        // Pick: a resize corner of the module under the cursor first.
        let mut hover: Option<usize> = None; // an index into `shown`
        let mut handle: Option<(usize, usize)> = None;
        if !blocked && self.drag.is_none() && self.sizing.is_none() && tag_hover.is_none() {
            'outer: for i in (0..self.shown.len()).rev() {
                let r = self.rects[i];
                for k in 0..4 {
                    let (hx, hy) = (corner_x(&r, k), corner_y(&r, k));
                    if ui.hover(c, hx - 8.0, hy - 8.0, 16.0, 16.0) {
                        handle = Some((i, k));
                        hover = Some(i);
                        break 'outer;
                    }
                }
            }
        }
        if let Some((i, k)) = handle
            && ui.pressed
        {
            let r = self.rects[i];
            let mi = self.shown[i];
            self.sizing = Some(mi);
            self.size_corner = k;
            self.size_start = mods.all[mi].state.hud.as_ref().map_or(1.0, Hud::scale);
            self.start_size = [r[2].max(4.0), r[3].max(4.0)];
            // The opposite corner stays where it is while this one is dragged.
            self.anchor = [
                if k == 0 || k == 3 { r[0] + r[2] } else { r[0] },
                if k == 0 || k == 1 { r[1] + r[3] } else { r[1] },
            ];
            self.popover = None;
            hover = None;
        }
        if let Some(mi) = self.sizing {
            match self.shown.iter().position(|s| *s == mi) {
                Some(si) if ui.down => {
                    let left = self.size_corner == 0 || self.size_corner == 3;
                    let up = self.size_corner == 0 || self.size_corner == 1;
                    let dx = (if left { self.anchor[0] - ui.mx } else { ui.mx - self.anchor[0] }).max(0.0);
                    let dy = (if up { self.anchor[1] - ui.my } else { ui.my - self.anchor[1] }).max(0.0);
                    // How far along the box's own diagonal the cursor is: grows evenly whichever way you pull.
                    let [sw, sh] = self.start_size;
                    let ratio = (dx * sw + dy * sh) / (sw * sw + sh * sh);
                    let v = (self.size_start * ratio / 0.01).round() * 0.01;
                    if let Some(s) = mods.all[mi].state.hud.as_mut().and_then(|h| h.get_mut(SCALE))
                        && (v - s.as_f32()).abs() > 0.001
                    {
                        s.set_num(v);
                    }
                    let g = Self::game_for(mods, mi, live, &sample, in_world);
                    let sz = mods.all[mi].measure(c, g);
                    let (nw, nh) = (sz[0], sz[1]);
                    let mut nx = if left { self.anchor[0] - nw } else { self.anchor[0] };
                    let mut ny = if up { self.anchor[1] - nh } else { self.anchor[1] };
                    nx = nx.clamp(0.0, (w_all - nw).max(0.0));
                    ny = ny.clamp(0.0, (h_all - nh).max(0.0));
                    if let Some(hud) = mods.all[mi].state.hud.as_mut() {
                        hud.fx = Hud::frac(nx, w_all, nw);
                        hud.fy = Hud::frac(ny, h_all, nh);
                        self.rects[si] = [Hud::pos(hud.fx, w_all, nw), Hud::pos(hud.fy, h_all, nh), nw, nh];
                    }
                    ui.cursor_hand = true;
                }
                _ => {
                    self.sizing = None;
                    mods.changed();
                }
            }
        }
        if !blocked && self.drag.is_none() && self.sizing.is_none() && hover.is_none() && tag_hover.is_none() {
            for i in (0..self.shown.len()).rev() {
                let r = self.rects[i];
                if ui.hover(c, r[0] - 3.0, r[1] - 3.0, r[2] + 6.0, r[3] + 6.0) {
                    hover = Some(i);
                    break;
                }
            }
        }
        if let Some(i) = hover {
            ui.cursor_hand = true;
            let mi = self.shown[i];
            if ui.pressed {
                self.drag = Some(mi);
                let r = self.rects[i];
                self.press = [ui.mx, ui.my];
                self.grab = [ui.mx - r[0], ui.my - r[1]];
                self.popover = None;
            } else if ui.right_pressed {
                self.popover = if self.popover == Some(mi) { None } else { Some(mi) };
            } else if ui.scroll != 0.0 {
                if let Some(s) = mods.all[mi].state.hud.as_mut().and_then(|h| h.get_mut(SCALE)) {
                    let v = s.as_f32() + if ui.scroll > 0.0 { 0.05 } else { -0.05 };
                    s.set_num(v);
                }
                mods.changed();
                ui.scroll = 0.0;
            }
        } else if ui.pressed && !blocked && tag_hover.is_none() {
            self.popover = None;
        }
        self.guide_x = None;
        self.guide_y = None;
        if let Some(mi) = self.drag {
            match self.shown.iter().position(|s| *s == mi) {
                Some(di) if ui.down => {
                    let r = self.rects[di];
                    let (nx, ny) = (ui.mx - self.grab[0], ui.my - self.grab[1]);
                    let (sx, gx) = self.snap(nx, r[2], w_all, di, true);
                    let (sy, gy) = self.snap(ny, r[3], h_all, di, false);
                    self.guide_x = gx;
                    self.guide_y = gy;
                    let nx = sx.clamp(0.0, (w_all - r[2]).max(0.0));
                    let ny = sy.clamp(0.0, (h_all - r[3]).max(0.0));
                    if let Some(hud) = mods.all[mi].state.hud.as_mut() {
                        hud.fx = Hud::frac(nx, w_all, r[2]);
                        hud.fy = Hud::frac(ny, h_all, r[3]);
                        self.rects[di][0] = Hud::pos(hud.fx, w_all, r[2]);
                        self.rects[di][1] = Hud::pos(hud.fy, h_all, r[3]);
                    }
                    let r = self.rects[di];
                    let t = &mut self.tags[di];
                    let lw = t[2];
                    let row_w = lw + 4.0 + 18.0 + 4.0 + 18.0;
                    t[0] = (r[0] - 3.0).min(w_all - row_w - 2.0).max(2.0);
                    t[1] = if r[1] - 3.0 - 22.0 < 2.0 { r[1] + r[3] + 7.0 } else { r[1] - 3.0 - 22.0 };
                    t[3] = t[0] + lw + 4.0;
                    t[4] = t[0] + lw + 26.0;
                }
                _ => {
                    self.drag = None;
                    mods.changed();
                }
            }
        }
        if let Some(gx) = self.guide_x {
            c.fill(gx, 0.0, 1.0, h_all, theme::alpha(theme::ACCENT, 0.8));
        }
        if let Some(gy) = self.guide_y {
            c.fill(0.0, gy, w_all, 1.0, theme::alpha(theme::ACCENT, 0.8));
        }
        for i in 0..self.shown.len() {
            let mi = self.shown[i];
            let r = self.rects[i];
            let t = self.tags[i];
            let g = Self::game_for(mods, mi, live, &sample, in_world);
            let active = hover == Some(i) || self.drag == Some(mi) || self.popover == Some(mi) || self.sizing == Some(mi) || tag_hover == Some(i);
            let (id, name) = (mods.all[mi].state.id, mods.all[mi].state.name);
            let a = ui.anim_on(&format!("hud:{id}#h"), active, 16.0);
            c.round(r[0] - 3.0, r[1] - 3.0, r[2] + 6.0, r[3] + 6.0, 6.0, theme::alpha(0x14FFFFFF, a));
            mods.all[mi].paint(c, r[0], r[1], g);
            c.outline(r[0] - 3.0, r[1] - 3.0, r[2] + 6.0, r[3] + 6.0, 6.0, 1.0, theme::mix(0x59FFFFFF, 0xE6FFFFFF, a));
            // Name tag with the gear and minus badges.
            c.round(t[0], t[1], t[2], 18.0, 9.0, theme::mix(0xE60E0F13, 0xF2181A20, a));
            c.outline(t[0], t[1], t[2], 18.0, 9.0, 1.0, theme::alpha(0xFFFFFFFF, 0.1 + 0.2 * a));
            let lh = c.line_height(font::SEMIBOLD, 10.5);
            c.text(font::SEMIBOLD, 10.5, name, t[0] + 8.0, t[1] + (18.0 - lh) / 2.0, theme::mix(0xFFD4D4D8, 0xFFFFFFFF, a));
            badge(ui, c, &format!("hud:gear:{id}"), t[3], t[1], BLUE, theme::I_SETTINGS, self.popover == Some(mi));
            badge(ui, c, &format!("hud:minus:{id}"), t[4], t[1], RED, theme::I_MINUS, false);
            if a > 0.3 {
                // Resize handles on all four corners: drag one, the opposite corner stays put.
                for k in 0..4 {
                    let (hx, hy) = (corner_x(&r, k), corner_y(&r, k));
                    let on = self.sizing == Some(mi) && k == self.size_corner;
                    let near = ui.hover(c, hx - 8.0, hy - 8.0, 16.0, 16.0);
                    let hs = if on || near { 9.0 } else { 7.0 };
                    c.fill(hx - hs / 2.0, hy - hs / 2.0, hs, hs, theme::alpha(0xFFFFFFFF, a));
                    c.fill(
                        hx - hs / 2.0 + 1.5,
                        hy - hs / 2.0 + 1.5,
                        hs - 3.0,
                        hs - 3.0,
                        theme::alpha(if on { theme::ACCENT } else { 0xFF09090B }, a),
                    );
                }
                if hover == Some(i) || self.drag == Some(mi) || self.sizing == Some(mi) {
                    let scale = mods.all[mi].state.hud.as_ref().map_or(1.0, Hud::scale);
                    let size = format!("{}%", (scale * 100.0).round() as i32);
                    let sw = c.text_width(font::MEDIUM, 10.0, &size) + 12.0;
                    let sx = r[0] + r[2] + 3.0 - sw;
                    let mut sy = r[1] + r[3] + 6.0;
                    if sy + 16.0 > h_all {
                        sy = r[1] - 22.0;
                    }
                    if t[1] > r[1] && sy < r[1] {
                        sy = r[1] + r[3] + 28.0;
                    }
                    c.round(sx, sy, sw, 16.0, 8.0, 0xE60E0F13);
                    let sh = c.line_height(font::MEDIUM, 10.0);
                    c.text(font::MEDIUM, 10.0, &size, sx + 6.0, sy + (16.0 - sh) / 2.0, theme::TEXT_SECONDARY);
                }
            }
        }

        let under = ui.interactive;
        ui.interactive = under && !over_pop && !over_notice && !over_drawer; // The settings card sits on top of the hub and the drawer.
        let hub_action = self.hub(ui, c, mods, inn, self.shown.is_empty());
        ui.interactive = under;
        if hub_action != Action::None {
            return hub_action;
        }
        ui.interactive = under && !over_pop && !over_notice;
        if dt > 0.01 {
            self.drawer(ui, c, mods, dt, w_all, h_all);
        }
        ui.interactive = under && !over_pop;
        self.notice(ui, c, mods);
        ui.interactive = under;
        if let Some(pm) = self.popover {
            if self.shown.contains(&pm) {
                let si = self.shown.iter().position(|s| *s == pm).unwrap_or(0);
                let r = self.rects[si];
                self.draw_popover(ui, c, mods, pm, r, w_all, h_all);
            } else {
                self.popover = None;
            }
        }
        action
    }

    // ── hub ──────────────────────────────────────────────────────────────

    /// The middle of the screen: the Native mark, the way into the settings and a row of shortcuts.
    fn hub(&mut self, ui: &mut Ui, c: &mut Canvas, mods: &mut Modules, a: f32, empty: bool) -> Action {
        let w_all = c.width();
        let (_, y0) = (self.hub_rect[0], self.hub_rect[1]);
        if let Some(logo) = self.logo.clone() {
            c.stamp(LOGO_KEY, &logo, w_all / 2.0 - 28.0, y0, 56.0, 56.0, true, theme::alpha(0xFFFFFFFF, a));
        }
        // The big button.
        let (bw, bh) = (236.0, 42.0);
        let (bx, by) = (w_all / 2.0 - bw / 2.0, y0 + 70.0);
        let over = ui.hover(c, bx, by, bw, bh);
        let click = ui.clicked(c, "hud:hub:mods", bx, by, bw, bh);
        let hv = ui.anim_on("hud:hub:mods#h", over, 16.0);
        let pressing = ui.is_pressing("hud:hub:mods");
        let pr = ui.anim_on("hud:hub:mods#p", pressing, 24.0);
        c.round(
            bx,
            by,
            bw,
            bh,
            bh / 2.0,
            theme::alpha(theme::mix(theme::mix(0xFFF4F4F5, 0xFFFFFFFF, hv), 0xFFD4D4D8, pr), a),
        );
        let label = "NATIVE MODS";
        let fs = 12.5;
        let tw = c.text_width(font::SEMIBOLD, fs, label);
        let tx = bx + (bw - tw - 24.0) / 2.0;
        c.icon(theme::I_NAV_MODS, 15.0, tx + 7.0, by + bh / 2.0, theme::alpha(theme::SOLID_FG, a));
        let lh = c.line_height(font::SEMIBOLD, fs);
        c.text(font::SEMIBOLD, fs, label, tx + 24.0, by + (bh - lh) / 2.0, theme::alpha(theme::SOLID_FG, a));
        if over {
            ui.cursor_hand = true;
        }
        if click {
            return Action::OpenMenu(TAB_MODS);
        }
        // Round shortcuts.
        let tile_icons = [theme::I_PLUS, theme::I_SETTINGS, theme::I_SHIRT, theme::I_MESSAGE, icons::MUSIC, theme::I_RESET];
        let tips = ["HUD modules", "Settings", "Cosmetics", "Relay chat", "Music", "Reset layout"];
        let (d, gap) = (40.0, 8.0);
        let row_w = tile_icons.len() as f32 * d + (tile_icons.len() - 1) as f32 * gap;
        let (rx, ry) = (w_all / 2.0 - row_w / 2.0, by + bh + 16.0);
        for i in 0..tile_icons.len() {
            let cx = rx + i as f32 * (d + gap);
            let on = i == 0 && self.drawer_open;
            let hit = self.round(ui, c, &format!("hud:hub:{i}"), cx, ry, d, tile_icons[i], tips[i], on, a);
            if hit {
                match i {
                    0 => self.drawer_open = !self.drawer_open,
                    1 => return Action::OpenMenu(TAB_SETTINGS),
                    2 => return Action::OpenMenu(TAB_COSMETICS),
                    3 => return Action::OpenRelay,
                    4 => return Action::OpenMenu(TAB_MUSIC),
                    _ => {
                        mods.reset_positions();
                        self.say(ui, "Layout reset".to_owned(), None);
                    }
                }
            }
        }
        if empty {
            let t = "No HUD modules on screen";
            let b = "Press + to add some.";
            let ty = ry + d + 22.0;
            let tw = c.text_width(font::SEMIBOLD, 13.0, t);
            c.text(font::SEMIBOLD, 13.0, t, w_all / 2.0 - tw / 2.0, ty, theme::alpha(theme::TEXT_STRONG, a));
            let bw = c.text_width(font::REGULAR, 11.5, b);
            c.text(font::REGULAR, 11.5, b, w_all / 2.0 - bw / 2.0, ty + 20.0, theme::alpha(theme::TEXT_SECONDARY, a));
        }
        Action::None
    }

    #[allow(clippy::too_many_arguments)]
    fn round(&mut self, ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, d: f32, icon: u32, tip: &str, on: bool, alpha: f32) -> bool {
        let over = ui.hover(c, x, y, d, d);
        let click = ui.clicked(c, id, x, y, d, d);
        let hv = ui.anim_on(&format!("{id}#h"), over, 16.0);
        let pressing = ui.is_pressing(id);
        let pr = ui.anim_on(&format!("{id}#p"), pressing, 24.0);
        let act = ui.anim_on(&format!("{id}#on"), on, 14.0);
        // Lifts slightly on hover, sinks on press, turns solid while active.
        let r = 11.0;
        let lift = hv * 1.5 - pr;
        let ty = y - lift;
        self.tile_lift = lift;
        // Flat tile: one solid fill and a hairline, no shadow or sheen.
        let bg = theme::mix(theme::mix(0xF20C0D11, 0xFF17191F, hv), 0xFFF4F4F5, act);
        c.round(x, ty, d, d, r, theme::alpha(theme::mix(bg, if act > 0.5 { 0xFFD4D4D8 } else { 0xFF08090C }, pr * 0.6), alpha));
        c.outline(
            x,
            ty,
            d,
            d,
            r,
            1.0,
            theme::alpha(theme::mix(theme::mix(theme::HAIRLINE_STRONG, theme::BORDER_HOVER, hv), 0x00FFFFFF, act), alpha),
        );
        let fg = theme::mix(theme::mix(0xFFD4D4D8, theme::TEXT_STRONG, hv), theme::SOLID_FG, act);
        c.icon(icon, 16.0, x + d / 2.0, ty + d / 2.0, theme::alpha(fg, alpha));
        if over {
            ui.cursor_hand = true;
            ui.tip(tip);
        }
        click
    }

    // ── drawer: every HUD module, switch them on and off ─────────────────

    fn drawer(&mut self, ui: &mut Ui, c: &mut Canvas, mods: &mut Modules, t: f32, w_all: f32, h_all: f32) {
        let [mut x, mut y, w, h] = self.drawer_rect;
        // Grab the header to move the drawer anywhere on the screen.
        let over_head = ui.hover(c, x, y, w - 60.0, 58.0);
        ui.clicked(c, "hud:drawer:grab", x, y, w - 60.0, 58.0);
        if ui.is_pressing("hud:drawer:grab") {
            if ui.pressed || !self.drawer_moved {
                self.grab_d = [ui.mx - x, ui.my - y];
            }
            self.drawer_moved = true;
            self.drawer_pos = [
                (ui.mx - self.grab_d[0]).clamp(0.0, (w_all - w).max(0.0)),
                (ui.my - self.grab_d[1]).clamp(0.0, (h_all - h).max(0.0)),
            ];
            x = self.drawer_pos[0];
            y = self.drawer_pos[1];
            self.drawer_rect[0] = x;
            self.drawer_rect[1] = y;
        }
        if over_head || ui.is_pressing("hud:drawer:grab") {
            ui.cursor_hand = true;
        }
        c.push_alpha(t);
        c.round(x, y, w, h, 14.0, 0xFA0B0C10);
        c.outline(x, y, w, h, 14.0, 1.0, theme::HAIRLINE_STRONG);
        let huds: Vec<usize> = (0..mods.all.len()).filter(|i| mods.all[*i].state.hud.is_some()).collect();
        let on = huds.iter().filter(|i| mods.all[**i].state.enabled).count();
        c.text(font::SEMIBOLD, 14.5, "HUD modules", x + 18.0, y + 16.0, theme::TEXT_STRONG);
        c.text(font::REGULAR, 11.0, &format!("{on} of {} on screen", huds.len()), x + 18.0, y + 36.0, theme::TEXT_MUTED);
        for gx in 0..2 {
            // A grip: the header drags the drawer.
            for gy in 0..3 {
                c.circle(
                    x + w - 18.0 - 26.0 - 18.0 + gx as f32 * 5.0,
                    y + 20.0 + gy as f32 * 5.0,
                    1.1,
                    theme::alpha(0xFFFFFFFF, if over_head { 0.55 } else { 0.3 }),
                );
            }
        }
        if ui.icon_button(c, "hud:drawer:close", x + w - 18.0 - 26.0, y + 14.0, 26.0, theme::I_X, None) {
            self.drawer_open = false;
            c.pop_alpha();
            return;
        }
        // Search and state filters stay fixed while the result list scrolls.
        c.round(x + 14.0, y + 64.0, w - 28.0, 34.0, 8.0, 0xFF101217);
        let focused = self.drawer_search.focused(ui);
        c.outline(x + 14.0, y + 64.0, w - 28.0, 34.0, 8.0, 1.0, if focused { theme::BORDER_HOVER } else { 0x00FFFFFF });
        c.icon(theme::I_SEARCH, 14.0, x + 30.0, y + 81.0, theme::TEXT_MUTED);
        if ui.pressed && ui.hover(c, x + 14.0, y + 64.0, w - 60.0, 34.0) {
            self.drawer_search.focus(ui);
            ui.focus_claimed = true;
        }
        self.drawer_search.draw(ui, c, x + 34.0, y + 64.0, w - 82.0, 34.0, "Find a HUD module", 12.0);
        if !self.drawer_search.is_empty() && ui.icon_button(c, "hud:drawer:clear", x + w - 42.0, y + 69.0, 24.0, theme::I_X, None) {
            self.drawer_search.clear();
            self.drawer_search.focus(ui);
            ui.focus_claimed = true;
        }
        let filters = ["All", "On", "Off"];
        let fw = (w - 36.0) / 3.0;
        for (i, label) in filters.iter().enumerate() {
            let fx = x + 14.0 + i as f32 * (fw + 4.0);
            let selected = self.drawer_filter == i;
            if ui.clicked(c, &format!("hud:drawer:filter:{i}"), fx, y + 108.0, fw, 28.0) {
                self.drawer_filter = i;
            }
            if selected {
                c.round(fx, y + 108.0, fw, 28.0, 7.0, 0xFFE4E4E7);
            }
            let tw = c.text_width(font::MEDIUM, 11.0, label);
            let lh = c.line_height(font::MEDIUM, 11.0);
            c.text(
                font::MEDIUM,
                11.0,
                label,
                fx + (fw - tw) / 2.0,
                y + 108.0 + (28.0 - lh) / 2.0,
                if selected { theme::SOLID_FG } else { theme::TEXT_SECONDARY },
            );
        }
        let q = self.drawer_search.value();
        let results: Vec<usize> = huds
            .iter()
            .copied()
            .filter(|i| {
                let m = &mods.all[*i];
                !(self.drawer_filter == 1 && !m.state.enabled || self.drawer_filter == 2 && m.state.enabled) && menu::Menu::query_matches(m, &q)
            })
            .collect();
        let mut key = format!("{}:{}", menu::normalize(&q), self.drawer_filter);
        for i in &results {
            key.push(':');
            key.push_str(mods.all[*i].state.id);
        }
        if key != self.drawer_key {
            self.drawer_key = key;
            self.drawer_scroll = 0.0;
        }
        let ly = y + 150.0;
        let lh = (h - 188.0).max(0.0);
        let row_h = 60.0;
        let total = results.len() as f32 * row_h;
        let max_scroll = (total - lh).max(0.0);
        if ui.hover(c, x, ly, w, lh) && ui.scroll != 0.0 {
            self.drawer_scroll -= ui.scroll * 40.0;
            ui.scroll = 0.0;
        }
        self.drawer_scroll = self.drawer_scroll.clamp(0.0, max_scroll);
        c.push_clip(x + 8.0, ly, w - 16.0, lh);
        let mut ry = ly - self.drawer_scroll;
        for mi in &results {
            let mi = *mi;
            if ry + row_h > ly && ry < ly + lh {
                let id = format!("hud:drawer:row:{}", mods.all[mi].state.id);
                let over = ui.hover(c, x + 12.0, ry, w - 24.0, row_h - 6.0);
                let click = ui.clicked(c, &id, x + 12.0, ry, w - 76.0, row_h - 6.0);
                let hv = ui.anim_on(&format!("{id}#h"), over, 16.0);
                if hv > 0.01 {
                    c.round(x + 12.0, ry, w - 24.0, row_h - 6.0, 10.0, theme::alpha(0xFFFFFFFF, 0.05 * hv));
                }
                let m = &mods.all[mi];
                let enabled = m.state.enabled;
                c.icon(m.state.icon, 18.0, x + 39.0, ry + 27.0, if enabled { theme::TEXT_STRONG } else { theme::TEXT_MUTED });
                let name = c.ellipsize(font::SEMIBOLD, 12.0, m.state.name, w - 132.0);
                c.text(font::SEMIBOLD, 12.0, &name, x + 66.0, ry + 8.0, theme::TEXT_STRONG);
                c.text(font::REGULAR, 10.0, if enabled { "On screen" } else { "Hidden" }, x + 66.0, ry + 29.0, theme::TEXT_MUTED);
                let toggle = ui.toggle(c, &format!("hud:drawer:tg:{}", mods.all[mi].state.id), x + w - 58.0, ry + 17.0, enabled);
                if click || toggle {
                    mods.all[mi].set_enabled(!enabled);
                    mods.changed();
                    if enabled {
                        let name = mods.all[mi].state.name;
                        self.say(ui, format!("{name} hidden"), Some(mi));
                        if self.popover == Some(mi) {
                            self.popover = None;
                        }
                    }
                }
            }
            ry += row_h;
        }
        if results.is_empty() {
            c.text(font::SEMIBOLD, 12.0, "No matching modules", x + 24.0, ly + 24.0, theme::TEXT_SECONDARY);
            c.text(font::REGULAR, 11.0, "Try another search or filter.", x + 24.0, ly + 46.0, theme::TEXT_MUTED);
        }
        c.pop_clip();
        if max_scroll > 0.0 && lh > 0.0 {
            let thumb = (lh * lh / total).max(24.0);
            c.round(x + w - 8.0, ly + (lh - thumb) * self.drawer_scroll / max_scroll, 3.0, thumb, 1.5, theme::BORDER_HOVER);
        }
        c.fill(x + 14.0, y + h - 32.0, w - 28.0, 1.0, theme::HAIRLINE);
        c.text(
            font::REGULAR,
            10.0,
            &format!("{} modules  \u{b7}  Drag the header to move", results.len()),
            x + 18.0,
            y + h - 24.0,
            theme::TEXT_MUTED,
        );
        c.pop_alpha();
    }

    // ── notices ──────────────────────────────────────────────────────────

    fn notice(&mut self, ui: &mut Ui, c: &mut Canvas, mods: &mut Modules) {
        self.notice_rect[2] = 0.0;
        let Some(text) = self.notice.clone() else { return };
        let undo = self.removed.is_some();
        let (w_all, h_all) = (c.width(), c.height());
        let w = 340.0f32.min(w_all - 32.0);
        let h = 48.0;
        let (x, mut y) = ((w_all - w) / 2.0, h_all - h - 24.0);
        // Do not let the action disappear while the pointer is over it.
        if ui.hover(c, x, y, w, h) {
            self.notice_at += (ui.now - self.notice_frame_at).max(0.0);
        }
        self.notice_frame_at = ui.now;
        let age = ((ui.now - self.notice_at) / 1000.0) as f32;
        if age >= 6.0 {
            self.notice = None;
            self.removed = None;
            return;
        }
        let t = (age * 8.0).min(1.0) * ((6.0 - age) * 4.0).min(1.0);
        y += (1.0 - t) * 8.0;
        self.notice_rect = [x, y, w, h];
        c.push_alpha(t);
        c.round(x, y, w, h, 12.0, 0xFA0B0C10);
        c.outline(x, y, w, h, 12.0, 1.0, theme::HAIRLINE_STRONG);
        let text_w = w - if undo { 130.0 } else { 60.0 };
        let shown = c.ellipsize(font::MEDIUM, 12.0, &text, text_w);
        let lh = c.line_height(font::MEDIUM, 12.0);
        c.text(font::MEDIUM, 12.0, &shown, x + 16.0, y + (h - lh) / 2.0, theme::TEXT_STRONG);
        if let Some(mi) = self.removed {
            let (ux, uy) = (x + w - 92.0, y + 9.0);
            let over = ui.hover(c, ux, uy, 56.0, 30.0);
            let click = ui.clicked(c, "hud:notice:undo", ux, uy, 56.0, 30.0);
            let tw = c.text_width(font::SEMIBOLD, 12.0, "Undo");
            let th = c.line_height(font::SEMIBOLD, 12.0);
            c.text(
                font::SEMIBOLD,
                12.0,
                "Undo",
                ux + (56.0 - tw) / 2.0,
                uy + (30.0 - th) / 2.0,
                if over { theme::TEXT_STRONG } else { theme::TEXT_SECONDARY },
            );
            if over {
                ui.cursor_hand = true;
            }
            if click {
                mods.all[mi].set_enabled(true);
                mods.changed();
                self.notice = None;
                self.removed = None;
            }
        }
        if ui.icon_button(c, "hud:notice:close", x + w - 34.0, y + 13.0, 22.0, theme::I_X, None) {
            self.notice = None;
            self.removed = None;
        }
        c.pop_alpha();
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_popover(&mut self, ui: &mut Ui, c: &mut Canvas, mods: &mut Modules, mi: usize, r: [f32; 4], w_all: f32, h_all: f32) {
        let mut rows: Vec<(Part, usize)> = vec![(Part::Keybind, 0)];
        rows.extend(mods.all[mi].shown_settings().into_iter().map(|i| (Part::Options, i)));
        rows.extend(mods.all[mi].shown_appearance().into_iter().map(|i| (Part::Appearance, i)));
        let (w, pad) = (320.0, 14.0);
        let mut h = pad + 30.0 + ROW + pad;
        for (part, i) in &rows {
            h += Widgets::row_height(setting_ref(&mods.all[mi], *part, *i));
        }
        let h = h.min(h_all - 20.0);
        let mut x = r[0] + r[2] + 12.0;
        if x + w > w_all - 8.0 {
            x = r[0] - w - 12.0;
        }
        if x < 8.0 {
            x = r[0].min(w_all - w - 8.0).max(8.0);
        }
        let y = r[1].min(h_all - h - 8.0).max(8.0);
        self.pop = [x, y, w, h];
        c.shadow(x, y + 3.0, w, h, 12.0, 18.0, 0x88000000);
        c.round(x, y, w, h, 12.0, 0xF808090C);
        c.outline(x, y, w, h, 12.0, 1.0, theme::HAIRLINE_STRONG);
        let (id, name, icon, enabled) = {
            let s = &mods.all[mi].state;
            (s.id, s.name, s.icon, s.enabled)
        };
        c.icon(icon, 14.0, x + pad + 8.0, y + pad + 10.0, theme::TEXT_STRONG);
        c.text(font::SEMIBOLD, 13.5, name, x + pad + 24.0, y + pad + 1.0, theme::TEXT_STRONG);
        if ui.icon_button(c, "hud:pop:close", x + w - pad - 24.0, y + pad - 2.0, 24.0, theme::I_X, None) {
            self.popover = None;
            return;
        }
        let mut ry = y + pad + 30.0;
        c.push_clip(x, ry, w, y + h - pad - ry + 4.0);
        let rw = w - pad * 2.0;
        let lh = c.line_height(font::MEDIUM, 12.5);
        c.text(font::MEDIUM, 12.5, "Enabled", x + pad, ry + (ROW - lh) / 2.0, theme::TEXT);
        if ui.toggle(c, "hud:pop:on", x + pad + rw - 34.0, ry + 10.0, enabled) {
            mods.all[mi].set_enabled(false);
            mods.changed();
            self.popover = None;
            c.pop_clip();
            return;
        }
        ry += ROW;
        for (part, i) in rows {
            c.fill(x + pad, ry, rw, 1.0, theme::HAIRLINE);
            let warning = if part == Part::Keybind { menu::conflict(mods, mi) } else { None };
            let m = &mut mods.all[mi];
            let s: &mut Setting = match part {
                Part::Keybind => &mut m.state.toggle_key,
                Part::Options => &mut m.state.settings[i],
                Part::Appearance => match m.state.hud.as_mut() {
                    Some(hud) => &mut hud.appearance[i],
                    None => continue,
                },
            };
            let sid = s.id;
            let rh = Widgets::row_height(s);
            if self.widgets.row(ui, c, &format!("hud:pop:{id}:{sid}"), x + pad, ry, rw, s, warning.as_deref()) {
                mods.changed();
            }
            ry += rh;
        }
        c.pop_clip();
    }

    /// Snaps one axis to the screen edges / centre / other modules. Returns the position and the guide line.
    fn snap(&self, p: f32, size: f32, screen: f32, me: usize, x_axis: bool) -> (f32, Option<f32>) {
        let mut best = SNAP + 1.0;
        let mut out = p;
        let mut guide = None;
        let m = MARGIN;
        let cands = [[m, m], [screen - size - m, screen - m], [screen / 2.0 - size / 2.0, screen / 2.0]];
        for cnd in cands {
            let d = (p - cnd[0]).abs();
            if d < best {
                best = d;
                out = cnd[0];
                guide = Some(cnd[1]);
            }
        }
        for (i, r) in self.rects.iter().enumerate() {
            if i == me {
                continue;
            }
            let (o0, os) = if x_axis { (r[0], r[2]) } else { (r[1], r[3]) };
            let edges = [
                [o0, o0],
                [o0 + os - size, o0 + os],
                [o0 + os + 4.0, o0 + os + 2.0],
                [o0 - size - 4.0, o0 - 2.0],
                [o0 + os / 2.0 - size / 2.0, o0 + os / 2.0],
            ];
            for e in edges {
                let d = (p - e[0]).abs();
                if d < best {
                    best = d;
                    out = e[0];
                    guide = Some(e[1]);
                }
            }
        }
        (out, if best <= SNAP { guide } else { None })
    }
}

fn setting_ref(m: &super::module::Module, part: Part, i: usize) -> &Setting {
    match part {
        Part::Keybind => &m.state.toggle_key,
        Part::Options => &m.state.settings[i],
        Part::Appearance => &m.state.hud.as_ref().map(|h| &h.appearance[i]).unwrap_or(&m.state.toggle_key),
    }
}

fn corner_x(r: &[f32; 4], k: usize) -> f32 {
    if k == 0 || k == 3 { r[0] - 3.0 } else { r[0] + r[2] + 3.0 }
}

fn corner_y(r: &[f32; 4], k: usize) -> f32 {
    if k == 0 || k == 1 { r[1] - 3.0 } else { r[1] + r[3] + 3.0 }
}

fn badge(ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, color: u32, icon: u32, on: bool) {
    let over = ui.hover(c, x, y, 18.0, 18.0);
    let hv = ui.anim_on(&format!("{id}#h"), over || on, 18.0);
    let k = 18.0 + hv * 2.0;
    let (bx, by) = (x - (k - 18.0) / 2.0, y - (k - 18.0) / 2.0);
    c.round(bx, by, k, k, k / 2.0, theme::mix(color, theme::mix(color, 0xFFFFFFFF, 0.22), hv));
    c.outline(bx, by, k, k, k / 2.0, 1.0, 0x40FFFFFF);
    c.icon(icon, 11.0, x + 9.0, y + 9.0, 0xFFFFFFFF);
    if over {
        ui.tip(if icon == theme::I_SETTINGS { "Settings" } else { "Remove from screen" });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dragging_snaps_to_the_screen_edge_and_other_modules() {
        let mut e = HudEditor::new();
        e.rects = vec![[100.0, 100.0, 50.0, 20.0], [300.0, 300.0, 50.0, 20.0]];
        // Near the left margin.
        let (p, guide) = e.snap(MARGIN + 3.0, 50.0, 1000.0, 0, true);
        assert_eq!(p, MARGIN);
        assert_eq!(guide, Some(MARGIN));
        // Next to the other module's right edge (+ 4 gap).
        let (p, _) = e.snap(356.0, 50.0, 1000.0, 0, true);
        assert_eq!(p, 354.0);
        // Far from everything: nothing snaps.
        let (p, guide) = e.snap(700.0, 50.0, 1000.0, 0, true);
        assert_eq!(p, 700.0);
        assert_eq!(guide, None);
    }

    #[test]
    fn the_editor_draws_the_hub_and_the_modules() {
        let mut c = horizon_client::native_ui::canvas();
        c.begin(1280, 760, 1.0);
        let mut ui = Ui::default();
        ui.begin(&[super::super::ui::Event::Move { x: 640.0, y: 380.0 }], 1.0);
        let mut mods = Modules::new(None);
        let mut e = HudEditor::new();
        e.opened(ui.now);
        let live = GameView::sample();
        e.draw(&mut ui, &mut c, &mut mods, &live, true);
        assert!(!e.shown.is_empty());
        e.drawer_open = true;
        e.popover = Some(e.shown[0]);
        e.draw(&mut ui, &mut c, &mut mods, &live, true);
    }
}
