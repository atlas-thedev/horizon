//! `MenuView`: the Native menu (Right Shift): a floating icon rail on the
//! left, a small toolbar and one big panel with the client mods, the
//! cosmetics locker and the UI settings. Same glass look as the launcher.
#![allow(dead_code)]

use super::module::{GameView, Module};
use super::modules::Modules;
use super::locker::Locker;
use super::screenshots::Screenshots;
use super::scroll::Scroll;
use super::setting::{Kind, Setting};
use super::textfield::TextField;
use super::ui::{self, Ui};
use super::uiconfig::UiConfig;
use super::widgets::Widgets;
use horizon_client::native_ui::{Canvas, Image, assets, font, icons, theme};
use std::collections::HashSet;
use std::sync::Arc;

pub const TABS: [&str; 5] = ["Mods", "Cosmetics", "Settings", "Screenshots", "Music"];
pub const TAB_MODS: usize = 0;
pub const TAB_COSMETICS: usize = 1;
pub const TAB_SETTINGS: usize = 2;
pub const TAB_SHOTS: usize = 3;
pub const TAB_MUSIC: usize = 4;
const RAIL: f32 = 54.0;
/// Corner radius of the floating top bars, squared off a little so square avatars sit well.
const PILL_R: f32 = 13.0;
const CHIPS: [&str; 6] = ["All", "Favorites", "HUD", "Mechanic", "Visual", "Settings"];
const CHIP_ICONS: [u32; 6] = [theme::I_GRID, theme::I_HEART, theme::I_LAYOUT, theme::I_MOUSE, theme::I_EYE, theme::I_SLIDERS];
pub const PANEL_BG: u32 = 0xF205060A;
pub const CARD: u32 = 0xB808090C;
pub const CARD_HOVER: u32 = 0xD80E1014;
/// Card state colours (everything else stays monochrome like the rest of the Native UI).
const ON: u32 = 0xFF22C55E;
const OFF: u32 = 0xFFEF4444;
const LOGO_KEY: u64 = 0x4D45_4E55_4C4F_474F;

/// What the menu asks the game to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Close,
    EditHud,
    /// Open the menu on a tab.
    OpenMenu(usize),
    /// Open a module's own page in the menu.
    OpenMod(String),
    OpenRelay,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Part {
    Keybind,
    Options,
    Appearance,
}

pub struct Menu {
    pub tab: usize,
    chip: usize,
    result_key: String,
    list_view: bool,
    selected: Option<String>,
    search: TextField,
    grid: Scroll,
    detail: Scroll,
    prefs: Scroll,
    pub widgets: Widgets,
    shown_at: f64,
    page_at: f64,
    grid_at: f64,
    first: bool,
    sample: GameView,
    folded: HashSet<String>,
    logo: Option<Arc<Image>>,
    backdrop: Option<Arc<Image>>,
    confirm_reset: bool,
    shots: Screenshots,
    locker: Locker,
    who: String,
    /// The instance folder (screenshots live under it).
    pub game_dir: Option<std::path::PathBuf>,
    s_title: Setting,
    s_notify: Setting,
    s_activity: Setting,
    s_scale: Setting,
    s_menu: Setting,
    s_chat: Setting,
    s_play: Setting,
    s_next: Setting,
    s_prev: Setting,
    scale_dragged: bool,
}

impl Default for Menu {
    fn default() -> Self {
        Self::new()
    }
}

impl Menu {
    /// A menu for the instance in `game_dir`.
    pub fn new_in(game_dir: Option<std::path::PathBuf>) -> Self {
        let mut menu = Self::new();
        menu.game_dir = game_dir;
        menu
    }

    pub fn new() -> Self {
        let mut search = TextField::new("menu:search");
        search.max_length = 64;
        search.bare = true;
        Menu {
            tab: TAB_MODS,
            chip: 0,
            result_key: String::new(),
            list_view: false,
            selected: None,
            search,
            grid: Scroll::new("grid"),
            detail: Scroll::new("detail"),
            prefs: Scroll::new("prefs"),
            widgets: Widgets::default(),
            shown_at: 0.0,
            page_at: 0.0,
            grid_at: 0.0,
            first: true,
            sample: GameView::sample(),
            folded: HashSet::new(),
            logo: Image::decode(assets::LOGO),
            backdrop: Image::decode(assets::HUD_BACKGROUND),
            confirm_reset: false,
            shots: Screenshots::new(),
            locker: Locker::new(),
            who: String::new(),
            game_dir: None,
            s_title: Setting::bool("title", "Native title screen", true),
            s_notify: Setting::bool("notify", "Message notifications in game", true),
            s_activity: Setting::bool("activity", "Show friends what I'm playing", true),
            s_scale: Setting::num("scale", "UI size", 1.0, 0.75, 1.5, 0.05, "x"),
            s_menu: Setting::key("menukey", "Menu key", 344),
            s_chat: Setting::key("chatkey", "Chat key", 89),
            s_play: Setting::key("musicplay", "Music: play / pause", -1),
            s_next: Setting::key("musicnext", "Music: next song", -1),
            s_prev: Setting::key("musicprev", "Music: previous song", -1),
            scale_dragged: false,
        }
    }

    /// Called when the menu opens: the intro animation plays again.
    pub fn opened(&mut self, now: f64) {
        self.shown_at = now;
        self.page_at = now;
        self.grid_at = now;
        self.widgets.listening = None;
        self.confirm_reset = false;
        self.first = false;
    }

    /// Esc: closes the innermost thing first. True when it was consumed (the menu stays open).
    pub fn escape(&mut self, ui: &mut Ui) -> bool {
        if self.widgets.escape() {
            return true;
        }
        if self.search.focused(ui) {
            ui.focus = None;
            if !self.search.is_empty() {
                self.search.clear();
            }
            return true;
        }
        if self.confirm_reset {
            self.confirm_reset = false;
            return true;
        }
        if self.tab == TAB_SHOTS && self.shots.escape() {
            return true;
        }
        if self.selected.is_some() {
            self.selected = None;
            self.page_at = ui.now;
            return true;
        }
        false
    }

    pub fn open_tab(&mut self, t: usize, now: f64) {
        self.set_tab(t, now);
    }

    /// Jumps to a module's own page (settings + live preview) in the mods tab.
    pub fn open_mod(&mut self, id: &str, now: f64) {
        self.set_tab(TAB_MODS, now);
        self.selected = Some(id.to_owned());
        self.detail.reset();
        self.page_at = now;
        self.widgets.listening = None;
    }

