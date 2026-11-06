//! The Cosmetics tab (same layout as the launcher locker): a stage with the
//! player's model on the left, the slots and the owned items on the right.
#![allow(dead_code)]

use super::menu::{CARD, CARD_HOVER, clamp01, disabled, ease, empty, glow};
use super::net::{self, Pic};
use super::preview3d::PlayerPreview;
use super::scroll::Scroll;
use super::ui::{self, Ui};
use super::wardrobe::{self, Item, SLOT_NAMES, SLOTS, State};
use horizon_client::native_ui::{Canvas, font, theme};

const SLOT_ICONS: [u32; 6] = [theme::I_LAYERS, theme::I_HAT, theme::I_GLASSES, theme::I_BACKPACK, theme::I_FOOTPRINTS, theme::I_HAND];

pub struct Locker {
    slot: usize,
    /// Selection in the current slot: `None` = nothing picked yet, `Some("")` = the "Nothing" card, else an item id.
    pick: Option<String>,
    scroll: Scroll,
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
    ["cape", "hat", "glasses", "back item", "shoes", "hand item"][slot]
}

impl Locker {
    pub fn new() -> Self {
        Locker { slot: 0, pick: None, scroll: Scroll::new("locker"), preview: PlayerPreview::new(), locker_at: 0.0 }
    }

