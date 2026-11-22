//! The unified Locker (Cosmetics + Emotes sidebar, search + A-Z sort, 3-column square rounded cards with crisp
//! emerald `✓ Equipped` bottom strip and centered labels below, and right-hand 3D rotatable player preview +
//! Equip / Take off controls).
#![allow(dead_code)]

use super::menu::{CARD, CARD_HOVER, clamp01, disabled, ease, empty, glow};
use super::net::{self, Pic};
use super::preview3d::PlayerPreview;
use super::scroll::Scroll;
use super::textfield::TextField;
use super::ui::{self, Ui};
use super::wardrobe::{self, Item, SLOT_NAMES, SLOTS, State};
use horizon_client::native_emotes;
use horizon_client::native_ui::{Canvas, font, icons, theme};

/// One per wardrobe slot, for item cards without a picture; `BALLOON_ICON` is drawn by hand (the icon font has no balloon).
const BALLOON_ICON: u32 = 0;
pub const SLOT_ICONS: [u32; 7] = [
    theme::I_LAYERS,
    theme::I_HAT,
    theme::I_BACKPACK,
    theme::I_FOOTPRINTS,
    theme::I_HAND,
    theme::I_SPARKLES,
    BALLOON_ICON,
];

/// Navigation choice returned by the shared Locker sidebar (`Some(0..7)` = cosmetic slot, `Some(7)` = Emotes).
pub const SIDEBAR_EMOTES: usize = 7;
pub const SIDEBAR_W: f32 = 194.0;

