//! The Native multiplayer screen: the port of native-mod's `ServersView`, laid out like vanilla's so it is at
//! home from the first click: the title on top, one server list in the middle and vanilla's two rows of buttons at
//! the bottom (Join Server · Direct Connection · Add Server, then Edit · Delete · Refresh · Back).
//!
//! The list holds, top to bottom: the servers Native features (`/v1/site/spotlight`, pinned), the servers the
//! Horizon project partners with (its official server and partners, which the game adds to the player's list) and
//! the player's own servers.dat. Every row pings live. Click selects, double-click joins; hovering a row's icon
//! shows vanilla's play and move arrows. Add, Edit, Direct Connection and Delete open the game's own screens,
//! which Native draws as its own dialogs over this list (servers_dialogs.rs).
//!
//! Keyboard: Up / Down select, Shift+Up / Down move a server, Enter joins, Del removes. Typing anywhere filters the
//! list; Ctrl+F search, Ctrl+N add, Ctrl+D direct connect, Ctrl+E edit, Ctrl+R refresh.

use super::net;
use super::scroll::Scroll;
use super::textfield::TextField;
use super::title_view::TitleAct;
use super::ui::{self, Ui};
use crate::native_ui::{Canvas, Image, font, theme};
use horizon_client::multiplayer::{OFFICIAL_SERVER, PARTNER_SERVERS, Ping};
use horizon_client::title::{Mode, Title};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const ROW_H: f32 = 72.0;
const ICON: f32 = 56.0;
const BTN_H: f32 = 36.0;
const BTN_GAP: f32 = 8.0;
const LIST_TOP: f32 = 60.0;
const KEY_F: i32 = 70;
const KEY_N: i32 = 78;
const KEY_D: i32 = 68;
const KEY_R: i32 = 82;
const KEY_E: i32 = 69;
/// The game's own list buttons (`multiplayer::Screen::click`).
const DIRECT: u8 = 41;
const ADD: u8 = 42;
const EDIT: u8 = 43;
const DELETE: u8 = 44;
const REFRESH: u8 = 45;
const BACK: u8 = 46;

/// A piece of a MOTD line as the server styled it (colours, bold, underline, strikethrough, obfuscated).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Run {
    pub text: String,
    /// RGB, none for the default colour.
    pub color: Option<u32>,
    pub bold: bool,
    pub underline: bool,
    pub strike: bool,
    pub obfuscated: bool,
}

/// The first two lines of a MOTD as styled runs: the game's own text styling (`§` codes in the text or the JSON
/// component's styles), the way the vanilla list shows it.
fn motd_lines(t: &horizon_client::text::Text) -> Vec<Vec<Run>> {
    let mut lines: Vec<Vec<Run>> = vec![Vec::new()];
    for span in &t.resolve_legacy().spans {
        for (i, part) in span.text.split('\n').enumerate() {
            if i > 0 {
                lines.push(Vec::new());
            }
            if part.is_empty() {
                continue;
            }
            let s = &span.style;
            lines.last_mut().expect("a line").push(Run {
                text: part.to_owned(),
                color: s.color.map(|c| c & 0xFF_FFFF),
                bold: s.bold == Some(true),
                underline: s.underlined == Some(true),
                strike: s.strikethrough == Some(true),
                obfuscated: s.obfuscated == Some(true),
            });
        }
    }
    // Blank lines at the ends say nothing; two lines are shown.
    while lines.last().is_some_and(|l| l.iter().all(|r| r.text.trim().is_empty())) {
        lines.pop();
    }
    lines.into_iter().take(2).collect()
}

/// What a row's server answered.
#[derive(Clone, Debug)]
pub enum Status {
    Pinging,
    Online { motd: Vec<Vec<Run>>, players: Option<(i64, i64)>, ms: u128 },
    Incompatible { motd: Vec<Vec<Run>>, version: String },
    Failed,
}

impl Status {
    fn of(ping: Option<&Ping>) -> Self {
        let lines = motd_lines;
        match ping {
            None | Some(Ping::Pinging) => Status::Pinging,
            Some(Ping::Ok { motd, players, ms, .. }) => Status::Online { motd: lines(motd), players: *players, ms: *ms },
            Some(Ping::Incompatible { motd, version, .. }) => Status::Incompatible { motd: lines(motd), version: version.plain() },
            Some(Ping::Failed(_)) => Status::Failed,
        }
    }
}

/// One row of the player's servers.dat, as the game's list holds it.
#[derive(Clone, Debug)]
pub struct Row {
    pub name: String,
    pub address: String,
    /// Its place in servers.dat.
    pub index: usize,
    pub status: Status,
    pub icon: Option<Arc<Vec<u8>>>,
}

/// The game's multiplayer list, read for one frame.
#[derive(Clone, Debug, Default)]
pub struct ServersInfo {
    pub rows: Vec<Row>,
    pub saves: PathBuf,
}

