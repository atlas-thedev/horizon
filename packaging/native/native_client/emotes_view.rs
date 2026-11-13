//! The Emotes tab of the Native menu: the in-game twin of the launcher's emote locker (the Java mod's EmotesView).
//! The wheel's eight slots on top (more pages with Native Plus), your emotes under them and a few to try on. Pick a
//! slot, then an emote to put it there; drag emotes onto slots, drag slots onto each other to swap, or drag a slot off
//! the wheel to empty it. The stage on the left plays the emote on your own skin, and in a world it can play it for
//! everyone.
use super::emote_wheel::poster_url;
use super::locker::session_lock;
use super::menu::{CARD, CARD_HOVER, clamp01, ease, empty, glow};
use super::net::{self, Pic};
use super::preview3d;
use super::scroll::Scroll;
use super::textfield::TextField;
use super::ui::{self, Ui};
use horizon_client::native_emotes::{self, WheelPage};
use horizon_client::native_ui::{Canvas, Image, font, theme};
use serde_json::Value;
use std::sync::Arc;

/// One emote of the account's catalogue (`me.catalog`).
#[derive(Clone)]
struct Emote {
    id: String,
    name: String,
    hash: String,
    /// In the player's locker; free emotes not yet claimed and store emotes are not.
    owned: bool,
    free: bool,
    exclusive: bool,
    raw: Value,
}

impl Emote {
    /// "dance_floor" -> "dance floor", as the launcher shows names.
    fn label(&self) -> String {
        self.name.replace(['_', '-'], " ")
    }
}

fn catalog(me: &Value) -> Vec<Emote> {
    me["catalog"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|o| {
            Some(Emote {
                id: o["id"].as_str()?.to_owned(),
                name: o["name"].as_str().unwrap_or("Emote").to_owned(),
                hash: o["hash"].as_str().unwrap_or("").to_owned(),
                // an older reply (or cache) without the flag lists only what the player has
                owned: o["owned"].as_bool().unwrap_or(true),
                free: o["free"].as_bool().unwrap_or(false),
                exclusive: o["exclusive"].as_bool().unwrap_or(false),
                raw: o.clone(),
            })
        })
        .collect()
}

fn find<'a>(items: &'a [Emote], id: Option<&str>) -> Option<&'a Emote> {
    let id = id?;
    items.iter().find(|e| e.id == id)
}

fn index_of(row: &WheelPage, id: &str) -> Option<usize> {
    row.iter().position(|s| s.as_deref() == Some(id))
}

/// After filling a slot, the next empty one after it (round the wheel), or the same slot when all are full.
fn next_open(row: &WheelPage, after: usize) -> usize {
    (1..=8).map(|k| (after + k) % 8).find(|&i| row[i].is_none()).unwrap_or(after)
}

pub struct EmotesView {
    page: usize,
    slot: usize,
    /// The emote on the stage (its id), and since when it plays.
    pick: Option<String>,
    pick_at: f64,
    equipped_only: bool,
    shown_at: f64,
    /// Dragging an emote, from a wheel slot (`drag_from`) or a card (`None`); armed until the mouse moves.
    drag_id: Option<String>,
    drag_from: Option<usize>,
    drag_x: f32,
    drag_y: f32,
    dragging: bool,
    /// The slot under the mouse this frame (set while the wheel draws), for drops.
    hovered_slot: Option<usize>,
    scroll: Scroll,
    search: TextField,
}

impl Default for EmotesView {
    fn default() -> Self {
        Self::new()
    }
}

impl EmotesView {
    pub fn new() -> Self {
        let mut search = TextField::new("emotes:search");
        search.max_length = 40;
        search.bare = true;
        EmotesView {
            page: 0,
            slot: 0,
            pick: None,
            pick_at: 0.0,
            equipped_only: false,
            shown_at: 0.0,
            drag_id: None,
            drag_from: None,
            drag_x: 0.0,
            drag_y: 0.0,
            dragging: false,
            hovered_slot: None,
            scroll: Scroll::new("emotes"),
            search,
        }
    }

    /// The tab was opened: ask for the account's emotes again.
    pub fn opened(&mut self, now: f64) {
        self.shown_at = now;
        self.drag_id = None;
        self.dragging = false;
        native_emotes::request_refresh();
    }

