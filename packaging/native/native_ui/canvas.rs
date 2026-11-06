//! `Canvas`: the immediate-mode 2D painter of the Native UI. Everything is
//! described in logical units, converted to physical pixels with
//! [`Canvas::scale`], clipped on the CPU and collected as textured quads in
//! batches (one per texture). [`Canvas::finish`] turns them into the HUD's
//! vertices: runs drawn from the Native UI's own texture pages
//! (`Blend::Native`), page 0 the atlas, the others whole images.

use super::atlas::Atlas;
use super::font::{self, Fonts};
use super::icons;
use super::image::Image;
use super::shapes::Shapes;
use crate::hud::{Blend, HudBatch, HudVertex, NativeUpload};
use std::collections::HashMap;
use std::sync::Arc;

/// A quad's texture: the atlas, or an image with a page of its own.
#[derive(Clone)]
enum Tex {
    Atlas,
    Image(Arc<Image>),
}

impl Tex {
    fn same(&self, other: &Tex) -> bool {
        match (self, other) {
            (Tex::Atlas, Tex::Atlas) => true,
            (Tex::Image(a), Tex::Image(b)) => a.id == b.id,
            _ => false,
        }
    }
}

#[derive(Clone, Copy)]
enum Geo {
    /// Axis-aligned: x0 y0 x1 y1, u0 v0 u1 v1 (physical pixels, 0..1).
    Rect([f32; 8]),
    /// Four corners (physical pixels) and their UVs, in order around the quad.
    Free([f32; 8], [f32; 8]),
}

struct Quad {
    geo: Geo,
    /// ARGB: top left, top right, bottom right, bottom left.
    col: [u32; 4],
}

struct Batch {
    tex: Tex,
    start: usize,
}

/// Something the game itself draws at a place on the canvas (an item icon, a
/// potion icon): the canvas keeps its place in the draw order and hands it
/// to the host in [`Canvas::finish_with`]. Positions are in the canvas's logical units.
#[derive(Clone, Debug)]
pub struct HostDraw {
    pub kind: u32,
    pub key: u64,
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub argb: u32,
}

/// `Canvas.mulAlpha`.
pub fn mul_alpha(argb: u32, a: f32) -> u32 {
    let al = ((argb >> 24) as f32 * a).round() as u32;
    (al.min(255) << 24) | (argb & 0xFFFFFF)
}

/// `Canvas.lerpColor`: blends two ARGB colours in premultiplied space, so
/// fading a translucent colour into an opaque one does not pass through a
/// bright grey on the way.
pub fn lerp_color(a: u32, b: u32, t: f32) -> u32 {
    let (aa, ba) = ((a >> 24) as f32, (b >> 24) as f32);
    let al = aa + (ba - aa) * t;
    if al <= 0.0001 {
        return 0;
    }
    let mut out = (al.round() as u32) << 24;
    for sh in [16, 8, 0] {
        let ca = ((a >> sh) & 0xFF) as f32 * aa;
        let cb = ((b >> sh) & 0xFF) as f32 * ba;
        let v = ((ca + (cb - ca) * t) / al).round() as i32;
        out |= (v.clamp(0, 255) as u32) << sh;
    }
    out
}

/// ARGB to the HUD's vertex colour (RGBA bytes, red first).
fn vertex_color(argb: u32) -> u32 {
    let (a, r, g, b) = (argb >> 24, (argb >> 16) & 0xFF, (argb >> 8) & 0xFF, argb & 0xFF);
    a << 24 | b << 16 | g << 8 | r
}

/// Images drawn from pages of their own: which page each holds, and when it was last drawn.
#[derive(Default)]
pub struct Pages {
    by_image: HashMap<u64, (u16, u64)>,
    free: Vec<u16>,
    next: u16,
    frame: u64,
    atlas_uploaded: bool,
}

/// Pages not drawn for this many frames are freed (`TextureBudget`'s idle rule, by frame).
const PAGE_IDLE_FRAMES: u64 = 1200;

pub struct Canvas {
    pub atlas: Atlas,
    pub fonts: Fonts,
    shapes: Shapes,
    pub scale: f32,
    pub fb_w: u32,
    pub fb_h: u32,
    quads: Vec<Quad>,
    batches: Vec<Batch>,
    clip: Vec<[i32; 4]>,
    alpha: f32,
    alpha_stack: Vec<f32>,
    /// Atlas regions of stamped images: per key, runs of (w, h, x, y).
    stamped: HashMap<u64, Vec<[u32; 4]>>,
    stamped_generation: Option<u32>,
    logo: Option<Arc<Image>>,
    pages: Pages,
    host: Vec<(usize, HostDraw)>,
}