    fn set_tab(&mut self, t: usize, now: f64) {
        if self.tab == t {
            return;
        }
        self.tab = t;
        if t == TAB_SHOTS {
            self.shots.stale();
        }
        if t == TAB_COSMETICS {
            self.locker.opened();
        }
        self.selected = None;
        self.widgets.listening = None;
        self.confirm_reset = false;
        self.page_at = now;
        self.grid_at = now;
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(&mut self, ui: &mut Ui, c: &mut Canvas, mods: &mut Modules, cfg: &mut UiConfig, who: Option<&str>, in_world: bool) -> Action {
        let mut action = Action::None;
        self.who = who.unwrap_or("").to_owned();
        let (w_all, h_all) = (c.width(), c.height());
        let e = ease(clamp01(((ui.now - self.shown_at) / 260.0) as f32));
        if in_world {
            c.fill(0.0, 0.0, w_all, h_all, theme::alpha(0xA0000000, e));
        }
        let ww = (w_all - 32.0).min(1060.0);
        let wh = (h_all - 32.0).min(640.0);
        let x0 = (w_all - ww) / 2.0;
        let y0 = (h_all - wh) / 2.0 + (1.0 - e) * 18.0;
        c.push_alpha(e);

        // Icon rail.
        let rw = 72.0;
        panel(c, x0, y0, rw, wh);
        if let Some(logo) = self.logo.clone() {
            c.stamp(LOGO_KEY, &logo, x0 + (rw - 32.0) / 2.0, y0 + 16.0, 32.0, 32.0, true, 0xFFFFFFFF);
        }
        c.fill(x0 + 16.0, y0 + 60.0, rw - 32.0, 1.0, theme::HAIRLINE);
        let rx = x0 + (rw - RAIL) / 2.0;
        let mut ty = y0 + 74.0;
        let tab_icons = [theme::I_NAV_MODS, theme::I_NAV_LOCKER, theme::I_NAV_SETTINGS, icons::CAMERA, icons::MUSIC];
        for (i, label) in TABS.iter().enumerate() {
            if rail_tile(ui, c, &format!("menu:tab:{i}"), rx, ty, tab_icons[i], label, self.tab == i) {
                self.set_tab(i, ui.now);
            }
            ty += RAIL + 8.0;
        }
        if rail_tile(ui, c, "menu:rail:close", rx, y0 + wh - RAIL - 16.0, theme::I_X, "Close", false) {
            c.pop_alpha();
            return Action::Close;
        }

        // Toolbar pill.
        let mx = x0 + rw + 12.0;
        let mw = ww - rw - 12.0;
        let tb_h = 40.0;
        let mods_grid = self.tab == TAB_MODS && self.selected.is_none();
        let (tb_w, tb_action) = self.toolbar(ui, c, mx, y0, tb_h, mods_grid);
        if tb_action == Action::EditHud {
            action = Action::EditHud;
        }
        if let Some(who) = who {
            let hw = c.text_width(font::SEMIBOLD, 12.0, who);
            let av = tb_h - 14.0;
            let pw = hw + av + 30.0;
            let px = mx + mw - pw;
            if px > mx + tb_w + 20.0 {
                c.shadow(px, y0 + 3.0, pw, tb_h, PILL_R, 18.0, 0x66000000);
                c.round(px, y0, pw, tb_h, PILL_R, PANEL_BG);
                c.outline(px, y0, pw, tb_h, PILL_R, 1.0, theme::HAIRLINE_STRONG);
                let (ax, ay) = (px + 7.0, y0 + (tb_h - av) / 2.0);
                avatar(c, who, ax, ay, av, 7.0);
                // Online dot, bottom-right of the head.
                c.circle(ax + av - 3.0, ay + av - 3.0, 5.0, 0xFF05060A);
                c.circle(ax + av - 3.0, ay + av - 3.0, 3.5, theme::ONLINE);
                let lh = c.line_height(font::SEMIBOLD, 12.0);
                c.text(font::SEMIBOLD, 12.0, who, ax + av + 11.0, y0 + (tb_h - lh) / 2.0, theme::TEXT);
            }
        }

        // Main panel.
        let py = y0 + tb_h + 10.0;
        let ph = wh - tb_h - 10.0;
        panel(c, mx, py, mw, ph);
        let hh = 60.0;
        c.fill(mx + 1.0, py + hh, mw - 2.0, 1.0, theme::HAIRLINE);
        let (bx, by, bw, bh) = (mx + 20.0, py + hh + 16.0, mw - 40.0, ph - hh - 32.0);
        let pe = ease(clamp01(((ui.now - self.page_at) / 220.0) as f32));
        match self.tab {
            TAB_MODS => {
                if let Some(id) = self.selected.clone() {
                    c.push_alpha(pe);
                    let a = self.mod_page(ui, c, mods, &id, mx, py, mw, ph);
                    if a != Action::None {
                        action = a;
                    }
                    c.pop_alpha();
                } else {
                    let tx = header(c, "Mods", mx, py, hh);
                    let sw = (mx + mw - 20.0 - tx - 30.0).clamp(160.0, 340.0);
                    self.draw_search(ui, c, mx + mw - 20.0 - sw, py + (hh - 34.0) / 2.0, sw, 34.0);
                    let cw = 184.0f32.min(bw * 0.24);
                    self.categories(ui, c, mods, bx, by, cw, bh);
                    c.fill(bx + cw + 12.0, by, 1.0, bh, theme::HAIRLINE);
                    let (gx, gw) = (bx + cw + 26.0, bw - cw - 26.0);
                    let ge = ease(clamp01(((ui.now - self.grid_at) / 220.0) as f32));
                    c.push_alpha(ge);
                    self.mods_grid(ui, c, mods, gx, by + (1.0 - ge) * 10.0, gw, bh);
                    c.pop_alpha();
                }
            }
            TAB_COSMETICS | TAB_SHOTS | TAB_MUSIC => {
                header(c, TABS[self.tab], mx, py, hh);
                if self.tab == TAB_COSMETICS {
                    let store = "Get more in the Native Store";
                    let sw = c.text_width(font::MEDIUM, 11.5, store);
                    c.icon(theme::I_STORE, 14.0, mx + mw - 20.0 - sw - 12.0, py + hh / 2.0, theme::TEXT_MUTED);
                    let lh = c.line_height(font::MEDIUM, 11.5);
                    c.text(font::MEDIUM, 11.5, store, mx + mw - 20.0 - sw, py + (hh - lh) / 2.0, theme::TEXT_MUTED);
                }
                c.push_alpha(pe);
                let a = self.other_tab(ui, c, bx, by + (1.0 - pe) * 10.0, bw, bh);
                if a != Action::None {
                    action = a;
                }
                c.pop_alpha();
            }
            _ => {
                header(c, "Settings", mx, py, hh);
                c.push_alpha(pe);
                let a = self.settings(ui, c, mods, cfg, who, bx, by + (1.0 - pe) * 10.0, bw, bh);
                if a != Action::None {
                    action = a;
                }
                c.pop_alpha();
            }
        }
        c.pop_alpha();
        action
    }

    /// The tabs another part of the port draws (cosmetics, screenshots, music).
    fn other_tab(&mut self, ui: &mut Ui, c: &mut Canvas, x: f32, y: f32, w: f32, h: f32) -> Action {
        if self.tab == TAB_SHOTS {
            self.shots.draw(ui, c, self.game_dir.as_deref(), x, y, w, h);
            return Action::None;
        }
        if self.tab == TAB_COSMETICS {
            self.locker.draw(ui, c, &self.who, self.page_at, x, y, w, h);
            return Action::None;
        }
        empty(c, x, y, w, h.min(260.0), theme::I_SPARKLES, "Coming soon", "This page is still being ported to the Rust Edition.");
        Action::None
    }

    /// Floating pill above the panel. Returns its width and what was pressed.
    fn toolbar(&mut self, ui: &mut Ui, c: &mut Canvas, x: f32, y: f32, h: f32, grid_tab: bool) -> (f32, Action) {
        let w = if grid_tab { 206.0 } else { 120.0 };
        c.shadow(x, y + 3.0, w, h, PILL_R, 18.0, 0x66000000);
        c.round(x, y, w, h, PILL_R, PANEL_BG);
        c.outline(x, y, w, h, PILL_R, 1.0, theme::HAIRLINE_STRONG);
        let mut action = Action::None;
        if ui.button(c, "menu:tb:hud", x + 4.0, y + 4.0, 112.0, h - 8.0, "Edit HUD", theme::I_MOVE, ui::BTN_GHOST) {
            action = Action::EditHud;
        }
        if grid_tab {
            c.fill(x + 122.0, y + 10.0, 1.0, h - 20.0, theme::HAIRLINE_STRONG);
            if pill_icon(ui, c, "menu:tb:grid", x + 130.0, y + 4.0, h - 8.0, theme::I_GRID, !self.list_view) {
                self.list_view = false;
            }
            if pill_icon(ui, c, "menu:tb:list", x + 130.0 + h - 4.0, y + 4.0, h - 8.0, theme::I_LIST, self.list_view) {
                self.list_view = true;
            }
        }
        (w, action)
    }

    fn draw_search(&mut self, ui: &mut Ui, c: &mut Canvas, x: f32, y: f32, w: f32, h: f32) {
        let f = self.search.focused(ui);
        let fa = ui.anim_on("menu:search#f", f, 14.0);
        c.round(x, y, w, h, h / 2.0, theme::mix(0xB808090C, 0xE00E1014, fa));
        c.outline(x, y, w, h, h / 2.0, 1.0, theme::mix(theme::HAIRLINE_STRONG, theme::BORDER_HOVER, fa));
        c.icon(theme::I_SEARCH, 14.0, x + 18.0, y + h / 2.0, theme::TEXT_MUTED);
        let clear = !self.search.is_empty();
        if ui.pressed && ui.hover(c, x, y, w - if clear { 32.0 } else { 0.0 }, h) {
            self.search.focus(ui);
            ui.focus_claimed = true;
        }
        self.search.draw(ui, c, x + 20.0, y, w - if clear { 54.0 } else { 24.0 }, h, "Search modules", 12.5);
        if clear && ui.icon_button(c, "menu:search:clear", x + w - 30.0, y + (h - 24.0) / 2.0, 24.0, theme::I_X, None) {
            self.search.clear();
            self.search.focus(ui);
            ui.focus_claimed = true;
        }
    }

    fn in_chip(m: &Module, chip: usize) -> bool {
        match chip {
            0 => true,
            1 => m.state.favorite,
            _ => m.state.category == CHIPS[chip],
        }
    }

    /// `ModuleSearch.matches`.
    pub fn query_matches(m: &Module, q: &str) -> bool {
        let q = normalize(q);
        if q.is_empty() {
            return true;
        }
        let hay = format!("{} {} {}", m.state.id, m.state.name, m.state.description).to_lowercase();
        q.split(' ').all(|word| hay.contains(word))
    }

    fn filtered(&self, mods: &Modules) -> Vec<usize> {
        let q = self.search.value();
        mods.all
            .iter()
            .enumerate()
            .filter(|(_, m)| Self::in_chip(m, self.chip) && Self::query_matches(m, &q))
            .map(|(i, _)| i)
            .collect()
    }

    // ── Mods ─────────────────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn mods_grid(&mut self, ui: &mut Ui, c: &mut Canvas, mods: &mut Modules, x: f32, y: f32, w: f32, h: f32) {
        let list = self.filtered(mods);
        let mut key = format!("{}:{}:{}", normalize(&self.search.value()), self.chip, self.list_view);
        for i in &list {
            key.push(':');
            key.push_str(mods.all[*i].state.id);
        }
        if key != self.result_key {
            self.result_key = key;
            self.grid.reset(); // Reset BEFORE drawing, even for an empty result set.
        }
        if list.is_empty() {
            if self.chip == 1 && self.search.value().trim().is_empty() {
                empty(c, x, y, w, h.min(260.0), theme::I_HEART, "No favorites yet", "Tap the heart on a mod to pin it here.");
            } else {
                empty(c, x, y, w, h.min(260.0), theme::I_SEARCH, "No mods found", "Try another search or category.");
            }
            return;
        }
        let gap = 12.0;
        let (cols, card_w, card_h);
        if self.list_view {
            cols = if w > 640.0 { 2 } else { 1 };
            card_w = (w - gap * (cols - 1) as f32) / cols as f32;
            card_h = 60.0;
        } else {
            cols = (((w + gap) / (150.0 + gap)) as usize).max(2);
            card_w = (w - gap * (cols - 1) as f32) / cols as f32;
            card_h = 156.0;
        }
        let off = self.grid.begin(ui, c, x, y, w + 6.0, h);
        let mut opened: Option<String> = None;
        for (n, mi) in list.iter().enumerate() {
            let px = x + (n % cols) as f32 * (card_w + gap);
            let py = y + (n / cols) as f32 * (card_h + gap) - off;
            if py + card_h < y - 4.0 || py > y + h + 4.0 {
                continue;
            }
            let clicked = if self.list_view {
                self.list_row(ui, c, mods, *mi, px, py, card_w, card_h, n)
            } else {
                self.card(ui, c, mods, *mi, px, py, card_w, card_h, n)
            };
            if clicked {
                opened = Some(mods.all[*mi].state.id.to_owned());
            }
        }
        let rows = list.len().div_ceil(cols);
        self.grid.end(ui, c, rows as f32 * (card_h + gap) - gap);
        if let Some(id) = opened {
            self.open_mod(&id, ui.now);
        }
    }

