//! Full in-game remake of the Native Launcher Locker:
//! 3-column layout with Your cosmetics / Your emotes on the left (vertical category rail with coloured dots +
//! owned counts, side picker, owned items + "More in the Native Store" try-on, or embedded Emotes wheel),
//! the 3D Stage in the centre (avatar + name-only floating header, dye swatches, try-on bar, zoom/reset/pause
//! controls), and Saved skins on the right (purple active card + 3D upper-body skin portraits).
#![allow(dead_code)]

use super::emotes_view::{EmotesAct, EmotesView};
use super::menu::{avatar, clamp01, ease, empty};
use super::net::{self, Pic};
use super::preview3d::{self, PlayerPreview};
use super::scroll::Scroll;
use super::textfield::TextField;
use super::ui::{self, Ui};
use super::wardrobe::{self, Item, SavedSkin, SLOTS, State};
use horizon_client::native_ui::{Canvas, Image, font, theme};
use std::sync::Arc;

const SURFACE_CARD: u32 = 0xCC090B10;
const TILE_BG: u32 = 0xB80E1016;
const TILE_HOVER: u32 = 0xE0161922;
const EQUIP_GREEN: u32 = 0xFF16A34A;
const SKIN_PURPLE_TOP: u32 = 0xFF8B6EFF;
const SKIN_PURPLE_BOT: u32 = 0xFF6242E8;
const BALLOON_ICON: u32 = 0;

const SLOT_LABELS: [&str; 7] = ["Cloaks", "Hats", "Backs", "Shoes", "Hand", "Auras", "Balloons"];
const SLOT_COLORS: [u32; 7] = [0xFF8B5CF6, 0xFFF97316, 0xFF0EA5E9, 0xFFEAB308, 0xFFA855F7, 0xFFF59E0B, 0xFFEC4899];
const SLOT_ICONS: [u32; 7] = [theme::I_LAYERS, theme::I_HAT, theme::I_BACKPACK, theme::I_FOOTPRINTS, theme::I_HAND, theme::I_SPARKLES, BALLOON_ICON];

/// Outcome of drawing the Locker view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockerAct {
    None,
    /// Played an emote in the world: close the menu.
    Close,
}

fn slot_icon(c: &mut Canvas, icon: u32, size: f32, cx: f32, cy: f32, color: u32) {
    if icon != BALLOON_ICON {
        c.icon(icon, size, cx, cy, color);
        return;
    }
    let r = size * 0.3;
    let by = cy - size * 0.16;
    c.circle(cx, by, r, color);
    c.fill(cx - size * 0.06, by + r - 0.5, size * 0.12, size * 0.08, color);
    let (sy, step) = (by + r + size * 0.06, size * 0.11);
    for i in 0..3 {
        let dx = if i % 2 == 0 { 0.0 } else { size * 0.06 };
        c.fill(cx + dx - 0.5, sy + i as f32 * step, 1.0, step, theme::alpha(color, 0.75));
    }
}

fn parse_hex(hex: &str) -> u32 {
    let h = hex.trim_start_matches('#');
    u32::from_str_radix(h, 16).map_or(0xFF8B5CF6, |v| 0xFF000000 | v)
}

pub struct Locker {
    /// `false` = Cosmetics, `true` = Emotes
    emotes_mode: bool,
    slot: usize,
    try_on: Option<Item>,
    preview_skin_id: Option<String>,
    item_search: TextField,
    skin_search: TextField,
    item_scroll: Scroll,
    skin_scroll: Scroll,
    preview: PlayerPreview,
    emotes: EmotesView,
    slot_at: f64,
    emotes_need_open: bool,
}

impl Default for Locker {
    fn default() -> Self {
        Self::new()
    }
}

impl Locker {
    pub fn new() -> Self {
        let mut item_search = TextField::new("locker:item:search");
        item_search.max_length = 48;
        item_search.bare = true;
        let mut skin_search = TextField::new("locker:skin:search");
        skin_search.max_length = 48;
        skin_search.bare = true;
        Locker {
            emotes_mode: false,
            slot: 0,
            try_on: None,
            preview_skin_id: None,
            item_search,
            skin_search,
            item_scroll: Scroll::new("locker:items"),
            skin_scroll: Scroll::new("locker:skins"),
            preview: PlayerPreview::new(),
            emotes: EmotesView::new(),
            slot_at: 0.0,
            emotes_need_open: false,
        }
    }

    pub fn opened(&mut self) {
        wardrobe::refresh(false);
        if self.emotes_mode {
            self.emotes_need_open = true;
        }
    }

    pub fn open_emotes(&mut self, now: f64) {
        self.emotes_mode = true;
        self.try_on = None;
        self.emotes_need_open = true;
        self.emotes.opened(now);
        wardrobe::refresh(false);
    }