/// The key the Native logo is stamped under.
const LOGO_KEY: u64 = u64::MAX;

impl Canvas {
    pub fn new(atlas: Atlas, fonts: Fonts, logo: Option<Arc<Image>>) -> Self {
        Canvas {
            atlas,
            fonts,
            shapes: Shapes::default(),
            scale: 1.0,
            fb_w: 0,
            fb_h: 0,
            quads: Vec::with_capacity(4096),
            batches: Vec::with_capacity(64),
            clip: vec![[0, 0, 0, 0]],
            alpha: 1.0,
            alpha_stack: Vec::new(),
            stamped: HashMap::new(),
            stamped_generation: None,
            logo,
            pages: Pages::default(),
            host: Vec::new(),
        }
    }

    pub fn begin(&mut self, fb_w: u32, fb_h: u32, scale: f32) {
        if self.atlas.is_full() {
            self.atlas.reset();
        }
        self.fb_w = fb_w;
        self.fb_h = fb_h;
        self.scale = scale;
        self.quads.clear();
        self.batches.clear();
        self.host.clear();
        self.clip.clear();
        self.clip.push([0, 0, fb_w as i32, fb_h as i32]);
        self.alpha = 1.0;
        self.alpha_stack.clear();
    }

    pub fn width(&self) -> f32 {
        self.fb_w as f32 / self.scale
    }

    pub fn height(&self) -> f32 {
        self.fb_h as f32 / self.scale
    }

    pub fn px(&self, v: f32) -> i32 {
        (v * self.scale).round() as i32
    }

    // ── state ────────────────────────────────────────────────────────────

    pub fn push_clip(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let (x0, y0, x1, y1) = (self.px(x), self.px(y), self.px(x + w), self.px(y + h));
        let b = *self.clip.last().unwrap_or(&[0, 0, 0, 0]);
        if self.clip.len() < 32 {
            self.clip.push([b[0].max(x0), b[1].max(y0), b[2].min(x1), b[3].min(y1)]);
        }
    }

    pub fn pop_clip(&mut self) {
        if self.clip.len() > 1 {
            self.clip.pop();
        }
    }

    /// Multiplies the alpha of everything drawn until [`Canvas::pop_alpha`].
    pub fn push_alpha(&mut self, a: f32) {
        if self.alpha_stack.len() < 32 {
            self.alpha_stack.push(self.alpha);
        }
        self.alpha *= a.clamp(0.0, 1.0);
    }

    pub fn pop_alpha(&mut self) {
        if let Some(a) = self.alpha_stack.pop() {
            self.alpha = a;
        }
    }

    /// True when a logical point is inside the current clip (for hit testing).
    pub fn in_clip(&self, x: f32, y: f32) -> bool {
        let b = *self.clip.last().unwrap_or(&[0, 0, 0, 0]);
        let (px, py) = (x * self.scale, y * self.scale);
        px >= b[0] as f32 && px < b[2] as f32 && py >= b[1] as f32 && py < b[3] as f32
    }

    // ── primitives ───────────────────────────────────────────────────────

    pub fn fill(&mut self, x: f32, y: f32, w: f32, h: f32, argb: u32) {
        let (x0, y0, x1, y1) = (self.px(x), self.px(y), self.px(x + w), self.px(y + h));
        self.solid(x0, y0, x1, y1, [argb; 4]);
    }

    pub fn gradient_v(&mut self, x: f32, y: f32, w: f32, h: f32, top: u32, bottom: u32) {
        let (x0, y0, x1, y1) = (self.px(x), self.px(y), self.px(x + w), self.px(y + h));
        self.solid(x0, y0, x1, y1, [top, top, bottom, bottom]);
    }

    pub fn gradient_h(&mut self, x: f32, y: f32, w: f32, h: f32, left: u32, right: u32) {
        let (x0, y0, x1, y1) = (self.px(x), self.px(y), self.px(x + w), self.px(y + h));
        self.solid(x0, y0, x1, y1, [left, right, right, left]);
    }

