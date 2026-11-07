//! `RelayView`: the in-game Relay chat: friends and groups on the left, the
//! conversation on the right (like the launcher). The conversation itself is
//! in `relay_chat.rs`; this file has the shell, the inbox, the friends home,
//! the profile panel, the right-click menu, the nickname dialog and the
//! chat settings.
#![allow(dead_code)]

use super::avatars;
use super::common::{self, badge};
use super::locker::session_lock;
use super::relay::{self, Friend, Group, Link, Message, Snapshot};
use super::scroll::Scroll;
use super::textfield::TextField;
use super::ui::{self, Ui};
use super::uiconfig::UiConfig;
use horizon_client::native_ui::{Canvas, font, theme};
use std::collections::{HashMap, HashSet};

pub const HOME_TABS: [&str; 4] = ["Online", "All", "Pending", "Blocked"];

/// What the Relay screen asks the game to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RelayAction {
    None,
    Close,
}

pub struct RelayView {
    pub(super) selected: Option<String>,
    /// 0 chats, 1 groups
    pub(super) tab: usize,
    /// Friends home (no chat open): 0 online, 1 all, 2 pending, 3 blocked, 4 add friend.
    pub(super) home_tab: usize,
    pub(super) collapsed: HashSet<String>,
    pub(super) add_field: TextField,
    pub(super) home_scroll: Scroll,
    pub(super) profile_scroll: Scroll,
    /// The friend profile panel on the right of a chat.
    pub(super) profile_open: bool,
    /// Two-click confirm for remove / block: which action is armed and since when.
    pub(super) armed: Option<String>,
    pub(super) armed_at: f64,
    pub(super) search: TextField,
    pub(super) input: TextField,
    pub(super) list_scroll: Scroll,
    pub(super) chat_scrolls: HashMap<String, Scroll>,
    pub(super) drafts: HashMap<String, String>,
    pub(super) first_ids: HashMap<String, String>,
    pub(super) settings_open: bool,
    pub(super) binding_key: bool,
    pub(super) last_count: i64,
    pub(super) open_at_bottom: bool,
    pub(super) user_scrolled_up: bool,
    // Reply / edit / react (the launcher's message actions).
    pub(super) reply_to: Option<Message>,
    pub(super) editing_id: Option<String>,
    pub(super) react_for: Option<String>,
    pub(super) jump_to: Option<String>,
    pub(super) flash_id: Option<String>,
    pub(super) jump_tries: u32,
    pub(super) flash_at: f64,
    pub(super) picker_rect: Option<[f32; 4]>,
    pub(super) toolbar_rect: Option<[f32; 4]>,
    pub(super) pick_row: Option<[f32; 3]>,
    pub(super) jump_rect: Option<[f32; 4]>,
    // Right-click menu on a chat row, nickname dialog.
    pub(super) menu_key: Option<String>,
    pub(super) nick_for: Option<String>,
    pub(super) menu_x: f32,
    pub(super) menu_y: f32,
    pub(super) menu_fresh: bool,
    pub(super) nick: TextField,
    pub(super) row_typing: bool,
    pub(super) row_pinned: bool,
    pub(super) row_dot: u32,
    pub(super) row_check: u8,
    pub(super) row_time: String,
    // Picture viewer and GIF picker.
    pub(super) viewer: Option<std::sync::Arc<super::chat_media::Media>>,
    pub(super) viewer_url: Option<String>,
    pub(super) gif_open: bool,
    pub(super) gif_search: TextField,
    pub(super) gif_scroll: Scroll,
    pub(super) gif_results: std::sync::Arc<std::sync::Mutex<GifState>>,
    pub(super) gif_query: Option<String>,
    pub(super) gif_typed: f64,
    pub(super) gif_serial: u64,
    pub(super) wrap_cache: HashMap<String, (f32, String, std::sync::Arc<Vec<String>>)>,
    pub(super) media_sizes: HashMap<String, (f32, f32)>,
    pub(super) shown_at: f64,
    pub(super) first: bool,
    pub(super) anchor_conv: Option<String>,
    pub(super) anchor_id: Option<String>,
    pub(super) anchor_y: f32,
    pub(super) staged: Option<Staged>,
    pub(super) staged_note: Option<String>,
    pub(super) pasted: std::sync::Arc<std::sync::Mutex<Option<Vec<u8>>>>,
    pub(super) shots_open: bool,
    pub(super) shots: std::sync::Arc<std::sync::Mutex<Option<Vec<(std::path::PathBuf, i64)>>>>,
    pub(super) shot_thumbs: std::sync::Arc<std::sync::Mutex<HashMap<String, std::sync::Arc<horizon_client::native_ui::Image>>>>,
}

/// A picture waiting in the composer (pasted or picked from the screenshots).
pub struct Staged {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
    pub key: String,
}

/// The GIF search the picker waits for.
#[derive(Default)]
pub struct GifState {
    pub serial: u64,
    pub results: Option<Vec<(String, String, String)>>,
    pub loading: bool,
    pub failed: bool,
}

impl Default for RelayView {
    fn default() -> Self {
        Self::new()
    }
}

impl RelayView {
    pub fn new() -> Self {
        let mut input = TextField::new("relay:input");
        input.max_length = 2000;
        input.bare = true;
        let mut search = TextField::new("relay:search");
        search.bare = true;
        let mut add_field = TextField::new("relay:add");
        add_field.bare = true;
        RelayView {
            selected: None,
            tab: 0,
            home_tab: 0,
            collapsed: HashSet::new(),
            add_field,
            home_scroll: Scroll::new("relay-home"),
            profile_scroll: Scroll::new("relay-profile"),
            profile_open: true,
            armed: None,
            armed_at: 0.0,
            search,
            input,
            list_scroll: Scroll::new("relay-list"),
            chat_scrolls: HashMap::new(),
            drafts: HashMap::new(),
            first_ids: HashMap::new(),
            settings_open: false,
            binding_key: false,
            last_count: -1,
            open_at_bottom: true,
            user_scrolled_up: false,
            reply_to: None,
            editing_id: None,
            react_for: None,
            jump_to: None,
            flash_id: None,
            jump_tries: 0,
            flash_at: 0.0,
            picker_rect: None,
            toolbar_rect: None,
            pick_row: None,
            jump_rect: None,
            menu_key: None,
            nick_for: None,
            menu_x: 0.0,
            menu_y: 0.0,
            menu_fresh: false,
            nick: TextField::new("relay:nick"),
            row_typing: false,
            row_pinned: false,
            row_dot: 0,
            row_check: 0,
            row_time: String::new(),
            viewer: None,
            viewer_url: None,
            gif_open: false,
            gif_search: TextField::new("relay:gifs"),
            gif_scroll: Scroll::new("relay-gifs"),
            gif_results: Default::default(),
            gif_query: None,
            gif_typed: 0.0,
            gif_serial: 0,
            wrap_cache: HashMap::new(),
            media_sizes: HashMap::new(),
            shown_at: 0.0,
            first: true,
            anchor_conv: None,
            anchor_id: None,
            anchor_y: 0.0,
            staged: None,
            staged_note: None,
            pasted: Default::default(),
            shots_open: false,
            shots: Default::default(),
            shot_thumbs: Default::default(),
        }
    }

    /// The Relay screen opens (optionally on a conversation): the intro plays and the chat opens at its newest message.
    pub fn opened(&mut self, ui: &mut Ui, select: Option<&str>) {
        self.shown_at = ui.now;
        self.first = false;
        match select {
            Some(key) => self.select(key),
            None => {
                if let Some(key) = self.selected.clone() {
                    relay::open(&key);
                    self.last_count = -1; // Coming back to Relay opens the chat at its newest message, like Discord.
                    self.open_at_bottom = true;
                    self.user_scrolled_up = false;
                }
            }
        }
        self.input.focus(ui);
        ui.focus_claimed = true;
    }

    /// Esc: closes popovers / clears search first; false = close the chat.
    pub fn escape(&mut self, ui: &mut Ui) -> bool {
        // Whatever sits on top goes first: the picture viewer, dialogs, overlays, then the composer's states.
        if self.viewer.take().is_some() {
            self.viewer_url = None;
            return true;
        }
        if self.nick_for.take().is_some() {
            return true;
        }
        if self.menu_key.take().is_some() {
            return true;
        }
        if self.settings_open {
            self.settings_open = false;
            self.binding_key = false;
            return true;
        }
        if self.gif_open {
            self.gif_open = false;
            return true;
        }
        if self.shots_open {
            self.shots_open = false;
            return true;
        }
        if self.react_for.take().is_some() {
            return true;
        }
        if self.editing_id.is_some() {
            self.stop_editing();
            return true;
        }
        if self.reply_to.take().is_some() {
            return true;
        }
        let _ = ui;
        false
    }

    /// Leaves edit mode and puts back what you were typing before you started editing.
    pub(super) fn stop_editing(&mut self) {
        self.editing_id = None;
        let draft = self.selected.as_ref().and_then(|s| self.drafts.get(s)).cloned().unwrap_or_default();
        self.input.set(&draft);
    }

    pub(super) fn save_draft(&mut self) {
        if let Some(s) = &self.selected
            && self.editing_id.is_none()
        {
            self.drafts.insert(s.clone(), self.input.value());
        }
    }

