//! `Atlas`: one RGBA texture that holds every glyph, icon, rounded-corner
//! sprite and player face the Native UI draws, so a whole frame is one page.
//! Regions are packed into shelves; when the atlas fills up it is cleared at
//! the start of the next frame and everything re-bakes on demand (bumping
//! [`Atlas::generation`]). Sampled texel for texel: every sprite is baked at
//! the exact pixel size it is drawn at.

pub struct Atlas {
    pub size: u32,
    /// RGBA, row-major.
    pub pixels: Vec<u8>,
    shelf_x: u32,
    shelf_y: u32,
    shelf_h: u32,
    full: bool,
    dirty: Option<[u32; 4]>,
    /// Bumped on every reset: caches holding atlas regions compare against it.
    pub generation: u32,
    /// Texel centre of an always-white texel, for solid fills (0..1).
    pub white_u: f32,
    pub white_v: f32,
}

impl Atlas {
    pub fn new(size: u32) -> Self {
        let mut atlas = Self {
            size,
            pixels: vec![0; (size * size * 4) as usize],
            shelf_x: 0,
            shelf_y: 0,
            shelf_h: 0,
            full: false,
            dirty: None,
            generation: 0,
            white_u: 1.5 / size as f32,
            white_v: 1.5 / size as f32,
        };
        atlas.reset();
        atlas
    }

    pub fn reset(&mut self) {
        self.pixels.fill(0);
        for y in 0..4 {
            for x in 0..4 {
                let i = ((y * self.size + x) * 4) as usize;
                self.pixels[i..i + 4].copy_from_slice(&[255; 4]);
            }
        }
        self.shelf_x = 6;
        self.shelf_y = 0;
        self.shelf_h = 6;
        self.full = false;
        self.generation = self.generation.wrapping_add(1);
        self.mark_dirty(0, 0, self.size, self.size);
    }

    /// True once an allocation failed: the owner resets the atlas before the next frame.
    pub fn is_full(&self) -> bool {
        self.full
    }

    /// Reserves a w x h region (with a 1 px gutter); `None` when there is no room left.
    pub fn alloc(&mut self, w: u32, h: u32) -> Option<[u32; 2]> {
        let (pw, ph) = (w + 1, h + 1);
        if pw > self.size || ph > self.size {
            return None;
        }
        if self.shelf_x + pw > self.size {
            self.shelf_y += self.shelf_h;
            self.shelf_x = 0;
            self.shelf_h = 0;
        }
        if self.shelf_y + ph > self.size {
            self.full = true;
            return None;
        }
        let at = [self.shelf_x, self.shelf_y];
        self.shelf_x += pw;
        self.shelf_h = self.shelf_h.max(ph);
        Some(at)
    }

    /// Writes white with the given coverage (0-255) per pixel.
    pub fn put_alpha(&mut self, x: u32, y: u32, w: u32, h: u32, alpha: &[u8], stride: usize) {
        for row in 0..h {
            let mut dst = (((y + row) * self.size + x) * 4) as usize;
            let src = row as usize * stride;
            for col in 0..w as usize {
                self.pixels[dst..dst + 4].copy_from_slice(&[255, 255, 255, alpha[src + col]]);
                dst += 4;
            }
        }
        self.mark_dirty(x, y, w, h);
    }

    /// Writes straight ARGB pixels.
    pub fn put_argb(&mut self, x: u32, y: u32, w: u32, h: u32, argb: &[u32]) {
        for row in 0..h {
            let mut dst = (((y + row) * self.size + x) * 4) as usize;
            for col in 0..w {
                let c = argb[(row * w + col) as usize];
                self.pixels[dst..dst + 4]
                    .copy_from_slice(&[(c >> 16) as u8, (c >> 8) as u8, c as u8, (c >> 24) as u8]);
                dst += 4;
            }
        }
        self.mark_dirty(x, y, w, h);
    }

    fn mark_dirty(&mut self, x: u32, y: u32, w: u32, h: u32) {
        self.dirty = Some(match self.dirty {
            None => [x, y, x + w, y + h],
            Some([x0, y0, x1, y1]) => [x0.min(x), y0.min(y), x1.max(x + w), y1.max(y + h)],
        });
    }

    /// The rectangle changed since the last call (x, y, w, h) and its pixels; clears it.
    pub fn take_dirty(&mut self) -> Option<([u32; 4], Vec<u8>)> {
        let [x0, y0, x1, y1] = self.dirty.take()?;
        if x0 >= x1 || y0 >= y1 {
            return None;
        }
        let (w, h) = (x1 - x0, y1 - y0);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in y0..y1 {
            let start = ((y * self.size + x0) * 4) as usize;
            rgba.extend_from_slice(&self.pixels[start..start + (w * 4) as usize]);
        }
        Some(([x0, y0, w, h], rgba))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shelves_fill_then_report_full() {
        let mut atlas = Atlas::new(16);
        assert_eq!(atlas.alloc(4, 4), Some([6, 0]));
        assert_eq!(atlas.alloc(4, 4), Some([11, 0]));
        // The next one starts a new shelf under the tallest of the row (6, the white block's).
        assert_eq!(atlas.alloc(4, 4), Some([0, 6]));
        assert!(atlas.alloc(20, 1).is_none());
        assert!(!atlas.is_full());
        while atlas.alloc(8, 8).is_some() {}
        assert!(atlas.is_full());
        let generation = atlas.generation;
        atlas.reset();
        assert_eq!(atlas.generation, generation + 1);
        assert!(!atlas.is_full());
    }

    #[test]
    fn the_dirty_rectangle_covers_every_write() {
        let mut atlas = Atlas::new(16);
        atlas.take_dirty();
        atlas.put_alpha(2, 3, 2, 1, &[10, 20], 2);
        atlas.put_argb(8, 9, 1, 1, &[0x80112233]);
        let (rect, rgba) = atlas.take_dirty().unwrap();
        assert_eq!(rect, [2, 3, 7, 7]);
        assert_eq!(&rgba[..8], &[255, 255, 255, 10, 255, 255, 255, 20]);
        assert!(atlas.take_dirty().is_none());
    }
}
