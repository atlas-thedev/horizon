//! Eight radial sectors over the world, with actual player renders and an open cancel zone.
use super::{net, ui::Ui};
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
fn slot_at(dx: f32, dy: f32, inner: f32, outer: f32) -> Option<usize> {
    let d = dx * dx + dy * dy;
    if d < inner * inner || d > outer * outer {
        return None;
    }
    Some(
        (((dy.atan2(dx) + std::f32::consts::FRAC_PI_2) / std::f32::consts::FRAC_PI_4).round()
            as i32)
            .rem_euclid(8) as usize,
    )
}
fn sector(c: &mut Canvas, cx: f32, cy: f32, inner: f32, outer: f32, a: f32, b: f32, color: u32) {
    for n in 0..14 {
        let x = a + (b - a) * n as f32 / 14.0;
        let y = a + (b - a) * (n + 1) as f32 / 14.0;
        c.free_quad(
            None,
            [
                cx + x.cos() * inner,
                cy + x.sin() * inner,
                cx + x.cos() * outer,
                cy + x.sin() * outer,
                cx + y.cos() * outer,
                cy + y.sin() * outer,
                cx + y.cos() * inner,
                cy + y.sin() * inner,
            ],
            [0.0; 8],
            color,
        );
    }
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
        let outer = (c.height() * 0.34).min(230.0);
        let inner = outer * 0.19;
        let (cx, cy) = (c.width() * 0.5, c.height() * 0.48);
        c.fill(0.0, 0.0, c.width(), c.height(), 0x50000000);
        self.selected = slot_at(ui.mx - cx, ui.my - cy, inner, outer);
        for slot in 0..8 {
            let angle = slot as f32 * std::f32::consts::FRAC_PI_4 - std::f32::consts::FRAC_PI_2;
            let a = angle - std::f32::consts::FRAC_PI_4 * 0.5 + 0.022;
            let b = angle + std::f32::consts::FRAC_PI_4 * 0.5 - 0.022;
            let active = self.selected == Some(slot);
            sector(
                c,
                cx,
                cy,
                inner,
                outer,
                a,
                b,
                if active { 0xffb99ceb } else { 0xff292d36 },
            );
            sector(
                c,
                cx,
                cy,
                inner + 1.3,
                outer - 1.3,
                a + 0.005,
                b - 0.005,
                if active { 0xf22d253d } else { 0xef111319 },
            );
            let item = me["pages"][self.page][slot]
                .as_str()
                .and_then(|id| me["catalog"].as_array()?.iter().find(|i| i["id"] == id));
            let label = item
                .and_then(|i| i["name"].as_str())
                .map(str::to_string)
                .unwrap_or_else(|| horizon_client::lang::get("native.emotes.empty"));
            let (x, y) = (
                cx + angle.cos() * outer * 0.64,
                cy + angle.sin() * outer * 0.64,
            );
            let h = outer * 0.33;
            let w = h * 300.0 / 340.0;
            let mut drawn = false;
            if let Some(item) = item {
                let url = item["posterUrl"].as_str().map(str::to_string).or_else(|| {
                    item["clipUrl"]
                        .as_str()
                        .map(|u| format!("{}.png", u.replace("/clips/", "/posters/")))
                });
                if let Some(url) = url
                    && let net::Pic::Ready(img) = net::image(&url, 512 * 1024)
                {
                    c.image(
                        &img,
                        x - w / 2.0,
                        y - h * 0.72,
                        w,
                        h,
                        [0.0, 0.0, 1.0, 1.0],
                        0xffffffff,
                    );
                    drawn = true;
                }
            }
            if !drawn {
                let n = (slot + 1).to_string();
                let fs = outer * 0.085;
                let nw = c.text_width(font::SEMIBOLD, fs, &n);
                c.text(font::SEMIBOLD, fs, &n, x - nw / 2.0, y - 15.0, 0xff5d536c);
            }
            let label = c.ellipsize(font::SEMIBOLD, 11.0, &label, outer * 0.49);
            let tw = c.text_width(font::SEMIBOLD, 11.0, &label);
            c.text(
                font::SEMIBOLD,
                11.0,
                &label,
                x - tw / 2.0,
                y + h * 0.33,
                if active { 0xfff1e8ff } else { 0xffb6b9c3 },
            );
        }
        let title = horizon_client::lang::get("native.emotes.select");
        let tw = c.text_width(font::SEMIBOLD, 18.0, &title);
        c.text(
            font::SEMIBOLD,
            18.0,
            &title,
            cx - tw / 2.0,
            cy - outer - 38.0,
            theme::TEXT_STRONG,
        );
        c.icon(theme::I_X, 14.0, cx, cy, 0xff7b7f8c);
        let count = format!(
            "{} {} / {}",
            horizon_client::lang::get("native.emotes.page"),
            self.page + 1,
            pages
        );
        let tw = c.text_width(font::REGULAR, 12.0, &count);
        let footer = cy + outer + 17.0;
        c.text(
            font::REGULAR,
            12.0,
            &count,
            cx - tw / 2.0,
            footer,
            0xffdfdfe8,
        );
        if pages > 1 {
            c.text(
                font::SEMIBOLD,
                20.0,
                "‹",
                cx - 76.0,
                footer - 4.0,
                0xffc3aedf,
            );
            c.text(
                font::SEMIBOLD,
                20.0,
                "›",
                cx + 67.0,
                footer - 4.0,
                0xffc3aedf,
            );
            if ui.clicked(c, "emote:prev", cx - 88.0, footer - 6.0, 32.0, 28.0) {
                self.page = (self.page + pages - 1) % pages;
            }
            if ui.clicked(c, "emote:next", cx + 56.0, footer - 6.0, 32.0, 28.0) {
                self.page = (self.page + 1) % pages;
            }
        }
        let hint = horizon_client::lang::get(if me.is_null() {
            "native.emotes.signin"
        } else {
            "native.emotes.hint"
        });
        let tw = c.text_width(font::REGULAR, 10.0, &hint);
        c.text(
            font::REGULAR,
            10.0,
            &hint,
            cx - tw / 2.0,
            footer + 26.0,
            0xff777c8a,
        );
        if ui.pressed && self.selected.is_some() {
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
#[cfg(test)]
mod tests {
    use super::slot_at;
    #[test]
    fn radial_selection_and_cancel_zone() {
        assert_eq!(slot_at(0., -100., 20., 200.), Some(0));
        assert_eq!(slot_at(100., 0., 20., 200.), Some(2));
        assert_eq!(slot_at(0., 100., 20., 200.), Some(4));
        assert_eq!(slot_at(-100., 0., 20., 200.), Some(6));
        assert_eq!(slot_at(1., 1., 20., 200.), None);
        assert_eq!(slot_at(300., 0., 20., 200.), None);
    }
}
