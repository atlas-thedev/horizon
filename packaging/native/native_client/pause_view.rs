//! `PauseView`: Feather / Lunar inspired Native pause menu (Esc in a world):
//! - Frosted dark glass layout (Modern card or Compact floating) with smooth entrance
//! - Session status header (Native brand, Singleplayer / Server + live ping, player avatar, Customize toggle)
//! - Prominent blue "Back to Game" CTA with ESC keycap, icon-driven 2-column grid, quick-action bar
//! - Inline Customize popover (Modern / Compact / Vanilla, status header, quick bar, backdrop dim)
//! - Leaves the world on the first click like vanilla (no confirmation)

use super::avatars;
use super::common::badge;
use super::ui::{self, Ui};
use super::uiconfig::UiConfig;
use crate::native_ui::{Canvas, font, icons, theme};

const CARD_BG: u32 = 0xEA080A0F;
const TILE_BG: u32 = 0xC40F1219;
const TILE_HOVER: u32 = 0xEC181C26;
const TILE_PRESS: u32 = 0xF5202532;

/// What a click on the pause menu asks of the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PauseAction {
    None,
    Resume,
    Advancements,
    Stats,
    Options,
    WorldOptions,
    Leave,
    Relay,
    Menu,
    Cosmetics,
    HudEditor,
    Settings,
    ClassicPause,
}

/// The words (the game's language) and facts the view needs.
#[derive(Clone, Debug, Default)]
pub struct PauseInfo {
    pub title: String,
    pub back: String,
    pub advancements: String,
    pub stats: String,
    pub options: String,
    pub world: String,
    pub leave: String,
    /// Playing a world of your own (not a server).
    pub single: bool,
    pub unread: i32,
    pub server: String,
    pub ping: i32,
    pub player: String,
}

#[derive(Default)]
pub struct PauseView {
    customize_open: bool,
    customize_motion: f32,
}

impl PauseView {
    /// The menu just came up: the intro plays again and the Customize popover starts closed.
    pub fn opened(&mut self, ui: &mut Ui) {
        self.customize_open = false;
        self.customize_motion = 0.0;
        ui.set_anim("pause#in", 0.0);
    }

    /// Esc closes the Customize popover first; returns true when consumed.
    pub fn escape(&mut self) -> bool {
        if self.customize_open {
            self.customize_open = false;
            return true;
        }
        false
    }

