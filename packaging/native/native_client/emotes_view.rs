//! The Emotes tab: the account's emotes (equipped cards and the store's), the 8 wheel slots per page and the 3D preview.
//! search + A-Z sort + 3-column square rounded emote cards with emerald `✓ Equipped` bottom strip and centered
//! names below, and right-hand 2×4 vertical wheel slots panel (Slots 1..8 per page, numbered top-left, remove
//! button on occupied slots, `◀ Page N ▶` footer, plus Play & Replay actions).

use super::emote_wheel::poster_url;
use super::locker::session_lock;
use super::menu::{clamp01, ease, empty};
use super::net::{self, Pic};
use super::preview3d;
use super::scroll::Scroll;
use super::textfield::TextField;
use super::ui::{self, Ui};
use horizon_client::native_emotes::{self, WheelPage};
use horizon_client::native_ui::{Canvas, Image, font, theme};
use serde_json::Value;
use std::sync::Arc;

/// Outcome of drawing the Emotes Locker view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmotesAct {
    None,
    /// Played an emote in the world: close the menu.
    Close,
}

/// One emote of the account's catalogue (`me.catalog`).
#[derive(Clone)]
struct Emote {
    id: String,
    name: String,
    hash: String,
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
    pick: Option<String>,
    pick_at: f64,
    sort_desc: bool,
    shown_at: f64,
    drag_id: Option<String>,
    drag_from: Option<usize>,
    drag_x: f32,
    drag_y: f32,
    dragging: bool,
    hovered_slot: Option<usize>,
    hover_card: Option<String>,
    hover_at: f64,
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
            sort_desc: false,
            shown_at: 0.0,
            drag_id: None,
            drag_from: None,
            drag_x: 0.0,
            drag_y: 0.0,
            dragging: false,
            hovered_slot: None,
            hover_card: None,
            hover_at: 0.0,
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

    /// Draws the unified Emotes Locker view.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&mut self, ui: &mut Ui, c: &mut Canvas, who: &str, x: f32, y: f32, w: f32, h: f32, in_world: bool) -> EmotesAct {
        // The Emotes tab owns the whole page (the menu's rail switches to Cosmetics).
        let (body_x, body_w) = (x, w.max(200.0));

