//! `MenuView`: the Native Client menu (Right Shift): a unified sleek dark obsidian window (~1020x620) with a top
//! navigation bar (`MODS`, `COSMETICS`, `EMOTES`, `WAYPOINTS`, `MUSIC`, `SHOTS`, `SETTINGS`), a left Profiles
//! sidebar with persistent saved profiles (`config/native-profiles.json`), `SAVE AS NEW PROFILE`, and vibrant blue
//! `EDIT HUD LAYOUT` primary button, plus colored uppercase category pills and 3-column compact horizontal mod cards
//! with emerald green borders when enabled and crimson red borders when disabled.
#![allow(dead_code)]

use super::emotes_view::{EmotesAct, EmotesView};
use super::locker::Locker;
use super::module::{GameView, Module, label};
use super::modules::Modules;
use super::music_view::MusicView;
use super::screenshots::Screenshots;
use super::scroll::Scroll;
use super::setting::{Kind, Setting};
use super::textfield::TextField;
use super::ui::{self, Ui};
use super::uiconfig::UiConfig;
use super::widgets::Widgets;
use horizon_client::native_ui::{Canvas, Image, assets, font, theme};
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

pub const TABS: [&str; 7] = ["Mods", "Cosmetics", "Emotes", "Settings", "Screenshots", "Music", "Waypoints"];
/// Top header tab order: MODS, COSMETICS, EMOTES, WAYPOINTS, MUSIC, SHOTS, SETTINGS.
const TOP_ORDER: [usize; 7] = [TAB_MODS, TAB_COSMETICS, TAB_EMOTES, TAB_WAYPOINTS, TAB_MUSIC, TAB_SHOTS, TAB_SETTINGS];
const TOP_LABELS: [&str; 7] = ["MODS", "COSMETICS", "EMOTES", "SETTINGS", "SHOTS", "MUSIC", "WAYPOINTS"];
pub const TAB_MODS: usize = 0;
pub const TAB_COSMETICS: usize = 1;
pub const TAB_EMOTES: usize = 2;
pub const TAB_SETTINGS: usize = 3;
pub const TAB_SHOTS: usize = 4;
pub const TAB_MUSIC: usize = 5;
pub const TAB_WAYPOINTS: usize = 6;

const CAT_LABELS: [&str; 6] = ["ALL", "NEW", "HUD", "VISUAL", "MECHANIC", "FAV"];
const CAT_COLORS: [u32; 6] = [
    0xFF4F86F7, // ALL (blue)
    0xFF4D8B31, // NEW (green)
    0xFF4B6FA5, // HUD (steel blue)
    0xFF566BAA, // VISUAL (slate indigo)
    0xFF4E6B94, // MECHANIC (slate blue)
    0xFFD97706, // FAV (amber)
];
const NEW_MODS: [&str; 4] = ["markers", "waypoints", "voice", "emotes"];

pub const PANEL_BG: u32 = 0xF40B0C10;
pub const CARD: u32 = 0xD811131A;
pub const CARD_HOVER: u32 = 0xEC181B24;
const ON: u32 = 0xFF22C55E;
const OFF: u32 = 0xFFDC2626;
const PRIMARY_BLUE: u32 = 0xFF4F86F7;
const LOGO_KEY: u64 = 0x4D45_4E55_4C4F_474F;
const SIDEBAR_W: f32 = 176.0;

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

#[derive(Clone, Debug)]
struct SavedProfile {
    name: String,
    modules: Map<String, Value>,
}

struct Profiles {
    loaded: bool,
    active: String,
    list: Vec<SavedProfile>,
    renaming: Option<usize>,
    rename_field: TextField,
    scroll: Scroll,
}

impl Default for Profiles {
    fn default() -> Self {
        let mut rename_field = TextField::new("menu:profile:rename");
        rename_field.max_length = 24;
        rename_field.bare = true;
        Profiles {
            loaded: false,
            active: "Default".to_owned(),
            list: vec![SavedProfile { name: "Default".to_owned(), modules: Map::new() }],
            renaming: None,
            rename_field,
            scroll: Scroll::new("menu:profiles"),
        }
    }
}

impl Profiles {
    fn file(game_dir: Option<&std::path::Path>) -> Option<PathBuf> {
        game_dir.map(|d| d.join("config").join("native-profiles.json"))
    }

    fn ensure_loaded(&mut self, game_dir: Option<&std::path::Path>, mods: &Modules) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        if let Some(path) = Self::file(game_dir)
            && let Ok(text) = std::fs::read_to_string(path)
            && let Ok(root) = serde_json::from_str::<Value>(&text)
        {
            let mut loaded_list = Vec::new();
            if let Some(arr) = root.get("profiles").and_then(Value::as_array) {
                for entry in arr {
                    if let Some(name) = entry.get("name").and_then(Value::as_str).filter(|s| !s.trim().is_empty()) {
                        let modules = entry.get("modules").and_then(Value::as_object).cloned().unwrap_or_default();
                        loaded_list.push(SavedProfile { name: name.to_owned(), modules });
                    }
                }
            }
            if !loaded_list.is_empty() {
                // The first row is the base profile whatever it is called (it can be renamed, never deleted), so a
                // list without one named "Default" is fine: adding one back would duplicate a renamed Default.
                self.list = loaded_list;
                if let Some(act) = root.get("active").and_then(Value::as_str)
                    && self.list.iter().any(|p| p.name == act)
                {
                    self.active = act.to_owned();
                } else {
                    self.active = self.list[0].name.clone();
                }
                return;
            }
        }
        self.list = vec![SavedProfile { name: "Default".to_owned(), modules: snapshot_modules(mods) }];
        self.active = "Default".to_owned();
    }

    fn save(&self, game_dir: Option<&std::path::Path>) {
        let Some(path) = Self::file(game_dir) else { return };
        let profiles: Vec<Value> = self
            .list
            .iter()
            .map(|p| json!({ "name": p.name, "modules": Value::Object(p.modules.clone()) }))
            .collect();
        let text = serde_json::to_string_pretty(&json!({ "version": 1, "active": self.active, "profiles": profiles })).unwrap_or_default();
        let tmp = path.with_extension("json.tmp");
        let _ = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|_| std::fs::write(&tmp, text))
            .and_then(|_| std::fs::rename(&tmp, &path));
    }

    fn update_active(&mut self, mods: &Modules) {
        let snap = snapshot_modules(mods);
        if let Some(p) = self.list.iter_mut().find(|p| p.name == self.active) {
            p.modules = snap;
        }
    }

    fn switch_to(&mut self, idx: usize, mods: &mut Modules, game_dir: Option<&std::path::Path>) {
        if idx >= self.list.len() {
            return;
        }
        self.update_active(mods);
        let target_name = self.list[idx].name.clone();
        let target_data = self.list[idx].modules.clone();
        self.active = target_name;
        if !target_data.is_empty() {
            apply_modules(mods, &target_data);
            mods.changed();
        }
        self.save(game_dir);
    }

    fn create_from_current(&mut self, mods: &Modules, game_dir: Option<&std::path::Path>) {
        self.update_active(mods);
        let name = (1..)
            .map(|n| format!("Profile {n}"))
            .find(|cand| !self.list.iter().any(|p| p.name.eq_ignore_ascii_case(cand)))
            .unwrap_or_else(|| "Profile".to_owned());
        self.list.push(SavedProfile { name: name.clone(), modules: snapshot_modules(mods) });
        self.active = name;
        self.save(game_dir);
    }

    fn delete(&mut self, idx: usize, mods: &mut Modules, game_dir: Option<&std::path::Path>) {
        if idx == 0 || idx >= self.list.len() {
            return;
        }
        let was_active = self.list[idx].name == self.active;
        self.list.remove(idx);
        if self.renaming == Some(idx) {
            self.renaming = None;
        }
        if was_active {
            self.active = self.list[0].name.clone();
            if !self.list[0].modules.is_empty() {
                apply_modules(mods, &self.list[0].modules);
                mods.changed();
            }
        }
        self.save(game_dir);
    }

    fn commit_rename(&mut self, idx: usize, game_dir: Option<&std::path::Path>) {
        if idx >= self.list.len() {
            self.renaming = None;
            return;
        }
        let typed = self.rename_field.value().trim().to_owned();
        if !typed.is_empty() && !self.list.iter().enumerate().any(|(i, p)| i != idx && p.name.eq_ignore_ascii_case(&typed)) {
            if self.list[idx].name == self.active {
                self.active = typed.clone();
            }
            self.list[idx].name = typed;
            self.save(game_dir);
        }
        self.renaming = None;
    }
}