/// A slot's icon; the balloon is a round balloon, its knot and a wavy string.
pub fn slot_icon(c: &mut Canvas, icon: u32, size: f32, cx: f32, cy: f32, color: u32) {
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

/// Shared unified Locker left sidebar (~194 px): Locker header + Cosmetics slots (`Capes`..`Balloons`) with counts
/// and green equipped dots + `Emotes` entry + bottom `Native Store` button.
/// Returns `Some(0..7)` when a cosmetic slot is clicked, or `Some(SIDEBAR_EMOTES)` when Emotes is clicked.
#[allow(clippy::too_many_arguments)]
pub fn draw_sidebar(ui: &mut Ui, c: &mut Canvas, active: usize, x: f32, y: f32, w: f32, h: f32) -> Option<usize> {
    let mut picked = None;
    let l = wardrobe::snapshot();
    let emotes_snap = native_emotes::snapshot();
    let emote_count = emotes_snap["catalog"]
        .as_array()
        .map_or(0, |a| a.iter().filter(|o| o["owned"].as_bool().unwrap_or(true)).count());
    let has_emote_equipped = native_emotes::pages_of(&emotes_snap).iter().any(|p| p.iter().any(Option::is_some));

    // Locker header
    let emblem = icons::for_module("watermark", theme::I_SPARKLES);
    c.icon(emblem, 18.0, x + 16.0, y + 16.0, theme::TEXT_STRONG);
    let lh = c.line_height(font::BOLD, 15.0);
    c.text(font::BOLD, 15.0, "Locker", x + 32.0, y + 16.0 - lh / 2.0, theme::TEXT_STRONG);

    let total_cosmetics: usize = (0..SLOTS.len()).map(|i| wardrobe::owned_in(&l, SLOTS[i]).len()).sum();
    let any_cosmetic = l.equipped.is_some() || !l.wearing.is_empty();
    let mut ry = y + 42.0;

    // Cosmetics group header
    c.icon(theme::I_SHIRT, 13.0, x + 14.0, ry + 11.0, theme::TEXT_SECONDARY);
    let hdr = format!("Cosmetics ({total_cosmetics})");
    let hlh = c.line_height(font::SEMIBOLD, 11.5);
    c.text(font::SEMIBOLD, 11.5, &hdr, x + 28.0, ry + 11.0 - hlh / 2.0, theme::TEXT_STRONG);
    if any_cosmetic {
        c.circle(x + w - 14.0, ry + 11.0, 3.2, theme::ONLINE);
    }
    ry += 26.0;

    // 7 Cosmetic slot rows
    let row_h = 31.0;
    for i in 0..SLOTS.len() {
        let count = wardrobe::owned_in(&l, SLOTS[i]).len();
        let worn = if i == 0 { l.equipped.is_some() } else { l.wearing.contains_key(SLOTS[i]) };
        let sel = active == i;
        let id = format!("locker:nav:{i}");
        let over = ui.hover(c, x, ry, w, row_h);
        if ui.clicked(c, &id, x, ry, w, row_h) {
            picked = Some(i);
        }
        let hv = ui.anim_on(&format!("{id}#h"), over, 14.0);
        let on = ui.anim_on(&format!("{id}#on"), sel, 16.0);
        let bg = theme::mix(theme::alpha(0x14FFFFFF, hv), 0x22FFFFFF, on);
        if bg >> 24 > 0 {
            c.round(x, ry, w, row_h, 7.0, bg);
        }
        if on > 0.05 {
            c.outline(x, ry, w, row_h, 7.0, 1.0, theme::alpha(theme::HAIRLINE_STRONG, on));
        }
        let fg = theme::mix(theme::mix(theme::TEXT_SECONDARY, theme::TEXT_STRONG, hv), theme::TEXT_STRONG, on);
        slot_icon(c, SLOT_ICONS[i], 13.5, x + 20.0, ry + row_h / 2.0, fg);
        let label = format!("{} ({count})", SLOT_NAMES[i]);
        let label = c.ellipsize(font::MEDIUM, 11.5, &label, w - 50.0);
        let rlh = c.line_height(font::MEDIUM, 11.5);
        c.text(font::MEDIUM, 11.5, &label, x + 34.0, ry + (row_h - rlh) / 2.0, fg);
        if worn {
            c.circle(x + w - 14.0, ry + row_h / 2.0, 3.2, theme::ONLINE);
        }
        if over {
            ui.cursor_hand = true;
        }
        ry += row_h + 2.0;
    }

    ry += 6.0;
    c.fill(x + 4.0, ry, w - 8.0, 1.0, theme::HAIRLINE);
    ry += 8.0;

    // Emotes entry
    {
        let sel = active == SIDEBAR_EMOTES;
        let id = "locker:nav:emotes";
        let over = ui.hover(c, x, ry, w, row_h + 2.0);
        if ui.clicked(c, id, x, ry, w, row_h + 2.0) {
            picked = Some(SIDEBAR_EMOTES);
        }
        let hv = ui.anim_on("locker:nav:emotes#h", over, 14.0);
        let on = ui.anim_on("locker:nav:emotes#on", sel, 16.0);
        let bg = theme::mix(theme::alpha(0x14FFFFFF, hv), 0x22FFFFFF, on);
        if bg >> 24 > 0 {
            c.round(x, ry, w, row_h + 2.0, 7.0, bg);
        }
        if on > 0.05 {
            c.outline(x, ry, w, row_h + 2.0, 7.0, 1.0, theme::alpha(theme::HAIRLINE_STRONG, on));
        }
        let fg = theme::mix(theme::mix(theme::TEXT_SECONDARY, theme::TEXT_STRONG, hv), theme::TEXT_STRONG, on);
        c.icon(theme::I_SMILE, 14.0, x + 20.0, ry + (row_h + 2.0) / 2.0, fg);
        let label = format!("Emotes ({emote_count})");
        let label = c.ellipsize(font::SEMIBOLD, 11.5, &label, w - 50.0);
        let rlh = c.line_height(font::SEMIBOLD, 11.5);
        c.text(font::SEMIBOLD, 11.5, &label, x + 34.0, ry + (row_h + 2.0 - rlh) / 2.0, fg);
        if has_emote_equipped {
            c.circle(x + w - 14.0, ry + (row_h + 2.0) / 2.0, 3.2, theme::ONLINE);
        }
        if over {
            ui.cursor_hand = true;
        }
    }

    // Bottom: Native Store button
    let store_h = 34.0;
    let store_y = y + h - store_h;
    let over_store = ui.hover(c, x, store_y, w, store_h);
    let shv = ui.anim_on("locker:store#h", over_store, 14.0);
    c.round(x, store_y, w, store_h, 8.0, theme::mix(0xFF171A24, 0xFF212636, shv));
    c.outline(x, store_y, w, store_h, 8.0, 1.0, theme::mix(0x334F86F7, 0x774F86F7, shv));
    let stxt = "Native Store";
    let sw = c.text_width(font::SEMIBOLD, 11.5, stxt);
    let slh = c.line_height(font::SEMIBOLD, 11.5);
    let sx = x + (w - (sw + 18.0)) / 2.0;
    c.icon(theme::I_STORE, 13.0, sx + 6.0, store_y + store_h / 2.0, 0xFF8AB4FF);
    c.text(font::SEMIBOLD, 11.5, stxt, sx + 18.0, store_y + (store_h - slh) / 2.0, theme::TEXT_STRONG);
    if over_store {
        ui.cursor_hand = true;
        ui.tip("Get more cosmetics & emotes in the Native Store in the launcher");
    }

    picked
}

pub struct Locker {
    slot: usize,
    /// Selection in the current slot: `None` = nothing picked yet, `Some("")` = the "Nothing" card, else an item id.
    pick: Option<String>,
    scroll: Scroll,
    search: TextField,
    sort_desc: bool,
    preview: PlayerPreview,
    /// When the slot last changed: only the items animate, the 3D preview stays solid.
    locker_at: f64,
}

impl Default for Locker {
    fn default() -> Self {
        Self::new()
    }
}

fn singular(slot: usize) -> &'static str {
    ["cape", "hat", "back item", "shoes", "hand item", "aura", "balloon"][slot]
}