    /// The tab was opened: look at the locker again.
    pub fn opened(&mut self) {
        wardrobe::refresh(false);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(&mut self, ui: &mut Ui, c: &mut Canvas, who: &str, page_at: f64, x: f32, y: f32, w: f32, h: f32) {
        let l = wardrobe::snapshot();
        let sw = (w * 0.36).min(320.0);
        let worn = if self.slot == 0 { wardrobe::item(&l, l.equipped.as_deref()) } else { wardrobe::item(&l, l.wearing.get(SLOTS[self.slot]).map(String::as_str)) };
        let picked = self.pick.as_deref().filter(|p| !p.is_empty()).and_then(|p| wardrobe::item(&l, Some(p)));
        let nothing = self.pick.as_deref() == Some("");

        // The stage.
        c.round(x, y, sw, h, 14.0, 0x66000000);
        glow(c, x, y, sw, h * 0.6, 14.0, 0x10FFFFFF);
        c.outline(x, y, sw, h, 14.0, 1.0, theme::HAIRLINE);
        c.text(font::SEMIBOLD, 10.5, "PREVIEW", x + 16.0, y + 16.0, theme::TEXT_MUTED);
        let title = if nothing {
            format!("No {}", singular(self.slot))
        } else if let Some(p) = &picked {
            p.display().to_owned()
        } else {
            "Your look".to_owned()
        };
        let title = c.ellipsize(font::SEMIBOLD, 15.0, &title, sw - 32.0);
        c.text(font::SEMIBOLD, 15.0, &title, x + 16.0, y + 32.0, theme::TEXT_STRONG);
        let bare = nothing.then_some(SLOTS[self.slot]);
        c.push_clip(x + 1.0, y + 56.0, sw - 2.0, h - 56.0 - 96.0);
        self.preview.draw(ui, c, "menu:preview", x, y + 60.0, sw, h - 60.0 - 104.0, &l, who, picked.as_ref(), bare);
        c.pop_clip();
        let hint = "Drag to turn";
        let hw = c.text_width(font::REGULAR, 11.0, hint);
        c.text(font::REGULAR, 11.0, hint, x + (sw - hw) / 2.0, y + h - 92.0, theme::TEXT_MUTED);
        c.fill(x + 16.0, y + h - 70.0, sw - 32.0, 1.0, theme::HAIRLINE);
        let (bx, by, bw, bh) = (x + 16.0, y + h - 54.0, sw - 32.0, 38.0);
        let ready = l.state == State::Ready;
        if l.busy.is_some() {
            c.round(bx, by, bw, bh, 10.0, theme::SUBTLE_HOVER);
            ui.dots(c, bx + bw / 2.0, by + bh / 2.0, theme::TEXT_STRONG);
        } else if !ready || (picked.is_none() && !nothing) {
            disabled(c, bx, by, bw, bh, if ready { "Pick something to try on" } else { "Equip" });
        } else if nothing {
            if let Some(worn) = &worn {
                if ui.button(c, "menu:locker:off", bx, by, bw, bh, &format!("Take off {}", singular(self.slot)), theme::I_BAN, ui::BTN_PRIMARY) {
                    wardrobe::toggle(worn);
                }
            } else {
                disabled(c, bx, by, bw, bh, "Nothing worn");
            }
        } else if let Some(p) = &picked {
            if wardrobe::is_worn(&l, p) {
                if ui.button(c, "menu:locker:off", bx, by, bw, bh, "Take off", theme::I_X, ui::BTN_GLASS) {
                    wardrobe::toggle(p);
                }
            } else if ui.button(c, "menu:locker:on", bx, by, bw, bh, "Equip", theme::I_CHECK, ui::BTN_PRIMARY) {
                wardrobe::toggle(p);
            }
        }

        // The locker.
        let (lx, lw) = (x + sw + 20.0, w - sw - 20.0);
        if !net::signed_in() {
            // Guest, expired or offline: no locker to show (and nothing stale to equip).
            session_lock(ui, c, "menu:locker:lock", lx, y, lw, h);
            return;
        }
        if l.state == State::NoAccount {
            wardrobe::refresh(false); // Signed in again: reload the locker.
        }
        if matches!(l.state, State::Idle | State::Loading | State::NoAccount) {
            ui.dots(c, lx + lw / 2.0, y + h / 2.0, theme::TEXT_SECONDARY);
            if l.state == State::Idle {
                wardrobe::refresh(false);
            }
            return;
        }
        if l.state == State::Error {
            empty(c, lx, y, lw, h - 60.0, theme::I_WIFI_OFF, "Could not load your locker", "Check your connection and try again.");
            if ui.button(c, "menu:wardrobe:retry", lx + lw / 2.0 - 60.0, y + h / 2.0 + 20.0, 120.0, 36.0, "Retry", theme::I_REFRESH, ui::BTN_GLASS) {
                wardrobe::refresh(true);
            }
            return;
        }
        // Slot tabs.
        let (mut tx, mut ty) = (lx, y);
        for i in 0..SLOTS.len() {
            let label = SLOT_NAMES[i].replace("Wings & Backpacks", "Back").replace("Hand Items", "Hand");
            let tw = c.text_width(font::SEMIBOLD, 11.5, &label) + 44.0;
            if tx + tw > lx + lw {
                tx = lx;
                ty += 36.0;
            }
            let id = format!("menu:slot:{i}");
            let over = ui.hover(c, tx, ty, tw, 30.0);
            if ui.clicked(c, &id, tx, ty, tw, 30.0) && self.slot != i {
                self.slot = i;
                self.pick = None;
                self.scroll.reset();
                // Not the page's fade: it would make the 3D model show through itself.
                self.locker_at = ui.now;
            }
            let on = ui.anim_on(&format!("{id}#on"), self.slot == i, 16.0);
            let hv = ui.anim_on(&format!("{id}#h"), over, 14.0);
            c.round(tx, ty, tw, 30.0, 15.0, theme::mix(theme::mix(theme::SUBTLE, theme::SUBTLE_HOVER, hv), 0xFFF4F4F5, on));
            if on < 0.99 {
                c.outline(tx, ty, tw, 30.0, 15.0, 1.0, theme::alpha(theme::HAIRLINE, 1.0 - on));
            }
            let fg = theme::mix(theme::mix(theme::TEXT_SECONDARY, theme::TEXT_STRONG, hv), theme::SOLID_FG, on);
            c.icon(SLOT_ICONS[i], 13.0, tx + 19.0, ty + 15.0, fg);
            let lh = c.line_height(font::SEMIBOLD, 11.5);
            c.text(font::SEMIBOLD, 11.5, &label, tx + 32.0, ty + (30.0 - lh) / 2.0, fg);
            tx += tw + 6.0;
        }
        let items = wardrobe::owned_in(&l, SLOTS[self.slot]);
        let hy = ty + 50.0;
        c.text(font::SEMIBOLD, 10.5, "YOUR LOCKER", lx, hy, theme::TEXT_MUTED);
        c.text(font::SEMIBOLD, 17.0, SLOT_NAMES[self.slot], lx, hy + 16.0, theme::TEXT_STRONG);
        let owned = if items.len() == 1 { "1 owned".to_owned() } else { format!("{} owned", items.len()) };
        let ow = c.text_width(font::MEDIUM, 11.5, &owned);
        c.text(font::MEDIUM, 11.5, &owned, lx + lw - ow, hy + 22.0, theme::TEXT_MUTED);
        let gy = hy + 52.0;
        let gh = y + h - gy - if l.error.is_some() { 26.0 } else { 0.0 };
        let gap = 12.0;
        let cols = (((lw + gap) / (124.0 + gap)) as usize).max(2);
        let tw = (lw - gap * (cols - 1) as f32) / cols as f32;
        let th = tw * 0.8 + 38.0;
        let off = self.scroll.begin(ui, c, lx, gy, lw + 6.0, gh);
        let n = items.len() + 1;
        for i in 0..n {
            let ix = lx + (i % cols) as f32 * (tw + gap);
            let iy = gy + (i / cols) as f32 * (th + gap) - off;
            if iy + th < gy - 4.0 || iy > gy + gh + 4.0 {
                continue;
            }
            let e = ease(clamp01(((ui.now - page_at.max(self.locker_at) - 40.0 - i as f64 * 22.0) / 300.0) as f32));
            c.push_alpha(e);
            let item = if i == 0 { None } else { Some(&items[i - 1]) };
            self.tile(ui, c, &l, item, ix, iy + (1.0 - e) * 10.0, tw, th, worn.as_ref());
            c.pop_alpha();
        }
        self.scroll.end(ui, c, n.div_ceil(cols) as f32 * (th + gap) - gap);
        if items.is_empty() && gh > th + 120.0 {
            let msg = format!("Get {} in the Native Store in the launcher.", SLOT_NAMES[self.slot].to_lowercase());
            c.icon(theme::I_STORE, 14.0, lx + 8.0, gy + th + 30.0, theme::TEXT_MUTED);
            let msg = c.ellipsize(font::REGULAR, 11.5, &msg, lw - 24.0);
            let mh = c.line_height(font::REGULAR, 11.5);
            c.text(font::REGULAR, 11.5, &msg, lx + 22.0, gy + th + 30.0 - mh / 2.0, theme::TEXT_MUTED);
        }
        if let Some(err) = &l.error {
            let err = c.ellipsize(font::MEDIUM, 11.5, err, lw);
            c.text(font::MEDIUM, 11.5, &err, lx, y + h - 16.0, 0xFFFCA5A5);
        }
    }

    /// One locker card; `None` is the "Nothing" card that empties the slot.
    #[allow(clippy::too_many_arguments)]
    fn tile(&mut self, ui: &mut Ui, c: &mut Canvas, l: &wardrobe::Locker, it: Option<&Item>, x: f32, y: f32, w: f32, h: f32, worn: Option<&Item>) {
        let key = it.map_or("", |i| i.id.as_str()).to_owned();
        let id = format!("menu:item:{}:{key}", self.slot);
        let over = ui.hover(c, x, y, w, h);
        if ui.clicked(c, &id, x, y, w, h) {
            self.pick = Some(key.clone());
        }
        let is_worn = it.map_or(worn.is_none(), |i| wardrobe::is_worn(l, i));
        let sel = self.pick.as_deref() == Some(key.as_str());
        let hv = ui.anim_on(&format!("{id}#h"), over, 14.0);
        let sl = ui.anim_on(&format!("{id}#s"), sel, 16.0);
        c.round(x, y, w, h, 12.0, theme::mix(CARD, CARD_HOVER, hv.max(sl * 0.7)));
        c.outline(
            x,
            y,
            w,
            h,
            12.0,
            if sel { 1.5 } else { 1.0 },
            theme::mix(theme::mix(theme::HAIRLINE, theme::BORDER_HOVER, hv), 0xE6FFFFFF, sl),
        );
        let meta = 34.0;
        let (ix, iy, iw, ih) = (x + 8.0, y + 8.0, w - 16.0, h - meta - 12.0);
        c.round(ix, iy, iw, ih, 8.0, 0x40000000);
        match it {
            None => c.icon(theme::I_BAN, 26.0, ix + iw / 2.0, iy + ih / 2.0, theme::mix(theme::TEXT_MUTED, theme::TEXT_SECONDARY, hv)),
            Some(item) if item.is_cape() => {
                let still = item.still_url.as_deref().map(|u| net::image(u, 4 * 1024 * 1024));
                let strip = if item.animated() { item.strip_url.as_deref().map(|u| net::image(u, 4 * 1024 * 1024)) } else { None };
                let (dh, dw) = (ih - 12.0, (ih - 12.0) * 10.0 / 16.0);
                match (strip, still) {
                    (Some(Pic::Ready(strip)), _) => {
                        // An animated cape: the current frame of the strip (frames are stacked top to bottom).
                        let (fr, n) = (item.frame(ui.now / 1000.0) as f32, item.frames as f32);
                        c.image(&strip, ix + (iw - dw) / 2.0, iy + 6.0, dw, dh, [1.0 / 64.0, (fr * 32.0 + 1.0) / (32.0 * n), 11.0 / 64.0, (fr * 32.0 + 17.0) / (32.0 * n)], 0xFFFFFFFF);
                    }
                    (_, Some(Pic::Ready(img))) => {
                        c.image(&img, ix + (iw - dw) / 2.0, iy + 6.0, dw, dh, [1.0 / 64.0, 1.0 / 32.0, 11.0 / 64.0, 17.0 / 32.0], 0xFFFFFFFF);
                    }
                    (_, Some(Pic::Failed)) | (_, None) => {}
                    _ => ui.dots(c, ix + iw / 2.0, iy + ih / 2.0, theme::TEXT_MUTED),
                }
            }
            Some(item) => match item.still_url.as_deref().map(|u| net::image(u, 4 * 1024 * 1024)) {
                Some(Pic::Ready(img)) => {
                    let k = ((iw - 8.0) / img.width as f32).min((ih - 8.0) / img.height as f32);
                    let (dw, dh) = (img.width as f32 * k, img.height as f32 * k);
                    c.image(&img, ix + (iw - dw) / 2.0, iy + (ih - dh) / 2.0, dw, dh, [0.0, 0.0, 1.0, 1.0], 0xFFFFFFFF);
                }
                Some(Pic::Loading) => ui.dots(c, ix + iw / 2.0, iy + ih / 2.0, theme::TEXT_MUTED),
                _ => c.icon(SLOT_ICONS[self.slot], 22.0, ix + iw / 2.0, iy + ih / 2.0, theme::TEXT_MUTED),
            },
        }
        if let Some(item) = it
            && item.partner_name.is_some()
        {
            partner_tag(c, item, ix + 5.0, iy + 5.0, iw - 34.0);
        }
        let on = ui.anim_on(&format!("{id}#on"), is_worn, 14.0);
        if on > 0.01 {
            c.circle(x + w - 17.0, y + 17.0, 9.0, theme::alpha(0xFFF4F4F5, on));
            c.icon(theme::I_CHECK, 11.0, x + w - 17.0, y + 17.0, theme::alpha(theme::SOLID_FG, on));
        }
        // The card's meta line.
        c.fill(x + 1.0, y + h - meta, w - 2.0, 1.0, theme::HAIRLINE);
        let my = y + h - meta / 2.0;
        if it.is_some_and(|i| l.busy.as_deref() == Some(i.id.as_str())) {
            ui.dots(c, x + w / 2.0, my, theme::TEXT_STRONG);
        } else {
            let name = it.map_or("Nothing", Item::display);
            let tag = is_worn.then_some("Worn");
            let tag_w = tag.map_or(0.0, |t| c.text_width(font::MEDIUM, 10.0, t) + 8.0);
            let name = c.ellipsize(font::SEMIBOLD, 11.5, name, w - 22.0 - tag_w);
            let lh = c.line_height(font::SEMIBOLD, 11.5);
            c.text(font::SEMIBOLD, 11.5, &name, x + 11.0, my - lh / 2.0, if is_worn || sel { theme::TEXT_STRONG } else { theme::TEXT });
            if let Some(tag) = tag {
                let th = c.line_height(font::MEDIUM, 10.0);
                c.text(font::MEDIUM, 10.0, tag, x + w - 11.0 - tag_w + 8.0, my - th / 2.0, theme::ACCENT);
            }
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