    pub fn escape(&mut self, ui: &mut Ui) -> bool {
        if self.try_on.is_some() {
            self.try_on = None;
            return true;
        }
        if self.emotes_mode && self.emotes.escape(ui) {
            return true;
        }
        if self.item_search.focused(ui) {
            ui.focus = None;
            if !self.item_search.is_empty() {
                self.item_search.clear();
            }
            return true;
        }
        if self.skin_search.focused(ui) {
            ui.focus = None;
            if !self.skin_search.is_empty() {
                self.skin_search.clear();
            }
            return true;
        }
        false
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(&mut self, ui: &mut Ui, c: &mut Canvas, who: &str, page_at: f64, x: f32, y: f32, w: f32, h: f32, in_world: bool) -> LockerAct {
        wardrobe::sync_session();
        if !net::signed_in() {
            self.try_on = None;
            self.preview_skin_id = None;
            card_shell(c, x, y, w, h);
            session_lock(ui, c, "locker:lock", x + 16.0, y + 16.0, w - 32.0, h - 32.0);
            return LockerAct::None;
        }
        let l = wardrobe::snapshot();
        if !who.is_empty() && l.saved_skins.is_empty() && !l.skins_loading {
            wardrobe::refresh_skins(who, false);
        }
        if self.emotes_mode && self.emotes_need_open {
            self.emotes_need_open = false;
            self.emotes.opened(ui.now);
        }

        let gap = 12.0;
        if self.emotes_mode {
            let stage_w = (w * 0.29).clamp(220.0, 286.0);
            let left_w = (w - stage_w - gap).max(200.0);
            let act = self.draw_left_card(ui, c, &l, who, x, y, left_w, h, page_at, in_world);
            self.draw_stage_card(ui, c, &l, who, x + left_w + gap, y, stage_w, h);
            act
        } else {
            let left_w = (w * 0.44).clamp(330.0, 460.0);
            let right_w = (w * 0.265).clamp(200.0, 270.0);
            let stage_w = (w - left_w - right_w - gap * 2.0).max(160.0);
            let act = self.draw_left_card(ui, c, &l, who, x, y, left_w, h, page_at, in_world);
            self.draw_stage_card(ui, c, &l, who, x + left_w + gap, y, stage_w, h);
            self.draw_skins_card(ui, c, &l, who, x + left_w + gap + stage_w + gap, y, right_w, h);
            act
        }
    }

    // ── Left Column: Your cosmetics / Your emotes ────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn draw_left_card(&mut self, ui: &mut Ui, c: &mut Canvas, l: &wardrobe::Locker, who: &str, x: f32, y: f32, w: f32, h: f32, page_at: f64, in_world: bool) -> LockerAct {
        card_shell(c, x, y, w, h);

        let hh = 46.0;
        let title = if self.emotes_mode { "Your emotes" } else { "Your cosmetics" };
        let tlh = c.line_height(font::SEMIBOLD, 14.0);
        c.text(font::SEMIBOLD, 14.0, title, x + 14.0, y + (hh - tlh) / 2.0, theme::TEXT_STRONG);

        // Segmented toggle: Cosmetics | Emotes
        let owned_count = wardrobe::owned_total(l);
        let cos_lbl = if owned_count > 0 { format!("Cosmetics {owned_count}") } else { "Cosmetics".to_owned() };
        let emo_lbl = "Emotes";
        let cos_w = c.text_width(font::SEMIBOLD, 11.0, &cos_lbl) + 22.0;
        let emo_w = c.text_width(font::SEMIBOLD, 11.0, emo_lbl) + 22.0;
        let seg_w = cos_w + emo_w + 6.0;
        let seg_h = 28.0;
        let seg_x = x + w - 12.0 - seg_w;
        let seg_y = y + (hh - seg_h) / 2.0;
        c.round(seg_x, seg_y, seg_w, seg_h, 9.0, 0xCC050609);
        c.outline(seg_x, seg_y, seg_w, seg_h, 9.0, 1.0, theme::HAIRLINE);

        if seg_button(ui, c, "locker:seg:cos", seg_x + 3.0, seg_y + 3.0, cos_w, seg_h - 6.0, &cos_lbl, !self.emotes_mode) && self.emotes_mode {
            self.emotes_mode = false;
            self.try_on = None;
        }
        if seg_button(ui, c, "locker:seg:emo", seg_x + 3.0 + cos_w, seg_y + 3.0, emo_w, seg_h - 6.0, emo_lbl, self.emotes_mode) && !self.emotes_mode {
            self.emotes_mode = true;
            self.try_on = None;
            self.emotes_need_open = true;
        }

        c.fill(x + 1.0, y + hh, w - 2.0, 1.0, theme::HAIRLINE);

        let (bx, by, bw, bh) = (x + 12.0, y + hh + 10.0, w - 24.0, h - hh - 20.0);
        if self.emotes_mode {
            return if matches!(self.emotes.draw(ui, c, who, bx, by, bw, bh, in_world), EmotesAct::Close) {
                LockerAct::Close
            } else {
                LockerAct::None
            };
        }

        if !net::signed_in() {
            session_lock(ui, c, "locker:lock", bx, by, bw, bh);
            return LockerAct::None;
        }
        if l.state == State::NoAccount {
            wardrobe::refresh(false);
        }
        if matches!(l.state, State::Idle | State::Loading | State::NoAccount) {
            ui.dots(c, bx + bw / 2.0, by + bh / 2.0, theme::TEXT_SECONDARY);
            if l.state == State::Idle {
                wardrobe::refresh(false);
            }
            return LockerAct::None;
        }
        if l.state == State::Error {
            empty(c, bx, by, bw, bh - 44.0, theme::I_WIFI_OFF, "Could not load your locker", "Check your connection and try again.");
            if ui.button(c, "locker:retry", bx + bw / 2.0 - 58.0, by + bh / 2.0 + 36.0, 116.0, 34.0, "Retry", theme::I_REFRESH, ui::BTN_GLASS) {
                wardrobe::refresh(true);
            }
            return LockerAct::None;
        }

        let rail_w = (bw * 0.29).clamp(106.0, 130.0);
        self.draw_category_rail(ui, c, l, bx, by, rail_w, bh);
        c.fill(bx + rail_w + 8.0, by, 1.0, bh, theme::HAIRLINE);
        self.draw_cosmetics_grid(ui, c, l, bx + rail_w + 16.0, by, bw - rail_w - 16.0, bh, page_at);
        LockerAct::None
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_category_rail(&mut self, ui: &mut Ui, c: &mut Canvas, l: &wardrobe::Locker, x: f32, y: f32, w: f32, h: f32) {
        let mut ry = y;
        let gap = 5.0;
        let rh = ((h - gap * (SLOTS.len() - 1) as f32) / SLOTS.len() as f32).min(36.0);
        let lh = c.line_height(font::SEMIBOLD, 11.5);
        let clh = c.line_height(font::SEMIBOLD, 9.5);
        for i in 0..SLOTS.len() {
            let count = wardrobe::owned_in(l, SLOTS[i]).len();
            let id = format!("locker:cat:{i}");
            let active = self.slot == i;
            let over = ui.hover(c, x, ry, w, rh);
            if over {
                ui.cursor_hand = true;
            }
            if ui.clicked(c, &id, x, ry, w, rh) && self.slot != i {
                self.slot = i;
                self.try_on = None;
                self.item_scroll.reset();
                self.slot_at = ui.now;
                if i == 0 || i == 2 {
                    self.preview.face_back();
                } else {
                    self.preview.face_front();
                }
            }
            let on = ui.anim_on(&format!("{id}#on"), active, 16.0);
            let hv = ui.anim_on(&format!("{id}#h"), over, 14.0);
            let accent = SLOT_COLORS[i];
            let bg = theme::mix(theme::alpha(theme::SUBTLE_HOVER, hv * 0.72), theme::alpha(accent, 0.16), on);
            c.round(x, ry, w, rh, 10.0, bg);
            let edge = theme::mix(theme::alpha(theme::HAIRLINE, hv), theme::mix(theme::HAIRLINE_STRONG, theme::alpha(accent, 0.55), 0.65), on);
            if hv > 0.01 || on > 0.01 {
                c.outline(x, ry, w, rh, 10.0, 1.0, edge);
            }
            if on > 0.02 {
                c.round(x, ry + 7.0, 3.0, (rh - 14.0).max(8.0), 1.5, theme::alpha(accent, on));
            }
            // Crisp category icon badge (no dot)
            let icon_s = (rh - 10.0).min(22.0);
            let icon_x = x + 8.0;
            let icon_y = ry + (rh - icon_s) / 2.0;
            c.round(icon_x, icon_y, icon_s, icon_s, 6.5, theme::mix(theme::mix(0x14FFFFFF, 0x22FFFFFF, hv), theme::alpha(accent, 0.24), on));
            let icon_fg = theme::mix(theme::mix(theme::TEXT_MUTED, theme::TEXT_STRONG, hv), accent, on);
            slot_icon(c, SLOT_ICONS[i], (icon_s * 0.58).min(12.5), icon_x + icon_s / 2.0, icon_y + icon_s / 2.0, icon_fg);

            let fg = theme::mix(theme::mix(theme::TEXT_SECONDARY, theme::TEXT_STRONG, hv), theme::TEXT_STRONG, on);
            let cnt = count.to_string();
            let badge_w = if count > 0 { (c.text_width(font::SEMIBOLD, 9.5, &cnt) + 10.0).max(18.0) } else { 0.0 };
            let text_x = icon_x + icon_s + 8.0;
            let lbl_max = (w - (text_x - x) - if count > 0 { badge_w + 10.0 } else { 8.0 }).max(20.0);
            let lbl = c.ellipsize(font::SEMIBOLD, 11.5, SLOT_LABELS[i], lbl_max);
            c.text(font::SEMIBOLD, 11.5, &lbl, text_x, ry + (rh - lh) / 2.0, fg);
            if count > 0 {
                let bh = 16.0;
                let bx = x + w - 7.0 - badge_w;
                let by = ry + (rh - bh) / 2.0;
                c.round(bx, by, badge_w, bh, 8.0, theme::mix(theme::SUBTLE, accent, on));
                let cw = c.text_width(font::SEMIBOLD, 9.5, &cnt);
                c.text(font::SEMIBOLD, 9.5, &cnt, bx + (badge_w - cw) / 2.0, by + (bh - clh) / 2.0, theme::mix(theme::TEXT_SECONDARY, 0xFFFFFFFF, on));
            }
            ry += rh + gap;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_cosmetics_grid(&mut self, ui: &mut Ui, c: &mut Canvas, l: &wardrobe::Locker, x: f32, y: f32, w: f32, h: f32, page_at: f64) {
        let slot_key = SLOTS[self.slot];
        let worn = if self.slot == 0 { wardrobe::item(l, l.equipped.as_deref()) } else { wardrobe::item(l, l.wearing.get(slot_key).map(String::as_str)) };

        // Top bar: Search pill + optional Left/Right side toggle for hand/balloons
        let top_h = 28.0;
        let sided = matches!(slot_key, "hand" | "balloons") && worn.is_some();
        let side_w = if sided { 92.0 } else { 0.0 };
        let srch_w = if sided { (w - side_w - 8.0).max(80.0) } else { w };
        let ph = format!("Search {}...", SLOT_LABELS[self.slot].to_lowercase());
        search_box(ui, c, &mut self.item_search, "locker:item:search", x, y, srch_w, top_h, &ph);
        if sided {
            let cur_side = l.sides.get(slot_key).copied().unwrap_or(2);
            let sx = x + w - side_w;
            c.round(sx, y, side_w, top_h, 8.0, 0xCC050609);
            c.outline(sx, y, side_w, top_h, 8.0, 1.0, theme::HAIRLINE);
            let half = (side_w - 6.0) / 2.0;
            if seg_button(ui, c, "locker:side:l", sx + 3.0, y + 3.0, half, top_h - 6.0, "Left", cur_side == 1) {
                wardrobe::set_side(slot_key, 1);
            }
            if seg_button(ui, c, "locker:side:r", sx + 3.0 + half, y + 3.0, half, top_h - 6.0, "Right", cur_side != 1) {
                wardrobe::set_side(slot_key, 2);
            }
        }

        let needle = self.item_search.value().trim().to_lowercase();
        let mut owned: Vec<Item> = wardrobe::owned_in(l, slot_key)
            .into_iter()
            .filter(|it| needle.is_empty() || it.display().to_lowercase().contains(&needle))
            .collect();
        owned.sort_by_key(|it| it.display().to_lowercase());
        let store_more: Vec<Item> = wardrobe::not_owned_in(l, slot_key)
            .into_iter()
            .filter(|it| needle.is_empty() || it.display().to_lowercase().contains(&needle))
            .collect();

        let gy = y + top_h + 10.0;
        let gh = h - top_h - 10.0 - if l.error.is_some() { 22.0 } else { 0.0 };
        let gap = 9.0;
        let cols = if w >= 330.0 { 3usize } else { 2usize };
        let tw = (w - gap * (cols - 1) as f32) / cols as f32;
        let th = tw * 0.88 + 30.0;

        let off = self.item_scroll.begin(ui, c, x, gy, w + 4.0, gh);
        let cy = gy - off;

        let owned_n = owned.len() + 1;
        for i in 0..owned_n {
            let ix = x + (i % cols) as f32 * (tw + gap);
            let iy = cy + (i / cols) as f32 * (th + gap);
            if iy + th >= gy - 4.0 && iy <= gy + gh + 4.0 {
                let e = ease(clamp01(((ui.now - page_at.max(self.slot_at) - 25.0 - (i.min(10)) as f64 * 16.0) / 240.0) as f32));
                c.push_alpha(e);
                let item = if i == 0 { None } else { Some(&owned[i - 1]) };
                self.cosmetic_tile(ui, c, l, item, true, worn.as_ref(), ix, iy + (1.0 - e) * 6.0, tw, th);
                c.pop_alpha();
            }
        }
        let mut used = owned_n.div_ceil(cols) as f32 * (th + gap);

        if !store_more.is_empty() {
            let sy = cy + used + 8.0;
            c.fill(x, sy - 4.0, w, 1.0, theme::HAIRLINE);
            c.icon(theme::I_STORE, 11.0, x + 7.0, sy + 11.0, theme::TEXT_MUTED);
            c.text(font::SEMIBOLD, 10.0, "MORE IN THE NATIVE STORE", x + 18.0, sy + 5.0, theme::TEXT_MUTED);
            let sy = sy + 24.0;
            for (i, item) in store_more.iter().enumerate() {
                let ix = x + (i % cols) as f32 * (tw + gap);
                let iy = sy + (i / cols) as f32 * (th + gap);
                if iy + th >= gy - 4.0 && iy <= gy + gh + 4.0 {
                    self.cosmetic_tile(ui, c, l, Some(item), false, worn.as_ref(), ix, iy, tw, th);
                }
            }
            used += 32.0 + store_more.len().div_ceil(cols) as f32 * (th + gap);
        }
        self.item_scroll.end(ui, c, (used - gap).max(0.0));

        if let Some(err) = &l.error {
            let err = c.ellipsize(font::MEDIUM, 11.0, err, w);
            c.text(font::MEDIUM, 11.0, &err, x, y + h - 14.0, 0xFFFCA5A5);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn cosmetic_tile(&mut self, ui: &mut Ui, c: &mut Canvas, l: &wardrobe::Locker, it: Option<&Item>, is_owned: bool, worn: Option<&Item>, x: f32, y: f32, w: f32, h: f32) {
        let key = it.map_or("none", |i| i.id.as_str());
        let id = format!("locker:item:{}:{}:{key}", self.slot, if is_owned { "o" } else { "s" });
        let over = ui.hover(c, x, y, w, h);
        if ui.clicked(c, &id, x, y, w, h) {
            match it {
                None => {
                    self.try_on = None;
                    if worn.is_some() {
                        wardrobe::unequip_slot(SLOTS[self.slot]);
                    }
                }
                Some(item) if is_owned => {
                    self.try_on = None;
                    wardrobe::toggle(item);
                }
                Some(item) => {
                    if self.try_on.as_ref().is_some_and(|t| t.id == item.id) {
                        self.try_on = None;
                    } else {
                        self.try_on = Some(item.clone());
                        if item.is_cape() || item.group() == "back" {
                            self.preview.face_back();
                        } else {
                            self.preview.face_front();
                        }
                    }
                }
            }
        }

        let is_worn = is_owned && it.map_or(worn.is_none(), |i| wardrobe::is_worn(l, i));
        let is_trying = it.is_some_and(|i| self.try_on.as_ref().is_some_and(|t| t.id == i.id));
        let hv = ui.anim_on(&format!("{id}#h"), over, 14.0);
        let eq = ui.anim_on(&format!("{id}#eq"), is_worn, 14.0);
        let tr = ui.anim_on(&format!("{id}#tr"), is_trying, 14.0);

        let mut base = theme::mix(TILE_BG, TILE_HOVER, hv);
        if is_worn {
            base = theme::mix(base, theme::mix(0xFF122619, 0xFF183422, hv), eq);
        } else if is_trying {
            base = theme::mix(base, 0xFF1E1936, tr);
        }
        c.round(x, y, w, h, 10.0, base);
        let mut edge = theme::mix(theme::HAIRLINE, theme::BORDER_HOVER, hv);
        if is_worn {
            edge = theme::mix(edge, theme::alpha(EQUIP_GREEN, 0.92), eq);
        } else if is_trying {
            edge = theme::mix(edge, 0xFF8B5CF6, tr);
        }
        c.outline(x, y, w, h, 10.0, if is_worn || is_trying { 1.4 } else { 1.0 }, edge);

        let meta = 28.0;
        let (ix, iy, iw, ih) = (x + 6.0, y + 6.0, w - 12.0, h - meta - 8.0);
        c.round(ix, iy, iw, ih, 7.0, 0x40000000);

        match it {
            None => c.icon(theme::I_BAN, 22.0, ix + iw / 2.0, iy + ih / 2.0, theme::mix(theme::TEXT_MUTED, theme::TEXT_SECONDARY, hv)),
            Some(item) if item.is_cape() => {
                let still = item.still_url.as_deref().map(|u| net::image(u, 4 * 1024 * 1024));
                let strip = if item.animated() { item.strip_url.as_deref().map(|u| net::image(u, 4 * 1024 * 1024)) } else { None };
                let (dh, dw) = (ih - 10.0, (ih - 10.0) * 10.0 / 16.0);
                match (strip, still) {
                    (Some(Pic::Ready(strip)), _) => {
                        let (fr, n) = (item.frame(ui.now / 1000.0) as f32, item.frames as f32);
                        c.image(&strip, ix + (iw - dw) / 2.0, iy + 5.0, dw, dh, [1.0 / 64.0, (fr * 32.0 + 1.0) / (32.0 * n), 11.0 / 64.0, (fr * 32.0 + 17.0) / (32.0 * n)], 0xFFFFFFFF);
                    }
                    (_, Some(Pic::Ready(img))) => {
                        c.image(&img, ix + (iw - dw) / 2.0, iy + 5.0, dw, dh, [1.0 / 64.0, 1.0 / 32.0, 11.0 / 64.0, 17.0 / 32.0], 0xFFFFFFFF);
                    }
                    (_, Some(Pic::Failed)) | (_, None) => {}
                    _ => ui.dots(c, ix + iw / 2.0, iy + ih / 2.0, theme::TEXT_MUTED),
                }
            }
            Some(item) => match item.still_url.as_deref().map(|u| net::image(u, 4 * 1024 * 1024)) {
                Some(Pic::Ready(img)) => {
                    let k = ((iw - 6.0) / img.width.max(1) as f32).min((ih - 6.0) / img.height.max(1) as f32);
                    let (dw, dh) = (img.width as f32 * k, img.height as f32 * k);
                    c.image(&img, ix + (iw - dw) / 2.0, iy + (ih - dh) / 2.0, dw, dh, [0.0, 0.0, 1.0, 1.0], 0xFFFFFFFF);
                }
                Some(Pic::Loading) => ui.dots(c, ix + iw / 2.0, iy + ih / 2.0, theme::TEXT_MUTED),
                _ => slot_icon(c, SLOT_ICONS[self.slot], 20.0, ix + iw / 2.0, iy + ih / 2.0, theme::TEXT_MUTED),
            },
        }

        if !is_owned
            && let Some(item) = it
        {
            let tag = item.price_label();
            let pw = c.text_width(font::SEMIBOLD, 9.0, &tag) + 10.0;
            let bg = if item.is_free_claimable() {
                0xD916A34A
            } else if item.exclusive {
                0xD97C3AED
            } else {
                0xD9181B24
            };
            c.round(ix + iw - pw - 4.0, iy + 4.0, pw, 16.0, 8.0, bg);
            let plh = c.line_height(font::SEMIBOLD, 9.0);
            c.text(font::SEMIBOLD, 9.0, &tag, ix + iw - pw + 1.0, iy + 4.0 + (16.0 - plh) / 2.0, 0xFFFFFFFF);
        }

        if eq > 0.02 {
            let sh = 18.0;
            c.push_clip(ix, iy + ih - sh * eq, iw, sh * eq);
            c.round(ix, iy + ih - sh, iw, sh, 7.0, theme::alpha(EQUIP_GREEN, 0.92 * eq));
            c.pop_clip();
            let txt = if it.is_none() { "Active" } else { "Equipped" };
            let etw = c.text_width(font::SEMIBOLD, 9.5, txt);
            let (ex, ey) = (ix + (iw - etw - 12.0) / 2.0, iy + ih - sh / 2.0);
            c.push_alpha(eq);
            c.icon(theme::I_CHECK, 9.5, ex + 4.0, ey, 0xFFFFFFFF);
            let lh = c.line_height(font::SEMIBOLD, 9.5);
            c.text(font::SEMIBOLD, 9.5, txt, ex + 12.0, ey - lh / 2.0, 0xFFFFFFFF);
            c.pop_alpha();
        } else if tr > 0.02 {
            let sh = 18.0;
            c.push_clip(ix, iy + ih - sh * tr, iw, sh * tr);
            c.round(ix, iy + ih - sh, iw, sh, 7.0, theme::alpha(0xFF7C3AED, 0.92 * tr));
            c.pop_clip();
            let txt = "Trying on";
            let etw = c.text_width(font::SEMIBOLD, 9.5, txt);
            let lh = c.line_height(font::SEMIBOLD, 9.5);
            c.push_alpha(tr);
            c.text(font::SEMIBOLD, 9.5, txt, ix + (iw - etw) / 2.0, iy + ih - sh / 2.0 - lh / 2.0, 0xFFFFFFFF);
            c.pop_alpha();
        }

        c.fill(x + 1.0, y + h - meta, w - 2.0, 1.0, theme::HAIRLINE);
        let my = y + h - meta / 2.0;
        if it.is_some_and(|i| l.busy.as_deref() == Some(i.id.as_str())) {
            ui.dots(c, x + w / 2.0, my, theme::TEXT_STRONG);
        } else {
            let n = c.ellipsize(font::SEMIBOLD, 11.0, it.map_or("Nothing", Item::display), w - 16.0);
            let lh = c.line_height(font::SEMIBOLD, 11.0);
            c.text(font::SEMIBOLD, 11.0, &n, x + 8.0, my - lh / 2.0, if is_worn || is_trying { theme::TEXT_STRONG } else { theme::TEXT });
        }
    }

    // ── Centre Column: 3D Stage (Avatar + Name ONLY, Dye Bar, Preview, Controls) ──

    #[allow(clippy::too_many_arguments)]
    fn draw_stage_card(&mut self, ui: &mut Ui, c: &mut Canvas, l: &wardrobe::Locker, who: &str, x: f32, y: f32, w: f32, h: f32) {
        c.round(x, y, w, h, 14.0, 0xCC06070B);
        c.gradient_v(x + 1.0, y + 1.0, w - 2.0, h * 0.55, 0x168B5CF6, 0x00000000);
        c.outline(x, y, w, h, 14.0, 1.0, theme::HAIRLINE);

        // Floating top bar: ONLY avatar and name (matching Launcher Locker!)
        let head_h = 44.0;
        let (hx, hy, hw) = (x + 12.0, y + 10.0, w - 24.0);
        c.round(hx, hy, hw, head_h, 11.0, 0xB00C0E15);
        c.outline(hx, hy, hw, head_h, 11.0, 1.0, theme::HAIRLINE);
        let display = if who.is_empty() { "Player" } else { who };
        let av = 28.0;
        let (ax, ay) = (hx + 9.0, hy + (head_h - av) / 2.0);
        avatar(c, display, ax, ay, av, 6.0);
        let fit_name = c.ellipsize(font::SEMIBOLD, 13.5, display, (hw - av - 26.0).max(20.0));
        let nlh = c.line_height(font::SEMIBOLD, 13.5);
        c.text(font::SEMIBOLD, 13.5, &fit_name, ax + av + 10.0, hy + (head_h - nlh) / 2.0, theme::TEXT_STRONG);

        let mut top_y = hy + head_h + 8.0;

        // Optional dye swatches bar when wearing a dyeable cosmetic in current slot
        if !self.emotes_mode
            && self.try_on.is_none()
            && let Some(dye_item) = self.active_dye_item(l)
            && !dye_item.dye_colors.is_empty()
        {
            let dye_h = 34.0;
            draw_dye_bar(ui, c, l, &dye_item, hx, top_y, hw, dye_h);
            top_y += dye_h + 6.0;
        }

        // Bottom controls pill
        let ctrl_h = 34.0;
        let ctrl_y = y + h - ctrl_h - 12.0;
        let mut bottom_y = ctrl_y - 8.0;

        // Optional "Trying on" bar
        if let Some(trying) = self.try_on.clone() {
            let try_h = 34.0;
            let try_y = bottom_y - try_h;
            let stop = draw_try_on_bar(ui, c, &trying, hx, try_y, hw, try_h);
            if stop {
                self.try_on = None;
            }
            bottom_y = try_y - 6.0;
        }

        // 3D Player preview
        self.preview.override_skin = self.resolve_preview_skin(l);
        let ph = (bottom_y - top_y).max(40.0);
        c.push_clip(x + 2.0, top_y, w - 4.0, ph);
        self.preview.draw(ui, c, "locker:stage:preview", x + 2.0, top_y, w - 4.0, ph, l, who, self.try_on.as_ref(), None);
        c.pop_clip();
        self.preview.override_skin = None;

        // Stage controls pill: Reset, Zoom -, Zoom +, Pause/Play
        let (btn_s, btn_gap) = (28.0, 6.0);
        let bar_w = btn_s * 4.0 + btn_gap * 3.0 + 12.0;
        let bar_x = x + (w - bar_w) / 2.0;
        c.round(bar_x, ctrl_y, bar_w, ctrl_h, 10.0, 0xCC0D1017);
        c.outline(bar_x, ctrl_y, bar_w, ctrl_h, 10.0, 1.0, theme::HAIRLINE);
        let mut bx = bar_x + 6.0;
        let by = ctrl_y + (ctrl_h - btn_s) / 2.0;
        if ui.icon_button(c, "locker:ctrl:reset", bx, by, btn_s, theme::I_RESET, Some("Reset view")) {
            self.preview.reset_view();
        }
        bx += btn_s + btn_gap;
        if ui.icon_button(c, "locker:ctrl:zout", bx, by, btn_s, theme::I_MINUS, Some("Zoom out")) {
            self.preview.zoom_by(0.88);
        }
        bx += btn_s + btn_gap;
        if ui.icon_button(c, "locker:ctrl:zin", bx, by, btn_s, theme::I_PLUS, Some("Zoom in")) {
            self.preview.zoom_by(1.14);
        }
        bx += btn_s + btn_gap;
        let pause_icon = if self.preview.paused { theme::I_PLAY } else { theme::I_PAUSE };
        let pause_tip = if self.preview.paused { "Resume idle" } else { "Pause idle" };
        if ui.icon_button(c, "locker:ctrl:pause", bx, by, btn_s, pause_icon, Some(pause_tip)) {
            self.preview.paused = !self.preview.paused;
        }
    }

    fn active_dye_item(&self, l: &wardrobe::Locker) -> Option<Item> {
        let slot_key = SLOTS[self.slot];
        let worn = if self.slot == 0 { wardrobe::item(l, l.equipped.as_deref()) } else { wardrobe::item(l, l.wearing.get(slot_key).map(String::as_str)) };
        worn.filter(|i| i.dyeable)
    }

    fn resolve_preview_skin(&self, l: &wardrobe::Locker) -> Option<(Arc<Image>, bool)> {
        let id = self.preview_skin_id.as_deref()?;
        let s = l.saved_skins.iter().find(|s| s.id == id)?;
        let u = match &s.hash {
            Some(h) => net::hash_url(&wardrobe::base(), h),
            None => s.url.clone(),
        };
        match net::image(&u, 2 * 1024 * 1024) {
            Pic::Ready(img) => Some((img, s.is_slim())),
            _ => None,
        }
    }

    // ── Right Column: Saved skins ────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn draw_skins_card(&mut self, ui: &mut Ui, c: &mut Canvas, l: &wardrobe::Locker, who: &str, x: f32, y: f32, w: f32, h: f32) {
        card_shell(c, x, y, w, h);

        let hh = 46.0;
        let tlh = c.line_height(font::SEMIBOLD, 14.0);
        c.text(font::SEMIBOLD, 14.0, "Saved skins", x + 14.0, y + (hh - tlh) / 2.0, theme::TEXT_STRONG);
        let tw = c.text_width(font::SEMIBOLD, 14.0, "Saved skins");
        let cnt = l.saved_skins.len().to_string();
        let cw = c.text_width(font::MEDIUM, 10.5, &cnt) + 12.0;
        let clh = c.line_height(font::MEDIUM, 10.5);
        c.round(x + 14.0 + tw + 8.0, y + (hh - 20.0) / 2.0, cw, 20.0, 10.0, theme::SUBTLE);
        c.text(font::MEDIUM, 10.5, &cnt, x + 14.0 + tw + 14.0, y + (hh - clh) / 2.0, theme::TEXT_SECONDARY);

        if ui.icon_button(c, "locker:skins:refresh", x + w - 36.0, y + (hh - 26.0) / 2.0, 26.0, theme::I_REFRESH, Some("Refresh saved skins")) && !who.is_empty() {
            wardrobe::refresh_skins(who, true);
        }
        c.fill(x + 1.0, y + hh, w - 2.0, 1.0, theme::HAIRLINE);

        let (bx, mut by, bw, mut bh) = (x + 10.0, y + hh + 10.0, w - 20.0, h - hh - 20.0);
        if l.saved_skins.len() > 6 {
            search_box(ui, c, &mut self.skin_search, "locker:skin:search", bx, by, bw, 28.0, "Search skins...");
            by += 36.0;
            bh -= 36.0;
        }
        if l.skin_notice.is_some() {
            bh -= 20.0;
        }

        if l.saved_skins.is_empty() {
            if l.skins_loading {
                ui.dots(c, bx + bw / 2.0, by + bh / 2.0, theme::TEXT_SECONDARY);
            } else {
                empty(c, bx, by, bw, bh.min(220.0), theme::I_USER, "No saved skins", "Skins you wear in the launcher appear here.");
            }
            return;
        }

        let needle = self.skin_search.value().trim().to_lowercase();
        let list: Vec<&SavedSkin> = l
            .saved_skins
            .iter()
            .filter(|s| needle.is_empty() || s.name.to_lowercase().contains(&needle))
            .collect();

        let dir_hash = if who.is_empty() {
            None
        } else {
            let dir = horizon_client::native_skins::directory();
            dir.entry(who).and_then(|e| e.skin.clone())
        };
        let active_hash = l.active_skin_hash.as_deref().or(dir_hash.as_deref());

        let gap = 8.0;
        let cols = 2usize;
        let card_w = (bw - gap) / cols as f32;
        let card_h = card_w * 1.15;
        let off = self.skin_scroll.begin(ui, c, bx, by, bw + 4.0, bh);
        for (i, skin) in list.iter().enumerate() {
            let ix = bx + (i % cols) as f32 * (card_w + gap);
            let iy = by + (i / cols) as f32 * (card_h + gap) - off;
            if iy + card_h < by - 4.0 || iy > by + bh + 4.0 {
                continue;
            }
            self.skin_tile(ui, c, l, skin, active_hash, who, ix, iy, card_w, card_h);
        }
        self.skin_scroll.end(ui, c, (list.len().div_ceil(cols) as f32 * (card_h + gap) - gap).max(0.0));

        if let Some(msg) = &l.skin_notice {
            let msg = c.ellipsize(font::MEDIUM, 10.5, msg, bw);
            c.text(font::MEDIUM, 10.5, &msg, bx, y + h - 15.0, theme::TEXT_SECONDARY);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn skin_tile(&mut self, ui: &mut Ui, c: &mut Canvas, l: &wardrobe::Locker, skin: &SavedSkin, active_hash: Option<&str>, who: &str, x: f32, y: f32, w: f32, h: f32) {
        let id = format!("locker:skin:{}", skin.id);
        let over = ui.hover(c, x, y, w, h);
        let active = active_hash.is_some_and(|ah| skin.hash.as_deref().is_some_and(|sh| sh.eq_ignore_ascii_case(ah)));
        if ui.clicked(c, &id, x, y, w, h) {
            self.preview_skin_id = Some(skin.id.clone());
            wardrobe::equip_skin(skin, who);
        }
        let hv = ui.anim_on(&format!("{id}#h"), over, 14.0);
        let on = ui.anim_on(&format!("{id}#on"), active, 14.0);

        if on > 0.02 {
            c.round(x, y, w, h, 12.0, theme::mix(TILE_BG, SKIN_PURPLE_BOT, on));
            c.push_clip(x, y, w, h);
            c.gradient_v(x, y, w, h, theme::alpha(SKIN_PURPLE_TOP, on), theme::alpha(SKIN_PURPLE_BOT, on));
            c.pop_clip();
        } else {
            c.round(x, y, w, h, 12.0, theme::mix(TILE_BG, TILE_HOVER, hv));
        }
        c.outline(
            x,
            y,
            w,
            h,
            12.0,
            if active { 1.5 } else { 1.0 },
            if active { 0xE6C4B5FD } else { theme::mix(theme::HAIRLINE, theme::BORDER_HOVER, hv) },
        );

        let url = match &skin.hash {
            Some(h) => net::hash_url(&wardrobe::base(), h),
            None => skin.url.clone(),
        };
        match net::image(&url, 2 * 1024 * 1024) {
            Pic::Ready(img) => {
                c.push_clip(x + 2.0, y + 2.0, w - 4.0, h - 4.0);
                preview3d::draw_skin_portrait(c, &img, skin.is_slim(), x + 4.0, y + 6.0, w - 8.0, h - 12.0);
                c.pop_clip();
            }
            Pic::Loading => ui.dots(c, x + w / 2.0, y + h / 2.0, theme::TEXT_MUTED),
            Pic::Failed => c.icon(theme::I_USER, 22.0, x + w / 2.0, y + h / 2.0, theme::TEXT_MUTED),
        }

        // White checkmark circle top-right on active skin
        if active {
            let (cx, cy) = (x + w - 14.0, y + 14.0);
            c.circle(cx, cy, 9.0, 0xFFFFFFFF);
            c.icon(theme::I_CHECK, 10.0, cx, cy, SKIN_PURPLE_BOT);
        }

        // Hover name bar at bottom
        if hv > 0.02 {
            let lh = 22.0;
            c.push_alpha(hv);
            c.push_clip(x + 1.0, y + h - lh, w - 2.0, lh);
            c.round(x + 1.0, y + h - lh - 6.0, w - 2.0, lh + 6.0, 11.0, 0xCC06070B);
            c.pop_clip();
            let name = c.ellipsize(font::SEMIBOLD, 10.5, &skin.name, w - 12.0);
            let nw = c.text_width(font::SEMIBOLD, 10.5, &name);
            let nlh = c.line_height(font::SEMIBOLD, 10.5);
            c.text(font::SEMIBOLD, 10.5, &name, x + (w - nw) / 2.0, y + h - lh + (lh - nlh) / 2.0, theme::TEXT_STRONG);
            c.pop_alpha();
        }
        if l.skin_busy.as_deref() == Some(skin.id.as_str()) {
            c.round(x, y, w, h, 12.0, 0x77000000);
            ui.dots(c, x + w / 2.0, y + h / 2.0, theme::TEXT_STRONG);
        }
    }
}

fn card_shell(c: &mut Canvas, x: f32, y: f32, w: f32, h: f32) {
    c.round(x, y, w, h, 14.0, SURFACE_CARD);
    c.outline(x, y, w, h, 14.0, 1.0, theme::HAIRLINE);
}

#[allow(clippy::too_many_arguments)]
fn seg_button(ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, w: f32, h: f32, label: &str, on: bool) -> bool {
    let over = ui.hover(c, x, y, w, h);
    let click = ui.clicked(c, id, x, y, w, h);
    let hv = ui.anim_on(&format!("{id}#h"), over, 14.0);
    let sl = ui.anim_on(&format!("{id}#on"), on, 16.0);
    if sl > 0.01 || hv > 0.01 {
        c.round(x, y, w, h, 7.0, theme::mix(theme::alpha(theme::SUBTLE_HOVER, hv * 0.7), 0xFFF4F4F5, sl));
    }
    let fg = theme::mix(theme::mix(theme::TEXT_SECONDARY, theme::TEXT_STRONG, hv), theme::SOLID_FG, sl);
    let tw = c.text_width(font::SEMIBOLD, 11.0, label);
    let lh = c.line_height(font::SEMIBOLD, 11.0);
    c.text(font::SEMIBOLD, 11.0, label, x + (w - tw) / 2.0, y + (h - lh) / 2.0, fg);
    click
}

#[allow(clippy::too_many_arguments)]
fn search_box(ui: &mut Ui, c: &mut Canvas, field: &mut TextField, id: &str, x: f32, y: f32, w: f32, h: f32, placeholder: &str) {
    let f = field.focused(ui);
    let fa = ui.anim_on(&format!("{id}#f"), f, 14.0);
    c.round(x, y, w, h, h / 2.0, theme::mix(0xB808090C, 0xE00E1014, fa));
    c.outline(x, y, w, h, h / 2.0, 1.0, theme::mix(theme::HAIRLINE, theme::BORDER_HOVER, fa));
    c.icon(theme::I_SEARCH, 12.0, x + 14.0, y + h / 2.0, theme::TEXT_MUTED);
    let clear = !field.is_empty();
    if ui.pressed && ui.hover(c, x, y, w - if clear { 26.0 } else { 0.0 }, h) {
        field.focus(ui);
        ui.focus_claimed = true;
    }
    field.draw(ui, c, x + 16.0, y, w - if clear { 44.0 } else { 20.0 }, h, placeholder, 11.5);
    if clear && ui.icon_button(c, &format!("{id}:clear"), x + w - 24.0, y + (h - 20.0) / 2.0, 20.0, theme::I_X, None) {
        field.clear();
        field.focus(ui);
        ui.focus_claimed = true;
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_dye_bar(ui: &mut Ui, c: &mut Canvas, l: &wardrobe::Locker, it: &Item, x: f32, y: f32, w: f32, h: f32) {
    c.round(x, y, w, h, 9.0, 0xB80D1017);
    c.outline(x, y, w, h, 9.0, 1.0, theme::HAIRLINE);
    c.icon(theme::I_PALETTE, 12.0, x + 14.0, y + h / 2.0, theme::TEXT_MUTED);
    let current = l.dyes.get(&it.id).map(String::as_str);
    let mut sx = x + 28.0;
    let sw = 18.0;
    let sy = y + (h - sw) / 2.0;
    let max_swatches = ((w - 36.0) / (sw + 5.0)).max(1.0) as usize;
    for (i, hex) in it.dye_colors.iter().take(max_swatches).enumerate() {
        let col = parse_hex(hex);
        let active = match current {
            Some(cur) => cur.eq_ignore_ascii_case(hex),
            None => it.dye_default.as_deref().is_some_and(|d| d.eq_ignore_ascii_case(hex)),
        };
        let over = ui.hover(c, sx, sy, sw, sw);
        if ui.clicked(c, &format!("locker:dye:{}:{i}", it.id), sx, sy, sw, sw) {
            wardrobe::set_dye(it, Some(hex));
        }
        c.circle(sx + sw / 2.0, sy + sw / 2.0, sw / 2.0 - 1.0, col);
        if active || over {
            c.outline(sx, sy, sw, sw, sw / 2.0, if active { 1.8 } else { 1.0 }, if active { 0xFFFFFFFF } else { theme::BORDER_HOVER });
        }
        sx += sw + 5.0;
    }
}

/// Returns `true` when the try-on bar should close.
#[allow(clippy::too_many_arguments)]
fn draw_try_on_bar(ui: &mut Ui, c: &mut Canvas, it: &Item, x: f32, y: f32, w: f32, h: f32) -> bool {
    c.round(x, y, w, h, 9.0, 0xD9161224);
    c.outline(x, y, w, h, 9.0, 1.0, 0x998B5CF6);
    let close_s = 22.0;
    let close_x = x + w - close_s - 6.0;
    let act_w = if it.is_free_claimable() { 78.0 } else { 0.0 };
    let label = c.ellipsize(font::SEMIBOLD, 11.0, &format!("Trying: {}", it.display()), (w - close_s - act_w - 22.0).max(20.0));
    let lh = c.line_height(font::SEMIBOLD, 11.0);
    c.text(font::SEMIBOLD, 11.0, &label, x + 10.0, y + (h - lh) / 2.0, theme::TEXT_STRONG);
    let mut close = false;
    if it.is_free_claimable() && ui.button(c, "locker:try:add", close_x - act_w - 4.0, y + 4.0, act_w, h - 8.0, "Add & wear", 0, ui::BTN_PRIMARY) {
        wardrobe::claim_and_wear(it);
        close = true;
    }
    if ui.icon_button(c, "locker:try:close", close_x, y + (h - close_s) / 2.0, close_s, theme::I_X, Some("Stop trying on")) {
        close = true;
    }
    close
}

/// What Relay and the locker show while the game is not signed in to Native: the reason and, where it helps, a "Try again".
pub fn session_lock(ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, w: f32, h: f32) {
    let status = net::status();
    if status == net::Status::Checking {
        ui.dots(c, x + w / 2.0, y + h / 2.0 - 24.0, theme::TEXT_SECONDARY);
        let title = net::lock_title();
        let tw = c.text_width(font::SEMIBOLD, 13.5, title);
        c.text(font::SEMIBOLD, 13.5, title, x + (w - tw) / 2.0, y + h / 2.0, theme::TEXT_STRONG);
        return;
    }
    let icon = if status == net::Status::Offline { theme::I_WIFI_OFF } else { theme::I_LOCK };
    let action = matches!(status, net::Status::Offline | net::Status::Expired);
    empty(c, x, y, w, if action { h - 40.0 } else { h }, icon, net::lock_title(), net::lock_body());
    if action && ui.button(c, &format!("{id}:retry"), x + w / 2.0 - 62.0, y + h / 2.0 + 44.0, 124.0, 34.0, "Try again", theme::I_REFRESH, ui::BTN_GLASS) {
        net::retry();
    }
}