    fn solid(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: [u32; 4]) {
        let (u, v) = (self.atlas.white_u, self.atlas.white_v);
        self.quad(Tex::Atlas, [x0 as f32, y0 as f32, x1 as f32, y1 as f32, u, v, u, v], c);
    }

    /// A rounded block that pulses softly while its content is still loading.
    /// Every skeleton shares one clock, so a whole screen of them breathes together.
    pub fn skeleton(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32) {
        let t = ((clock_ms() / 420.0).sin() as f32 + 1.0) / 2.0;
        self.round(x, y, w, h, r, super::theme::mix(0xFF16171C, 0xFF1F2026, t));
    }

    pub fn round(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, argb: u32) {
        let (x0, y0, x1, y1, pr) = (self.px(x), self.px(y), self.px(x + w), self.px(y + h), self.px(r));
        self.nine(x0, y0, x1, y1, pr, 0, 0, argb);
    }

    pub fn outline(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, width: f32, argb: u32) {
        let (x0, y0, x1, y1, pr) = (self.px(x), self.px(y), self.px(x + w), self.px(y + h), self.px(r));
        let bw = self.px(width).max(1);
        self.nine(x0, y0, x1, y1, pr, 0, bw, argb);
    }

    /// A horizontal gradient inside a rounded box: the fade stays inside the radius.
    pub fn round_gradient_h(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, left: u32, right: u32) {
        let (x0, y0, x1, y1, pr) = (self.px(x), self.px(y), self.px(x + w), self.px(y + h), self.px(r));
        self.nine_gradient(x0, y0, x1, y1, pr, left, right);
    }