    pub(super) fn forget_draft(&mut self) {
        if let Some(s) = &self.selected {
            self.drafts.remove(s);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(&mut self, ui: &mut Ui, c: &mut Canvas, cfg: &mut UiConfig, in_world: bool) -> RelayAction {
        let (w_all, h_all) = (c.width(), c.height());
        let snap = relay::snapshot();
        let inn = ui.anim("relay#in", 1.0, 9.0);
        if self.first {
            ui.set_anim("relay#in", 0.0);
            self.first = false;
        }
        c.fill(0.0, 0.0, w_all, h_all, theme::alpha(if in_world { 0x8C000000 } else { 0xB8000000 }, inn));
        let (pw, ph) = ((w_all - 32.0).min(1220.0), (h_all - 32.0).min(724.0));
        let (px, py) = ((w_all - pw) / 2.0, (h_all - ph) / 2.0 + (1.0 - inn) * 18.0);
        c.push_alpha(inn);
        c.shadow(px, py + 12.0, pw, ph, 10.0, 44.0, 0xAA000000);
        c.round(px, py, pw, ph, 10.0, 0xFA0A0B0F);
        // The sidebar sits on a slightly darker plate, like the launcher.
        let sbw = 320.0f32.min(pw * 0.32);
        c.push_clip(px, py, sbw, ph);
        c.round(px, py, sbw + 24.0, ph, 10.0, 0xFF07080B);
        c.pop_clip();
        c.outline(px, py, pw, ph, 10.0, 1.0, theme::HAIRLINE_STRONG);
        if self.binding_key {
            if let Some((code, _)) = ui.keys.first().copied() {
                if code != ui::KEY_ESCAPE {
                    cfg.relay_key = code;
                    cfg.save();
                }
                self.binding_key = false;
                ui.keys.clear();
            }
        }
        ui.interactive = !self.settings_open && self.menu_key.is_none() && self.nick_for.is_none();
        let mut action = RelayAction::None;
        if !super::net::signed_in() {
            // Not signed in / sign-in expired: lock the whole page instead of showing stale chats.
            wordmark(c, px + 18.0, py + 19.0);
            session_lock(ui, c, "relay:lock", px, py + 48.0, pw, ph - 48.0);
            if ui.icon_button(c, "relay:close", px + pw - 46.0, py + 14.0, 32.0, theme::I_X, Some("Close (Esc)")) {
                action = RelayAction::Close;
            }
        } else {
            self.sidebar(ui, c, &snap, px, py, sbw, ph);
            c.fill(px + sbw, py, 1.0, ph, theme::HAIRLINE);
            let show_profile = self.profile_open && self.selected.as_deref().is_some_and(|s| s.starts_with("dm:")) && pw - sbw > 640.0;
            let pp = if show_profile { 270.0f32.min((pw - sbw) * 0.36) } else { 0.0 };
            if self.chat(ui, c, &snap, cfg, px + sbw + 1.0, py, pw - sbw - 1.0 - pp, ph) {
                action = RelayAction::Close;
            }
            if pp > 0.0 {
                c.fill(px + pw - pp, py, 1.0, ph, theme::HAIRLINE);
                self.profile_panel(ui, c, &snap, px + pw - pp + 1.0, py, pp - 1.0, ph);
            }
        }
        ui.interactive = true;
        if self.settings_open {
            self.settings(ui, c, cfg, px, py, sbw);
        }
        if self.menu_key.is_some() {
            self.context_menu(ui, c, &snap, w_all, h_all);
        }
        if self.nick_for.is_some() {
            self.nickname_dialog(ui, c, &snap, w_all, h_all);
        }
        if self.viewer.is_some() {
            self.lightbox(ui, c, w_all, h_all);
        }
        c.pop_alpha();
        action
    }

    pub(super) fn select(&mut self, key: &str) {
        if let Some(home) = key.strip_prefix("home:") {
            let _ = home;
            self.go_home(if key == relay::REQUESTS_KEY { 2 } else { 0 });
            return;
        }
        self.save_draft();
        self.selected = Some(key.to_owned());
        self.reply_to = None;
        self.editing_id = None;
        self.react_for = None;
        self.jump_to = None;
        let draft = self.drafts.get(key).cloned().unwrap_or_default();
        self.input.set(&draft);
        self.last_count = -1;
        self.open_at_bottom = true;
        self.user_scrolled_up = false;
        self.gif_open = false;
        relay::open(key);
        self.tab = usize::from(key.starts_with("g:"));
    }

    // ── left column: the launcher's inbox ────────────────────────────────

    pub(super) fn go_home(&mut self, home_tab: usize) {
        self.save_draft();
        self.selected = None;
        self.home_tab = home_tab;
        self.reply_to = None;
        self.editing_id = None;
        self.react_for = None;
        self.jump_to = None;
        self.gif_open = false;
        self.armed = None;
        relay::set_viewing(None);
    }

    #[allow(clippy::too_many_arguments)]
    fn sidebar(&mut self, ui: &mut Ui, c: &mut Canvas, snap: &Snapshot, x: f32, y: f32, w: f32, h: f32) {
        let pad = 16.0;
        let wx = wordmark(c, x + pad + 2.0, y + 19.0);
        let dy = y + 19.0 + c.line_height(font::BOLD, 17.0) / 2.0;
        let live = match snap.link {
            Link::Online => theme::ONLINE,
            Link::Connecting => theme::IDLE,
            _ => 0xFF52525B,
        };
        c.circle(wx + 8.0, dy, 3.0, live);
        if ui.hover(c, wx + 2.0, dy - 8.0, 14.0, 16.0) {
            ui.tip(match snap.link {
                Link::Online => "Realtime connected",
                Link::Connecting => "Connecting",
                _ => "Offline",
            });
        }
        if ui.icon_button(c, "relay:settings", x + w - pad - 28.0, y + 15.0, 28.0, theme::I_SETTINGS, Some("Chat settings")) {
            self.settings_open = !self.settings_open;
        }
        let hbx = x + w - pad - 28.0 - 32.0;
        if self.selected.is_none() {
            c.round(hbx, y + 15.0, 28.0, 28.0, 7.0, theme::SUBTLE_HOVER);
        }
        if ui.icon_button(c, "relay:home", hbx, y + 15.0, 28.0, theme::I_USERS, Some("Friends")) {
            self.go_home(0);
        }
        let mut ty = y + 56.0;
        // Search: icon + bare field in one box.
        let sf = ui.anim_on("relay:search#f", self.search.focused(ui), 14.0);
        c.round(x + pad, ty, w - pad * 2.0, 34.0, 7.0, theme::mix(0xFF0B0C10, 0xFF0E1014, sf));
        c.outline(x + pad, ty, w - pad * 2.0, 34.0, 7.0, 1.0, theme::mix(theme::HAIRLINE, 0x47FFFFFF, sf));
        c.icon(theme::I_SEARCH, 13.0, x + pad + 15.0, ty + 17.0, theme::TEXT_MUTED);
        self.search.draw(ui, c, x + pad + 18.0, ty, w - pad * 2.0 - 18.0, 34.0, "Search conversations", 12.5);
        ty += 46.0;
        // Chats / Groups.
        let tw = (w - pad * 2.0) / 2.0;
        c.round(x + pad, ty, w - pad * 2.0, 34.0, 7.0, 0xFF0C0D11);
        c.outline(x + pad, ty, w - pad * 2.0, 34.0, 7.0, 1.0, theme::HAIRLINE);
        let tab_pos = ui.anim("relay:tab", self.tab as f32, 16.0);
        c.shadow(x + pad + 3.0 + tab_pos * tw, ty + 4.0, tw - 6.0, 28.0, 5.0, 6.0, 0x66000000);
        c.round(x + pad + 3.0 + tab_pos * tw, ty + 3.0, tw - 6.0, 28.0, 5.0, 0xFF1F2027);
        let viewing = snap.viewing.clone();
        let fu: i32 = snap.friends.iter().filter(|f| viewing.as_deref() != Some(format!("dm:{}", f.id).as_str())).map(|f| f.unread).sum();
        let gu: i32 = snap.groups.iter().filter(|g| viewing.as_deref() != Some(format!("g:{}", g.id).as_str())).map(|g| g.unread).sum();
        for i in 0..2 {
            let label = if i == 0 { "Chats" } else { "Groups" };
            let n = if i == 0 { fu } else { gu };
            let bx = x + pad + i as f32 * tw;
            if ui.clicked(c, &format!("relay:tab{i}"), bx, ty, tw, 34.0) {
                self.tab = i;
            }
            let lw = c.text_width(font::SEMIBOLD, 12.0, label) + if n > 0 { 24.0 } else { 0.0 };
            let col = if self.tab == i { theme::TEXT_STRONG } else { theme::TEXT_MUTED };
            let lh = c.line_height(font::SEMIBOLD, 12.0);
            c.text(font::SEMIBOLD, 12.0, label, bx + (tw - lw) / 2.0, ty + 17.0 - lh / 2.0, col);
            if n > 0 {
                badge(c, bx + (tw + lw) / 2.0, ty + 17.0, n);
            }
        }
        ty += 46.0;
        c.fill(x, ty - 1.0, w, 1.0, theme::HAIRLINE);
        ty += 8.0;
        let list_h = y + h - ty - 8.0;
        let q = self.search.value().trim().to_lowercase();
        let off = self.list_scroll.begin(ui, c, x, ty, w, list_h);
        let mut ry = ty - off;
        let rh = 52.0;
        if self.tab == 0 {
            let mut pinned: Vec<&Friend> = Vec::new();
            let mut direct: Vec<&Friend> = Vec::new();
            for f in &snap.friends {
                let sn = snippet(&format!("dm:{}", f.id), snap);
                if !q.is_empty()
                    && !f.display().to_lowercase().contains(&q)
                    && !f.name.to_lowercase().contains(&q)
                    && !sn.as_ref().is_some_and(|s| s.to_lowercase().contains(&q))
                {
                    continue;
                }
                if f.pinned { pinned.push(f) } else { direct.push(f) }
            }
            if !snap.friends_loaded {
                ry = self.section(ui, c, "direct", "Direct messages", 0, x + 8.0, ry, w - 16.0);
                skeleton_rows(c, x + 8.0, ry, w - 16.0, 6);
            } else if snap.friends.is_empty() {
                self.empty_inbox(ui, c, x, ry, w, list_h.min(260.0));
            } else {
                if !pinned.is_empty() {
                    ry = self.section(ui, c, "pinned", "Pinned", pinned.len(), x + 8.0, ry, w - 16.0);
                    if !self.collapsed.contains("pinned") {
                        for f in pinned {
                            ry = self.friend_row(ui, c, snap, f, x + 8.0, ry, w - 16.0, rh, ty, list_h);
                        }
                    }
                    ry += 6.0;
                }
                ry = self.section(ui, c, "direct", "Direct messages", direct.len(), x + 8.0, ry, w - 16.0);
                if !self.collapsed.contains("direct") {
                    if direct.is_empty() {
                        c.text(font::REGULAR, 11.5, if q.is_empty() { "No direct messages" } else { "No matching chats" }, x + 18.0, ry + 6.0, theme::TEXT_MUTED);
                        ry += 30.0;
                    }
                    for f in direct {
                        ry = self.friend_row(ui, c, snap, f, x + 8.0, ry, w - 16.0, rh, ty, list_h);
                    }
                }
            }
        } else {
            let mut pinned: Vec<&Group> = Vec::new();
            let mut rest: Vec<&Group> = Vec::new();
            for g in &snap.groups {
                if !q.is_empty() && !g.name.to_lowercase().contains(&q) {
                    continue;
                }
                if g.pinned { pinned.push(g) } else { rest.push(g) }
            }
            if !snap.groups_loaded {
                ry = self.section(ui, c, "groups", "Groups", 0, x + 8.0, ry, w - 16.0);
                skeleton_rows(c, x + 8.0, ry, w - 16.0, 3);
            } else {
                if !pinned.is_empty() {
                    ry = self.section(ui, c, "gpinned", "Pinned", pinned.len(), x + 8.0, ry, w - 16.0);
                    if !self.collapsed.contains("gpinned") {
                        for g in pinned {
                            ry = self.group_row(ui, c, snap, g, x + 8.0, ry, w - 16.0, rh, ty, list_h);
                        }
                    }
                    ry += 6.0;
                }
                ry = self.section(ui, c, "groups", "Groups", rest.len(), x + 8.0, ry, w - 16.0);
                if !self.collapsed.contains("groups") {
                    if rest.is_empty() {
                        let t = if q.is_empty() { "No groups yet. Create one in the launcher." } else { "No matching groups" };
                        c.text(font::REGULAR, 11.5, t, x + 18.0, ry + 6.0, theme::TEXT_MUTED);
                        ry += 30.0;
                    }
                    for g in rest {
                        ry = self.group_row(ui, c, snap, g, x + 8.0, ry, w - 16.0, rh, ty, list_h);
                    }
                }
            }
        }
        self.list_scroll.end(ui, c, ry + off - ty + 8.0);
    }

    /// Collapsible "v Direct messages ........ 3" header.
    #[allow(clippy::too_many_arguments)]
    fn section(&mut self, ui: &mut Ui, c: &mut Canvas, id: &str, label: &str, count: usize, x: f32, y: f32, w: f32) -> f32 {
        let sh = 26.0;
        let over = ui.hover(c, x, y, w, sh);
        if ui.clicked(c, &format!("relay:sec:{id}"), x, y, w, sh) && !self.collapsed.remove(id) {
            self.collapsed.insert(id.to_owned());
        }
        let col = if over { theme::TEXT_SECONDARY } else { theme::TEXT_MUTED };
        caret(c, x + 12.0, y + sh / 2.0, !self.collapsed.contains(id), col);
        let lh = c.line_height(font::SEMIBOLD, 12.0);
        c.text(font::SEMIBOLD, 12.0, label, x + 22.0, y + (sh - lh) / 2.0, col);
        if count > 0 {
            let n = count.to_string();
            let nw = c.text_width(font::REGULAR, 11.0, &n);
            let nh = c.line_height(font::REGULAR, 11.0);
            c.text(font::REGULAR, 11.0, &n, x + w - 8.0 - nw, y + (sh - nh) / 2.0, theme::alpha(theme::TEXT_MUTED, 0.75));
        }
        y + sh + 2.0
    }

    fn empty_inbox(&mut self, ui: &mut Ui, c: &mut Canvas, x: f32, y: f32, w: f32, h: f32) {
        common::empty(c, x, y - 20.0, w, h, theme::I_USERS, "Nothing here yet", "Add friends by their Native or Minecraft username.");
        let bw = 128.0;
        if ui.button(c, "relay:empty:add", x + (w - bw) / 2.0, y + h - 34.0, bw, 30.0, "Add friend", theme::I_USER_PLUS, ui::BTN_GLASS) {
            self.go_home(4);
            self.add_field.focus(ui);
            ui.focus_claimed = true;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn friend_row(&mut self, ui: &mut Ui, c: &mut Canvas, snap: &Snapshot, f: &Friend, x: f32, ry: f32, w: f32, rh: f32, top: f32, list_h: f32) -> f32 {
        let key = format!("dm:{}", f.id);
        if ry + rh >= top && ry <= top + list_h {
            let sn = snippet(&key, snap);
            let last = relay::peek_last(&key);
            let now = relay::now_ms();
            self.row_typing = !relay::typing(&key).is_empty();
            self.row_pinned = f.pinned;
            self.row_dot = f.dot();
            self.row_time = common::short_time(last.as_ref().map_or(f.last_msg, |l| l.created_at.max(f.last_msg)), now);
            self.row_check = match (&last, &sn) {
                (Some(l), Some(_)) if snap.me_id.as_deref() == Some(l.sender_id.as_str()) && !l.pending && !l.failed && !l.deleted => {
                    if l.read { 2 } else { 1 }
                }
                _ => 0,
            };
            let unread = if snap.viewing.as_deref() == Some(key.as_str()) { 0 } else { f.unread };
            let sub = sn.unwrap_or_else(|| if f.last_msg > 0 { f.line() } else { "No messages".to_owned() });
            if self.row(ui, c, &key, x, ry, w, rh, f.display(), f.skin.as_deref(), &sub, unread, i32::from(f.online)) {
                self.select(&key);
                self.input.focus(ui);
                ui.focus_claimed = true;
            }
        }
        ry + rh + 2.0
    }

    #[allow(clippy::too_many_arguments)]
    fn group_row(&mut self, ui: &mut Ui, c: &mut Canvas, snap: &Snapshot, g: &Group, x: f32, ry: f32, w: f32, rh: f32, top: f32, list_h: f32) -> f32 {
        let key = format!("g:{}", g.id);
        if ry + rh >= top && ry <= top + list_h {
            let who = match &g.last_sender {
                Some(s) if snap.me_name.as_deref() == Some(s.as_str()) => "You: ".to_owned(),
                Some(s) => format!("{s}: "),
                None => String::new(),
            };
            let sub = match &g.last_text {
                None => format!("{} members", g.members),
                Some(t) => format!("{who}{}", if t.is_empty() { "Picture" } else { t }),
            };
            self.row_dot = 0;
            self.row_typing = !relay::typing(&key).is_empty();
            self.row_pinned = g.pinned;
            self.row_time = common::short_time(g.last_at, relay::now_ms());
            self.row_check = 0;
            let unread = if snap.viewing.as_deref() == Some(key.as_str()) { 0 } else { g.unread };
            if self.row(ui, c, &key, x, ry, w, rh, &g.name, None, &sub, unread, -1) {
                self.select(&key);
                self.input.focus(ui);
                ui.focus_claimed = true;
            }
        }
        ry + rh + 2.0
    }

    /// One inbox row like the launcher's ThreadRow. `presence`: 1 online, 0 offline, -1 group (no dot).
    #[allow(clippy::too_many_arguments)]
    fn row(&mut self, ui: &mut Ui, c: &mut Canvas, key: &str, x: f32, y: f32, w: f32, h: f32, title: &str, skin: Option<&str>, sub: &str, unread: i32, presence: i32) -> bool {
        let sel = self.selected.as_deref() == Some(key);
        let over = ui.hover(c, x, y, w, h);
        let click = ui.clicked(c, &format!("row:{key}"), x, y, w, h);
        if over && ui.right_pressed {
            self.menu_key = Some(key.to_owned());
            self.menu_x = ui.mx;
            self.menu_y = ui.my;
            self.menu_fresh = true;
        }
        let hv = ui.anim_on(&format!("row:{key}"), over, 14.0);
        let sv = ui.anim_on(&format!("rowsel:{key}"), sel, 14.0);
        c.round(x, y, w, h, 7.0, theme::mix(theme::alpha(0x0DFFFFFF, hv), 0x10FFFFFF, sv));
        if sv > 0.01 {
            c.outline(x, y, w, h, 7.0, 1.0, theme::alpha(theme::HAIRLINE_STRONG, sv));
            c.round(x, y + 8.0 + (h - 16.0) * (1.0 - sv) / 2.0, 2.0, (h - 16.0) * sv, 1.0, theme::alpha(theme::WHITE, sv));
        }
        let av = 36.0;
        let (ax, ay) = (x + 10.0, y + (h - av) / 2.0);
        let ring = if sel { 0xFF15161A } else { 0xFF07080B };
        if presence == -1 {
            c.round(ax, ay, av, av, 8.0, theme::mix(theme::name_color(Some(title)), 0xFF000000, 0.45));
            c.icon(theme::I_HASH, 17.0, ax + av / 2.0, ay + av / 2.0, 0xFFFFFFFF);
        } else {
            avatars::draw(c, skin, ax, ay, av, 7.0);
            c.circle(ax + av - 1.0, ay + av - 1.0, 5.5, ring);
            let dot = if self.row_dot != 0 { self.row_dot } else if presence == 1 { theme::ONLINE } else { 0xFF52525B };
            c.circle(ax + av - 1.0, ay + av - 1.0, 3.5, dot);
        }
        let tx = ax + av + 10.0;
        let right = x + w - 10.0;
        let time_w = if self.row_time.is_empty() { 0.0 } else { c.text_width(font::REGULAR, 10.0, &self.row_time) };
        let top_y = y + 9.0;
        let title_w = right - tx - time_w - 8.0 - if self.row_pinned { 16.0 } else { 0.0 };
        let shown = c.ellipsize(font::SEMIBOLD, 13.0, title, title_w);
        c.text(font::SEMIBOLD, 13.0, &shown, tx, top_y, if sel { theme::TEXT_STRONG } else { theme::mix(theme::TEXT, theme::TEXT_STRONG, hv) });
        if self.row_pinned {
            let lh = c.line_height(font::SEMIBOLD, 13.0);
            let sw = c.text_width(font::SEMIBOLD, 13.0, &shown);
            c.icon(theme::I_MAP_PIN, 10.0, tx + sw + 10.0, top_y + lh / 2.0, theme::TEXT_MUTED);
        }
        if time_w > 0.0 {
            c.text(font::REGULAR, 10.0, &self.row_time, right - time_w, top_y + 2.0, if unread > 0 { theme::TEXT_STRONG } else { theme::TEXT_MUTED });
        }
        let by = y + 28.0;
        let sub_w = right - tx - if unread > 0 { 26.0 } else { 0.0 } - if self.row_check > 0 { 16.0 } else { 0.0 };
        let lh = c.line_height(font::REGULAR, 11.0);
        if self.row_typing {
            let tw2 = c.text(font::MEDIUM, 11.0, "typing", tx, by, theme::ONLINE);
            ui.dots(c, tx + tw2 + 14.0, by + lh / 2.0, theme::ONLINE);
        } else {
            let s = c.ellipsize(font::REGULAR, 11.0, sub, sub_w);
            let sw2 = c.text(font::REGULAR, 11.0, &s, tx, by, if unread > 0 { theme::TEXT } else { theme::TEXT_MUTED });
            if self.row_check > 0 {
                c.icon(
                    if self.row_check == 2 { theme::I_CHECKS } else { theme::I_CHECK },
                    11.0,
                    tx + sw2 + 9.0,
                    by + lh / 2.0,
                    if self.row_check == 2 { 0xFF60A5FA } else { theme::TEXT_MUTED },
                );
            }
        }
        if unread > 0 {
            badge(c, right, by + lh / 2.0, unread);
        }
        click
    }

    // ── friends home ──────────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    pub(super) fn friends_home(&mut self, ui: &mut Ui, c: &mut Canvas, snap: &Snapshot, x: f32, y: f32, w: f32, h: f32) {
        let bar_h = 58.0;
        let online: Vec<&Friend> = snap.friends.iter().filter(|f| f.online).collect();
        let pending = snap.received.len() + snap.sent.len();
        let counts = [online.len(), snap.friends.len(), pending, snap.blocked.len()];
        // Right side of the bar: tabs, divider, Add Friend (the close button sits after it).
        let right = x + w - 58.0;
        let al = "Add Friend";
        let aw = c.text_width(font::SEMIBOLD, 12.5, al) + 44.0;
        let mut tws = [0.0f32; 4];
        let mut tabs_w = 0.0;
        for i in 0..4 {
            tws[i] = c.text_width(font::SEMIBOLD, 13.0, HOME_TABS[i]) + 22.0 + count_w(c, i, counts[i]);
            tabs_w += tws[i] + 4.0;
        }
        let ax = right - aw;
        let tx0 = ax - 25.0 - tabs_w;
        if tx0 > x + 130.0 {
            c.icon(theme::I_USERS, 17.0, x + 32.0, y + bar_h / 2.0, theme::TEXT_MUTED);
            let lh = c.line_height(font::BOLD, 14.0);
            c.text(font::BOLD, 14.0, "Friends", x + 48.0, y + (bar_h - lh) / 2.0, theme::TEXT_STRONG);
        }
        let mut tx = tx0.max(x + 12.0);
        for i in 0..4 {
            let (bw, bh) = (tws[i], 28.0);
            let by = y + (bar_h - bh) / 2.0;
            let on = self.home_tab == i;
            let over = ui.hover(c, tx, by, bw, bh);
            let hv = ui.anim_on(&format!("relay:home:tab{i}"), over || on, 16.0);
            c.round(tx, by, bw, bh, 7.0, theme::alpha(0xFFFFFFFF, if on { 0.1 } else { 0.05 * hv }));
            let col = if on { theme::TEXT_STRONG } else { theme::mix(theme::TEXT_SECONDARY, 0xFFDDDDDD, hv) };
            let lh = c.line_height(font::SEMIBOLD, 13.0);
            let lw = c.text(font::SEMIBOLD, 13.0, HOME_TABS[i], tx + 11.0, by + (bh - lh) / 2.0, col);
            if i == 2 {
                if counts[i] > 0 {
                    let cw = count_w(c, i, counts[i]);
                    badge(c, tx + 11.0 + lw + 6.0 + cw - 6.0, by + bh / 2.0, counts[i] as i32);
                }
            } else {
                let nh = c.line_height(font::BOLD, 11.0);
                c.text(font::BOLD, 11.0, &counts[i].to_string(), tx + 11.0 + lw + 6.0, by + (bh - nh) / 2.0, col);
            }
            if over {
                ui.cursor_hand = true;
            }
            if ui.clicked(c, &format!("relay:home:tab{i}"), tx, by, bw, bh) {
                self.home_tab = i;
                self.armed = None;
            }
            tx += bw + 4.0;
        }
        c.fill(ax - 13.0, y + bar_h / 2.0 - 9.0, 1.0, 18.0, theme::HAIRLINE_STRONG);
        {
            let (bh, by) = (30.0, y + (bar_h - 30.0) / 2.0);
            let on = self.home_tab == 4;
            let over = ui.hover(c, ax, by, aw, bh);
            let hv = ui.anim_on("relay:home:add", over, 16.0);
            c.round(ax, by, aw, bh, 9.0, if on { 0xFFFFFFFF } else { theme::alpha(0xFFFFFFFF, 0.06 + 0.06 * hv) });
            c.outline(ax, by, aw, bh, 9.0, 1.0, if on { 0xFFFFFFFF } else { theme::alpha(0xFFFFFFFF, 0.12 + 0.1 * hv) });
            let col = if on { 0xFF000000 } else { theme::TEXT_STRONG };
            c.icon(theme::I_USER_PLUS, 13.0, ax + 22.0, by + bh / 2.0, col);
            let lh = c.line_height(font::SEMIBOLD, 12.5);
            c.text(font::SEMIBOLD, 12.5, al, ax + 34.0, by + (bh - lh) / 2.0, col);
            if over {
                ui.cursor_hand = true;
            }
            if ui.clicked(c, "relay:home:add", ax, by, aw, bh) {
                self.home_tab = 4;
                self.add_field.focus(ui);
                ui.focus_claimed = true;
            }
        }
        c.fill(x, y + bar_h, w, 1.0, theme::HAIRLINE);
        // Body.
        let (px, pw) = (x + 28.0, w - 56.0);
        let top = y + bar_h + 1.0;
        let mut by = top + 18.0;
        if let Some((note, is_error)) = &snap.note {
            let col = if *is_error { 0xFFF87171 } else { 0xFF23A55A };
            let nh = 36.0;
            c.round(px, by, pw, nh, 9.0, theme::alpha(col, 0.12));
            c.outline(px, by, pw, nh, 9.0, 1.0, theme::alpha(col, 0.35));
            let t = c.ellipsize(font::MEDIUM, 12.0, note, pw - 60.0);
            let lh = c.line_height(font::MEDIUM, 12.0);
            c.text(font::MEDIUM, 12.0, &t, px + 14.0, by + (nh - lh) / 2.0, theme::mix(col, 0xFFFFFFFF, 0.4));
            by += nh + 14.0;
        }
        if self.armed.is_some() && ui.now - self.armed_at > 3000.0 {
            self.armed = None;
        }
        let view_h = y + h - by;
        let off = self.home_scroll.begin(ui, c, x, by, w, view_h);
        let mut cy = by - off;
        let v_top = by;
        match self.home_tab {
            4 => cy = self.add_panel(ui, c, snap, px, cy, pw),
            2 => {
                cy = home_title(c, &format!("PENDING \u{2014} {pending}"), px, cy);
                if pending == 0 {
                    home_empty(c, x, cy, w, view_h, theme::I_USERS, if snap.requests_loaded { "There are no pending friend requests." } else { "Loading\u{2026}" });
                }
                for r in &snap.received {
                    if visible(cy, 68.0, v_top, view_h) {
                        person_card(ui, c, &format!("req:{}", r.id), px, cy, pw, &r.name, r.skin.as_deref(), 0, "Incoming Friend Request", theme::TEXT_MUTED);
                        let bcy = cy + 34.0;
                        if circle_btn(ui, c, &format!("relay:req:ok:{}", r.id), px + pw - 18.0 - 17.0 - 42.0, bcy, theme::I_CHECK, 0xFF23A55A, "Accept") {
                            relay::respond(&r.id, "accept");
                        }
                        if circle_btn(ui, c, &format!("relay:req:no:{}", r.id), px + pw - 18.0 - 17.0, bcy, theme::I_X, 0xFFF23F43, "Decline") {
                            relay::respond(&r.id, "decline");
                        }
                    }
                    cy += 76.0;
                }
                for r in &snap.sent {
                    if visible(cy, 68.0, v_top, view_h) {
                        person_card(ui, c, &format!("req:{}", r.id), px, cy, pw, &r.name, r.skin.as_deref(), 0, "Outgoing Friend Request", theme::TEXT_MUTED);
                        if ui.button(c, &format!("relay:req:cancel:{}", r.id), px + pw - 18.0 - 92.0, cy + 20.0, 92.0, 28.0, "Cancel", theme::I_X, ui::BTN_GLASS) {
                            relay::respond(&r.id, "cancel");
                        }
                    }
                    cy += 76.0;
                }
            }
            3 => {
                cy = home_title(c, &format!("BLOCKED USERS \u{2014} {}", snap.blocked.len()), px, cy);
                if snap.blocked.is_empty() {
                    home_empty(c, x, cy, w, view_h, theme::I_BAN, if snap.blocked_loaded { "You haven't blocked anyone." } else { "Loading\u{2026}" });
                }
                for b in &snap.blocked {
                    if visible(cy, 68.0, v_top, view_h) {
                        person_card(ui, c, &format!("blk:{}", b.id), px, cy, pw, &b.name, b.skin.as_deref(), 0, "Blocked", theme::TEXT_MUTED);
                        if ui.button(c, &format!("relay:unblock:{}", b.id), px + pw - 18.0 - 92.0, cy + 20.0, 92.0, 28.0, "Unblock", theme::I_CHECK, ui::BTN_GLASS) {
                            relay::unblock(&b.id);
                        }
                    }
                    cy += 76.0;
                }
            }
            tab => {
                let list: Vec<&Friend> = if tab == 0 { online } else { snap.friends.iter().collect() };
                cy = home_title(c, &format!("{} \u{2014} {}", if tab == 0 { "ONLINE" } else { "ALL FRIENDS" }, list.len()), px, cy);
                if list.is_empty() {
                    let t = if !snap.friends_loaded { "Loading\u{2026}" } else if tab == 0 { "No one is online right now." } else { "You have no friends yet." };
                    home_empty(c, x, cy, w, view_h, theme::I_USERS, t);
                }
                for f in list {
                    if visible(cy, 68.0, v_top, view_h) {
                        self.friend_card(ui, c, f, px, cy, pw);
                    }
                    cy += 76.0;
                }
            }
        }
        self.home_scroll.end(ui, c, cy + off - by + 12.0);
    }

    #[allow(clippy::too_many_arguments)]
    fn friend_card(&mut self, ui: &mut Ui, c: &mut Canvas, f: &Friend, x: f32, y: f32, w: f32) {
        let ch = 68.0;
        let playing = f.online && !f.server_address.is_empty();
        let actions_w = 3.0 * 38.0 + 18.0;
        let mut click = ui.clicked(c, &format!("relay:fcard:{}", f.id), x, y, w - actions_w, ch);
        if ui.hover(c, x, y, w - actions_w, ch) {
            ui.cursor_hand = true;
        }
        let line_color = if !f.online {
            theme::TEXT_MUTED
        } else if playing || !f.activity.is_empty() {
            theme::mix(theme::ONLINE, 0xFFFFFFFF, 0.35)
        } else {
            theme::TEXT_SECONDARY
        };
        person_card(ui, c, &format!("f:{}", f.id), x, y, w, f.display(), f.skin.as_deref(), f.dot(), &if f.online { f.line() } else { "Offline".to_owned() }, line_color);
        // "@handle" after the name, like the launcher.
        let tx = x + 16.0 + 40.0 + 14.0;
        let shown = c.ellipsize(font::SEMIBOLD, 14.0, f.display(), w - (tx - x) - 170.0);
        let nw = c.text_width(font::SEMIBOLD, 14.0, &shown);
        if tx + nw + 8.0 + 60.0 < x + w - actions_w {
            let handle = c.ellipsize(font::REGULAR, 12.0, &format!("@{}", f.name), x + w - actions_w - tx - nw - 8.0);
            c.text(font::REGULAR, 12.0, &handle, tx + nw + 8.0, y + 17.0, theme::TEXT_MUTED);
        }
        let mut bx = x + w - 18.0 - 34.0;
        let bcy = y + ch / 2.0;
        let (bk, rk) = (format!("block:{}", f.id), format!("remove:{}", f.id));
        if self.armed.as_deref() == Some(bk.as_str()) {
            c.round(bx, bcy - 17.0, 34.0, 34.0, 17.0, theme::alpha(theme::DANGER, 0.25));
        }
        let tip = if self.armed.as_deref() == Some(bk.as_str()) { "Click again to block" } else { "Block" };
        if ui.icon_button(c, &format!("relay:fcard:block:{}", f.id), bx, bcy - 17.0, 34.0, theme::I_BAN, Some(tip)) {
            if self.armed.as_deref() == Some(bk.as_str()) {
                self.armed = None;
                relay::block(&f.id);
            } else {
                self.armed = Some(bk);
                self.armed_at = ui.now;
            }
        }
        bx -= 38.0;
        if self.armed.as_deref() == Some(rk.as_str()) {
            c.round(bx, bcy - 17.0, 34.0, 34.0, 17.0, theme::alpha(theme::DANGER, 0.25));
        }
        let tip = if self.armed.as_deref() == Some(rk.as_str()) { "Click again to remove" } else { "Remove Friend" };
        if ui.icon_button(c, &format!("relay:fcard:remove:{}", f.id), bx, bcy - 17.0, 34.0, theme::I_TRASH, Some(tip)) {
            if self.armed.as_deref() == Some(rk.as_str()) {
                self.armed = None;
                relay::remove_friend(&f.id);
            } else {
                self.armed = Some(rk);
                self.armed_at = ui.now;
            }
        }
        bx -= 38.0;
        if ui.icon_button(c, &format!("relay:fcard:msg:{}", f.id), bx, bcy - 17.0, 34.0, theme::I_MESSAGE, Some("Message")) {
            click = true;
        }
        if click {
            self.select(&format!("dm:{}", f.id));
            self.input.focus(ui);
            ui.focus_claimed = true;
        }
    }

    /// ADD FRIEND: exact-name request box, plus live player search under it.
    fn add_panel(&mut self, ui: &mut Ui, c: &mut Canvas, snap: &Snapshot, x: f32, y: f32, w: f32) -> f32 {
        c.text(font::BOLD, 15.0, "ADD FRIEND", x, y, theme::TEXT_STRONG);
        c.text(font::REGULAR, 12.5, "You can add friends with their exact Native or Minecraft username.", x, y + 24.0, theme::TEXT_SECONDARY);
        let (fy, fh) = (y + 54.0, 50.0);
        if !ui.focus_claimed && ui.focus.is_none() && !self.settings_open && self.nick_for.is_none() {
            self.add_field.focus(ui);
        }
        let focused = self.add_field.focused(ui);
        let fa = ui.anim_on("relay:addbox#f", focused, 14.0);
        c.round(x, fy, w, fh, 8.0, 0x73000000);
        c.outline(x, fy, w, fh, 8.0, 1.0, theme::mix(0x1AFFFFFF, 0x59FFFFFF, fa));
        let bl = "Send Friend Request";
        let bw = c.text_width(font::BOLD, 12.5, bl) + 32.0;
        let bh = 32.0;
        let (bxx, byy) = (x + w - bw - 9.0, fy + (fh - bh) / 2.0);
        self.add_field.max_length = 32;
        let enter = self.add_field.draw(ui, c, x + 4.0, fy, w - bw - 22.0, fh, "You can add friends with their username", 13.5);
        let mut query = self.add_field.value().trim().to_owned();
        let can = !query.is_empty();
        let over = can && ui.hover(c, bxx, byy, bw, bh);
        let hv = ui.anim_on("relay:addbtn", over, 16.0);
        c.round(bxx, byy, bw, bh, 9.0, if can { theme::mix(0xFFFFFFFF, 0xE0FFFFFF, hv) } else { 0x40FFFFFF });
        let lh = c.line_height(font::BOLD, 12.5);
        c.text(font::BOLD, 12.5, bl, bxx + 16.0, byy + (bh - lh) / 2.0, if can { 0xFF000000 } else { 0x99000000 });
        if over {
            ui.cursor_hand = true;
        }
        let click = ui.clicked(c, "relay:addbtn", bxx, byy, bw, bh);
        if (enter || click) && can {
            relay::send_request(&query);
            self.add_field.set("");
            query.clear();
        }
        relay::search(&query);
        let mut ry = fy + fh + 26.0;
        if query.chars().count() >= 2 {
            ry = home_title(c, "PLAYERS", x, ry);
            if snap.found.is_empty() {
                c.text(font::REGULAR, 12.0, if snap.searching { "Searching\u{2026}" } else { "No Native player with that name." }, x, ry, theme::TEXT_MUTED);
                ry += 28.0;
            }
            for p in &snap.found {
                let friend = snap.friends.iter().any(|f| f.id == p.id);
                let pending = snap.sent.iter().any(|r| r.user_id == p.id);
                let line = if friend { "Already friends" } else if pending { "Request sent" } else { "Native player" };
                person_card(ui, c, &format!("find:{}", p.id), x, ry, w, &p.name, p.skin.as_deref(), 0, line, theme::TEXT_MUTED);
                if !friend && !pending && ui.button(c, &format!("relay:find:{}", p.id), x + w - 18.0 - 76.0, ry + 20.0, 76.0, 28.0, "Add", theme::I_USER_PLUS, ui::BTN_PRIMARY) {
                    relay::send_request(&p.name);
                }
                ry += 76.0;
            }
        }
        ry
    }

    // ── friend profile panel ─────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn profile_panel(&mut self, ui: &mut Ui, c: &mut Canvas, snap: &Snapshot, x: f32, y: f32, w: f32, h: f32) {
        c.push_clip(x, y, w, h);
        c.round(x - 20.0, y, w + 20.0, h, 10.0, 0xFF0C0D11);
        let Some(f) = self.selected.as_deref().and_then(|s| s.strip_prefix("dm:")).and_then(|id| snap.friends.iter().find(|f| f.id == id)).cloned() else {
            common::empty(c, x, y, w, h, theme::I_USER, "Not a friend", "This chat is no longer in your friends list.");
            c.pop_clip();
            return;
        };
        let pad = 18.0;
        let off = self.profile_scroll.begin(ui, c, x, y, w, h);
        let mut ty = y + 22.0 - off;
        let iw = w - pad * 2.0;
        let p = relay::profile(&f.name);
        let plus = p.as_ref().is_some_and(|p| p.badges.iter().any(|b| b == "plus"));
        // Round avatar with the presence dot, like the launcher's profile panel.
        let av = 76.0;
        c.shadow(x + pad, ty + 6.0, av, av, av / 2.0, 22.0, theme::alpha(f.dot(), 0.22));
        avatars::draw_round(c, f.skin.as_deref(), x + pad, ty, av);
        c.circle(x + pad + av - 9.0, ty + av - 9.0, 11.0, 0xFF0C0D11);
        c.circle(x + pad + av - 9.0, ty + av - 9.0, 7.5, f.dot());
        ty += av + 12.0;
        // Name (+ Native+ mark), @handle, presence.
        let display = c.ellipsize(font::BOLD, 19.0, f.display(), iw - if plus { 26.0 } else { 0.0 });
        c.text(font::BOLD, 19.0, &display, x + pad, ty, theme::TEXT_STRONG);
        if plus {
            let mx = x + pad + c.text_width(font::BOLD, 19.0, &display) + 7.0;
            c.icon(theme::I_STAR, 14.0, mx + 8.0, ty + 11.0, theme::ACCENT);
            if ui.hover(c, mx, ty + 3.0, 17.0, 17.0) {
                ui.tip("Native+ member");
            }
        }
        ty += 26.0;
        c.text(font::REGULAR, 12.0, &format!("@{}", f.name), x + pad, ty, theme::TEXT_MUTED);
        ty += 19.0;
        let line = if f.online {
            f.line()
        } else if f.last_seen > 0 {
            format!("Last seen {}", relay::ago(f.last_seen))
        } else {
            "Offline".to_owned()
        };
        let line = c.ellipsize(font::MEDIUM, 11.5, &line, iw);
        c.text(font::MEDIUM, 11.5, &line, x + pad, ty, if f.online { theme::mix(f.dot(), 0xFFFFFFFF, 0.45) } else { theme::TEXT_MUTED });
        ty += 26.0;
        match &p {
            None => {
                c.skeleton(x + pad, ty, iw, 46.0, 12.0);
                c.skeleton(x + pad, ty + 60.0, iw * 0.6, 10.0, 5.0);
                ty += 90.0;
            }
            Some(p) => {
                // Activity: playing on a server or singleplayer / realms / menus.
                let activity = if f.online { relay::nice_activity(&f.activity, &f.status) } else { None };
                if let Some(act) = activity {
                    ty = heading(c, "Activity", x + pad, ty);
                    let bh = 68.0;
                    c.round(x + pad, ty, iw, bh, 12.0, theme::SUBTLE);
                    c.outline(x + pad, ty, iw, bh, 12.0, 1.0, theme::HAIRLINE);
                    c.text(font::SEMIBOLD, 9.5, if f.server_address.is_empty() { "CURRENT ACTIVITY" } else { "PLAYING ON A SERVER" }, x + pad + 14.0, ty + 12.0, theme::TEXT_MUTED);
                    c.round(x + pad + 14.0, ty + 28.0, 30.0, 30.0, 8.0, 0xFF000000);
                    c.icon(if f.server_address.is_empty() { theme::I_PLAY } else { theme::I_SERVER }, 14.0, x + pad + 29.0, ty + 43.0, theme::ONLINE);
                    let title = c.ellipsize(font::BOLD, 12.5, &act, iw - 68.0);
                    c.text(font::BOLD, 12.5, &title, x + pad + 54.0, ty + 35.0, theme::TEXT_STRONG);
                    ty += bh + 16.0;
                }
                if p.premium {
                    ty = heading(c, "Connections", x + pad, ty);
                    avatars::draw(c, f.skin.as_deref(), x + pad, ty, 32.0, 8.0);
                    c.text(font::SEMIBOLD, 12.5, &f.name, x + pad + 42.0, ty + 1.0, theme::TEXT_STRONG);
                    let nw = c.text_width(font::SEMIBOLD, 12.5, &f.name);
                    c.icon(theme::I_CHECK_CIRCLE, 12.0, x + pad + 42.0 + nw + 10.0, ty + 8.0, theme::ONLINE);
                    c.text(font::REGULAR, 11.0, "Minecraft \u{b7} Premium", x + pad + 42.0, ty + 18.0, theme::TEXT_MUTED);
                    ty += 46.0;
                }
                // Details.
                ty = heading(c, "Details", x + pad, ty);
                if p.joined_at > 0 {
                    ty = detail(c, "Member since", &relay::date(p.joined_at), x + pad, ty, iw);
                }
                ty = detail(c, "Friends", &p.friends.to_string(), x + pad, ty, iw);
                ty += 8.0;
                if let Some(bio) = p.bio.as_deref().map(str::trim).filter(|b| !b.is_empty()) {
                    ty = heading(c, "About me", x + pad, ty);
                    for l in common::wrap(c, font::REGULAR, 12.0, bio, iw) {
                        c.text(font::REGULAR, 12.0, &l, x + pad, ty, theme::TEXT);
                        ty += 18.0;
                    }
                    ty += 10.0;
                }
            }
        }
        ty += 4.0;
        // Actions.
        ty = heading(c, "Friend", x + pad, ty);
        let bw = w - pad * 2.0;
        if ui.button(c, "relay:prof:nick", x + pad, ty, bw, 32.0, if f.nickname.is_none() { "Set nickname" } else { "Change nickname" }, theme::I_PENCIL, ui::BTN_GLASS) {
            self.nick_for = Some(f.id.clone());
            self.nick.max_length = 28;
            self.nick.set(f.nickname.as_deref().unwrap_or(""));
            self.nick.focus(ui);
            ui.focus_claimed = true;
        }
        ty += 38.0;
        if ui.button(c, "relay:prof:pin", x + pad, ty, bw, 32.0, if f.pinned { "Unpin chat" } else { "Pin chat" }, theme::I_MAP_PIN, ui::BTN_GLASS) {
            relay::set_pinned(&format!("dm:{}", f.id), !f.pinned);
        }
        ty += 38.0;
        if self.armed.is_some() && ui.now - self.armed_at > 3000.0 {
            self.armed = None;
        }
        let (rk, bk) = (format!("remove:{}", f.id), format!("block:{}", f.id));
        let label = if self.armed.as_deref() == Some(rk.as_str()) { "Click again to remove" } else { "Remove friend" };
        if ui.button(c, "relay:prof:remove", x + pad, ty, bw, 32.0, label, theme::I_TRASH, ui::BTN_DANGER) {
            if self.armed.as_deref() == Some(rk.as_str()) {
                self.armed = None;
                relay::remove_friend(&f.id);
                self.selected = None;
            } else {
                self.armed = Some(rk);
                self.armed_at = ui.now;
            }
        }
        ty += 38.0;
        let label = if self.armed.as_deref() == Some(bk.as_str()) { "Click again to block" } else { "Block" };
        if ui.button(c, "relay:prof:block", x + pad, ty, bw, 32.0, label, theme::I_BAN, ui::BTN_DANGER) {
            if self.armed.as_deref() == Some(bk.as_str()) {
                self.armed = None;
                relay::block(&f.id);
                self.selected = None;
            } else {
                self.armed = Some(bk);
                self.armed_at = ui.now;
            }
        }
        ty += 44.0;
        self.profile_scroll.end(ui, c, ty + off - y);
        c.pop_clip();
    }

    // ── right-click menu, nickname dialog, settings ──────────────────────

    fn context_menu(&mut self, ui: &mut Ui, c: &mut Canvas, snap: &Snapshot, w_all: f32, h_all: f32) {
        let Some(key) = self.menu_key.clone() else { return };
        let group = key.starts_with("g:");
        let pinned = if group {
            snap.groups.iter().any(|g| key == format!("g:{}", g.id) && g.pinned)
        } else {
            snap.friends.iter().any(|f| key == format!("dm:{}", f.id) && f.pinned)
        };
        let pin = if pinned { "Unpin chat" } else { "Pin chat" };
        let (labels, icons): (Vec<&str>, Vec<u32>) = if group {
            (vec![pin, "Delete chat"], vec![theme::I_MAP_PIN, theme::I_TRASH])
        } else {
            (vec![pin, "Set nickname", "Delete chat"], vec![theme::I_MAP_PIN, theme::I_PENCIL, theme::I_TRASH])
        };
        let (mw, ih) = (196.0, 34.0);
        let mh = labels.len() as f32 * ih + 12.0;
        let (mx, my) = (self.menu_x.min(w_all - mw - 8.0), self.menu_y.min(h_all - mh - 8.0));
        c.shadow(mx, my + 6.0, mw, mh, 12.0, 24.0, 0x99000000);
        c.round(mx, my, mw, mh, 12.0, 0xFF121318);
        c.outline(mx, my, mw, mh, 12.0, 1.0, theme::HAIRLINE_STRONG);
        let mut picked = None;
        for (i, label) in labels.iter().enumerate() {
            let iy = my + 6.0 + i as f32 * ih;
            let danger = i == labels.len() - 1;
            let over = ui.hover(c, mx + 6.0, iy, mw - 12.0, ih);
            let hv = ui.anim_on(&format!("relay:ctx:{i}"), over, 18.0);
            c.round(mx + 6.0, iy, mw - 12.0, ih, 8.0, theme::alpha(if danger { 0xFFEF4444 } else { 0xFFFFFFFF }, 0.1 * hv));
            let col = if danger { theme::mix(0xFFFCA5A5, 0xFFFFFFFF, hv * 0.3) } else { theme::mix(theme::TEXT, theme::TEXT_STRONG, hv) };
            c.icon(icons[i], 14.0, mx + 24.0, iy + ih / 2.0, col);
            let lh = c.line_height(font::MEDIUM, 12.5);
            c.text(font::MEDIUM, 12.5, label, mx + 40.0, iy + (ih - lh) / 2.0, col);
            if ui.clicked(c, &format!("relay:ctx:{i}"), mx + 6.0, iy, mw - 12.0, ih) {
                picked = Some(i);
            }
        }
        if let Some(p) = picked {
            self.menu_key = None;
            if p == 0 {
                relay::set_pinned(&key, !pinned);
            } else if !group && p == 1 {
                let id = key.trim_start_matches("dm:").to_owned();
                let cur = snap.friends.iter().find(|f| f.id == id).and_then(|f| f.nickname.clone());
                self.nick.max_length = 28;
                self.nick.set(cur.as_deref().unwrap_or(""));
                self.nick_for = Some(id);
                self.nick.focus(ui);
                ui.focus_claimed = true;
            } else {
                relay::clear_chat(&key);
                self.chat_scrolls.remove(&key);
                self.last_count = -1;
            }
            return;
        }
        if self.menu_fresh {
            self.menu_fresh = false;
        } else if (ui.pressed || ui.right_pressed) && !(ui.mx >= mx && ui.mx < mx + mw && ui.my >= my && ui.my < my + mh) {
            self.menu_key = None;
        }
    }

    fn nickname_dialog(&mut self, ui: &mut Ui, c: &mut Canvas, snap: &Snapshot, w_all: f32, h_all: f32) {
        let Some(id) = self.nick_for.clone() else { return };
        let name = snap.friends.iter().find(|f| f.id == id).map_or_else(|| id.clone(), |f| f.name.clone());
        c.fill(0.0, 0.0, w_all, h_all, 0x99000000);
        let (dw, dh) = (340.0, 168.0);
        let (dx, dy) = ((w_all - dw) / 2.0, (h_all - dh) / 2.0);
        c.shadow(dx, dy + 8.0, dw, dh, 10.0, 32.0, 0xAA000000);
        c.round(dx, dy, dw, dh, 10.0, 0xFF121318);
        c.outline(dx, dy, dw, dh, 10.0, 1.0, theme::HAIRLINE_STRONG);
        c.text(font::BOLD, 15.0, "Set nickname", dx + 20.0, dy + 18.0, theme::TEXT_STRONG);
        let shown = c.ellipsize(font::REGULAR, 12.0, &name, dw - 190.0);
        c.text(font::REGULAR, 12.0, &format!("Only you see this name for {shown}."), dx + 20.0, dy + 42.0, theme::TEXT_MUTED);
        let enter = self.nick.draw(ui, c, dx + 20.0, dy + 68.0, dw - 40.0, 38.0, "Nickname", 13.5);
        if ui.focus.is_none() {
            self.nick.focus(ui);
        }
        ui.focus_claimed = true;
        let mut save = enter;
        if ui.button(c, "relay:nick:save", dx + dw - 20.0 - 96.0, dy + dh - 20.0 - 34.0, 96.0, 34.0, "Save", theme::I_CHECK, ui::BTN_PRIMARY) {
            save = true;
        }
        if ui.button(c, "relay:nick:cancel", dx + dw - 20.0 - 96.0 - 8.0 - 96.0, dy + dh - 20.0 - 34.0, 96.0, 34.0, "Cancel", theme::I_X, ui::BTN_GLASS) {
            self.nick_for = None;
            return;
        }
        if save {
            relay::set_nickname(&id, &self.nick.value());
            self.nick_for = None;
            self.input.focus(ui);
        }
    }

    fn settings(&mut self, ui: &mut Ui, c: &mut Canvas, cfg: &mut UiConfig, px: f32, py: f32, sw: f32) {
        let (w, h) = (300.0, 328.0);
        let (x, y) = (px + 16.0, py + 54.0);
        if ui.pressed && !ui.hover(c, x, y, w, h) && !ui.hover(c, px + sw - 46.0, py + 16.0, 30.0, 30.0) {
            self.settings_open = false;
            self.binding_key = false;
            return;
        }
        c.shadow(x, y + 6.0, w, h, 14.0, 26.0, 0xAA000000);
        c.round(x, y, w, h, 14.0, 0xFF101115);
        c.outline(x, y, w, h, 14.0, 1.0, theme::HAIRLINE_STRONG);
        c.text(font::SEMIBOLD, 13.5, "Chat settings", x + 16.0, y + 14.0, theme::TEXT_STRONG);
        let mut ry = y + 46.0;
        let mut dirty = false;
        for (title, sub, id, on) in [
            ("Native title screen", "Replace the vanilla main menu", "set:title", cfg.custom_title),
            ("Message pop-ups", "Show new messages while playing", "set:toasts", cfg.notifications),
            ("Activity status", "Show friends what you're playing", "set:activity", cfg.share_activity),
        ] {
            ui.interactive = true;
            c.text(font::MEDIUM, 12.5, title, x + 16.0, ry - 2.0, theme::TEXT);
            c.text(font::REGULAR, 10.5, sub, x + 16.0, ry + 16.0, theme::TEXT_MUTED);
            if ui.toggle(c, id, x + w - 16.0 - 34.0, ry + 4.0, on) {
                match id {
                    "set:title" => cfg.custom_title = !on,
                    "set:toasts" => cfg.notifications = !on,
                    _ => cfg.share_activity = !on,
                }
                dirty = true;
            }
            ry += 46.0;
        }
        // The key that opens the chat.
        c.text(font::MEDIUM, 12.5, "Open chat key", x + 16.0, ry + 4.0, theme::TEXT);
        let label = if self.binding_key { "Press a key\u{2026}".to_owned() } else { super::keys::name(cfg.relay_key) };
        let bw = (c.text_width(font::SEMIBOLD, 12.0, &label) + 24.0).max(70.0);
        ui.interactive = true;
        if ui.button(c, "set:key", x + w - 16.0 - bw, ry - 2.0, bw, 30.0, &label, 0, ui::BTN_GLASS) {
            self.binding_key = !self.binding_key;
        }
        ry += 44.0;
        c.text(font::MEDIUM, 12.5, "Interface size", x + 16.0, ry + 4.0, theme::TEXT);
        let pct = format!("{}%", (cfg.scale * 100.0).round() as i32);
        let pw = c.text_width(font::SEMIBOLD, 12.0, &pct);
        let bx = x + w - 16.0 - 30.0;
        if ui.button(c, "set:bigger", bx, ry - 2.0, 30.0, 30.0, "", theme::I_PLUS, ui::BTN_GLASS) {
            cfg.scale = (((cfg.scale + 0.1) * 10.0).round() / 10.0).min(2.0);
            dirty = true;
        }
        let lh = c.line_height(font::SEMIBOLD, 12.0);
        c.text(font::SEMIBOLD, 12.0, &pct, bx - 12.0 - pw, ry + 13.0 - lh / 2.0, theme::TEXT_STRONG);
        if ui.button(c, "set:smaller", bx - 24.0 - pw - 30.0, ry - 2.0, 30.0, 30.0, "\u{2212}", 0, ui::BTN_GLASS) {
            cfg.scale = (((cfg.scale - 0.1) * 10.0).round() / 10.0).max(0.6);
            dirty = true;
        }
        if dirty {
            cfg.save();
        }
    }
}

/// "RELAY" in the launcher's tracked wordmark; returns the x after the last letter.
fn wordmark(c: &mut Canvas, mut x: f32, y: f32) -> f32 {
    let s = "RELAY";
    for (i, ch) in s.chars().enumerate() {
        let t = ch.to_string();
        c.text(font::BOLD, 17.0, &t, x, y, theme::TEXT_STRONG);
        x += c.text_width(font::BOLD, 17.0, &t) + if i < s.len() - 1 { 2.4 } else { 0.0 };
    }
    x
}

/// A small solid caret (down when open, right when collapsed).
fn caret(c: &mut Canvas, cx: f32, cy: f32, open: bool, col: u32) {
    for i in 0..4 {
        let len = 7.0 - i as f32 * 2.0;
        if open {
            c.fill(cx - len / 2.0, cy - 2.0 + i as f32, len, 1.0, col);
        } else {
            c.fill(cx - 2.0 + i as f32, cy - len / 2.0, 1.0, len, col);
        }
    }
}

/// The last message of a chat that was opened before ("You: hi", "Picture"), `None` when unknown.
fn snippet(key: &str, snap: &Snapshot) -> Option<String> {
    let m = relay::peek_last(key)?;
    if m.created_at <= relay::cleared_at(key) {
        return None;
    }
    let media = m.media_url.is_some() || m.local_key.is_some();
    let body = if m.deleted {
        "Message deleted".to_owned()
    } else if media && (m.content.trim().is_empty() || m.content.starts_with('[')) {
        m.media_name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| "Picture".to_owned())
    } else {
        m.content.replace('\n', " ")
    };
    Some(format!("{}{body}", if snap.me_id.as_deref() == Some(m.sender_id.as_str()) { "You: " } else { "" }))
}

fn skeleton_rows(c: &mut Canvas, x: f32, y: f32, w: f32, n: usize) {
    for i in 0..n {
        let ry = y + i as f32 * 56.0;
        c.skeleton(x + 9.0, ry + 9.0, 36.0, 36.0, 10.0);
        let lw = 70.0 + ((i * 37) % 60) as f32;
        c.skeleton(x + 56.0, ry + 12.0, lw.min(w - 70.0), 11.0, 5.0);
        c.skeleton(x + 56.0, ry + 31.0, (lw + 50.0).min(w - 70.0), 9.0, 4.0);
    }
}

pub(super) fn visible(y: f32, h: f32, top: f32, view_h: f32) -> bool {
    y + h >= top && y <= top + view_h
}

fn count_w(c: &mut Canvas, tab: usize, n: usize) -> f32 {
    if tab == 2 {
        return if n > 0 {
            (c.text_width(font::BOLD, 10.5, &if n > 99 { "99+".to_owned() } else { n.to_string() }) + 10.0).max(18.0) + 6.0
        } else {
            0.0
        };
    }
    c.text_width(font::BOLD, 11.0, &n.to_string()) + 6.0
}

fn home_title(c: &mut Canvas, label: &str, x: f32, y: f32) -> f32 {
    c.text(font::SEMIBOLD, 11.5, label, x, y, theme::TEXT_MUTED);
    y + 26.0
}

/// Big muted icon + one line, like the launcher's empty friends list.
fn home_empty(c: &mut Canvas, x: f32, y: f32, w: f32, h: f32, icon: u32, text: &str) {
    let cy = y + (h * 0.22).clamp(50.0, 110.0);
    c.icon(icon, 44.0, x + w / 2.0, cy, theme::alpha(theme::TEXT_MUTED, 0.85));
    let tw = c.text_width(font::MEDIUM, 13.0, text);
    c.text(font::MEDIUM, 13.0, text, x + (w - tw) / 2.0, cy + 38.0, theme::TEXT_SECONDARY);
}

fn heading(c: &mut Canvas, label: &str, x: f32, y: f32) -> f32 {
    c.text(font::SEMIBOLD, 10.5, &label.to_uppercase(), x, y + 4.0, theme::TEXT_MUTED);
    y + 26.0
}

/// "Member since ........ Oct 11, 2026" with a hairline under it.
fn detail(c: &mut Canvas, label: &str, value: &str, x: f32, y: f32, w: f32) -> f32 {
    c.text(font::REGULAR, 12.0, label, x, y, theme::TEXT_MUTED);
    let vw = c.text_width(font::SEMIBOLD, 12.0, value);
    c.text(font::SEMIBOLD, 12.0, value, x + w - vw, y, theme::TEXT_STRONG);
    c.fill(x, y + 24.0, w, 1.0, theme::HAIRLINE);
    y + 32.0
}

/// Card shell: avatar 40 (with presence when `dot` != 0), name, line. Buttons are drawn by the caller.
#[allow(clippy::too_many_arguments)]
fn person_card(ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, w: f32, name: &str, skin: Option<&str>, dot: u32, line: &str, line_color: u32) {
    let ch = 68.0;
    let over = ui.hover(c, x, y, w, ch);
    let hv = ui.anim_on(&format!("relay:card:{id}"), over, 16.0);
    c.round(x, y, w, ch, 8.0, theme::alpha(0xFFFFFFFF, 0.025 + 0.02 * hv));
    c.outline(x, y, w, ch, 8.0, 1.0, theme::alpha(0xFFFFFFFF, 0.07 + 0.05 * hv));
    let av = 40.0;
    let (ax, ay) = (x + 16.0, y + (ch - av) / 2.0);
    avatars::draw(c, skin, ax, ay, av, 8.0);
    if dot != 0 {
        c.circle(ax + av - 1.0, ay + av - 1.0, 6.5, 0xFF0B0C10);
        c.circle(ax + av - 1.0, ay + av - 1.0, 4.5, dot);
    }
    let tx = ax + av + 14.0;
    let max_w = w - (tx - x) - 170.0;
    let name = c.ellipsize(font::SEMIBOLD, 14.0, if name.is_empty() { "Player" } else { name }, max_w);
    c.text(font::SEMIBOLD, 14.0, &name, tx, y + 15.0, theme::TEXT_STRONG);
    let line = c.ellipsize(font::REGULAR, 12.0, line, max_w);
    c.text(font::REGULAR, 12.0, &line, tx, y + 36.0, line_color);
}

/// Round tinted action (accept / decline).
#[allow(clippy::too_many_arguments)]
fn circle_btn(ui: &mut Ui, c: &mut Canvas, id: &str, cx: f32, cy: f32, icon: u32, color: u32, tip: &str) -> bool {
    let over = ui.hover(c, cx - 17.0, cy - 17.0, 34.0, 34.0);
    let hv = ui.anim_on(&format!("{id}#h"), over, 16.0);
    c.circle(cx, cy, 17.0, theme::mix(theme::alpha(color, 0.2), color, hv));
    c.icon(icon, 17.0, cx, cy, theme::mix(color, 0xFFFFFFFF, hv));
    if over {
        ui.cursor_hand = true;
        ui.tip(tip);
    }
    ui.clicked(c, id, cx - 17.0, cy - 17.0, 34.0, 34.0)
}