fn snapshot_modules(mods: &Modules) -> Map<String, Value> {
    let mut root = Map::new();
    for m in &mods.all {
        let mut o = Map::new();
        o.insert("enabled".into(), Value::Bool(m.state.enabled));
        if m.state.favorite {
            o.insert("favorite".into(), Value::Bool(true));
        }
        if let Some(h) = &m.state.hud {
            o.insert("x".into(), json!(h.fx));
            o.insert("y".into(), json!(h.fy));
        }
        let settings: Map<String, Value> = m.state.all_settings().map(|s| (s.id.to_owned(), s.save())).collect();
        o.insert("settings".into(), Value::Object(settings));
        root.insert(m.state.id.to_owned(), Value::Object(o));
    }
    root
}

fn apply_modules(mods: &mut Modules, data: &Map<String, Value>) {
    for m in &mut mods.all {
        let Some(o) = data.get(m.state.id).and_then(Value::as_object) else {
            continue;
        };
        if let Some(on) = o.get("enabled").and_then(Value::as_bool) {
            m.set_enabled(on);
        }
        m.state.favorite = o.get("favorite").and_then(Value::as_bool).unwrap_or(false);
        if let Some(h) = m.state.hud.as_mut() {
            if let Some(x) = o.get("x").and_then(Value::as_f64) {
                h.fx = (x as f32).clamp(0.0, 1.0);
            }
            if let Some(y) = o.get("y").and_then(Value::as_f64) {
                h.fy = (y as f32).clamp(0.0, 1.0);
            }
        }
        if let Some(s) = o.get("settings").and_then(Value::as_object) {
            for st in m.state.all_settings_mut() {
                if let Some(v) = s.get(st.id) {
                    st.load(v);
                }
            }
        }
    }
}

pub struct Menu {
    pub tab: usize,
    chip: usize,
    /// 0 = Default order, 1 = Enabled first, 2 = A-Z.
    sort_mode: u8,
    result_key: String,
    list_view: bool,
    selected: Option<String>,
    search: TextField,
    grid: Scroll,
    detail: Scroll,
    prefs: Scroll,
    profiles: Profiles,
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
    music: MusicView,
    locker: Locker,
    emotes: EmotesView,
    waypoints: super::waypoints_view::WaypointsView,
    in_world: bool,
    who: String,
    /// The instance folder (screenshots and profiles live under it).
    pub game_dir: Option<PathBuf>,
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
    pub fn new_in(game_dir: Option<PathBuf>) -> Self {
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
            sort_mode: 0,
            result_key: String::new(),
            list_view: false,
            selected: None,
            search,
            grid: Scroll::new("grid"),
            detail: Scroll::new("detail"),
            prefs: Scroll::new("prefs"),
            profiles: Profiles::default(),
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
            music: MusicView::new(),
            locker: Locker::new(),
            emotes: EmotesView::new(),
            waypoints: super::waypoints_view::WaypointsView::new(),
            in_world: false,
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
        self.profiles.renaming = None;
        self.first = false;
    }

