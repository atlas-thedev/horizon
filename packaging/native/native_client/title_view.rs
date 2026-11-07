//! `TitleView`: the Native title screen, vanilla's layout over the version
//! artwork with Native's extras in the corners: one centred column
//! (Singleplayer, Multiplayer, Realms, Native Mods), then Options and Quit
//! Game side by side with a small button on each side (Relay on the left, the
//! classic title on the right), the brand top-left, friends top-right, the
//! sponsored card bottom-right, the version bottom-left and Mojang's line
//! bottom-right.

use super::ads::{self, Ad};
use super::avatars;
use super::common::badge;
use super::relay;
use super::ui::{self, Ui};
use crate::native_ui::{Canvas, font, icons, theme};
use std::collections::HashSet;

const AD_ROTATE_MS: f64 = 12_000.0;

/// What a click on the title asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TitleAct {
    Singleplayer,
    Multiplayer,
    Realms,
    Options,
    Quit,
    NativeMods,
    Relay,
    /// The vanilla title screen from now on.
    Classic,
    /// An ad's server.
    Join(String),
}

/// What the title needs to know of the game.
#[allow(dead_code)]
#[derive(Clone, Debug, Default)]
pub struct TitleInfo {
    pub allows_multiplayer: bool,
    pub game_dir: Option<std::path::PathBuf>,
    pub player: String,
}

#[derive(Default)]
pub struct TitleView {
    shown_at: f64,
    hidden_ads: HashSet<String>,
    ad_index: usize,
    ad_switch_at: f64,
    ad_prev: Option<usize>,
    ad_fade_at: f64,
}

fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

/// A launcher button on a dark backing, so its label reads over any artwork.
#[allow(clippy::too_many_arguments)]
fn button(ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, w: f32, h: f32, label: &str, icon: u32) -> bool {
    c.round(x, y, w, h, 10.0f32.min(h / 2.0), 0x8C000000);
    ui.button(c, id, x, y, w, h, label, icon, ui::BTN_GLASS)
}

impl TitleView {
    /// The title came up: the rows rise in again.
    pub fn opened(&mut self, ui: &mut Ui) {
        if self.shown_at == 0.0 {
            self.shown_at = ui.now;
            ui.set_anim("title#in", 0.0);
        }
    }

    /// Rows fade and rise in one after another when the screen first opens.
    fn appear(&self, ui: &Ui, index: usize) -> f32 {
        let a = clamp01(((ui.now - self.shown_at - 80.0 - index as f64 * 45.0) / 380.0) as f32);
        1.0 - (1.0 - a).powi(3)
    }

    #[allow(clippy::too_many_arguments)]
    fn row_button(&self, ui: &mut Ui, c: &mut Canvas, index: usize, id: &str, x: f32, y: f32, w: f32, h: f32, label: &str, enabled: bool) -> bool {
        let e = self.appear(ui, index);
        c.push_alpha(e * if enabled { 1.0 } else { 0.5 });
        let was = ui.interactive;
        ui.interactive = was && enabled;
        let hit = button(ui, c, id, x, y + (1.0 - e) * 10.0, w, h, label, 0);
        ui.interactive = was;
        c.pop_alpha();
        hit && enabled
    }

    /// The shading over the game's own title panorama (vanilla's background shows through; behind Relay and the
    /// menu too when they open from the title), darker towards the top and bottom edges where the text sits.
    pub fn background(&self, c: &mut Canvas) {
        let (w, h) = (c.width(), c.height());
        c.gradient_v(0.0, 0.0, w, 120.0, 0x66000000, 0x00000000);
        c.gradient_v(0.0, h * 0.55, w, h * 0.45, 0x00000000, 0x99000000);
    }