/// The game's server list when the title shows it (not its Add / Edit / Direct / Delete screens).
pub fn info(title: &Title) -> Option<ServersInfo> {
    let Mode::Multiplayer(horizon_client::multiplayer::Screen::List { servers, pings, icons, .. }) = &title.mode else {
        return None;
    };
    let pings = pings.lock().unwrap_or_else(|e| e.into_inner());
    let icons = icons.lock().unwrap_or_else(|e| e.into_inner());
    let rows = servers
        .iter()
        .enumerate()
        .map(|(index, s)| Row {
            name: s.name.clone(),
            address: s.ip.clone(),
            index,
            status: Status::of(pings.get(&s.ip)),
            icon: icons.get(&s.ip).cloned(),
        })
        .collect();
    Some(ServersInfo { rows, saves: title.saves.clone() })
}

/// An address as the game compares them: no case, no default port.
fn key(address: &str) -> String {
    let a = address.trim().to_ascii_lowercase();
    a.strip_suffix(":25565").map(str::to_owned).unwrap_or(a)
}

/// The servers the Horizon project partners with: its official server and the partners it adds to the list.
fn horizon_partner(address: &str) -> bool {
    let k = key(address);
    key(OFFICIAL_SERVER.1) == k || PARTNER_SERVERS.iter().any(|(_, a)| key(a) == k)
}

// ── Featured by Native ───────────────────────────────────────────────────

#[derive(Clone)]
struct Featured {
    name: String,
    address: String,
    description: String,
    icon: Option<Arc<Image>>,
}

type PingMap = Arc<Mutex<HashMap<String, Ping>>>;
type IconMap = Arc<Mutex<HashMap<String, Arc<Vec<u8>>>>>;

#[derive(Default)]
struct Spotlight {
    list: Vec<Featured>,
    at: Option<Instant>,
    loading: bool,
    pings: PingMap,
    icons: IconMap,
}

fn spotlight_state() -> std::sync::MutexGuard<'static, Spotlight> {
    static S: OnceLock<Mutex<Spotlight>> = OnceLock::new();
    S.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

/// `ServerDirectory.spotlight`: loaded at most every ten minutes (a minute after a failure), then pinged.
fn spotlight(saves: &std::path::Path, force: bool) -> Vec<Featured> {
    let mut s = spotlight_state();
    let due = s.at.is_none_or(|t| t.elapsed() > Duration::from_secs(600));
    if !s.loading && (due || force) {
        s.loading = true;
        let file = horizon_client::multiplayer::servers_file(saves);
        let _ = std::thread::Builder::new().name("Native spotlight".into()).spawn(move || {
            let loaded = load_spotlight();
            let mut s = spotlight_state();
            match loaded {
                Some(list) => {
                    let entries: Vec<_> = list.iter().map(|f| horizon_protocol::servers::ServerEntry::new(&f.name, &f.address)).collect();
                    let (pings, icons): (PingMap, IconMap) = (Arc::default(), Arc::default());
                    horizon_client::multiplayer::ping_all(&entries, &pings, &icons, &file);
                    s.list = list;
                    s.pings = pings;
                    s.icons = icons;
                    s.at = Some(Instant::now());
                }
                None => s.at = Some(Instant::now() - Duration::from_secs(540)),
            }
            s.loading = false;
        });
    }
    s.list.clone()
}

fn load_spotlight() -> Option<Vec<Featured>> {
    let root = net::get_json(&format!("{}/v1/site/spotlight", net::api()), None).ok()?;
    let mut out = Vec::new();
    for o in root.get("servers").and_then(|v| v.as_array()).into_iter().flatten().filter(|o| o.is_object()).take(20) {
        let text = |k: &str| o.get(k).and_then(|v| v.as_str()).unwrap_or("").to_owned();
        let address = text("address").to_ascii_lowercase();
        let valid = !address.is_empty()
            && address.split(':').next().is_some_and(|h| h.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')))
            && address.split(':').nth(1).is_none_or(|p| p.len() <= 5 && p.chars().all(|c| c.is_ascii_digit()));
        if !valid {
            continue;
        }
        let url = text("icon");
        let icon = if url.starts_with("https://") {
            net::get_bytes(&url, None, 512 * 1024).ok().and_then(|b| Image::decode(&b))
        } else {
            None
        };
        out.push(Featured { name: text("name"), address, description: text("description"), icon });
    }
    Some(out)
}

fn featured_status(address: &str) -> (Status, Option<Arc<Vec<u8>>>) {
    let s = spotlight_state();
    let status = Status::of(s.pings.lock().unwrap_or_else(|e| e.into_inner()).get(address));
    let icon = s.icons.lock().unwrap_or_else(|e| e.into_inner()).get(address).cloned();
    (status, icon)
}

// ── the view ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum Badge {
    None,
    Featured,
    Partner,
}

pub struct ServersView {
    scroll: Scroll,
    search: TextField,
    /// Address of the selected row.
    selected: Option<String>,
    last_click: Option<(String, f64)>,
    /// Rows drawn last frame, top to bottom (address, saved index): what Up / Down walk through.
    order: Vec<(String, Option<usize>)>,
    building: Vec<(String, Option<usize>)>,
    reveal: bool,
    content_top: f32,
    view_h: f32,
    last_needle: String,
    opened: bool,
    icons: HashMap<usize, Arc<Image>>,
}