    pub fn draw(&mut self, ui: &mut Ui, c: &mut Canvas, info: &PauseInfo, cfg: &mut UiConfig) -> PauseAction {
        let compact = cfg.pause_style == "compact";
        let show_status = cfg.pause_show_status;
        let show_quick = cfg.pause_show_quick;
        let dim = cfg.pause_backdrop_dim.clamp(0.2, 0.9);

        let (w_all, h_all) = (c.width(), c.height());
        let inn = ui.anim("pause#in", 1.0, 13.0);
        c.fill(0.0, 0.0, w_all, h_all, theme::alpha(0xFF000000, dim * inn));
        c.gradient_v(0.0, 0.0, w_all, h_all * 0.24, theme::alpha(0x52000000, inn), 0x00000000);
        c.gradient_v(0.0, h_all * 0.72, w_all, h_all * 0.28 + 1.0, 0x00000000, theme::alpha(0x66000000, inn));

        let interactive = ui.interactive;
        if self.customize_open && ui.key(ui::KEY_ESCAPE) {
            self.customize_open = false;
        }
        if self.customize_open {
            ui.interactive = false;
        }

        let pad = if compact { 0.0 } else { 18.0 };
        let inner_w = (if compact { 420.0f32 } else { 436.0f32 }).min(w_all - 40.0 - pad * 2.0);
        let card_w = inner_w + pad * 2.0;
        let gap = 8.0f32;
        let half = (inner_w - gap) / 2.0;
        let (resume_h, row_h, quick_h, leave_h) = (46.0f32, 42.0f32, 34.0f32, 42.0f32);
        let header_h = if show_status { 56.0 } else { 36.0 };
        let quick_block_h = if show_quick { quick_h + 10.0 } else { 0.0 };

        let content_h = header_h + 10.0 + resume_h + 10.0 + (row_h + gap) * 3.0 + quick_block_h + 6.0 + leave_h;
        let card_h = content_h + pad * 2.0;
        let card_x = (w_all - card_w) / 2.0;
        let card_y = 20.0f32.max((h_all - card_h) / 2.0) + (1.0 - inn) * 10.0;

        c.push_alpha(inn);
        if !compact {
            c.shadow(card_x, card_y + 10.0, card_w, card_h, 18.0, 34.0, 0x99000000);
            c.round(card_x, card_y, card_w, card_h, 18.0, CARD_BG);
            c.outline(card_x, card_y, card_w, card_h, 18.0, 1.0, theme::HAIRLINE_STRONG);
        }

        let x = card_x + pad;
        let mut y = card_y + pad;
        let logo_icon = icons::for_module("watermark", theme::I_SPARKLES);

        let gear_s = 30.0f32;
        let gear_x = x + inner_w - gear_s;
        let gear_y = y + if show_status { 4.0 } else { 3.0 };

        if show_status {
            c.round(x, y + 3.0, 34.0, 34.0, 10.0, 0x1A3B8CF0);
            c.outline(x, y + 3.0, 34.0, 34.0, 10.0, 1.0, 0x333B8CF0);
            c.icon(logo_icon, 20.0, x + 17.0, y + 20.0, 0xFFFFFFFF);
            let tx = x + 44.0;
            c.text(font::BOLD, 16.0, &info.title, tx, y + 1.0, theme::TEXT_STRONG);

            let sy = y + 24.0;
            let dot_col = if info.single { 0xFF22C55E } else { ping_color(info.ping) };
            let status_text = if info.single || info.server.is_empty() { "Singleplayer" } else { info.server.as_str() };
            let ping_text = (!info.single && info.ping >= 0).then(|| format!("{} ms", info.ping));
            let ping_w = ping_text.as_deref().map_or(0.0, |p| c.text_width(font::SEMIBOLD, 10.0, p) + 12.0);
            let max_status_w = (gear_x - tx - 56.0 - ping_w).max(80.0);
            let fit_status = c.ellipsize(font::MEDIUM, 11.5, status_text, max_status_w);
            c.circle(tx + 4.0, sy + 7.5, 3.2, dot_col);
            c.text(font::MEDIUM, 11.5, &fit_status, tx + 13.0, sy + 0.5, theme::TEXT_SECONDARY);
            if let Some(pt) = ping_text.as_deref() {
                let px = tx + 13.0 + c.text_width(font::MEDIUM, 11.5, &fit_status) + 8.0;
                c.round(px, sy - 0.5, ping_w, 16.0, 8.0, theme::alpha(dot_col, 0.16));
                let plh = c.line_height(font::SEMIBOLD, 10.0);
                c.text(font::SEMIBOLD, 10.0, pt, px + 6.0, sy + (16.0 - plh) / 2.0 - 0.5, dot_col);
            }

            let av_s = 28.0f32;
            let av_x = gear_x - av_s - 8.0;
            let av_y = gear_y + 1.0;
            avatars::draw(c, avatars::own(&info.player).as_deref(), av_x, av_y, av_s, 7.0);
            if ui.hover(c, av_x, av_y, av_s, av_s) && !info.player.is_empty() {
                ui.tip(&info.player);
            }
        } else {
            c.icon(logo_icon, 20.0, x + 11.0, y + 17.0, 0xFFFFFFFF);
            c.text(font::BOLD, 16.0, &info.title, x + 30.0, y + 6.0, theme::TEXT_STRONG);
        }

        // Customize button stays clickable even while popover is open
        ui.interactive = interactive;
        let over_gear = ui.hover(c, gear_x, gear_y, gear_s, gear_s);
        if over_gear {
            ui.cursor_hand = true;
        }
        let gh = ui.anim_on("pause:customize#h", over_gear || self.customize_open, 16.0);
        c.round(gear_x, gear_y, gear_s, gear_s, 9.0, theme::mix(0xB810131B, 0xE6222836, gh));
        c.outline(gear_x, gear_y, gear_s, gear_s, 9.0, 1.0, theme::mix(theme::HAIRLINE, 0x8060A5FA, if self.customize_open { 1.0 } else { gh * 0.6 }));
        c.icon(theme::I_SLIDERS, 14.0, gear_x + gear_s / 2.0, gear_y + gear_s / 2.0, if self.customize_open { 0xFF60A5FA } else { theme::TEXT_STRONG });
        if over_gear && !self.customize_open {
            ui.tip("Customize pause menu");
        }
        if ui.clicked(c, "pause:customize", gear_x, gear_y, gear_s, gear_s) {
            self.customize_open = !self.customize_open;
        }
        if self.customize_open {
            ui.interactive = false;
        }

        y += header_h;
        if !compact {
            c.fill(x, y - 4.0, inner_w, 1.0, theme::HAIRLINE);
        }
        y += 10.0;

        let mut action = PauseAction::None;
        if resume_button(ui, c, "pause:resume", x, y, inner_w, resume_h, &info.back) {
            action = PauseAction::Resume;
        }
        y += resume_h + 10.0;

        if tile_button(ui, c, "pause:adv", x, y, half, row_h, &info.advancements, theme::I_CROWN, 0) {
            action = PauseAction::Advancements;
        }
        if tile_button(ui, c, "pause:stats", x + half + gap, y, half, row_h, &info.stats, theme::I_ACTIVITY, 0) {
            action = PauseAction::Stats;
        }
        y += row_h + gap;

        if tile_button(ui, c, "pause:menu", x, y, half, row_h, "Native Mods", logo_icon, 0xFF3B8CF0) {
            action = PauseAction::Menu;
        }
        if tile_button(ui, c, "pause:relay", x + half + gap, y, half, row_h, "Relay", theme::I_MESSAGE, 0) {
            action = PauseAction::Relay;
        }
        if info.unread > 0 {
            badge(c, x + inner_w - 10.0, y + row_h / 2.0, info.unread);
        }
        y += row_h + gap;

        if tile_button(ui, c, "pause:options", x, y, half, row_h, &info.options, theme::I_SETTINGS, 0) {
            action = PauseAction::Options;
        }
        if info.single {
            if tile_button(ui, c, "pause:lan", x + half + gap, y, half, row_h, &info.world, theme::I_GLOBE, 0) {
                action = PauseAction::WorldOptions;
            }
        } else if tile_button(ui, c, "pause:locker", x + half + gap, y, half, row_h, "Locker", theme::I_SHIRT, 0) {
            action = PauseAction::Cosmetics;
        }
        y += row_h + gap;

        if show_quick {
            let qw = (inner_w - gap * 2.0) / 3.0;
            if quick_pill(ui, c, "pause:hud", x, y, qw, quick_h, "HUD Editor", theme::I_LAYOUT) {
                action = PauseAction::HudEditor;
            }
            if quick_pill(ui, c, "pause:quick:locker", x + qw + gap, y, qw, quick_h, "Locker", theme::I_SHIRT) {
                action = PauseAction::Cosmetics;
            }
            if quick_pill(ui, c, "pause:quick:settings", x + (qw + gap) * 2.0, y, qw, quick_h, "Settings", theme::I_SLIDERS) {
                action = PauseAction::Settings;
            }
            y += quick_h + 10.0;
        }

        y += 6.0;
        if leave_button(ui, c, "pause:leave", x, y, inner_w, leave_h, &info.leave) {
            action = PauseAction::Leave;
        }
        c.pop_alpha();

        ui.interactive = interactive;
        if let Some(cfg_act) = self.draw_customize_popover(ui, c, cfg, card_x + card_w, gear_y + gear_s + 8.0, gear_x, gear_y, gear_s) {
            action = cfg_act;
        }
        action
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_customize_popover(&mut self, ui: &mut Ui, c: &mut Canvas, cfg: &mut UiConfig, right_edge: f32, top: f32, gear_x: f32, gear_y: f32, gear_s: f32) -> Option<PauseAction> {
        let target = if self.customize_open { 1.0 } else { 0.0 };
        self.customize_motion += (target - self.customize_motion) * (1.0 - (-22.0 * ui.dt).exp());
        if (self.customize_motion - target).abs() < 0.01 {
            self.customize_motion = target;
        }
        if self.customize_motion <= 0.0 {
            return None;
        }
        let (w, h) = (264.0f32, 252.0f32);
        let px = (c.width() - w - 12.0).min((right_edge - w).max(12.0));
        let py = (c.height() - h - 12.0).min(top) - 8.0 * (1.0 - self.customize_motion);
        let on_gear = ui.hover(c, gear_x, gear_y, gear_s, gear_s);
        if ui.interactive && self.customize_open && ui.pressed && !on_gear && !ui.hover(c, px, py, w, h) {
            self.customize_open = false;
            return None;
        }

        let mut act = None;
        c.push_alpha(self.customize_motion);
        c.shadow(px, py + 6.0, w, h, 14.0, 24.0, 0xB8000000);
        c.round(px, py, w, h, 14.0, 0xFC090B10);
        c.outline(px, py, w, h, 14.0, 1.0, theme::HAIRLINE_STRONG);

        let (ix, iw) = (px + 14.0, w - 28.0);
        let mut iy = py + 12.0;
        c.text(font::BOLD, 13.0, "Customize Pause Menu", ix, iy, theme::TEXT_STRONG);
        iy += 26.0;

        c.text(font::SEMIBOLD, 10.5, "STYLE", ix, iy, theme::TEXT_MUTED);
        iy += 16.0;
        let compact = cfg.pause_style == "compact";
        let seg_h = 28.0;
        c.round(ix, iy, iw, seg_h, 8.0, 0xCC050609);
        c.outline(ix, iy, iw, seg_h, 8.0, 1.0, theme::HAIRLINE);
        let half = (iw - 6.0) / 2.0;
        if seg_pill(ui, c, "pause:cfg:modern", ix + 3.0, iy + 3.0, half, seg_h - 6.0, "Modern", !compact) {
            cfg.pause_style = "modern".to_owned();
            cfg.save();
        }
        if seg_pill(ui, c, "pause:cfg:compact", ix + 3.0 + half, iy + 3.0, half, seg_h - 6.0, "Compact", compact) {
            cfg.pause_style = "compact".to_owned();
            cfg.save();
        }
        iy += seg_h + 12.0;

        if toggle_row(ui, c, "pause:cfg:status", ix, iy, iw, 28.0, "Session & ping header", cfg.pause_show_status) {
            cfg.pause_show_status = !cfg.pause_show_status;
            cfg.save();
        }
        iy += 31.0;
        if toggle_row(ui, c, "pause:cfg:quick", ix, iy, iw, 28.0, "Quick-action bar", cfg.pause_show_quick) {
            cfg.pause_show_quick = !cfg.pause_show_quick;
            cfg.save();
        }
        iy += 34.0;

        c.text(font::SEMIBOLD, 10.5, "BACKDROP", ix, iy, theme::TEXT_MUTED);
        iy += 16.0;
        c.round(ix, iy, iw, seg_h, 8.0, 0xCC050609);
        c.outline(ix, iy, iw, seg_h, 8.0, 1.0, theme::HAIRLINE);
        let third = (iw - 6.0) / 3.0;
        if seg_pill(ui, c, "pause:cfg:dim0", ix + 3.0, iy + 3.0, third, seg_h - 6.0, "Light", cfg.pause_backdrop_dim < 0.45) {
            cfg.pause_backdrop_dim = 0.35;
            cfg.save();
        }
        if seg_pill(ui, c, "pause:cfg:dim1", ix + 3.0 + third, iy + 3.0, third, seg_h - 6.0, "Medium", (0.45..=0.68).contains(&cfg.pause_backdrop_dim)) {
            cfg.pause_backdrop_dim = 0.58;
            cfg.save();
        }
        if seg_pill(ui, c, "pause:cfg:dim2", ix + 3.0 + third * 2.0, iy + 3.0, third, seg_h - 6.0, "Dark", cfg.pause_backdrop_dim > 0.68) {
            cfg.pause_backdrop_dim = 0.78;
            cfg.save();
        }
        iy += seg_h + 12.0;

        if ui.button(c, "pause:cfg:vanilla", ix, iy, iw, 30.0, "Switch to Vanilla Pause", theme::I_GAMEPAD, ui::BTN_GLASS) {
            self.customize_open = false;
            cfg.custom_pause = false;
            cfg.save();
            act = Some(PauseAction::ClassicPause);
        }
        c.pop_alpha();
        act
    }
}

#[allow(clippy::too_many_arguments)]
fn resume_button(ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, w: f32, h: f32, label: &str) -> bool {
    let over = ui.hover(c, x, y, w, h);
    let click = ui.clicked(c, id, x, y, w, h);
    let hv = ui.anim_on(&format!("{id}#h"), over, 14.0);
    let pr = ui.anim_on(&format!("{id}#p"), ui.is_pressing(id), 20.0);
    if over {
        ui.cursor_hand = true;
    }
    let r = 12.0;
    let lift = -hv * 1.2 + pr * 1.2;
    c.shadow(x + 6.0, y + 8.0 + lift, w - 12.0, h - 4.0, r, 18.0, theme::alpha(0xFF2563EB, 0.38 + 0.18 * hv));
    c.round_gradient_h(x, y + lift, w, h, r, theme::mix(0xFF3B8CF0, 0xFF55A0FF, hv), theme::mix(0xFF1F5FC8, 0xFF2D72E0, hv));
    c.outline(x, y + lift, w, h, r, 1.0, 0x45FFFFFF);
    c.fill(x + r, y + lift + 1.0, w - r * 2.0, 1.0, 0x38FFFFFF);
    c.icon(theme::I_PLAY, 16.0, x + 22.0, y + lift + h / 2.0, 0xFFFFFFFF);
    let fit = c.ellipsize(font::BOLD, 14.5, label, (w - 92.0).max(20.0));
    let lh = c.line_height(font::BOLD, 14.5);
    c.text(font::BOLD, 14.5, &fit, x + 42.0, y + lift + (h - lh) / 2.0, 0xFFFFFFFF);
    let (kw, kh) = (34.0f32, 20.0f32);
    let (kx, ky) = (x + w - kw - 12.0, y + lift + (h - kh) / 2.0);
    c.round(kx, ky, kw, kh, 6.0, 0x2B000000);
    c.outline(kx, ky, kw, kh, 6.0, 1.0, 0x38FFFFFF);
    let ew = c.text_width(font::BOLD, 9.5, "ESC");
    let elh = c.line_height(font::BOLD, 9.5);
    c.text(font::BOLD, 9.5, "ESC", kx + (kw - ew) / 2.0, ky + (kh - elh) / 2.0, 0xE6FFFFFF);
    click
}

#[allow(clippy::too_many_arguments)]
fn tile_button(ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, w: f32, h: f32, label: &str, icon: u32, accent: u32) -> bool {
    let over = ui.hover(c, x, y, w, h);
    let click = ui.clicked(c, id, x, y, w, h);
    let hv = ui.anim_on(&format!("{id}#h"), over, 16.0);
    let pr = ui.anim_on(&format!("{id}#p"), ui.is_pressing(id), 22.0);
    if over {
        ui.cursor_hand = true;
    }
    let r = 11.0;
    let base_bg = if accent != 0 {
        theme::mix(TILE_BG, theme::alpha(accent, 0.22), 0.35 + 0.25 * hv)
    } else {
        theme::mix(TILE_BG, TILE_HOVER, hv)
    };
    c.round(x, y, w, h, r, theme::mix(base_bg, TILE_PRESS, pr * 0.6));
    let edge = if accent != 0 {
        theme::mix(theme::alpha(accent, 0.42), theme::alpha(accent, 0.78), hv)
    } else {
        theme::mix(theme::HAIRLINE, theme::BORDER_HOVER, hv)
    };
    c.outline(x, y, w, h, r, 1.0, edge);

    let icon_box = 26.0;
    let (ix, iy) = (x + 8.0, y + (h - icon_box) / 2.0);
    let badge_bg = if accent != 0 {
        theme::alpha(accent, 0.24 + 0.12 * hv)
    } else {
        theme::mix(0x14FFFFFF, 0x24FFFFFF, hv)
    };
    c.round(ix, iy, icon_box, icon_box, 7.5, badge_bg);
    let ic_col = if accent != 0 { 0xFFFFFFFF } else { theme::mix(theme::TEXT_SECONDARY, theme::TEXT_STRONG, hv) };
    c.icon(icon, 13.5, ix + icon_box / 2.0, iy + icon_box / 2.0, ic_col);

    let tx = ix + icon_box + 9.0;
    let fit = c.ellipsize(font::SEMIBOLD, 13.0, label, (w - (tx - x) - 10.0).max(20.0));
    let lh = c.line_height(font::SEMIBOLD, 13.0);
    c.text(font::SEMIBOLD, 13.0, &fit, tx, y + (h - lh) / 2.0, theme::mix(0xE6FFFFFF, 0xFFFFFFFF, hv));
    click
}

#[allow(clippy::too_many_arguments)]
fn quick_pill(ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, w: f32, h: f32, label: &str, icon: u32) -> bool {
    let over = ui.hover(c, x, y, w, h);
    let click = ui.clicked(c, id, x, y, w, h);
    let hv = ui.anim_on(&format!("{id}#h"), over, 16.0);
    if over {
        ui.cursor_hand = true;
    }
    c.round(x, y, w, h, 9.0, theme::mix(0xAD0D1017, 0xDF181D29, hv));
    c.outline(x, y, w, h, 9.0, 1.0, theme::mix(theme::HAIRLINE, theme::BORDER_HOVER, hv));
    let fit = c.ellipsize(font::SEMIBOLD, 11.5, label, (w - 30.0).max(20.0));
    let tw = c.text_width(font::SEMIBOLD, 11.5, &fit);
    let sx = x + (w - (13.0 + 6.0 + tw)) / 2.0;
    c.icon(icon, 12.5, sx + 6.5, y + h / 2.0, theme::mix(theme::TEXT_SECONDARY, theme::TEXT_STRONG, hv));
    let lh = c.line_height(font::SEMIBOLD, 11.5);
    c.text(font::SEMIBOLD, 11.5, &fit, sx + 19.0, y + (h - lh) / 2.0, theme::mix(theme::TEXT, theme::TEXT_STRONG, hv));
    click
}

#[allow(clippy::too_many_arguments)]
fn leave_button(ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, w: f32, h: f32, label: &str) -> bool {
    let over = ui.hover(c, x, y, w, h);
    let click = ui.clicked(c, id, x, y, w, h);
    let hv = ui.anim_on(&format!("{id}#h"), over, 16.0);
    if over {
        ui.cursor_hand = true;
    }
    c.round(x, y, w, h, 11.0, theme::mix(0xC0120C10, 0xE63D151B, hv));
    c.outline(x, y, w, h, 11.0, 1.0, theme::mix(0x38EF4444, 0xB8F87171, hv));
    let fit = c.ellipsize(font::SEMIBOLD, 13.5, label, (w - 48.0).max(20.0));
    let tw = c.text_width(font::SEMIBOLD, 13.5, &fit);
    let sx = x + (w - (15.0 + 8.0 + tw)) / 2.0;
    let fg = theme::mix(0xFFFCA5A5, 0xFFFFFFFF, hv);
    c.icon(theme::I_LOG_OUT, 14.0, sx + 7.0, y + h / 2.0, fg);
    let lh = c.line_height(font::SEMIBOLD, 13.5);
    c.text(font::SEMIBOLD, 13.5, &fit, sx + 22.0, y + (h - lh) / 2.0, fg);
    click
}

#[allow(clippy::too_many_arguments)]
fn seg_pill(ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, w: f32, h: f32, label: &str, on: bool) -> bool {
    let over = ui.hover(c, x, y, w, h);
    let click = ui.clicked(c, id, x, y, w, h);
    if over {
        ui.cursor_hand = true;
    }
    let hv = ui.anim_on(&format!("{id}#h"), over, 14.0);
    let sl = ui.anim_on(&format!("{id}#on"), on, 16.0);
    if sl > 0.01 || hv > 0.01 {
        c.round(x, y, w, h, 6.0, theme::mix(theme::alpha(theme::SUBTLE_HOVER, hv * 0.7), 0xFF3B8CF0, sl));
    }
    let fg = theme::mix(theme::mix(theme::TEXT_SECONDARY, theme::TEXT_STRONG, hv), 0xFFFFFFFF, sl);
    let tw = c.text_width(font::SEMIBOLD, 11.0, label);
    let lh = c.line_height(font::SEMIBOLD, 11.0);
    c.text(font::SEMIBOLD, 11.0, label, x + (w - tw) / 2.0, y + (h - lh) / 2.0, fg);
    click
}

#[allow(clippy::too_many_arguments)]
fn toggle_row(ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, w: f32, h: f32, label: &str, on: bool) -> bool {
    let over = ui.hover(c, x, y, w, h);
    let click = ui.clicked(c, id, x, y, w, h);
    if over {
        ui.cursor_hand = true;
    }
    let hv = ui.anim_on(&format!("{id}#h"), over, 14.0);
    let t = ui.anim_on(&format!("{id}#on"), on, 16.0);
    if hv > 0.01 {
        c.round(x - 4.0, y, w + 8.0, h, 7.0, theme::alpha(theme::SUBTLE_HOVER, hv * 0.55));
    }
    let lh = c.line_height(font::MEDIUM, 12.0);
    c.text(font::MEDIUM, 12.0, label, x, y + (h - lh) / 2.0, theme::TEXT_STRONG);
    let (sw, sh) = (32.0f32, 18.0f32);
    let (sx, sy) = (x + w - sw, y + (h - sh) / 2.0);
    c.round(sx, sy, sw, sh, sh / 2.0, theme::mix(0xFF272A34, 0xFF3B8CF0, t));
    c.circle(sx + 9.0 + t * 14.0, sy + sh / 2.0, 6.5, 0xFFFFFFFF);
    click
}

fn ping_color(ping: i32) -> u32 {
    if ping < 85 {
        0xFF22C55E
    } else if ping < 160 {
        0xFFF59E0B
    } else {
        0xFFEF4444
    }
}