    /// Feather-style tile: icon and name on top, a wide ENABLED / DISABLED bar and a settings gear along the bottom.
    #[allow(clippy::too_many_arguments)]
    fn card(&mut self, ui: &mut Ui, c: &mut Canvas, mods: &mut Modules, mi: usize, x: f32, y: f32, w: f32, h: f32, index: usize) -> bool {
        let m = &mut mods.all[mi];
        let id = format!("menu:card:{}", m.state.id);
        let e = ease(clamp01(((ui.now - self.grid_at - 40.0 - index as f64 * 18.0) / 300.0) as f32));
        c.push_alpha(e);
        let y = y + (1.0 - e) * 12.0;
        let over = ui.hover(c, x, y, w, h);
        let mut click = ui.clicked(c, &id, x, y, w, h - 44.0);
        let hv = ui.anim_on(&format!("{id}#h"), over, 14.0);
        let on = ui.anim_on(&format!("{id}#on"), m.state.enabled, 14.0);
        c.round(x, y, w, h, 12.0, theme::mix(CARD, CARD_HOVER, hv));
        if on > 0.01 {
            glow(c, x, y, w, (h - 44.0) * 0.9, 12.0, theme::alpha(0x14FFFFFF, on));
        }
        c.outline(
            x,
            y,
            w,
            h,
            12.0,
            1.0,
            theme::mix(theme::mix(theme::HAIRLINE, theme::HAIRLINE_STRONG, on), theme::BORDER_HOVER, hv),
        );
        let iy = y + (h - 44.0) * 0.42;
        c.icon(
            m.state.icon,
            30.0,
            x + w / 2.0,
            iy - hv * 2.0,
            theme::mix(theme::mix(0xFF52525B, theme::TEXT_SECONDARY, hv), theme::TEXT_STRONG, on),
        );
        let name = c.ellipsize(font::SEMIBOLD, 12.5, m.state.name, w - 20.0);
        let nw = c.text_width(font::SEMIBOLD, 12.5, &name);
        c.text(font::SEMIBOLD, 12.5, &name, x + (w - nw) / 2.0, y + h - 44.0 - 26.0, theme::mix(theme::TEXT, theme::TEXT_STRONG, on));
        if heart(ui, c, &format!("{id}:fav"), &mut m.state.favorite, x + w - 32.0, y + 8.0, over) {
            click = false;
            mods.changed();
        }
        let m = &mut mods.all[mi];
        let (mut bx, by, mut bw) = (x + 10.0, y + h - 38.0, w - 20.0);
        let has_settings = !m.shown_settings().is_empty() || !m.shown_appearance().is_empty() || m.is_hud();
        if has_settings {
            if ui.icon_button(c, &format!("{id}:gear"), bx, by, 28.0, theme::I_SETTINGS, Some("Settings")) {
                click = true;
            }
            bx += 32.0;
            bw -= 32.0;
        }
        let enabled = m.state.enabled;
        if state_button(ui, c, &format!("{id}:t"), bx, by, bw, 28.0, enabled, 10.5) {
            m.set_enabled(!enabled);
            mods.changed();
        }
        c.pop_alpha();
        click
    }

    #[allow(clippy::too_many_arguments)]
    fn list_row(&mut self, ui: &mut Ui, c: &mut Canvas, mods: &mut Modules, mi: usize, x: f32, y: f32, w: f32, h: f32, index: usize) -> bool {
        let m = &mut mods.all[mi];
        let id = format!("menu:row:{}", m.state.id);
        let e = ease(clamp01(((ui.now - self.grid_at - 40.0 - index as f64 * 18.0) / 280.0) as f32));
        c.push_alpha(e);
        let y = y + (1.0 - e) * 10.0;
        let over = ui.hover(c, x, y, w, h);
        let click = ui.clicked(c, &id, x, y, w - 96.0, h);
        let hv = ui.anim_on(&format!("{id}#h"), over, 14.0);
        let on = ui.anim_on(&format!("{id}#on"), m.state.enabled, 14.0);
        c.round(x, y, w, h, 12.0, theme::mix(CARD, CARD_HOVER, hv));
        c.outline(x, y, w, h, 12.0, 1.0, theme::mix(theme::HAIRLINE, theme::BORDER_HOVER, hv));
        c.round(x + 12.0, y + 12.0, 36.0, 36.0, 10.0, theme::mix(theme::SUBTLE_HOVER, 0xFFF4F4F5, on));
        c.icon(m.state.icon, 17.0, x + 30.0, y + 30.0, theme::mix(theme::TEXT, theme::SOLID_FG, on));
        let tw = w - 160.0;
        let name = c.ellipsize(font::SEMIBOLD, 13.0, m.state.name, tw);
        c.text(font::SEMIBOLD, 13.0, &name, x + 60.0, y + 12.0, theme::TEXT_STRONG);
        let desc = c.ellipsize(font::REGULAR, 11.0, m.state.description, tw);
        c.text(font::REGULAR, 11.0, &desc, x + 60.0, y + 31.0, theme::TEXT_MUTED);
        let mut fav_changed = heart(ui, c, &format!("{id}:fav"), &mut m.state.favorite, x + w - 84.0, y + 18.0, true);
        let enabled = m.state.enabled;
        let toggled = ui.toggle(c, &format!("{id}:t"), x + w - 46.0, y + 20.0, enabled);
        if toggled {
            m.set_enabled(!enabled);
            fav_changed = true;
        }
        if fav_changed {
            mods.changed();
        }
        c.pop_alpha();
        click && !fav_changed
    }

