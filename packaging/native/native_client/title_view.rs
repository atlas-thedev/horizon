//! Native home screen, with shortcuts to saved servers and launcher profiles.

use super::ads::{self, Ad};
use super::common::badge;
use super::ui::{self, Ui};
use super::{net, profiles::Profiles, relay, scroll::Scroll};
use crate::native_ui::{Canvas, Image, font, icons, theme};
use horizon_client::lang::get_or as tr;
use std::collections::HashSet;
use std::sync::Arc;

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
    Settings,
    Cosmetics,
    Relay,
    /// The vanilla title screen from now on.
    Classic,
    /// An ad's server.
    Join(String),
    /// One of the game's multiplayer list buttons (`multiplayer::Screen::click`), with the saved server selected.
    ServerButton {
        id: u8,
        index: Option<usize>,
    },
    /// Move a saved server up or down the list.
    ServerMove {
        index: usize,
        up: bool,
    },
    /// The game's own multiplayer screen, until the title shows again.
    ServersClassic,
    /// A Native dialog's answer for the game's Add / Edit server screen (`done` false: cancelled).
    ServerForm {
        name: String,
        address: String,
        packs: Option<bool>,
        done: bool,
    },
    /// The Direct Connection dialog's answer (`join` false: cancelled).
    ServerDirect {
        address: String,
        join: bool,
    },
    /// The Delete dialog's answer.
    ServerDelete {
        yes: bool,
    },
}

/// What the title needs to know of the game.
#[allow(dead_code)]
#[derive(Clone, Debug, Default)]
pub struct TitleInfo {
    pub allows_multiplayer: bool,
    pub game_dir: Option<std::path::PathBuf>,
    pub player: String,
    pub servers: std::path::PathBuf,
}

#[derive(Default)]
pub struct TitleView {
    shown_at: f64,
    hidden_ads: HashSet<String>,
    ad_index: usize,
    ad_switch_at: f64,
    ad_prev: Option<usize>,
    ad_fade_at: f64,
    quick_servers: Vec<QuickServer>,
    quick_stamp: Option<std::time::SystemTime>,
    quick_checked_at: f64,
    profile_open: bool,
    profiles: Profiles,
    profile_scroll: Scroll,
    profile_error: bool,
}

struct QuickServer {
    name: String,
    address: String,
    icon: Option<Arc<Image>>,
}

fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