impl Locker {
    pub fn new() -> Self {
        let mut search = TextField::new("locker:search");
        search.max_length = 48;
        search.bare = true;
        Locker {
            slot: 0,
            pick: None,
            scroll: Scroll::new("locker"),
            search,
            sort_desc: false,
            preview: PlayerPreview::new(),
            locker_at: 0.0,
        }
    }

    /// Switches to a specific cosmetic slot (0..7) from the unified Locker sidebar.
    pub fn set_slot(&mut self, slot: usize, now: f64) {
        let slot = slot.min(SLOTS.len() - 1);
        if self.slot != slot {
            self.slot = slot;
            self.pick = None;
            self.scroll.reset();
            self.locker_at = now;
        }
    }

    pub fn escape(&mut self, ui: &mut Ui) -> bool {
        if self.search.focused(ui) {
            ui.focus = None;
            if !self.search.is_empty() {
                self.search.clear();
            }
            return true;
        }
        false
    }

    /// The tab was opened: look at the locker again.
    pub fn opened(&mut self) {
        wardrobe::refresh(false);
    }

    /// Draws the unified Cosmetics Locker view. Returns `true` when the user clicked `Emotes` in the sidebar.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&mut self, ui: &mut Ui, c: &mut Canvas, who: &str, page_at: f64, x: f32, y: f32, w: f32, h: f32) -> bool {
        // 1. Left Sidebar (~194 px)
        let sb_w = SIDEBAR_W.min(w * 0.24);
        if let Some(nav) = draw_sidebar(ui, c, self.slot, x, y, sb_w, h) {
            if nav == SIDEBAR_EMOTES {
                return true;
            }
            self.set_slot(nav, ui.now);
        }
        c.fill(x + sb_w + 10.0, y, 1.0, h, theme::HAIRLINE);

        let l = wardrobe::snapshot();
        let worn = if self.slot == 0 {
            wardrobe::item(&l, l.equipped.as_deref())
        } else {
            wardrobe::item(&l, l.wearing.get(SLOTS[self.slot]).map(String::as_str))
        };
        let picked = self.pick.as_deref().filter(|p| !p.is_empty()).and_then(|p| wardrobe::item(&l, Some(p)));
        let nothing = self.pick.as_deref() == Some("");

        // 2. Right column: 3D rotatable player preview + Equip / Take off controls
        let right_w = (w * 0.28).clamp(220.0, 268.0);
        let rx = x + w - right_w;
        c.fill(rx - 10.0, y, 1.0, h, theme::HAIRLINE);
        self.draw_preview_pane(ui, c, &l, who, worn.as_ref(), picked.as_ref(), nothing, rx, y, right_w, h);

        // 3. Center column: Search bar + A-Z sort toggle + 3-column square rounded cards
        let cx = x + sb_w + 22.0;
        let cw = (rx - 22.0 - cx).max(160.0);