    /// Category list on the left of the mods grid (Feather style): icon, name, count, a marker on the active one.
    #[allow(clippy::too_many_arguments)]
    fn categories(&mut self, ui: &mut Ui, c: &mut Canvas, mods: &Modules, x: f32, y: f32, w: f32, h: f32) {
        c.text(font::SEMIBOLD, 10.0, "CATEGORIES", x + 6.0, y + 2.0, theme::TEXT_MUTED);
        let mut ry = y + 24.0;
        let rh = 38.0;
        for (i, name) in CHIPS.iter().enumerate() {
            let n = mods.all.iter().filter(|m| Self::in_chip(m, i)).count();
            let id = format!("menu:cat:{i}");
            let over = ui.hover(c, x, ry, w, rh);
            if ui.clicked(c, &id, x, ry, w, rh) && self.chip != i {
                self.chip = i;
                self.grid.reset();
                self.page_at = ui.now;
                self.grid_at = ui.now;
            }
            let on = ui.anim_on(&format!("{id}#on"), self.chip == i, 16.0);
            let hv = ui.anim_on(&format!("{id}#h"), over, 14.0);
            c.round(x, ry, w, rh, 10.0, theme::mix(theme::alpha(theme::SUBTLE_HOVER, hv * 0.7), 0x16FFFFFF, on));
            if on > 0.01 {
                c.outline(x, ry, w, rh, 10.0, 1.0, theme::alpha(theme::HAIRLINE_STRONG, on));
                c.round(x - 5.0, ry + (rh - 18.0 * on) / 2.0, 3.0, 18.0 * on, 1.5, theme::alpha(theme::WHITE, on));
            }
            let fg = theme::mix(theme::mix(theme::TEXT_SECONDARY, theme::TEXT_STRONG, hv), theme::TEXT_STRONG, on);
            c.icon(CHIP_ICONS[i], 15.0, x + 22.0, ry + rh / 2.0, fg);
            let lh = c.line_height(font::SEMIBOLD, 12.5);
            c.text(font::SEMIBOLD, 12.5, name, x + 40.0, ry + (rh - lh) / 2.0, fg);
            let count = n.to_string();
            let nw = c.text_width(font::MEDIUM, 10.5, &count);
            c.round(x + w - nw - 22.0, ry + (rh - 20.0) / 2.0, nw + 12.0, 20.0, 10.0, theme::mix(theme::SUBTLE, 0xFFF4F4F5, on));
            let nh = c.line_height(font::MEDIUM, 10.5);
            c.text(font::MEDIUM, 10.5, &count, x + w - nw - 16.0, ry + (rh - nh) / 2.0, theme::mix(theme::TEXT_MUTED, theme::SOLID_FG, on));
            ry += rh + 4.0;
        }
        let active = mods.all.iter().filter(|m| m.state.enabled).count();
        let a = format!("{active} of {} enabled", mods.all.len());
        c.circle(x + 10.0, y + h - 12.0, 3.5, ON);
        let ah = c.line_height(font::MEDIUM, 11.0);
        c.text(font::MEDIUM, 11.0, &a, x + 20.0, y + h - 12.0 - ah / 2.0, theme::TEXT_MUTED);
    }

    // ── A module's page ──────────────────────────────────────────────────

    /// A mod's settings on a dedicated full page: header with back button, enable switch, live preview, settings, and appearance.
    #[allow(clippy::too_many_arguments)]
    fn mod_page(&mut self, ui: &mut Ui, c: &mut Canvas, mods: &mut Modules, id: &str, x: f32, y: f32, w: f32, h: f32) -> Action {
        let Some(mi) = mods.all.iter().position(|m| m.state.id == id) else {
            self.selected = None;
            return Action::None;
        };
        let mut action = Action::None;
        let hh = 60.0;

        // Top header bar.
        let (back_w, back_h) = (92.0, 34.0);
        let (back_x, back_y) = (x + 20.0, y + (hh - back_h) / 2.0);
        if ui.button(c, "menu:page:back", back_x, back_y, back_w, back_h, "Back", theme::I_ARROW_LEFT, ui::BTN_GHOST) {
            self.selected = None;
            self.page_at = ui.now;
            return Action::None;
        }
        let div_x = back_x + back_w + 14.0;
        c.fill(div_x, y + 16.0, 1.0, hh - 32.0, theme::HAIRLINE);

        let (name, category, icon, enabled, is_hud) = {
            let m = &mods.all[mi];
            (m.state.name, m.state.category, m.state.icon, m.state.enabled, m.is_hud())
        };
        let ic = 36.0;
        let (ix, iy) = (div_x + 16.0, y + (hh - ic) / 2.0);
        c.round(ix, iy, ic, ic, 10.0, if enabled { 0xFFF4F4F5 } else { theme::SUBTLE_HOVER });
        c.icon(icon, 18.0, ix + ic / 2.0, iy + ic / 2.0, if enabled { theme::SOLID_FG } else { theme::TEXT_STRONG });
        let tx = ix + ic + 12.0;
        c.text(font::SEMIBOLD, 15.0, name, tx, y + 15.0, theme::TEXT_STRONG);
        c.text(font::MEDIUM, 10.0, &category.to_uppercase(), tx, y + 34.0, theme::TEXT_MUTED);
        let heart_x = tx + c.text_width(font::SEMIBOLD, 15.0, name) + 12.0;
        if heart(ui, c, "menu:page:fav", &mut mods.all[mi].state.favorite, heart_x, y + 18.0, true) {
            mods.changed();
        }

        // Header action buttons (right aligned).
        let pad = 20.0;
        let (state_w, state_h) = (104.0, 34.0);
        let (state_x, state_y) = (x + w - pad - state_w, y + (hh - state_h) / 2.0);
        if state_button(ui, c, &format!("menu:page:on:{id}"), state_x, state_y, state_w, state_h, enabled, 10.0) {
            mods.all[mi].set_enabled(!enabled);
            mods.changed();
        }
        let mut cur_right = state_x;
        if is_hud {
            let hud_w = 116.0;
            let hud_x = cur_right - 10.0 - hud_w;
            if ui.button(c, "menu:page:hud", hud_x, state_y, hud_w, state_h, "Edit HUD", theme::I_MOVE, ui::BTN_PRIMARY) {
                if !mods.all[mi].state.enabled {
                    mods.all[mi].set_enabled(true);
                    mods.changed();
                }
                action = Action::EditHud;
            }
            cur_right = hud_x;
        }
        let reset_w = 138.0;
        let reset_x = cur_right - 10.0 - reset_w;
        if ui.button(c, "menu:page:reset", reset_x, state_y, reset_w, state_h, "Reset Settings", theme::I_RESET, ui::BTN_GHOST) {
            mods.all[mi].reset_settings();
            mods.changed();
        }

        // Content: settings on the left, a tall live preview on the right (stacked on narrow windows).
        let (bx, by, bw, bh) = (x + pad, y + hh + 4.0, w - pad * 2.0, h - hh - 20.0);
        let gap = 16.0;
        if !mods.all[mi].has_preview() {
            // Nothing to preview: the options fill the page edge to edge, lined up with the header.
            self.settings_panel(ui, c, mods, mi, bx, by, bw, bh);
        } else if bw >= 640.0 {
            let lw = (bw * 0.56).clamp(340.0, 600.0).min(bw - 200.0).max(340.0);
            self.settings_panel(ui, c, mods, mi, bx, by, lw, bh);
            self.stage(ui, c, mods, mi, bx + lw + gap, by, bw - lw - gap, bh);
        } else {
            let stage_h = (bh * 0.38).clamp(120.0, 200.0);
            self.stage(ui, c, mods, mi, bx, by, bw, stage_h);
            self.settings_panel(ui, c, mods, mi, bx, by + stage_h + gap, bw, bh - stage_h - gap);
        }
        action
    }

    /// The live preview: the module drawn large over a game scene, inside its own card.
    #[allow(clippy::too_many_arguments)]
    fn stage(&mut self, ui: &mut Ui, c: &mut Canvas, mods: &mut Modules, mi: usize, x: f32, y: f32, w: f32, h: f32) {
        c.round(x, y, w, h, 12.0, CARD);
        c.outline(x, y, w, h, 12.0, 1.0, theme::HAIRLINE);
        let inset = 6.0;
        let (ix, iy, iw, ih) = (x + inset, y + inset, w - inset * 2.0, h - inset * 2.0);
        c.push_clip(ix, iy, iw, ih);
        if let Some(bg) = self.backdrop.clone() {
            c.image_cover(&bg, ix, iy, iw, ih, 1.0, 0.5, 0.5, 0xFFFFFFFF);
            c.fill(ix, iy, iw, ih, 0x40000000);
        } else {
            c.fill(ix, iy, iw, ih, 0xFF0B0C10);
        }
        let sample = self.sample.clone();
        let m = &mut mods.all[mi];
        let enabled = m.state.enabled;
        if m.is_hud() {
            let keep = hud_scale(m);
            let sz = m.measure(c, &sample);
            let k = 2.2f32.min(((iw - 48.0) / sz[0].max(1.0)).min((ih - 56.0) / sz[1].max(1.0)));
            set_hud_scale(m, (keep * k).max(0.5));
            let mut sz = m.measure(c, &sample);
            let fit = 1.0f32.min(((iw - 24.0) / sz[0].max(1.0)).min((ih - 40.0) / sz[1].max(1.0)));
            if fit < 1.0 {
                let s = hud_scale(m);
                set_hud_scale(m, (s * fit).max(0.3));
                sz = m.measure(c, &sample);
            }
            c.push_alpha(if enabled { 1.0 } else { 0.4 });
            m.paint(c, ix + (iw - sz[0]) / 2.0, iy + (ih - sz[1]) / 2.0, &sample);
            c.pop_alpha();
            set_hud_scale(m, keep);
        } else if !m.behaviour.preview(&m.state, c, &sample, ui.now, ix, iy, iw, ih) {
            let cy = iy + ih / 2.0;
            c.circle(ix + iw / 2.0, cy, 32.0, if enabled { 0xFFF4F4F5 } else { theme::SUBTLE_HOVER });
            c.icon(m.state.icon, 24.0, ix + iw / 2.0, cy, if enabled { theme::SOLID_FG } else { theme::TEXT_STRONG });
        }
        c.pop_clip();
        // Tag in the corner.
        let tag = if enabled { "LIVE PREVIEW" } else { "PREVIEW  \u{b7}  OFF" };
        let tw = c.text_width(font::SEMIBOLD, 9.5, tag) + 18.0;
        c.round(ix + 10.0, iy + 10.0, tw, 20.0, 10.0, 0xB0000000);
        c.outline(ix + 10.0, iy + 10.0, tw, 20.0, 10.0, 1.0, theme::HAIRLINE);
        let lh = c.line_height(font::SEMIBOLD, 9.5);
        c.text(font::SEMIBOLD, 9.5, tag, ix + 19.0, iy + 10.0 + (20.0 - lh) / 2.0, theme::TEXT_SECONDARY);
    }