    /// Esc: drops a drag, then leaves the search box. True when it was consumed.
    pub fn escape(&mut self, ui: &mut Ui) -> bool {
        if self.dragging || self.drag_id.is_some() {
            self.drag_id = None;
            self.dragging = false;
            return true;
        }
        if self.search.focused(ui) {
            ui.focus = None;
            self.search.clear();
            return true;
        }
        false
    }

    fn show(&mut self, it: &Emote, now: f64) {
        self.pick = Some(it.id.clone());
        self.pick_at = now;
    }

    /// The page. True when an emote was played in the world (the menu then closes).
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&mut self, ui: &mut Ui, c: &mut Canvas, who: &str, x: f32, y: f32, w: f32, h: f32, in_world: bool) -> bool {
        if !net::signed_in() {
            session_lock(ui, c, "emotes:lock", x, y, w, h);
            return false;
        }
        let me = native_emotes::snapshot();
        if !me.is_object() {
            ui.dots(c, x + w / 2.0, y + h / 2.0, theme::TEXT_SECONDARY);
            return false;
        }
        let items = catalog(&me);
        if items.is_empty() {
            empty(c, x, y, w, h.min(280.0), theme::I_SMILE, "No emotes yet", "Get emotes in the Native Store in the launcher, then put them on your wheel here.");
            return false;
        }
        let pages = native_emotes::pages_of(&me);
        let limit = me["pageLimit"].as_u64().unwrap_or(1).max(1) as usize;
        let usable = (me["usablePages"].as_u64().unwrap_or(1).max(1) as usize).min(pages.len());
        let page_count = pages.len().min(limit).max(1);
        self.page = self.page.min(page_count - 1);
        let row = pages[self.page.min(pages.len() - 1)].clone();
        if find(&items, self.pick.as_deref()).is_none()
            && let Some(it) = find(&items, row[self.slot].as_deref()).cloned()
        {
            self.show(&it, ui.now);
        }
        let name = native_emotes::local_name();
        let (skin, slim) = preview3d::wheel_skin(if name.is_empty() { who } else { &name });

        let sw = (w * 0.34).min(300.0);
        let close = self.stage(ui, c, &items, &row, &pages, usable, in_world, &skin, slim, x, y, sw, h);

        let (lx, lw) = (x + sw + 20.0, w - sw - 20.0);
        let is_plus = me["isPlus"].as_bool().unwrap_or(false);
        let wy = self.wheel(ui, c, &items, &row, &pages, page_count, limit, usable, is_plus, &skin, slim, lx, y, lw);

        // the player's emotes, then a few to try on
        let fy = wy + 18.0;
        c.text(font::SEMIBOLD, 10.5, "YOUR EMOTES", lx, fy, theme::TEXT_MUTED);
        let mut chip_x = lx + c.text_width(font::SEMIBOLD, 10.5, "YOUR EMOTES") + 14.0;
        chip_x += self.chip(ui, c, "emotes:all", chip_x, fy - 7.0, "All", !self.equipped_only) + 6.0;
        self.chip(ui, c, "emotes:equipped", chip_x, fy - 7.0, "On this page", self.equipped_only);
        let search_w = (lw * 0.4).min(220.0);
        self.search_box(ui, c, lx + lw - search_w, fy - 10.0, search_w, 30.0);

        let needle = self.search.value().trim().to_lowercase();
        let mut mine: Vec<&Emote> = Vec::new();
        let mut others: Vec<&Emote> = Vec::new();
        for it in &items {
            if !needle.is_empty() && !it.label().to_lowercase().contains(&needle) {
                continue;
            }
            if it.owned {
                if !self.equipped_only || index_of(&row, &it.id).is_some() {
                    mine.push(it);
                }
            } else if !self.equipped_only && others.len() < 8 {
                others.push(it);
            }
        }
        let save_error = native_emotes::save_error();
        let gy = fy + 30.0;
        let gh = y + h - gy - if save_error.is_some() { 22.0 } else { 0.0 };
        let over_grid = ui.hover(c, lx, gy, lw, gh);
        let gap = 10.0;
        let cols = (((lw + gap) / (112.0 + gap)) as usize).max(3);
        let tw = (lw - gap * (cols - 1) as f32) / cols as f32;
        let th = tw * 0.9 + 30.0;
        let off = self.scroll.begin(ui, c, lx, gy, lw + 6.0, gh);
        let cy = gy - off;
        let mut n = 0usize;
        for (i, it) in mine.iter().enumerate() {
            let ix = lx + (i % cols) as f32 * (tw + gap);
            let iy = cy + (i / cols) as f32 * (th + gap);
            if iy + th >= gy - 4.0 && iy <= gy + gh + 4.0 {
                let e = ease(clamp01(((ui.now - self.shown_at - 30.0 - n as f64 * 18.0) / 280.0) as f32));
                c.push_alpha(e);
                self.card(ui, c, it, &row, &pages, &skin, slim, ix, iy + (1.0 - e) * 8.0, tw, th);
                c.pop_alpha();
            }
            n += 1;
        }
        let mut used = if mine.is_empty() { 0.0 } else { mine.len().div_ceil(cols) as f32 * (th + gap) };
        if mine.is_empty() {
            let msg = if self.equipped_only {
                "Nothing on this page yet: pick a slot, then an emote.".to_owned()
            } else if needle.is_empty() {
                "No emotes in your locker yet.".to_owned()
            } else {
                format!("No emote matches \u{201C}{}\u{201D}.", self.search.value().trim())
            };
            let msg = c.ellipsize(font::REGULAR, 12.0, &msg, lw);
            c.text(font::REGULAR, 12.0, &msg, lx, cy + 8.0, theme::TEXT_MUTED);
            used = 36.0;
        }
        if !others.is_empty() {
            let oy = cy + used + 10.0;
            c.text(font::SEMIBOLD, 10.5, "TRY ONE ON", lx, oy, theme::TEXT_MUTED);
            let store = "More in the Native Store";
            let stw = c.text_width(font::MEDIUM, 11.0, store);
            c.text(font::MEDIUM, 11.0, store, lx + lw - stw, oy, theme::TEXT_MUTED);
            let oy = oy + 22.0;
            for (i, it) in others.iter().enumerate() {
                let ix = lx + (i % cols) as f32 * (tw + gap);
                let iy = oy + (i / cols) as f32 * (th + gap);
                if iy + th >= gy - 4.0 && iy <= gy + gh + 4.0 {
                    self.card(ui, c, it, &row, &pages, &skin, slim, ix, iy, tw, th);
                }
            }
            used += 32.0 + others.len().div_ceil(cols) as f32 * (th + gap);
        }
        self.scroll.end(ui, c, used);
        if let Some(err) = &save_error {
            let err = c.ellipsize(font::MEDIUM, 11.5, err, lw);
            c.text(font::MEDIUM, 11.5, &err, lx, y + h - 14.0, 0xFFFCA5A5);
        }

        self.drag(ui, c, &items, &pages, over_grid, &skin, slim);
        close
    }

    /// Left: the emote playing on your own skin, and what you can do with it. True when it was played in the world.
    #[allow(clippy::too_many_arguments)]
    fn stage(
        &mut self,
        ui: &mut Ui,
        c: &mut Canvas,
        items: &[Emote],
        row: &WheelPage,
        pages: &[WheelPage],
        usable: usize,
        in_world: bool,
        skin: &Arc<Image>,
        slim: bool,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) -> bool {
        let it = find(items, self.pick.as_deref()).cloned();
        c.round(x, y, w, h, 14.0, 0x66000000);
        glow(c, x, y, w, h * 0.6, 14.0, 0x10FFFFFF);
        c.outline(x, y, w, h, 14.0, 1.0, theme::HAIRLINE);
        c.text(font::SEMIBOLD, 10.5, "PREVIEW", x + 16.0, y + 16.0, theme::TEXT_MUTED);
        let title = it.as_ref().map_or_else(|| "Pick an emote".to_owned(), Emote::label);
        let title = c.ellipsize(font::SEMIBOLD, 15.0, &title, w - 32.0);
        c.text(font::SEMIBOLD, 15.0, &title, x + 16.0, y + 32.0, theme::TEXT_STRONG);
        let (px, py, pw, ph) = (x + 12.0, y + 64.0, w - 24.0, h - 64.0 - 132.0);
        match &it {
            None => c.icon(theme::I_SMILE, 34.0, x + w / 2.0, py + ph / 2.0, theme::TEXT_MUTED),
            Some(e) => {
                native_emotes::want_clip(&e.hash);
                let elapsed = Some((ui.now - self.pick_at).max(0.0));
                match native_emotes::preview_pose(&e.hash, elapsed) {
                    Some(pose) => {
                        c.push_clip(x + 1.0, py, w - 2.0, ph);
                        // draw_wheel takes the emote's props too (tools/native/native_emotes.py adds them)
                        let props = native_emotes::preview_props(&e.hash, elapsed);
                        preview3d::draw_wheel(c, skin, slim, pose, [px + pw * 0.1, py + ph * 0.04, pw * 0.8, ph * 0.92], props);
                        c.pop_clip();
                    }
                    None => ui.dots(c, x + w / 2.0, py + ph / 2.0, theme::TEXT_MUTED),
                }
            }
        }
        let at = it.as_ref().and_then(|e| index_of(row, &e.id));
        let sub = match &it {
            None => "Your emotes play here".to_owned(),
            Some(e) if e.owned => at.map_or_else(|| "In your locker".to_owned(), |i| format!("On slot {}", i + 1)),
            Some(e) if e.free => "Free emote".to_owned(),
            Some(e) if e.exclusive => "Exclusive".to_owned(),
            Some(_) => "In the Native Store".to_owned(),
        };
        let subw = c.text_width(font::REGULAR, 11.0, &sub);
        c.text(font::REGULAR, 11.0, &sub, x + (w - subw) / 2.0, y + h - 122.0, theme::TEXT_MUTED);
        c.fill(x + 16.0, y + h - 100.0, w - 32.0, 1.0, theme::HAIRLINE);
        let (bx, bw, bh) = (x + 16.0, w - 32.0, 34.0);
        // the big one: play it in the world for everyone, or say why not
        let ry = y + h - 88.0;
        let mut close = false;
        let playable = in_world && at.is_some() && self.page < usable;
        let hint = if !in_world { "Join a world to play emotes" } else if at.is_none() { "Put it on the wheel to play it" } else { "Play in game" };
        if playable {
            if ui.button(c, "emotes:play", bx, ry, bw, bh, hint, theme::I_PLAY, ui::BTN_PRIMARY)
                && let Some(slot) = at
            {
                close = native_emotes::play(self.page, slot);
            }
        } else if it.is_some() {
            c.round(bx, ry, bw, bh, 10.0, theme::SUBTLE);
            c.outline(bx, ry, bw, bh, 10.0, 1.0, theme::HAIRLINE);
            let tw = c.text_width(font::SEMIBOLD, 12.0, hint);
            let lh = c.line_height(font::SEMIBOLD, 12.0);
            c.text(font::SEMIBOLD, 12.0, hint, bx + (bw - tw) / 2.0, ry + (bh - lh) / 2.0, theme::TEXT_MUTED);
        }
        let Some(e) = it else { return close };
        let (by, hw) = (ry + bh + 8.0, (bw - 8.0) / 2.0);
        let hx = bx + hw + 8.0;
        if ui.button(c, "emotes:replay", bx, by, hw, bh, "Replay", theme::I_REFRESH, ui::BTN_GLASS) {
            self.pick_at = ui.now;
        }
        if native_emotes::saving() {
            c.round(hx, by, hw, bh, 10.0, theme::SUBTLE_HOVER);
            ui.dots(c, hx + hw / 2.0, by + bh / 2.0, theme::TEXT_STRONG);
        } else if let Some(i) = at {
            if ui.button(c, "emotes:remove", hx, by, hw, bh, "Unequip", theme::I_X, ui::BTN_GLASS) {
                native_emotes::save_loadout(native_emotes::equip(pages, self.page, i, None));
            }
        } else if e.owned {
            if ui.button(c, "emotes:equip", hx, by, hw, bh, &format!("To slot {}", self.slot + 1), theme::I_PLUS, ui::BTN_GLASS) {
                self.equip_into(&e, pages, self.slot, ui.now);
            }
        } else if e.free && ui.button(c, "emotes:claim", hx, by, hw, bh, "Add", theme::I_PLUS, ui::BTN_GLASS) {
            native_emotes::claim(&e.id, &e.label());
        }
        close
    }

    /// The eight slots of one wheel page, with the page switcher. Returns the bottom edge.
    #[allow(clippy::too_many_arguments)]
    fn wheel(
        &mut self,
        ui: &mut Ui,
        c: &mut Canvas,
        items: &[Emote],
        row: &WheelPage,
        pages: &[WheelPage],
        page_count: usize,
        limit: usize,
        usable: usize,
        is_plus: bool,
        skin: &Arc<Image>,
        slim: bool,
        x: f32,
        y: f32,
        w: f32,
    ) -> f32 {
        c.text(font::SEMIBOLD, 10.5, "YOUR WHEEL", x, y, theme::TEXT_MUTED);
        c.text(font::SEMIBOLD, 17.0, "Emote slots", x, y + 14.0, theme::TEXT_STRONG);
        // page switcher, right
        let mut px = x + w;
        if is_plus && pages.len() < limit {
            px -= 30.0;
            if ui.icon_button(c, "emotes:addpage", px, y + 6.0, 28.0, theme::I_PLUS, Some("Add wheel page")) && !native_emotes::saving() {
                native_emotes::save_loadout(native_emotes::with_new_page(pages, limit));
                self.page = pages.len();
                self.slot = 0;
            }
        }
        if page_count > 1 || is_plus {
            px -= 30.0;
            if ui.icon_button(c, "emotes:next", px, y + 6.0, 28.0, theme::I_CHEVRON_RIGHT, Some("Next wheel page")) && self.page + 1 < page_count {
                self.page += 1;
                self.pick = None;
            }
            let label = format!("Page {} / {}", self.page + 1, page_count);
            let lw = c.text_width(font::MEDIUM, 11.5, &label);
            px -= lw + 10.0;
            let lh = c.line_height(font::MEDIUM, 11.5);
            c.text(font::MEDIUM, 11.5, &label, px + 4.0, y + 20.0 - lh / 2.0, theme::TEXT_SECONDARY);
            px -= 30.0;
            if ui.icon_button(c, "emotes:prev", px, y + 6.0, 28.0, theme::I_CHEVRON_LEFT, Some("Previous wheel page")) && self.page > 0 {
                self.page -= 1;
                self.pick = None;
            }
        } else {
            let tip = "Native Plus adds more wheel pages";
            let tw = c.text_width(font::REGULAR, 11.0, tip);
            let lh = c.line_height(font::REGULAR, 11.0);
            c.text(font::REGULAR, 11.0, tip, x + w - tw, y + 20.0 - lh / 2.0, theme::TEXT_MUTED);
        }

        let (sy, gap) = (y + 44.0, 8.0);
        let s = ((w - gap * 7.0) / 8.0).min(92.0);
        let sx = x + (w - (s * 8.0 + gap * 7.0)) / 2.0;
        let locked = self.page >= usable;
        self.hovered_slot = None;
        for i in 0..8 {
            self.slot_tile(ui, c, items, row, pages, i, sx + i as f32 * (s + gap), sy, s, locked, skin, slim);
        }
        let bottom = sy + s + 20.0;
        let help = if locked { "This page needs Native Plus to play." } else { "Pick a slot, then an emote. Drag to move, drag off to remove." };
        let help = c.ellipsize(font::REGULAR, 11.0, help, w);
        c.text(font::REGULAR, 11.0, &help, x, bottom - 8.0, theme::TEXT_MUTED);
        c.fill(x, bottom + 10.0, w, 1.0, theme::HAIRLINE);
        bottom + 14.0
    }

    #[allow(clippy::too_many_arguments)]
    fn slot_tile(
        &mut self,
        ui: &mut Ui,
        c: &mut Canvas,
        items: &[Emote],
        row: &WheelPage,
        pages: &[WheelPage],
        i: usize,
        x: f32,
        y: f32,
        s: f32,
        locked: bool,
        skin: &Arc<Image>,
        slim: bool,
    ) {
        let id = format!("emotes:slot:{i}");
        let it = find(items, row[i].as_deref()).cloned();
        let over = ui.hover(c, x, y, s, s);
        if over {
            self.hovered_slot = Some(i);
        }
        let target = self.dragging && over;
        if ui.pressed && over && let Some(e) = &it {
            self.drag_id = Some(e.id.clone());
            self.drag_from = Some(i);
            self.drag_x = ui.mx;
            self.drag_y = ui.my;
        }
        if ui.clicked(c, &id, x, y, s, s) && !self.dragging {
            self.slot = i;
            if let Some(e) = &it {
                self.show(e, ui.now);
            }
        }
        let hv = ui.anim_on(&format!("{id}#h"), over || target, 14.0);
        let on = ui.anim_on(&format!("{id}#on"), self.slot == i, 16.0);
        c.round(x, y, s, s, 12.0, theme::mix(theme::mix(CARD, CARD_HOVER, hv), 0xFF16181D, on * 0.6));
        c.outline(
            x,
            y,
            s,
            s,
            12.0,
            if on > 0.5 || target { 1.5 } else { 1.0 },
            theme::mix(theme::mix(theme::HAIRLINE, theme::BORDER_HOVER, hv), 0xE6FFFFFF, on.max(if target { 1.0 } else { 0.0 })),
        );
        match &it {
            Some(e) if !(self.dragging && self.drag_from == Some(i)) => {
                let pad = s * 0.12;
                self.poster(ui, c, e, skin, slim, x + pad, y + pad * 0.6, s - pad * 2.0, s - pad * 2.0 - 8.0, false);
                let n = c.ellipsize(font::MEDIUM, 9.5, &e.label(), s - 10.0);
                let nw = c.text_width(font::MEDIUM, 9.5, &n);
                c.text(font::MEDIUM, 9.5, &n, x + (s - nw) / 2.0, y + s - 15.0, theme::TEXT_SECONDARY);
            }
            Some(_) => {}
            None => c.icon(theme::I_PLUS, 16.0, x + s / 2.0, y + s / 2.0 - 4.0, theme::alpha(theme::TEXT_MUTED, 0.6 + 0.4 * hv)),
        }
        c.text(font::SEMIBOLD, 9.5, &(i + 1).to_string(), x + 7.0, y + 5.0, if locked { theme::TEXT_MUTED } else { theme::TEXT_SECONDARY });
        let tip = format!("Remove from slot {}", i + 1);
        if it.is_some()
            && over
            && !self.dragging
            && ui.icon_button(c, &format!("{id}:clear"), x + s - 24.0, y + 3.0, 21.0, theme::I_X, Some(tip.as_str()))
            && !native_emotes::saving()
        {
            self.drag_id = None;
            native_emotes::save_loadout(native_emotes::equip(pages, self.page, i, None));
        }
    }

    /// An emote card: a click puts it on the chosen slot (or previews it when it is not yours).
    #[allow(clippy::too_many_arguments)]
    fn card(&mut self, ui: &mut Ui, c: &mut Canvas, it: &Emote, row: &WheelPage, pages: &[WheelPage], skin: &Arc<Image>, slim: bool, x: f32, y: f32, w: f32, h: f32) {
        let id = format!("emotes:card:{}", it.id);
        let over = ui.hover(c, x, y, w, h);
        if ui.pressed && over && it.owned {
            self.drag_id = Some(it.id.clone());
            self.drag_from = None;
            self.drag_x = ui.mx;
            self.drag_y = ui.my;
        }
        if ui.clicked(c, &id, x, y, w, h) && !self.dragging {
            self.show(it, ui.now);
            if it.owned && index_of(row, &it.id) != Some(self.slot) && !native_emotes::saving() {
                self.equip_into(it, pages, self.slot, ui.now);
            }
        }
        let at = index_of(row, &it.id);
        let sel = self.pick.as_deref() == Some(it.id.as_str());
        let hv = ui.anim_on(&format!("{id}#h"), over, 14.0);
        let sl = ui.anim_on(&format!("{id}#s"), sel, 16.0);
        c.round(x, y, w, h, 12.0, theme::mix(CARD, CARD_HOVER, hv.max(sl * 0.7)));
        c.outline(x, y, w, h, 12.0, if sel { 1.5 } else { 1.0 }, theme::mix(theme::mix(theme::HAIRLINE, theme::BORDER_HOVER, hv), 0xE6FFFFFF, sl));
        let meta = 28.0;
        c.round(x + 6.0, y + 6.0, w - 12.0, h - meta - 10.0, 8.0, 0x40000000);
        self.poster(ui, c, it, skin, slim, x + 10.0, y + 8.0, w - 20.0, h - meta - 14.0, over);
        if !it.owned {
            c.icon(if it.free { theme::I_PLUS } else { theme::I_LOCK }, 11.0, x + w - 15.0, y + 15.0, theme::TEXT_SECONDARY);
        } else if let Some(i) = at {
            c.circle(x + w - 15.0, y + 15.0, 9.0, 0xFFF4F4F5);
            let n = (i + 1).to_string();
            let nw = c.text_width(font::SEMIBOLD, 9.5, &n);
            let lh = c.line_height(font::SEMIBOLD, 9.5);
            c.text(font::SEMIBOLD, 9.5, &n, x + w - 15.0 - nw / 2.0, y + 15.0 - lh / 2.0, theme::SOLID_FG);
        }
        c.fill(x + 1.0, y + h - meta, w - 2.0, 1.0, theme::HAIRLINE);
        let tag = if it.owned { None } else if it.free { Some("Free") } else { Some("Store") };
        let tag_w = tag.map_or(0.0, |t| c.text_width(font::MEDIUM, 10.0, t) + 8.0);
        let n = c.ellipsize(font::SEMIBOLD, 11.0, &it.label(), w - 18.0 - tag_w);
        let my = y + h - meta / 2.0;
        let lh = c.line_height(font::SEMIBOLD, 11.0);
        c.text(font::SEMIBOLD, 11.0, &n, x + 9.0, my - lh / 2.0, if sel || at.is_some() { theme::TEXT_STRONG } else { theme::TEXT });
        if let Some(tag) = tag {
            let th = c.line_height(font::MEDIUM, 10.0);
            c.text(font::MEDIUM, 10.0, tag, x + w - 9.0 - tag_w + 8.0, my - th / 2.0, if it.free { theme::ACCENT } else { theme::TEXT_MUTED });
        }
    }

    /// The emote's still pose on your skin, or its poster while the clip downloads; it plays while hovered.
    #[allow(clippy::too_many_arguments)]
    fn poster(&self, ui: &Ui, c: &mut Canvas, it: &Emote, skin: &Arc<Image>, slim: bool, x: f32, y: f32, w: f32, h: f32, play: bool) {
        native_emotes::want_clip(&it.hash);
        let elapsed = play.then(|| (ui.now - self.shown_at).max(0.0));
        if let Some(pose) = native_emotes::preview_pose(&it.hash, elapsed) {
            preview3d::draw_wheel(c, skin, slim, pose, [x, y, w, h], native_emotes::preview_props(&it.hash, elapsed));
            return;
        }
        match poster_url(&it.raw, &net::api()).map(|u| net::image(&u, 512 * 1024)) {
            Some(Pic::Ready(img)) => {
                let k = (w / img.width.max(1) as f32).min(h / img.height.max(1) as f32);
                let (dw, dh) = (img.width as f32 * k, img.height as f32 * k);
                c.image(&img, x + (w - dw) / 2.0, y + (h - dh) / 2.0, dw, dh, [0.0, 0.0, 1.0, 1.0], 0xFFFFFFFF);
            }
            Some(Pic::Loading) => ui.dots(c, x + w / 2.0, y + h / 2.0, theme::TEXT_MUTED),
            _ => c.icon(theme::I_SMILE, (w * 0.4).min(22.0), x + w / 2.0, y + h / 2.0, theme::TEXT_MUTED),
        }
    }

    /// Drag and drop: onto a slot equips (from a card) or swaps (from a slot); a slot let go off the wheel empties.
    #[allow(clippy::too_many_arguments)]
    fn drag(&mut self, ui: &mut Ui, c: &mut Canvas, items: &[Emote], pages: &[WheelPage], over_grid: bool, skin: &Arc<Image>, slim: bool) {
        let Some(drag_id) = self.drag_id.clone() else { return };
        if !self.dragging && ui.down && (ui.mx - self.drag_x).abs() + (ui.my - self.drag_y).abs() > 6.0 {
            self.dragging = true;
        }
        let it = find(items, Some(&drag_id)).cloned();
        if self.dragging && let Some(e) = &it {
            let s = 64.0;
            c.round(ui.mx - s / 2.0, ui.my - s / 2.0, s, s, 12.0, 0xE00E1014);
            c.outline(ui.mx - s / 2.0, ui.my - s / 2.0, s, s, 12.0, 1.0, 0xE6FFFFFF);
            self.poster(ui, c, e, skin, slim, ui.mx - s / 2.0 + 6.0, ui.my - s / 2.0 + 4.0, s - 12.0, s - 10.0, false);
        }
        if ui.down {
            return;
        }
        // let go
        if self.dragging && let Some(e) = &it && !native_emotes::saving() {
            match (self.hovered_slot, self.drag_from) {
                (Some(to), Some(from)) if to != from => {
                    native_emotes::save_loadout(native_emotes::swap(pages, self.page, from, to));
                    self.slot = to;
                }
                (Some(to), None) => self.equip_into(e, pages, to, ui.now),
                (None, Some(from)) if over_grid => {
                    native_emotes::save_loadout(native_emotes::equip(pages, self.page, from, None));
                    self.slot = from;
                }
                _ => {}
            }
            self.show(e, ui.now);
        }
        self.drag_id = None;
        self.dragging = false;
    }

    fn equip_into(&mut self, it: &Emote, pages: &[WheelPage], target: usize, now: f64) {
        let next = native_emotes::equip(pages, self.page, target, Some(&it.id));
        let open = next.get(self.page).map_or(target, |row| next_open(row, target));
        native_emotes::save_loadout(next);
        self.show(it, now);
        self.slot = open;
    }

    #[allow(clippy::too_many_arguments)]
    fn chip(&mut self, ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, label: &str, on: bool) -> f32 {
        let (w, h) = (c.text_width(font::SEMIBOLD, 11.0, label) + 22.0, 24.0);
        let over = ui.hover(c, x, y, w, h);
        if ui.clicked(c, id, x, y, w, h) {
            self.equipped_only = id.ends_with("equipped");
            self.scroll.reset();
        }
        let a = ui.anim_on(&format!("{id}#on"), on, 16.0);
        let hv = ui.anim_on(&format!("{id}#h"), over, 14.0);
        c.round(x, y, w, h, h / 2.0, theme::mix(theme::mix(theme::SUBTLE, theme::SUBTLE_HOVER, hv), 0xFFF4F4F5, a));
        let fg = theme::mix(theme::mix(theme::TEXT_SECONDARY, theme::TEXT_STRONG, hv), theme::SOLID_FG, a);
        let lh = c.line_height(font::SEMIBOLD, 11.0);
        c.text(font::SEMIBOLD, 11.0, label, x + 11.0, y + (h - lh) / 2.0, fg);
        w
    }

    fn search_box(&mut self, ui: &mut Ui, c: &mut Canvas, x: f32, y: f32, w: f32, h: f32) {
        let f = self.search.focused(ui);
        let fa = ui.anim_on("emotes:search#f", f, 14.0);
        c.round(x, y, w, h, h / 2.0, theme::mix(0xB808090C, 0xE00E1014, fa));
        c.outline(x, y, w, h, h / 2.0, 1.0, theme::mix(theme::HAIRLINE_STRONG, theme::BORDER_HOVER, fa));
        c.icon(theme::I_SEARCH, 13.0, x + 16.0, y + h / 2.0, theme::TEXT_MUTED);
        let clear = !self.search.is_empty();
        if ui.pressed && ui.hover(c, x, y, w - if clear { 30.0 } else { 0.0 }, h) {
            self.search.focus(ui);
            ui.focus_claimed = true;
        }
        self.search.draw(ui, c, x + 18.0, y, w - if clear { 50.0 } else { 22.0 }, h, "Search emotes", 12.0);
        if clear && ui.icon_button(c, "emotes:search:clear", x + w - 28.0, y + (h - 22.0) / 2.0, 22.0, theme::I_X, None) {
            self.search.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filling_a_slot_moves_on_to_the_next_empty_one_round_the_wheel() {
        let mut row: WheelPage = Default::default();
        row[0] = Some("a".into());
        row[1] = Some("b".into());
        assert_eq!(next_open(&row, 0), 2);
        row[7] = Some("h".into());
        assert_eq!(next_open(&row, 6), 2);
        let full: WheelPage = std::array::from_fn(|i| Some(i.to_string()));
        assert_eq!(next_open(&full, 3), 3);
    }

    #[test]
    fn the_catalogue_reads_ownership_with_old_replies_counting_as_owned() {
        let me = serde_json::json!({"catalog": [
            {"id": "hi", "name": "say_hi", "hash": "h"},
            {"id": "groove", "name": "Groove", "hash": "g", "owned": false, "free": true},
            {"name": "no id"}
        ]});
        let items = catalog(&me);
        assert_eq!(items.len(), 2);
        assert!(items[0].owned && items[0].label() == "say hi");
        assert!(!items[1].owned && items[1].free);
    }
}