        if !net::signed_in() {
            session_lock(ui, c, "menu:locker:lock", cx, y, cw, h);
            return false;
        }
        if l.state == State::NoAccount {
            wardrobe::refresh(false);
        }
        if matches!(l.state, State::Idle | State::Loading | State::NoAccount) {
            ui.dots(c, cx + cw / 2.0, y + h / 2.0, theme::TEXT_SECONDARY);
            if l.state == State::Idle {
                wardrobe::refresh(false);
            }
            return false;
        }
        if l.state == State::Error {
            empty(c, cx, y, cw, h - 60.0, theme::I_WIFI_OFF, "Could not load your locker", "Check your connection and try again.");
            if ui.button(c, "menu:wardrobe:retry", cx + cw / 2.0 - 60.0, y + h / 2.0 + 20.0, 120.0, 36.0, "Retry", theme::I_REFRESH, ui::BTN_GLASS) {
                wardrobe::refresh(true);
            }
            return false;
        }

        // Top search + A-Z sort bar
        let bar_h = 32.0;
        let sort_w = 66.0;
        let search_w = (cw - sort_w - 8.0).max(80.0);
        self.draw_search(ui, c, cx, y, search_w, bar_h);
        let sort_x = cx + search_w + 8.0;
        let sort_lbl = if self.sort_desc { "Z-A" } else { "A-Z" };
        let over_sort = ui.hover(c, sort_x, y, sort_w, bar_h);
        let shv = ui.anim_on("locker:sort#h", over_sort, 14.0);
        c.round(sort_x, y, sort_w, bar_h, 8.0, theme::mix(0xFF14161E, 0xFF1C202C, shv));
        c.outline(sort_x, y, sort_w, bar_h, 8.0, 1.0, theme::mix(theme::HAIRLINE, theme::HAIRLINE_STRONG, shv));
        c.icon(theme::I_ARROWS_UP, 11.5, sort_x + 16.0, y + bar_h / 2.0, theme::TEXT_SECONDARY);
        let slh = c.line_height(font::SEMIBOLD, 11.0);
        c.text(font::SEMIBOLD, 11.0, sort_lbl, sort_x + 28.0, y + (bar_h - slh) / 2.0, theme::TEXT_STRONG);
        if over_sort {
            ui.cursor_hand = true;
            ui.tip("Toggle alphabetical sort");
        }
        if ui.clicked(c, "locker:sort", sort_x, y, sort_w, bar_h) {
            self.sort_desc = !self.sort_desc;
        }

        // Filter & sort items
        let needle = self.search.value().trim().to_lowercase();
        let mut items: Vec<Item> = wardrobe::owned_in(&l, SLOTS[self.slot])
            .into_iter()
            .filter(|it| needle.is_empty() || it.display().to_lowercase().contains(&needle))
            .collect();
        items.sort_by(|a, b| {
            let ord = a.display().to_lowercase().cmp(&b.display().to_lowercase());
            if self.sort_desc { ord.reverse() } else { ord }
        });

        let gy = y + bar_h + 12.0;
        let gh = y + h - gy - if l.error.is_some() { 24.0 } else { 0.0 };
        let gap = 12.0;
        let cols = 3usize;
        let card_w = (cw - gap * (cols - 1) as f32) / cols as f32;
        let card_sq = card_w;
        let label_h = 22.0;
        let cell_h = card_sq + label_h;
        let show_nothing = needle.is_empty();
        let total = items.len() + usize::from(show_nothing);

        if total == 0 {
            empty(c, cx, gy + 10.0, cw, gh.min(220.0), theme::I_SEARCH, "No cosmetics found", "Try another search or category.");
        } else {
            let off = self.scroll.begin(ui, c, cx, gy, cw + 6.0, gh);
            for i in 0..total {
                let ix = cx + (i % cols) as f32 * (card_w + gap);
                let iy = gy + (i / cols) as f32 * (cell_h + gap) - off;
                if iy + cell_h < gy - 4.0 || iy > gy + gh + 4.0 {
                    continue;
                }
                let e = ease(clamp01(((ui.now - page_at.max(self.locker_at) - 20.0 - i as f64 * 16.0) / 260.0) as f32));
                c.push_alpha(e);
                let item = if show_nothing && i == 0 {
                    None
                } else {
                    Some(&items[i - usize::from(show_nothing)])
                };
                self.tile(ui, c, &l, item, ix, iy + (1.0 - e) * 8.0, card_w, card_sq, label_h, worn.as_ref());
                c.pop_alpha();
            }
            self.scroll.end(ui, c, total.div_ceil(cols) as f32 * (cell_h + gap) - gap);
        }