    /// What the module does, then its options in foldable sections (like a game client's module screen).
    #[allow(clippy::too_many_arguments)]
    fn settings_panel(&mut self, ui: &mut Ui, c: &mut Canvas, mods: &mut Modules, mi: usize, x: f32, y: f32, w: f32, h: f32) {
        c.round(x, y, w, h, 12.0, CARD);
        c.outline(x, y, w, h, 12.0, 1.0, theme::HAIRLINE);
        let pad = 18.0;
        let desc = mods.all[mi].state.description;
        let text_w = w - pad * 2.0 - 26.0;
        let desc_h = wrap_height(c, font::REGULAR, 11.5, desc, text_w, 16.0, 2);
        let info_h = 34.0f32.max(desc_h + 20.0);
        let iy = y + info_h / 2.0;
        c.circle(x + pad + 8.0, iy, 8.0, theme::SUBTLE_HOVER);
        let iw = c.text_width(font::BOLD, 10.0, "i");
        let ih = c.line_height(font::BOLD, 10.0);
        c.text(font::BOLD, 10.0, "i", x + pad + 8.0 - iw / 2.0, iy - ih / 2.0, theme::TEXT_SECONDARY);
        wrap(c, font::REGULAR, 11.5, desc, x + pad + 26.0, y + (info_h - desc_h) / 2.0, text_w, 16.0, theme::TEXT_SECONDARY, 2);
        c.fill(x + 1.0, y + info_h, w - 2.0, 1.0, theme::HAIRLINE);

        let top = y + info_h + 1.0;
        let ph = h - info_h - 2.0;
        let off = self.detail.begin(ui, c, x + 1.0, top, w - 2.0, ph);
        let mut cy = top - off + 6.0;
        cy = self.section(ui, c, mods, mi, "Keybind", Part::Keybind, x, cy, w);
        cy = self.section(ui, c, mods, mi, "Options", Part::Options, x, cy, w);
        cy = self.section(ui, c, mods, mi, "Appearance", Part::Appearance, x, cy, w);
        self.detail.end(ui, c, cy - (top - off) + 8.0);
    }

    /// One foldable group: a header row (chevron, title, option count) and, when open, the option rows. Returns the y below it.
    #[allow(clippy::too_many_arguments)]
    fn section(&mut self, ui: &mut Ui, c: &mut Canvas, mods: &mut Modules, mi: usize, title: &str, part: Part, x: f32, y: f32, w: f32) -> f32 {
        let rows: Vec<usize> = match part {
            Part::Keybind => vec![0],
            Part::Options => mods.all[mi].shown_settings(),
            Part::Appearance => mods.all[mi].shown_appearance(),
        };
        if rows.is_empty() {
            return y;
        }
        let mod_id = mods.all[mi].state.id;
        let key = format!("{mod_id}:{title}");
        let open = !self.folded.contains(&key);
        let (pad, hh) = (18.0, 34.0);
        let id = format!("menu:sec:{key}");
        let over = ui.hover(c, x + 6.0, y, w - 12.0, hh);
        let hv = ui.anim_on(&format!("{id}#h"), over, 14.0);
        if hv > 0.01 {
            c.round(x + 6.0, y + 1.0, w - 12.0, hh - 2.0, 8.0, theme::alpha(0xFFFFFFFF, 0.04 * hv));
        }
        let rot = ui.anim_on(&format!("{id}#o"), open, 14.0);
        c.icon(
            if rot > 0.5 { theme::I_CHEVRON_DOWN } else { theme::I_CHEVRON_RIGHT },
            12.0,
            x + pad + 6.0,
            y + hh / 2.0,
            theme::mix(theme::TEXT_MUTED, theme::TEXT_STRONG, hv),
        );
        let lh = c.line_height(font::SEMIBOLD, 12.5);
        c.text(font::SEMIBOLD, 12.5, title, x + pad + 22.0, y + (hh - lh) / 2.0, theme::TEXT_STRONG);
        let count = format!("{} {}", rows.len(), if rows.len() == 1 { "option" } else { "options" });
        let cw = c.text_width(font::REGULAR, 10.5, &count);
        let ch = c.line_height(font::REGULAR, 10.5);
        c.text(font::REGULAR, 10.5, &count, x + w - pad - 8.0 - cw, y + (hh - ch) / 2.0, theme::TEXT_MUTED);
        if over {
            ui.cursor_hand = true;
        }
        if ui.clicked(c, &id, x + 6.0, y, w - 12.0, hh) && !self.folded.remove(&key) {
            self.folded.insert(key);
        }
        let mut y = y + hh;
        if !open {
            return y + 4.0;
        }
        let (rx, rw) = (x + pad + 4.0, w - pad * 2.0 - 14.0);
        for (n, si) in rows.iter().enumerate() {
            if n > 0 {
                c.fill(rx, y, rw, 1.0, theme::HAIRLINE);
            }
            let warning = if part == Part::Keybind { conflict(mods, mi) } else { None };
            let m = &mut mods.all[mi];
            let s = match part {
                Part::Keybind => &mut m.state.toggle_key,
                Part::Options => &mut m.state.settings[*si],
                Part::Appearance => match m.state.hud.as_mut() {
                    Some(h) => &mut h.appearance[*si],
                    None => continue,
                },
            };
            let sid = s.id;
            let rid = format!("menu:set:{mod_id}:{sid}");
            let rh = Widgets::row_height(s);
            if self.widgets.row(ui, c, &rid, rx, y, rw, s, warning.as_deref()) {
                mods.changed();
            }
            y += rh;
        }
        y + 10.0
    }

    // ── Settings ─────────────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn settings(&mut self, ui: &mut Ui, c: &mut Canvas, mods: &mut Modules, cfg: &mut UiConfig, who: Option<&str>, x: f32, y: f32, w: f32, h: f32) -> Action {
        let mut action = Action::None;
        self.s_title.set_bool(cfg.custom_title);
        self.s_notify.set_bool(cfg.notifications);
        self.s_activity.set_bool(cfg.share_activity);
        // While the slider is held (and on the frame it is let go) the slider owns the value.
        let scale_held = ui.is_pressing("menu:pref:scale");
        if !scale_held && !self.scale_dragged {
            self.s_scale.set_num(cfg.scale);
        }
        self.s_menu.set_key(cfg.menu_key);
        self.s_chat.set_key(cfg.relay_key);
        self.s_play.set_key(cfg.music_play_key);
        self.s_next.set_key(cfg.music_next_key);
        self.s_prev.set_key(cfg.music_prev_key);

        let wide = w >= 760.0;
        let side_w = if wide { 320.0f32.min(w * 0.36) } else { 0.0 };
        let lw = if wide { w - side_w - 20.0 } else { w };
        let off = self.prefs.begin(ui, c, x, y, lw + 8.0, h);
        let mut ry = y - off;
        let cw = lw - 6.0;
        ry = self.group(ui, c, "general", x, ry, cw, theme::I_SLIDERS, "General", "How Native looks and talks to you", Group::General);
        ry = self.group(ui, c, "keys", x, ry, cw, theme::I_KEYBOARD, "Controls", "Click a key, then press the one you want", Group::Keys);
        let (y2, a) = self.hud_card(ui, c, mods, x, ry, cw);
        ry = y2;
        if a != Action::None {
            action = a;
        }
        if !wide {
            ry = self.profile_card(ui, c, mods, who, x, ry, cw) + 16.0;
            ry = self.about_card(ui, c, x, ry, cw) + 16.0;
        }
        c.text(
            font::REGULAR,
            11.0,
            "Saved automatically to config/native-ui.json and config/native-modules.json.",
            x + 4.0,
            ry,
            theme::TEXT_MUTED,
        );
        ry += 28.0;
        self.prefs.end(ui, c, ry + off - y);

        if wide {
            let sx = x + w - side_w;
            let sy = self.profile_card(ui, c, mods, who, sx, y, side_w);
            self.about_card(ui, c, sx, sy + 16.0, side_w);
        }