impl Default for ServersView {
    fn default() -> Self {
        let mut search = TextField::new("servers:search");
        search.max_length = 64;
        ServersView {
            scroll: Scroll::new("servers:list"),
            search,
            selected: None,
            last_click: None,
            order: Vec::new(),
            building: Vec::new(),
            reveal: false,
            content_top: 0.0,
            view_h: 0.0,
            last_needle: String::new(),
            opened: false,
            icons: HashMap::new(),
        }
    }
}

pub(super) fn tr(key: &str, fallback: &str) -> String {
    horizon_client::lang::get_or(key, fallback)
}

fn footer(w: f32, h: f32) -> [f32; 5] {
    let list_w = list_width(w);
    let w1 = 200.0f32.min((list_w - BTN_GAP * 2.0) / 3.0);
    let total = w1 * 3.0 + BTN_GAP * 2.0;
    let w2 = (total - BTN_GAP * 3.0) / 4.0;
    let y2 = h - 16.0 - BTN_H;
    [(w - total) / 2.0, y2 - BTN_GAP - BTN_H, y2, w1, w2]
}

fn list_width(w: f32) -> f32 {
    (w - 48.0).min(640.0)
}

fn matches(needle: &str, haystack: &str) -> bool {
    needle.is_empty() || haystack.to_lowercase().contains(needle)
}

impl ServersView {
    /// The screen went away (back to the title, or into a game): it starts fresh next time.
    pub fn closed(&mut self) {
        self.opened = false;
    }

    /// Esc: clears the search first, then goes back.
    pub fn escape(&mut self, ui: &mut Ui) -> Option<TitleAct> {
        if !self.search.is_empty() {
            self.search.clear();
            ui.focus = None;
            return None;
        }
        if ui.focus.is_some() {
            ui.focus = None;
            return None;
        }
        Some(TitleAct::ServerButton { id: BACK, index: None })
    }

    fn selected_saved(&self) -> Option<usize> {
        let sel = self.selected.as_deref()?;
        self.order.iter().find(|(a, _)| a.eq_ignore_ascii_case(sel)).and_then(|(_, i)| *i)
    }

    /// The list as it was, under an open Add / Edit / Direct Connection / Delete dialog (as the Java mod draws them):
    /// it sees no mouse, keys or typing, so only the dialog answers this frame.
    pub fn draw_behind(&mut self, ui: &mut Ui, c: &mut Canvas, info: &ServersInfo) {
        let input = (ui.interactive, ui.down, ui.pressed, ui.released, ui.right_pressed, ui.scroll);
        let typed = (std::mem::take(&mut ui.chars), std::mem::take(&mut ui.keys));
        (ui.interactive, ui.down, ui.pressed, ui.released, ui.right_pressed, ui.scroll) = (false, false, false, false, false, 0.0);
        let _ = self.draw(ui, c, info);
        (ui.interactive, ui.down, ui.pressed, ui.released, ui.right_pressed, ui.scroll) = input;
        (ui.chars, ui.keys) = typed;
    }