        if !net::signed_in() {
            session_lock(ui, c, "emotes:lock", body_x, y, body_w, h);
            return EmotesAct::None;
        }
        let me = native_emotes::snapshot();
        if !me.is_object() {
            ui.dots(c, body_x + body_w / 2.0, y + h / 2.0, theme::TEXT_SECONDARY);
            return EmotesAct::None;
        }
        let items = catalog(&me);
        if items.is_empty() {
            empty(
                c,
                body_x,
                y,
                body_w,
                h.min(280.0),
                theme::I_SMILE,
                "No emotes yet",
                "Get emotes in the Native Store in the launcher, then put them on your wheel here.",
            );
            return EmotesAct::None;
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
        let is_plus = me["isPlus"].as_bool().unwrap_or(false);

        // 2. Right column (~244 px): 2x4 vertical wheel slots panel + Play/Replay + Page footer
        let right_w = (w * 0.27).clamp(218.0, 256.0);
        let rx = x + w - right_w;
        c.fill(rx - 10.0, y, 1.0, h, theme::HAIRLINE);
        let close = self.draw_wheel_panel(
            ui,
            c,
            &items,
            &row,
            &pages,
            page_count,
            limit,
            usable,
            is_plus,
            in_world,
            &skin,
            slim,
            rx,
            y,
            right_w,
            h,
        );

        // 3. Center column: Search + A-Z sort + 3-column square rounded emote cards
        let cx = body_x;
        let cw = (rx - 22.0 - cx).max(160.0);
        let bar_h = 32.0;
        let sort_w = 66.0;
        let search_w = (cw - sort_w - 8.0).max(80.0);
        self.search_box(ui, c, cx, y, search_w, bar_h);
        let sort_x = cx + search_w + 8.0;
        let sort_lbl = if self.sort_desc { "Z-A" } else { "A-Z" };
        let over_sort = ui.hover(c, sort_x, y, sort_w, bar_h);
        let shv = ui.anim_on("emotes:sort#h", over_sort, 14.0);
        c.round(sort_x, y, sort_w, bar_h, 8.0, theme::mix(0xFF14161E, 0xFF1C202C, shv));
        c.outline(sort_x, y, sort_w, bar_h, 8.0, 1.0, theme::mix(theme::HAIRLINE, theme::HAIRLINE_STRONG, shv));
        c.icon(theme::I_ARROWS_UP, 11.5, sort_x + 16.0, y + bar_h / 2.0, theme::TEXT_SECONDARY);
        let slh = c.line_height(font::SEMIBOLD, 11.0);
        c.text(font::SEMIBOLD, 11.0, sort_lbl, sort_x + 28.0, y + (bar_h - slh) / 2.0, theme::TEXT_STRONG);
        if over_sort {
            ui.cursor_hand = true;
            ui.tip("Toggle alphabetical sort");
        }
        if ui.clicked(c, "emotes:sort", sort_x, y, sort_w, bar_h) {
            self.sort_desc = !self.sort_desc;
        }

        let needle = self.search.value().trim().to_lowercase();
        let mut mine: Vec<&Emote> = Vec::new();
        let mut others: Vec<&Emote> = Vec::new();
        for it in &items {
            if !needle.is_empty() && !it.label().to_lowercase().contains(&needle) {
                continue;
            }
            if it.owned {
                mine.push(it);
            } else if others.len() < 9 {
                others.push(it);
            }
        }
        let cmp = |a: &&Emote, b: &&Emote| {
            let ord = a.label().to_lowercase().cmp(&b.label().to_lowercase());
            if self.sort_desc { ord.reverse() } else { ord }
        };
        mine.sort_by(cmp);
        others.sort_by(cmp);

        let save_error = native_emotes::save_error();
        let gy = y + bar_h + 12.0;
        let gh = y + h - gy - if save_error.is_some() { 22.0 } else { 0.0 };
        let over_grid = ui.hover(c, cx, gy, cw, gh);
        let gap = 12.0;
        let cols = 3usize;
        let card_w = (cw - gap * (cols - 1) as f32) / cols as f32;
        let sq_h = card_w;
        let label_h = 22.0;
        let cell_h = sq_h + label_h;
        let off = self.scroll.begin(ui, c, cx, gy, cw + 6.0, gh);
        let cy = gy - off;
        let mut n = 0usize;
        for (i, it) in mine.iter().enumerate() {
            let ix = cx + (i % cols) as f32 * (card_w + gap);
            let iy = cy + (i / cols) as f32 * (cell_h + gap);
            if iy + cell_h >= gy - 4.0 && iy <= gy + gh + 4.0 {
                let e = ease(clamp01(((ui.now - self.shown_at - 20.0 - n as f64 * 16.0) / 260.0) as f32));
                c.push_alpha(e);
                self.card(ui, c, it, &row, &pages, &skin, slim, ix, iy + (1.0 - e) * 8.0, card_w, sq_h, label_h);
                c.pop_alpha();
            }
            n += 1;
        }
        let mut used = if mine.is_empty() { 0.0 } else { mine.len().div_ceil(cols) as f32 * (cell_h + gap) };
        if mine.is_empty() {
            let msg = if needle.is_empty() {
                "No emotes in your locker yet.".to_owned()
            } else {
                format!("No emote matches \u{201C}{}\u{201D}.", self.search.value().trim())
            };
            let msg = c.ellipsize(font::REGULAR, 12.0, &msg, cw);
            c.text(font::REGULAR, 12.0, &msg, cx, cy + 8.0, theme::TEXT_MUTED);
            used = 36.0;
        }
        if !others.is_empty() {
            let oy = cy + used + 8.0;
            c.text(font::SEMIBOLD, 10.0, "TRY ONE ON", cx, oy, theme::TEXT_MUTED);
            let oy = oy + 20.0;
            for (i, it) in others.iter().enumerate() {
                let ix = cx + (i % cols) as f32 * (card_w + gap);
                let iy = oy + (i / cols) as f32 * (cell_h + gap);
                if iy + cell_h >= gy - 4.0 && iy <= gy + gh + 4.0 {
                    self.card(ui, c, it, &row, &pages, &skin, slim, ix, iy, card_w, sq_h, label_h);
                }
            }
            used += 28.0 + others.len().div_ceil(cols) as f32 * (cell_h + gap);
        }
        self.scroll.end(ui, c, used);
        if let Some(err) = &save_error {
            let err = c.ellipsize(font::MEDIUM, 11.0, err, cw);
            c.text(font::MEDIUM, 11.0, &err, cx, y + h - 14.0, 0xFFFCA5A5);
        }

        self.drag(ui, c, &items, &pages, over_grid, &skin, slim);
        if close { EmotesAct::Close } else { EmotesAct::None }
    }

