//! The conversation half of `RelayView`: header, messages (day dividers,
//! grouping, replies, reactions, pictures), hover tools, the emoji picker, the
//! GIF and screenshot pickers, the picture viewer and the composer.
#![allow(dead_code)]

use super::avatars;
use super::chat_media::{self, Media};
use super::common;
use super::net;
use super::relay::{self, Message, Snapshot};
use super::relay_view::{RelayView, Staged, visible};
use super::scroll::Scroll;
use super::ui::{self, Ui};
use super::uiconfig::UiConfig;
use horizon_client::native_ui::{Canvas, Image, font, theme};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

/// The quick reactions, in the picker's order.
pub const PALETTE: [&str; 8] = ["\u{2764}\u{FE0F}", "\u{1F602}", "\u{1F525}", "\u{1F44D}", "\u{1F62E}", "\u{1F622}", "\u{1F389}", "\u{1F480}"];

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn emoji_image(e: &str) -> Option<Arc<Image>> {
    static CACHE: OnceLock<HashMap<String, Arc<Image>>> = OnceLock::new();
    let map = CACHE.get_or_init(|| {
        horizon_client::native_ui::assets::EMOJI.iter().filter_map(|(hex, bytes)| Some(((*hex).to_owned(), Image::decode(bytes)?))).collect()
    });
    let hex = format!("{:x}", e.chars().next()? as u32);
    map.get(&hex).cloned()
}

fn draw_emoji(c: &mut Canvas, e: &str, x: f32, y: f32, size: f32) {
    match emoji_image(e) {
        Some(img) => {
            let key = 0xE30_0000_0000u64 | e.chars().next().map_or(0, |ch| ch as u64);
            c.stamp(key, &img, x, y, size, size, true, 0xFFFFFFFF);
        }
        None => c.icon(theme::I_SMILE, size * 0.9, x + size / 2.0, y + size / 2.0, theme::TEXT_SECONDARY),
    }
}

/// Reactions of a message as (emoji, count, yours), in the order they first appeared.
fn grouped(m: &Message, me: Option<&str>) -> Vec<(String, u32, bool)> {
    let mut out: Vec<(String, u32, bool)> = Vec::new();
    for (user, emoji) in &m.reactions {
        let mine = me == Some(user.as_str());
        match out.iter_mut().find(|r| &r.0 == emoji) {
            Some(r) => {
                r.1 += 1;
                r.2 |= mine;
            }
            None => out.push((emoji.clone(), 1, mine)),
        }
    }
    out
}

fn sniff_mime(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("image/png")
    } else if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if b.starts_with(b"GIF8") {
        Some("image/gif")
    } else if b.len() > 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

fn is_local_key(key: &str) -> bool {
    key.starts_with("local-") || key.starts_with("staged-")
}

/// The picture for a key: our own pending upload comes from the local copy, everything else from the cache.
fn media_of(key: &str, api: &str, fetch: bool) -> Option<Arc<Media>> {
    if is_local_key(key) {
        return chat_media::local(key);
    }
    if fetch { chat_media::get(key, api) } else { chat_media::peek(key) }
}

fn same(a: &str, b: Option<&str>) -> bool {
    !a.is_empty() && b == Some(a)
}

impl RelayView {
    fn wrapped(&mut self, c: &mut Canvas, m: &Message, width: f32, face: usize, size: f32) -> Arc<Vec<String>> {
        let key = format!("{}:{}", m.id, size);
        if let Some((w, content, lines)) = self.wrap_cache.get(&key)
            && (*w - width).abs() < 0.01
            && *content == m.content
        {
            return lines.clone();
        }
        let lines = Arc::new(common::wrap(c, face, size, &m.content, width));
        if self.wrap_cache.len() > 4000 {
            self.wrap_cache.clear();
        }
        self.wrap_cache.insert(key, (width, m.content.clone(), lines.clone()));
        lines
    }

    /// The picture a message shows: its local copy while ours is still uploading, else the server's.
    fn media_key(&self, m: &Message) -> Option<String> {
        match (&m.local_key, &m.media_url) {
            (Some(l), url) if url.is_none() || chat_media::local(l).is_some() => Some(l.clone()),
            (_, Some(u)) => Some(u.clone()),
            _ => None,
        }
    }

    fn known_size(&self, m: &Message, key: &str) -> Option<(f32, f32)> {
        self.media_sizes.get(key).copied().or_else(|| m.local_key.as_ref().and_then(|l| self.media_sizes.get(l).copied()))
    }

    fn media_height(&self, m: &Message, max_w: f32) -> f32 {
        let Some(key) = self.media_key(m) else { return 0.0 };
        let (mut dw, mut dh) = match self.known_size(m, &key) {
            Some((w, h)) => (w, if h < 0.0 { 54.0 } else { h }),
            None => (240.0, 150.0),
        };
        if let Some(md) = media_of(&key, "", false) {
            if md.failed {
                return 54.0;
            } else if md.width > 0 {
                let (mw, mh) = (md.width as f32, md.height as f32);
                let k = 1f32.min((max_w / mw).min(300.0 / mh));
                let up = if mw < 160.0 && mh < 160.0 { 1.0 } else { k };
                dw = mw * up;
                dh = mh * up;
                if dw > max_w {
                    dh *= max_w / dw;
                }
            }
        }
        let _ = dw;
        dh
    }

    fn reactions_height(&self, c: &mut Canvas, m: &Message, width: f32) -> f32 {
        let (mut cx, mut cy, ch) = (0.0f32, 0.0f32, 22.0f32);
        for (_, n, _) in grouped(m, None) {
            let cw = 10.0 + 14.0 + 5.0 + c.text_width(font::SEMIBOLD, 11.5, &n.to_string()) + 10.0;
            if cx + cw > width && cx > 0.0 {
                cx = 0.0;
                cy += ch + 4.0;
            }
            cx += cw + 6.0;
        }
        cy + ch
    }