    pub fn draw(&mut self, ui: &mut Ui, c: &mut Canvas, info: &ServersInfo) -> Option<TitleAct> {
        if !self.opened {
            self.opened = true;
            self.search.clear();
            self.selected = None;
            self.last_click = None;
            self.scroll.reset();
            ui.focus = None;
            ui.set_anim("servers#in", 0.0);
            spotlight(&info.saves, false);
        }
        let (w, h) = (c.width(), c.height());
        let fade = ui.anim("servers#in", 1.0, 8.0);
        let mut act = None;

        // keyboard first, so the search below sees this frame's typing
        if ui.focus.is_none() && ui.chars.iter().any(|ch| *ch > ' ') {
            self.search.focus(ui);
        }
        if let Some(a) = self.keys(ui) {
            act = Some(a);
        }

        c.fill(0.0, 0.0, w, h, theme::alpha(0xB8000000, fade));
        c.push_alpha(fade);

        let title = tr("multiplayer.title", "Play Multiplayer");
        let tw = c.text_width(font::BOLD, 18.0, &title);
        c.text(font::BOLD, 18.0, &title, (w - tw) / 2.0, 20.0 + (1.0 - fade) * 8.0, theme::TEXT_STRONG);

        // top-left: the search, then the game's own screen
        let search_w = 160.0f32.max(240.0f32.min(w * 0.2));
        let enter = self.search_box(ui, c, 14.0, search_w);
        if ui.icon_button(c, "servers:classic", 14.0 + search_w + 6.0, 14.0, 34.0, theme::I_LIST, Some("Open the vanilla multiplayer screen")) {
            act = Some(TitleAct::ServersClassic);
        }

        let f = footer(w, h);
        let list_bottom = f[1] - 30.0;
        let list_h = 80.0f32.max(list_bottom - LIST_TOP);
        c.fill(0.0, LIST_TOP, w, list_h, 0x8C000000);
        c.fill(0.0, LIST_TOP, w, 1.0, theme::HAIRLINE_STRONG);
        c.fill(0.0, LIST_TOP + list_h - 1.0, w, 1.0, theme::HAIRLINE_STRONG);

        let needle = self.search.value().trim().to_lowercase();
        if needle != self.last_needle {
            self.last_needle = needle.clone();
            self.scroll.target = 0.0;
        }
        self.building.clear();
        let off = self.scroll.begin(ui, c, 0.0, LIST_TOP, w, list_h);
        self.content_top = LIST_TOP - off;
        self.view_h = list_h;
        let rw = list_width(w);
        let rx = (w - rw) / 2.0;
        let mut cy = LIST_TOP + 10.0 - off;
        let saved_keys: Vec<String> = info.rows.iter().map(|r| key(&r.address)).collect();

        let mut sections = 0;
        let mut shown = 0;
        for fs in spotlight(&info.saves, false) {
            // a featured server already in the player's list shows once, in their list
            if saved_keys.contains(&key(&fs.address)) || !matches(&needle, &format!("{} {}", fs.name, fs.address)) {
                continue;
            }
            if shown == 0 {
                cy = label(c, rx, cy, "Featured by Native", None);
                sections += 1;
            }
            shown += 1;
            let (status, pinged) = featured_status(&fs.address);
            let icon = fs.icon.clone().or_else(|| pinged.map(|p| self.icon_image(&p)));
            if let Some(a) = self.row(ui, c, &fs.address, None, &fs.name, icon, &fs.description, &status, Badge::Featured, rx, cy, rw, None) {
                act = Some(a);
            }
            cy += ROW_H + 4.0;
        }

        let count = info.rows.len();
        let mut groups = [(Vec::new(), "Horizon Partnered"), (Vec::new(), "Your servers")];
        for r in &info.rows {
            if matches(&needle, &format!("{} {}", r.name, r.address)) {
                groups[usize::from(!horizon_partner(&r.address))].0.push(r.clone());
            }
        }
        let partners_shown = !groups[0].0.is_empty();
        for (gi, (rows, name)) in groups.iter().enumerate() {
            if gi == 0 && rows.is_empty() {
                continue;
            }
            if gi == 0 {
                cy = label(c, rx, cy, name, Some("from the Horizon project"));
                sections += 1;
            } else if sections > 0 {
                cy = label(c, rx, cy, name, None);
            }
            if gi == 1 && rows.is_empty() {
                let text = if needle.is_empty() {
                    format!("No servers yet. Add one with {}.", tr("selectServer.add", "Add Server"))
                } else {
                    format!("No server matches \u{201c}{}\u{201d}.", self.search.value())
                };
                if !partners_shown || !needle.is_empty() || gi == 1 {
                    let tw = c.text_width(font::MEDIUM, 12.5, &text);
                    c.text(font::MEDIUM, 12.5, &text, (w - tw) / 2.0, cy + 18.0, theme::TEXT_MUTED);
                    cy += 52.0;
                }
            }
            for r in rows {
                let icon = r.icon.as_ref().map(|p| self.icon_image(p));
                let name = if r.name.is_empty() { r.address.clone() } else { r.name.clone() };
                let badge = if gi == 0 { Badge::Partner } else { Badge::None };
                let movable = needle.is_empty();
                if let Some(a) = self.row(ui, c, &r.address, Some(r.index), &name, icon, &r.address, &r.status, badge, rx, cy, rw, movable.then_some(count)) {
                    act = Some(a);
                }
                cy += ROW_H + 4.0;
            }
        }
        let content = cy + 10.0 - self.content_top;
        self.scroll.end(ui, c, content);
        self.reveal = false;
        self.order = std::mem::take(&mut self.building);

        // the game's buttons, in its places
        let can_join = self.selected.as_deref().is_some_and(|s| self.order.iter().any(|(a, _)| a.eq_ignore_ascii_case(s)));
        let mine = self.selected_saved();
        let mut bx = f[0];
        if bottom_button(ui, c, "servers:join", bx, f[1], f[3], &tr("selectServer.select", "Join Server"), if can_join { ui::BTN_PRIMARY } else { ui::BTN_GLASS }, can_join) {
            act = self.selected.clone().map(TitleAct::Join);
        }
        bx += f[3] + BTN_GAP;
        if bottom_button(ui, c, "servers:direct", bx, f[1], f[3], &tr("selectServer.direct", "Direct Connection"), ui::BTN_GLASS, true) {
            act = Some(TitleAct::ServerButton { id: DIRECT, index: None });
        }
        bx += f[3] + BTN_GAP;
        if bottom_button(ui, c, "servers:add", bx, f[1], f[3], &tr("selectServer.add", "Add Server"), ui::BTN_GLASS, true) {
            act = Some(TitleAct::ServerButton { id: ADD, index: None });
        }
        bx = f[0];
        if bottom_button(ui, c, "servers:edit", bx, f[2], f[4], &tr("selectServer.edit", "Edit"), ui::BTN_GLASS, mine.is_some()) {
            act = Some(TitleAct::ServerButton { id: EDIT, index: mine });
        }
        bx += f[4] + BTN_GAP;
        if bottom_button(ui, c, "servers:delete", bx, f[2], f[4], &tr("selectServer.delete", "Delete"), ui::BTN_GLASS, mine.is_some()) {
            act = Some(TitleAct::ServerButton { id: DELETE, index: mine });
        }
        bx += f[4] + BTN_GAP;
        if bottom_button(ui, c, "servers:refresh", bx, f[2], f[4], &tr("selectServer.refresh", "Refresh"), ui::BTN_GLASS, true) {
            spotlight(&info.saves, true);
            act = Some(TitleAct::ServerButton { id: REFRESH, index: None });
        }
        bx += f[4] + BTN_GAP;
        if bottom_button(ui, c, "servers:back", bx, f[2], f[4], &tr("gui.back", "Back"), ui::BTN_GLASS, true) {
            act = Some(TitleAct::ServerButton { id: BACK, index: None });
        }

        // Enter in the search joins the first match (or the selected one if it is still listed)
        if enter && !self.search.is_empty() && !self.order.is_empty() {
            ui.focus = None;
            if !can_join {
                self.selected = Some(self.order[0].0.clone());
            }
            act = self.selected.clone().map(TitleAct::Join);
        }
        c.pop_alpha();
        if act.as_ref().is_some_and(|a| matches!(a, TitleAct::ServerButton { id: BACK, .. } | TitleAct::Join(_) | TitleAct::ServersClassic)) {
            self.opened = false;
        }
        act
    }

