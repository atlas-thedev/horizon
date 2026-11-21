//! `ModPreviews`: what the module page's live previews are made of: the real
//! in-game screenshot every preview is drawn over (the same one behind the HUD
//! editor), a way to put things on top of it exactly where they are in the
//! picture, and a few labels.
#![allow(dead_code)]

use horizon_client::native_ui::{Canvas, Image, assets, font, theme};
use std::sync::{Arc, OnceLock};

/// The in-game screenshot (1280 x 720), or `None` when it can't be read.
pub fn shot() -> Option<Arc<Image>> {
    static SHOT: OnceLock<Option<Arc<Image>>> = OnceLock::new();
    SHOT.get_or_init(|| Image::decode(assets::HUD_BACKGROUND)).clone()
}

/// Where the screenshot ended up on screen: maps a point of the picture (in pixels of the 1280 x 720 file) to the screen.
#[derive(Clone, Copy, Debug)]
pub struct View {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    u0: f32,
    v0: f32,
    uw: f32,
    vh: f32,
}

impl View {
    /// Screen x of picture pixel `px`.
    pub fn sx(&self, px: f32) -> f32 {
        self.x + (px / 1280.0 - self.u0) / self.uw * self.w
    }

    /// Screen y of picture pixel `py`.
    pub fn sy(&self, py: f32) -> f32 {
        self.y + (py / 720.0 - self.v0) / self.vh * self.h
    }

    /// How many screen units one picture pixel takes.
    pub fn k(&self) -> f32 {
        self.w / self.uw / 1280.0
    }
}

/// Fills the box with the screenshot like CSS "cover", magnified `zoom` times around the picture point
/// (`fx`, `fy`) in pixels (kept inside the picture). The caller clips to the box.
#[allow(clippy::too_many_arguments)]
pub fn backdrop(c: &mut Canvas, x: f32, y: f32, w: f32, h: f32, zoom: f32, fx: f32, fy: f32) -> View {
    let box_aspect = w / h;
    let img_aspect = 1280.0 / 720.0;
    let (mut uw, mut vh) = (1.0, 1.0);
    if img_aspect > box_aspect {
        uw = box_aspect / img_aspect;
    } else {
        vh = img_aspect / box_aspect;
    }
    uw /= zoom.max(1.0);
    vh /= zoom.max(1.0);
    let cu = (fx / 1280.0).clamp(uw / 2.0, 1.0 - uw / 2.0);
    let cv = (fy / 720.0).clamp(vh / 2.0, 1.0 - vh / 2.0);
    let (u0, v0) = (cu - uw / 2.0, cv - vh / 2.0);
    if let Some(img) = shot() {
        c.image(&img, x, y, w, h, [u0, v0, u0 + uw, v0 + vh], 0xFFFFFFFF);
    } else {
        c.gradient_v(x, y, w, h, 0xFF4F7FB8, 0xFF3C5F2E);
    }
    View { x, y, w, h, u0, v0, uw, vh }
}

/// Two views to compare ("Vanilla" and the module), each with room for a label below it: side by side in a
/// wide box, one above the other in a tall one, so neither ends up small with empty space around it. Returns
/// x, y, w, h for each view. `aspect` is the height / width the views look best at.
pub fn pair(x: f32, y: f32, w: f32, h: f32, aspect: f32) -> [[f32; 4]; 2] {
    let (pad, label) = (12.0, 30.0);
    let y = y + 26.0; // Keep clear of the PREVIEW tag in the corner.
    let h = h - 26.0;
    let side_w = (w - pad * 3.0) / 2.0;
    let side_h = (h - pad * 2.0 - label).min(side_w * aspect);
    let stack_w = w - pad * 2.0;
    let stack_h = ((h - pad * 3.0 - label * 2.0) / 2.0).min(stack_w * aspect);
    if stack_h > side_h {
        let top = y + (h - (stack_h * 2.0 + label * 2.0 + pad)) / 2.0;
        return [[x + pad, top, stack_w, stack_h], [x + pad, top + stack_h + label + pad, stack_w, stack_h]];
    }
    let top = y + (h - side_h - label) / 2.0;
    [[x + pad, top, side_w, side_h], [x + pad * 2.0 + side_w, top, side_w, side_h]]
}

/// A small rounded label centred on `cx`.
pub fn chip(c: &mut Canvas, text: &str, cx: f32, y: f32, fg: u32) {
    let tw = c.text_width(font::SEMIBOLD, 10.5, text) + 20.0;
    c.round(cx - tw / 2.0, y, tw, 22.0, 11.0, 0xC0000000);
    c.outline(cx - tw / 2.0, y, tw, 22.0, 11.0, 1.0, theme::HAIRLINE_STRONG);
    let lh = c.line_height(font::SEMIBOLD, 10.5);
    c.text(font::SEMIBOLD, 10.5, text, cx - tw / 2.0 + 10.0, y + (22.0 - lh) / 2.0, fg);
}

/// The little caption under one of a [`pair`]'s views.
pub fn caption(c: &mut Canvas, view: &[f32; 4], text: &str, good: bool) {
    let [vx, top, half, vh] = *view;
    let tw = c.text_width(font::SEMIBOLD, 10.5, text) + 18.0;
    c.round(vx + (half - tw) / 2.0, top + vh + 8.0, tw, 20.0, 10.0, 0xC0000000);
    let lh = c.line_height(font::SEMIBOLD, 10.5);
    c.text(
        font::SEMIBOLD,
        10.5,
        text,
        vx + (half - tw) / 2.0 + 9.0,
        top + vh + 8.0 + (20.0 - lh) / 2.0,
        if good { theme::ONLINE } else { theme::TEXT_SECONDARY },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_backdrop_keeps_the_picture_inside_the_box() {
        let mut c = horizon_client::native_ui::canvas();
        c.begin(800, 600, 1.0);
        let v = backdrop(&mut c, 10.0, 10.0, 200.0, 200.0, 2.0, 0.0, 0.0);
        // Zoomed at a corner the view is kept inside the picture.
        assert!(v.sx(0.0) <= 10.0 + 0.01);
        assert!(v.k() > 0.0);
        let [a, b] = pair(0.0, 0.0, 600.0, 300.0, 0.7);
        assert!(a[0] < b[0] || a[1] < b[1]);
    }
}
