//! `TitleView`: the Native title screen, a modern take (the same as the Java
//! mod's): the menu down the left (Play first, in the launcher's blue), you
//! standing in a whole block world (`title_world`) in the middle with your nametag
//! over your head, the quest and the sponsored card on the right when there is
//! room. Friends top-right, the announcement
//! top-centre, the version and Mojang's line along the bottom.

use super::ads::{self, Ad};
use super::avatars;
use super::common::badge;
use super::relay;
use super::ui::{self, Ui};
use crate::native_ui::{Canvas, font, icons, theme};
use std::collections::HashSet;

const AD_ROTATE_MS: f64 = 12_000.0;

/// What a click on the title asks for.
#[derive(Clone, PartialEq, Eq)]
pub enum TitleAct {
    Singleplayer,
    Multiplayer,
    Realms,
    Options,
    Quit,
    SwitchAccount(super::title_accounts::SessionGrant),
    NativeMods,
    Relay,
    /// The game's Language screen (vanilla's globe button).
    Language,
    /// The game's Accessibility Settings (vanilla's person button).
    Accessibility,
    /// The vanilla title screen from now on.
    Classic,
    /// Switch between "modern" (3D world + player) and "simple" (v2.5.75 centered) title layouts.
    SetStyle(String),
    /// An ad's server.
    Join(String),
    /// One of the game's multiplayer list buttons (`multiplayer::Screen::click`), with the saved server selected.
    ServerButton { id: u8, index: Option<usize> },
    /// Move a saved server up or down the list.
    ServerMove { index: usize, up: bool },
    /// The game's own multiplayer screen, until the title shows again.
    ServersClassic,
    /// A Native dialog's answer for the game's Add / Edit server screen (`done` false: cancelled).
    ServerForm { name: String, address: String, packs: Option<bool>, done: bool },
    /// The Direct Connection dialog's answer (`join` false: cancelled).
    ServerDirect { address: String, join: bool },
    /// The Delete dialog's answer.
    ServerDelete { yes: bool },
}

/// What the title needs to know of the game.
#[allow(dead_code)]
#[derive(Clone, Debug, Default)]
pub struct TitleInfo {
    pub allows_multiplayer: bool,
    pub game_dir: Option<std::path::PathBuf>,
    pub player: String,
    pub simple_style: bool,
}

/// The preview tilts down onto you as much as the world's camera does, so you stand in it.
const PITCH: f32 = super::title_world::PITCH as f32;

#[derive(Default)]
pub struct TitleView {
    accounts: super::title_accounts::TitleAccounts,
    style_menu_open: bool,
    style_menu_motion: f32,
    style_chip_rect: [f64; 4],
    classic_quest_rect: [f64; 4],
    classic_ad_rect: [f64; 4],
    /// You, standing in the world (`title_world`).
    player: super::preview3d::PlayerPreview,
    /// The banner text closed with its X (this session), the quest card's quest and the picked reward.
    hidden_banner: Option<String>,
    quest_index: usize,
    picked: Option<(String, usize)>,
    promo_art: Option<(u64, u32, u32, std::sync::Arc<crate::native_ui::Image>)>,
    shown_at: f64,
    hidden_ads: HashSet<String>,
    ad_index: usize,
    ad_switch_at: f64,
    ad_prev: Option<usize>,
    ad_fade_at: f64,
}

fn style_label(mode: &str) -> &'static str {
    match mode {
        "simple" => "Simple",
        "vanilla" => "Vanilla",
        _ => "Modern",
    }
}

fn style_icon(mode: &str) -> u32 {
    match mode {
        "simple" => theme::I_LAYERS,
        "vanilla" => theme::I_GAMEPAD,
        _ => theme::I_LAYOUT,
    }
}

fn style_chip_width(c: &mut Canvas, mode: &str) -> f32 {
    (c.text_width(font::SEMIBOLD, 12.0, style_label(mode)) + 50.0).max(96.0)
}