    /// The game's 64×64 RGBA server icon as a canvas image, made once per icon.
    fn icon_image(&mut self, rgba: &Arc<Vec<u8>>) -> Arc<Image> {
        let k = Arc::as_ptr(rgba) as usize;
        if let Some(img) = self.icons.get(&k) {
            return img.clone();
        }
        let side = ((rgba.len() / 4) as f32).sqrt() as u32;
        let px = rgba
            .chunks_exact(4)
            .map(|p| u32::from(p[3]) << 24 | u32::from(p[0]) << 16 | u32::from(p[1]) << 8 | u32::from(p[2]))
            .collect();
        let img = Image::new(side.max(1), side.max(1), px);
        if self.icons.len() > 256 {
            self.icons.clear();
        }
        self.icons.insert(k, img.clone());
        img
    }

    fn search_box(&mut self, ui: &mut Ui, c: &mut Canvas, sx: f32, sw: f32) -> bool {
        let (sh, sy) = (34.0, 14.0);
        let focused = self.search.focused(ui);
        let fa = ui.anim_on("servers:search#f", focused, 14.0);
        let has_text = !self.search.is_empty();
        c.round(sx, sy, sw, sh, 10.0, theme::mix(0x59000000, 0x99000000, fa));
        c.outline(sx, sy, sw, sh, 10.0, 1.0, theme::mix(theme::HAIRLINE, theme::BORDER_HOVER, fa));
        c.icon(theme::I_SEARCH, 13.0, sx + 17.0, sy + sh / 2.0, theme::mix(theme::TEXT_MUTED, theme::TEXT_STRONG, fa));
        let enter = self.search.draw(ui, c, sx + 28.0, sy, sw - 28.0 - if has_text { 30.0 } else { 4.0 }, sh, "Search", 12.5);
        if has_text && ui.icon_button(c, "servers:search:clear", sx + sw - 30.0, sy + 4.0, 26.0, theme::I_X, Some("Clear")) {
            self.search.clear();
            self.search.focus(ui);
        }
        enter
    }

    fn keys(&mut self, ui: &mut Ui) -> Option<TitleAct> {
        let typing = ui.focus.is_some();
        let keys = ui.keys.clone();
        for (k, mods) in keys {
            let ctrl = mods & (ui::MOD_CONTROL | ui::MOD_SUPER) != 0;
            let shift = mods & ui::MOD_SHIFT != 0;
            if ctrl && k == KEY_F {
                self.search.focus(ui);
                self.search.select_all();
            } else if ctrl && k == KEY_N {
                return Some(TitleAct::ServerButton { id: ADD, index: None });
            } else if ctrl && k == KEY_D {
                return Some(TitleAct::ServerButton { id: DIRECT, index: None });
            } else if ctrl && k == KEY_R {
                return Some(TitleAct::ServerButton { id: REFRESH, index: None });
            } else if ctrl && k == KEY_E {
                if let Some(i) = self.selected_saved() {
                    return Some(TitleAct::ServerButton { id: EDIT, index: Some(i) });
                }
            } else if shift && !typing && (k == ui::KEY_UP || k == ui::KEY_DOWN) {
                // vanilla: Shift + arrows move the selected server in the list
                if let Some(i) = self.selected_saved()
                    && self.search.is_empty()
                {
                    self.reveal = true;
                    return Some(TitleAct::ServerMove { index: i, up: k == ui::KEY_UP });
                }
            } else if k == ui::KEY_DOWN {
                self.move_selection(1);
            } else if k == ui::KEY_UP {
                self.move_selection(-1);
            } else if k == ui::KEY_PAGE_DOWN {
                self.move_selection(5);
            } else if k == ui::KEY_PAGE_UP {
                self.move_selection(-5);
            } else if !typing && (k == ui::KEY_ENTER || k == ui::KEY_KP_ENTER) {
                if let Some(s) = self.selected.clone() {
                    self.opened = false;
                    return Some(TitleAct::Join(s));
                }
            } else if !typing && k == ui::KEY_HOME {
                self.move_selection(-1000);
            } else if !typing && k == ui::KEY_END {
                self.move_selection(1000);
            } else if !typing && k == ui::KEY_DELETE {
                if let Some(i) = self.selected_saved() {
                    return Some(TitleAct::ServerButton { id: DELETE, index: Some(i) });
                }
            }
        }
        None
    }