        // Write back.
        let mut save = false;
        if self.s_title.as_bool() != cfg.custom_title {
            cfg.custom_title = self.s_title.as_bool();
            save = true;
        }
        if self.s_notify.as_bool() != cfg.notifications {
            cfg.notifications = self.s_notify.as_bool();
            save = true;
        }
        if self.s_activity.as_bool() != cfg.share_activity {
            cfg.share_activity = self.s_activity.as_bool();
            save = true;
        }
        let scale = self.s_scale.as_f32();
        if scale_held {
            self.scale_dragged = true;
        } else if self.scale_dragged {
            self.scale_dragged = false;
            if (scale - cfg.scale).abs() > 0.001 {
                cfg.scale = scale; // Applied on release so the window doesn't jump under the cursor.
                save = true;
            }
        } else if (scale - cfg.scale).abs() > 0.001 {
            cfg.scale = scale; // The reset button.
            save = true;
        }
        if self.s_menu.as_key() != cfg.menu_key && self.s_menu.as_key() != -1 {
            cfg.menu_key = self.s_menu.as_key();
            save = true;
        }
        if self.s_chat.as_key() != cfg.relay_key && self.s_chat.as_key() != -1 {
            cfg.relay_key = self.s_chat.as_key();
            save = true;
        }
        if self.s_play.as_key() != cfg.music_play_key
            || self.s_next.as_key() != cfg.music_next_key
            || self.s_prev.as_key() != cfg.music_prev_key
        {
            cfg.music_play_key = self.s_play.as_key();
            cfg.music_next_key = self.s_next.as_key();
            cfg.music_prev_key = self.s_prev.as_key();
            save = true;
        }
        if save {
            cfg.save();
        }
        action
    }

    /// A settings card: icon tile, title and a hint on top, then the rows. Returns the y below it.
    #[allow(clippy::too_many_arguments)]
    fn group(&mut self, ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, w: f32, icon: u32, title: &str, hint: &str, which: Group) -> f32 {
        let head = 62.0;
        let rows: Vec<&mut Setting> = match which {
            Group::General => vec![&mut self.s_title, &mut self.s_notify, &mut self.s_activity, &mut self.s_scale],
            Group::Keys => vec![&mut self.s_menu, &mut self.s_chat, &mut self.s_play, &mut self.s_next, &mut self.s_prev],
        };
        let rh: f32 = rows.iter().map(|s| Widgets::row_height(s)).sum();
        let h = head + rh + 8.0;
        card_frame(ui, c, &format!("menu:pref:card:{id}"), x, y, w, h);
        card_head(c, x, y, w, icon, title, hint);
        let mut cy = y + head;
        for s in rows {
            c.fill(x + 18.0, cy, w - 36.0, 1.0, theme::HAIRLINE);
            let sid = s.id;
            let rh = Widgets::row_height(s);
            self.widgets.row(ui, c, &format!("menu:pref:{sid}"), x + 18.0, cy, w - 36.0, s, None);
            cy += rh;
        }
        y + h + 16.0
    }

    fn hud_card(&mut self, ui: &mut Ui, c: &mut Canvas, mods: &mut Modules, x: f32, y: f32, w: f32) -> (f32, Action) {
        let mut action = Action::None;
        let h = 62.0 + 74.0 + 14.0;
        card_frame(ui, c, "menu:pref:card:hud", x, y, w, h);
        card_head(c, x, y, w, theme::I_LAYOUT, "HUD", "Where your HUD mods sit on screen");
        let ty = y + 62.0;
        let gap = 10.0;
        let tw = (w - 36.0 - gap * 2.0) / 3.0;
        let th = 74.0;
        if self.tile(ui, c, "menu:pref:edit", x + 18.0, ty, tw, th, theme::I_MOVE, "Edit layout", "Drag and resize", false) {
            action = Action::EditHud;
        }
        if self.tile(ui, c, "menu:pref:positions", x + 18.0 + tw + gap, ty, tw, th, theme::I_RESET, "Reset positions", "Back to defaults", false) {
            mods.reset_positions();
        }
        let label = if self.confirm_reset { "Click to confirm" } else { "Reset all mods" };
        let hint = if self.confirm_reset { "Settings + on/off" } else { "Every setting" };
        if self.tile(ui, c, "menu:pref:reset", x + 18.0 + (tw + gap) * 2.0, ty, tw, th, theme::I_POWER, label, hint, true) {
            if self.confirm_reset {
                for m in &mut mods.all {
                    m.reset_settings();
                    let on = m.state.default_enabled;
                    m.set_enabled(on);
                }
                mods.reset_positions();
                mods.changed();
                self.confirm_reset = false;
            } else {
                self.confirm_reset = true;
            }
        }
        if self.confirm_reset && ui.pressed && !ui.hover(c, x + 18.0 + (tw + gap) * 2.0, ty, tw, th) {
            self.confirm_reset = false; // Clicking anywhere else cancels.
        }
        (y + h + 16.0, action)
    }

    /// A square-ish action tile with an icon, a title and a hint.
    #[allow(clippy::too_many_arguments)]
    fn tile(&mut self, ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, w: f32, h: f32, icon: u32, title: &str, hint: &str, danger: bool) -> bool {
        let over = ui.hover(c, x, y, w, h);
        let click = ui.clicked(c, id, x, y, w, h);
        let hv = ui.anim_on(&format!("{id}#h"), over, 14.0);
        let pressing = ui.is_pressing(id);
        let pr = ui.anim_on(&format!("{id}#p"), pressing, 20.0);
        let armed = danger && self.confirm_reset;
        let bg = if armed {
            theme::mix(0x26EF4444, 0x40EF4444, hv)
        } else {
            theme::mix(theme::SUBTLE, theme::SUBTLE_HOVER, hv)
        };
        let sy = y + pr * 1.5;
        c.round(x, sy, w, h, 12.0, bg);
        c.outline(
            x,
            sy,
            w,
            h,
            12.0,
            1.0,
            if armed { 0x4DEF4444 } else { theme::mix(theme::HAIRLINE, theme::HAIRLINE_STRONG, hv) },
        );
        let fg = if armed {
            0xFFFCA5A5
        } else if danger {
            theme::mix(theme::TEXT_SECONDARY, 0xFFFCA5A5, hv)
        } else {
            theme::TEXT_STRONG
        };
        c.icon(icon, 15.0, x + 22.0, sy + 22.0, fg);
        let t = c.ellipsize(font::SEMIBOLD, 12.0, title, w - 24.0);
        c.text(font::SEMIBOLD, 12.0, &t, x + 12.0, sy + h - 36.0, fg);
        let hnt = c.ellipsize(font::REGULAR, 10.5, hint, w - 24.0);
        c.text(font::REGULAR, 10.5, &hnt, x + 12.0, sy + h - 19.0, theme::TEXT_MUTED);
        if over {
            ui.cursor_hand = true;
        }
        click
    }

    #[allow(clippy::too_many_arguments)]
    fn profile_card(&mut self, ui: &mut Ui, c: &mut Canvas, mods: &Modules, who: Option<&str>, x: f32, y: f32, w: f32) -> f32 {
        let h = 196.0;
        card_frame(ui, c, "menu:pref:card:me", x, y, w, h);
        glow(c, x, y, w, 90.0, 16.0, 0x10FFFFFF);
        let name = who.unwrap_or("Player");
        let av = 56.0;
        avatar(c, name, x + 18.0, y + 18.0, av, 12.0);
        c.circle(x + 18.0 + av - 4.0, y + 18.0 + av - 4.0, 7.0, 0xFF05060A);
        c.circle(x + 18.0 + av - 4.0, y + 18.0 + av - 4.0, 5.0, theme::ONLINE);
        let tx = x + 18.0 + av + 14.0;
        let n = c.ellipsize(font::SEMIBOLD, 16.0, name, w - (tx - x) - 16.0);
        c.text(font::SEMIBOLD, 16.0, &n, tx, y + 24.0, theme::TEXT_STRONG);
        let status = c.ellipsize(font::REGULAR, 11.0, "Native Client \u{b7} Rust Edition", w - (tx - x) - 16.0);
        c.text(font::REGULAR, 11.0, &status, tx, y + 48.0, theme::TEXT_MUTED);
        let on = mods.all.iter().filter(|m| m.state.enabled).count();
        let sy = y + 92.0;
        let sw = (w - 36.0 - 10.0) / 2.0;
        stat(c, x + 18.0, sy, sw, &on.to_string(), &format!("of {} mods on", mods.all.len()));
        stat(c, x + 18.0 + sw + 10.0, sy, sw, "\u{2014}", "cosmetics owned");
        let by = y + h - 18.0 - 34.0;
        if ui.button(c, "menu:pref:locker", x + 18.0, by, w - 36.0, 34.0, "Open locker", theme::I_SHIRT, ui::BTN_PRIMARY) {
            self.set_tab(TAB_COSMETICS, ui.now);
        }
        y + h
    }

    fn about_card(&mut self, ui: &mut Ui, c: &mut Canvas, x: f32, y: f32, w: f32) -> f32 {
        let h = 112.0;
        card_frame(ui, c, "menu:pref:card:about", x, y, w, h);
        let lg = 34.0;
        if let Some(logo) = self.logo.clone() {
            c.image(&logo, x + 18.0, y + 16.0, lg, lg, [0.0, 0.0, 1.0, 1.0], 0xFFFFFFFF);
        }
        c.text(font::SEMIBOLD, 14.0, "Native Client", x + 18.0 + lg + 12.0, y + 17.0, theme::TEXT_STRONG);
        c.text(font::REGULAR, 11.0, "Rust Edition  \u{b7}  Minecraft 26.3", x + 18.0 + lg + 12.0, y + 36.0, theme::TEXT_MUTED);
        c.fill(x + 18.0, y + 64.0, w - 36.0, 1.0, theme::HAIRLINE);
        let t = c.ellipsize(font::REGULAR, 11.0, "Updates install from the launcher. playnative.net", w - 36.0);
        c.text(font::REGULAR, 11.0, &t, x + 18.0, y + 80.0, theme::TEXT_SECONDARY);
        y + h
    }
}