        if let Some(err) = &l.error {
            let err = c.ellipsize(font::MEDIUM, 11.0, err, cw);
            c.text(font::MEDIUM, 11.0, &err, cx, y + h - 16.0, 0xFFFCA5A5);
        }
        false
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_preview_pane(
        &mut self,
        ui: &mut Ui,
        c: &mut Canvas,
        l: &wardrobe::Locker,
        who: &str,
        worn: Option<&Item>,
        picked: Option<&Item>,
        nothing: bool,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) {
        c.round(x, y, w, h, 10.0, 0xFF101218);
        glow(c, x, y, w, h * 0.45, 10.0, 0x0CFFFFFF);
        c.outline(x, y, w, h, 10.0, 1.0, theme::HAIRLINE);
        c.text(font::SEMIBOLD, 10.0, "PREVIEW", x + 14.0, y + 14.0, theme::TEXT_MUTED);
        let title = if nothing {
            format!("No {}", singular(self.slot))
        } else if let Some(p) = picked {
            p.display().to_owned()
        } else {
            "Your look".to_owned()
        };
        let title = c.ellipsize(font::SEMIBOLD, 14.0, &title, w - 28.0);
        c.text(font::SEMIBOLD, 14.0, &title, x + 14.0, y + 29.0, theme::TEXT_STRONG);
        let bare = nothing.then_some(SLOTS[self.slot]);
        c.push_clip(x + 1.0, y + 52.0, w - 2.0, h - 52.0 - 92.0);
        self.preview.draw(ui, c, "menu:preview", x, y + 56.0, w, h - 56.0 - 98.0, l, who, picked, bare);
        c.pop_clip();
        let hint = "Drag to turn";
        let hw = c.text_width(font::REGULAR, 10.5, hint);
        c.text(font::REGULAR, 10.5, hint, x + (w - hw) / 2.0, y + h - 84.0, theme::TEXT_MUTED);
        c.fill(x + 14.0, y + h - 64.0, w - 28.0, 1.0, theme::HAIRLINE);
        let (bx, by, bw, bh) = (x + 14.0, y + h - 50.0, w - 28.0, 36.0);
        let ready = l.state == State::Ready;
        if l.busy.is_some() {
            c.round(bx, by, bw, bh, 8.0, theme::SUBTLE_HOVER);
            ui.dots(c, bx + bw / 2.0, by + bh / 2.0, theme::TEXT_STRONG);
        } else if !ready || (picked.is_none() && !nothing) {
            disabled(c, bx, by, bw, bh, if ready { "Pick an item to try on" } else { "Equip" });
        } else if nothing {
            if let Some(worn) = worn {
                if ui.button(c, "menu:locker:off", bx, by, bw, bh, &format!("Take off {}", singular(self.slot)), theme::I_BAN, ui::BTN_PRIMARY) {
                    wardrobe::toggle(worn);
                }
            } else {
                disabled(c, bx, by, bw, bh, "Nothing worn");
            }
        } else if let Some(p) = picked {
            if wardrobe::is_worn(l, p) {
                if ui.button(c, "menu:locker:off", bx, by, bw, bh, "Take off", theme::I_X, ui::BTN_GLASS) {
                    wardrobe::toggle(p);
                }
            } else if ui.button(c, "menu:locker:on", bx, by, bw, bh, "Equip", theme::I_CHECK, ui::BTN_PRIMARY) {
                wardrobe::toggle(p);
            }
        }
    }