    fn move_selection(&mut self, delta: i32) {
        if self.order.is_empty() {
            return;
        }
        let at = self.selected.as_deref().and_then(|s| self.order.iter().position(|(a, _)| a.eq_ignore_ascii_case(s)));
        let last = self.order.len() as i32 - 1;
        let i = match at {
            None if delta > 0 => 0,
            None => last,
            Some(i) => (i as i32 + delta).clamp(0, last),
        };
        self.selected = Some(self.order[i as usize].0.clone());
        self.last_click = None;
        self.reveal = true;
    }

    fn is_selected(&self, address: &str) -> bool {
        self.selected.as_deref().is_some_and(|s| s.eq_ignore_ascii_case(address))
    }

    /// One server, laid out like vanilla's: icon, name, two MOTD lines, players and signal on the right.
    /// `movable`: the list's length, for the player's own servers when they can be moved with the icon's arrows.
    #[allow(clippy::too_many_arguments)]
    fn row(
        &mut self,
        ui: &mut Ui,
        c: &mut Canvas,
        address: &str,
        index: Option<usize>,
        name: &str,
        icon: Option<Arc<Image>>,
        fallback: &str,
        status: &Status,
        badge: Badge,
        x: f32,
        y: f32,
        w: f32,
        movable: Option<usize>,
    ) -> Option<TitleAct> {
        let id = format!("servers:row:{address}");
        let h = ROW_H;
        let over = ui.hover(c, x, y, w, h);
        let hv = ui.anim_on(&format!("{id}#h"), over, 14.0);
        let sel = self.is_selected(address);
        self.building.push((address.to_owned(), index));
        if self.reveal && sel {
            let top = y - self.content_top;
            if top - 12.0 < self.scroll.target {
                self.scroll.target = (top - 12.0).max(0.0);
            } else if top + h + 12.0 > self.scroll.target + self.view_h {
                self.scroll.target = top + h + 12.0 - self.view_h;
            }
        }
        if sel {
            c.round(x, y, w, h, 12.0, 0x1AFFFFFF);
            c.outline(x, y, w, h, 12.0, 1.5, 0xB3FFFFFF);
        } else if hv > 0.01 {
            c.round(x, y, w, h, 12.0, theme::alpha(0x0FFFFFFF, hv));
        }
        let (ix, iy) = (x + 8.0, y + 8.0);
        match &icon {
            Some(img) => c.image(img, ix, iy, ICON, ICON, [0.0, 0.0, 1.0, 1.0], 0xFFFFFFFF),
            None => {
                c.round(ix, iy, ICON, ICON, ICON * 0.2, theme::SUBTLE_HOVER);
                let letters: String = name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect::<String>().to_uppercase();
                let size = ICON * 0.34;
                let lw = c.text_width(font::BOLD, size, &letters);
                let lh = c.line_height(font::BOLD, size);
                c.text(font::BOLD, size, &letters, ix + (ICON - lw) / 2.0, iy + (ICON - lh) / 2.0, theme::TEXT_SECONDARY);
            }
        }

        let right = x + w - 12.0;
        let status_w = self.status(ui, c, status, right, y + 12.0);
        let tx = x + 76.0;
        let text_w = right - status_w - 12.0 - tx;
        let shown = c.ellipsize(font::BOLD, 14.0, name, text_w);
        c.text(font::BOLD, 14.0, &shown, tx, y + 11.0, theme::TEXT_STRONG);
        if badge != Badge::None {
            let bx = tx + c.text_width(font::BOLD, 14.0, &shown) + 8.0;
            let (label, col) = if badge == Badge::Featured { ("Featured", 0xFFFCD34D) } else { ("Horizon Partner", 0xFF93C5FD) };
            let bw = c.text_width(font::BOLD, 9.5, label) + if badge == Badge::Featured { 24.0 } else { 12.0 };
            if bx + bw < tx + text_w {
                c.round(bx, y + 13.0, bw, 17.0, 8.5, theme::alpha(col, 0.14));
                let mut lx = bx + 6.0;
                if badge == Badge::Featured {
                    c.icon(theme::I_CROWN, 10.0, bx + 11.0, y + 21.5, col);
                    lx = bx + 18.0;
                }
                let lh = c.line_height(font::BOLD, 9.5);
                c.text(font::BOLD, 9.5, label, lx, y + 13.0 + (17.0 - lh) / 2.0, col);
            }
        }
        let motd_w = x + w - 12.0 - tx;
        match status {
            Status::Online { motd, .. } | Status::Incompatible { motd, .. } => {
                let fallback_line = [vec![Run { text: fallback.to_owned(), ..Run::default() }]];
                let lines: &[Vec<Run>] = if motd.is_empty() { &fallback_line } else { motd };
                for (i, line) in lines.iter().take(2).enumerate() {
                    motd_line(c, ui.now, line, tx, y + 34.0 + i as f32 * 15.0, motd_w);
                }
            }
            Status::Pinging => {
                c.text(font::MEDIUM, 12.0, &tr("multiplayer.status.pinging", "Pinging..."), tx, y + 35.0, theme::TEXT_MUTED);
            }
            Status::Failed => {
                c.text(font::MEDIUM, 12.0, &tr("multiplayer.status.cannot_connect", "Can't connect to server"), tx, y + 35.0, 0xFFF87171);
            }
        }

        // the row's own click first: the icon's buttons are checked after it and win the press
        let mut act = None;
        if ui.clicked(c, &id, x, y, w, h) {
            let again = self.last_click.as_ref().is_some_and(|(a, at)| a == address && ui.now - at < 400.0);
            if again {
                act = Some(TitleAct::Join(address.to_owned()));
            } else {
                self.selected = Some(address.to_owned());
                self.last_click = Some((address.to_owned(), ui.now));
            }
        }
        if ui.hover(c, ix, iy, ICON, ICON) {
            c.round(ix, iy, ICON, ICON, ICON * 0.2, 0xA6000000);
            let count = movable.zip(index);
            let play_w = if count.is_some() { ICON * 0.6 } else { ICON };
            let over_play = ui.hover(c, ix, iy, play_w, ICON);
            c.icon(theme::I_PLAY, 20.0, ix + play_w / 2.0, iy + ICON / 2.0, if over_play { theme::TEXT_STRONG } else { theme::TEXT_SECONDARY });
            if ui.clicked(c, &format!("{id}:play"), ix, iy, play_w, ICON) {
                act = Some(TitleAct::Join(address.to_owned()));
            }
            if let Some((count, i)) = count {
                let (ax, aw) = (ix + play_w, ICON - play_w);
                if i > 0 {
                    let o = ui.hover(c, ax, iy, aw, ICON / 2.0);
                    c.icon(theme::I_CHEVRON_UP, 16.0, ax + aw / 2.0, iy + ICON * 0.27, if o { theme::TEXT_STRONG } else { theme::TEXT_SECONDARY });
                    if ui.clicked(c, &format!("{id}:up"), ax, iy, aw, ICON / 2.0) {
                        self.selected = Some(address.to_owned());
                        act = Some(TitleAct::ServerMove { index: i, up: true });
                    }
                }
                if i + 1 < count {
                    let o = ui.hover(c, ax, iy + ICON / 2.0, aw, ICON / 2.0);
                    c.icon(theme::I_CHEVRON_DOWN, 16.0, ax + aw / 2.0, iy + ICON * 0.73, if o { theme::TEXT_STRONG } else { theme::TEXT_SECONDARY });
                    if ui.clicked(c, &format!("{id}:down"), ax, iy + ICON / 2.0, aw, ICON / 2.0) {
                        self.selected = Some(address.to_owned());
                        act = Some(TitleAct::ServerMove { index: i, up: false });
                    }
                }
            }
        }
        act
    }

