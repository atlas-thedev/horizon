//! `ModuleIcons` (the original CC0 Native module artwork, a font whose
//! codepoints mirror `native-modules.json`) and `NavIcons` (the navigation
//! icons drawn in code: 24 x 24 line art, rasterised with anti-aliasing at
//! the exact pixel size).

use super::image::Image;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// The first module icon's codepoint.
pub const MODULE_FIRST: u32 = 0xF400;
/// Append only: the font's codepoints follow this order.
const MODULE_IDS: [&str; 43] = [
    "fps", "cps", "keystrokes", "coords", "direction", "clock", "memory", "ping", "server", "speed", "scoreboard",
    "bossbar", "togglesprint", "zoom", "fullbright", "bgfps", "effects", "armor", "inventory", "crosshair",
    "hitbox-colours", "critical-colours", "watermark", "voice", "voice-overlay", "nether-coords", "moon-phase",
    "day-night", "totem-counter", "item-counter", "playtime", "reach", "low-fire", "damage-tint", "combo",
    "cooldowns", "block-overlay", "freelook", "no-hurt-cam", "time-changer", "weather-changer", "now-playing",
    "motion-blur",
];

/// A module's icon codepoint, or `fallback`.
pub fn for_module(id: &str, fallback: u32) -> u32 {
    MODULE_IDS
        .iter()
        .position(|m| *m == id)
        .map_or(fallback, |i| MODULE_FIRST + i as u32)
}

/// A codepoint of the module icon font.
pub fn custom(codepoint: u32) -> bool {
    (MODULE_FIRST..MODULE_FIRST + MODULE_IDS.len() as u32).contains(&codepoint)
}

pub const MODS: u32 = 0xF500;
pub const LOCKER: u32 = 0xF501;
pub const SETTINGS: u32 = 0xF502;
pub const MOON: u32 = 0xF503;
pub const CAMERA: u32 = 0xF504;
pub const FLAME: u32 = 0xF505;
pub const MUSIC: u32 = 0xF506;
const STROKE: f32 = 1.9;

/// A navigation icon codepoint.
pub fn nav(codepoint: u32) -> bool {
    (MODS..=MUSIC).contains(&codepoint)
}

/// A white navigation icon of `px` x `px` pixels (tinted when drawn).
pub fn nav_image(codepoint: u32, px: u32) -> Arc<Image> {
    static CACHE: OnceLock<Mutex<HashMap<(u32, u32), Arc<Image>>>> = OnceLock::new();
    let mut cache = CACHE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if let Some(img) = cache.get(&(codepoint, px)) {
        return img.clone();
    }
    if cache.len() > 256 {
        cache.clear();
    }
    let img = render(codepoint, px);
    cache.insert((codepoint, px), img.clone());
    img
}

fn render(cp: u32, px: u32) -> Arc<Image> {
    let mut argb = vec![0u32; (px * px) as usize];
    let k = 24.0 / px as f32;
    let (half, aa) = (STROKE / 2.0, k);
    for j in 0..px {
        for i in 0..px {
            let (x, y) = ((i as f32 + 0.5) * k, (j as f32 + 0.5) * k);
            let d = dist(cp, x, y);
            let a = (0.5 - (d - half) / aa).clamp(0.0, 1.0);
            if a > 0.0 {
                argb[(j * px + i) as usize] = ((a * 255.0).round() as u32) << 24 | 0xFFFFFF;
            }
        }
    }
    Image::new(px, px, argb)
}

fn len(x: f32, y: f32) -> f32 {
    (x * x + y * y).sqrt()
}