/// A launcher button on a dark backing, so its label reads over any artwork.
#[allow(clippy::too_many_arguments)]
fn button(
    ui: &mut Ui,
    c: &mut Canvas,
    id: &str,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    label: &str,
    icon: u32,
) -> bool {
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
    fn row_button(
        &self,
        ui: &mut Ui,
        c: &mut Canvas,
        index: usize,
        id: &str,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        label: &str,
        enabled: bool,
    ) -> bool {
        let e = self.appear(ui, index);
        c.push_alpha(e * if enabled { 1.0 } else { 0.5 });
        let was = ui.interactive;
        ui.interactive = was && enabled;
        let hit = if id == "menu:singleplayer" {
            ui.button(
                c,
                id,
                x,
                y + (1.0 - e) * 10.0,
                w,
                h,
                label,
                theme::I_PLAY,
                ui::BTN_PRIMARY,
            )
        } else {
            button(ui, c, id, x, y + (1.0 - e) * 10.0, w, h, label, 0)
        };
        ui.interactive = was;
        c.pop_alpha();
        hit && enabled
    }

    /// The panorama shading, submitted before the Minecraft logo and all foreground UI.
    pub fn background(&self, c: &mut Canvas) {
        let (w, h) = (c.width(), c.height());
        c.fill(0.0, 0.0, w, h, 0x45000000);
        c.gradient_v(0.0, 0.0, w, 120.0, 0x7A000000, 0x00000000);
        c.gradient_v(0.0, h * 0.55, w, h * 0.45, 0x00000000, 0xA6000000);
    }

    pub fn draw(&mut self, ui: &mut Ui, c: &mut Canvas, info: &TitleInfo) -> Option<TitleAct> {
        self.opened(ui);
        let (w_all, h_all) = (c.width(), c.height());
        let inn = ui.anim("title#in", 1.0, 4.0);
        c.push_alpha(inn);
        let mut act = None;
        let interactive = ui.interactive;
        if ui.keys.iter().any(|(key, _)| *key == ui::KEY_ESCAPE) {
            self.profile_open = false;
        }
        ui.interactive = interactive && !self.profile_open;

        let wide = w_all >= 700.0;
        let rail = if wide {
            205.0_f32.min(w_all * 0.24)
        } else {
            50.0
        };
        let bw = 374.0_f32.min(w_all - 124.0_f32.max(2.0 * (rail + 38.0)));
        let bh = if h_all < 500.0 { 34.0 } else { 42.0 };
        let step = bh + 7.0;
        let half = (bw - 8.0) / 2.0;
        let x = (w_all - bw) / 2.0;
        let total = step * 4.0 + 12.0 + bh;
        let mut y = 100.0_f32.max(((h_all - total) / 2.0 + 35.0).min(h_all - 34.0 - total));
        let name = "Native Client";
        let word_w = c.text_width(font::BOLD, 26.0, name);
        let logo_x = (w_all - word_w - 46.0) / 2.0;
        c.round(
            logo_x - 14.0,
            y - 58.0,
            word_w + 74.0,
            49.0,
            12.0,
            theme::alpha(theme::panel(), 0.91),
        );
        c.outline(
            logo_x - 14.0,
            y - 58.0,
            word_w + 74.0,
            49.0,
            12.0,
            1.0,
            theme::hairline(),
        );
        c.icon(
            icons::for_module("watermark", u32::MAX),
            35.0,
            logo_x + 17.5,
            y - 33.5,
            theme::text_strong(),
        );
        c.text(
            font::BOLD,
            26.0,
            name,
            logo_x + 46.0,
            y - 49.0,
            theme::text_strong(),
        );
        if let Some(action) = self.quick_rail(ui, c, info, y, rail, wide) {
            act = Some(action);
        }
        let mp = info.allows_multiplayer;
        if self.row_button(
            ui,
            c,
            0,
            "menu:singleplayer",
            x,
            y,
            bw,
            bh,
            &horizon_client::lang::get_or("menu.singleplayer", "Singleplayer"),
            true,
        ) {
            act = Some(TitleAct::Singleplayer);
        }
        y += step;
        if self.row_button(
            ui,
            c,
            1,
            "menu:multiplayer",
            x,
            y,
            bw,
            bh,
            &horizon_client::lang::get_or("menu.multiplayer", "Multiplayer"),
            mp,
        ) {
            act = Some(TitleAct::Multiplayer);
        }
        y += step;
        if self.row_button(
            ui,
            c,
            2,
            "menu:realms",
            x,
            y,
            bw,
            bh,
            &horizon_client::lang::get_or("menu.online", "Minecraft Realms"),
            mp,
        ) {
            act = Some(TitleAct::Realms);
        }
        y += step;
        if self.row_button(
            ui,
            c,
            3,
            "menu:nativemods",
            x,
            y,
            half,
            bh,
            &tr("native.title.mods", "Native Mods"),
            true,
        ) {
            act = Some(TitleAct::NativeMods);
        }
        let signed = net::signed_in();
        if self.row_button(
            ui,
            c,
            3,
            "menu:cosmetics",
            x + half + 8.0,
            y,
            half,
            bh,
            &tr("native.title.cosmetics", "Cosmetics"),
            signed,
        ) {
            act = Some(TitleAct::Cosmetics);
        }
        y += step + 12.0;

        let appear = self.appear(ui, 4);
        c.push_alpha(appear);
        if button(
            ui,
            c,
            "menu:options",
            x,
            y,
            half,
            bh,
            &horizon_client::lang::get_or("menu.options", "Options..."),
            0,
        ) {
            act = Some(TitleAct::Options);
        }
        // Like vanilla: Quit Game quits, no confirmation.
        if button(
            ui,
            c,
            "menu:quit",
            x + half + 8.0,
            y,
            half,
            bh,
            &horizon_client::lang::get_or("menu.quit", "Quit Game"),
            0,
        ) {
            act = Some(TitleAct::Quit);
        }
        c.pop_alpha();
        if button(
            ui,
            c,
            "menu:classic",
            16.0,
            16.0,
            40.0,
            36.0,
            "",
            theme::I_LAYERS,
        ) {
            act = Some(TitleAct::Classic);
        }
        if ui.hover(c, 16.0, 16.0, 40.0, 36.0) {
            ui.tip(&tr("native.title.classic", "Classic title screen"));
        }
        let profile_w = if w_all < 400.0 { 94.0 } else { 138.0 };
        let px = w_all - 16.0 - profile_w;
        if button(
            ui,
            c,
            "menu:preferences",
            px - 46.0,
            16.0,
            38.0,
            36.0,
            "",
            theme::I_SETTINGS,
        ) {
            act = Some(TitleAct::Settings);
        }
        if ui.hover(c, px - 46.0, 16.0, 38.0, 36.0) {
            ui.tip(&tr(
                "native.title.preferences",
                "Native settings and themes",
            ));
        }
        if signed {
            if button(
                ui,
                c,
                "menu:relay",
                px - 92.0,
                16.0,
                38.0,
                36.0,
                "",
                theme::I_MESSAGE,
            ) {
                act = Some(TitleAct::Relay);
            }
            if ui.hover(c, px - 92.0, 16.0, 38.0, 36.0) {
                ui.tip("Relay");
            }
            let unread = relay::unread_total();
            if unread > 0 {
                badge(c, px - 60.0, 16.0, unread);
            }
        }

        // Bottom-right: the sponsored card, when there is room beside the column.
        let ads: Vec<Ad> = ads::list(info.game_dir.clone())
            .into_iter()
            .filter(|a| !self.hidden_ads.contains(&a.id))
            .collect();
        let side = (w_all - bw) / 2.0 - bh - 8.0 - 32.0;
        let cw = side.min(300.0);
        if !ads.is_empty() && cw >= 230.0 {
            let ad_h = cw * 5.0 / 12.0 + 62.0;
            let ay = h_all - 36.0 - ad_h;
            if ay >= 64.0 {
                let e = self.appear(ui, 5);
                c.push_alpha(e);
                if let Some(a) =
                    self.ad_card(ui, c, &ads, w_all - 16.0 - cw + (1.0 - e) * 20.0, ay, cw)
                {
                    act = Some(a);
                }
                c.pop_alpha();
            }
        }

        // The footer, as vanilla: the version bottom-left, Mojang bottom-right.
        let fy = h_all - 22.0;
        let version = format!(
            "Minecraft {}  \u{b7}  Native Client",
            horizon_core::VANILLA_VERSION
        );
        c.text(font::MEDIUM, 11.0, &version, 16.0, fy, 0xB3FFFFFF);
        let copy = "Copyright Mojang AB. Do not distribute!";
        let cwid = c.text_width(font::MEDIUM, 11.0, copy);
        c.text(
            font::MEDIUM,
            11.0,
            copy,
            w_all - 16.0 - cwid,
            fy,
            0xB3FFFFFF,
        );
        ui.interactive = interactive;
        let label = c.ellipsize(font::SEMIBOLD, 12.0, &info.player, profile_w - 39.0);
        if button(
            ui,
            c,
            "menu:profiles",
            px,
            16.0,
            profile_w,
            36.0,
            &label,
            theme::I_USER,
        ) {
            self.profile_open = !self.profile_open;
            if self.profile_open {
                self.profiles = info
                    .game_dir
                    .as_deref()
                    .map(Profiles::load)
                    .unwrap_or_default();
                self.profile_scroll = Scroll::new("title:profiles");
                self.profile_error = false;
            }
        }
        if self.profile_open && self.profile_card(ui, c, info) {
            act = Some(TitleAct::Quit);
        }
        c.pop_alpha();
        act
    }

    /// "3 friends online" with their faces; true when clicked. Right-aligned to `right`.
    fn quick_rail(
        &mut self,
        ui: &mut Ui,
        c: &mut Canvas,
        info: &TitleInfo,
        menu_y: f32,
        w: f32,
        wide: bool,
    ) -> Option<TitleAct> {
        if self.quick_checked_at == 0.0 || ui.now - self.quick_checked_at >= 2_000.0 {
            self.quick_checked_at = ui.now;
            let stamp = info
                .servers
                .metadata()
                .ok()
                .and_then(|meta| meta.modified().ok());
            if stamp != self.quick_stamp {
                self.quick_stamp = stamp;
                self.quick_servers = horizon_protocol::servers::load(&info.servers)
                    .into_iter()
                    .filter(|server| !server.ip.trim().is_empty())
                    .take(4)
                    .map(|server| QuickServer {
                        name: if server.name.is_empty() {
                            server.ip.clone()
                        } else {
                            server.name
                        },
                        address: server.ip,
                        icon: server
                            .icon
                            .as_deref()
                            .filter(|data| data.len() <= 87_400)
                            .and_then(horizon_client::skins::base64_decode)
                            .and_then(|bytes| Image::decode(&bytes))
                            .filter(|image| image.width <= 128 && image.height <= 128),
                    })
                    .collect();
            }
        }
        let x = 16.0;
        let y = (menu_y - 19.0).max(68.0);
        let start = y + if wide { 37.0 } else { 9.0 };
        let available = ((c.height() - 34.0 - start - 52.0) / 47.0).max(0.0) as usize;
        let count = self.quick_servers.len().min(available);
        let height = start - y + count as f32 * 47.0 + 47.0;
        c.round(x, y, w, height, 12.0, theme::alpha(theme::panel(), 0.94));
        c.outline(x, y, w, height, 12.0, 1.0, theme::hairline());
        if wide {
            c.text(
                font::SEMIBOLD,
                13.0,
                &tr("native.title.quick_servers", "Quick Servers"),
                x + 12.0,
                y + 13.0,
                theme::text_strong(),
            );
        }
        let was = ui.interactive;
        ui.interactive &= info.allows_multiplayer;
        let mut action = None;
        for (i, server) in self.quick_servers.iter().take(count).enumerate() {
            let (name, address) = (&server.name, &server.address);
            let top = start + i as f32 * 47.0;
            if ui.button(
                c,
                &format!("menu:quick:{i}"),
                x + 6.0,
                top,
                w - 12.0,
                40.0,
                "",
                0,
                ui::BTN_GLASS,
            ) {
                action = Some(TitleAct::Join(address.clone()));
            }
            if let Some(icon) = &server.icon {
                c.image(
                    icon,
                    x + 11.0,
                    top + 7.0,
                    26.0,
                    26.0,
                    [0.0, 0.0, 1.0, 1.0],
                    0xFFFFFFFF,
                );
            } else {
                c.round(
                    x + 11.0,
                    top + 7.0,
                    26.0,
                    26.0,
                    8.0,
                    theme::alpha(theme::accent(), 0.16),
                );
                c.icon(theme::I_SERVER, 13.0, x + 24.0, top + 20.0, theme::accent());
            }
            if wide {
                let name = c.ellipsize(font::SEMIBOLD, 12.0, name, w - 51.0);
                let address = c.ellipsize(font::REGULAR, 10.0, address, w - 51.0);
                c.text(
                    font::SEMIBOLD,
                    12.0,
                    &name,
                    x + 44.0,
                    top + 6.0,
                    theme::text_strong(),
                );
                c.text(
                    font::REGULAR,
                    10.0,
                    &address,
                    x + 44.0,
                    top + 23.0,
                    theme::text_muted(),
                );
            }
            if ui.hover(c, x + 6.0, top, w - 12.0, 40.0) {
                ui.tip(&format!("{name} · {address}"));
            }
        }
        let browse = start + count as f32 * 47.0;
        let label = if wide {
            tr("native.title.server_list", "Server list")
        } else {
            String::new()
        };
        if ui.button(
            c,
            "menu:quick:list",
            x + 6.0,
            browse,
            w - 12.0,
            38.0,
            &label,
            theme::I_GRID,
            ui::BTN_GHOST,
        ) {
            action = Some(TitleAct::Multiplayer);
        }
        ui.interactive = was;
        action
    }

    fn profile_card(&mut self, ui: &mut Ui, c: &mut Canvas, info: &TitleInfo) -> bool {
        let w = 320.0_f32.min(c.width() - 32.0);
        let x = c.width() - 16.0 - w;
        let y = 60.0;
        let list = (c.height() - 172.0)
            .max(46.0)
            .min((self.profiles.profiles.len() as f32 * 42.0).max(46.0));
        let h = 88.0 + list;
        if ui.pressed && (ui.mx < x || ui.mx > x + w || ui.my > y + h) && ui.my > 52.0 {
            self.profile_open = false;
        }
        c.round(x, y, w, h, 14.0, theme::panel());
        c.outline(x, y, w, h, 14.0, 1.0, theme::hairline_strong());
        c.text(
            font::BOLD,
            15.0,
            &tr("native.profile.title", "Minecraft profiles"),
            x + 14.0,
            y + 12.0,
            theme::text_strong(),
        );
        c.text(
            font::REGULAR,
            11.0,
            &tr("native.profile.restart", "Switching restarts Minecraft."),
            x + 14.0,
            y + 34.0,
            theme::text_secondary(),
        );
        let off = self
            .profile_scroll
            .begin(ui, c, x + 8.0, y + 54.0, w - 16.0, list);
        let mut rename = None;
        for (i, profile) in self.profiles.profiles.iter().enumerate() {
            let selected = self.profiles.active_id.as_deref() == Some(&profile.id);
            let row = y + 54.0 - off + 42.0 * i as f32;
            let name = c.ellipsize(font::SEMIBOLD, 12.0, &profile.name, w - 125.0);
            let label = format!("{name} · {}", profile.label());
            if ui.button(
                c,
                &format!("menu:profile:{}", profile.id),
                x + 9.0,
                row,
                w - 22.0,
                38.0,
                &label,
                if selected {
                    theme::I_CHECK
                } else {
                    theme::I_USER
                },
                ui::BTN_GLASS,
            ) && !selected
            {
                rename = Some(profile);
            }
        }
        self.profile_scroll
            .end(ui, c, self.profiles.profiles.len() as f32 * 42.0);
        if let Some(profile) = rename {
            if info
                .game_dir
                .as_deref()
                .is_some_and(|dir| self.profiles.request(dir, profile).is_ok())
            {
                return true;
            }
            self.profile_error = true;
        }
        let note = if self.profile_error {
            tr(
                "native.profile.failed",
                "The launcher could not accept the switch.",
            )
        } else if self.profiles.profiles.is_empty() {
            tr(
                "native.profile.empty",
                "Add profiles in the Native launcher.",
            )
        } else {
            tr(
                "native.profile.offline_note",
                "Offline profiles disable Relay and cosmetics.",
            )
        };
        let note = c.ellipsize(font::REGULAR, 10.0, &note, w - 28.0);
        c.text(
            font::REGULAR,
            10.0,
            &note,
            x + 14.0,
            y + h - 21.0,
            theme::text_muted(),
        );
        false
    }

    fn show_ad(&mut self, index: usize, now: f64) {
        self.ad_prev = Some(self.ad_index);
        self.ad_index = index;
        self.ad_fade_at = now;
        self.ad_switch_at = now + AD_ROTATE_MS;
    }

    /// A sponsored card: banner, text, buttons, dots; it rotates every 12 s unless hovered.
    #[allow(clippy::too_many_arguments)]
    fn ad_card(
        &mut self,
        ui: &mut Ui,
        c: &mut Canvas,
        ads: &[Ad],
        x: f32,
        y: f32,
        w: f32,
    ) -> Option<TitleAct> {
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
        let mut fade = if self.ad_prev.is_none() {
            1.0
        } else {
            clamp01(((ui.now - self.ad_fade_at) / 420.0) as f32)
        };
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
        c.image_cover(
            &ad.image,
            x,
            y,
            w,
            mh,
            1.0 + 0.03 * (1.0 - fade),
            0.0,
            0.0,
            theme::alpha(0xFFFFFFFF, fade),
        );
        c.outline(x, y, w, h, 16.0, 1.0, theme::hairline());

        // Close: hide this ad for the rest of the session.
        let (cs, cxx, cyy) = (22.0f32, x + w - 22.0 - 8.0, y + 8.0);
        let over_close = ui.hover(c, cxx, cyy, cs, cs);
        let hc = ui.anim_on("ad#close", over_close, 14.0);
        c.circle(
            cxx + cs / 2.0,
            cyy + cs / 2.0,
            cs / 2.0,
            theme::alpha(0xFF000000, 0.5 + 0.25 * hc),
        );
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
            let bg = if i == 0 {
                theme::mix(0xFFFFFFFF, 0xFFE8E9EF, hb)
            } else {
                theme::alpha(0xFFFFFFFF, 0.10 + 0.08 * hb)
            };
            let fg = if i == 0 { 0xFF0D0E12 } else { 0xFFFFFFFF };
            c.round(xx, yy, buttons_w, bh, bh / 2.0, bg);
            let icon = if b.server() {
                theme::I_PLAY
            } else {
                theme::I_CHEVRON_RIGHT
            };
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
        let tag = if ad.tag.is_empty() {
            "AD".to_owned()
        } else {
            format!("AD \u{b7} {}", ad.tag.to_uppercase())
        };
        let lines = if ad.body.is_empty() { 2 } else { 3 };
        let ty = fy + (fh - if lines == 3 { 46.0 } else { 30.0 }) / 2.0;
        let tag = c.ellipsize(font::BOLD, 9.5, &tag, max_w);
        c.text(font::BOLD, 9.5, &tag, tx, ty, theme::text_muted());
        let title = c.ellipsize(font::SEMIBOLD, 13.5, &ad.title, max_w);
        c.text(
            font::SEMIBOLD,
            13.5,
            &title,
            tx,
            ty + 13.0,
            theme::text_strong(),
        );
        if !ad.body.is_empty() {
            let body = c.ellipsize(font::REGULAR, 11.0, &ad.body, max_w);
            c.text(
                font::REGULAR,
                11.0,
                &body,
                tx,
                ty + 32.0,
                theme::text_secondary(),
            );
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
            c.round(
                dx - 4.0,
                dy - 4.0,
                total + 8.0,
                dw + 8.0,
                (dw + 8.0) / 2.0,
                0x66000000,
            );
            for i in 0..count {
                let active = i == self.ad_index % count;
                let ww = if active { dw + 6.0 } else { dw };
                c.round(
                    dx,
                    dy,
                    ww,
                    dw,
                    dw / 2.0,
                    if active { 0xFFFFFFFF } else { 0x80FFFFFF },
                );
                if ui.clicked(
                    c,
                    &format!("ad#dot{i}"),
                    dx - 2.0,
                    dy - 4.0,
                    ww + 4.0,
                    dw + 8.0,
                ) && !active
                {
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