    /// Right column: Play / Replay header + 2×4 vertical wheel slots (Slots 1..8 per page, numbered top-left,
    /// small remove button on occupied slots) + `◀ Page N ▶` footer.
    #[allow(clippy::too_many_arguments)]
    fn draw_wheel_panel(
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
        in_world: bool,
        skin: &Arc<Image>,
        slim: bool,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) -> bool {
        let mut close = false;
        let it = find(items, self.pick.as_deref()).cloned();
        let at = it.as_ref().and_then(|e| index_of(row, &e.id));

        // Top action bar: Play & Replay buttons (matching Native's button theme).
        let top_h = 32.0;
        let playable = in_world && at.is_some() && self.page < usable;
        let play_w = (w - 8.0) * 0.56;
        let replay_w = w - play_w - 8.0;
        if playable {
            if ui.button(c, "emotes:play", x, y, play_w, top_h, "Play", theme::I_PLAY, ui::BTN_PRIMARY)
                && let Some(slot) = at
            {
                close = native_emotes::play(self.page, slot);
            }
        } else {
            let lbl = if !in_world { "Join world" } else if at.is_none() { "Select slot" } else { "Play" };
            c.round(x, y, play_w, top_h, 8.0, theme::SUBTLE);
            c.outline(x, y, play_w, top_h, 8.0, 1.0, theme::HAIRLINE);
            let tw = c.text_width(font::SEMIBOLD, 11.0, lbl);
            let lh = c.line_height(font::SEMIBOLD, 11.0);
            c.text(font::SEMIBOLD, 11.0, lbl, x + (play_w - tw) / 2.0, y + (top_h - lh) / 2.0, theme::TEXT_MUTED);
        }
        if ui.button(c, "emotes:replay", x + play_w + 8.0, y, replay_w, top_h, "Replay", theme::I_REFRESH, ui::BTN_GLASS) {
            self.pick_at = ui.now;
        }

        // Main dark container for 2×4 wheel slots + bottom page footer
        let box_y = y + top_h + 10.0;
        let box_h = h - top_h - 10.0;
        c.round(x, box_y, w, box_h, 10.0, 0xFF101218);
        c.outline(x, box_y, w, box_h, 10.0, 1.0, theme::HAIRLINE);

        let footer_h = 34.0;
        let grid_area_h = box_h - footer_h - 16.0;
        let gap = 10.0;
        let slot_by_w = (w - 28.0 - gap) / 2.0;
        let slot_by_h = (grid_area_h - gap * 3.0) / 4.0;
        let s = slot_by_w.min(slot_by_h).clamp(48.0, 92.0);
        let gx = x + (w - (s * 2.0 + gap)) / 2.0;
        let gy = box_y + (grid_area_h + 12.0 - (s * 4.0 + gap * 3.0)) / 2.0;

        let locked = self.page >= usable;
        self.hovered_slot = None;
        for i in 0..8usize {
            let col = i % 2;
            let r = i / 2;
            self.slot_tile(
                ui,
                c,
                items,
                row,
                pages,
                i,
                gx + col as f32 * (s + gap),
                gy + r as f32 * (s + gap),
                s,
                locked,
                skin,
                slim,
            );
        }

        // Bottom footer: ◀  Page N  ▶ (+ Add page if Plus)
        let fy = box_y + box_h - footer_h;
        c.fill(x + 1.0, fy, w - 2.0, 1.0, theme::HAIRLINE);
        let page_lbl = format!("Page {}", self.page + 1);
        let plw = c.text_width(font::SEMIBOLD, 11.5, &page_lbl);
        let plh = c.line_height(font::SEMIBOLD, 11.5);
        c.text(font::SEMIBOLD, 11.5, &page_lbl, x + (w - plw) / 2.0, fy + (footer_h - plh) / 2.0, theme::TEXT_STRONG);

        let can_prev = self.page > 0;
        let can_next = self.page + 1 < page_count;
        let over_prev = can_prev && ui.hover(c, x + 8.0, fy + 3.0, 28.0, 28.0);
        let over_next = can_next && ui.hover(c, x + w - 36.0, fy + 3.0, 28.0, 28.0);
        c.icon(
            theme::I_CHEVRON_LEFT,
            13.0,
            x + 22.0,
            fy + footer_h / 2.0,
            if over_prev { theme::TEXT_STRONG } else if can_prev { theme::TEXT_SECONDARY } else { theme::alpha(theme::TEXT_MUTED, 0.4) },
        );
        if can_prev && ui.clicked(c, "emotes:prev", x + 8.0, fy + 3.0, 28.0, 28.0) {
            self.page -= 1;
            self.pick = None;
        }
        let next_x = if is_plus && pages.len() < limit { x + w - 62.0 } else { x + w - 36.0 };
        c.icon(
            theme::I_CHEVRON_RIGHT,
            13.0,
            next_x + 14.0,
            fy + footer_h / 2.0,
            if over_next { theme::TEXT_STRONG } else if can_next { theme::TEXT_SECONDARY } else { theme::alpha(theme::TEXT_MUTED, 0.4) },
        );
        if can_next && ui.clicked(c, "emotes:next", next_x, fy + 3.0, 28.0, 28.0) {
            self.page += 1;
            self.pick = None;
        }
        if is_plus
            && pages.len() < limit
            && ui.icon_button(c, "emotes:addpage", x + w - 32.0, fy + 4.0, 26.0, theme::I_PLUS, Some("Add wheel page"))
            && !native_emotes::saving()
        {
            native_emotes::save_loadout(native_emotes::with_new_page(pages, limit));
            self.page = pages.len();
            self.slot = 0;
        }
        close
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
        c.round(x, y, s, s, 8.0, theme::mix(0xFF191C26, 0xFF232736, hv.max(on * 0.7)));
        c.outline(
            x,
            y,
            s,
            s,
            8.0,
            if on > 0.5 || target { 1.5 } else { 1.0 },
            theme::mix(theme::mix(theme::HAIRLINE, theme::BORDER_HOVER, hv), 0xE6FFFFFF, on.max(if target { 1.0 } else { 0.0 })),
        );
        match &it {
            Some(e) if !(self.dragging && self.drag_from == Some(i)) => {
                let pad = s * 0.10;
                let playing = self.pick.as_deref() == Some(e.id.as_str());
                self.poster(ui, c, e, skin, slim, x + pad, y + pad, s - pad * 2.0, s - pad * 2.0, playing);
            }
            Some(_) => {}
            None => {}
        }
        // Slot number 1..8 in top-left corner
        c.text(
            font::SEMIBOLD,
            10.0,
            &(i + 1).to_string(),
            x + 6.0,
            y + 5.0,
            if locked { theme::TEXT_MUTED } else { theme::TEXT_SECONDARY },
        );
        // Small circular remove button in top-right corner on occupied slots
        if it.is_some() && !self.dragging {
            let (rx, ry, rs) = (x + s - 20.0, y + 4.0, 16.0);
            let over_rm = ui.hover(c, rx - 2.0, ry - 2.0, rs + 4.0, rs + 4.0);
            c.circle(rx + rs / 2.0, ry + rs / 2.0, rs / 2.0, if over_rm { 0xFFDC2626 } else { 0xCC222633 });
            c.icon(theme::I_TRASH, 9.0, rx + rs / 2.0, ry + rs / 2.0, 0xFFFFFFFF);
            if over_rm {
                ui.cursor_hand = true;
                ui.tip(&format!("Remove from slot {}", i + 1));
            }
            if ui.clicked(c, &format!("{id}:clear"), rx - 2.0, ry - 2.0, rs + 4.0, rs + 4.0) && !native_emotes::saving() {
                self.drag_id = None;
                native_emotes::save_loadout(native_emotes::equip(pages, self.page, i, None));
            }
        }
        if over {
            ui.cursor_hand = true;
        }
    }