    /// Esc: closes the innermost thing first. True when it was consumed (the menu stays open).
    pub fn escape(&mut self, ui: &mut Ui) -> bool {
        if self.widgets.escape() {
            return true;
        }
        if self.profiles.renaming.take().is_some() {
            ui.focus = None;
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
        if self.tab == TAB_COSMETICS && self.locker.escape(ui) {
            return true;
        }
        if self.tab == TAB_EMOTES && self.emotes.escape(ui) {
            return true;
        }
        if self.tab == TAB_SHOTS && self.shots.escape() {
            return true;
        }
        if self.tab == TAB_MUSIC && self.music.escape(ui) {
            return true;
        }
        if self.tab == TAB_WAYPOINTS && self.waypoints.escape(ui) {
            return true;
        }
        if self.selected.is_some() {
            self.selected = None;
            self.page_at = ui.now;
            return true;
        }
        false
    }

    /// A click on this page may need the clipboard (Waypoints: Paste coordinates).
    pub fn wants_clipboard(&self) -> bool {
        self.tab == TAB_WAYPOINTS
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
        if t == TAB_EMOTES {
            self.emotes.opened(now);
        }
        if t == TAB_WAYPOINTS {
            self.waypoints.opened();
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
        self.in_world = in_world;
        self.profiles.ensure_loaded(self.game_dir.as_deref(), mods);

        let (w_all, h_all) = (c.width(), c.height());
        let e = ease(clamp01(((ui.now - self.shown_at) / 240.0) as f32));
        if in_world {
            c.fill(0.0, 0.0, w_all, h_all, theme::alpha(0x9C000000, e));
        }

        // Unified dark obsidian window (~1020 x 620)
        let ww = (w_all - 32.0).min(1020.0);
        let wh = (h_all - 32.0).min(620.0);
        let x0 = (w_all - ww) / 2.0;
        let y0 = (h_all - wh) / 2.0 + (1.0 - e) * 14.0;
        c.push_alpha(e);

        c.shadow(x0, y0 + 5.0, ww, wh, 12.0, 26.0, 0x99000000);
        c.round(x0, y0, ww, wh, 12.0, PANEL_BG);
        c.outline(x0, y0, ww, wh, 12.0, 1.0, 0x24FFFFFF);

        // ── Top header bar (54 px) ──
        let hh = 54.0;
        c.fill(x0 + 1.0, y0 + hh, ww - 2.0, 1.0, theme::HAIRLINE);

        // Left: Native logo + NATIVE CLIENT
        if let Some(logo) = self.logo.clone() {
            c.stamp(LOGO_KEY, &logo, x0 + 16.0, y0 + (hh - 26.0) / 2.0, 26.0, 26.0, true, 0xFFFFFFFF);
        }
        let brand_lh = c.line_height(font::BOLD, 13.5);
        c.text(font::BOLD, 13.5, "NATIVE", x0 + 49.0, y0 + (hh - brand_lh) / 2.0, 0xFFFFFFFF);
        let nw = c.text_width(font::BOLD, 13.5, "NATIVE ");
        c.text(font::MEDIUM, 13.5, "CLIENT", x0 + 49.0 + nw, y0 + (hh - brand_lh) / 2.0, 0xFFC4C4CC);

        // Right: Square close button + player avatar
        let close_sz = 28.0;
        let close_x = x0 + ww - 14.0 - close_sz;
        let close_y = y0 + (hh - close_sz) / 2.0;
        let over_close = ui.hover(c, close_x, close_y, close_sz, close_sz);
        let chv = ui.anim_on("menu:top:close#h", over_close, 16.0);
        c.round(close_x, close_y, close_sz, close_sz, 6.0, theme::mix(0x18FFFFFF, 0x55EF4444, chv));
        c.outline(close_x, close_y, close_sz, close_sz, 6.0, 1.0, theme::mix(theme::HAIRLINE, 0x88EF4444, chv));
        c.icon(theme::I_X, 13.0, close_x + close_sz / 2.0, close_y + close_sz / 2.0, theme::mix(theme::TEXT_SECONDARY, 0xFFFFFFFF, chv));
        if over_close {
            ui.cursor_hand = true;
            ui.tip("Close");
        }
        if ui.clicked(c, "menu:top:close", close_x, close_y, close_sz, close_sz) {
            c.pop_alpha();
            return Action::Close;
        }
        let mut right_edge = close_x - 10.0;
        if let Some(who) = who {
            let av = 28.0;
            let ax = right_edge - av;
            let ay = y0 + (hh - av) / 2.0;
            avatar(c, who, ax, ay, av, 6.0);
            c.circle(ax + av - 2.0, ay + av - 2.0, 4.2, 0xFF0B0C10);
            c.circle(ax + av - 2.0, ay + av - 2.0, 3.0, theme::ONLINE);
            if ui.hover(c, ax, ay, av, av) {
                ui.tip(who);
            }
            right_edge = ax - 10.0;
        }

        // Center: Uppercase pill tabs (MODS, COSMETICS, EMOTES, WAYPOINTS, MUSIC, SHOTS, SETTINGS)
        let brand_right = x0 + 178.0;
        let avail_tabs = (right_edge - brand_right).max(200.0);
        let tab_h = 28.0;
        let tab_y = y0 + (hh - tab_h) / 2.0;
        let tab_fs = if avail_tabs < 560.0 { 9.5 } else { 10.5 };
        let tab_pad = if avail_tabs < 560.0 { 16.0 } else { 22.0 };
        let tab_gap = 6.0;
        let widths: Vec<f32> = TOP_ORDER
            .iter()
            .map(|&ti| c.text_width(font::BOLD, tab_fs, TOP_LABELS[ti]) + tab_pad)
            .collect();
        let total_tabs_w: f32 = widths.iter().sum::<f32>() + tab_gap * (TOP_ORDER.len() - 1) as f32;
        let mut tx = (x0 + (ww - total_tabs_w) / 2.0).max(brand_right);
        for (idx, &ti) in TOP_ORDER.iter().enumerate() {
            let tw = widths[idx];
            let on = self.tab == ti;
            let id = format!("menu:toptab:{ti}");
            let over = ui.hover(c, tx, tab_y, tw, tab_h);
            let hv = ui.anim_on(&format!("{id}#h"), over, 14.0);
            let sl = ui.anim_on(&format!("{id}#on"), on, 16.0);
            c.round(tx, tab_y, tw, tab_h, 6.0, theme::mix(theme::mix(0x0EFFFFFF, 0x18FFFFFF, hv), 0x26FFFFFF, sl));
            c.outline(
                tx,
                tab_y,
                tw,
                tab_h,
                6.0,
                1.0,
                theme::mix(theme::alpha(theme::HAIRLINE, 0.7), 0x42FFFFFF, sl.max(hv * 0.5)),
            );
            let lbl = TOP_LABELS[ti];
            let lw = c.text_width(font::BOLD, tab_fs, lbl);
            let lh = c.line_height(font::BOLD, tab_fs);
            c.text(
                font::BOLD,
                tab_fs,
                lbl,
                tx + (tw - lw) / 2.0,
                tab_y + (tab_h - lh) / 2.0,
                theme::mix(theme::mix(theme::TEXT_MUTED, theme::TEXT, hv), 0xFFFFFFFF, sl),
            );
            if over {
                ui.cursor_hand = true;
            }
            if ui.clicked(c, &id, tx, tab_y, tw, tab_h) {
                self.set_tab(ti, ui.now);
            }
            tx += tw + tab_gap;
        }

        // ── Body below header ──
        let body_y = y0 + hh + 1.0;
        let body_h = wh - hh - 1.0;
        let pe = ease(clamp01(((ui.now - self.page_at) / 200.0) as f32));

        // Unified Locker tabs (Cosmetics & Emotes) use the Locker's own category sidebar to avoid double sidebars.
        if self.tab == TAB_COSMETICS || self.tab == TAB_EMOTES {
            let (lx, ly, lw, lh) = (x0 + 14.0, body_y + 12.0, ww - 28.0, body_h - 24.0);
            c.push_alpha(pe);
            if self.tab == TAB_COSMETICS {
                if self.locker.draw(ui, c, &self.who, self.page_at, lx, ly, lw, lh) {
                    self.set_tab(TAB_EMOTES, ui.now);
                }
            } else {
                match self.emotes.draw(ui, c, &self.who, lx, ly, lw, lh, self.in_world) {
                    EmotesAct::Close => action = Action::Close,
                    EmotesAct::SwitchCosmetic(slot) => {
                        self.locker.set_slot(slot, ui.now);
                        self.set_tab(TAB_COSMETICS, ui.now);
                    }
                    EmotesAct::None => {}
                }
            }
            c.pop_alpha();
            c.pop_alpha();
            return action;
        }

        // ── Left Sidebar (~176 px): Profiles + SAVE AS NEW PROFILE + EDIT HUD LAYOUT ──
        let sb_w = SIDEBAR_W.min(ww * 0.22);
        c.fill(x0 + sb_w, body_y, 1.0, body_h, theme::HAIRLINE);
        let sb_action = self.draw_profiles_sidebar(ui, c, mods, x0, body_y, sb_w, body_h);
        if sb_action != Action::None {
            action = sb_action;
        }

        // ── Right Main Content Area ──
        let (mx, my, mw, mh) = (x0 + sb_w + 16.0, body_y + 14.0, ww - sb_w - 32.0, body_h - 28.0);
        match self.tab {
            TAB_MODS => {
                if let Some(id) = self.selected.clone() {
                    c.push_alpha(pe);
                    let a = self.mod_page(ui, c, mods, cfg, &id, x0 + sb_w + 1.0, body_y, ww - sb_w - 1.0, body_h);
                    if a != Action::None {
                        action = a;
                    }
                    c.pop_alpha();
                } else {
                    let bar_h = 30.0;
                    self.draw_mods_bar(ui, c, mx, my, mw, bar_h);
                    let ge = ease(clamp01(((ui.now - self.grid_at) / 200.0) as f32));
                    c.push_alpha(ge);
                    self.mods_grid(ui, c, mods, mx, my + bar_h + 12.0 + (1.0 - ge) * 8.0, mw, mh - bar_h - 12.0);
                    c.pop_alpha();
                }
            }
            TAB_WAYPOINTS => {
                c.push_alpha(pe);
                self.waypoints.draw(ui, c, mx, my + (1.0 - pe) * 8.0, mw, mh);
                c.pop_alpha();
            }
            TAB_MUSIC => {
                c.push_alpha(pe);
                self.music.draw(ui, c, cfg, mx, my + (1.0 - pe) * 8.0, mw, mh);
                c.pop_alpha();
            }
            TAB_SHOTS => {
                c.push_alpha(pe);
                self.shots.draw(ui, c, self.game_dir.as_deref(), mx, my + (1.0 - pe) * 8.0, mw, mh);
                c.pop_alpha();
            }
            _ => {
                c.push_alpha(pe);
                let a = self.settings(ui, c, mods, cfg, who, mx, my + (1.0 - pe) * 8.0, mw, mh);
                if a != Action::None {
                    action = a;
                }
                c.pop_alpha();
            }
        }
        c.pop_alpha();
        action
    }

    /// Left sidebar (~176 px): Profiles list with inline rename/switch/delete, `SAVE AS NEW PROFILE`, and `EDIT HUD LAYOUT`.
    #[allow(clippy::too_many_arguments)]
    fn draw_profiles_sidebar(&mut self, ui: &mut Ui, c: &mut Canvas, mods: &mut Modules, x: f32, y: f32, w: f32, h: f32) -> Action {
        let pad = 10.0;
        let btn_h = 34.0;
        let save_h = 30.0;
        let hud_y = y + h - pad - btn_h;
        let save_y = hud_y - 8.0 - save_h;

        // Scrollable profiles list
        let list_top = y + 10.0;
        let list_h = (save_y - list_top - 10.0).max(40.0);
        let row_h = 34.0;
        let off = self.profiles.scroll.begin(ui, c, x, list_top, w, list_h);
        let mut ry = list_top - off;
        let mut switch_idx = None;
        let mut delete_idx = None;
        let mut commit_idx = None;
        let count = self.profiles.list.len();
        for i in 0..count {
            let name = self.profiles.list[i].name.clone();
            let active = name == self.profiles.active;
            let renaming = self.profiles.renaming == Some(i);
            let id = format!("menu:prof:{i}");
            let over = ui.hover(c, x, ry, w, row_h);
            let hv = ui.anim_on(&format!("{id}#h"), over, 14.0);
            let on = ui.anim_on(&format!("{id}#on"), active, 16.0);
            let bg = theme::mix(theme::alpha(0x14FFFFFF, hv), 0x22FFFFFF, on);
            if bg >> 24 > 0 {
                c.fill(x + 1.0, ry, w - 2.0, row_h, bg);
            }
            if on > 0.05 {
                c.fill(x + 1.0, ry + 5.0, 2.5, row_h - 10.0, theme::alpha(PRIMARY_BLUE, on));
            }
            if renaming {
                let enter = self.profiles.rename_field.draw(ui, c, x + 8.0, ry + 3.0, w - 16.0, row_h - 6.0, "Profile name", 11.5);
                if enter || (ui.pressed && !ui.hover(c, x, ry, w, row_h)) {
                    commit_idx = Some(i);
                }
            } else {
                let can_del = i > 0;
                let icons_w = if over || active { if can_del { 42.0 } else { 24.0 } } else { 8.0 };
                let shown = c.ellipsize(font::SEMIBOLD, 11.5, &name, w - 16.0 - icons_w);
                let lh = c.line_height(font::SEMIBOLD, 11.5);
                c.text(
                    font::SEMIBOLD,
                    11.5,
                    &shown,
                    x + 12.0,
                    ry + (row_h - lh) / 2.0,
                    theme::mix(theme::mix(theme::TEXT_SECONDARY, theme::TEXT_STRONG, hv), 0xFFFFFFFF, on),
                );
                let mut took = false;
                if over || active {
                    let mut ix = x + w - 24.0;
                    if can_del {
                        let over_del = ui.hover(c, ix, ry + 6.0, 20.0, 22.0);
                        c.icon(theme::I_TRASH, 11.0, ix + 10.0, ry + row_h / 2.0, if over_del { 0xFFF87171 } else { theme::TEXT_MUTED });
                        if over_del {
                            ui.cursor_hand = true;
                            ui.tip("Delete profile");
                        }
                        if ui.clicked(c, &format!("{id}:del"), ix, ry + 6.0, 20.0, 22.0) {
                            delete_idx = Some(i);
                            took = true;
                        }
                        ix -= 20.0;
                    }
                    let over_edit = ui.hover(c, ix, ry + 6.0, 20.0, 22.0);
                    c.icon(theme::I_PENCIL, 11.0, ix + 10.0, ry + row_h / 2.0, if over_edit { theme::TEXT_STRONG } else { theme::TEXT_MUTED });
                    if over_edit {
                        ui.cursor_hand = true;
                        ui.tip("Rename profile");
                    }
                    if ui.clicked(c, &format!("{id}:edit"), ix, ry + 6.0, 20.0, 22.0) {
                        self.profiles.renaming = Some(i);
                        self.profiles.rename_field.set(&name);
                        self.profiles.rename_field.select_all();
                        self.profiles.rename_field.focus(ui);
                        ui.focus_claimed = true;
                        took = true;
                    }
                }
                if !took && ui.clicked(c, &id, x, ry, w, row_h) && !active {
                    switch_idx = Some(i);
                }
                if over {
                    ui.cursor_hand = true;
                }
            }
            c.fill(x + 10.0, ry + row_h - 1.0, w - 20.0, 1.0, 0x0CFFFFFF);
            ry += row_h;
        }
        self.profiles.scroll.end(ui, c, count as f32 * row_h);

        let gd = self.game_dir.as_deref();
        if let Some(i) = commit_idx {
            self.profiles.commit_rename(i, gd);
            ui.focus = None;
        }
        if let Some(i) = delete_idx {
            self.profiles.delete(i, mods, gd);
        } else if let Some(i) = switch_idx {
            self.profiles.switch_to(i, mods, gd);
        }

        // Bottom: SAVE AS NEW PROFILE (outlined)
        let bw = w - pad * 2.0;
        let bx = x + pad;
        let over_save = ui.hover(c, bx, save_y, bw, save_h);
        let shv = ui.anim_on("menu:prof:save#h", over_save, 14.0);
        c.round(bx, save_y, bw, save_h, 5.0, theme::mix(0x0AFFFFFF, 0x1CFFFFFF, shv));
        c.outline(bx, save_y, bw, save_h, 5.0, 1.0, theme::mix(0x28FFFFFF, 0x55FFFFFF, shv));
        let stxt = "SAVE AS NEW PROFILE";
        let stw = c.text_width(font::BOLD, 9.0, stxt);
        let slh = c.line_height(font::BOLD, 9.0);
        c.text(font::BOLD, 9.0, stxt, bx + (bw - stw) / 2.0, save_y + (save_h - slh) / 2.0, theme::mix(theme::TEXT_SECONDARY, 0xFFFFFFFF, shv));
        if over_save {
            ui.cursor_hand = true;
        }
        if ui.clicked(c, "menu:prof:save", bx, save_y, bw, save_h) {
            self.profiles.create_from_current(mods, gd);
        }

        // Bottom: EDIT HUD LAYOUT (vibrant blue #4F86F7 primary button)
        let over_hud = ui.hover(c, bx, hud_y, bw, btn_h);
        let hhv = ui.anim_on("menu:sb:hud#h", over_hud, 16.0);
        c.round(bx, hud_y, bw, btn_h, 6.0, theme::mix(PRIMARY_BLUE, 0xFF6698FF, hhv));
        let htxt = "EDIT HUD LAYOUT";
        let htw = c.text_width(font::BOLD, 10.5, htxt);
        let hlh = c.line_height(font::BOLD, 10.5);
        c.text(font::BOLD, 10.5, htxt, bx + (bw - htw) / 2.0, hud_y + (btn_h - hlh) / 2.0, 0xFFFFFFFF);
        if over_hud {
            ui.cursor_hand = true;
        }
        if ui.clicked(c, "menu:sb:hud", bx, hud_y, bw, btn_h) {
            return Action::EditHud;
        }
        Action::None
    }

    /// Mods top bar: colored uppercase category pills (`ALL`, `NEW`, `HUD`, `VISUAL`, `MECHANIC`, `FAV`),
    /// grid/list toggle, sort toggle, and compact search box.
    fn draw_mods_bar(&mut self, ui: &mut Ui, c: &mut Canvas, x: f32, y: f32, w: f32, h: f32) {
        // Right side: Search box + Sort toggle + Grid/List toggle
        let search_w = (w * 0.24).clamp(138.0, 204.0);
        let search_x = x + w - search_w;
        self.draw_search(ui, c, search_x, y + 1.0, search_w, h - 2.0);

        let icon_s = h - 2.0;
        let sort_x = search_x - 6.0 - icon_s;
        let sort_tip = match self.sort_mode {
            1 => "Sort: Enabled first",
            2 => "Sort: A-Z",
            _ => "Sort: Default",
        };
        if bar_icon(ui, c, "menu:mods:sort", sort_x, y + 1.0, icon_s, theme::I_ARROWS_UP, self.sort_mode != 0, sort_tip) {
            self.sort_mode = (self.sort_mode + 1) % 3;
            self.grid.reset();
        }
        let view_x = sort_x - 5.0 - icon_s;
        let view_icon = if self.list_view { theme::I_LIST } else { theme::I_GRID };
        if bar_icon(ui, c, "menu:mods:view", view_x, y + 1.0, icon_s, view_icon, self.list_view, if self.list_view { "List view" } else { "Grid view" }) {
            self.list_view = !self.list_view;
            self.grid.reset();
        }

        // Left side: Colored uppercase category pills
        let pills_right = view_x - 8.0;
        let mut px = x;
        let ph = 26.0;
        let py = y + (h - ph) / 2.0;
        for (i, &name) in CAT_LABELS.iter().enumerate() {
            let pw = (c.text_width(font::BOLD, 10.0, name) + 20.0).max(46.0);
            if px + pw > pills_right {
                break;
            }
            let id = format!("menu:cat:{i}");
            let active = self.chip == i;
            let over = ui.hover(c, px, py, pw, ph);
            let hv = ui.anim_on(&format!("{id}#h"), over, 14.0);
            let on = ui.anim_on(&format!("{id}#on"), active, 16.0);
            let base = CAT_COLORS[i];
            let bg = theme::mix(theme::alpha(base, 0.36 + 0.16 * hv), base, on);
            c.round(px, py, pw, ph, 5.0, bg);
            if on > 0.05 {
                c.outline(px, py, pw, ph, 5.0, 1.0, theme::alpha(0xFFFFFFFF, 0.25 * on));
            }
            let lw = c.text_width(font::BOLD, 10.0, name);
            let lh = c.line_height(font::BOLD, 10.0);
            c.text(font::BOLD, 10.0, name, px + (pw - lw) / 2.0, py + (ph - lh) / 2.0, theme::mix(0xFFE4E4E7, 0xFFFFFFFF, on));
            if over {
                ui.cursor_hand = true;
            }
            if ui.clicked(c, &id, px, py, pw, ph) && self.chip != i {
                self.chip = i;
                self.grid.reset();
                self.grid_at = ui.now;
            }
            px += pw + 6.0;
        }
    }

    fn draw_search(&mut self, ui: &mut Ui, c: &mut Canvas, x: f32, y: f32, w: f32, h: f32) {
        let f = self.search.focused(ui);
        let fa = ui.anim_on("menu:search#f", f, 14.0);
        c.round(x, y, w, h, 6.0, theme::mix(0xFF171920, 0xFF1F222C, fa));
        c.outline(x, y, w, h, 6.0, 1.0, theme::mix(theme::HAIRLINE_STRONG, theme::BORDER_HOVER, fa));
        c.icon(theme::I_SEARCH, 12.0, x + 13.0, y + h / 2.0, theme::TEXT_MUTED);
        let clear = !self.search.is_empty();
        if ui.pressed && ui.hover(c, x, y, w - if clear { 24.0 } else { 0.0 }, h) {
            self.search.focus(ui);
            ui.focus_claimed = true;
        }
        self.search.draw(ui, c, x + 16.0, y, w - if clear { 42.0 } else { 20.0 }, h, "Search...", 11.5);
        if clear && ui.icon_button(c, "menu:search:clear", x + w - 24.0, y + (h - 20.0) / 2.0, 20.0, theme::I_X, None) {
            self.search.clear();
            self.search.focus(ui);
            ui.focus_claimed = true;
        }
    }

    fn in_chip(m: &Module, chip: usize) -> bool {
        match chip {
            0 => true,
            1 => NEW_MODS.contains(&m.state.id),
            2 => m.state.category == super::module::HUD,
            3 => m.state.category == super::module::VISUAL,
            4 => m.state.category == super::module::MECHANIC,
            5 => m.state.favorite,
            _ => true,
        }
    }

    /// `ModuleSearch.matches`.
    pub fn query_matches(m: &Module, q: &str) -> bool {
        let q = normalize(q);
        if q.is_empty() {
            return true;
        }
        let hay = format!("{} {} {}", m.state.id, label(m.state.name), label(m.state.description)).to_lowercase();
        q.split(' ').all(|word| hay.contains(word))
    }

    fn filtered(&self, mods: &Modules) -> Vec<usize> {
        let q = self.search.value();
        let mut out: Vec<usize> = mods
            .all
            .iter()
            .enumerate()
            .filter(|(_, m)| Self::in_chip(m, self.chip) && Self::query_matches(m, &q))
            .map(|(i, _)| i)
            .collect();
        match self.sort_mode {
            1 => out.sort_by(|&a, &b| {
                mods.all[b]
                    .state
                    .enabled
                    .cmp(&mods.all[a].state.enabled)
                    .then_with(|| label(mods.all[a].state.name).to_lowercase().cmp(&label(mods.all[b].state.name).to_lowercase()))
            }),
            2 => out.sort_by(|&a, &b| label(mods.all[a].state.name).to_lowercase().cmp(&label(mods.all[b].state.name).to_lowercase())),
            _ => {}
        }
        out
    }

    // ── Mods Grid (3-column compact horizontal cards) ────────────────────

    #[allow(clippy::too_many_arguments)]
    fn mods_grid(&mut self, ui: &mut Ui, c: &mut Canvas, mods: &mut Modules, x: f32, y: f32, w: f32, h: f32) {
        let list = self.filtered(mods);
        let mut key = format!("{}:{}:{}:{}", normalize(&self.search.value()), self.chip, self.list_view, self.sort_mode);
        for i in &list {
            key.push(':');
            key.push_str(mods.all[*i].state.id);
        }
        if key != self.result_key {
            self.result_key = key;
            self.grid.reset();
        }
        if list.is_empty() {
            if self.chip == 5 && self.search.value().trim().is_empty() {
                empty(c, x, y, w, h.min(260.0), theme::I_HEART, "No favorites yet", "Open a mod and tap the heart to pin it here.");
            } else {
                empty(c, x, y, w, h.min(260.0), theme::I_SEARCH, "No mods found", "Try another search or category.");
            }
            return;
        }
        let gap = 10.0;
        let (cols, card_w, card_h);
        if self.list_view {
            cols = if w > 640.0 { 2 } else { 1 };
            card_w = (w - gap * (cols - 1) as f32) / cols as f32;
            card_h = 56.0;
        } else {
            cols = if w >= 540.0 { 3 } else { 2 };
            card_w = (w - gap * (cols - 1) as f32) / cols as f32;
            card_h = 44.0;
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

    /// Compact 44 px horizontal card: emerald green border when ENABLED, crimson red border when DISABLED.
    /// Left click toggles ON/OFF; vertical divider + gear icon on right opens Settings + Live Preview.
    #[allow(clippy::too_many_arguments)]
    fn card(&mut self, ui: &mut Ui, c: &mut Canvas, mods: &mut Modules, mi: usize, x: f32, y: f32, w: f32, h: f32, index: usize) -> bool {
        let m = &mut mods.all[mi];
        let mod_id = m.state.id;
        let id = format!("menu:card:{mod_id}");
        let e = ease(clamp01(((ui.now - self.grid_at - 15.0 - index as f64 * 10.0) / 220.0) as f32));
        c.push_alpha(e);
        let y = y + (1.0 - e) * 6.0;
        let gear_w = 36.0;
        let main_w = (w - gear_w).max(60.0);

        let over_card = ui.hover(c, x, y, w, h);
        let over_main = ui.hover(c, x, y, main_w, h);
        let over_gear = ui.hover(c, x + main_w, y, gear_w, h);
        let hv = ui.anim_on(&format!("{id}#h"), over_card, 14.0);
        let ghv = ui.anim_on(&format!("{id}#gh"), over_gear, 14.0);
        let on = ui.anim_on(&format!("{id}#on"), m.state.enabled, 14.0);

        // Dark tinted card fill + emerald green (ON) / crimson red (OFF) border
        let fill_off = theme::mix(0xFF130E10, 0xFF1D1316, hv);
        let fill_on = theme::mix(0xFF0E1612, 0xFF14211A, hv);
        c.round(x, y, w, h, 6.0, theme::mix(fill_off, fill_on, on));
        let border = theme::mix(theme::alpha(OFF, 0.65 + 0.25 * hv), theme::alpha(ON, 0.72 + 0.24 * hv), on);
        c.outline(x, y, w, h, 6.0, 1.2, border);

        // Left section: icon + name + optional red `NEW` badge
        let icon_col = theme::mix(theme::mix(0xFF8E8E98, 0xFFD4D4DC, hv), 0xFFFFFFFF, on);
        c.icon(m.state.icon, 16.0, x + 22.0, y + h / 2.0, icon_col);
        let is_new = NEW_MODS.contains(&mod_id);
        let badge_w = if is_new { 28.0 } else { 0.0 };
        let max_name = (main_w - 44.0 - badge_w).max(30.0);
        let name = c.ellipsize(font::SEMIBOLD, 11.5, &label(m.state.name), max_name);
        let lh = c.line_height(font::SEMIBOLD, 11.5);
        let nw = c.text_width(font::SEMIBOLD, 11.5, &name);
        c.text(font::SEMIBOLD, 11.5, &name, x + 38.0, y + (h - lh) / 2.0, theme::mix(0xFFB4B4BE, 0xFFFFFFFF, on.max(hv * 0.6)));
        if is_new {
            c.text(font::BOLD, 7.5, "NEW", x + 38.0 + nw + 4.0, y + 8.0, 0xFFEF4444);
        }

        // Right section: vertical divider + gear icon
        c.fill(x + main_w, y + 7.0, 1.0, h - 14.0, theme::alpha(0xFFFFFFFF, 0.12));
        if ghv > 0.01 {
            c.round(x + main_w + 3.0, y + 4.0, gear_w - 6.0, h - 8.0, 4.0, theme::alpha(0xFFFFFFFF, 0.08 * ghv));
        }
        c.icon(
            theme::I_SETTINGS,
            14.0,
            x + main_w + gear_w / 2.0,
            y + h / 2.0,
            theme::mix(theme::TEXT_MUTED, 0xFFFFFFFF, ghv),
        );

        if over_main || over_gear {
            ui.cursor_hand = true;
        }
        if over_gear {
            ui.tip("Configure settings");
        }

        let open_settings = ui.clicked(c, &format!("{id}:gear"), x + main_w, y, gear_w, h);
        if !open_settings && ui.clicked(c, &id, x, y, main_w, h) {
            let next = !m.state.enabled;
            m.set_enabled(next);
            mods.changed();
        }
        c.pop_alpha();
        open_settings
    }

    #[allow(clippy::too_many_arguments)]
    fn list_row(&mut self, ui: &mut Ui, c: &mut Canvas, mods: &mut Modules, mi: usize, x: f32, y: f32, w: f32, h: f32, index: usize) -> bool {
        let m = &mut mods.all[mi];
        let id = format!("menu:row:{}", m.state.id);
        let e = ease(clamp01(((ui.now - self.grid_at - 15.0 - index as f64 * 12.0) / 240.0) as f32));
        c.push_alpha(e);
        let y = y + (1.0 - e) * 6.0;
        let over = ui.hover(c, x, y, w, h);
        let hv = ui.anim_on(&format!("{id}#h"), over, 14.0);
        let on = ui.anim_on(&format!("{id}#on"), m.state.enabled, 14.0);
        let fill_off = theme::mix(0xFF130E10, 0xFF1C1316, hv);
        let fill_on = theme::mix(0xFF0E1612, 0xFF14211A, hv);
        c.round(x, y, w, h, 8.0, theme::mix(fill_off, fill_on, on));
        let border = theme::mix(theme::alpha(OFF, 0.65), theme::alpha(ON, 0.72), on);
        c.outline(x, y, w, h, 8.0, 1.1, border);
        c.round(x + 10.0, y + 10.0, 36.0, 36.0, 7.0, theme::mix(0x14FFFFFF, 0x2222C55E, on));
        c.icon(m.state.icon, 16.0, x + 28.0, y + 28.0, theme::mix(theme::TEXT_SECONDARY, 0xFFFFFFFF, on));
        let tw = w - 156.0;
        let name = c.ellipsize(font::SEMIBOLD, 12.5, &label(m.state.name), tw);
        c.text(font::SEMIBOLD, 12.5, &name, x + 56.0, y + 10.0, theme::TEXT_STRONG);
        let desc = c.ellipsize(font::REGULAR, 10.5, &label(m.state.description), tw);
        c.text(font::REGULAR, 10.5, &desc, x + 56.0, y + 29.0, theme::TEXT_MUTED);
        let mut fav_changed = heart(ui, c, &format!("{id}:fav"), &mut m.state.favorite, x + w - 96.0, y + 16.0, true);
        let open_gear = ui.icon_button(c, &format!("{id}:gear"), x + w - 68.0, y + 14.0, 28.0, theme::I_SETTINGS, Some("Settings"));
        let enabled = m.state.enabled;
        let click_main = !fav_changed && !open_gear && ui.clicked(c, &id, x, y, w - 102.0, h);
        let toggled = ui.toggle(c, &format!("{id}:t"), x + w - 38.0, y + 18.0, enabled) || click_main;
        if toggled {
            m.set_enabled(!enabled);
            fav_changed = true;
        }
        if fav_changed {
            mods.changed();
        }
        c.pop_alpha();
        open_gear
    }

    // ── A module's page ──────────────────────────────────────────────────

    /// A mod's settings on a dedicated full page: header with back button, enable switch, live preview, settings, and appearance.
    #[allow(clippy::too_many_arguments)]
    fn mod_page(&mut self, ui: &mut Ui, c: &mut Canvas, mods: &mut Modules, cfg: &UiConfig, id: &str, x: f32, y: f32, w: f32, h: f32) -> Action {
        let Some(mi) = mods.all.iter().position(|m| m.state.id == id) else {
            self.selected = None;
            return Action::None;
        };
        let mut action = Action::None;
        let hh = 56.0;
        c.fill(x, y + hh, w, 1.0, theme::HAIRLINE);

        let (back_w, back_h) = (84.0, 32.0);
        let (back_x, back_y) = (x + 16.0, y + (hh - back_h) / 2.0);
        if ui.button(c, "menu:page:back", back_x, back_y, back_w, back_h, "Back", theme::I_ARROW_LEFT, ui::BTN_GHOST) {
            self.selected = None;
            self.page_at = ui.now;
            return Action::None;
        }
        let div_x = back_x + back_w + 12.0;
        c.fill(div_x, y + 14.0, 1.0, hh - 28.0, theme::HAIRLINE);

        let (name, category, icon, enabled, is_hud) = {
            let m = &mods.all[mi];
            (label(m.state.name), m.state.category, m.state.icon, m.state.enabled, m.is_hud())
        };
        let ic = 34.0;
        let (ix, iy) = (div_x + 14.0, y + (hh - ic) / 2.0);
        c.round(ix, iy, ic, ic, 8.0, if enabled { 0xFF192C21 } else { theme::SUBTLE_HOVER });
        c.outline(ix, iy, ic, ic, 8.0, 1.0, if enabled { 0x9922C55E } else { theme::HAIRLINE });
        c.icon(icon, 16.0, ix + ic / 2.0, iy + ic / 2.0, if enabled { 0xFF86EFAC } else { theme::TEXT_STRONG });
        let tx = ix + ic + 10.0;
        c.text(font::SEMIBOLD, 14.5, &name, tx, y + 13.0, theme::TEXT_STRONG);
        c.text(font::MEDIUM, 9.5, &category.to_uppercase(), tx, y + 32.0, theme::TEXT_MUTED);
        let heart_x = tx + c.text_width(font::SEMIBOLD, 14.5, &name) + 10.0;
        if heart(ui, c, "menu:page:fav", &mut mods.all[mi].state.favorite, heart_x, y + 16.0, true) {
            mods.changed();
        }

        let pad = 16.0;
        let (state_w, state_h) = (96.0, 32.0);
        let (state_x, state_y) = (x + w - pad - state_w, y + (hh - state_h) / 2.0);
        if state_button(ui, c, &format!("menu:page:on:{id}"), state_x, state_y, state_w, state_h, enabled, 10.0) {
            mods.all[mi].set_enabled(!enabled);
            mods.changed();
        }
        let mut cur_right = state_x;
        if is_hud {
            let hud_w = 104.0;
            let hud_x = cur_right - 8.0 - hud_w;
            if ui.button(c, "menu:page:hud", hud_x, state_y, hud_w, state_h, "Edit HUD", theme::I_MOVE, ui::BTN_PRIMARY) {
                if !mods.all[mi].state.enabled {
                    mods.all[mi].set_enabled(true);
                    mods.changed();
                }
                action = Action::EditHud;
            }
            cur_right = hud_x;
        }
        let reset_w = 112.0;
        let reset_x = cur_right - 8.0 - reset_w;
        if ui.button(c, "menu:page:reset", reset_x, state_y, reset_w, state_h, "Reset", theme::I_RESET, ui::BTN_GHOST) {
            mods.all[mi].reset_settings();
            mods.changed();
        }

        let (bx, by, bw, bh) = (x + pad, y + hh + 12.0, w - pad * 2.0, h - hh - 24.0);
        let gap = 14.0;
        if !mods.all[mi].has_preview() {
            self.settings_panel(ui, c, mods, cfg, mi, bx, by, bw, bh);
        } else if bw >= 580.0 {
            let lw = (bw * 0.56).clamp(320.0, 520.0).min(bw - 190.0).max(300.0);
            self.settings_panel(ui, c, mods, cfg, mi, bx, by, lw, bh);
            self.stage(ui, c, mods, mi, bx + lw + gap, by, bw - lw - gap, bh);
        } else {
            let stage_h = (bh * 0.36).clamp(110.0, 190.0);
            self.stage(ui, c, mods, mi, bx, by, bw, stage_h);
            self.settings_panel(ui, c, mods, cfg, mi, bx, by + stage_h + gap, bw, bh - stage_h - gap);
        }
        action
    }

    /// The live preview: the module drawn large over a game scene, inside its own card.
    #[allow(clippy::too_many_arguments)]
    fn stage(&mut self, ui: &mut Ui, c: &mut Canvas, mods: &mut Modules, mi: usize, x: f32, y: f32, w: f32, h: f32) {
        c.round(x, y, w, h, 10.0, CARD);
        c.outline(x, y, w, h, 10.0, 1.0, theme::HAIRLINE);
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
        let tag = if enabled { "LIVE PREVIEW" } else { "PREVIEW  \u{b7}  OFF" };
        let tw = c.text_width(font::SEMIBOLD, 9.5, tag) + 18.0;
        c.round(ix + 10.0, iy + 10.0, tw, 20.0, 10.0, 0xB0000000);
        c.outline(ix + 10.0, iy + 10.0, tw, 20.0, 10.0, 1.0, theme::HAIRLINE);
        let lh = c.line_height(font::SEMIBOLD, 9.5);
        c.text(font::SEMIBOLD, 9.5, tag, ix + 19.0, iy + 10.0 + (20.0 - lh) / 2.0, theme::TEXT_SECONDARY);
    }

    #[allow(clippy::too_many_arguments)]
    fn settings_panel(&mut self, ui: &mut Ui, c: &mut Canvas, mods: &mut Modules, cfg: &UiConfig, mi: usize, x: f32, y: f32, w: f32, h: f32) {
        c.round(x, y, w, h, 10.0, CARD);
        c.outline(x, y, w, h, 10.0, 1.0, theme::HAIRLINE);
        let pad = 16.0;
        let desc = label(mods.all[mi].state.description);
        let text_w = w - pad * 2.0 - 26.0;
        let desc_h = wrap_height(c, font::REGULAR, 11.5, &desc, text_w, 16.0, 2);
        let info_h = 34.0f32.max(desc_h + 18.0);
        let iy = y + info_h / 2.0;
        c.circle(x + pad + 8.0, iy, 8.0, theme::SUBTLE_HOVER);
        let iw = c.text_width(font::BOLD, 10.0, "i");
        let ih = c.line_height(font::BOLD, 10.0);
        c.text(font::BOLD, 10.0, "i", x + pad + 8.0 - iw / 2.0, iy - ih / 2.0, theme::TEXT_SECONDARY);
        wrap(c, font::REGULAR, 11.5, &desc, x + pad + 26.0, y + (info_h - desc_h) / 2.0, text_w, 16.0, theme::TEXT_SECONDARY, 2);
        c.fill(x + 1.0, y + info_h, w - 2.0, 1.0, theme::HAIRLINE);

        let top = y + info_h + 1.0;
        let ph = h - info_h - 2.0;
        let off = self.detail.begin(ui, c, x + 1.0, top, w - 2.0, ph);
        let mut cy = top - off + 6.0;
        cy = self.section(ui, c, mods, cfg, mi, "Keybind", Part::Keybind, x, cy, w);
        cy = self.section(ui, c, mods, cfg, mi, "Options", Part::Options, x, cy, w);
        cy = self.section(ui, c, mods, cfg, mi, "Appearance", Part::Appearance, x, cy, w);
        self.detail.end(ui, c, cy - (top - off) + 8.0);
    }

    #[allow(clippy::too_many_arguments)]
    fn section(&mut self, ui: &mut Ui, c: &mut Canvas, mods: &mut Modules, cfg: &UiConfig, mi: usize, title: &str, part: Part, x: f32, y: f32, w: f32) -> f32 {
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
        let (pad, hh) = (16.0, 32.0);
        let id = format!("menu:sec:{key}");
        let over = ui.hover(c, x + 6.0, y, w - 12.0, hh);
        let hv = ui.anim_on(&format!("{id}#h"), over, 14.0);
        if hv > 0.01 {
            c.round(x + 6.0, y + 1.0, w - 12.0, hh - 2.0, 6.0, theme::alpha(0xFFFFFFFF, 0.04 * hv));
        }
        let rot = ui.anim_on(&format!("{id}#o"), open, 14.0);
        c.icon(
            if rot > 0.5 { theme::I_CHEVRON_DOWN } else { theme::I_CHEVRON_RIGHT },
            12.0,
            x + pad + 6.0,
            y + hh / 2.0,
            theme::mix(theme::TEXT_MUTED, theme::TEXT_STRONG, hv),
        );
        let lh = c.line_height(font::SEMIBOLD, 12.0);
        c.text(font::SEMIBOLD, 12.0, title, x + pad + 22.0, y + (hh - lh) / 2.0, theme::TEXT_STRONG);
        let count = format!("{} {}", rows.len(), if rows.len() == 1 { "option" } else { "options" });
        let cw = c.text_width(font::REGULAR, 10.0, &count);
        let ch = c.line_height(font::REGULAR, 10.0);
        c.text(font::REGULAR, 10.0, &count, x + w - pad - 8.0 - cw, y + (hh - ch) / 2.0, theme::TEXT_MUTED);
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
        let (rx, rw) = (x + pad + 4.0, w - pad * 2.0 - 12.0);
        for (n, si) in rows.iter().enumerate() {
            if n > 0 {
                c.fill(rx, y, rw, 1.0, theme::HAIRLINE);
            }
            let current = match part {
                Part::Keybind => Some(&mods.all[mi].state.toggle_key),
                Part::Options => mods.all[mi].state.settings.get(*si),
                Part::Appearance => None,
            };
            let warning = current.and_then(|s| super::emotes_module::conflict(mods, s, cfg));
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
        y + 8.0
    }

    // ── Settings ─────────────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn settings(&mut self, ui: &mut Ui, c: &mut Canvas, mods: &mut Modules, cfg: &mut UiConfig, who: Option<&str>, x: f32, y: f32, w: f32, h: f32) -> Action {
        let mut action = Action::None;
        self.s_title.set_bool(cfg.custom_title);
        self.s_notify.set_bool(cfg.notifications);
        self.s_activity.set_bool(cfg.share_activity);
        let scale_held = ui.is_pressing("menu:pref:scale");
        if !scale_held && !self.scale_dragged {
            self.s_scale.set_num(cfg.scale);
        }
        self.s_menu.set_key(cfg.menu_key);
        self.s_chat.set_key(cfg.relay_key);
        self.s_play.set_key(cfg.music_play_key);
        self.s_next.set_key(cfg.music_next_key);
        self.s_prev.set_key(cfg.music_prev_key);

        let wide = w >= 720.0;
        let side_w = if wide { 270.0f32.min(w * 0.34) } else { 0.0 };
        let lw = if wide { w - side_w - 16.0 } else { w };
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
            ry = self.profile_card(ui, c, mods, who, x, ry, cw) + 14.0;
            ry = self.about_card(ui, c, x, ry, cw) + 14.0;
        }
        c.text(
            font::REGULAR,
            10.5,
            "Saved automatically to config/native-ui.json, native-modules.json & native-profiles.json.",
            x + 4.0,
            ry,
            theme::TEXT_MUTED,
        );
        ry += 26.0;
        self.prefs.end(ui, c, ry + off - y);

        if wide {
            let sx = x + w - side_w;
            let sy = self.profile_card(ui, c, mods, who, sx, y, side_w);
            self.about_card(ui, c, sx, sy + 14.0, side_w);
        }

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
                cfg.scale = scale;
                save = true;
            }
        } else if (scale - cfg.scale).abs() > 0.001 {
            cfg.scale = scale;
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

    #[allow(clippy::too_many_arguments)]
    fn group(&mut self, ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, w: f32, icon: u32, title: &str, hint: &str, which: Group) -> f32 {
        let head = 58.0;
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
            c.fill(x + 16.0, cy, w - 32.0, 1.0, theme::HAIRLINE);
            let sid = s.id;
            let rh = Widgets::row_height(s);
            self.widgets.row(ui, c, &format!("menu:pref:{sid}"), x + 16.0, cy, w - 32.0, s, None);
            cy += rh;
        }
        y + h + 14.0
    }

    fn hud_card(&mut self, ui: &mut Ui, c: &mut Canvas, mods: &mut Modules, x: f32, y: f32, w: f32) -> (f32, Action) {
        let mut action = Action::None;
        let h = 58.0 + 70.0 + 14.0;
        card_frame(ui, c, "menu:pref:card:hud", x, y, w, h);
        card_head(c, x, y, w, theme::I_LAYOUT, "HUD", "Where your HUD mods sit on screen");
        let ty = y + 58.0;
        let gap = 10.0;
        let tw = (w - 32.0 - gap * 2.0) / 3.0;
        let th = 70.0;
        if self.tile(ui, c, "menu:pref:edit", x + 16.0, ty, tw, th, theme::I_MOVE, "Edit layout", "Drag and resize", false) {
            action = Action::EditHud;
        }
        if self.tile(ui, c, "menu:pref:positions", x + 16.0 + tw + gap, ty, tw, th, theme::I_RESET, "Reset positions", "Back to defaults", false) {
            mods.reset_positions();
        }
        let label = if self.confirm_reset { "Click to confirm" } else { "Reset all mods" };
        let hint = if self.confirm_reset { "Settings + on/off" } else { "Every setting" };
        if self.tile(ui, c, "menu:pref:reset", x + 16.0 + (tw + gap) * 2.0, ty, tw, th, theme::I_POWER, label, hint, true) {
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
        if self.confirm_reset && ui.pressed && !ui.hover(c, x + 16.0 + (tw + gap) * 2.0, ty, tw, th) {
            self.confirm_reset = false;
        }
        (y + h + 14.0, action)
    }

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
        c.round(x, sy, w, h, 10.0, bg);
        c.outline(x, sy, w, h, 10.0, 1.0, if armed { 0x4DEF4444 } else { theme::mix(theme::HAIRLINE, theme::HAIRLINE_STRONG, hv) });
        let fg = if armed {
            0xFFFCA5A5
        } else if danger {
            theme::mix(theme::TEXT_SECONDARY, 0xFFFCA5A5, hv)
        } else {
            theme::TEXT_STRONG
        };
        c.icon(icon, 14.0, x + 20.0, sy + 20.0, fg);
        let t = c.ellipsize(font::SEMIBOLD, 11.5, title, w - 20.0);
        c.text(font::SEMIBOLD, 11.5, &t, x + 12.0, sy + h - 34.0, fg);
        let hnt = c.ellipsize(font::REGULAR, 10.0, hint, w - 20.0);
        c.text(font::REGULAR, 10.0, &hnt, x + 12.0, sy + h - 18.0, theme::TEXT_MUTED);
        if over {
            ui.cursor_hand = true;
        }
        click
    }

    #[allow(clippy::too_many_arguments)]
    fn profile_card(&mut self, ui: &mut Ui, c: &mut Canvas, mods: &Modules, who: Option<&str>, x: f32, y: f32, w: f32) -> f32 {
        let h = 186.0;
        card_frame(ui, c, "menu:pref:card:me", x, y, w, h);
        glow(c, x, y, w, 82.0, 12.0, 0x10FFFFFF);
        let name = who.unwrap_or("Player");
        let av = 50.0;
        avatar(c, name, x + 16.0, y + 16.0, av, 10.0);
        c.circle(x + 16.0 + av - 4.0, y + 16.0 + av - 4.0, 6.5, 0xFF0B0C10);
        c.circle(x + 16.0 + av - 4.0, y + 16.0 + av - 4.0, 4.5, theme::ONLINE);
        let tx = x + 16.0 + av + 12.0;
        let n = c.ellipsize(font::SEMIBOLD, 15.0, name, w - (tx - x) - 14.0);
        c.text(font::SEMIBOLD, 15.0, &n, tx, y + 20.0, theme::TEXT_STRONG);
        let status = c.ellipsize(font::REGULAR, 10.5, "Native Client \u{b7} Rust Edition", w - (tx - x) - 14.0);
        c.text(font::REGULAR, 10.5, &status, tx, y + 42.0, theme::TEXT_MUTED);
        let on = mods.all.iter().filter(|m| m.state.enabled).count();
        let sy = y + 84.0;
        let sw = (w - 32.0 - 8.0) / 2.0;
        stat(c, x + 16.0, sy, sw, &on.to_string(), &format!("of {} mods on", mods.all.len()));
        stat(c, x + 16.0 + sw + 8.0, sy, sw, &self.profiles.list.len().to_string(), "profiles");
        let by = y + h - 16.0 - 32.0;
        if ui.button(c, "menu:pref:locker", x + 16.0, by, w - 32.0, 32.0, "Open locker", theme::I_SHIRT, ui::BTN_PRIMARY) {
            self.set_tab(TAB_COSMETICS, ui.now);
        }
        y + h
    }

    fn about_card(&mut self, ui: &mut Ui, c: &mut Canvas, x: f32, y: f32, w: f32) -> f32 {
        let h = 108.0;
        card_frame(ui, c, "menu:pref:card:about", x, y, w, h);
        let lg = 32.0;
        if let Some(logo) = self.logo.clone() {
            c.image(&logo, x + 16.0, y + 14.0, lg, lg, [0.0, 0.0, 1.0, 1.0], 0xFFFFFFFF);
        }
        c.text(font::SEMIBOLD, 13.5, "Native Client", x + 16.0 + lg + 10.0, y + 15.0, theme::TEXT_STRONG);
        c.text(font::REGULAR, 10.5, "Rust Edition  \u{b7}  Minecraft 26.3", x + 16.0 + lg + 10.0, y + 34.0, theme::TEXT_MUTED);
        c.fill(x + 16.0, y + 60.0, w - 32.0, 1.0, theme::HAIRLINE);
        let t = c.ellipsize(font::REGULAR, 10.5, "Updates install from the launcher. playnative.net", w - 32.0);
        c.text(font::REGULAR, 10.5, &t, x + 16.0, y + 76.0, theme::TEXT_SECONDARY);
        y + h
    }
}

#[derive(Clone, Copy)]
enum Group {
    General,
    Keys,
}

#[allow(clippy::too_many_arguments)]
fn bar_icon(ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, s: f32, icon: u32, active: bool, tip: &str) -> bool {
    let over = ui.hover(c, x, y, s, s);
    let hv = ui.anim_on(&format!("{id}#h"), over, 14.0);
    let on = ui.anim_on(&format!("{id}#on"), active, 14.0);
    c.round(x, y, s, s, 6.0, theme::mix(theme::mix(0xFF171920, 0xFF222530, hv), 0x334F86F7, on));
    c.outline(x, y, s, s, 6.0, 1.0, theme::mix(theme::HAIRLINE, 0x884F86F7, on.max(hv * 0.4)));
    c.icon(icon, 13.0, x + s / 2.0, y + s / 2.0, theme::mix(theme::TEXT_SECONDARY, 0xFFFFFFFF, on.max(hv)));
    if over {
        ui.cursor_hand = true;
        ui.tip(tip);
    }
    ui.clicked(c, id, x, y, s, s)
}

/// `ModuleKeybinds.conflict` for the toggle key of module `mi`.
pub fn conflict(mods: &Modules, mi: usize) -> Option<String> {
    let b = &mods.all[mi].state.toggle_key;
    if b.is_default() {
        return None;
    }
    let Kind::Binding { key, modifiers } = b.kind else { return None };
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
        for s in m.state.all_settings() {
            if !std::ptr::eq(s, b) && matches!(s.kind, Kind::Binding { key: k, modifiers: md } if k == key && md == modifiers) {
                return Some(format!("Conflict: {} / {}", label(m.state.name), label(s.label)));
            }
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

/// The player's face (their Native or Minecraft skin), the initial on a soft tile until it is known.
pub fn avatar(c: &mut Canvas, name: &str, x: f32, y: f32, s: f32, r: f32) {
    if let Some(skin) = super::avatars::own(name) {
        super::avatars::draw(c, Some(&skin), x, y, s, r);
        return;
    }
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
    c.shadow(x, y + 4.0, w, h, 14.0, 22.0, 0x88000000);
    c.round(x, y, w, h, 14.0, PANEL_BG);
    c.outline(x, y, w, h, 14.0, 1.0, theme::HAIRLINE_STRONG);
}

pub fn header(c: &mut Canvas, title: &str, x: f32, y: f32, h: f32) -> f32 {
    let lh = c.line_height(font::SEMIBOLD, 18.0);
    c.text(font::SEMIBOLD, 18.0, title, x + 20.0, y + (h - lh) / 2.0, theme::TEXT_STRONG);
    x + 20.0 + c.text_width(font::SEMIBOLD, 18.0, title)
}

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

#[allow(clippy::too_many_arguments)]
fn state_button(ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, w: f32, h: f32, enabled: bool, fs: f32) -> bool {
    let over = ui.hover(c, x, y, w, h);
    let click = ui.clicked(c, id, x, y, w, h);
    let hv = ui.anim_on(&format!("{id}#h"), over, 16.0);
    let on = ui.anim_on(&format!("{id}#on"), enabled, 14.0);
    let tint = theme::mix(OFF, ON, on);
    c.round(x, y, w, h, 6.0, theme::alpha(tint, 0.16 + 0.1 * hv));
    c.outline(x, y, w, h, 6.0, 1.0, theme::alpha(tint, 0.45 + 0.25 * hv));
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
    c.round(x, y, w, h, 12.0, theme::mix(CARD, CARD_HOVER, hv));
    glow(c, x, y, w, h.min(64.0), 12.0, 0x0AFFFFFF);
    c.outline(x, y, w, h, 12.0, 1.0, theme::mix(theme::HAIRLINE, theme::HAIRLINE_STRONG, hv));
}

fn card_head(c: &mut Canvas, x: f32, y: f32, w: f32, icon: u32, title: &str, hint: &str) {
    let ic = 32.0;
    c.round(x + 16.0, y + 13.0, ic, ic, 8.0, theme::SUBTLE_HOVER);
    c.outline(x + 16.0, y + 13.0, ic, ic, 8.0, 1.0, theme::HAIRLINE);
    c.icon(icon, 15.0, x + 16.0 + ic / 2.0, y + 13.0 + ic / 2.0, theme::TEXT_STRONG);
    let tx = x + 16.0 + ic + 12.0;
    c.text(font::SEMIBOLD, 13.5, title, tx, y + 13.0, theme::TEXT_STRONG);
    let hint = c.ellipsize(font::REGULAR, 10.5, hint, w - (tx - x) - 16.0);
    c.text(font::REGULAR, 10.5, &hint, tx, y + 31.0, theme::TEXT_MUTED);
}

fn stat(c: &mut Canvas, x: f32, y: f32, w: f32, value: &str, label: &str) {
    c.round(x, y, w, 42.0, 8.0, theme::SUBTLE);
    c.text(font::BOLD, 14.5, value, x + 10.0, y + 6.0, theme::TEXT_STRONG);
    let l = c.ellipsize(font::REGULAR, 10.0, label, w - 20.0);
    c.text(font::REGULAR, 10.0, &l, x + 10.0, y + 24.0, theme::TEXT_MUTED);
}

pub fn disabled(c: &mut Canvas, x: f32, y: f32, w: f32, h: f32, label: &str) {
    c.round(x, y, w, h, 8.0, theme::SUBTLE);
    c.outline(x, y, w, h, 8.0, 1.0, theme::HAIRLINE);
    let tw = c.text_width(font::SEMIBOLD, 12.0, label);
    let lh = c.line_height(font::SEMIBOLD, 12.0);
    c.text(font::SEMIBOLD, 12.0, label, x + (w - tw) / 2.0, y + (h - lh) / 2.0, theme::TEXT_MUTED);
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
    // The body wraps (up to three lines, the last one shortened) instead of being cut to one line.
    let lines = super::common::wrap(c, font::REGULAR, 11.5, body, (w - 48.0).max(60.0));
    let shown = lines.len().min(3);
    for (i, line) in lines.iter().take(shown).enumerate() {
        let line = if i + 1 == shown && lines.len() > shown {
            c.ellipsize(font::REGULAR, 11.5, &format!("{line} …"), w - 24.0)
        } else {
            line.clone()
        };
        let lw = c.text_width(font::REGULAR, 11.5, &line);
        c.text(font::REGULAR, 11.5, &line, x + (w - lw) / 2.0, cy + 54.0 + i as f32 * 17.0, theme::TEXT_MUTED);
    }
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
        let mut batch = horizon_client::hud::HudBatch::new([1280, 760], horizon_client::hud::GuiScale { requested: 0, unicode: false });
        c.finish(&mut batch);
        assert!(!batch.vertices.is_empty());
    }
}