#[derive(Clone, Copy)]
enum Group {
    General,
    Keys,
}

/// `ModuleKeybinds.conflict` for the toggle key of module `mi`.
pub fn conflict(mods: &Modules, mi: usize) -> Option<String> {
    let b = &mods.all[mi].state.toggle_key;
    if b.is_default() {
        return None;
    }
    let Kind::Binding { key, modifiers } = b.kind else { return None };
    // Reserved by Native itself: the menu key (Right Shift), chat (Y), voice call control (V, N).
    if modifiers & 6 == 0 && key == 344 {
        return Some("Reserved: Native menu".to_owned());
    }
    if modifiers == 0 && key == 89 {
        return Some("Reserved: Relay".to_owned());
    }
    if modifiers == 0 && (key == 86 || key == 78) {
        return Some("Reserved: voice call control".to_owned());
    }
    for (i, m) in mods.all.iter().enumerate() {
        if i != mi && matches!(m.state.toggle_key.kind, Kind::Binding { key: k, modifiers: md } if k == key && md == modifiers) {
            return Some(format!("Conflict: {}", m.state.name));
        }
        // Hold-to-use controls poll the key state independently of modifiers, even while disabled.
        for s in m.state.all_settings() {
            if matches!(s.kind, Kind::Key { value, .. } if value == key) {
                return Some(format!("Reserved: {} / {}", m.state.name, s.label));
            }
        }
    }
    None
}

fn hud_scale(m: &Module) -> f32 {
    m.state.hud.as_ref().map_or(1.0, |h| h.scale())
}

/// Sets the module's size without the step snapping (the preview fits the module to its stage).
fn set_hud_scale(m: &mut Module, v: f32) {
    if let Some(s) = m.state.hud.as_mut().and_then(|h| h.get_mut(super::module::SCALE))
        && let Kind::Num { value, .. } = &mut s.kind
    {
        *value = v;
    }
}