    /// Square rounded emote card (`radius = 10px`) with crisp green `✓ Equipped` bottom strip and centered name below.
    #[allow(clippy::too_many_arguments)]
    fn card(
        &mut self,
        ui: &mut Ui,
        c: &mut Canvas,
        it: &Emote,
        row: &WheelPage,
        pages: &[WheelPage],
        skin: &Arc<Image>,
        slim: bool,
        x: f32,
        y: f32,
        w: f32,
        sq_h: f32,
        label_h: f32,
    ) {
        let id = format!("emotes:card:{}", it.id);
        let over = ui.hover(c, x, y, w, sq_h + label_h);
        if ui.pressed && over && it.owned {
            self.drag_id = Some(it.id.clone());
            self.drag_from = None;
            self.drag_x = ui.mx;
            self.drag_y = ui.my;
        }
        if ui.clicked(c, &id, x, y, w, sq_h + label_h) && !self.dragging {
            self.show(it, ui.now);
            if it.owned && index_of(row, &it.id) != Some(self.slot) && !native_emotes::saving() {
                self.equip_into(it, pages, self.slot, ui.now);
            } else if !it.owned && it.free {
                native_emotes::claim(&it.id, &it.label());
            }
        }
        let at = index_of(row, &it.id);
        let equipped = at.is_some();
        if over && self.hover_card.as_deref() != Some(it.id.as_str()) {
            self.hover_card = Some(it.id.clone());
            self.hover_at = ui.now;
        } else if !over && self.hover_card.as_deref() == Some(it.id.as_str()) {
            self.hover_card = None;
        }
        let sel = self.pick.as_deref() == Some(it.id.as_str());
        let hv = ui.anim_on(&format!("{id}#h"), over, 14.0);
        let sl = ui.anim_on(&format!("{id}#s"), sel, 16.0);
        let base_bg = if equipped { 0xFF142119 } else { 0xFF181A22 };
        let hover_bg = if equipped { 0xFF1A2D21 } else { 0xFF212430 };
        c.round(x, y, w, sq_h, 10.0, theme::mix(base_bg, hover_bg, hv.max(sl * 0.6)));
        let border = if sel {
            0xE6FFFFFF
        } else if equipped {
            0xAA22C55E
        } else {
            theme::mix(theme::HAIRLINE, theme::BORDER_HOVER, hv)
        };
        c.outline(x, y, w, sq_h, 10.0, if sel || equipped { 1.4 } else { 1.0 }, border);

        let strip_h = if equipped { 22.0 } else { 0.0 };
        self.poster(ui, c, it, skin, slim, x + 8.0, y + 8.0, w - 16.0, sq_h - 14.0 - strip_h, over);

        if !it.owned {
            c.icon(if it.free { theme::I_PLUS } else { theme::I_LOCK }, 11.0, x + w - 14.0, y + 14.0, theme::TEXT_SECONDARY);
        }

        // Crisp emerald green `✓ Equipped` bottom strip inside the square card.
        if equipped {
            let sy = y + sq_h - strip_h;
            c.push_clip(x, sy, w, strip_h);
            c.round(x, sy - 8.0, w, strip_h + 8.0, 10.0, 0xFF15803D);
            c.pop_clip();
            let txt = "Equipped";
            let tw = c.text_width(font::SEMIBOLD, 10.0, txt);
            let tlh = c.line_height(font::SEMIBOLD, 10.0);
            let tx = x + (w - (tw + 15.0)) / 2.0;
            c.circle(tx + 5.0, sy + strip_h / 2.0, 5.0, 0xFFFFFFFF);
            c.icon(theme::I_CHECK, 7.5, tx + 5.0, sy + strip_h / 2.0, 0xFF15803D);
            c.text(font::SEMIBOLD, 10.0, txt, tx + 14.0, sy + (strip_h - tlh) / 2.0, 0xFFFFFFFF);
        }

        // Centered emote name below card
        let n = c.ellipsize(font::SEMIBOLD, 11.0, &it.label(), w - 4.0);
        let nw = c.text_width(font::SEMIBOLD, 11.0, &n);
        c.text(font::SEMIBOLD, 11.0, &n, x + (w - nw) / 2.0, y + sq_h + 4.0, if sel || equipped { theme::TEXT_STRONG } else { theme::TEXT });
        if over {
            ui.cursor_hand = true;
        }
    }

