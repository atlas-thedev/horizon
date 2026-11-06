//! `Scroll`: smooth vertical scrolling for a clipped region, with a slim
//! launcher-style scrollbar.
#![allow(dead_code)]

use super::ui::Ui;
use horizon_client::native_ui::{Canvas, theme};

#[derive(Default)]
pub struct Scroll {
    pub offset: f32,
    pub target: f32,
    pub content: f32,
    view: [f32; 4],
    dragging: bool,
    drag_from: f32,
    drag_offset: f32,
    /// Stick to the bottom when new content arrives (chat).
    pub stick_bottom: bool,
    /// Pinned to the bottom this frame (decided before the wheel moves anything).
    pinned: bool,
    /// The scrollbar's hover animation id.
    id: String,
}

impl Scroll {
    pub fn new(id: &str) -> Self {
        Scroll {
            id: format!("scroll#{id}"),
            ..Scroll::default()
        }
    }

    /// Begins the region: pushes a clip and returns the y offset to subtract from content.
    pub fn begin(&mut self, ui: &Ui, c: &mut Canvas, x: f32, y: f32, w: f32, h: f32) -> f32 {
        self.view = [x, y, w, h];
        // Pinned = the target sits at the bottom (read before the wheel moves it).
        self.pinned = self.stick_bottom && self.content > 0.0 && self.target >= (self.content - h).max(0.0) - 2.0;
        if ui.hover(c, x, y, w, h) && ui.scroll != 0.0 {
            self.target -= ui.scroll * 48.0;
            self.pinned = false;
        }
        c.push_clip(x, y, w, h);
        self.offset
    }

    /// Ends the region with the content height measured while drawing.
    pub fn end(&mut self, ui: &mut Ui, c: &mut Canvas, content_height: f32) {
        c.pop_clip();
        let [vx, vy, vw, vh] = self.view;
        let max = (content_height - vh).max(0.0);
        let grew = content_height > self.content + 0.5;
        self.content = content_height;
        if self.pinned && !self.dragging {
            // Stuck to the bottom: follow growing content at once instead of gliding after it.
            self.target = max;
            if grew {
                self.offset = max;
            }
        }
        self.target = self.target.clamp(0.0, max);
        let k = 1.0 - (-18.0 * ui.dt).exp();
        self.offset += (self.target - self.offset) * k;
        if (self.target - self.offset).abs() < 0.5 {
            self.offset = self.target;
        }
        if max <= 0.0 {
            self.offset = 0.0;
            self.target = 0.0;
            return;
        }
        let track_h = vh - 8.0;
        let thumb_h = (track_h * vh / content_height).max(24.0);
        let thumb_y = vy + 4.0 + (track_h - thumb_h) * (self.offset / max);
        let tx = vx + vw - 6.0;
        let over = ui.hover(c, tx - 4.0, vy, 10.0, vh);
        if over && ui.pressed {
            self.dragging = true;
            self.drag_from = ui.my;
            self.drag_offset = self.target;
        }
        if !ui.down {
            self.dragging = false;
        }
        if self.dragging {
            self.target = (self.drag_offset + (ui.my - self.drag_from) * (content_height / track_h.max(1.0))).clamp(0.0, max);
            self.offset = self.target;
        }
        let id = if self.id.is_empty() { "scroll" } else { &self.id };
        let a = ui.anim_on(id, over || self.dragging, 12.0);
        c.round(tx, thumb_y, 4.0, thumb_h, 2.0, theme::mix(0x29FFFFFF, 0x47FFFFFF, a));
    }

    /// Content was added above what is on screen: move by the same amount so nothing jumps.
    pub fn shift(&mut self, dy: f32) {
        self.offset += dy;
        self.target += dy;
    }

    /// Jump straight to the bottom (no glide) for a view of height `view_h`.
    pub fn snap_bottom(&mut self, view_h: f32) {
        let max = (self.content - view_h).max(0.0);
        self.target = max;
        self.offset = max;
    }

    pub fn to_bottom(&mut self) {
        self.target = 1e9;
    }

    pub fn at_bottom(&self) -> bool {
        self.target >= (self.content - self.view[3]).max(0.0) - 4.0
    }

    pub fn is_dragging(&self) -> bool {
        self.dragging
    }

    pub fn reset(&mut self) {
        self.offset = 0.0;
        self.target = 0.0;
    }
}
