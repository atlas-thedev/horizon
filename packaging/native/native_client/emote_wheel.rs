//! Hold B, point at a slot, release to play. A centre dead zone cancels.
use super::ui::Ui;
use horizon_client::{
    native_emotes,
    native_ui::{Canvas, font, theme},
};
#[derive(Default)]
pub struct Wheel {
    pub open: bool,
    pub page: usize,
    pub selected: Option<usize>,
}
impl Wheel {
    pub fn draw(&mut self, ui: &mut Ui, c: &mut Canvas) -> bool {
        let me = native_emotes::snapshot();
        let pages = me["usablePages"].as_u64().unwrap_or(1).max(1) as usize;
        self.page = self.page.min(pages - 1);
        if ui.scroll != 0.0 {
            self.page = (self.page as i32 - if ui.scroll > 0.0 { 1 } else { -1 })
                .rem_euclid(pages as i32) as usize;
        }
        let (cx, cy) = (c.width() * 0.5, c.height() * 0.5);
        let radius = (c.height() * 0.27).clamp(100.0, 160.0);
        c.fill(0.0, 0.0, c.width(), c.height(), 0x88000000);
        let title = horizon_client::lang::get("native.emotes.title");
        c.text(
            font::SEMIBOLD,
            18.0,
            &title,
            cx - 35.0,
            cy - 25.0,
            theme::TEXT_STRONG,
        );
        let count = format!("{} / {}", self.page + 1, pages);
        c.text(
            font::REGULAR,
            12.0,
            &count,
            cx - 13.0,
            cy + 3.0,
            theme::TEXT_MUTED,
        );
        let dx = ui.mx - cx;
        let dy = ui.my - cy;
        let len = (dx * dx + dy * dy).sqrt();
        self.selected = if len < 45.0 || len > radius + 100.0 {
            None
        } else {
            Some(
                (((dy.atan2(dx) + std::f32::consts::FRAC_PI_2) / std::f32::consts::FRAC_PI_4)
                    .round() as i32)
                    .rem_euclid(8) as usize,
            )
        };
        for slot in 0..8 {
            let a = slot as f32 * std::f32::consts::FRAC_PI_4 - std::f32::consts::FRAC_PI_2;
            let (x, y) = (cx + a.cos() * radius - 57.0, cy + a.sin() * radius - 21.0);
            let id = me["pages"][self.page][slot].as_str();
            let label = id
                .and_then(|id| me["catalog"].as_array()?.iter().find(|i| i["id"] == id))
                .and_then(|i| i["name"].as_str())
                .map(str::to_string)
                .unwrap_or_else(|| horizon_client::lang::get("native.emotes.empty"));
            c.round(
                x,
                y,
                114.0,
                42.0,
                5.0,
                if self.selected == Some(slot) {
                    0xff36557c
                } else {
                    0xee20242b
                },
            );
            c.text(
                font::REGULAR,
                11.0,
                &label,
                x + 8.0,
                y + 14.0,
                if id.is_some() {
                    theme::TEXT_STRONG
                } else {
                    theme::TEXT_MUTED
                },
            );
        }
        let hint = horizon_client::lang::get(if me.is_null() {
            "native.emotes.signin"
        } else {
            "native.emotes.hint"
        });
        c.text(
            font::REGULAR,
            12.0,
            &hint,
            cx - 180.0,
            cy + radius + 65.0,
            theme::TEXT_MUTED,
        );
        if ui.pressed {
            self.release();
            return true;
        }
        false
    }
    pub fn release(&mut self) {
        if let Some(slot) = self.selected.take() {
            native_emotes::play(self.page, slot);
        }
        self.open = false;
    }
}
