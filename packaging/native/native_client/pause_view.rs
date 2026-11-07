//! `PauseView`: the pause menu (Esc in a world), laid out like vanilla's:
//! "Game Menu" on top, then Back to Game, Advancements and Statistics, Relay
//! and Native Mods, Options and World Options (Cosmetics on a server), and
//! Save and Quit to Title / Disconnect. Plain centred labels on dark buttons
//! like the Native title screen. Leaving happens on the first click, like
//! vanilla: no confirmation.

use super::common::badge;
use super::ui::{self, Ui};
use crate::native_ui::{Canvas, font, theme};

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
}

#[derive(Default)]
pub struct PauseView;

impl PauseView {
    /// The menu just came up: the intro plays again.
    pub fn opened(&mut self, ui: &mut Ui) {
        ui.set_anim("pause#in", 0.0);
    }

    pub fn draw(&mut self, ui: &mut Ui, c: &mut Canvas, info: &PauseInfo) -> PauseAction {
        let (w_all, h_all) = (c.width(), c.height());
        let inn = ui.anim("pause#in", 1.0, 12.0);
        c.fill(0.0, 0.0, w_all, h_all, theme::alpha(0x8C000000, inn));
        // Vanilla's grid: 204 x 20 buttons, 4 px apart, a quarter of the way down (in GUI pixels, doubled here).
        let (bw, bh, step, gap) = ((408.0f32).min(w_all - 40.0), 40.0f32, 48.0f32, 8.0f32);
        let half = (bw - gap) / 2.0;
        let x = (w_all - bw) / 2.0;
        let mut y = 64.0f32.max((h_all / 4.0 + 16.0).min(h_all - 24.0 - (step * 4.0 + bh))) + (1.0 - inn) * 8.0;
        c.push_alpha(inn);
        let tw = c.text_width(font::BOLD, 17.0, &info.title);
        let ty = 18.0f32.max(y - 64.0);
        c.text(font::BOLD, 17.0, &info.title, (w_all - tw) / 2.0 + 1.0, ty + 1.0, 0x99000000);
        c.text(font::BOLD, 17.0, &info.title, (w_all - tw) / 2.0, ty, theme::TEXT_STRONG);
        let mut action = PauseAction::None;
        if button(ui, c, "pause:resume", x, y, bw, bh, &info.back) {
            action = PauseAction::Resume;
        }
        y += step;
        if button(ui, c, "pause:adv", x, y, half, bh, &info.advancements) {
            action = PauseAction::Advancements;
        }
        if button(ui, c, "pause:stats", x + half + gap, y, half, bh, &info.stats) {
            action = PauseAction::Stats;
        }
        y += step;
        if button(ui, c, "pause:relay", x, y, half, bh, "Relay") {
            action = PauseAction::Relay;
        }
        if info.unread > 0 {
            badge(c, x + half - 10.0, y + bh / 2.0, info.unread);
        }
        if button(ui, c, "pause:menu", x + half + gap, y, half, bh, "Native Mods") {
            action = PauseAction::Menu;
        }
        y += step;
        if button(ui, c, "pause:options", x, y, half, bh, &info.options) {
            action = PauseAction::Options;
        }
        if info.single {
            if button(ui, c, "pause:lan", x + half + gap, y, half, bh, &info.world) {
                action = PauseAction::WorldOptions;
            }
        } else if button(ui, c, "pause:locker", x + half + gap, y, half, bh, "Cosmetics") {
            action = PauseAction::Cosmetics;
        }
        y += step;
        if button(ui, c, "pause:leave", x, y, bw, bh, &info.leave) {
            action = PauseAction::Leave;
        }
        c.pop_alpha();
        action
    }
}

/// A plain, centred launcher button on a dark backing (the title screen's buttons), so it reads over any world.
#[allow(clippy::too_many_arguments)]
fn button(ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, w: f32, h: f32, label: &str) -> bool {
    c.round(x, y, w, h, 10.0f32.min(h / 2.0), 0x8C000000);
    let label = c.ellipsize(font::SEMIBOLD, 14.0, label, w - 16.0);
    ui.button(c, id, x, y, w, h, &label, 0, ui::BTN_GLASS)
}