    pub fn draw(&mut self, ui: &mut Ui, c: &mut Canvas, info: &TitleInfo) -> Option<TitleAct> {
        self.opened(ui);
        let (w_all, h_all) = (c.width(), c.height());
        let inn = ui.anim("title#in", 1.0, 4.0);
        self.background(c);
        c.push_alpha(inn);
        let mut act = None;

        // Top-left: the Native brand, small, so the logo spot in the middle stays the game's.
        c.icon(icons::for_module("watermark", u32::MAX), 26.0, 18.0 + 13.0, 16.0 + 13.0, 0xFFFFFFFF);
        c.text(font::BOLD, 16.0, "Native", 52.0, 18.0, theme::TEXT_STRONG);

        // The column, where vanilla puts it (a quarter down plus two rows).
        let (bw, bh, step) = ((w_all - 120.0).min(400.0), 40.0f32, 48.0f32);
        let half = (bw - 8.0) / 2.0;
        let x = (w_all - bw) / 2.0;
        let total = step * 4.0 + 12.0 + bh;
        let mut y = 96.0f32.max((h_all / 4.0 + 96.0).min(h_all - 44.0 - total));
        let mp = info.allows_multiplayer;
        if self.row_button(ui, c, 0, "menu:singleplayer", x, y, bw, bh, &horizon_client::lang::get_or("menu.singleplayer", "Singleplayer"), true) {
            act = Some(TitleAct::Singleplayer);
        }
        y += step;
        if self.row_button(ui, c, 1, "menu:multiplayer", x, y, bw, bh, &horizon_client::lang::get_or("menu.multiplayer", "Multiplayer"), mp) {
            act = Some(TitleAct::Multiplayer);
        }
        y += step;
        if self.row_button(ui, c, 2, "menu:realms", x, y, bw, bh, &horizon_client::lang::get_or("menu.online", "Minecraft Realms"), mp) {
            act = Some(TitleAct::Realms);
        }
        y += step;
        if self.row_button(ui, c, 3, "menu:nativemods", x, y, bw, bh, "Native Mods", true) {
            act = Some(TitleAct::NativeMods);
        }
        y += step + 12.0;

        let appear = self.appear(ui, 4);
        c.push_alpha(appear);
        if button(ui, c, "menu:options", x, y, half, bh, &horizon_client::lang::get_or("menu.options", "Options..."), 0) {
            act = Some(TitleAct::Options);
        }
        // Like vanilla: Quit Game quits, no confirmation.
        if button(ui, c, "menu:quit", x + half + 8.0, y, half, bh, &horizon_client::lang::get_or("menu.quit", "Quit Game"), 0) {
            act = Some(TitleAct::Quit);
        }
        // The small square buttons beside the row, where vanilla has language and accessibility.
        if button(ui, c, "menu:relay", x - bh - 8.0, y, bh, bh, "", theme::I_MESSAGE) {
            act = Some(TitleAct::Relay);
        }
        if ui.hover(c, x - bh - 8.0, y, bh, bh) {
            ui.tip("Relay: friends and chat");
        }
        let unread = relay::unread_total();
        if unread > 0 {
            badge(c, x - 8.0 + 6.0, y + 2.0, unread);
        }
        if button(ui, c, "menu:classic", x + bw + 8.0, y, bh, bh, "", theme::I_LAYERS) {
            act = Some(TitleAct::Classic);
        }
        if ui.hover(c, x + bw + 8.0, y, bh, bh) {
            ui.tip("Classic title screen");
        }
        c.pop_alpha();

        // Top-right: friends online, opens Relay.
        let pill = self.appear(ui, 1);
        c.push_alpha(pill);
        if self.friends_pill(ui, c, w_all - 16.0, 14.0) {
            act = Some(TitleAct::Relay);
        }
        c.pop_alpha();

        // Bottom-right: the sponsored card, when there is room beside the column.
        let ads: Vec<Ad> = ads::list(info.game_dir.clone()).into_iter().filter(|a| !self.hidden_ads.contains(&a.id)).collect();
        let side = (w_all - bw) / 2.0 - bh - 8.0 - 32.0;
        let cw = side.min(300.0);
        if !ads.is_empty() && cw >= 230.0 {
            let ad_h = cw * 5.0 / 12.0 + 62.0;
            let ay = h_all - 36.0 - ad_h;
            if ay >= 64.0 {
                let e = self.appear(ui, 5);
                c.push_alpha(e);
                if let Some(a) = self.ad_card(ui, c, &ads, w_all - 16.0 - cw + (1.0 - e) * 20.0, ay, cw) {
                    act = Some(a);
                }
                c.pop_alpha();
            }
        }

        // The footer, as vanilla: the version bottom-left, Mojang bottom-right.
        let fy = h_all - 22.0;
        let version = format!("Minecraft {}  \u{b7}  Native Client", horizon_core::VANILLA_VERSION);
        c.text(font::MEDIUM, 11.0, &version, 16.0, fy, 0xB3FFFFFF);
        let copy = "Copyright Mojang AB. Do not distribute!";
        let cwid = c.text_width(font::MEDIUM, 11.0, copy);
        c.text(font::MEDIUM, 11.0, copy, w_all - 16.0 - cwid, fy, 0xB3FFFFFF);
        c.pop_alpha();
        act
    }