pub fn normalize(q: &str) -> String {
    q.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A head stand-in: the player's initial on a soft tile (the skin is drawn by the game).
pub fn avatar(c: &mut Canvas, name: &str, x: f32, y: f32, s: f32, r: f32) {
    c.round(x, y, s, s, r, theme::mix(0xFF1B1D24, theme::name_color(Some(name)), 0.35));
    c.outline(x, y, s, s, r, 1.0, theme::HAIRLINE_STRONG);
    let initial = name.chars().next().map_or_else(|| "?".to_owned(), |ch| ch.to_uppercase().collect());
    let size = (s * 0.46).max(9.0);
    let tw = c.text_width(font::BOLD, size, &initial);
    let lh = c.line_height(font::BOLD, size);
    c.text(font::BOLD, size, &initial, x + (s - tw) / 2.0, y + (s - lh) / 2.0, theme::TEXT_STRONG);
}

/// Top-down sheen inside a rounded box; the top corners follow the radius so nothing pokes out of the outline.
pub fn glow(c: &mut Canvas, x: f32, y: f32, w: f32, gh: f32, r: f32, top: u32) {
    let cap = r + 1.0;
    c.push_clip(x, y, w, cap);
    c.round(x + 1.0, y + 1.0, w - 2.0, cap * 2.0 + 2.0, r - 1.0, top);
    c.pop_clip();
    c.gradient_v(x + 1.0, y + cap, w - 2.0, gh - cap, theme::alpha(top, 1.0 - cap / gh), 0x00FFFFFF);
}

pub fn panel(c: &mut Canvas, x: f32, y: f32, w: f32, h: f32) {
    c.shadow(x, y + 4.0, w, h, 16.0, 24.0, 0x88000000);
    c.round(x, y, w, h, 16.0, PANEL_BG);
    c.outline(x, y, w, h, 16.0, 1.0, theme::HAIRLINE_STRONG);
}

pub fn header(c: &mut Canvas, title: &str, x: f32, y: f32, h: f32) -> f32 {
    let lh = c.line_height(font::SEMIBOLD, 18.0);
    c.text(font::SEMIBOLD, 18.0, title, x + 20.0, y + (h - lh) / 2.0, theme::TEXT_STRONG);
    x + 20.0 + c.text_width(font::SEMIBOLD, 18.0, title)
}

/// Square icon tile with an active marker; the label shows up as a tooltip.
#[allow(clippy::too_many_arguments)]
fn rail_tile(ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, icon: u32, label: &str, on: bool) -> bool {
    let s = RAIL;
    let over = ui.hover(c, x, y, s, s);
    let click = ui.clicked(c, id, x, y, s, s);
    let hv = ui.anim_on(&format!("{id}#h"), over, 14.0);
    let sel = ui.anim_on(&format!("{id}#on"), on, 16.0);
    c.round(x, y, s, s, 15.0, theme::mix(theme::alpha(theme::SUBTLE_HOVER, hv * 0.85), 0x16FFFFFF, sel));
    if sel > 0.01 {
        c.outline(x, y, s, s, 15.0, 1.0, theme::alpha(theme::HAIRLINE_STRONG, sel));
        let bh = 18.0 * sel;
        c.round(x - 5.0, y + (s - bh) / 2.0, 3.0, bh, 1.5, theme::alpha(theme::WHITE, sel));
    }
    let fg = theme::mix(theme::mix(theme::TEXT_MUTED, theme::TEXT_SECONDARY, hv), theme::TEXT_STRONG, sel);
    c.icon(icon, 24.0, x + s / 2.0, y + s / 2.0, fg);
    if over {
        ui.tip(label);
    }
    click
}

#[allow(clippy::too_many_arguments)]
fn pill_icon(ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, s: f32, icon: u32, on: bool) -> bool {
    let over = ui.hover(c, x, y, s, s);
    let click = ui.clicked(c, id, x, y, s, s);
    let hv = ui.anim_on(&format!("{id}#h"), over, 14.0);
    let sel = ui.anim_on(&format!("{id}#on"), on, 16.0);
    c.round(x, y, s, s, 9.0, theme::mix(theme::alpha(theme::SUBTLE_HOVER, hv), 0xFFF4F4F5, sel));
    c.icon(icon, 14.0, x + s / 2.0, y + s / 2.0, theme::mix(theme::mix(theme::TEXT_MUTED, theme::TEXT_STRONG, hv), theme::SOLID_FG, sel));
    click
}

/// The pin heart; visible on hover or when pinned. True when it took the click.
fn heart(ui: &mut Ui, c: &mut Canvas, id: &str, favorite: &mut bool, x: f32, y: f32, visible: bool) -> bool {
    let s = 24.0;
    let over = ui.hover(c, x, y, s, s);
    let click = (visible || *favorite) && ui.clicked(c, id, x, y, s, s);
    if click {
        *favorite = !*favorite;
    }
    let v = ui.anim_on(&format!("{id}#v"), visible || *favorite, 14.0);
    let hv = ui.anim_on(&format!("{id}#h"), over, 16.0);
    let fav = ui.anim_on(&format!("{id}#f"), *favorite, 14.0);
    if v > 0.01 {
        c.round(x, y, s, s, s / 2.0, theme::alpha(theme::SUBTLE_HOVER, hv * v));
        let col = theme::alpha(theme::mix(theme::mix(theme::TEXT_MUTED, theme::TEXT_STRONG, hv), theme::TEXT_STRONG, fav), v);
        // Pinned: a solid white heart (the icon font only has outlines, so stack shrinking copies to fill it).
        if fav > 0.01 {
            let fill = theme::alpha(theme::TEXT_STRONG, fav * v);
            let mut fs = 11.5;
            while fs >= 2.5 {
                c.icon(theme::I_HEART, fs, x + s / 2.0, y + s / 2.0, fill);
                fs -= 1.5;
            }
        }
        c.icon(theme::I_HEART, 13.0, x + s / 2.0, y + s / 2.0, col);
    }
    click || (over && ui.pressed)
}

/// The wide ENABLED / DISABLED bar of a Feather card. True when clicked.
#[allow(clippy::too_many_arguments)]
fn state_button(ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, w: f32, h: f32, enabled: bool, fs: f32) -> bool {
    let over = ui.hover(c, x, y, w, h);
    let click = ui.clicked(c, id, x, y, w, h);
    let hv = ui.anim_on(&format!("{id}#h"), over, 16.0);
    let on = ui.anim_on(&format!("{id}#on"), enabled, 14.0);
    let tint = theme::mix(OFF, ON, on);
    c.round(x, y, w, h, 8.0, theme::alpha(tint, 0.16 + 0.1 * hv));
    c.outline(x, y, w, h, 8.0, 1.0, theme::alpha(tint, 0.35 + 0.25 * hv));
    let t = if enabled { "ENABLED" } else { "DISABLED" };
    let tw = c.text_width(font::BOLD, fs, t);
    let lh = c.line_height(font::BOLD, fs);
    c.text(font::BOLD, fs, t, x + (w - tw) / 2.0, y + (h - lh) / 2.0, theme::mix(0xFFFCA5A5, 0xFF86EFAC, on));
    if over {
        ui.cursor_hand = true;
    }
    click
}

fn card_frame(ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, w: f32, h: f32) {
    let over = ui.hover(c, x, y, w, h);
    let hv = ui.anim_on(id, over, 10.0);
    c.round(x, y, w, h, 16.0, theme::mix(CARD, CARD_HOVER, hv));
    glow(c, x, y, w, h.min(70.0), 16.0, 0x0AFFFFFF);
    c.outline(x, y, w, h, 16.0, 1.0, theme::mix(theme::HAIRLINE, theme::HAIRLINE_STRONG, hv));
}

fn card_head(c: &mut Canvas, x: f32, y: f32, w: f32, icon: u32, title: &str, hint: &str) {
    let ic = 34.0;
    c.round(x + 18.0, y + 14.0, ic, ic, 10.0, theme::SUBTLE_HOVER);
    c.outline(x + 18.0, y + 14.0, ic, ic, 10.0, 1.0, theme::HAIRLINE);
    c.icon(icon, 16.0, x + 18.0 + ic / 2.0, y + 14.0 + ic / 2.0, theme::TEXT_STRONG);
    let tx = x + 18.0 + ic + 14.0;
    c.text(font::SEMIBOLD, 14.0, title, tx, y + 15.0, theme::TEXT_STRONG);
    let hint = c.ellipsize(font::REGULAR, 11.0, hint, w - (tx - x) - 18.0);
    c.text(font::REGULAR, 11.0, &hint, tx, y + 34.0, theme::TEXT_MUTED);
}

fn stat(c: &mut Canvas, x: f32, y: f32, w: f32, value: &str, label: &str) {
    c.round(x, y, w, 44.0, 10.0, theme::SUBTLE);
    c.text(font::BOLD, 15.0, value, x + 12.0, y + 6.0, theme::TEXT_STRONG);
    let l = c.ellipsize(font::REGULAR, 10.5, label, w - 24.0);
    c.text(font::REGULAR, 10.5, &l, x + 12.0, y + 25.0, theme::TEXT_MUTED);
}

pub fn disabled(c: &mut Canvas, x: f32, y: f32, w: f32, h: f32, label: &str) {
    c.round(x, y, w, h, 10.0, theme::SUBTLE);
    c.outline(x, y, w, h, 10.0, 1.0, theme::HAIRLINE);
    let tw = c.text_width(font::SEMIBOLD, 12.5, label);
    let lh = c.line_height(font::SEMIBOLD, 12.5);
    c.text(font::SEMIBOLD, 12.5, label, x + (w - tw) / 2.0, y + (h - lh) / 2.0, theme::TEXT_MUTED);
}

/// Word-wraps text into at most `max_lines` lines; the last one is ellipsized. Returns the y below it.
#[allow(clippy::too_many_arguments)]
pub fn wrap(c: &mut Canvas, face: usize, size: f32, text: &str, x: f32, mut y: f32, w: f32, lh: f32, color: u32, max_lines: usize) -> f32 {
    let words: Vec<&str> = text.split(' ').collect();
    let mut line = String::new();
    let mut lines = 0;
    for (i, word) in words.iter().enumerate() {
        let next = if line.is_empty() { (*word).to_owned() } else { format!("{line} {word}") };
        if c.text_width(face, size, &next) > w && !line.is_empty() {
            if lines == max_lines - 1 {
                let rest = format!("{line} {}", words[i..].join(" "));
                let t = c.ellipsize(face, size, &rest, w);
                c.text(face, size, &t, x, y, color);
                return y + lh;
            }
            c.text(face, size, &line, x, y, color);
            y += lh;
            lines += 1;
            line = (*word).to_owned();
        } else {
            line = next;
        }
    }
    if !line.is_empty() {
        c.text(face, size, &line, x, y, color);
        y += lh;
    }
    y
}

#[allow(clippy::too_many_arguments)]
pub fn wrap_height(c: &mut Canvas, face: usize, size: f32, text: &str, w: f32, lh: f32, max_lines: usize) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let mut line = String::new();
    let mut lines = 0;
    for word in text.split(' ') {
        let next = if line.is_empty() { word.to_owned() } else { format!("{line} {word}") };
        if c.text_width(face, size, &next) > w {
            lines += 1;
            if lines >= max_lines {
                return lines as f32 * lh;
            }
            line = word.to_owned();
        } else {
            line = next;
        }
    }
    if !line.is_empty() {
        lines += 1;
    }
    lines as f32 * lh
}

pub fn empty(c: &mut Canvas, x: f32, y: f32, w: f32, h: f32, icon: u32, title: &str, body: &str) {
    let cy = y + h / 2.0 - 34.0;
    c.circle(x + w / 2.0, cy, 22.0, theme::SUBTLE_HOVER);
    c.icon(icon, 18.0, x + w / 2.0, cy, theme::TEXT_SECONDARY);
    let tw = c.text_width(font::SEMIBOLD, 13.5, title);
    c.text(font::SEMIBOLD, 13.5, title, x + (w - tw) / 2.0, cy + 32.0, theme::TEXT_STRONG);
    let b = c.ellipsize(font::REGULAR, 11.5, body, w - 24.0);
    let bw = c.text_width(font::REGULAR, 11.5, &b);
    c.text(font::REGULAR, 11.5, &b, x + (w - bw) / 2.0, cy + 54.0, theme::TEXT_MUTED);
}

pub fn ease(t: f32) -> f32 {
    1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t)
}

pub fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_matches_words_in_any_order() {
        assert_eq!(normalize("  Frames   PER second "), "frames per second");
        let mods = Modules::new(None);
        let fps = mods.get("fps").unwrap();
        assert!(Menu::query_matches(fps, "fps"));
        assert!(Menu::query_matches(fps, ""));
        assert!(!Menu::query_matches(fps, "zzzz"));
    }

    #[test]
    fn a_toggle_key_that_collides_is_flagged() {
        let mut mods = Modules::new(None);
        let (a, b) = (0, 1);
        mods.all[a].state.toggle_key.kind = Kind::Binding { key: 75, modifiers: 0 };
        mods.all[b].state.toggle_key.kind = Kind::Binding { key: 75, modifiers: 0 };
        assert!(conflict(&mods, a).unwrap().starts_with("Conflict"));
        mods.all[b].state.toggle_key.kind = Kind::Binding { key: 76, modifiers: 0 };
        assert!(conflict(&mods, a).is_none());
        mods.all[a].state.toggle_key.kind = Kind::Binding { key: 344, modifiers: 0 };
        assert_eq!(conflict(&mods, a).as_deref(), Some("Reserved: Native menu"));
    }

    #[test]
    fn the_menu_draws_without_panicking() {
        let mut c = horizon_client::native_ui::canvas();
        c.begin(1280, 760, 1.0);
        let mut ui = Ui::default();
        ui.begin(&[ui::Event::Move { x: 400.0, y: 300.0 }], 1.0);
        let mut mods = Modules::new(None);
        let mut cfg = UiConfig::default();
        let mut menu = Menu::new();
        menu.opened(ui.now - 1000.0);
        for tab in 0..TABS.len() {
            menu.open_tab(tab, ui.now);
            menu.draw(&mut ui, &mut c, &mut mods, &mut cfg, Some("Steve"), true);
        }
        menu.open_mod("fps", ui.now);
        menu.draw(&mut ui, &mut c, &mut mods, &mut cfg, Some("Steve"), true);
        let mut batch = horizon_client::hud::HudBatch::new([1280, 760]);
        c.finish(&mut batch);
        assert!(!batch.vertices.is_empty());
    }
}