    /// Vanilla's top-right corner of a row: "12/100" and the signal bars. Returns the width used.
    fn status(&mut self, ui: &mut Ui, c: &mut Canvas, st: &Status, right: f32, y: f32) -> f32 {
        let over_bars = ui.hover(c, right - 21.0, y + 1.0, 23.0, 15.0);
        match st {
            Status::Pinging => {
                for i in 0..3 {
                    let t = ((ui.now / 160.0) - f64::from(i) * 0.9).sin() as f32 * 0.5 + 0.5;
                    c.circle(right - 4.0 - (2 - i) as f32 * 7.0, y + 8.0, 2.2, theme::alpha(0xFFFFFFFF, 0.25 + 0.5 * t));
                }
                22.0
            }
            Status::Failed => {
                bars(c, right - 18.0, y + 14.0, None);
                c.icon(theme::I_X, 11.0, right - 9.0, y + 8.0, 0xFFF87171);
                if over_bars {
                    ui.tip(&tr("multiplayer.status.cannot_connect", "Can't connect to server"));
                }
                22.0
            }
            Status::Incompatible { version, .. } => {
                bars(c, right - 18.0, y + 14.0, None);
                let v = c.ellipsize(font::MEDIUM, 12.0, version, 160.0);
                let vw = c.text_width(font::MEDIUM, 12.0, &v);
                c.text(font::MEDIUM, 12.0, &v, right - 26.0 - vw, y + 1.0, 0xFFF87171);
                if over_bars {
                    ui.tip(&tr("multiplayer.status.incompatible", "Incompatible version!"));
                }
                26.0 + vw
            }
            Status::Online { players, ms, .. } => {
                bars(c, right - 18.0, y + 14.0, Some(*ms));
                let text = players.map(|(on, max)| format!("{}/{}", group(on), group(max.max(0)))).unwrap_or_default();
                let pw = c.text_width(font::MEDIUM, 12.0, &text);
                c.text(font::MEDIUM, 12.0, &text, right - 26.0 - pw, y + 1.0, theme::TEXT_SECONDARY);
                if over_bars {
                    ui.tip(&format!("{}: {ms} ms", tr("multiplayer.status.ping", "Ping").replace("%s", "").trim()));
                }
                26.0 + pw
            }
        }
    }
}