fn in_rect(x: f64, y: f64, r: [f64; 4]) -> bool {
    r[2] > 0.0 && r[3] > 0.0 && x >= r[0] && x <= r[0] + r[2] && y >= r[1] && y <= r[1] + r[3]
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
    pub(super) fn account_applied(&mut self, id: String) {
        self.accounts.applied(id);
    }

    pub(super) fn account_failed(&mut self) {
        self.accounts.apply_failed();
    }

    pub fn accounts_open(&self) -> bool {
        self.accounts.is_open() || self.style_menu_open
    }

    pub fn escape_style_menu(&mut self) -> bool {
        if self.style_menu_open {
            self.style_menu_open = false;
            return true;
        }
        false
    }

    /// True when a pointer event at physical window coordinates belongs to Native's Vanilla title overlay.
    pub fn wants_classic_pointer(&self, x: f64, y: f64) -> bool {
        if in_rect(x, y, self.style_chip_rect)
            || in_rect(x, y, self.classic_quest_rect)
            || in_rect(x, y, self.classic_ad_rect)
        {
            return true;
        }
        self.style_menu_open
    }

    /// Draws the style dropdown, quests, and ads cleanly over Vanilla's title screen.
    pub fn draw_classic_style_overlay(
        &mut self,
        ui: &mut Ui,
        c: &mut Canvas,
        game_dir: Option<std::path::PathBuf>,
    ) -> Option<TitleAct> {
        let (w, h) = (c.width(), c.height());
        self.classic_quest_rect = [0.0; 4];
        self.classic_ad_rect = [0.0; 4];
        let mut act = None;
        let home = super::home::get();
        let ads: Vec<Ad> = ads::list(game_dir).into_iter().filter(|a| !self.hidden_ads.contains(&a.id)).collect();
        let quests = home.as_ref().is_some_and(|d| !d.quests.is_empty());
        let card_w = if w >= 860.0 { ((w - 440.0) / 2.0 - 24.0).clamp(0.0, 280.0) } else { 0.0 };
        let interactive = ui.interactive;
        if self.style_menu_open {
            ui.interactive = false;
        }
        if card_w >= 195.0 && (quests || !ads.is_empty()) {
            let cy = h - 34.0;
            if let Some(d) = home.as_ref().filter(|_| quests) {
                let qh = quest_height(card_w);
                let qy = cy - qh;
                if qy >= 64.0 {
                    let sc = f64::from(c.scale);
                    self.classic_quest_rect = [16.0 * sc, f64::from(qy) * sc, f64::from(card_w) * sc, f64::from(qh) * sc];
                    if let Some(a) = self.quest_card(ui, c, d, 16.0, qy, card_w, qh) {
                        act = Some(a);
                    }
                }
            }
            if !ads.is_empty() {
                let ad_h = card_w * 5.0 / 12.0 + 62.0;
                let ay = cy - ad_h;
                let ax = w - 16.0 - card_w;
                if ay >= 64.0 {
                    let sc = f64::from(c.scale);
                    self.classic_ad_rect = [f64::from(ax) * sc, f64::from(ay) * sc, f64::from(card_w) * sc, f64::from(ad_h) * sc];
                    if let Some(a) = self.ad_card(ui, c, &ads, ax, ay, card_w) {
                        act = Some(a);
                    }
                }
            }
        }
        ui.interactive = interactive;
        if let Some(style_act) = self.draw_style_dropdown(ui, c, w - 16.0, 16.0, "vanilla", 1.0) {
            act = Some(style_act);
        }
        act
    }

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

    /// The background, submitted before all foreground UI: you in a whole block world (`title_world`: one still
    /// picture of the map, made once and kept in the game folder) over the game's panorama, and its shading. The new title (`main`)
    /// keeps the world clear where you stand, darker behind the menu, along the top and along the bottom (under the
    /// cards and the footer).
    pub fn background(&self, c: &mut Canvas, main: bool, simple: bool) {
        let (w, h) = (c.width(), c.height());
        if simple {
            c.fill(0.0, 0.0, w, h, 0x33000000);
            c.gradient_v(0.0, 0.0, w, 120.0, 0x7A000000, 0x00000000);
            c.gradient_v(0.0, h * 0.55, w, h * 0.45 + 1.0, 0x00000000, 0xA6000000);
            return;
        }
        super::title_world::draw(c, 0.0, 0.0, w, h);
        if main {
            c.fill(0.0, 0.0, w, h, 0x1A000000);
            c.gradient_h(0.0, 0.0, (w * 0.55).min(680.0), h, 0xB3000000, 0x00000000);
            c.gradient_v(0.0, 0.0, w, 110.0, 0x59000000, 0x00000000);
            c.gradient_v(0.0, h * 0.7, w, h * 0.3 + 1.0, 0x00000000, 0xB3000000);
            return;
        }
        c.fill(0.0, 0.0, w, h, 0x45000000);
        c.gradient_v(0.0, 0.0, w, 120.0, 0x7A000000, 0x00000000);
        c.gradient_v(0.0, h * 0.55, w, h * 0.45, 0x00000000, 0xA6000000);
    }

    pub fn draw(&mut self, ui: &mut Ui, c: &mut Canvas, info: &TitleInfo) -> Option<TitleAct> {
        self.opened(ui);
        super::wardrobe::sync_session();
        let (w_all, h_all) = (c.width(), c.height());
        let simple = info.simple_style;
        if self.accounts.is_open() && self.style_menu_open {
            self.style_menu_open = false;
        }
        if ui.interactive && self.style_menu_open && ui.key(ui::KEY_ESCAPE) {
            self.style_menu_open = false;
        }
        let overlay_open = self.accounts.is_open() || self.style_menu_open;
        let interactive = ui.interactive;
        if overlay_open {
            ui.interactive = false;
        }
        let inn = ui.anim("title#in", 1.0, 4.0);
        c.push_alpha(inn);
        let mut act = None;
        let home = super::home::get();
        let promo = home.as_ref().and_then(|d| {
            d.promos
                .iter()
                .find(|p| !(d.discord_linked && p.buttons.first().is_some_and(|b| b.action == "discord-link")))
                .cloned()
        });
        let ads: Vec<Ad> = ads::list(info.game_dir.clone()).into_iter().filter(|a| !self.hidden_ads.contains(&a.id)).collect();
        let quests = home.as_ref().is_some_and(|d| !d.quests.is_empty());
        let pad = if simple { 16.0 } else if w_all < 1000.0 { 24.0 } else { 44.0 };
        let pill = self.appear(ui, 1);

        if simple {
            if let Some(a) = self.draw_simple_layout(ui, c, info, home.as_ref(), promo.as_ref(), &ads, w_all, h_all, pill) {
                act = Some(a);
            }
        } else {
            // Three columns: the menu, you, the cards (only when there is room for all three).
            let pw = (w_all * 0.26).clamp(270.0, 340.0);
            let cards_w = (w_all * 0.23).min(300.0);
            let cards = (quests || !ads.is_empty()) && cards_w >= 205.0 && w_all - pad * 2.0 - pw - cards_w >= 300.0;
            let stage_x = pad + pw + 12.0;
            let stage_r = if cards { w_all - pad - cards_w - 12.0 } else { w_all - pad };
            let stage_w = stage_r - stage_x;

            // You, standing on the world's ground (it was drawn for your feet to land here).
            if stage_w >= 200.0 {
                let s = (super::title_world::player_height(h_all) / 32.0).min(stage_w / 66.0);
                let feet = h_all * super::title_world::FEET;
                let (bw, bh) = (30.0 * s, 50.0 * s); // the preview frames the model in 30 x 50 model pixels
                let cx = stage_x + stage_w / 2.0;
                let top = feet - 19.0 * s - bh / 2.0;
                let e = self.appear(ui, 3);
                c.push_alpha(e);
                self.player.set_pitch(PITCH);
                self.player.nametag = Some(info.player.clone()).filter(|n| !n.is_empty());
                let locker = super::wardrobe::snapshot();
                self.player.draw(ui, c, "title:you", cx - bw / 2.0, top + (1.0 - e) * 18.0, bw, bh, &locker, &info.player, None, None);
                c.pop_alpha();
            }

            // The menu.
            if let Some(a) = self.menu(ui, c, info, home.as_ref(), promo.as_ref(), pad, pw, h_all) {
                act = Some(a);
            }

            // Top-right: friends online (opens Relay).
            c.push_alpha(pill);
            if self.friends_pill(ui, c, w_all - 16.0, 61.0) {
                act = Some(TitleAct::Relay);
            }
            c.pop_alpha();
            // Top-centre: the sale (with its countdown) or the announcement.
            if let Some(d) = home.as_ref()
                && let Some(b) = d.banner.as_ref()
                && self.hidden_banner.as_deref() != Some(b.text.as_str())
            {
                c.push_alpha(self.appear(ui, 0));
                self.banner(ui, c, d, b, w_all, 14.0);
                c.pop_alpha();
            }
            // Right: the sponsored card at the bottom, the quest card over it (if tight vertically, keep quests visible).
            if cards {
                let card_x = w_all - pad - cards_w;
                let mut cy = h_all - 40.0;
                let e = self.appear(ui, 5);
                c.push_alpha(e);
                let ad_h = if ads.is_empty() { 0.0 } else { cards_w * 5.0 / 12.0 + 62.0 };
                let qh = if quests { quest_height(cards_w) } else { 0.0 };
                let show_ad = !ads.is_empty() && (!quests || cy - ad_h - 12.0 - qh >= 64.0) && cy - ad_h >= 64.0;
                if show_ad {
                    cy -= ad_h;
                    if let Some(a) = self.ad_card(ui, c, &ads, card_x + (1.0 - e) * 20.0, cy, cards_w) {
                        act = Some(a);
                    }
                    cy -= 12.0;
                }
                if let Some(d) = home.as_ref().filter(|_| quests)
                    && cy - qh >= 64.0
                    && let Some(a) = self.quest_card(ui, c, d, card_x + (1.0 - e) * 20.0, cy - qh, cards_w, qh)
                {
                    act = Some(a);
                }
                c.pop_alpha();
            }

            // The footer, as vanilla: the version bottom-left, Mojang bottom-right.
            let fy = h_all - 22.0;
            let version = format!("Minecraft {}  \u{b7}  Native Client", horizon_core::VANILLA_VERSION);
            c.text(font::MEDIUM, 11.0, &version, pad, fy, 0x99FFFFFF);
            let copy = "Copyright Mojang AB. Do not distribute!";
            let cwid = c.text_width(font::MEDIUM, 11.0, copy);
            c.text(font::MEDIUM, 11.0, copy, w_all - pad - cwid, fy, 0x99FFFFFF);
        }
        c.pop_alpha();

        // Top-right Native bar: Style dropdown pinned at exact same spot (Modern / Simple / Vanilla) + Account switcher beside it
        let mode = if simple { "simple" } else { "modern" };
        let right_edge = w_all - 16.0;
        let style_w = style_chip_width(c, mode);
        ui.interactive = interactive && !self.accounts.is_open();
        if let Some(style_act) = self.draw_style_dropdown(ui, c, right_edge, 16.0, mode, inn * pill) {
            act = Some(style_act);
        }
        ui.interactive = interactive && !self.style_menu_open;
        if let Some(grant) = self.accounts.draw(ui, c, right_edge - style_w - 8.0, 16.0, &info.player, inn * pill) {
            ui.interactive = interactive;
            return Some(TitleAct::SwitchAccount(grant));
        }
        ui.interactive = interactive;
        act
    }

    /// v2.5.75 Simple Native title layout: centered vanilla-style column over the game's panorama & logo, extras in corners.
    #[allow(clippy::too_many_arguments)]
    fn draw_simple_layout(
        &mut self,
        ui: &mut Ui,
        c: &mut Canvas,
        info: &TitleInfo,
        home: Option<&super::home::Doc>,
        promo: Option<&super::home::Promo>,
        ads: &[Ad],
        w_all: f32,
        h_all: f32,
        pill: f32,
    ) -> Option<TitleAct> {
        let mut act = None;
        let tr = |key: &str, fallback: &str| horizon_client::lang::get_or(key, fallback);
        let mp = info.allows_multiplayer;

        // Top-left: compact Native brand
        c.icon(icons::for_module("watermark", u32::MAX), 24.0, 31.0, 29.0, 0xFFFFFFFF);
        c.text(font::BOLD, 16.0, "Native", 52.0, 18.0, theme::TEXT_STRONG);

        // Centered column (v2.5.75 proportions)
        let (bw, bh, step) = (400.0f32.min(w_all - 120.0), 40.0f32, 48.0f32);
        let half = (bw - 8.0) / 2.0;
        let x = (w_all - bw) / 2.0;
        let total = step * 4.0 + 12.0 + bh + if promo.is_some() { PROMO_H + 12.0 } else { 0.0 };
        let mut y = 96.0f32.max((h_all / 4.0 + 96.0).min(h_all - 44.0 - total));

        if self.simple_row_button(ui, c, 0, "menu:singleplayer", x, y, bw, bh, &tr("menu.singleplayer", "Singleplayer"), true) {
            act = Some(TitleAct::Singleplayer);
        }
        y += step;
        if self.simple_row_button(ui, c, 1, "menu:multiplayer", x, y, bw, bh, &tr("menu.multiplayer", "Multiplayer"), mp) {
            act = Some(TitleAct::Multiplayer);
        }
        y += step;
        if self.simple_row_button(ui, c, 2, "menu:realms", x, y, bw, bh, &tr("menu.online", "Minecraft Realms"), mp) {
            act = Some(TitleAct::Realms);
        }
        y += step;
        if self.simple_row_button(ui, c, 3, "menu:nativemods", x, y, bw, bh, "Native Mods", true) {
            act = Some(TitleAct::NativeMods);
        }
        y += step + 12.0;

        let e = self.appear(ui, 4);
        c.push_alpha(e);
        if button(ui, c, "menu:options", x, y, half, bh, &tr("menu.options", "Options..."), 0) {
            act = Some(TitleAct::Options);
        }
        if button(ui, c, "menu:quit", x + half + 8.0, y, half, bh, &tr("menu.quit", "Quit Game"), 0) {
            act = Some(TitleAct::Quit);
        }
        if button(ui, c, "menu:relay", x - bh - 8.0, y, bh, bh, "", theme::I_MESSAGE) {
            act = Some(TitleAct::Relay);
        }
        if ui.hover(c, x - bh - 8.0, y, bh, bh) {
            ui.tip("Relay: friends and chat");
        }
        let unread = relay::unread_total();
        if unread > 0 {
            badge(c, x - 2.0, y + 2.0, unread);
        }
        if button(ui, c, "menu:language", x - (bh + 8.0) * 2.0, y, bh, bh, "", theme::I_GLOBE) {
            act = Some(TitleAct::Language);
        }
        if ui.hover(c, x - (bh + 8.0) * 2.0, y, bh, bh) {
            ui.tip(&tr("options.language", "Language..."));
        }
        if button(ui, c, "menu:classic", x + bw + 8.0, y, bh, bh, "", theme::I_LAYERS) {
            act = Some(TitleAct::Classic);
        }
        if ui.hover(c, x + bw + 8.0, y, bh, bh) {
            ui.tip("Vanilla title screen");
        }
        if button(ui, c, "menu:accessibility", x + bw + 8.0 + bh + 8.0, y, bh, bh, "", theme::I_PERSON) {
            act = Some(TitleAct::Accessibility);
        }
        if ui.hover(c, x + bw + 8.0 + bh + 8.0, y, bh, bh) {
            ui.tip(&tr("options.accessibility", "Accessibility Settings..."));
        }
        if let (Some(p), Some(d)) = (promo, home)
            && let Some(a) = self.promo_card(ui, c, p, d, x, y + bh + 12.0, bw, PROMO_H)
        {
            act = Some(a);
        }
        c.pop_alpha();

        // Top-right under the Native bar: friends online
        c.push_alpha(pill);
        if self.friends_pill(ui, c, w_all - 16.0, 61.0) {
            act = Some(TitleAct::Relay);
        }
        c.pop_alpha();
        if let Some(d) = home
            && let Some(b) = d.banner.as_ref()
            && self.hidden_banner.as_deref() != Some(b.text.as_str())
        {
            c.push_alpha(self.appear(ui, 0));
            self.banner(ui, c, d, b, w_all, 14.0);
            c.pop_alpha();
        }
        // Bottom-left: quest card when there is room
        let qside = (w_all - bw) / 2.0 - bh - 8.0 - 32.0;
        let qw = 300.0f32.min(qside);
        if let Some(d) = home
            && !d.quests.is_empty()
            && qw >= 200.0
        {
            let qh = quest_height(qw);
            let qy = h_all - 36.0 - qh;
            if qy >= 64.0 {
                let e = self.appear(ui, 5);
                c.push_alpha(e);
                if let Some(a) = self.quest_card(ui, c, d, 16.0 - (1.0 - e) * 20.0, qy, qw, qh) {
                    act = Some(a);
                }
                c.pop_alpha();
            }
        }
        // Bottom-right: ad card when there is room
        let cw = 300.0f32.min(qside);
        if !ads.is_empty() && cw >= 230.0 {
            let ad_h = cw * 5.0 / 12.0 + 62.0;
            let ay = h_all - 36.0 - ad_h;
            if ay >= 64.0 {
                let e = self.appear(ui, 5);
                c.push_alpha(e);
                if let Some(a) = self.ad_card(ui, c, ads, w_all - 16.0 - cw + (1.0 - e) * 20.0, ay, cw) {
                    act = Some(a);
                }
                c.pop_alpha();
            }
        }
        let fy = h_all - 22.0;
        let version = format!("Minecraft {}  \u{b7}  Native Client", horizon_core::VANILLA_VERSION);
        c.text(font::MEDIUM, 11.0, &version, 16.0, fy, 0xB3FFFFFF);
        let copy = "Copyright Mojang AB. Do not distribute!";
        let cwid = c.text_width(font::MEDIUM, 11.0, copy);
        c.text(font::MEDIUM, 11.0, copy, w_all - 16.0 - cwid, fy, 0xB3FFFFFF);
        act
    }

    #[allow(clippy::too_many_arguments)]
    fn simple_row_button(&self, ui: &mut Ui, c: &mut Canvas, index: usize, id: &str, x: f32, y: f32, w: f32, h: f32, label: &str, enabled: bool) -> bool {
        let e = self.appear(ui, index);
        c.push_alpha(e * if enabled { 1.0 } else { 0.5 });
        let was = ui.interactive;
        ui.interactive = was && enabled;
        let hit = button(ui, c, id, x, y + (1.0 - e) * 10.0, w, h, label, 0) && enabled;
        ui.interactive = was;
        c.pop_alpha();
        hit
    }

    /// Minimal Native dropdown in the same top-right spot across Modern, Simple, and Vanilla title screens.
    fn draw_style_dropdown(&mut self, ui: &mut Ui, c: &mut Canvas, right: f32, top: f32, mode: &str, entrance: f32) -> Option<TitleAct> {
        if self.style_menu_open && ui.keys.iter().any(|(k, _)| *k == ui::KEY_ESCAPE) {
            self.style_menu_open = false;
        }
        let chip_h = 36.0;
        let label = style_label(mode);
        let chip_w = style_chip_width(c, mode);
        let x = right - chip_w;
        let sc = f64::from(c.scale);
        self.style_chip_rect = [f64::from(x) * sc, f64::from(top) * sc, f64::from(chip_w) * sc, f64::from(chip_h) * sc];

        let over_chip = ui.hover(c, x, top, chip_w, chip_h);
        if over_chip {
            ui.cursor_hand = true;
        }
        let hv = ui.anim_on("title:style#h", over_chip || self.style_menu_open, 16.0);
        c.push_alpha(entrance);
        c.round(x, top, chip_w, chip_h, 10.0, theme::mix(0xE2090A0C, 0xF514161B, hv));
        c.outline(x, top, chip_w, chip_h, 10.0, 1.0, theme::mix(0x1EFFFFFF, 0x36FFFFFF, hv));
        c.icon(style_icon(mode), 13.5, x + 16.0, top + chip_h / 2.0, theme::TEXT_STRONG);
        let lh = c.line_height(font::SEMIBOLD, 12.0);
        c.text(font::SEMIBOLD, 12.0, label, x + 29.0, top + (chip_h - lh) / 2.0, theme::TEXT_STRONG);
        c.icon(if self.style_menu_open { theme::I_CHEVRON_UP } else { theme::I_CHEVRON_DOWN }, 11.5, x + chip_w - 13.0, top + chip_h / 2.0, theme::TEXT_SECONDARY);
        if ui.clicked(c, "title:style:chip", x, top, chip_w, chip_h) {
            self.style_menu_open = !self.style_menu_open;
        }
        c.pop_alpha();

        let target = if self.style_menu_open { 1.0 } else { 0.0 };
        self.style_menu_motion += (target - self.style_menu_motion) * (1.0 - (-24.0 * ui.dt).exp());
        if (self.style_menu_motion - target).abs() < 0.01 {
            self.style_menu_motion = target;
        }
        if self.style_menu_motion <= 0.0 {
            return None;
        }

        let w = 148.0;
        let dx = (right - w).max(8.0);
        let dy = top + chip_h + 6.0 - 6.0 * (1.0 - self.style_menu_motion);
        let row_h = 32.0;
        let dh = 12.0 + row_h * 3.0;
        if ui.interactive && self.style_menu_open && ui.pressed && !over_chip && !ui.hover(c, dx, dy, w, dh) {
            self.style_menu_open = false;
            return None;
        }

        let mut act = None;
        c.push_alpha(self.style_menu_motion);
        c.shadow(dx, dy + 3.0, w, dh, 10.0, 16.0, 0x99000000);
        c.round(dx, dy, w, dh, 10.0, 0xF5090A0D);
        c.outline(dx, dy, w, dh, 10.0, 1.0, theme::HAIRLINE_STRONG);
        let (rx, rw) = (dx + 6.0, w - 12.0);
        let mut ry = dy + 6.0;

        if self.style_option_row(ui, c, "title:style:modern", rx, ry, rw, row_h, theme::I_LAYOUT, "Modern", mode == "modern") {
            self.style_menu_open = false;
            act = Some(TitleAct::SetStyle("modern".to_owned()));
        }
        ry += row_h;
        if self.style_option_row(ui, c, "title:style:simple", rx, ry, rw, row_h, theme::I_LAYERS, "Simple", mode == "simple") {
            self.style_menu_open = false;
            act = Some(TitleAct::SetStyle("simple".to_owned()));
        }
        ry += row_h;
        if self.style_option_row(ui, c, "title:style:vanilla", rx, ry, rw, row_h, theme::I_GAMEPAD, "Vanilla", mode == "vanilla") {
            self.style_menu_open = false;
            if mode != "vanilla" {
                act = Some(TitleAct::Classic);
            }
        }
        c.pop_alpha();
        act
    }

    #[allow(clippy::too_many_arguments)]
    fn style_option_row(&self, ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, w: f32, h: f32, icon: u32, title: &str, active: bool) -> bool {
        let over = ui.hover(c, x, y, w, h);
        if over {
            ui.cursor_hand = true;
        }
        let hv = ui.anim_on(&format!("{id}#h"), over, 18.0);
        c.round(x, y, w, h, 7.0, theme::mix(if active { 0x16FFFFFF } else { 0x00000000 }, 0x22FFFFFF, hv));
        let fg = if active { theme::TEXT_STRONG } else { theme::mix(theme::TEXT_SECONDARY, theme::TEXT_STRONG, hv) };
        c.icon(icon, 13.0, x + 15.0, y + h / 2.0, fg);
        let fnt = if active { font::SEMIBOLD } else { font::MEDIUM };
        let lh = c.line_height(font::SEMIBOLD, 12.0);
        c.text(fnt, 12.0, title, x + 28.0, y + (h - lh) / 2.0, fg);
        if active {
            c.icon(theme::I_CHECK, 12.0, x + w - 13.0, y + h / 2.0, theme::TEXT_STRONG);
        }
        ui.clicked(c, id, x, y, w, h)
    }

    /// The menu down the left: the brand, Play, the other ways in, then the small buttons and the promo.
    #[allow(clippy::too_many_arguments)]
    fn menu(&mut self, ui: &mut Ui, c: &mut Canvas, info: &TitleInfo, home: Option<&super::home::Doc>, promo: Option<&super::home::Promo>, x: f32, w: f32, h_all: f32) -> Option<TitleAct> {
        let mut act = None;
        let (rh, gap, sq) = (46.0f32, 8.0f32, 40.0f32);
        let total = 50.0 + 28.0 + (rh + gap) * 4.0 + 6.0 + 15.0 + sq + 10.0 + if promo.is_some() { PROMO_H + 12.0 } else { 0.0 };
        let mut y = 24.0f32.max((h_all * 0.2).min((h_all - 40.0 - total) / 2.0));
        let mp = info.allows_multiplayer;
        let tr = |key: &str, fallback: &str| horizon_client::lang::get_or(key, fallback);

        // The brand.
        let e0 = self.appear(ui, 0);
        c.push_alpha(e0);
        c.icon(icons::for_module("watermark", u32::MAX), 40.0, x + 21.0, y + 23.0, 0xFFFFFFFF);
        c.text(font::BOLD, 26.0, "Native", x + 54.0, y - 2.0, theme::TEXT_STRONG);
        c.text(font::MEDIUM, 12.5, &format!("Minecraft {}", horizon_core::VANILLA_VERSION), x + 55.0, y + 30.0, 0xB3FFFFFF);
        c.pop_alpha();
        y += 78.0;

        if self.list_row(ui, c, 1, "menu:singleplayer", x, y, w, rh, &tr("menu.singleplayer", "Singleplayer"), theme::I_PLAY, true) {
            act = Some(TitleAct::Singleplayer);
        }
        y += rh + gap;
        if self.list_row(ui, c, 2, "menu:multiplayer", x, y, w, rh, &tr("menu.multiplayer", "Multiplayer"), theme::I_SERVER, mp) {
            act = Some(TitleAct::Multiplayer);
        }
        y += rh + gap;
        if self.list_row(ui, c, 3, "menu:realms", x, y, w, rh, &tr("menu.online", "Minecraft Realms"), theme::I_CLOUD, mp) {
            act = Some(TitleAct::Realms);
        }
        y += rh + gap;
        if self.list_row(ui, c, 4, "menu:nativemods", x, y, w, rh, "Native Mods", icons::for_module("watermark", theme::I_SPARKLES), true) {
            act = Some(TitleAct::NativeMods);
        }
        y += rh + 6.0;

        // A hairline, then the small row: Options, Language, Accessibility, Relay, the classic title, Quit.
        let e = self.appear(ui, 5);
        c.push_alpha(e);
        c.fill(x + 4.0, y + 7.0, w - 8.0, 1.0, theme::HAIRLINE);
        y += 15.0;
        let opt_w = w - (sq + gap) * 5.0;
        let options = tr("menu.options", "Options...");
        let label = if opt_w >= 120.0 { options.as_str() } else { "" };
        if button(ui, c, "menu:options", x, y, opt_w, sq, label, theme::I_SETTINGS) {
            act = Some(TitleAct::Options);
        }
        if label.is_empty() && ui.hover(c, x, y, opt_w, sq) {
            ui.tip(&options);
        }
        let mut ix = x + opt_w + gap;
        let squares: [(&str, u32, String, TitleAct); 5] = [
            ("menu:language", theme::I_GLOBE, tr("options.language", "Language..."), TitleAct::Language),
            ("menu:accessibility", theme::I_PERSON, tr("options.accessibility", "Accessibility Settings..."), TitleAct::Accessibility),
            ("menu:relay", theme::I_MESSAGE, "Relay: friends and chat".to_owned(), TitleAct::Relay),
            ("menu:classic", theme::I_LAYERS, "Classic title screen".to_owned(), TitleAct::Classic),
            // Like vanilla: Quit Game quits, no confirmation.
            ("menu:quit", theme::I_POWER, tr("menu.quit", "Quit Game"), TitleAct::Quit),
        ];
        for (id, icon, tip, a) in squares {
            if button(ui, c, id, ix, y, sq, sq, "", icon) {
                act = Some(a.clone());
            }
            if ui.hover(c, ix, y, sq, sq) {
                ui.tip(&tip);
            }
            if a == TitleAct::Relay {
                let unread = relay::unread_total();
                if unread > 0 {
                    badge(c, ix + sq - 2.0, y + 2.0, unread);
                }
            }
            ix += sq + gap;
        }
        y += sq + 10.0;
        // The title promo (Link your Discord, an event...).
        if let (Some(p), Some(d)) = (promo, home)
            && let Some(a) = self.promo_card(ui, c, p, d, x, y + 2.0, w, PROMO_H)
        {
            act = Some(a);
        }
        c.pop_alpha();
        act
    }

    /// A menu row: dark glass, the icon and the name on the left, an arrow on the right when hovered.
    #[allow(clippy::too_many_arguments)]
    fn list_row(&self, ui: &mut Ui, c: &mut Canvas, index: usize, id: &str, x: f32, y: f32, w: f32, h: f32, label: &str, icon: u32, enabled: bool) -> bool {
        let e = self.appear(ui, index);
        c.push_alpha(e * if enabled { 1.0 } else { 0.5 });
        let y = y + (1.0 - e) * 10.0;
        let was = ui.interactive;
        ui.interactive = was && enabled;
        let over = ui.hover(c, x, y, w, h) && enabled;
        let click = ui.clicked(c, id, x, y, w, h) && enabled;
        ui.interactive = was;
        let hv = ui.anim_on(&format!("{id}#h"), over, 14.0);
        let pr = ui.anim_on(&format!("{id}#p"), ui.is_pressing(id), 20.0);
        if over {
            ui.cursor_hand = true;
        }
        let r = 12.0;
        c.round(x, y, w, h, r, theme::mix(theme::mix(0xA60A0B0E, 0xCC14161B, hv), 0xD91B1E25, pr));
        c.outline(x, y, w, h, r, 1.0, theme::mix(0x1FFFFFFF, 0x38FFFFFF, hv));
        c.round(x + 9.0, y + (h - 28.0) / 2.0, 28.0, 28.0, 8.0, theme::mix(0x14FFFFFF, 0x24FFFFFF, hv));
        c.icon(icon, 14.0, x + 23.0, y + h / 2.0, theme::mix(0xD9FFFFFF, 0xFFFFFFFF, hv));
        let lh = c.line_height(font::SEMIBOLD, 14.0);
        let text = c.ellipsize(font::SEMIBOLD, 14.0, label, w - 60.0);
        c.text(font::SEMIBOLD, 14.0, &text, x + 46.0, y + (h - lh) / 2.0, theme::mix(0xE6FFFFFF, 0xFFFFFFFF, hv));
        if hv > 0.01 && w > 150.0 {
            c.icon(theme::I_CHEVRON_RIGHT, 13.0, x + w - 20.0 + hv * 2.0, y + h / 2.0, theme::alpha(0x99FFFFFF, hv));
        }
        c.pop_alpha();
        click
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

const QUEST: u32 = 0xFFB5F23D;
const QUEST_DARK: u32 = 0xFF16210A;
const SALE: u32 = 0xFFB91C1C;
const PROMO_H: f32 = 88.0;

/// "3d 15h 59m 26s" until a time (server clock, ms).
fn countdown(ends_at: f64) -> String {
    let s = ((ends_at - super::home::server_now()) / 1000.0).max(0.0) as u64;
    let (d, h, m, sec) = (s / 86400, s / 3600 % 24, s / 60 % 60, s % 60);
    if d > 0 { format!("{d}d {h}h {m}m {sec}s") } else if h > 0 { format!("{h}h {m}m {sec}s") } else { format!("{m}m {sec}s") }
}

fn quest_height(w: f32) -> f32 {
    let tile = 84.0f32.min((w - 32.0 - 16.0) / 3.0);
    30.0 + 12.0 + 26.0 + 10.0 + tile + 10.0 + 18.0 + 14.0 + 36.0 + 14.0
}

fn goal_text(q: &super::home::Quest) -> String {
    if q.discord() {
        "Link your Discord: type /link with your code in the Native Discord.".into()
    } else {
        format!("Play {} minutes on {}.", q.goal_minutes, if q.servers.is_empty() { "any server".to_owned() } else { q.servers.join(", ") })
    }
}

fn fmt_thousands(n: i64) -> String {
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

/// A reward's picture: the cape's front, a cosmetic's render, or the coin amount.
fn reward_art(c: &mut Canvas, r: &super::home::Reward, x: f32, y: f32, s: f32) {
    if r.coins() {
        c.circle(x + s / 2.0, y + s / 2.0 - 8.0, s * 0.2, 0xFFF5B301);
        c.circle(x + s / 2.0, y + s / 2.0 - 8.0, s * 0.12, 0xFFB98300);
        let n = fmt_thousands(r.amount);
        let tw = c.text_width(font::BOLD, 12.0, &n);
        c.text(font::BOLD, 12.0, &n, x + (s - tw) / 2.0, y + s - 22.0, 0xFFFFD35C);
        return;
    }
    match super::net::image(&r.image, 2 * 1024 * 1024) {
        super::net::Pic::Ready(img) => {
            if r.item_kind == "cape" && img.width == img.height * 2 {
                let dh = s - 12.0;
                let dw = dh * 10.0 / 16.0;
                c.image(&img, x + (s - dw) / 2.0, y + 6.0, dw, dh, [1.0 / 64.0, 1.0 / 32.0, 11.0 / 64.0, 17.0 / 32.0], 0xFFFFFFFF);
            } else {
                let k = ((s - 10.0) / img.width as f32).min((s - 10.0) / img.height as f32);
                let (dw, dh) = (img.width as f32 * k, img.height as f32 * k);
                c.image(&img, x + (s - dw) / 2.0, y + (s - dh) / 2.0, dw, dh, [0.0, 0.0, 1.0, 1.0], 0xFFFFFFFF);
            }
        }
        _ => c.icon(theme::I_SPARKLES, s * 0.3, x + s / 2.0, y + s / 2.0, 0x66FFFFFF),
    }
}

impl TitleView {
    /// Cover-crops the promo artwork at 2x and bakes smooth anti-aliased rounded corners into its alpha channel.
    fn rounded_promo_art(&mut self, src: &std::sync::Arc<crate::native_ui::Image>, w: f32, h: f32, r: f32) -> std::sync::Arc<crate::native_ui::Image> {
        let out_w = ((w * 2.0).round() as u32).clamp(1, 800);
        let out_h = ((h * 2.0).round() as u32).clamp(1, 256);
        if let Some((id, cw, ch, ref cached)) = self.promo_art
            && id == src.id
            && cw == out_w
            && ch == out_h
        {
            return cached.clone();
        }
        let box_aspect = out_w as f32 / out_h as f32;
        let img_aspect = src.width as f32 / src.height as f32;
        let (mut uw, mut vh) = (1.0f32, 1.0f32);
        if img_aspect > box_aspect {
            uw = box_aspect / img_aspect;
        } else {
            vh = img_aspect / box_aspect;
        }
        let cu = 0.5 + 0.3 * (1.0 - uw) / 2.0;
        let cv = 0.5f32;
        let (u0, v0) = (cu - uw / 2.0, cv - vh / 2.0);
        let cr = r * out_h as f32 / h;
        let (sw, sh) = (src.width as usize, src.height as usize);
        let mut out = vec![0u32; (out_w * out_h) as usize];
        for py in 0..out_h {
            let sy = (v0 + (py as f32 + 0.5) / out_h as f32 * vh) * sh as f32 - 0.5;
            let y0 = (sy.floor() as isize).clamp(0, sh as isize - 1) as usize;
            let y1 = (y0 + 1).min(sh - 1);
            let fy = (sy - y0 as f32).clamp(0.0, 1.0);
            let cy = (py as f32 + 0.5).min(out_h as f32 - (py as f32 + 0.5));
            for px in 0..out_w {
                let mut cover = 1.0f32;
                let cx = (px as f32 + 0.5).min(out_w as f32 - (px as f32 + 0.5));
                if cx < cr && cy < cr {
                    let (dx, dy) = (cr - cx, cr - cy);
                    let d = (dx * dx + dy * dy).sqrt() - cr;
                    cover = if d <= -0.5 { 1.0 } else if d >= 0.5 { 0.0 } else { 0.5 - d };
                    if cover <= 0.0 {
                        continue;
                    }
                }
                let sx = (u0 + (px as f32 + 0.5) / out_w as f32 * uw) * sw as f32 - 0.5;
                let x0 = (sx.floor() as isize).clamp(0, sw as isize - 1) as usize;
                let x1 = (x0 + 1).min(sw - 1);
                let fx = (sx - x0 as f32).clamp(0.0, 1.0);
                let c00 = src.argb[y0 * sw + x0];
                let c10 = src.argb[y0 * sw + x1];
                let c01 = src.argb[y1 * sw + x0];
                let c11 = src.argb[y1 * sw + x1];
                let (w00, w10, w01, w11) = ((1.0 - fx) * (1.0 - fy), fx * (1.0 - fy), (1.0 - fx) * fy, fx * fy);
                let ch = |shift: u32| -> f32 {
                    ((c00 >> shift) & 0xFF) as f32 * w00
                        + ((c10 >> shift) & 0xFF) as f32 * w10
                        + ((c01 >> shift) & 0xFF) as f32 * w01
                        + ((c11 >> shift) & 0xFF) as f32 * w11
                };
                let a = (ch(24) * cover).round() as u32;
                let rr = ch(16).round() as u32;
                let gg = ch(8).round() as u32;
                let bb = ch(0).round() as u32;
                out[(py * out_w + px) as usize] = (a << 24) | (rr << 16) | (gg << 8) | bb;
            }
        }
        let img = crate::native_ui::Image::new(out_w, out_h, out);
        self.promo_art = Some((src.id, out_w, out_h, img.clone()));
        img
    }

    /// The top banner: a sale with "Ends in ..." (opens the Store) or the announcement. Its X hides it for the session.
    fn banner(&mut self, ui: &mut Ui, c: &mut Canvas, d: &super::home::Doc, b: &super::home::Banner, w_all: f32, y: f32) {
        let sale = b.kind == "sale";
        let ends = if b.ends_at > 0.0 { format!("Ends in {}", countdown(b.ends_at)) } else { String::new() };
        let (h, fs) = (32.0, 13.0);
        let max_w = 640.0f32.min(w_all - 500.0);
        if max_w < 260.0 {
            return;
        }
        let ends_w = if ends.is_empty() { 0.0 } else { c.text_width(font::SEMIBOLD, fs, &ends) + 14.0 };
        let text = c.ellipsize(font::MEDIUM, fs, &b.text, max_w - ends_w - 76.0);
        let text_w = c.text_width(font::MEDIUM, fs, &text);
        let w = 34.0 + text_w + ends_w + 40.0;
        let x = (w_all - w) / 2.0;
        let tint = if sale { SALE } else { 0xFF4F86F7 };
        let over = ui.hover(c, x, y, w - 32.0, h);
        let hv = ui.anim_on("title:banner#h", over, 14.0);
        c.shadow(x, y + 3.0, w, h, 9.0, 18.0, 0x66000000);
        let base = if sale { 0xF5240A0E } else { 0xF50C1426 };
        c.round(x, y, w, h, 9.0, theme::mix(base, theme::alpha(tint, 0.95), 0.25 + 0.15 * hv));
        c.outline(x, y, w, h, 9.0, 1.0, theme::alpha(tint, 0.85));
        let ty = y + (h - c.line_height(font::MEDIUM, fs)) / 2.0;
        c.icon(if sale { theme::I_STORE } else { theme::I_BELL }, 14.0, x + 18.0, y + h / 2.0, 0xFFFFFFFF);
        c.text(font::MEDIUM, fs, &text, x + 34.0, ty, 0xFFFFFFFF);
        if !ends.is_empty() {
            c.text(font::SEMIBOLD, fs, &ends, x + 34.0 + text_w + 14.0, ty, 0xFFFFFFFF);
        }
        let cx = x + w - 22.0;
        let over_x = ui.hover(c, cx - 10.0, y, 20.0, h);
        c.icon(theme::I_X, 13.0, cx, y + h / 2.0, if over_x { 0xFFFFFFFF } else { 0xCCFFFFFF });
        if ui.clicked(c, "title:banner:x", cx - 10.0, y, 20.0, h) {
            self.hidden_banner = Some(b.text.clone());
            return;
        }
        if over && (sale || b.action == "url") {
            ui.cursor_hand = true;
        }
        if ui.clicked(c, "title:banner", x, y, w - 32.0, h) {
            if sale || b.action == "store" {
                super::common::open_url(&d.store_url);
            } else if b.action == "url" && b.url.starts_with("https://") {
                super::common::open_url(&b.url);
            }
        }
    }

    /// The wide title promo: art, title and text, a label in the promo's colour; Link Discord runs the link flow.
    #[allow(clippy::too_many_arguments)]
    fn promo_card(&mut self, ui: &mut Ui, c: &mut Canvas, p: &super::home::Promo, d: &super::home::Doc, x: f32, y: f32, w: f32, h: f32) -> Option<TitleAct> {
        let main = p.buttons.first();
        let discord = main.is_some_and(|b| b.action == "discord-link");
        let linked = discord && d.discord_linked;
        let over = ui.hover(c, x, y, w, h);
        let hv = ui.anim_on("title:promo#h", over, 12.0);
        c.shadow(x, y + 6.0, w, h, 14.0, 26.0, theme::alpha(p.accent, 0.30 + 0.25 * hv));
        c.round(x, y, w, h, 14.0, 0xF00C0A16);
        match super::net::image(&p.image, 4 * 1024 * 1024) {
            super::net::Pic::Ready(img) => {
                let art = self.rounded_promo_art(&img, w, h, 14.0);
                c.image(&art, x, y, w, h, [0.0, 0.0, 1.0, 1.0], 0xFFFFFFFF);
            }
            _ => c.round_gradient_h(x, y, w, h, 14.0, theme::alpha(p.accent, 0.55), 0xFF0C0A16),
        }
        c.round_gradient_h(x, y, w * 0.75, h, 14.0, 0xC0000000, 0x00000000);
        if hv > 0.005 {
            c.round(x, y, w, h, 14.0, theme::alpha(0xFFFFFFFF, 0.04 * hv));
        }
        c.outline(x, y, w, h, 14.0, 1.2, theme::alpha(p.accent, 0.55 + 0.35 * hv));
        let tag = if linked { "LINKED \u{2713}".to_owned() } else { p.tag.to_uppercase() };
        if !tag.is_empty() {
            let tw = c.text_width(font::BOLD, 10.5, &tag) + 16.0;
            c.round(x + w - tw - 8.0, y - 8.0, tw, 22.0, 7.0, if linked { 0xFF16A34A } else { p.accent });
            let lh = c.line_height(font::BOLD, 10.5);
            c.text(font::BOLD, 10.5, &tag, x + w - tw, y - 8.0 + (22.0 - lh) / 2.0, 0xFFFFFFFF);
        }
        let (tx, max_w) = (x + 18.0, w - 80.0);
        let (code, link_error, link_busy) = super::home::link_state();
        if discord && !linked && let Some(code) = code {
            // The link code: type it in Discord; copy it, open the invite.
            c.text(font::MEDIUM, 11.5, "In the Native Discord, type:", tx, y + 14.0, 0xD9FFFFFF);
            c.text(font::BOLD, 20.0, &format!("/link {code}"), tx, y + 32.0, 0xFFFFFFFF);
            let (bx, by) = (x + w - 18.0 - 118.0, y + h - 34.0);
            let copy_icon = horizon_client::native_ui::icons::named("copy", theme::I_CHECK);
            if ui.button(c, "title:promo:copy", bx - 82.0, by, 76.0, 24.0, "Copy", copy_icon, ui::BTN_GLASS) {
                ui.clip_out = Some(format!("/link {code}"));
            }
            if ui.button(c, "title:promo:open", bx, by, 118.0, 24.0, "Open Discord", theme::I_CHEVRON_RIGHT, ui::BTN_PRIMARY) && !d.discord_invite.is_empty() {
                super::common::open_url(&d.discord_invite);
            }
            c.text(font::REGULAR, 10.5, "Waiting for Discord\u{2026}", tx, y + h - 26.0, 0xB3FFFFFF);
            return None;
        }
        let title = c.ellipsize(font::BOLD, 20.0, &p.title, max_w);
        c.text(font::BOLD, 20.0, &title, tx, y + if p.body.is_empty() { 30.0 } else { 22.0 }, 0xFFFFFFFF);
        let body = if linked {
            format!("Linked as {}. Rewards are yours!", if d.discord_name.is_empty() { "your Discord" } else { &d.discord_name })
        } else if discord && let Some(e) = link_error {
            e
        } else {
            p.body.clone()
        };
        if !body.is_empty() {
            let body = c.ellipsize(font::MEDIUM, 12.0, &body, max_w);
            c.text(font::MEDIUM, 12.0, &body, tx, y + 50.0, 0xD9FFFFFF);
        }
        let main = main?;
        if linked {
            return None;
        }
        let (ax, ay) = (x + w - 30.0, y + h - 26.0);
        c.circle(ax, ay, 12.0, theme::alpha(0xFFFFFFFF, 0.85 + 0.15 * hv));
        if discord && link_busy {
            ui.dots(c, ax, ay, 0xFF0C0A16);
        } else {
            c.icon(theme::I_CHEVRON_RIGHT, 13.0, ax, ay, 0xFF0C0A16);
        }
        if over {
            ui.cursor_hand = true;
        }
        if ui.clicked(c, "title:promo", x, y, w, h) {
            match main.action.as_str() {
                "discord-link" => super::home::start_link(),
                "store" => super::common::open_url(&d.store_url),
                "server" => return Some(TitleAct::Join(main.value.clone())),
                _ if main.value.starts_with("https://") => super::common::open_url(&main.value),
                _ => {}
            }
        }
        None
    }

    /// The quest card, bottom-left: state strip, title, rewards (pick one for "1 of N"), progress and the action.
    #[allow(clippy::too_many_arguments)]
    fn quest_card(&mut self, ui: &mut Ui, c: &mut Canvas, d: &super::home::Doc, x: f32, y: f32, w: f32, h: f32) -> Option<TitleAct> {
        let quests = &d.quests;
        if self.quest_index >= quests.len() {
            self.quest_index = 0;
        }
        let q = &quests[self.quest_index];
        let (active, complete, claimed) = (q.state == "active", q.state == "complete", q.state == "claimed");
        let (claim_busy, message) = super::home::claim_state();
        let busy = claim_busy.as_deref() == Some(q.id.as_str());
        let (code, _, link_busy) = super::home::link_state();
        let strip = if !d.signed_in {
            "Sign in to do quests".to_owned()
        } else if let Some(m) = message {
            m
        } else if claimed {
            "Reward claimed \u{2713}".into()
        } else if complete {
            "Quest complete! Claim your reward".into()
        } else if q.discord() {
            "Link your Discord to complete".into()
        } else {
            "Waiting for quest completion\u{2026}".into()
        };
        let sh = 30.0;
        c.shadow(x, y + 8.0, w, h, 16.0, 30.0, theme::alpha(QUEST, 0.18));
        c.round(x, y, w, h, 16.0, QUEST);
        let strip = c.ellipsize(font::SEMIBOLD, 12.5, &strip, w - 24.0);
        let sw = c.text_width(font::SEMIBOLD, 12.5, &strip);
        let lh = c.line_height(font::SEMIBOLD, 12.5);
        c.text(font::SEMIBOLD, 12.5, &strip, x + (w - sw) / 2.0, y + (sh - lh) / 2.0, QUEST_DARK);
        let (bx, by, bwid, bhgt) = (x + 3.0, y + sh, w - 6.0, h - sh - 3.0);
        c.round(bx, by, bwid, bhgt, 13.0, 0xF60B0D10);
        // Title row.
        let (px, py) = (bx + 14.0, by + 12.0);
        c.icon(horizon_client::native_ui::icons::named("trophy", theme::I_SPARKLES), 17.0, px + 8.0, py + 12.0, QUEST);
        let title = c.ellipsize(font::BOLD, 15.0, &q.title, bwid - 90.0);
        let tlh = c.line_height(font::BOLD, 15.0);
        c.text(font::BOLD, 15.0, &title, px + 24.0, py + 12.0 - tlh / 2.0, 0xFFFFFFFF);
        if quests.len() > 1 {
            let ax = bx + bwid - 50.0;
            if ui.icon_button(c, "title:quest:prev", ax, py, 22.0, theme::I_CHEVRON_LEFT, None) {
                self.quest_index = (self.quest_index + quests.len() - 1) % quests.len();
            }
            if ui.icon_button(c, "title:quest:next", ax + 24.0, py, 22.0, theme::I_CHEVRON_RIGHT, None) {
                self.quest_index = (self.quest_index + 1) % quests.len();
            }
        } else {
            let (hx, hy) = (bx + bwid - 24.0, py + 12.0);
            c.circle(hx, hy, 9.0, 0x33FFFFFF);
            let qw = c.text_width(font::BOLD, 11.0, "?");
            let qlh = c.line_height(font::BOLD, 11.0);
            c.text(font::BOLD, 11.0, "?", hx - qw / 2.0, hy - qlh / 2.0, 0xCCFFFFFF);
            if ui.hover(c, hx - 10.0, hy - 10.0, 20.0, 20.0) {
                ui.tip(&goal_text(q));
            }
        }
        // Reward tiles.
        let n = q.rewards.len().min(3);
        let tile = 84.0f32.min((bwid - 28.0 - 16.0) / 3.0);
        let tx0 = bx + (bwid - (n as f32 * tile + (n.saturating_sub(1)) as f32 * 8.0)) / 2.0;
        let ty = py + 34.0;
        let can_pick = complete && q.pick_one() && !busy;
        if self.picked.as_ref().map(|(id, _)| id.as_str()) != Some(q.id.as_str()) {
            self.picked = None;
        }
        let picked = self.picked.as_ref().map(|(_, i)| *i);
        for (i, r) in q.rewards.iter().take(n).enumerate() {
            let tx = tx0 + i as f32 * (tile + 8.0);
            let id = format!("title:quest:r{i}");
            let over = ui.hover(c, tx, ty, tile, tile);
            let sel = if claimed { q.choice == i as i64 || !q.pick_one() } else { picked == Some(i) };
            let hv = ui.anim_on(&format!("{id}#h"), over && can_pick, 14.0);
            c.round(tx, ty, tile, tile, 10.0, theme::mix(0xFF1A1D24, 0xFF232733, hv));
            if sel {
                c.outline(tx, ty, tile, tile, 10.0, 2.0, QUEST);
            }
            reward_art(c, r, tx, ty, tile);
            if r.owned && !r.coins() {
                c.round(tx + 5.0, ty + 5.0, 36.0, 14.0, 7.0, 0xCC000000);
                let olh = c.line_height(font::BOLD, 8.0);
                c.text(font::BOLD, 8.0, "OWNED", tx + 9.0, ty + 5.0 + (14.0 - olh) / 2.0, QUEST);
            }
            if over {
                ui.tip(&r.name);
                if can_pick {
                    ui.cursor_hand = true;
                }
            }
            if can_pick && ui.clicked(c, &id, tx, ty, tile, tile) {
                self.picked = Some((q.id.clone(), i));
            }
        }
        let picked = self.picked.as_ref().map(|(_, i)| *i);
        // Text.
        let ly = ty + tile + 10.0;
        let line = if !q.description.is_empty() {
            q.description.clone()
        } else if q.pick_one() {
            format!("Earn 1 of {} rewards{}", q.rewards.len(), if q.ends_at > 0.0 { " for a limited time!" } else { "!" })
        } else {
            format!("Earn {}!", if q.rewards.len() == 1 { q.rewards[0].name.clone() } else { "every reward".into() })
        };
        let line = c.ellipsize(font::MEDIUM, 12.0, &line, bwid - 24.0);
        let lw = c.text_width(font::MEDIUM, 12.0, &line);
        c.text(font::MEDIUM, 12.0, &line, bx + (bwid - lw) / 2.0, ly, 0xE6FFFFFF);
        // Action.
        let (ax, aw, ah) = (bx + 12.0, bwid - 24.0, 36.0);
        let ay = by + bhgt - 12.0 - ah;
        if active && !q.discord() && d.signed_in {
            // Progress bar with the minutes, and Join for the quest's server.
            let done = if q.goal_minutes > 0 { (q.minutes / q.goal_minutes as f64).min(1.0) as f32 } else { 0.0 };
            let server = q.servers.first();
            let join_w = if server.is_some() { 74.0 } else { 0.0 };
            let bar_w = aw - join_w - if join_w > 0.0 { 8.0 } else { 0.0 };
            c.round(ax, ay, bar_w, ah, 10.0, 0xFF1A1D24);
            if done > 0.0 {
                c.round(ax, ay, (ah * 0.5).max(bar_w * done), ah, 10.0, theme::alpha(QUEST, 0.9));
            }
            let prog = format!("{} / {} min{}", q.minutes.floor() as i64, q.goal_minutes, server.map_or(String::new(), |s| format!(" on {s}")));
            let prog = c.ellipsize(font::SEMIBOLD, 12.0, &prog, bar_w - 16.0);
            let pw = c.text_width(font::SEMIBOLD, 12.0, &prog);
            let plh = c.line_height(font::SEMIBOLD, 12.0);
            c.text(font::SEMIBOLD, 12.0, &prog, ax + (bar_w - pw) / 2.0, ay + (ah - plh) / 2.0, 0xFFFFFFFF);
            if let Some(server) = server
                && ui.button(c, "title:quest:join", ax + bar_w + 8.0, ay, join_w, ah, "Join", theme::I_PLAY, ui::BTN_GLASS)
            {
                return Some(TitleAct::Join(server.clone()));
            }
            return None;
        }
        let (label, enabled) = if !d.signed_in {
            ("Sign in with Native".to_owned(), false)
        } else if claimed {
            ("Claimed".to_owned(), false)
        } else if complete {
            (if q.pick_one() && picked.is_none() { "Pick a reward".to_owned() } else { "Claim reward".to_owned() }, !busy && (!q.pick_one() || picked.is_some()))
        } else {
            (code.as_ref().map_or_else(|| "Link Discord".to_owned(), |code| format!("/link {code}  \u{b7}  Copy")), !link_busy)
        };
        let over = ui.hover(c, ax, ay, aw, ah) && enabled;
        let hv = ui.anim_on("title:quest:act#h", over, 14.0);
        c.round(ax, ay, aw, ah, 10.0, if enabled { theme::mix(QUEST, 0xFFC8FF5A, hv) } else { 0xFF2A2E36 });
        if busy {
            ui.dots(c, ax + aw / 2.0, ay + ah / 2.0, QUEST_DARK);
        } else {
            let lw = c.text_width(font::BOLD, 13.5, &label);
            let llh = c.line_height(font::BOLD, 13.5);
            c.text(font::BOLD, 13.5, &label, ax + (aw - lw) / 2.0, ay + (ah - llh) / 2.0, if enabled { QUEST_DARK } else { 0xFF9CA3AF });
        }
        if over {
            ui.cursor_hand = true;
        }
        if enabled && ui.clicked(c, "title:quest:act", ax, ay, aw, ah) {
            if complete {
                super::home::claim(&q.id, if q.pick_one() { picked } else { None });
            } else if let Some(code) = code {
                ui.clip_out = Some(format!("/link {code}"));
                if !d.discord_invite.is_empty() {
                    super::common::open_url(&d.discord_invite);
                }
            } else {
                super::home::start_link();
            }
        }
        None
    }
}