    /// The emote's still pose on your skin, or its poster while the clip downloads; it plays while hovered.
    #[allow(clippy::too_many_arguments)]
    fn poster(&self, ui: &Ui, c: &mut Canvas, it: &Emote, skin: &Arc<Image>, slim: bool, x: f32, y: f32, w: f32, h: f32, play: bool) {
        native_emotes::want_clip(&it.hash);
        let elapsed = play.then(|| {
            if self.hover_card.as_deref() == Some(it.id.as_str()) {
                native_emotes::preview_start(&it.hash) + (ui.now - self.hover_at).max(0.0)
            } else {
                (ui.now - self.pick_at).max(0.0)
            }
        });
        if let Some(pose) = native_emotes::preview_pose(&it.hash, elapsed) {
            preview3d::fit_emote(&it.hash, slim);
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
            c.round(ui.mx - s / 2.0, ui.my - s / 2.0, s, s, 10.0, 0xE00E1014);
            c.outline(ui.mx - s / 2.0, ui.my - s / 2.0, s, s, 10.0, 1.0, 0xE6FFFFFF);
            self.poster(ui, c, e, skin, slim, ui.mx - s / 2.0 + 6.0, ui.my - s / 2.0 + 4.0, s - 12.0, s - 10.0, false);
        }
        if ui.down {
            return;
        }
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

    fn search_box(&mut self, ui: &mut Ui, c: &mut Canvas, x: f32, y: f32, w: f32, h: f32) {
        let f = self.search.focused(ui);
        let fa = ui.anim_on("emotes:search#f", f, 14.0);
        c.round(x, y, w, h, 8.0, theme::mix(0xFF14161E, 0xFF1A1E29, fa));
        c.outline(x, y, w, h, 8.0, 1.0, theme::mix(theme::HAIRLINE, theme::BORDER_HOVER, fa));
        c.icon(theme::I_SEARCH, 13.0, x + 15.0, y + h / 2.0, theme::TEXT_MUTED);
        let clear = !self.search.is_empty();
        if ui.pressed && ui.hover(c, x, y, w - if clear { 28.0 } else { 0.0 }, h) {
            self.search.focus(ui);
            ui.focus_claimed = true;
        }
        self.search.draw(ui, c, x + 18.0, y, w - if clear { 48.0 } else { 22.0 }, h, "Search emotes...", 12.0);
        if clear && ui.icon_button(c, "emotes:search:clear", x + w - 26.0, y + (h - 22.0) / 2.0, 22.0, theme::I_X, None) {
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