    fn nine_gradient(&mut self, sx0: i32, sy0: i32, sx1: i32, sy1: i32, r: i32, left: u32, right: u32) {
        let (w, h) = (sx1 - sx0, sy1 - sy0);
        if w <= 0 || h <= 0 {
            return;
        }
        let r = r.clamp(0, ((w.min(h) - 2) / 2).max(0));
        if r == 0 {
            let (u, v) = (self.atlas.white_u, self.atlas.white_v);
            self.quad(
                Tex::Atlas,
                [sx0 as f32, sy0 as f32, sx1 as f32, sy1 as f32, u, v, u, v],
                [left, right, right, left],
            );
            return;
        }
        let Some(s) = self.shapes.get(&mut self.atlas, r, 0, 0) else {
            return;
        };
        let m = 1;
        let (ox0, oy0, ox1, oy1) = (sx0 - m, sy0 - m, sx1 + m, sy1 + m);
        let c = s.c as i32;
        let inv = 1.0 / self.atlas.size as f32;
        let (u0, u1, u2, u3) = (
            s.x as f32 * inv,
            (s.x + s.c) as f32 * inv,
            (s.x + s.c + 2) as f32 * inv,
            (s.x + s.s) as f32 * inv,
        );
        let (v0, v1, v2, v3) = (
            s.y as f32 * inv,
            (s.y + s.c) as f32 * inv,
            (s.y + s.c + 2) as f32 * inv,
            (s.y + s.s) as f32 * inv,
        );
        let (xa, xb, ya, yb) = (ox0 + c, ox1 - c, oy0 + c, oy1 - c);
        let g = |me: &mut Self, x0: i32, y0: i32, x1: i32, y1: i32, uv: [f32; 4]| {
            me.grad_quad(x0, y0, x1, y1, uv, sx0, sx1, left, right);
        };
        g(self, ox0, oy0, xa, ya, [u0, v0, u1, v1]);
        g(self, xb, oy0, ox1, ya, [u2, v0, u3, v1]);
        g(self, ox0, yb, xa, oy1, [u0, v2, u1, v3]);
        g(self, xb, yb, ox1, oy1, [u2, v2, u3, v3]);
        if xb > xa {
            g(self, xa, oy0, xb, ya, [u1, v0, u2, v1]);
            g(self, xa, yb, xb, oy1, [u1, v2, u2, v3]);
        }
        if yb > ya {
            g(self, ox0, ya, xa, yb, [u0, v1, u1, v2]);
            g(self, xb, ya, ox1, yb, [u2, v1, u3, v2]);
        }
        if xb > xa && yb > ya {
            g(self, xa, ya, xb, yb, [u1, v1, u2, v2]);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn grad_quad(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, uv: [f32; 4], sx0: i32, sx1: i32, left: u32, right: u32) {
        let span = (sx1 - sx0).max(1) as f32;
        let cl = lerp_color(left, right, ((x0 - sx0) as f32 / span).clamp(0.0, 1.0));
        let cr = lerp_color(left, right, ((x1 - sx0) as f32 / span).clamp(0.0, 1.0));
        self.quad(
            Tex::Atlas,
            [x0 as f32, y0 as f32, x1 as f32, y1 as f32, uv[0], uv[1], uv[2], uv[3]],
            [cl, cr, cr, cl],
        );
    }

    /// Soft drop shadow around a rounded box; blur in logical units.
    pub fn shadow(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, blur: f32, argb: u32) {
        let (x0, y0, x1, y1, pr) = (self.px(x), self.px(y), self.px(x + w), self.px(y + h), self.px(r));
        let f = self.px(blur).max(1);
        self.nine(x0, y0, x1, y1, pr, f, 0, argb);
    }

    pub fn circle(&mut self, cx: f32, cy: f32, radius: f32, argb: u32) {
        self.round(cx - radius, cy - radius, radius * 2.0, radius * 2.0, radius, argb);
    }

    #[allow(clippy::too_many_arguments)]
    fn nine(&mut self, sx0: i32, sy0: i32, sx1: i32, sy1: i32, r: i32, f: i32, bw: i32, argb: u32) {
        let (w, h) = (sx1 - sx0, sy1 - sy0);
        if w <= 0 || h <= 0 {
            return;
        }
        let r = r.clamp(0, ((w.min(h) - 2) / 2).max(0));
        if r == 0 && f == 0 && bw == 0 {
            self.solid(sx0, sy0, sx1, sy1, [argb; 4]);
            return;
        }
        let Some(s) = self.shapes.get(&mut self.atlas, r, f, bw) else {
            return;
        };
        let m = f + 1;
        let (ox0, oy0, ox1, oy1) = (sx0 - m, sy0 - m, sx1 + m, sy1 + m);
        let c = s.c as i32;
        let inv = 1.0 / self.atlas.size as f32;
        let (u0, u1, u2, u3) = (
            s.x as f32 * inv,
            (s.x + s.c) as f32 * inv,
            (s.x + s.c + 2) as f32 * inv,
            (s.x + s.s) as f32 * inv,
        );
        let (v0, v1, v2, v3) = (
            s.y as f32 * inv,
            (s.y + s.c) as f32 * inv,
            (s.y + s.c + 2) as f32 * inv,
            (s.y + s.s) as f32 * inv,
        );
        let (xa, xb, ya, yb) = (ox0 + c, ox1 - c, oy0 + c, oy1 - c);
        let col = [argb; 4];
        let q = |x0: i32, y0: i32, x1: i32, y1: i32, uv: [f32; 4]| {
            [x0 as f32, y0 as f32, x1 as f32, y1 as f32, uv[0], uv[1], uv[2], uv[3]]
        };
        // Corners.
        self.quad(Tex::Atlas, q(ox0, oy0, xa, ya, [u0, v0, u1, v1]), col);
        self.quad(Tex::Atlas, q(xb, oy0, ox1, ya, [u2, v0, u3, v1]), col);
        self.quad(Tex::Atlas, q(ox0, yb, xa, oy1, [u0, v2, u1, v3]), col);
        self.quad(Tex::Atlas, q(xb, yb, ox1, oy1, [u2, v2, u3, v3]), col);
        // Edges.
        if xb > xa {
            self.quad(Tex::Atlas, q(xa, oy0, xb, ya, [u1, v0, u2, v1]), col);
            self.quad(Tex::Atlas, q(xa, yb, xb, oy1, [u1, v2, u2, v3]), col);
        }
        if yb > ya {
            self.quad(Tex::Atlas, q(ox0, ya, xa, yb, [u0, v1, u1, v2]), col);
            self.quad(Tex::Atlas, q(xb, ya, ox1, yb, [u2, v1, u3, v2]), col);
        }
        if !s.hollow && xb > xa && yb > ya {
            self.solid(xa, ya, xb, yb, col);
        }
    }

    // ── text ─────────────────────────────────────────────────────────────

    fn font_px(&self, size: f32) -> u32 {
        (size * self.scale).round().max(0.0) as u32
    }

    pub fn text_width(&mut self, face: usize, size: f32, text: &str) -> f32 {
        let px = self.font_px(size);
        self.fonts.width(face, px, text) / self.scale
    }

    /// Line height in logical units.
    pub fn line_height(&mut self, face: usize, size: f32) -> f32 {
        let px = self.font_px(size);
        self.fonts.metrics(face, px).2 / self.scale
    }

    /// Draws one line; `y` is the top of the line box. Returns the advance in logical units.
    pub fn text(&mut self, face: usize, size: f32, text: &str, x: f32, y: f32, argb: u32) -> f32 {
        let px = self.font_px(size);
        let (ascent, descent, line_height) = self.fonts.metrics(face, px);
        let mut pen = x * self.scale;
        let base = (y * self.scale + (line_height - (ascent + descent)) / 2.0 + ascent).round();
        let inv = 1.0 / self.atlas.size as f32;
        for c in text.chars() {
            let g = self.fonts.glyph(&mut self.atlas, face, px, c);
            if g.w > 0 {
                let gx = pen.round() as i32 + g.xoff;
                let gy = base as i32 + g.yoff;
                self.quad(
                    Tex::Atlas,
                    [
                        gx as f32,
                        gy as f32,
                        (gx + g.w as i32) as f32,
                        (gy + g.h as i32) as f32,
                        g.ax as f32 * inv,
                        g.ay as f32 * inv,
                        (g.ax + g.w) as f32 * inv,
                        (g.ay + g.h) as f32 * inv,
                    ],
                    [argb; 4],
                );
            }
            pen += g.advance;
        }
        (pen - x * self.scale) / self.scale
    }

    /// Text cut to fit `max_w` with an ellipsis.
    pub fn ellipsize(&mut self, face: usize, size: f32, text: &str, max_w: f32) -> String {
        let px = self.font_px(size);
        let max = max_w * self.scale;
        if self.fonts.width(face, px, text) <= max {
            return text.to_owned();
        }
        let dots = self.fonts.width(face, px, "\u{2026}");
        let mut w = 0.0;
        let mut end = 0;
        for (i, c) in text.char_indices() {
            let a = self.fonts.advance(face, px, c);
            if w + a + dots > max {
                break;
            }
            w += a;
            end = i + c.len_utf8();
        }
        format!("{}\u{2026}", &text[..end])
    }

    /// A Native module or navigation icon centred on (`cx`, `cy`).
    pub fn icon(&mut self, codepoint: u32, size: f32, cx: f32, cy: f32, argb: u32) {
        if icons::nav(codepoint) {
            let p = self.px(size).max(1) as u32;
            let s = p as f32 / self.scale;
            let nav = icons::nav_image(codepoint, p);
            let (x, y) = (
                (cx * self.scale - p as f32 / 2.0).round() / self.scale,
                (cy * self.scale - p as f32 / 2.0).round() / self.scale,
            );
            self.stamp(nav.id, &nav, x, y, s, s, false, argb);
            return;
        }
        if codepoint == icons::for_module("watermark", u32::MAX)
            && let Some(logo) = self.logo.clone()
        {
            self.stamp(LOGO_KEY, &logo, cx - size / 2.0, cy - size / 2.0, size, size, true, argb);
            return;
        }
        let Some(c) = char::from_u32(codepoint) else {
            return;
        };
        let face = if icons::custom(codepoint) { font::MODULE_ICONS } else { font::ICONS };
        let px = self.font_px(size);
        let g = self.fonts.glyph(&mut self.atlas, face, px, c);
        if g.w == 0 {
            return;
        }
        let inv = 1.0 / self.atlas.size as f32;
        // Centre the glyph's ink box on the point.
        let gx = (cx * self.scale - g.w as f32 / 2.0).round();
        let gy = (cy * self.scale - g.h as f32 / 2.0).round();
        self.quad(
            Tex::Atlas,
            [
                gx,
                gy,
                gx + g.w as f32,
                gy + g.h as f32,
                g.ax as f32 * inv,
                g.ay as f32 * inv,
                (g.ax + g.w) as f32 * inv,
                (g.ay + g.h) as f32 * inv,
            ],
            [argb; 4],
        );
    }

    // ── images ───────────────────────────────────────────────────────────

    /// An image with its own page, stretched over the box (`uv` sub-rectangle in 0..1).
    #[allow(clippy::too_many_arguments)]
    pub fn image(&mut self, img: &Arc<Image>, x: f32, y: f32, w: f32, h: f32, uv: [f32; 4], argb: u32) {
        let (x0, y0, x1, y1) = (self.px(x), self.px(y), self.px(x + w), self.px(y + h));
        self.quad(
            Tex::Image(img.clone()),
            [x0 as f32, y0 as f32, x1 as f32, y1 as f32, uv[0], uv[1], uv[2], uv[3]],
            [argb; 4],
        );
    }

    /// Covers the box with the image (CSS `background-size: cover`), zoomed around the centre.
    #[allow(clippy::too_many_arguments)]
    pub fn image_cover(&mut self, img: &Arc<Image>, x: f32, y: f32, w: f32, h: f32, zoom: f32, pan_x: f32, pan_y: f32, argb: u32) {
        let (box_aspect, img_aspect) = (w / h, img.width as f32 / img.height as f32);
        let (mut uw, mut vh) = (1.0, 1.0);
        if img_aspect > box_aspect {
            uw = box_aspect / img_aspect;
        } else {
            vh = img_aspect / box_aspect;
        }
        uw /= zoom;
        vh /= zoom;
        let (cu, cv) = (0.5 + pan_x * (1.0 - uw) / 2.0, 0.5 + pan_y * (1.0 - vh) / 2.0);
        self.image(img, x, y, w, h, [cu - uw / 2.0, cv - vh / 2.0, cu + uw / 2.0, cv + vh / 2.0], argb);
    }

    /// Draws a small image through the atlas at exactly its on-screen pixel
    /// size (logos, player faces). `smooth`: area-averaged resample; otherwise
    /// nearest (pixel art).
    #[allow(clippy::too_many_arguments)]
    pub fn stamp(&mut self, key: u64, src: &Arc<Image>, x: f32, y: f32, w: f32, h: f32, smooth: bool, argb: u32) {
        let (x0, y0, x1, y1) = (self.px(x), self.px(y), self.px(x + w), self.px(y + h));
        let (pw, ph) = (x1 - x0, y1 - y0);
        if pw <= 0 || ph <= 0 {
            return;
        }
        if self.stamped_generation != Some(self.atlas.generation) {
            self.stamped.clear();
            self.stamped_generation = Some(self.atlas.generation);
        }
        let (bw, bh) = if smooth { (pw as u32, ph as u32) } else { (src.width, src.height) };
        let known = self
            .stamped
            .get(&key)
            .and_then(|runs| runs.iter().find(|r| r[0] == bw && r[1] == bh))
            .map(|r| [r[2], r[3]]);
        let [ax, ay] = match known {
            Some(at) => at,
            None => {
                let scaled;
                let pixels = if smooth {
                    scaled = src.scaled(bw, bh);
                    &scaled.argb
                } else {
                    &src.argb
                };
                let Some(at) = self.atlas.alloc(bw, bh) else {
                    return;
                };
                self.atlas.put_argb(at[0], at[1], bw, bh, pixels);
                self.stamped.entry(key).or_default().push([bw, bh, at[0], at[1]]);
                at
            }
        };
        let inv = 1.0 / self.atlas.size as f32;
        self.quad(
            Tex::Atlas,
            [
                x0 as f32,
                y0 as f32,
                x1 as f32,
                y1 as f32,
                ax as f32 * inv,
                ay as f32 * inv,
                (ax + bw) as f32 * inv,
                (ay + bh) as f32 * inv,
            ],
            [argb; 4],
        );
    }

    // ── core ─────────────────────────────────────────────────────────────

    fn push(&mut self, tex: Tex, geo: Geo, col: [u32; 4]) {
        if self.batches.last().is_none_or(|b| !b.tex.same(&tex)) {
            self.batches.push(Batch {
                tex,
                start: self.quads.len(),
            });
        }
        self.quads.push(Quad { geo, col });
    }

    /// An axis-aligned quad, clipped to the current clip on the CPU (UVs and
    /// a vertical gradient's colours follow the cut).
    fn quad(&mut self, tex: Tex, q: [f32; 8], c: [u32; 4]) {
        let [mut x0, mut y0, mut x1, mut y1, mut u0, mut v0, mut u1, mut v1] = q;
        let [mut c0, mut c1, mut c2, mut c3] = c;
        let b = *self.clip.last().unwrap_or(&[0, 0, 0, 0]);
        let (cx0, cy0, cx1, cy1) = (b[0] as f32, b[1] as f32, b[2] as f32, b[3] as f32);
        if x1 <= cx0 || x0 >= cx1 || y1 <= cy0 || y0 >= cy1 || x1 <= x0 || y1 <= y0 {
            return;
        }
        if x0 < cx0 {
            u0 += (u1 - u0) * (cx0 - x0) / (x1 - x0);
            x0 = cx0;
        }
        if x1 > cx1 {
            u1 -= (u1 - u0) * (x1 - cx1) / (x1 - x0);
            x1 = cx1;
        }
        if y0 < cy0 {
            let t = (cy0 - y0) / (y1 - y0);
            v0 += (v1 - v0) * t;
            if c0 != c3 || c1 != c2 {
                c0 = lerp_color(c0, c3, t);
                c1 = lerp_color(c1, c2, t);
            }
            y0 = cy0;
        }
        if y1 > cy1 {
            let t = (y1 - cy1) / (y1 - y0);
            v1 -= (v1 - v0) * t;
            if c0 != c3 || c1 != c2 {
                c3 = lerp_color(c3, c0, t);
                c2 = lerp_color(c2, c1, t);
            }
            y1 = cy1;
        }
        if self.alpha < 1.0 {
            c0 = mul_alpha(c0, self.alpha);
            c1 = mul_alpha(c1, self.alpha);
            c2 = mul_alpha(c2, self.alpha);
            c3 = mul_alpha(c3, self.alpha);
        }
        if (c0 | c1 | c2 | c3) >> 24 == 0 {
            return;
        }
        self.push(tex, Geo::Rect([x0, y0, x1, y1, u0, v0, u1, v1]), [c0, c1, c2, c3]);
    }

    /// A textured quad with four arbitrary corners (logical units, in order
    /// around the quad), used by the 3D player preview. Not clipped per pixel:
    /// it is dropped when its bounds leave the clip rectangle.
    pub fn free_quad(&mut self, img: Option<&Arc<Image>>, xy: [f32; 8], uv: [f32; 8], argb: u32) {
        let s = self.scale;
        let xy = xy.map(|v| v * s);
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for i in 0..4 {
            min_x = min_x.min(xy[i * 2]);
            max_x = max_x.max(xy[i * 2]);
            min_y = min_y.min(xy[i * 2 + 1]);
            max_y = max_y.max(xy[i * 2 + 1]);
        }
        let b = *self.clip.last().unwrap_or(&[0, 0, 0, 0]);
        if max_x <= b[0] as f32 || min_x >= b[2] as f32 || max_y <= b[1] as f32 || min_y >= b[3] as f32 {
            return;
        }
        let color = if self.alpha < 1.0 { mul_alpha(argb, self.alpha) } else { argb };
        if color >> 24 == 0 {
            return;
        }
        let tex = img.map_or(Tex::Atlas, |i| Tex::Image(i.clone()));
        // Without an image the quad is solid: the atlas's white texel.
        let uv = if img.is_none() {
            let (u, v) = (self.atlas.white_u, self.atlas.white_v);
            [u, v, u, v, u, v, u, v]
        } else {
            uv
        };
        self.push(tex, Geo::Free(xy, uv), [color; 4]);
    }

    /// A square the game draws itself (`kind` and `key` tell the host what) at its place in the
    /// draw order. Dropped unless it lies fully inside the current clip.
    pub fn host(&mut self, kind: u32, key: u64, x: f32, y: f32, size: f32, argb: u32) {
        let b = *self.clip.last().unwrap_or(&[0, 0, 0, 0]);
        let (x0, y0, x1, y1) = (self.px(x), self.px(y), self.px(x + size), self.px(y + size));
        if x0 < b[0] || y0 < b[1] || x1 > b[2] || y1 > b[3] {
            return;
        }
        let argb = if self.alpha < 1.0 { mul_alpha(argb, self.alpha) } else { argb };
        if argb >> 24 == 0 {
            return;
        }
        self.host.push((self.quads.len(), HostDraw { kind, key, x, y, size, argb }));
    }

    /// Hands the frame to the HUD: the atlas's changed pixels and new images
    /// as page uploads, then every quad as vertices, batch by batch, each run
    /// marked with the page it samples.
    pub fn finish(&mut self, batch: &mut HudBatch) {
        self.finish_with(batch, &mut |_, _| {});
    }

    /// [`Canvas::finish`], with the host asked to draw its own squares between the quads.
    pub fn finish_with(&mut self, batch: &mut HudBatch, host: &mut dyn FnMut(&mut HudBatch, &HostDraw)) {
        let mut markers = std::mem::take(&mut self.host).into_iter().peekable();
        let pages = &mut self.pages;
        pages.frame += 1;
        let size = self.atlas.size;
        if !pages.atlas_uploaded {
            // The atlas page is made whole once; after that only what changed.
            self.atlas.take_dirty();
            batch.native.push(NativeUpload {
                page: 0,
                size: [size, size],
                rect: [0, 0, size, size],
                rgba: self.atlas.pixels.clone(),
                smooth: false,
            });
            pages.atlas_uploaded = true;
        } else if let Some((rect, rgba)) = self.atlas.take_dirty() {
            batch.native.push(NativeUpload {
                page: 0,
                size: [size, size],
                rect,
                rgba,
                smooth: false,
            });
        }
        let to_gui = 1.0 / batch.scale;
        for (i, b) in self.batches.iter().enumerate() {
            let end = self.batches.get(i + 1).map_or(self.quads.len(), |n| n.start);
            let page = match &b.tex {
                Tex::Atlas => 0,
                Tex::Image(img) => {
                    let frame = pages.frame;
                    if let Some(entry) = pages.by_image.get_mut(&img.id) {
                        entry.1 = frame;
                        entry.0
                    } else {
                        let page = pages.free.pop().unwrap_or_else(|| {
                            pages.next += 1;
                            pages.next
                        });
                        batch.native.push(NativeUpload {
                            page,
                            size: [img.width, img.height],
                            rect: [0, 0, img.width, img.height],
                            rgba: img.rgba(),
                            smooth: img.linear,
                        });
                        pages.by_image.insert(img.id, (page, frame));
                        page
                    }
                }
            };
            let mut seg = b.start;
            loop {
                // The host's own squares go where they were asked for, between the quads.
                while markers.peek().is_some_and(|(at, _)| *at <= seg) {
                    if let Some((_, d)) = markers.next() {
                        host(batch, &d);
                    }
                }
                let seg_end = markers.peek().map_or(end, |(at, _)| (*at).min(end));
                if seg_end > seg {
                    let start = batch.vertices.len();
                    for q in &self.quads[seg..seg_end] {
                        let (p, uv) = match q.geo {
                            Geo::Rect([x0, y0, x1, y1, u0, v0, u1, v1]) => (
                                [[x0, y0], [x1, y0], [x1, y1], [x0, y1]],
                                [[u0, v0], [u1, v0], [u1, v1], [u0, v1]],
                            ),
                            Geo::Free(xy, uv) => (
                                [[xy[0], xy[1]], [xy[2], xy[3]], [xy[4], xy[5]], [xy[6], xy[7]]],
                                [[uv[0], uv[1]], [uv[2], uv[3]], [uv[4], uv[5]], [uv[6], uv[7]]],
                            ),
                        };
                        for i in [0, 1, 2, 0, 2, 3] {
                            batch.vertices.push(HudVertex {
                                pos: [p[i][0] * to_gui, p[i][1] * to_gui],
                                uv: uv[i],
                                color: vertex_color(q.col[i]),
                                depth: 0.0,
                                glint_uv: [0.0, 0.0],
                                text: 0,
                            });
                        }
                    }
                    batch.blend_since(start, Blend::Native(page));
                }
                seg = seg_end;
                if seg >= end {
                    break;
                }
            }
        }
        for (_, d) in markers {
            host(batch, &d);
        }
        // Images nobody drew for a while give their page back.
        let frame = pages.frame;
        let stale: Vec<u64> = pages
            .by_image
            .iter()
            .filter(|(_, (_, last))| frame - last > PAGE_IDLE_FRAMES)
            .map(|(id, _)| *id)
            .collect();
        for id in stale {
            if let Some((page, _)) = pages.by_image.remove(&id) {
                batch.native.push(NativeUpload {
                    page,
                    ..NativeUpload::default()
                });
                pages.free.push(page);
            }
        }
    }
}

/// Milliseconds on one shared clock (`System.nanoTime() / 1e6`).
pub fn clock_ms() -> f64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_blend_in_premultiplied_space() {
        // Halfway from transparent black to opaque white stays white, at half alpha.
        assert_eq!(lerp_color(0x00000000, 0xFFFFFFFF, 0.5), 0x80FFFFFF);
        assert_eq!(mul_alpha(0xFF123456, 0.5), 0x80123456);
        assert_eq!(vertex_color(0x80112233), 0x80332211);
    }
}