    /// "3 friends online" with their faces; true when clicked. Right-aligned to `right`.
    fn friends_pill(&mut self, ui: &mut Ui, c: &mut Canvas, right: f32, y: f32) -> bool {
        if !super::net::signed_in() {
            return false;
        }
        let snap = relay::snapshot();
        let online: Vec<&relay::Friend> = snap.friends.iter().filter(|f| f.online).collect();
        let label = if !snap.friends_loaded {
            "Friends".to_owned()
        } else if online.is_empty() {
            "No friends online".to_owned()
        } else {
            format!("{} {}", online.len(), if online.len() == 1 { "friend online" } else { "friends online" })
        };
        let faces = online.len().min(3);
        let (h, av) = (34.0f32, 22.0f32);
        let tw = c.text_width(font::SEMIBOLD, 12.5, &label);
        let w = 14.0 + if faces > 0 { av + (faces - 1) as f32 * 14.0 + 8.0 } else { 22.0 } + tw + 14.0;
        let x = right - w;
        let over = ui.hover(c, x, y, w, h);
        let hv = ui.anim_on("title:friends#h", over, 14.0);
        c.round(x, y, w, h, h / 2.0, theme::mix(0xB308090C, 0xE00E1014, hv));
        c.outline(x, y, w, h, h / 2.0, 1.0, theme::mix(theme::HAIRLINE, theme::BORDER_HOVER, hv));
        let mut cx = x + 12.0;
        if faces > 0 {
            for i in (0..faces).rev() {
                let fx = cx + i as f32 * 14.0;
                c.round(fx - 2.0, y + (h - av) / 2.0 - 2.0, av + 4.0, av + 4.0, 8.0, 0xFF08090C);
                avatars::draw(c, online[i].skin.as_deref(), fx, y + (h - av) / 2.0, av, 6.0);
            }
            cx += av + (faces - 1) as f32 * 14.0 + 8.0;
        } else {
            c.icon(theme::I_USERS, 14.0, cx + 8.0, y + h / 2.0, theme::TEXT_SECONDARY);
            cx += 22.0;
        }
        let lh = c.line_height(font::SEMIBOLD, 12.5);
        c.text(font::SEMIBOLD, 12.5, &label, cx, y + (h - lh) / 2.0, theme::TEXT_STRONG);
        if !online.is_empty() {
            c.circle(x + w - 9.0, y + 9.0, 3.5, theme::ONLINE);
        }
        if over {
            ui.cursor_hand = true;
        }
        ui.clicked(c, "title:friends", x, y, w, h)
    }

