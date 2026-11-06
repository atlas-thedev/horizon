//! `Shapes`: anti-aliased rounded rectangles, rings and soft shadows as
//! 9-slice sprites. Each (radius, feather, ring width) is rasterised once on
//! the CPU from a signed distance function, then stretched to any size with 9
//! quads.

use super::atlas::Atlas;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug)]
pub struct Sprite {
    /// Atlas position.
    pub x: u32,
    pub y: u32,
    /// Corner tile size.
    pub c: u32,
    /// Sprite size.
    pub s: u32,
    pub hollow: bool,
}

#[derive(Default)]
pub struct Shapes {
    cache: HashMap<u32, Sprite>,
    generation: Option<u32>,
}

fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    v.clamp(lo, hi)
}

impl Shapes {
    /// `r` corner radius px, `f` feather px (0 = crisp edge), `bw` ring width px (0 = filled).
    pub fn get(&mut self, atlas: &mut Atlas, r: i32, f: i32, bw: i32) -> Option<Sprite> {
        if self.generation != Some(atlas.generation) {
            self.cache.clear();
            self.generation = Some(atlas.generation);
        }
        let r = r.clamp(0, 120) as u32;
        let f = f.clamp(0, 120) as u32;
        let bw = bw.clamp(0, 60) as u32;
        let key = r | (f << 8) | (bw << 16);
        if let Some(s) = self.cache.get(&key) {
            return Some(*s);
        }
        let s = bake(atlas, r, f, bw)?;
        self.cache.insert(key, s);
        Some(s)
    }
}

fn bake(atlas: &mut Atlas, r: u32, f: u32, bw: u32) -> Option<Sprite> {
    let c = r + f + 2;
    let size = 2 * c + 2;
    let [ax, ay] = atlas.alloc(size, size)?;
    let mut alpha = vec![0u8; (size * size) as usize];
    let m = f as f32 + 1.0;
    // Half extent of the shape box.
    let half = size as f32 / 2.0 - m;
    let (cx, cy) = (size as f32 / 2.0, size as f32 / 2.0);
    let (rf, ff, bwf) = (r as f32, f as f32, bw as f32);
    for y in 0..size {
        for x in 0..size {
            let px = (x as f32 + 0.5 - cx).abs() - (half - rf);
            let py = (y as f32 + 0.5 - cy).abs() - (half - rf);
            let (ox, oy) = (px.max(0.0), py.max(0.0));
            let d = (ox * ox + oy * oy).sqrt() + px.max(py).min(0.0) - rf;
            let mut a = if f > 0 {
                let t = clamp((d + ff) / (2.0 * ff), 0.0, 1.0);
                1.0 - t * t * (3.0 - 2.0 * t)
            } else {
                clamp(0.5 - d, 0.0, 1.0)
            };
            if bw > 0 {
                a *= clamp(0.5 + d + bwf, 0.0, 1.0);
            }
            alpha[(y * size + x) as usize] = (a * 255.0).round() as u8;
        }
    }
    atlas.put_alpha(ax, ay, size, size, &alpha, size as usize);
    Some(Sprite {
        x: ax,
        y: ay,
        c,
        s: size,
        hollow: bw > 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rounded_sprite_is_solid_inside_and_clear_at_the_corner() {
        let mut atlas = Atlas::new(256);
        let mut shapes = Shapes::default();
        let s = shapes.get(&mut atlas, 8, 0, 0).unwrap();
        let alpha = |x: u32, y: u32| atlas.pixels[(((s.y + y) * 256 + s.x + x) * 4 + 3) as usize];
        assert_eq!(alpha(s.s / 2, s.s / 2), 255);
        assert_eq!(alpha(0, 0), 0);
        // Cached: the same sprite again, no second bake.
        let again = shapes.get(&mut atlas, 8, 0, 0).unwrap();
        assert_eq!((again.x, again.y), (s.x, s.y));
        // A ring is hollow in the middle.
        let ring = shapes.get(&mut atlas, 8, 0, 2).unwrap();
        assert!(ring.hollow);
        let centre = atlas.pixels[(((ring.y + ring.s / 2) * 256 + ring.x + ring.s / 2) * 4 + 3) as usize];
        assert_eq!(centre, 0);
    }
}