    fn draw_search(&mut self, ui: &mut Ui, c: &mut Canvas, x: f32, y: f32, w: f32, h: f32) {
        let f = self.search.focused(ui);
        let fa = ui.anim_on("locker:search#f", f, 14.0);
        c.round(x, y, w, h, 8.0, theme::mix(0xFF14161E, 0xFF1A1E29, fa));
        c.outline(x, y, w, h, 8.0, 1.0, theme::mix(theme::HAIRLINE, theme::BORDER_HOVER, fa));
        c.icon(theme::I_SEARCH, 13.0, x + 15.0, y + h / 2.0, theme::TEXT_MUTED);
        let clear = !self.search.is_empty();
        if ui.pressed && ui.hover(c, x, y, w - if clear { 28.0 } else { 0.0 }, h) {
            self.search.focus(ui);
            ui.focus_claimed = true;
        }
        let ph = format!("Search {}...", SLOT_NAMES[self.slot].to_lowercase());
        self.search.draw(ui, c, x + 18.0, y, w - if clear { 48.0 } else { 22.0 }, h, &ph, 12.0);
        if clear && ui.icon_button(c, "locker:search:clear", x + w - 26.0, y + (h - 22.0) / 2.0, 22.0, theme::I_X, None) {
            self.search.clear();
            self.search.focus(ui);
            ui.focus_claimed = true;
        }
    }