    fn show_ad(&mut self, index: usize, now: f64) {
        self.ad_prev = Some(self.ad_index);
        self.ad_index = index;
        self.ad_fade_at = now;
        self.ad_switch_at = now + AD_ROTATE_MS;
    }

    /// A sponsored card: banner, text, buttons, dots; it rotates every 12 s unless hovered.
    #[allow(clippy::too_many_arguments)]
    fn ad_card(&mut self, ui: &mut Ui, c: &mut Canvas, ads: &[Ad], x: f32, y: f32, w: f32) -> Option<TitleAct> {
        let count = ads.len();
        let (mh, h) = (w * 5.0 / 12.0, w * 5.0 / 12.0 + 62.0);
        let over = ui.hover(c, x, y, w, h);
        if self.ad_switch_at == 0.0 || over {
            self.ad_switch_at = ui.now + AD_ROTATE_MS;
        } else if ui.now >= self.ad_switch_at && count > 1 {
            self.show_ad((self.ad_index + 1) % count, ui.now);
        }
        let ad = &ads[self.ad_index % count];
        // Crossfade from the previous banner instead of a hard cut.
        let mut fade = if self.ad_prev.is_none() { 1.0 } else { clamp01(((ui.now - self.ad_fade_at) / 420.0) as f32) };
        fade = fade * fade * (3.0 - 2.0 * fade);
        c.shadow(x, y + 8.0, w, h, 16.0, 28.0, 0x80000000);
        c.round(x, y, w, h, 16.0, 0xE608090C);
        if let Some(prev) = self.ad_prev
            && fade < 1.0
            && prev < count
            && prev != self.ad_index % count
        {
            c.image_cover(&ads[prev].image, x, y, w, mh, 1.0, 0.0, 0.0, 0xFFFFFFFF);
        }
        c.image_cover(&ad.image, x, y, w, mh, 1.0 + 0.03 * (1.0 - fade), 0.0, 0.0, theme::alpha(0xFFFFFFFF, fade));
        c.outline(x, y, w, h, 16.0, 1.0, theme::HAIRLINE);

        // Close: hide this ad for the rest of the session.
        let (cs, cxx, cyy) = (22.0f32, x + w - 22.0 - 8.0, y + 8.0);
        let over_close = ui.hover(c, cxx, cyy, cs, cs);
        let hc = ui.anim_on("ad#close", over_close, 14.0);
        c.circle(cxx + cs / 2.0, cyy + cs / 2.0, cs / 2.0, theme::alpha(0xFF000000, 0.5 + 0.25 * hc));
        c.icon(theme::I_X, 11.0, cxx + cs / 2.0, cyy + cs / 2.0, 0xFFFFFFFF);
        if ui.clicked(c, "ad#close", cxx, cyy, cs, cs) {
            self.hidden_ads.insert(ad.id.clone());
            self.ad_index = 0;
            self.ad_prev = None;
            return None;
        }
        c.push_alpha(0.35 + 0.65 * fade);
        let mut act = None;
        // Buttons (the right side of the footer).
        let (pad, fy, fh) = (14.0f32, y + mh, h - mh);
        let n = ad.buttons.len().min(2);
        let (bh, gap) = (if n > 1 { 22.0f32 } else { 28.0f32 }, 5.0f32);
        let by = fy + (fh - (bh * n as f32 + gap * (n as f32 - 1.0).max(0.0))) / 2.0;
        let mut buttons_w = 0.0f32;
        for b in ad.buttons.iter().take(n) {
            buttons_w = buttons_w.max(c.text_width(font::BOLD, 12.0, &b.label) + 34.0);
        }
        buttons_w = buttons_w.min(w * 0.42);
        let mut button_hit = false;
        for (i, b) in ad.buttons.iter().take(n).enumerate() {
            let (xx, yy) = (x + w - pad - buttons_w, by + i as f32 * (bh + gap));
            let id = format!("ad#b{i}");
            let ob = ui.hover(c, xx, yy, buttons_w, bh);
            let hb = ui.anim_on(&id, ob, 14.0);
            let bg = if i == 0 { theme::mix(0xFFFFFFFF, 0xFFE8E9EF, hb) } else { theme::alpha(0xFFFFFFFF, 0.10 + 0.08 * hb) };
            let fg = if i == 0 { 0xFF0D0E12 } else { 0xFFFFFFFF };
            c.round(xx, yy, buttons_w, bh, bh / 2.0, bg);
            let icon = if b.server() { theme::I_PLAY } else { theme::I_CHEVRON_RIGHT };
            let tw = c.text_width(font::BOLD, 12.0, &b.label);
            let tx = xx + (buttons_w - tw - 12.0 - 5.0) / 2.0;
            let label = c.ellipsize(font::BOLD, 12.0, &b.label, buttons_w - 30.0);
            let lh = c.line_height(font::BOLD, 12.0);
            c.text(font::BOLD, 12.0, &label, tx, yy + (bh - lh) / 2.0, fg);
            c.icon(icon, 11.0, tx + tw + 5.0 + 6.0, yy + bh / 2.0, fg);
            if ob {
                button_hit = true;
                ui.cursor_hand = true;
            }
            if ui.clicked(c, &id, xx, yy, buttons_w, bh) {
                act = run(b);
            }
        }
        // Text.
        let tx = x + pad;
        let max_w = w - pad * 2.0 - if n > 0 { buttons_w + 10.0 } else { 0.0 };
        let tag = if ad.tag.is_empty() { "AD".to_owned() } else { format!("AD \u{b7} {}", ad.tag.to_uppercase()) };
        let lines = if ad.body.is_empty() { 2 } else { 3 };
        let ty = fy + (fh - if lines == 3 { 46.0 } else { 30.0 }) / 2.0;
        let tag = c.ellipsize(font::BOLD, 9.5, &tag, max_w);
        c.text(font::BOLD, 9.5, &tag, tx, ty, theme::TEXT_MUTED);
        let title = c.ellipsize(font::SEMIBOLD, 13.5, &ad.title, max_w);
        c.text(font::SEMIBOLD, 13.5, &title, tx, ty + 13.0, theme::TEXT_STRONG);
        if !ad.body.is_empty() {
            let body = c.ellipsize(font::REGULAR, 11.0, &ad.body, max_w);
            c.text(font::REGULAR, 11.0, &body, tx, ty + 32.0, theme::TEXT_SECONDARY);
        }
        c.pop_alpha();
        // The banner / card does what the main button does.
        if !button_hit && !over_close && ui.clicked(c, "ad#card", x, y, w, h) {
            act = ad.primary().as_ref().and_then(run);
        }
        // Dots.
        if count > 1 {
            let (dw, dg) = (6.0f32, 5.0f32);
            let total = count as f32 * dw + (count - 1) as f32 * dg + 6.0;
            let (mut dx, dy) = (x + (w - total) / 2.0, y + mh - 14.0);
            c.round(dx - 4.0, dy - 4.0, total + 8.0, dw + 8.0, (dw + 8.0) / 2.0, 0x66000000);
            for i in 0..count {
                let active = i == self.ad_index % count;
                let ww = if active { dw + 6.0 } else { dw };
                c.round(dx, dy, ww, dw, dw / 2.0, if active { 0xFFFFFFFF } else { 0x80FFFFFF });
                if ui.clicked(c, &format!("ad#dot{i}"), dx - 2.0, dy - 4.0, ww + 4.0, dw + 8.0) && !active {
                    self.show_ad(i, ui.now);
                }
                dx += ww + dg;
            }
        }
        act
    }
}

/// An ad's button: join its server, or open its page.
fn run(b: &ads::Button) -> Option<TitleAct> {
    if b.server() {
        return Some(TitleAct::Join(b.value.clone()));
    }
    super::common::open_url(&b.value);
    None
}