fn seg(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let (dx, dy) = (bx - ax, by - ay);
    let t = (((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    len(px - ax - t * dx, py - ay - t * dy)
}

/// Arc outline, angles in degrees (screen space, y down), from `a0` to `a1`.
fn arc(px: f32, py: f32, cx: f32, cy: f32, r: f32, a0: f32, a1: f32) -> f32 {
    let mut a = f64::from(py - cy).atan2(f64::from(px - cx)).to_degrees();
    while a < f64::from(a0) {
        a += 360.0;
    }
    if a <= f64::from(a1) {
        return (len(px - cx, py - cy) - r).abs();
    }
    let (r0, r1) = (a0.to_radians(), a1.to_radians());
    let e0 = len(px - cx - r * r0.cos(), py - cy - r * r0.sin());
    let e1 = len(px - cx - r * r1.cos(), py - cy - r * r1.sin());
    e0.min(e1)
}

fn round_rect(px: f32, py: f32, x: f32, y: f32, w: f32, h: f32, r: f32) -> f32 {
    let (hx, hy) = (w / 2.0 - r, h / 2.0 - r);
    let (qx, qy) = ((px - (x + w / 2.0)).abs() - hx, (py - (y + h / 2.0)).abs() - hy);
    (len(qx.max(0.0), qy.max(0.0)) + qx.max(qy).min(0.0) - r).abs()
}

/// Signed distance to a filled four-point sparkle (negative inside).
fn star(px: f32, py: f32, cx: f32, cy: f32, outer: f32, inner: f32) -> f32 {
    let mut xs = [0.0f32; 8];
    let mut ys = [0.0f32; 8];
    for i in 0..8 {
        let a = ((i * 45) as f32 - 90.0).to_radians();
        let r = if i % 2 == 0 { outer } else { inner };
        xs[i] = cx + r * a.cos();
        ys[i] = cy + r * a.sin();
    }
    let mut d = f32::MAX;
    let mut inside = false;
    let mut j = 7;
    for i in 0..8 {
        d = d.min(seg(px, py, xs[j], ys[j], xs[i], ys[i]));
        if (ys[i] > py) != (ys[j] > py) && px < (xs[j] - xs[i]) * (py - ys[i]) / (ys[j] - ys[i]) + xs[i] {
            inside = !inside;
        }
        j = i;
    }
    if inside { -d } else { d }
}

/// Distance (viewBox units) from the point to the stroke's centre line; filled parts return -half.
fn dist(cp: u32, x: f32, y: f32) -> f32 {
    match cp {
        MODS => {
            let d = round_rect(x, y, 3.0, 3.0, 7.5, 7.5, 2.2)
                .min(round_rect(x, y, 3.0, 13.5, 7.5, 7.5, 2.2))
                .min(round_rect(x, y, 13.5, 13.5, 7.5, 7.5, 2.2));
            d.min(star(x, y, 17.25, 6.75, 4.6, 1.25) + STROKE / 2.0)
        }
        LOCKER => {
            let a = 40.0f32.to_radians();
            arc(x, y, 12.0, 6.0, 2.4, -180.0, 40.0)
                .min(seg(x, y, 12.0 + 2.4 * a.cos(), 6.0 + 2.4 * a.sin(), 12.0, 9.6))
                .min(seg(x, y, 12.0, 9.6, 2.8, 16.4))
                .min(seg(x, y, 12.0, 9.6, 21.2, 16.4))
                .min(seg(x, y, 2.8, 16.4, 4.2, 18.6))
                .min(seg(x, y, 21.2, 16.4, 19.8, 18.6))
                .min(seg(x, y, 4.2, 18.6, 19.8, 18.6))
        }
        MOON => {
            // A crescent: the big circle's outer arc and the bite taken out of it.
            let (cx, cy, r, bx, by, br) = (12.0, 12.0, 8.6, 16.2, 8.2, 7.0);
            let (d1, d2) = ((len(x - cx, y - cy) - r).abs(), (len(x - bx, y - by) - br).abs());
            let mut d = f32::MAX;
            if len(x - bx, y - by) >= br {
                d = d.min(d1);
            }
            if len(x - cx, y - cy) <= r {
                d = d.min(d2);
            }
            d
        }
        CAMERA => round_rect(x, y, 2.5, 7.0, 19.0, 12.5, 3.0)
            .min(seg(x, y, 8.2, 7.0, 9.6, 4.4))
            .min(seg(x, y, 9.6, 4.4, 14.4, 4.4))
            .min(seg(x, y, 14.4, 4.4, 15.8, 7.0))
            .min((len(x - 12.0, y - 13.2) - 3.6).abs()),
        MUSIC => {
            // Two beamed notes: heads, stems and the beam.
            (len(x - 6.6, y - 17.4) - 2.6)
                .abs()
                .min((len(x - 17.2, y - 15.4) - 2.6).abs())
                .min(seg(x, y, 9.2, 17.4, 9.2, 5.6))
                .min(seg(x, y, 19.8, 15.4, 19.8, 3.6))
                .min(seg(x, y, 9.2, 5.6, 19.8, 3.6))
                .min(seg(x, y, 9.2, 9.4, 19.8, 7.4))
        }
        FLAME => {
            let pts = [
                (12.0, 2.8),
                (15.8, 7.6),
                (18.4, 12.4),
                (17.4, 17.2),
                (14.4, 20.4),
                (12.0, 21.0),
                (9.6, 20.4),
                (6.6, 17.2),
                (5.8, 13.2),
                (7.4, 9.4),
                (9.6, 12.4),
                (10.4, 7.2),
            ];
            let mut d = f32::MAX;
            let mut j = pts.len() - 1;
            for i in 0..pts.len() {
                d = d.min(seg(x, y, pts[j].0, pts[j].1, pts[i].0, pts[i].1));
                j = i;
            }
            d.min((len(x - 12.0, y - 16.4) - 2.2).abs())
        }
        _ => {
            // SETTINGS: three sliders.
            let (ys, kx, r) = ([6.0, 12.0, 18.0], [15.5, 8.5, 13.5], 2.4);
            let mut d = f32::MAX;
            for i in 0..3 {
                d = d
                    .min(seg(x, y, 3.0, ys[i], kx[i] - r - 1.6, ys[i]))
                    .min(seg(x, y, kx[i] + r + 1.6, ys[i], 21.0, ys[i]))
                    .min((len(x - kx[i], y - ys[i]) - r).abs());
            }
            d
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_icons_follow_the_font_order() {
        assert_eq!(for_module("fps", 0), 0xF400);
        assert_eq!(for_module("motion-blur", 0), 0xF400 + 42);
        assert_eq!(for_module("unknown", 7), 7);
        assert!(custom(0xF400) && custom(0xF42A) && !custom(0xF42B));
    }

    #[test]
    fn nav_icons_draw_their_strokes() {
        let img = nav_image(SETTINGS, 24);
        // The first slider's line passes through (5, 6) of the 24 unit box.
        let at = |x: u32, y: u32| img.argb[(y * 24 + x) as usize] >> 24;
        assert!(at(4, 5) > 100);
        assert_eq!(at(0, 23), 0);
        assert!(nav(MUSIC) && !nav(0xF507));
    }
}