    /// Square rounded card (`radius = 10px`) with crisp emerald `✓ Equipped` bottom strip and centered name below.
    #[allow(clippy::too_many_arguments)]
    fn tile(
        &mut self,
        ui: &mut Ui,
        c: &mut Canvas,
        l: &wardrobe::Locker,
        it: Option<&Item>,
        x: f32,
        y: f32,
        w: f32,
        sq_h: f32,
        label_h: f32,
        worn: Option<&Item>,
    ) {
        let key = it.map_or("", |i| i.id.as_str()).to_owned();
        let id = format!("menu:item:{}:{key}", self.slot);
        let over = ui.hover(c, x, y, w, sq_h + label_h);
        if ui.clicked(c, &id, x, y, w, sq_h + label_h) {
            self.pick = Some(key.clone());
        }
        let is_worn = it.map_or(worn.is_none(), |i| wardrobe::is_worn(l, i));
        let sel = self.pick.as_deref() == Some(key.as_str());
        let hv = ui.anim_on(&format!("{id}#h"), over, 14.0);
        let sl = ui.anim_on(&format!("{id}#s"), sel, 16.0);
        let base_bg = if is_worn && it.is_some() { 0xFF142119 } else { 0xFF181A22 };
        let hover_bg = if is_worn && it.is_some() { 0xFF1A2D21 } else { 0xFF212430 };
        c.round(x, y, w, sq_h, 10.0, theme::mix(base_bg, hover_bg, hv.max(sl * 0.6)));
        let border = if sel {
            0xE6FFFFFF
        } else if is_worn && it.is_some() {
            0xAA22C55E
        } else {
            theme::mix(theme::HAIRLINE, theme::BORDER_HOVER, hv)
        };
        c.outline(x, y, w, sq_h, 10.0, if sel || (is_worn && it.is_some()) { 1.4 } else { 1.0 }, border);

        let strip_h = if is_worn && it.is_some() { 22.0 } else { 0.0 };
        let (ix, iy, iw, ih) = (x + 8.0, y + 8.0, w - 16.0, sq_h - 16.0 - strip_h);
        match it {
            None => c.icon(theme::I_BAN, 26.0, ix + iw / 2.0, iy + ih / 2.0, theme::mix(theme::TEXT_MUTED, theme::TEXT_SECONDARY, hv)),
            Some(item) if item.is_cape() => {
                let still = item.still_url.as_deref().map(|u| net::image(u, 4 * 1024 * 1024));
                let strip = if item.animated() { item.strip_url.as_deref().map(|u| net::image(u, 4 * 1024 * 1024)) } else { None };
                let dh = (ih - 6.0).max(20.0);
                let dw = dh * 10.0 / 16.0;
                match (strip, still) {
                    (Some(Pic::Ready(strip)), _) => {
                        let (fr, n) = (item.frame(ui.now / 1000.0) as f32, item.frames as f32);
                        c.image(&strip, ix + (iw - dw) / 2.0, iy + (ih - dh) / 2.0, dw, dh, [1.0 / 64.0, (fr * 32.0 + 1.0) / (32.0 * n), 11.0 / 64.0, (fr * 32.0 + 17.0) / (32.0 * n)], 0xFFFFFFFF);
                    }
                    (_, Some(Pic::Ready(img))) => {
                        c.image(&img, ix + (iw - dw) / 2.0, iy + (ih - dh) / 2.0, dw, dh, [1.0 / 64.0, 1.0 / 32.0, 11.0 / 64.0, 17.0 / 32.0], 0xFFFFFFFF);
                    }
                    (_, Some(Pic::Failed)) | (_, None) => {}
                    _ => ui.dots(c, ix + iw / 2.0, iy + ih / 2.0, theme::TEXT_MUTED),
                }
            }
            Some(item) => match item.still_url.as_deref().map(|u| net::image(u, 4 * 1024 * 1024)) {
                Some(Pic::Ready(img)) => {
                    let k = ((iw - 4.0) / img.width.max(1) as f32).min((ih - 4.0) / img.height.max(1) as f32);
                    let (dw, dh) = (img.width as f32 * k, img.height as f32 * k);
                    c.image(&img, ix + (iw - dw) / 2.0, iy + (ih - dh) / 2.0, dw, dh, [0.0, 0.0, 1.0, 1.0], 0xFFFFFFFF);
                }
                Some(Pic::Loading) => ui.dots(c, ix + iw / 2.0, iy + ih / 2.0, theme::TEXT_MUTED),
                _ => slot_icon(c, SLOT_ICONS[self.slot], 22.0, ix + iw / 2.0, iy + ih / 2.0, theme::TEXT_MUTED),
            },
        }
        if let Some(item) = it
            && item.partner_name.is_some()
        {
            partner_tag(c, item, x + 6.0, y + 6.0, w - 12.0);
        }

        // Crisp emerald green `✓ Equipped` bottom strip inside the square card.
        if is_worn && it.is_some() {
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

        // Centered name below the square card.
        let my = y + sq_h + 4.0;
        if it.is_some_and(|i| l.busy.as_deref() == Some(i.id.as_str())) {
            ui.dots(c, x + w / 2.0, my + 8.0, theme::TEXT_STRONG);
        } else {
            let name = it.map_or("Nothing", Item::display);
            let name = c.ellipsize(font::SEMIBOLD, 11.0, name, w - 4.0);
            let nw = c.text_width(font::SEMIBOLD, 11.0, &name);
            c.text(font::SEMIBOLD, 11.0, &name, x + (w - nw) / 2.0, my, if is_worn || sel { theme::TEXT_STRONG } else { theme::TEXT });
        }
        if over {
            ui.cursor_hand = true;
        }
    }
}

/// "Aurex" with the partner's logo: the item was made with a Native partner.
fn partner_tag(c: &mut Canvas, it: &Item, x: f32, y: f32, max_w: f32) {
    let name = it.partner_name.as_deref().unwrap_or("");
    let lower = name.to_lowercase();
    let label = ["hosting", "host", "network", "studios", "studio"]
        .iter()
        .find_map(|s| lower.strip_suffix(s).filter(|rest| rest.ends_with(' ')).map(|rest| name[..rest.trim_end().len()].to_owned()))
        .unwrap_or_else(|| name.to_owned());
    let (h, logo) = (18.0, 14.0);
    let label = c.ellipsize(font::SEMIBOLD, 10.0, &label, (max_w - logo - 14.0).max(20.0));
    let w = c.text_width(font::SEMIBOLD, 10.0, &label) + logo + 12.0;
    c.round(x, y, w, h, 9.0, theme::mix(it.partner_accent, 0xFF000000, 0.15));
    c.circle(x + 2.0 + logo / 2.0, y + h / 2.0, logo / 2.0, 0xFFFFFFFF);
    if let Some(Pic::Ready(img)) = it.partner_logo.as_deref().map(|u| net::image(u, 256 * 1024)) {
        c.image(&img, x + 3.0, y + 3.0, logo - 2.0, logo - 2.0, [0.0, 0.0, 1.0, 1.0], 0xFFFFFFFF);
    }
    let lh = c.line_height(font::SEMIBOLD, 10.0);
    c.text(font::SEMIBOLD, 10.0, &label, x + logo + 6.0, y + (h - lh) / 2.0, 0xFFFFFFFF);
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