/// One MOTD line: each run in the colour and style the server gave it, cut where the row ends. Unstyled text is the
/// UI's grey; `§k` text scrambles; underline and strikethrough are drawn as lines. (There is no italic face: it
/// shows upright.)
fn motd_line(c: &mut Canvas, now: f64, runs: &[Run], x: f32, y: f32, max_w: f32) {
    const SIZE: f32 = 12.0;
    const SCRAMBLE: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789#%&$?!";
    let mut pen = x;
    let end = x + max_w;
    for run in runs {
        let face = if run.bold { font::BOLD } else { font::MEDIUM };
        let color = run.color.map_or(theme::TEXT_SECONDARY, |rgb| 0xFF00_0000 | rgb);
        let mut shown = String::new();
        let mut width = 0.0;
        for (i, ch) in run.text.chars().enumerate() {
            let ch = if run.obfuscated && !ch.is_whitespace() {
                // the game redraws obfuscated characters constantly; here they change a few times a second
                let n = (now / 90.0) as usize + i * 7;
                SCRAMBLE[(n.wrapping_mul(2654435761) >> 7) % SCRAMBLE.len()] as char
            } else {
                ch
            };
            let w = c.text_width(face, SIZE, ch.encode_utf8(&mut [0; 4]));
            if pen + width + w > end {
                break;
            }
            shown.push(ch);
            width += w;
        }
        if shown.is_empty() {
            break;
        }
        c.text(face, SIZE, &shown, pen, y, color);
        if run.underline {
            c.fill(pen, y + SIZE + 1.0, width, 1.0, color);
        }
        if run.strike {
            c.fill(pen, y + SIZE * 0.55, width, 1.0, color);
        }
        pen += width;
        if pen >= end - 0.5 {
            break;
        }
    }
}

/// 12345 → "12,345".
fn group(n: i64) -> String {
    let s = n.abs().to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// Five signal bars, coloured by latency (all dim when the server did not answer).
fn bars(c: &mut Canvas, x: f32, bottom: f32, latency: Option<u128>) {
    let lit = match latency {
        None => 0,
        Some(l) if l < 150 => 5,
        Some(l) if l < 300 => 4,
        Some(l) if l < 600 => 3,
        Some(l) if l < 1000 => 2,
        Some(_) => 1,
    };
    let color = if lit >= 4 { theme::ONLINE } else if lit >= 3 { theme::IDLE } else { theme::DANGER };
    for i in 0..5 {
        let bh = 3.0 + i as f32 * 2.0;
        c.round(x + i as f32 * 4.0, bottom - bh, 2.5, bh, 1.0, if i < lit { color } else { theme::HAIRLINE_STRONG });
    }
}

/// A quiet divider between the parts of the list, with an optional note after it.
fn label(c: &mut Canvas, x: f32, y: f32, text: &str, note: Option<&str>) -> f32 {
    let t = text.to_uppercase();
    c.text(font::SEMIBOLD, 10.0, &t, x + 6.0, y + 6.0, theme::TEXT_MUTED);
    if let Some(note) = note {
        let tw = c.text_width(font::SEMIBOLD, 10.0, &t);
        c.text(font::MEDIUM, 10.0, note, x + 6.0 + tw + 8.0, y + 6.0, theme::alpha(theme::TEXT_MUTED, 0.8));
    }
    y + 26.0
}

/// A bottom button; a disabled one is drawn faded and ignores the mouse, like vanilla's greyed-out buttons.
#[allow(clippy::too_many_arguments)]
pub(super) fn bottom_button(ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, w: f32, label: &str, style: u8, enabled: bool) -> bool {
    let text = c.ellipsize(font::SEMIBOLD, 12.5, label, w - 16.0);
    if enabled {
        return ui.button(c, id, x, y, w, BTN_H, &text, 0, style);
    }
    let was = ui.interactive;
    ui.interactive = false;
    c.push_alpha(0.4);
    ui.button(c, id, x, y, w, BTN_H, &text, 0, ui::BTN_GLASS);
    c.pop_alpha();
    ui.interactive = was;
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizon_partners_are_told_apart_from_the_players_servers() {
        assert!(horizon_partner(OFFICIAL_SERVER.1));
        assert!(horizon_partner(&format!("{}:25565", PARTNER_SERVERS[0].1.to_uppercase())));
        assert!(!horizon_partner("mc.hypixel.net"));
    }

    #[test]
    fn motd_codes_become_styled_runs_and_lines() {
        let t = horizon_client::text::Text::literal("§f  §aHypixel §c§l[1.8]
§nSecond");
        let lines = motd_lines(&t);
        assert_eq!(lines.len(), 2);
        let first = &lines[0];
        assert_eq!(first[0].color, Some(0xFFFFFF));
        assert_eq!((first[1].text.as_str(), first[1].color), ("Hypixel ", Some(0x55FF55)));
        assert_eq!((first[2].text.as_str(), first[2].color, first[2].bold), ("[1.8]", Some(0xFF5555), true));
        assert!(lines[1][0].underline && lines[1][0].text == "Second");
        // no raw code is left in any run
        assert!(lines.iter().flatten().all(|r| !r.text.contains('§')));
    }

    #[test]
    fn player_counts_are_grouped() {
        assert_eq!(group(12345), "12,345");
        assert_eq!(group(100), "100");
        assert_eq!(group(1000000), "1,000,000");
    }
}