    fn measure(&mut self, c: &mut Canvas, msgs: &[Message], has_more: bool, text_w: f32, lh: f32) -> f32 {
        let mut h = 14.0 + if has_more { 26.0 } else { 0.0 };
        let mut prev: Option<&Message> = None;
        let media_w = text_w.min(340.0);
        for m in msgs {
            let new_day = prev.is_none_or(|p| !common::same_day(p.created_at, m.created_at));
            if new_day {
                h += 30.0;
            }
            if m.system {
                let lines = self.wrapped(c, m, text_w + 46.0, font::REGULAR, 11.5);
                h += lines.len() as f32 * lh + 6.0;
                prev = Some(m);
                continue;
            }
            let head = new_day || prev.is_none_or(|p| p.system || p.sender_id != m.sender_id || m.created_at - p.created_at > 5 * 60_000);
            if head {
                if prev.is_some() && !new_day {
                    h += 10.0;
                }
                h += 20.0;
            }
            if m.reply.is_some() {
                h += 19.0;
            }
            let has_media = m.media_url.is_some() || m.local_key.is_some();
            let auto = has_media && (m.content.trim().is_empty() || m.content.starts_with('['));
            if m.deleted {
                h += lh;
            } else if !auto {
                h += self.wrapped(c, m, text_w, font::REGULAR, 13.0).len() as f32 * lh;
            }
            if has_media {
                h += self.media_height(m, media_w) + 4.0;
            }
            if m.failed {
                h += 16.0;
            }
            if !m.reactions.is_empty() && !m.deleted {
                h += self.reactions_height(c, m, text_w) + 3.0;
            }
            h += 2.0;
            prev = Some(m);
        }
        h + 10.0
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn chat(&mut self, ui: &mut Ui, c: &mut Canvas, snap: &Snapshot, cfg: &mut UiConfig, x: f32, y: f32, w: f32, h: f32) -> bool {
        if ui.icon_button(c, "relay:close", x + w - 46.0, y + 14.0, 32.0, theme::I_X, Some("Close (Esc)")) {
            return true;
        }
        let Some(sel) = self.selected.clone() else {
            self.friends_home(ui, c, snap, x, y, w, h);
            return false;
        };
        if sel.starts_with("dm:")
            && ui.icon_button(c, "relay:profile", x + w - 46.0 - 38.0, y + 14.0, 32.0, theme::I_USER, Some(if self.profile_open { "Hide profile" } else { "Show profile" }))
        {
            self.profile_open = !self.profile_open;
        }
        let api = net::api();
        let conv = relay::conversation(&sel);
        let group = conv.is_group();
        let friend = if group { None } else { snap.friends.iter().find(|f| f.id == conv.target_id()) };
        let grp = if group { snap.groups.iter().find(|g| g.id == conv.target_id()) } else { None };
        let name: String = if group {
            grp.map_or("Group".to_owned(), |g| g.name.clone())
        } else {
            friend.map_or("Friend".to_owned(), |f| f.display().to_owned())
        };
        // Header.
        let (hx, hy) = (x + 20.0, y + 14.0);
        if group {
            c.round(hx, hy, 36.0, 36.0, 10.0, theme::mix(theme::name_color(Some(&name)), 0xFF000000, 0.45));
            c.icon(theme::I_HASH, 17.0, hx + 18.0, hy + 18.0, 0xFFFFFFFF);
        } else {
            avatars::draw(c, friend.and_then(|f| f.skin.as_deref()), hx, hy, 36.0, 10.0);
        }
        let name_w = (x + w - 46.0 - 38.0 * if group { 1.0 } else { 2.0 } - 10.0 - (hx + 48.0)).max(60.0);
        let shown = c.ellipsize(font::SEMIBOLD, 15.0, &name, name_w);
        c.text(font::SEMIBOLD, 15.0, &shown, hx + 48.0, hy, theme::TEXT_STRONG);
        let (status, status_color) = if group {
            (grp.map_or(String::new(), |g| format!("{} members", g.members)), theme::TEXT_MUTED)
        } else if let Some(f) = friend.filter(|f| f.online) {
            (f.line(), theme::mix(f.dot(), 0xFFFFFFFF, 0.45))
        } else {
            ("Offline".to_owned(), theme::TEXT_MUTED)
        };
        let status = c.ellipsize(font::REGULAR, 11.5, &status, name_w);
        c.text(font::REGULAR, 11.5, &status, hx + 48.0, hy + 20.0, status_color);
        c.fill(x, y + 64.0, w, 1.0, theme::HAIRLINE);

        // Messages.
        let input_h = 46.0;
        let bar_h = if self.reply_to.is_some() || self.editing_id.is_some() { 36.0 } else { 0.0 };
        let my0 = y + 65.0;
        let mh = h - 65.0 - input_h - 34.0 - bar_h;
        let mut sc = self.chat_scrolls.remove(&sel).unwrap_or_else(|| {
            let mut s = Scroll::new(&format!("relay-chat:{sel}"));
            s.stick_bottom = true;
            s
        });
        let cleared = relay::cleared_at(&sel);
        let messages: Vec<Message> = if cleared > 0 { conv.messages.iter().filter(|m| m.created_at > cleared).cloned().collect() } else { conv.messages.clone() };
        let pad = 20.0;
        let text_x = x + pad + 46.0;
        let text_w = w - pad * 2.0 - 46.0 - 10.0;
        let lh = 19.0;
        let pre = self.measure(c, &messages, conv.has_more, text_w, lh);
        if ui.hover(c, x, my0, w, mh) && ui.scroll > 0.0 {
            self.user_scrolled_up = true;
        }
        if self.open_at_bottom {
            sc.content = pre;
            sc.snap_bottom(mh);
            self.user_scrolled_up = false;
            if !messages.is_empty() {
                self.open_at_bottom = false;
                self.last_count = messages.len() as i64;
            }
        } else if !self.user_scrolled_up {
            sc.content = pre;
            sc.snap_bottom(mh);
            self.last_count = messages.len() as i64;
        }
        let off = sc.begin(ui, c, x, my0, w, mh);
        let mut cy = my0 + 14.0 - off;
        if conv.has_more {
            if conv.loading {
                c.skeleton(x + pad, cy - 2.0, 22.0, 22.0, 7.0);
                c.skeleton(x + pad + 32.0, cy + 1.0, 70.0, 8.0, 4.0);
                c.skeleton(x + pad + 32.0, cy + 13.0, (w - pad * 2.0 - 40.0).min(190.0), 7.0, 3.5);
            } else if self.user_scrolled_up && self.first_ids.contains_key(&sel) && sc.offset < 30.0 && !sc.at_bottom() {
                relay::load_older(&sel);
            }
            cy += 26.0;
        }
        if !conv.loaded && conv.error.is_none() {
            skeleton_messages(c, x + pad, my0 + 14.0, w - pad * 2.0, mh);
        } else if conv.loaded && messages.is_empty() {
            common::empty(c, x, my0, w, mh, theme::I_SMILE, &format!("Say hi to {name}"), "This is the beginning of your conversation.");
        } else if let (Some(e), true) = (&conv.error, messages.is_empty()) {
            common::empty(c, x, my0, w, mh, theme::I_WIFI_OFF, "Couldn't load messages", e);
        }
        let now_ms = relay::now_ms();
        let me = snap.me_id.as_deref();
        let mut prev: Option<&Message> = None;
        let mut next_anchor: Option<(String, f32)> = None;
        let mut anchor_now: Option<f32> = None;
        let mut jump_content_y: Option<f32> = None;
        self.pick_row = None;
        let can_moderate = grp.is_some_and(|g| g.can_moderate());
        let picker_hover = self.picker_rect.is_some_and(|r| ui.hover(c, r[0], r[1], r[2], r[3]));
        for m in &messages {
            if self.jump_to.as_deref().is_some_and(|j| j == m.id) {
                jump_content_y = Some(cy + off);
            }
            if !m.id.is_empty() {
                if self.anchor_id.as_deref() == Some(m.id.as_str()) {
                    anchor_now = Some(cy + off);
                }
                if next_anchor.is_none() && cy >= my0 {
                    next_anchor = Some((m.id.clone(), cy + off));
                }
            }
            let new_day = prev.is_none_or(|p| !common::same_day(p.created_at, m.created_at));
            if new_day {
                let day = common::day_label(m.created_at, now_ms);
                let dw = c.text_width(font::SEMIBOLD, 10.5, &day);
                if visible(cy, 30.0, my0, mh) {
                    c.fill(x + pad, cy + 12.0, (w - pad * 2.0 - dw) / 2.0 - 10.0, 1.0, theme::HAIRLINE);
                    c.fill(x + (w + dw) / 2.0 + 10.0, cy + 12.0, (w - pad * 2.0 - dw) / 2.0 - 10.0, 1.0, theme::HAIRLINE);
                    let dlh = c.line_height(font::SEMIBOLD, 10.5);
                    c.text(font::SEMIBOLD, 10.5, &day, x + (w - dw) / 2.0, cy + 12.0 - dlh / 2.0, theme::TEXT_MUTED);
                }
                cy += 30.0;
            }
            if m.system {
                let lines = self.wrapped(c, m, text_w + 46.0, font::REGULAR, 11.5);
                for line in lines.iter() {
                    if visible(cy, lh, my0, mh) {
                        let lw = c.text_width(font::REGULAR, 11.5, line);
                        c.text(font::REGULAR, 11.5, line, x + (w - lw) / 2.0, cy, theme::TEXT_MUTED);
                    }
                    cy += lh;
                }
                cy += 6.0;
                prev = Some(m);
                continue;
            }
            let row_top = cy;
            let mine_msg = same(&m.sender_id, me);
            let head = new_day || prev.is_none_or(|p| p.system || p.sender_id != m.sender_id || m.created_at - p.created_at > 5 * 60_000);
            if head {
                if prev.is_some() && !new_day {
                    cy += 10.0;
                }
                let who = if m.sender_name.is_empty() { "Someone" } else { m.sender_name.as_str() };
                if visible(cy, 40.0, my0, mh) {
                    let sender = if m.sender_id.is_empty() { None } else { snap.friends.iter().find(|f| f.id == m.sender_id) };
                    let skin = match sender {
                        Some(f) => f.skin.clone(),
                        None if mine_msg => snap.me_skin.clone(),
                        None => None,
                    };
                    avatars::draw(c, skin.as_deref(), x + pad, cy + 1.0, 34.0, 10.0);
                    let col = if mine_msg { theme::TEXT_STRONG } else { theme::mix(theme::name_color(Some(who)), 0xFFFFFFFF, 0.55) };
                    let nw = c.text(font::SEMIBOLD, 13.0, who, text_x, cy, col);
                    let stamp = common::hhmm(m.created_at);
                    let tw2 = c.text(font::REGULAR, 10.5, &stamp, text_x + nw + 8.0, cy + 2.5, theme::TEXT_MUTED);
                    if !group && mine_msg && !m.pending && !m.failed && !m.deleted {
                        c.icon(
                            if m.read { theme::I_CHECKS } else { theme::I_CHECK },
                            12.0,
                            text_x + nw + 8.0 + tw2 + 12.0,
                            cy + 8.0,
                            if m.read { 0xFF60A5FA } else { theme::TEXT_MUTED },
                        );
                    }
                }
                cy += 20.0;
            }
            if let Some(r) = &m.reply {
                if visible(cy, 18.0, my0, mh) {
                    let qn = if r.sender_name.is_empty() { "Someone" } else { r.sender_name.as_str() };
                    let snip = if r.deleted {
                        "Original message was deleted".to_owned()
                    } else if r.content.is_empty() {
                        if r.media { "Picture".to_owned() } else { String::new() }
                    } else {
                        r.content.replace('\n', " ")
                    };
                    let over = ui.hover(c, text_x, cy - 1.0, text_w, 17.0);
                    let qc = if over { theme::TEXT } else { theme::TEXT_MUTED };
                    c.fill(text_x, cy + 1.0, 2.0, 12.0, theme::HAIRLINE_STRONG);
                    c.icon(theme::I_REPLY_ARROW, 11.0, text_x + 11.0, cy + 7.0, qc);
                    let who = c.ellipsize(font::SEMIBOLD, 11.5, qn, (text_w * 0.35).max(40.0));
                    let qw = c.text(font::SEMIBOLD, 11.5, &who, text_x + 21.0, cy, theme::mix(theme::name_color(Some(qn)), 0xFFFFFFFF, 0.55));
                    let sn = c.ellipsize(font::REGULAR, 11.5, &snip, (text_w - 21.0 - qw - 8.0).max(20.0));
                    c.text(font::REGULAR, 11.5, &sn, text_x + 21.0 + qw + 6.0, cy, qc);
                    if over {
                        ui.cursor_hand = true;
                    }
                    if ui.clicked(c, &format!("relay:quote:{}", m.id), text_x, cy - 1.0, text_w, 17.0) && !r.id.is_empty() {
                        self.jump_to = Some(r.id.clone());
                        self.jump_tries = 0;
                    }
                }
                cy += 19.0;
            }
            let has_media = m.media_url.is_some() || m.local_key.is_some();
            let auto = has_media && (m.content.trim().is_empty() || m.content.starts_with('['));
            let color = if m.deleted {
                theme::TEXT_MUTED
            } else if m.pending {
                theme::alpha(theme::TEXT, 0.5)
            } else {
                theme::TEXT
            };
            if m.deleted {
                if visible(cy, lh, my0, mh) {
                    c.text(font::REGULAR, 12.5, "This message was deleted", text_x, cy, theme::alpha(theme::TEXT_MUTED, 0.85));
                }
                cy += lh;
            } else if !auto {
                let lines = self.wrapped(c, m, text_w, font::REGULAR, 13.0);
                for (li, line) in lines.iter().enumerate() {
                    if visible(cy, lh, my0, mh) {
                        let lw = c.text(font::REGULAR, 13.0, line, text_x, cy, color);
                        if m.edited && li + 1 == lines.len() {
                            c.text(font::REGULAR, 10.0, "(edited)", text_x + lw + 6.0, cy + 3.0, theme::TEXT_MUTED);
                        }
                    }
                    cy += lh;
                }
            }
            if has_media {
                cy += self.media(ui, c, &api, m, text_x, cy, text_w.min(340.0), my0, mh) + 4.0;
            }
            if m.failed {
                c.text(font::MEDIUM, 10.5, "Not sent \u{2014} check your connection", text_x, cy, 0xFFF87171);
                cy += 16.0;
            }
            if !m.reactions.is_empty() && !m.deleted {
                cy = self.reactions(ui, c, &sel, m, me, text_x, cy + 1.0, text_w, my0, mh) + 2.0;
            }
            cy += 2.0;
            if self.flash_id.as_deref() == Some(m.id.as_str()) {
                let fa = 1.0 - ((ui.now - self.flash_at) / 1600.0).min(1.0) as f32;
                if fa > 0.0 {
                    c.fill(x, row_top - 2.0, w - 10.0, cy - row_top + 3.0, theme::alpha(0xFF60A5FA, 0.16 * fa));
                } else {
                    self.flash_id = None;
                }
            }
            let row_hover = ui.interactive
                && self.viewer.is_none()
                && !self.gif_open
                && !picker_hover
                && ui.my >= my0
                && ui.my <= my0 + mh
                && ui.hover(c, x, row_top, w - 10.0, cy - row_top);
            if row_hover {
                c.round(x + 8.0, row_top - 2.0, w - 22.0, cy - row_top + 3.0, 8.0, 0x0AFFFFFF);
            }
            if (row_hover || self.react_for.as_deref() == Some(m.id.as_str())) && visible(row_top, 24.0, my0, mh) {
                self.tools(ui, c, &sel, m, group, mine_msg, can_moderate, x + w - 22.0, row_top - 4.0);
            }
            prev = Some(m);
        }
        let content = cy - my0 + off + 10.0;
        let first_id = messages.first().map(|m| m.id.clone());
        let prev_first = self.first_ids.get(&sel).cloned();
        let anchor_ok = self.user_scrolled_up && self.anchor_conv.as_deref() == Some(sel.as_str()) && self.anchor_id.is_some() && sc.content > 0.0 && !sc.at_bottom();
        if let (true, Some(now)) = (anchor_ok, anchor_now) {
            let d = now - self.anchor_y;
            if d.abs() > 0.5 {
                sc.shift(d);
            }
        } else if self.user_scrolled_up && first_id.is_some() && prev_first.is_some() && first_id != prev_first && sc.content > 0.0 && !sc.at_bottom() {
            sc.shift(content - sc.content);
        }
        self.anchor_conv = Some(sel.clone());
        match next_anchor {
            Some((id, ay)) => {
                self.anchor_id = Some(id);
                self.anchor_y = ay;
            }
            None => self.anchor_id = None,
        }
        if let Some(f) = &first_id {
            self.first_ids.insert(sel.clone(), f.clone());
        } else {
            self.first_ids.remove(&sel);
        }
        sc.end(ui, c, content);
        if sc.is_dragging() && !sc.at_bottom() {
            self.user_scrolled_up = true;
        } else if sc.at_bottom() {
            self.user_scrolled_up = false;
        }
        if prev_first.is_none() && first_id.is_some() {
            sc.snap_bottom(mh);
            self.user_scrolled_up = false;
        }
        if let Some(target) = self.jump_to.clone() {
            if let Some(cyy) = jump_content_y {
                self.user_scrolled_up = true;
                sc.target = (cyy - my0 - mh * 0.25).max(0.0);
                self.flash_id = Some(target);
                self.flash_at = ui.now;
                self.jump_to = None;
            } else if conv.has_more && self.jump_tries < 12 {
                if !conv.loading {
                    self.jump_tries += 1;
                    relay::load_older(&sel);
                }
            } else {
                self.jump_to = None;
            }
        }
        self.picker_rect = None;
        if let Some(rid) = self.react_for.clone() {
            match self.pick_row {
                Some(row) => self.picker(ui, c, &sel, &rid, row, my0),
                None => self.react_for = None,
            }
        }
        self.jump_rect = None;
        if !sc.at_bottom() && sc.content > mh + 40.0 {
            let t = "Jump to latest";
            let (pw2, ph2) = (c.text_width(font::SEMIBOLD, 11.5, t) + 40.0, 28.0);
            let (pxx, pyy) = (x + (w - pw2) / 2.0, my0 + mh - ph2 - 10.0);
            self.jump_rect = Some([pxx, pyy, pw2, ph2]);
            let over = ui.hover(c, pxx, pyy, pw2, ph2);
            c.shadow(pxx, pyy + 3.0, pw2, ph2, 14.0, 14.0, 0x88000000);
            c.round(pxx, pyy, pw2, ph2, 14.0, if over { 0xFF2A2B31 } else { 0xFF1E1F25 });
            c.outline(pxx, pyy, pw2, ph2, 14.0, 1.0, theme::HAIRLINE_STRONG);
            let tlh = c.line_height(font::SEMIBOLD, 11.5);
            c.text(font::SEMIBOLD, 11.5, t, pxx + 14.0, pyy + (ph2 - tlh) / 2.0, theme::TEXT_STRONG);
            c.icon(theme::I_CHEVRON_DOWN, 12.0, pxx + pw2 - 16.0, pyy + ph2 / 2.0, theme::TEXT_SECONDARY);
            if over {
                ui.cursor_hand = true;
            }
            if ui.clicked(c, "relay:jump", pxx, pyy, pw2, ph2) {
                self.user_scrolled_up = false;
                sc.snap_bottom(mh);
            }
        }
        if messages.len() as i64 != self.last_count {
            if self.last_count == -1 || !self.user_scrolled_up || messages.last().is_some_and(|m| same(&m.sender_id, me)) {
                sc.snap_bottom(mh);
                self.user_scrolled_up = false;
            }
            self.last_count = messages.len() as i64;
        }

        // Typing, reply / edit bar, staged picture and the composer.
        let typers = relay::typing(&sel);
        let iy = y + h - input_h - 16.0;
        if !typers.is_empty() {
            let t = if typers.len() == 1 { format!("{} is typing", typers[0]) } else { format!("{} people are typing", typers.len()) };
            ui.dots(c, x + pad + 12.0, iy - 12.0, theme::TEXT_SECONDARY);
            let tlh = c.line_height(font::MEDIUM, 11.0);
            c.text(font::MEDIUM, 11.0, &t, x + pad + 30.0, iy - 12.0 - tlh / 2.0, theme::TEXT_SECONDARY);
        }
        if bar_h > 0.0 {
            let (by, bw) = (my0 + mh + 3.0, w - pad * 2.0);
            c.round(x + pad, by, bw, bar_h - 8.0, 9.0, 0xFF121318);
            c.outline(x + pad, by, bw, bar_h - 8.0, 9.0, 1.0, theme::HAIRLINE);
            let bm = by + (bar_h - 8.0) / 2.0;
            let l12 = c.line_height(font::SEMIBOLD, 12.0);
            let l115 = c.line_height(font::REGULAR, 11.5);
            if self.editing_id.is_some() {
                c.icon(theme::I_PENCIL, 13.0, x + pad + 18.0, bm, theme::TEXT_SECONDARY);
                let ew = c.text(font::SEMIBOLD, 12.0, "Editing message", x + pad + 34.0, bm - l12 / 2.0, theme::TEXT_STRONG);
                c.text(font::REGULAR, 11.5, "Enter to save, Esc to cancel", x + pad + 34.0 + ew + 10.0, bm - l115 / 2.0, theme::TEXT_MUTED);
            } else if let Some(rt) = &self.reply_to {
                let q = relay::quote(rt);
                let qn = if q.sender_name.is_empty() { "Someone".to_owned() } else { q.sender_name.clone() };
                let snip = if q.deleted {
                    String::new()
                } else if q.content.is_empty() {
                    if q.media { "Picture".to_owned() } else { String::new() }
                } else {
                    q.content.replace('\n', " ")
                };
                c.icon(theme::I_REPLY_ARROW, 13.0, x + pad + 18.0, bm, theme::TEXT_SECONDARY);
                let rw = c.text(font::SEMIBOLD, 12.0, &format!("Replying to {qn}"), x + pad + 34.0, bm - l12 / 2.0, theme::TEXT_STRONG);
                let sn = c.ellipsize(font::REGULAR, 11.5, &snip, (bw - 34.0 - rw - 70.0).max(20.0));
                c.text(font::REGULAR, 11.5, &sn, x + pad + 34.0 + rw + 10.0, bm - l115 / 2.0, theme::TEXT_MUTED);
            }
            if ui.icon_button(c, "relay:barx", x + pad + bw - 30.0, by + (bar_h - 8.0 - 24.0) / 2.0, 24.0, theme::I_X, Some("Cancel (Esc)")) {
                if self.editing_id.is_some() {
                    self.stop_editing();
                }
                self.reply_to = None;
            }
        }
        let got = lock(&self.pasted).take();
        if let Some(got) = got {
            match sniff_mime(&got) {
                None => self.staged_note = Some("That picture isn't supported.".to_owned()),
                Some(_) if got.len() > 8 * 1024 * 1024 => self.staged_note = Some("Picture is too large (max 8 MB).".to_owned()),
                Some(mime) => {
                    let key = format!("staged-{}", relay::now_ms());
                    chat_media::put_local(&key, got.clone());
                    self.staged = Some(Staged { bytes: got, mime, key });
                    self.staged_note = None;
                }
            }
        }
        let mut unstage = false;
        if let Some(st) = &self.staged {
            let sm = chat_media::local(&st.key);
            let (cw, ch) = ((w - pad * 2.0).min(330.0), 76.0);
            let (cx, cyy) = (x + pad, iy - ch - 10.0 - if typers.is_empty() { 0.0 } else { 14.0 });
            c.shadow(cx, cyy + 4.0, cw, ch, 12.0, 16.0, 0x77000000);
            c.round(cx, cyy, cw, ch, 12.0, 0xFF121318);
            c.outline(cx, cyy, cw, ch, 12.0, 1.0, theme::HAIRLINE_STRONG);
            let tb = ch - 16.0;
            c.round(cx + 8.0, cyy + 8.0, tb, tb, 8.0, 0xFF0B0C10);
            match sm.as_ref().and_then(|m| m.frame(ui.now)) {
                Some(img) => {
                    c.push_clip(cx + 8.0, cyy + 8.0, tb, tb);
                    c.image_cover(img, cx + 8.0, cyy + 8.0, tb, tb, 1.0, 0.0, 0.0, 0xFFFFFFFF);
                    c.pop_clip();
                }
                None => ui.dots(c, cx + 8.0 + tb / 2.0, cyy + 8.0 + tb / 2.0, theme::TEXT_MUTED),
            }
            c.outline(cx + 8.0, cyy + 8.0, tb, tb, 8.0, 1.0, theme::HAIRLINE);
            c.text(font::SEMIBOLD, 12.5, "Picture ready", cx + tb + 22.0, cyy + 18.0, theme::TEXT_STRONG);
            let size = if st.bytes.len() < 1024 { format!("{} B", st.bytes.len()) } else { format!("{} KB", st.bytes.len() / 1024) };
            c.text(font::REGULAR, 11.0, &format!("{size} \u{2014} press Enter to send"), cx + tb + 22.0, cyy + 38.0, theme::TEXT_MUTED);
            if ui.icon_button(c, "relay:unstage", cx + cw - 36.0, cyy + 8.0, 28.0, theme::I_X, Some("Remove")) {
                unstage = true;
            }
        } else if let Some(note) = &self.staged_note {
            c.text(font::MEDIUM, 11.0, note, x + pad, iy - 26.0, 0xFFF87171);
        }
        if unstage {
            self.staged = None;
        }
        if !ui.focus_claimed && ui.focus.is_none() && !self.gif_open && !self.shots_open && ui.interactive {
            self.input.focus(ui);
        }
        let before = self.input.value();
        let btn = 36.0;
        let box_w = w - pad * 2.0;
        if ui.pressed && !self.gif_open && ui.hover(c, x + pad, iy, box_w, input_h) {
            self.input.focus(ui);
            ui.focus_claimed = true;
        }
        let fa = ui.anim_on("relay:composer#f", self.input.focused(ui), 14.0);
        c.round(x + pad, iy, box_w, input_h, 8.0, theme::mix(0xFF101116, 0xFF13141A, fa));
        c.outline(x + pad, iy, box_w, input_h, 8.0, 1.0, theme::mix(theme::HAIRLINE_STRONG, theme::BORDER_HOVER, fa));
        let placeholder = if self.editing_id.is_some() {
            "Edit your message".to_owned()
        } else if self.staged.is_some() {
            "Add a caption (optional)".to_owned()
        } else if group {
            format!("Message #{name}")
        } else {
            format!("Message @{name}")
        };
        let send = self.input.draw(ui, c, x + pad + 4.0, iy, box_w - btn * 3.0 - 18.0, input_h, &placeholder, 13.5);
        let by0 = iy + (input_h - btn) / 2.0;
        let now_val = self.input.value();
        if now_val != before && !now_val.is_empty() {
            relay::typing_ping(&sel);
        }
        // Attach: pick one of the newest screenshots.
        let ax = x + w - pad - btn * 3.0 - 13.0;
        let a_over = ui.hover(c, ax, by0, btn, btn);
        let a_hv = ui.anim_on("relay:attach", a_over || self.shots_open, 14.0);
        let a_click = ui.clicked(c, "relay:attach", ax, by0, btn, btn);
        c.round(ax, by0, btn, btn, 7.0, theme::alpha(0xFFFFFFFF, 0.1 * a_hv));
        c.icon(theme::I_PLUS, 17.0, ax + btn / 2.0, by0 + btn / 2.0, if self.staged.is_some() || a_hv > 0.5 { theme::TEXT_STRONG } else { theme::TEXT_SECONDARY });
        if a_over {
            ui.cursor_hand = true;
            ui.tip("Send a screenshot");
        }
        if a_click {
            self.shots_open = !self.shots_open;
            self.gif_open = false;
            self.staged_note = None;
            if self.shots_open {
                self.load_shots();
            }
        }
        // GIF button.
        let gx = x + w - pad - btn * 2.0 - 9.0;
        let g_over = ui.hover(c, gx, by0, btn, btn);
        let g_hv = ui.anim_on("relay:gifbtn", g_over || self.gif_open, 14.0);
        let g_click = ui.clicked(c, "relay:gifbtn", gx, by0, btn, btn);
        c.round(gx, by0, btn, btn, 7.0, theme::alpha(0xFFFFFFFF, 0.1 * g_hv));
        let gw = c.text_width(font::BOLD, 11.5, "GIF");
        let glh = c.line_height(font::BOLD, 11.5);
        c.text(font::BOLD, 11.5, "GIF", gx + (btn - gw) / 2.0, by0 + (btn - glh) / 2.0, if self.gif_open || g_hv > 0.5 { theme::TEXT_STRONG } else { theme::TEXT_SECONDARY });
        if g_over {
            ui.cursor_hand = true;
        }
        if g_click {
            self.gif_open = !self.gif_open;
            self.shots_open = false;
            if self.gif_open {
                self.gif_query = None;
                self.gif_search.clear();
                self.gif_search.focus(ui);
            } else {
                self.input.focus(ui);
            }
            ui.focus_claimed = true;
        }
        let can_send = !self.input.value().trim().is_empty() || self.staged.is_some();
        let sx = x + w - pad - btn - 5.0;
        let s_over = ui.hover(c, sx, by0, btn, btn);
        let hv = ui.anim_on("relay:send", s_over && can_send, 14.0);
        let ready = ui.anim_on("relay:send#on", can_send, 14.0);
        let s_click = ui.clicked(c, "relay:sendbtn", sx, by0, btn, btn);
        c.round(sx, by0, btn, btn, 7.0, theme::mix(theme::SUBTLE, theme::mix(0xFFE4E4E7, 0xFFFFFFFF, hv), ready));
        c.icon(theme::I_SEND, 16.0, sx + btn / 2.0, by0 + btn / 2.0, theme::mix(theme::TEXT_MUTED, theme::SOLID_FG, ready));
        if s_over && can_send {
            ui.cursor_hand = true;
        }
        if (send || s_click) && can_send {
            let text = self.input.value();
            if let Some(eid) = self.editing_id.clone() {
                if !text.trim().is_empty() {
                    relay::edit(&sel, &eid, &text);
                }
                self.stop_editing();
            } else {
                if let Some(st) = self.staged.take() {
                    relay::send_image(&sel, st.bytes, st.mime, &text, self.reply_to.as_ref());
                } else if !text.trim().is_empty() {
                    relay::send(&sel, &text, self.reply_to.as_ref());
                }
                self.reply_to = None;
                self.input.clear();
                self.forget_draft();
            }
            sc.to_bottom();
            self.input.focus(ui);
            ui.focus_claimed = true;
        }
        self.chat_scrolls.insert(sel.clone(), sc);
        if self.gif_open {
            self.gif_picker(ui, c, &sel, &api, x + w - pad - 372.0, iy - 360.0 - 8.0, 372.0, 360.0);
        }
        if self.shots_open {
            self.shots_picker(ui, c, ax, iy, x + w - pad);
        }
        let _ = cfg;
        false
    }

    /// The pill of tools at the top right of the hovered message: react, reply, edit (yours), delete.
    #[allow(clippy::too_many_arguments)]
    fn tools(&mut self, ui: &mut Ui, c: &mut Canvas, sel: &str, m: &Message, _group: bool, mine: bool, moderator: bool, right: f32, y: f32) {
        let sent = !m.id.is_empty() && !m.id.starts_with("local-") && !m.pending;
        let has_media = m.media_url.is_some() || m.local_key.is_some();
        let mut buttons: Vec<(usize, u32)> = Vec::new();
        if !m.deleted {
            if sent {
                buttons.push((0, theme::I_SMILE));
                buttons.push((1, theme::I_REPLY_ARROW));
            }
            if mine && sent && !has_media {
                buttons.push((2, theme::I_PENCIL));
            }
        }
        if ((mine || moderator) && !m.deleted && !m.pending) || (mine && m.failed) {
            buttons.push((3, theme::I_TRASH));
        }
        if buttons.is_empty() {
            return;
        }
        let (bs, pad) = (24.0f32, 3.0f32);
        let w = buttons.len() as f32 * bs + pad * 2.0;
        let x = right - w;
        c.shadow(x, y + 2.0, w, bs + pad * 2.0 - 2.0, 9.0, 10.0, 0x77000000);
        c.round(x, y, w, bs + 2.0, 9.0, 0xFF17181D);
        c.outline(x, y, w, bs + 2.0, 9.0, 1.0, theme::HAIRLINE_STRONG);
        self.toolbar_rect = Some([x, y, w, bs + 2.0]);
        if self.react_for.as_deref() == Some(m.id.as_str()) {
            self.pick_row = Some([x, y, w]);
        }
        const TIPS: [&str; 4] = ["Add reaction", "Reply", "Edit message", "Delete message"];
        for (i, (a, icon)) in buttons.iter().enumerate() {
            let bx = x + pad + i as f32 * bs;
            let over = ui.hover(c, bx, y + 1.0, bs, bs);
            let id = format!("relay:tool:{}:{a}", m.id);
            let click = ui.clicked(c, &id, bx, y + 1.0, bs, bs);
            let hv = ui.anim_on(&format!("{id}#h"), over, 16.0);
            let danger = *a == 3;
            c.round(bx + 1.0, y + 2.0, bs - 2.0, bs - 2.0, 7.0, theme::alpha(if danger { 0xFFEF4444 } else { 0xFFFFFFFF }, hv * if danger { 0.18 } else { 0.1 }));
            c.icon(
                *icon,
                13.5,
                bx + bs / 2.0,
                y + 1.0 + bs / 2.0,
                if danger { theme::mix(theme::TEXT_SECONDARY, 0xFFF87171, hv) } else { theme::mix(theme::TEXT_SECONDARY, theme::TEXT_STRONG, hv) },
            );
            if over {
                ui.cursor_hand = true;
                ui.tip(TIPS[*a]);
            }
            if !click {
                continue;
            }
            match a {
                0 => {
                    self.react_for = if self.react_for.as_deref() == Some(m.id.as_str()) { None } else { Some(m.id.clone()) };
                    if self.react_for.is_some() {
                        self.pick_row = Some([x, y, w]);
                    }
                }
                1 => {
                    self.reply_to = Some(m.clone());
                    self.editing_id = None;
                    self.input.focus(ui);
                    ui.focus_claimed = true;
                }
                2 => {
                    if self.editing_id.is_none() {
                        self.save_draft();
                    }
                    self.editing_id = Some(m.id.clone());
                    self.reply_to = None;
                    self.input.set(&m.content);
                    self.input.focus(ui);
                    ui.focus_claimed = true;
                }
                _ => {
                    if self.editing_id.as_deref() == Some(m.id.as_str()) {
                        self.stop_editing();
                    }
                    relay::delete(sel, &m.id);
                }
            }
        }
    }

    /// The row of emoji above a message's tools; picking one toggles your reaction.
    fn picker(&mut self, ui: &mut Ui, c: &mut Canvas, sel: &str, message_id: &str, row: [f32; 3], top: f32) {
        let (cell, pad) = (32.0f32, 6.0f32);
        let (w, h) = (PALETTE.len() as f32 * cell + pad * 2.0, cell + pad * 2.0);
        let (tx, ty, tw) = (row[0], row[1], row[2]);
        let mut x = tx + tw - w;
        let mut y = ty - h - 6.0;
        if y < top + 2.0 {
            y = ty + 30.0;
        }
        x = x.max(8.0);
        self.picker_rect = Some([x, y, w, h]);
        c.shadow(x, y + 4.0, w, h, 14.0, 18.0, 0x99000000);
        c.round(x, y, w, h, 14.0, 0xFF14151A);
        c.outline(x, y, w, h, 14.0, 1.0, theme::HAIRLINE_STRONG);
        let mut outside = true;
        for (i, e) in PALETTE.iter().enumerate() {
            let bx = x + pad + i as f32 * cell;
            let over = ui.hover(c, bx, y + pad, cell, cell);
            if over {
                outside = false;
            }
            let hv = ui.anim_on(&format!("relay:pick:{i}#h"), over, 18.0);
            let size = 20.0 + 5.0 * (hv * 3.0).round() / 3.0;
            c.round(bx + 1.0, y + pad + 1.0, cell - 2.0, cell - 2.0, 9.0, theme::alpha(0xFFFFFFFF, 0.08 * hv));
            draw_emoji(c, e, bx + (cell - size) / 2.0, y + pad + (cell - size) / 2.0 - hv, size);
            if over {
                ui.cursor_hand = true;
            }
            if ui.clicked(c, &format!("relay:pick:{i}"), bx, y + pad, cell, cell) {
                relay::react(sel, message_id, e);
                self.react_for = None;
                return;
            }
        }
        let in_tools = self.toolbar_rect.is_some_and(|r| ui.hover(c, r[0], r[1], r[2], r[3]));
        if ui.pressed && outside && !ui.hover(c, x, y, w, h) && !in_tools {
            self.react_for = None;
        }
    }

    /// The reaction chips under a message; click one to toggle it. Returns the y below them.
    #[allow(clippy::too_many_arguments)]
    fn reactions(&mut self, ui: &mut Ui, c: &mut Canvas, sel: &str, m: &Message, me: Option<&str>, x: f32, y: f32, width: f32, top: f32, view_h: f32) -> f32 {
        let (mut cx, mut cy, ch) = (x, y, 22.0f32);
        for (emoji, n, mine) in grouped(m, me) {
            let label = n.to_string();
            let cw = 10.0 + 14.0 + 5.0 + c.text_width(font::SEMIBOLD, 11.5, &label) + 10.0;
            if cx + cw > x + width && cx > x {
                cx = x;
                cy += ch + 4.0;
            }
            if visible(cy, ch, top, view_h) {
                let over = ui.hover(c, cx, cy, cw, ch);
                let fill = if mine { theme::alpha(0xFF60A5FA, if over { 0.3 } else { 0.2 }) } else if over { 0xFF26272D } else { theme::SUBTLE };
                c.round(cx, cy, cw, ch, 11.0, fill);
                c.outline(cx, cy, cw, ch, 11.0, 1.0, if mine { theme::alpha(0xFF60A5FA, 0.6) } else { theme::HAIRLINE });
                draw_emoji(c, &emoji, cx + 8.0, cy + 4.0, 14.0);
                let lh = c.line_height(font::SEMIBOLD, 11.5);
                c.text(font::SEMIBOLD, 11.5, &label, cx + 27.0, cy + (ch - lh) / 2.0, if mine { theme::TEXT_STRONG } else { theme::TEXT_SECONDARY });
                if over {
                    ui.cursor_hand = true;
                }
                if ui.clicked(c, &format!("relay:re:{}:{emoji}", m.id), cx, cy, cw, ch) {
                    relay::react(sel, &m.id, &emoji);
                }
            }
            cx += cw + 6.0;
        }
        cy + ch
    }

    /// Draws a picture message (or its loading box); returns its height.
    #[allow(clippy::too_many_arguments)]
    fn media(&mut self, ui: &mut Ui, c: &mut Canvas, api: &str, m: &Message, x: f32, y: f32, max_w: f32, top: f32, view_h: f32) -> f32 {
        let Some(key) = self.media_key(m) else { return 0.0 };
        let (mut dw, mut dh) = match self.known_size(m, &key) {
            Some((w, h)) => (w, if h < 0.0 { 54.0 } else { h }),
            None => (240.0, 150.0),
        };
        let near = visible(y - view_h, dh + view_h * 2.0, top, view_h);
        let md = media_of(&key, api, near);
        if self.media_sizes.len() > 2000 {
            self.media_sizes.clear();
        }
        if let Some(md) = &md {
            if md.failed {
                self.media_sizes.insert(key.clone(), (240.0, -1.0));
                dw = 240.0;
                dh = 54.0;
            } else if md.width > 0 {
                let (mw, mh) = (md.width as f32, md.height as f32);
                let k = 1f32.min((max_w / mw).min(300.0 / mh));
                let up = if mw < 160.0 && mh < 160.0 { 1.0 } else { k };
                dw = mw * up;
                dh = mh * up;
                if dw > max_w {
                    dh *= max_w / dw;
                    dw = max_w;
                }
                self.media_sizes.insert(key.clone(), (dw, dh));
                if let Some(u) = &m.media_url
                    && *u != key
                {
                    self.media_sizes.insert(u.clone(), (dw, dh));
                }
            }
        }
        let Some(md) = md else {
            if visible(y, dh, top, view_h) {
                c.skeleton(x, y, dw, dh, 10.0);
            }
            return dh;
        };
        if !visible(y, dh, top, view_h) {
            return dh;
        }
        let id = if m.id.is_empty() { key.clone() } else { m.id.clone() };
        if md.failed {
            c.round(x, y, 240.0, 54.0, 10.0, 0xFF121318);
            c.outline(x, y, 240.0, 54.0, 10.0, 1.0, theme::HAIRLINE);
            c.text(font::MEDIUM, 11.5, "Picture unavailable", x + 14.0, y + 19.0, theme::TEXT_MUTED);
            if let Some(u) = &m.media_url
                && ui.interactive
                && ui.my >= top
                && ui.my <= top + view_h
            {
                if ui.hover(c, x, y, 240.0, 54.0) {
                    ui.cursor_hand = true;
                }
                if ui.clicked(c, &format!("relay:img:fail:{id}"), x, y, 240.0, 54.0) {
                    common::open_url(u);
                }
            }
            return 54.0;
        }
        let Some(frame) = md.frame(ui.now) else {
            c.skeleton(x, y, dw, dh, 10.0);
            return dh;
        };
        let a = if m.pending { 0.55 } else { 1.0 };
        c.push_clip(x, y, dw, dh);
        c.image(frame, x, y, dw, dh, [0.0, 0.0, 1.0, 1.0], theme::alpha(0xFFFFFFFF, a));
        c.pop_clip();
        c.outline(x, y, dw, dh, 10.0, 1.0, theme::HAIRLINE);
        if m.pending {
            ui.dots(c, x + dw / 2.0, y + dh / 2.0, theme::TEXT_STRONG);
        }
        let under_pill = self.jump_rect.is_some_and(|r| ui.mx >= r[0] && ui.mx < r[0] + r[2] && ui.my >= r[1] && ui.my < r[1] + r[3]);
        if self.viewer.is_none() && !self.gif_open && !under_pill && ui.interactive && ui.my >= top && ui.my <= top + view_h {
            if ui.hover(c, x, y, dw, dh) {
                ui.cursor_hand = true;
            }
            if ui.clicked(c, &format!("relay:img:{id}"), x, y, dw, dh) {
                self.viewer = Some(md.clone());
                self.viewer_url = m.media_url.clone();
            }
        }
        dh
    }

    pub(super) fn lightbox(&mut self, ui: &mut Ui, c: &mut Canvas, w_all: f32, h_all: f32) {
        ui.interactive = true;
        c.fill(0.0, 0.0, w_all, h_all, 0xE6000000);
        if let Some(v) = &self.viewer
            && let Some(frame) = v.frame(ui.now)
            && v.width > 0
        {
            let k = ((w_all - 80.0) / v.width as f32).min((h_all - 120.0) / v.height as f32).min(4.0);
            let (dw, dh) = (v.width as f32 * k, v.height as f32 * k);
            c.image(frame, (w_all - dw) / 2.0, (h_all - dh) / 2.0 - 14.0, dw, dh, [0.0, 0.0, 1.0, 1.0], 0xFFFFFFFF);
        }
        let hint = "Click anywhere or press Esc to close";
        let hw = c.text_width(font::MEDIUM, 11.5, hint);
        c.text(font::MEDIUM, 11.5, hint, (w_all - hw) / 2.0, h_all - 28.0, theme::TEXT_SECONDARY);
        if let Some(u) = self.viewer_url.clone() {
            let (bw, bh) = (130.0, 28.0);
            if ui.button(c, "relay:lightbox:open", (w_all - bw) / 2.0, h_all - 64.0, bw, bh, "Open in browser", theme::I_GLOBE, ui::BTN_GLASS) {
                common::open_url(&u);
            }
        }
        if ui.pressed && !ui.hover(c, (w_all - 140.0) / 2.0, h_all - 70.0, 140.0, 36.0) {
            self.viewer = None;
            self.viewer_url = None;
            ui.pressed = false;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn gif_picker(&mut self, ui: &mut Ui, c: &mut Canvas, sel: &str, api: &str, x: f32, y: f32, w: f32, h: f32) {
        if ui.pressed && !ui.hover(c, x, y, w, h) && !ui.hover(c, x + w - 130.0, y + h + 8.0, 130.0, 60.0) {
            self.gif_open = false;
            return;
        }
        c.shadow(x, y + 8.0, w, h, 16.0, 28.0, 0xAA000000);
        c.round(x, y, w, h, 16.0, 0xFF101115);
        c.outline(x, y, w, h, 16.0, 1.0, theme::HAIRLINE_STRONG);
        self.gif_search.draw(ui, c, x + 12.0, y + 12.0, w - 24.0, 36.0, "Search GIFs", 12.5);
        let q = self.gif_search.value().trim().to_owned();
        if self.gif_query.as_deref() != Some(q.as_str()) {
            if self.gif_query.is_none() {
                self.gif_typed = 0.0;
            } else if self.gif_typed == 0.0 {
                self.gif_typed = ui.now;
            }
            if self.gif_query.is_none() || ui.now - self.gif_typed > 350.0 {
                self.gif_query = Some(q.clone());
                self.gif_typed = 0.0;
                self.gif_serial += 1;
                let serial = self.gif_serial;
                let state = self.gif_results.clone();
                {
                    let mut g = lock(&state);
                    g.serial = serial;
                    g.loading = true;
                    g.failed = false;
                }
                let _ = std::thread::Builder::new().name("Native gifs".into()).spawn(move || {
                    let r = relay::gifs(&q);
                    let mut g = lock(&state);
                    if g.serial != serial {
                        return;
                    }
                    match r {
                        Ok(v) => g.results = Some(v),
                        Err(_) => {
                            g.failed = true;
                            g.results = Some(Vec::new());
                        }
                    }
                    g.loading = false;
                });
            }
        }
        let (gy, gh) = (y + 58.0, h - 58.0 - 8.0);
        let (results, failed) = {
            let g = lock(&self.gif_results);
            (g.results.clone(), g.failed)
        };
        let off = self.gif_scroll.begin(ui, c, x, gy, w, gh);
        let cw = (w - 12.0 * 2.0 - 8.0) / 2.0;
        match results {
            Some(res) if !res.is_empty() => {
                let mut col_y = [gy - off, gy - off];
                for (url, preview, title) in &res {
                    let col = usize::from(col_y[0] > col_y[1]);
                    let md = chat_media::get(preview, api);
                    let ready = md.as_ref().is_some_and(|m| !m.failed && m.width > 0);
                    let th = match &md {
                        Some(m) if ready => (cw * m.height as f32 / m.width as f32).clamp(50.0, 180.0),
                        _ => 100.0,
                    };
                    let tx = x + 12.0 + col as f32 * (cw + 8.0);
                    let ty = col_y[col];
                    if ty + th >= gy && ty <= gy + gh {
                        match md.as_ref().and_then(|m| m.frame(ui.now)) {
                            None => c.skeleton(tx, ty, cw, th, 10.0),
                            Some(f) => {
                                c.push_clip(tx, ty, cw, th);
                                c.image_cover(f, tx, ty, cw, th, 1.0, 0.0, 0.0, 0xFFFFFFFF);
                                c.pop_clip();
                                c.outline(tx, ty, cw, th, 10.0, 1.0, theme::HAIRLINE);
                                if ui.hover(c, tx, ty, cw, th) && ui.my >= gy && ui.my <= gy + gh {
                                    c.round(tx, ty, cw, th, 10.0, 0x22FFFFFF);
                                    ui.cursor_hand = true;
                                }
                                if ui.my >= gy && ui.my <= gy + gh && ui.clicked(c, &format!("relay:gif:{url}"), tx, ty, cw, th) {
                                    relay::send_gif(sel, url, title);
                                    self.gif_open = false;
                                    if let Some(s) = self.chat_scrolls.get_mut(sel) {
                                        s.to_bottom();
                                    }
                                }
                            }
                        }
                    }
                    col_y[col] += th + 8.0;
                }
                self.gif_scroll.end(ui, c, col_y[0].max(col_y[1]) + off - gy + 4.0);
            }
            Some(_) => {
                let t = if failed { "GIF search is unavailable" } else { "No GIFs found" };
                let tw = c.text_width(font::MEDIUM, 12.0, t);
                c.text(font::MEDIUM, 12.0, t, x + (w - tw) / 2.0, gy + 60.0, theme::TEXT_MUTED);
                self.gif_scroll.end(ui, c, 0.0);
            }
            None => {
                for i in 0..6 {
                    c.skeleton(x + 12.0 + (i % 2) as f32 * (cw + 8.0), gy + (i / 2) as f32 * 108.0 - off, cw, 100.0, 10.0);
                }
                self.gif_scroll.end(ui, c, 0.0);
            }
        }
    }

    /// Lists the newest screenshots and decodes their thumbnails off-thread.
    fn load_shots(&mut self) {
        *lock(&self.shots) = None;
        let (shots, thumbs) = (self.shots.clone(), self.shot_thumbs.clone());
        let dir = super::game_dir().map(|d| d.join("screenshots"));
        let _ = std::thread::Builder::new().name("Native shots".into()).spawn(move || {
            let mut files: Vec<(std::path::PathBuf, i64)> = Vec::new();
            if let Some(entries) = dir.and_then(|d| std::fs::read_dir(d).ok()) {
                for e in entries.flatten() {
                    let path = e.path();
                    let n = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
                    if !(n.ends_with(".png") || n.ends_with(".jpg") || n.ends_with(".jpeg")) {
                        continue;
                    }
                    let modified = e
                        .metadata()
                        .ok()
                        .and_then(|m| m.modified().ok())
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map_or(0, |d| d.as_millis() as i64);
                    files.push((path, modified));
                }
            }
            files.sort_by_key(|f| std::cmp::Reverse(f.1));
            files.truncate(9);
            *lock(&shots) = Some(files.clone());
            for (path, modified) in files {
                let key = format!("{}|{modified}", path.display());
                if lock(&thumbs).contains_key(&key) {
                    continue;
                }
                let Some(img) = std::fs::read(&path).ok().and_then(|b| Image::decode(&b)) else { continue };
                let k = (240.0 / img.width as f32).min(1.0);
                let mut t = img.scaled(((img.width as f32 * k).round() as u32).max(1), ((img.height as f32 * k).round() as u32).max(1));
                if let Some(i) = Arc::get_mut(&mut t) {
                    i.linear = true;
                }
                lock(&thumbs).insert(key, t);
            }
        });
    }

    fn shots_picker(&mut self, ui: &mut Ui, c: &mut Canvas, anchor_x: f32, anchor_y: f32, right: f32) {
        let pw = 388.0f32;
        let tile = (pw - 24.0 - 16.0) / 3.0;
        let th = tile * 9.0 / 16.0;
        let list = lock(&self.shots).clone();
        let n = list.as_ref().map_or(0, Vec::len);
        let rows = if n == 0 { 1 } else { n.div_ceil(3) };
        let ph = 52.0 + if n == 0 { 96.0 } else { rows as f32 * (th + 8.0) } + 8.0;
        let (px, py) = ((right - pw).max(8.0), anchor_y - ph - 8.0);
        c.shadow(px, py + 6.0, pw, ph, 9.0, 28.0, 0x88000000);
        c.round(px, py, pw, ph, 9.0, 0xFF121318);
        c.outline(px, py, pw, ph, 9.0, 1.0, theme::HAIRLINE_STRONG);
        c.text(font::SEMIBOLD, 13.0, "Screenshots", px + 14.0, py + 16.0, theme::TEXT_STRONG);
        let gy = py + 46.0;
        match list {
            None => ui.dots(c, px + pw / 2.0, gy + 40.0, theme::TEXT_MUTED),
            Some(files) if files.is_empty() => {
                c.text(font::MEDIUM, 12.0, "No screenshots yet", px + 14.0, gy + 24.0, theme::TEXT_SECONDARY);
                c.text(font::REGULAR, 11.5, "Press F2 in game to take one, then pick it here.", px + 14.0, gy + 44.0, theme::TEXT_MUTED);
            }
            Some(files) => {
                for (i, (path, modified)) in files.iter().enumerate() {
                    let (tx, ty) = (px + 12.0 + (i % 3) as f32 * (tile + 8.0), gy + (i / 3) as f32 * (th + 8.0));
                    let over = ui.hover(c, tx, ty, tile, th);
                    let hv = ui.anim_on(&format!("relay:shot:{i}"), over, 16.0);
                    c.round(tx, ty, tile, th, 8.0, 0xFF08090C);
                    let thumb = lock(&self.shot_thumbs).get(&format!("{}|{modified}", path.display())).cloned();
                    match thumb {
                        Some(img) => c.image_cover(&img, tx, ty, tile, th, 1.0, 0.0, 0.0, 0xFFFFFFFF),
                        None => ui.dots(c, tx + tile / 2.0, ty + th / 2.0, theme::TEXT_MUTED),
                    }
                    c.outline(tx, ty, tile, th, 8.0, 1.0, theme::mix(theme::HAIRLINE, 0xCCFFFFFF, hv));
                    if over {
                        ui.cursor_hand = true;
                        ui.tip(&format!("{}, {}", common::short_day(*modified), common::hhmm(*modified)));
                    }
                    if ui.clicked(c, &format!("relay:shot:{i}"), tx, ty, tile, th) {
                        self.shots_open = false;
                        let (pasted, path) = (self.pasted.clone(), path.clone());
                        let _ = std::thread::Builder::new().name("Native shot".into()).spawn(move || {
                            if let Ok(bytes) = std::fs::read(path) {
                                *lock(&pasted) = Some(bytes);
                            }
                        });
                    }
                }
            }
        }
        if ui.pressed && !ui.hover(c, px, py, pw, ph) && !ui.hover(c, anchor_x, anchor_y, 46.0, 46.0) {
            self.shots_open = false;
        }
    }
}

fn skeleton_messages(c: &mut Canvas, x: f32, y: f32, w: f32, h: f32) {
    let mut cy = y;
    let mut i = 0usize;
    while cy < y + h - 40.0 && i < 8 {
        c.skeleton(x, cy + 1.0, 34.0, 34.0, 10.0);
        c.skeleton(x + 46.0, cy, (80 + (i * 29) % 50) as f32, 11.0, 5.0);
        let lines = 1 + (i * 7) % 3;
        for l in 0..lines {
            c.skeleton(x + 46.0, cy + 22.0 + l as f32 * 18.0, (w - 60.0).min((120 + ((i + l) * 71) % 260) as f32), 10.0, 5.0);
        }
        cy += 36.0 + lines as f32 * 18.0 + 12.0;
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reactions_group_by_emoji_and_remember_yours() {
        let m = Message {
            reactions: vec![("a".into(), "x".into()), ("b".into(), "y".into()), ("me".into(), "x".into())],
            ..Message::default()
        };
        let g = grouped(&m, Some("me"));
        assert_eq!(g, vec![("x".to_owned(), 2, true), ("y".to_owned(), 1, false)]);
        assert!(grouped(&m, None).iter().all(|r| !r.2));
    }

    #[test]
    fn pictures_are_told_apart_by_their_first_bytes() {
        assert_eq!(sniff_mime(&[0x89, b'P', b'N', b'G', 0, 0]), Some("image/png"));
        assert_eq!(sniff_mime(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(sniff_mime(b"GIF89a"), Some("image/gif"));
        assert_eq!(sniff_mime(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(sniff_mime(b"hello"), None);
    }

    #[test]
    fn every_quick_reaction_has_a_picture() {
        for e in PALETTE {
            assert!(emoji_image(e).is_some(), "no picture for {e}");
        }
    }

    #[test]
    fn local_uploads_never_hit_the_network_cache() {
        assert!(is_local_key("local-123"));
        assert!(is_local_key("staged-1"));
        assert!(!is_local_key("https://x/y.png"));
    }
}
