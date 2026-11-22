//! Eight radial sectors over the world, with actual player renders and an open cancel zone.
//! Styled cleanly after the sleek translucent circular wheel: floating `◀ Page N ▶` top pill,
//! smooth dark translucent annulus without cluttered grid dividers, translucent sector highlight
//! with a crisp white inner-rim arc indicator on the hovered sector, outer-rim numbers `1`..`8`,
//! Native logo centered in the inner circle, and large bold selected emote name below the wheel.
use super::{net, preview3d, ui::Ui};
use horizon_client::{
    native_emotes,
    native_ui::{Canvas, Image, font, icons, theme},
};
use std::collections::HashMap;
const SECTOR_HALF: f32 = std::f32::consts::FRAC_PI_4 * 0.5 - 0.027;
#[derive(Default)]
pub struct Wheel {
    pub open: bool,
    pub page: usize,
    pub selected: Option<usize>,
    hover_started: f64,
    bounds: HashMap<u64, [f32; 4]>,
}
pub(super) fn poster_url(item: &serde_json::Value, api: &str) -> Option<String> {
    let raw = item["posterUrl"]
        .as_str()
        .map(str::to_string)
        .or_else(|| {
            item["clipUrl"]
                .as_str()
                .map(|u| format!("{}.png", u.replace("/clips/", "/posters/")))
        })
        .or_else(|| {
            item["hash"]
                .as_str()
                .filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
                .map(|h| format!("{}/v1/emotes/posters/{h}.png", api.trim_end_matches('/')))
        })?;
    if raw.contains("?v=") || raw.contains("&v=") {
        Some(raw)
    } else if raw.contains('?') {
        Some(format!("{raw}&v=2"))
    } else {
        Some(format!("{raw}?v=2"))
    }
}
fn opaque_bounds(img: &Image) -> [f32; 4] {
    if img.width == 0 || img.height == 0 {
        return [0.0, 0.0, 1.0, 1.0];
    }
    let (mut x0, mut y0, mut x1, mut y1) = (img.width, img.height, 0u32, 0u32);
    for y in 0..img.height {
        let row = (y * img.width) as usize;
        for x in 0..img.width {
            if let Some(&px) = img.argb.get(row + x as usize)
                && (px >> 24) >= 8
            {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    if x0 >= x1 || y0 >= y1 {
        [0.0, 0.0, 1.0, 1.0]
    } else {
        [
            x0 as f32 / img.width as f32,
            y0 as f32 / img.height as f32,
            x1 as f32 / img.width as f32,
            y1 as f32 / img.height as f32,
        ]
    }
}
fn inside_sector(
    px: f32,
    py: f32,
    cx: f32,
    cy: f32,
    inner: f32,
    outer: f32,
    angle: f32,
    pad: f32,
) -> bool {
    let (dx, dy) = (px - cx, py - cy);
    let r = dx.hypot(dy);
    if r < inner + 1.3 + pad || r > outer - 1.3 - pad {
        return false;
    }
    let da = ((dy.atan2(dx) - angle + std::f32::consts::PI)
        .rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI)
        .abs();
    da < SECTOR_HALF && r * (SECTOR_HALF - da).sin() >= pad
}
fn fit_poster(
    bounds: [f32; 4],
    aspect: f32,
    cx: f32,
    cy: f32,
    inner: f32,
    outer: f32,
    angle: f32,
    label_y: f32,
) -> [f32; 4] {
    let [u0, v0, u1, v1] = bounds;
    let sx = cx + angle.cos() * outer * 0.65;
    let sy = cy + angle.sin() * outer * 0.65;
    let h0 = outer * 0.34;
    let w0 = h0 * aspect;
    let pad = (outer * 0.015).min(3.0);
    let gap = (outer * 0.01).min(2.0);
    let place = |k: f32| -> Option<[f32; 4]> {
        let (w, h) = (w0 * k, h0 * k);
        let rx = sx - w * 0.5;
        let max_y = label_y - gap - v1 * h;
        let base_y = (sy - h * 0.58).min(max_y);
        for shift in [0.0, 0.04, -0.04, 0.08, -0.08, 0.12] {
            let ty = (base_y + shift * h).min(max_y);
            let (ix0, iy0) = (rx + u0 * w, ty + v0 * h);
            let (ix1, iy1) = (rx + u1 * w, ty + v1 * h);
            let pts = [
                (ix0, iy0),
                (ix1, iy0),
                (ix0, iy1),
                (ix1, iy1),
                (cx.clamp(ix0, ix1), cy.clamp(iy0, iy1)),
            ];
            if pts
                .into_iter()
                .all(|(px, py)| inside_sector(px, py, cx, cy, inner, outer, angle, pad))
            {
                return Some([rx, ty, w, h]);
            }
        }
        None
    };
    if let Some(rect) = place(1.0) {
        return rect;
    }
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    let mut best = [sx, sy, 0.0, 0.0];
    for _ in 0..14 {
        let mid = (lo + hi) * 0.5;
        if let Some(rect) = place(mid) {
            best = rect;
            lo = mid;
        } else {
            hi = mid;
        }
    }
    best
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
fn radius(width: f32, height: f32) -> f32 {
    ((width - 32.0) / 2.0).min((height - 116.0) / 2.0).clamp(24.0, 220.0)
}
fn wrap_page(page: usize, delta: i32, pages: usize) -> usize {
    (page as i32 + delta).rem_euclid(pages.max(1) as i32) as usize
}
#[allow(clippy::too_many_arguments)]
fn sector(c: &mut Canvas, cx: f32, cy: f32, inner: f32, outer: f32, a: f32, b: f32, color: u32) {
    let steps = (((b - a).abs() / (std::f32::consts::PI / 28.0)).ceil() as usize).max(14);
    for n in 0..steps {
        let x = a + (b - a) * n as f32 / steps as f32;
        let y = a + (b - a) * (n + 1) as f32 / steps as f32;
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
/// A straight radial line from radius `r0` to `r1` at angle `ang`, `width` pixels wide.
fn spoke(c: &mut Canvas, cx: f32, cy: f32, r0: f32, r1: f32, ang: f32, width: f32, color: u32) {
    let (dx, dy) = (ang.cos(), ang.sin());
    let (nx, ny) = (-dy * width / 2.0, dx * width / 2.0);
    let (x0, y0, x1, y1) = (cx + dx * r0, cy + dy * r0, cx + dx * r1, cy + dy * r1);
    c.free_quad(None, [x0 + nx, y0 + ny, x1 + nx, y1 + ny, x1 - nx, y1 - ny, x0 - nx, y0 - ny], [0.0; 8], color);
}

impl Wheel {
    fn hover(&mut self, slot: Option<usize>, now: f64) -> f64 {
        if slot != self.selected { self.hover_started = now; }
        self.selected = slot;
        (now - self.hover_started).max(0.0)
    }
    fn change_page(&mut self, delta: i32, pages: usize) {
        self.page = wrap_page(self.page.min(pages.saturating_sub(1)), delta, pages);
        self.selected = None;
    }
    pub fn navigate(&mut self, key: i32, pressed: bool, repeat: bool) -> bool {
        if !matches!(key, 49..=56 | 257 | 262 | 263) { return false; }
        if pressed && !repeat {
            match key {
                49..=56 => { self.selected = Some((key - 49) as usize); self.select(); }
                257 => { self.select(); }
                _ => {
                    let pages = native_emotes::snapshot()["usablePages"].as_u64().unwrap_or(1).max(1) as usize;
                    self.change_page(if key == 263 { -1 } else { 1 }, pages);
                }
            }
        }
        true
    }
    pub fn key(&mut self, wheel_key: bool, escape: bool, pressed: bool, repeat: bool) -> bool {
        if !self.open || (!wheel_key && !escape) {
            return false;
        }
        if pressed && !repeat {
            self.open = false;
            self.selected = None;
        }
        true
    }
    pub fn draw(&mut self, ui: &mut Ui, c: &mut Canvas) -> bool {
        let me = native_emotes::snapshot();
        let pages = me["usablePages"].as_u64().unwrap_or(1).max(1) as usize;
        self.page = self.page.min(pages - 1);
        if ui.scroll != 0.0 {
            self.change_page(if ui.scroll > 0.0 { -1 } else { 1 }, pages);
        }
        let outer = radius(c.width(), c.height());
        let inner = outer * 0.34;
        let (cx, cy) = (c.width() * 0.5, c.height() * 0.5);
        let api = net::api();
        c.fill(0.0, 0.0, c.width(), c.height(), 0x24000000);

        // Top floating page pill: ◀   Page N   ▶
        let pill_w = 134.0;
        let pill_h = 30.0;
        let pill_x = cx - pill_w / 2.0;
        let pill_y = (cy - outer - 44.0).max(12.0);
        c.shadow(pill_x, pill_y + 2.0, pill_w, pill_h, 8.0, 14.0, 0x55000000);
        c.round(pill_x, pill_y, pill_w, pill_h, 8.0, 0xD811131A);
        c.outline(pill_x, pill_y, pill_w, pill_h, 8.0, 1.0, theme::HAIRLINE_STRONG);
        let page_txt = format!("{} {}", horizon_client::lang::get("native.emotes.page"), self.page + 1);
        let pw = c.text_width(font::SEMIBOLD, 12.0, &page_txt);
        let plh = c.line_height(font::SEMIBOLD, 12.0);
        c.text(font::SEMIBOLD, 12.0, &page_txt, cx - pw / 2.0, pill_y + (pill_h - plh) / 2.0, theme::TEXT_STRONG);
        let chev_col = if pages > 1 { theme::TEXT_SECONDARY } else { theme::alpha(theme::TEXT_MUTED, 0.45) };
        let over_prev = pages > 1 && ui.hover(c, pill_x + 2.0, pill_y, 28.0, pill_h);
        let over_next = pages > 1 && ui.hover(c, pill_x + pill_w - 30.0, pill_y, 28.0, pill_h);
        c.icon(theme::I_CHEVRON_LEFT, 13.0, pill_x + 16.0, pill_y + pill_h / 2.0, if over_prev { theme::TEXT_STRONG } else { chev_col });
        c.icon(theme::I_CHEVRON_RIGHT, 13.0, pill_x + pill_w - 16.0, pill_y + pill_h / 2.0, if over_next { theme::TEXT_STRONG } else { chev_col });
        let page_clicked = (pages > 1 && ui.clicked(c, "emote:prev", pill_x, pill_y, 32.0, pill_h) && {
            self.change_page(-1, pages);
            true
        }) || (pages > 1 && ui.clicked(c, "emote:next", pill_x + pill_w - 32.0, pill_y, 32.0, pill_h) && {
            self.change_page(1, pages);
            true
        });

        let hover_elapsed = self.hover(slot_at(ui.mx - cx, ui.my - cy, inner, outer), ui.now);
        let (skin, slim) = preview3d::wheel_skin(&native_emotes::local_name());

        // Smooth dark translucent circular ring (no cluttered grid lines).
        sector(c, cx, cy, inner, outer, 0.0, std::f32::consts::TAU, 0xD812151C);
        sector(c, cx, cy, outer - 1.2, outer, 0.0, std::f32::consts::TAU, 0x1EFFFFFF);
        sector(c, cx, cy, inner, inner + 1.2, 0.0, std::f32::consts::TAU, 0x22FFFFFF);
        // Thin divider lines between the eight slots.
        for i in 0..8 {
            let ang = i as f32 * std::f32::consts::FRAC_PI_4 - std::f32::consts::FRAC_PI_2 - std::f32::consts::FRAC_PI_8;
            spoke(c, cx, cy, inner + 1.5, outer - 1.5, ang, 1.2, 0x33FFFFFF);
        }

        // Hovered sector translucent highlight + crisp white inner rim arc indicator.
        if let Some(slot) = self.selected {
            let angle = slot as f32 * std::f32::consts::FRAC_PI_4 - std::f32::consts::FRAC_PI_2;
            let a = angle - std::f32::consts::FRAC_PI_4 * 0.5;
            let b = angle + std::f32::consts::FRAC_PI_4 * 0.5;
            sector(c, cx, cy, inner + 1.0, outer - 1.0, a, b, 0x24FFFFFF);
            sector(c, cx, cy, inner - 0.5, inner + 2.8, a + 0.02, b - 0.02, 0xFFFFFFFF);
            // Tiny outward pointer chevron at the midpoint of the inner rim arc
            let px = cx + angle.cos() * (inner + 5.0);
            let py = cy + angle.sin() * (inner + 5.0);
            c.circle(px, py, 2.2, 0xFFFFFFFF);
        }

        let mut selected_name: Option<String> = None;
        for slot in 0..8 {
            let angle = slot as f32 * std::f32::consts::FRAC_PI_4 - std::f32::consts::FRAC_PI_2;
            let active = self.selected == Some(slot);
            let item = me["pages"][self.page][slot]
                .as_str()
                .and_then(|id| me["catalog"].as_array()?.iter().find(|i| i["id"] == id));
            if active
                && let Some(n) = item.and_then(|i| i["name"].as_str())
            {
                selected_name = Some(n.replace(['_', '-'], " "));
            }
            let (x, y) = (
                cx + angle.cos() * outer * 0.67,
                cy + angle.sin() * outer * 0.67,
            );
            let h0 = outer * 0.36;
            let label_y = y + h0 * 0.44;
            let mut drawn = false;
            if let Some(hash) = item.and_then(|i| i["hash"].as_str())
                && let Some(pose) = native_emotes::preview_pose(hash, if active { Some(hover_elapsed) } else { None })
            {
                let rect = fit_poster([0.,0.,1.,1.], 0.75, cx, cy, inner, outer, angle, label_y);
                preview3d::fit_emote(hash, slim);
                preview3d::draw_wheel(c, &skin, slim, pose, rect);
                drawn = true;
            }
            if !drawn && let Some(item) = item
                && let Some(url) = poster_url(item, &api)
                && let net::Pic::Ready(img) = net::image(&url, 512 * 1024)
            {
                if self.bounds.len() > 256 {
                    self.bounds.clear();
                }
                let bounds = *self
                    .bounds
                    .entry(img.id)
                    .or_insert_with(|| opaque_bounds(&img));
                let aspect = if img.height > 0 {
                    img.width as f32 / img.height as f32
                } else {
                    300.0 / 340.0
                };
                let [rx, ry, rw, rh] =
                    fit_poster(bounds, aspect, cx, cy, inner, outer, angle, label_y);
                c.image(
                    &img,
                    rx,
                    ry,
                    rw,
                    rh,
                    [0.0, 0.0, 1.0, 1.0],
                    0xffffffff,
                );
                drawn = true;
            }
            // An empty slot (or one whose art is still loading) shows the smiley, as the Java wheel does.
            if !drawn {
                c.icon(
                    theme::I_SMILE,
                    (outer * 0.13).min(24.0),
                    x,
                    y,
                    if active { theme::TEXT_STRONG } else { theme::TEXT_MUTED },
                );
            }
            // Small slot number 1..8 cleanly tucked near the outer rim.
            let num_angle = angle - 0.18;
            let num_r = outer * 0.90;
            let num = (slot + 1).to_string();
            let nw = c.text_width(font::MEDIUM, 10.0, &num);
            let nh = c.line_height(font::MEDIUM, 10.0);
            c.text(
                font::MEDIUM,
                10.0,
                &num,
                cx + num_angle.cos() * num_r - nw / 2.0,
                cy + num_angle.sin() * num_r - nh / 2.0,
                if active { theme::TEXT_STRONG } else { 0x88A1A1AA },
            );
        }

        // Center inner circle with Native emblem (clicking closes/cancels).
        let over_center = (ui.mx - cx).powi(2) + (ui.my - cy).powi(2) < inner * inner;
        c.circle(cx, cy, inner - 1.0, if over_center { 0xA8161922 } else { 0x7A0E1016 });
        let emblem = icons::for_module("watermark", theme::I_SPARKLES);
        c.icon(emblem, (inner * 0.72).clamp(24.0, 56.0), cx, cy, if over_center { 0xFFFFFFFF } else { 0xE6FFFFFF });

        // Large bold selected emote name centered below the wheel (or quiet hint when empty).
        let footer = cy + outer + 20.0;
        if let Some(name) = selected_name {
            let fs = 20.0;
            let tw = c.text_width(font::BOLD, fs, &name);
            c.text(font::BOLD, fs, &name, cx - tw / 2.0 + 1.5, footer + 1.5, 0x99000000);
            c.text(font::BOLD, fs, &name, cx - tw / 2.0, footer, 0xFFFFFFFF);
        } else {
            let hint = if me.is_null() {
                horizon_client::lang::get("native.emotes.signin")
            } else if self.selected.is_some() {
                horizon_client::lang::get("native.emotes.empty")
            } else {
                horizon_client::lang::get("native.emotes.hint")
            };
            let hint = c.ellipsize(font::MEDIUM, 11.5, &hint, c.width() - 32.0);
            let tw = c.text_width(font::MEDIUM, 11.5, &hint);
            c.text(font::MEDIUM, 11.5, &hint, cx - tw / 2.0, footer + 6.0, theme::TEXT_MUTED);
        }

        if !page_clicked && ui.pressed && self.selected.is_some() {
            return self.select();
        }
        if !page_clicked && ui.pressed && over_center {
            self.open = false;
            self.selected = None;
            return true;
        }
        false
    }
    pub fn select(&mut self) -> bool {
        if let Some(slot) = self.selected
            && native_emotes::play(self.page, slot)
        {
            self.selected = None;
            self.open = false;
            return true;
        }
        false
    }
}
#[cfg(test)]
mod tests {
    use super::{
        Image, Wheel, fit_poster, inside_sector, opaque_bounds, poster_url, radius, slot_at,
        wrap_page,
    };
    #[test]
    fn each_hover_starts_its_own_preview_clock() {
        let mut wheel = Wheel::default();
        assert_eq!(wheel.hover(Some(1), 1000.0), 0.0);
        assert_eq!(wheel.hover(Some(1), 2250.0), 1250.0);
        assert_eq!(wheel.hover(Some(2), 2300.0), 0.0);
        wheel.hover(None, 2400.0);
        assert_eq!(wheel.hover(Some(2), 2500.0), 0.0);
    }
    #[test]
    fn poster_urls_request_v2_and_preserve_explicit_versions() {
        let hash = "a".repeat(64);
        assert_eq!(
            poster_url(
                &serde_json::json!({"posterUrl": "https://api.playnative.net/v1/emotes/posters/bow.png"}),
                "https://api.playnative.net",
            ),
            Some("https://api.playnative.net/v1/emotes/posters/bow.png?v=2".into()),
        );
        assert_eq!(
            poster_url(
                &serde_json::json!({"posterUrl": "https://api.playnative.net/v1/emotes/posters/bow.png?v=3"}),
                "https://api.playnative.net",
            ),
            Some("https://api.playnative.net/v1/emotes/posters/bow.png?v=3".into()),
        );
        assert_eq!(
            poster_url(
                &serde_json::json!({"clipUrl": "https://api.playnative.net/v1/emotes/clips/bow"}),
                "https://api.playnative.net",
            ),
            Some("https://api.playnative.net/v1/emotes/posters/bow.png?v=2".into()),
        );
        assert_eq!(
            poster_url(
                &serde_json::json!({"hash": hash}),
                "https://api.playnative.net/",
            ),
            Some(format!("https://api.playnative.net/v1/emotes/posters/{hash}.png?v=2")),
        );
    }
    #[test]
    fn posters_stay_inside_every_sector_across_window_sizes() {
        let mut pixels = vec![0u32; 30 * 34];
        for y in 1..26 {
            for x in 6..25 {
                pixels[y * 30 + x] = 0xff604020;
            }
        }
        let bow = Image::new(30, 34, pixels);
        let bow_bounds = opaque_bounds(&bow);
        assert!((bow_bounds[0] - 6.0 / 30.0).abs() < 1e-5);
        assert!((bow_bounds[1] - 1.0 / 34.0).abs() < 1e-5);
        for (w, h) in [(320.0, 240.0), (854.0, 480.0), (500.0, 550.0), (1920.0, 1080.0)] {
            let outer = radius(w, h);
            let inner = outer * 0.27;
            let (cx, cy) = (w * 0.5, h * 0.5 - 4.0);
            for bounds in [bow_bounds, [0.12, 0.24, 0.88, 0.86], [0.0, 0.0, 1.0, 1.0]] {
                for slot in 0..8 {
                    let angle =
                        slot as f32 * std::f32::consts::FRAC_PI_4 - std::f32::consts::FRAC_PI_2;
                    let sy = cy + angle.sin() * outer * 0.65;
                    let label_y = sy + outer * 0.34 * 0.38;
                    let [rx, ry, rw, rh] =
                        fit_poster(bounds, 300.0 / 340.0, cx, cy, inner, outer, angle, label_y);
                    assert!(rw > 0.0 && rh > 0.0);
                    let (ix0, iy0) = (rx + bounds[0] * rw, ry + bounds[1] * rh);
                    let (ix1, iy1) = (rx + bounds[2] * rw, ry + bounds[3] * rh);
                    assert!(iy1 <= label_y + 1e-4);
                    for (px, py) in [
                        (ix0, iy0),
                        (ix1, iy0),
                        (ix0, iy1),
                        (ix1, iy1),
                        (cx.clamp(ix0, ix1), cy.clamp(iy0, iy1)),
                    ] {
                        assert!(inside_sector(px, py, cx, cy, inner, outer, angle, 0.0));
                    }
                }
            }
        }
    }
    #[test]
    fn fits_short_and_narrow_windows() {
        for (w, h) in [(320.0, 240.0), (500.0, 550.0), (1920.0, 1080.0), (240.0, 500.0)] {
            let r = radius(w, h);
            assert!(r * 2.0 <= w - 32.0);
            assert!(r * 2.0 <= h - 116.0);
        }
    }
    #[test]
    fn paging_wraps_and_clears_selection() {
        assert_eq!(wrap_page(0, -1, 2), 1);
        assert_eq!(wrap_page(1, 1, 2), 0);
        assert_eq!(wrap_page(0, 1, 0), 0);
        let mut wheel = Wheel { selected: Some(2), ..Default::default() };
        wheel.change_page(1, 2);
        assert_eq!(wheel.page, 1);
        assert_eq!(wheel.selected, None);
    }
    #[test]
    fn tap_release_and_repeat_leave_wheel_open_until_explicit_dismissal() {
        let mut wheel = Wheel {
            open: true,
            selected: Some(2),
            ..Default::default()
        };
        assert!(wheel.key(true, false, false, false));
        assert!(wheel.open);
        assert_eq!(wheel.selected, Some(2));
        assert!(wheel.key(true, false, true, true));
        assert!(wheel.open);
        assert!(!wheel.key(false, false, true, false));
        assert!(wheel.open);
        assert!(wheel.key(true, false, true, false));
        assert!(!wheel.open);
        assert_eq!(wheel.selected, None);
        wheel.open = true;
        assert!(wheel.key(false, true, true, false));
        assert!(!wheel.open);
    }
    #[test]
    fn empty_selection_keeps_wheel_open() {
        let mut wheel = Wheel {
            open: true,
            ..Default::default()
        };
        assert!(!wheel.select());
        assert!(wheel.open);
    }
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
