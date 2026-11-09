//! `Image`: a decoded image (ARGB). Large ones (backgrounds) get a texture
//! page of their own; small ones are stamped into the atlas.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

pub struct Image {
    pub width: u32,
    pub height: u32,
    pub argb: Vec<u32>,
    /// Linear filtering when drawn from its own page.
    pub linear: bool,
    /// A process-wide id: the backend keys its page by it.
    pub id: u64,
}

static NEXT: AtomicU64 = AtomicU64::new(1);

impl Image {
    pub fn new(width: u32, height: u32, argb: Vec<u32>) -> Arc<Image> {
        Arc::new(Image {
            width,
            height,
            argb,
            linear: true,
            id: NEXT.fetch_add(1, Ordering::Relaxed),
        })
    }

    /// PNG, JPEG or WebP. Refuses pictures over 4096 x 4096, as the mod does.
    pub fn decode(bytes: &[u8]) -> Option<Arc<Image>> {
        let (w, h, rgba) = crate::hud::decode_image_bytes(bytes).ok()?;
        if w == 0 || h == 0 || u64::from(w) * u64::from(h) > 4096 * 4096 {
            return None;
        }
        let argb = rgba
            .chunks_exact(4)
            .map(|p| u32::from(p[3]) << 24 | u32::from(p[0]) << 16 | u32::from(p[1]) << 8 | u32::from(p[2]))
            .collect();
        Some(Image::new(w, h, argb))
    }

    /// The pixels as RGBA bytes, for upload.
    pub fn rgba(&self) -> Vec<u8> {
        self.argb
            .iter()
            .flat_map(|c| [(c >> 16) as u8, (c >> 8) as u8, *c as u8, (c >> 24) as u8])
            .collect()
    }

    /// Area-averaged downscale (premultiplied), for crisp small logos.
    pub fn scaled(&self, w: u32, h: u32) -> Arc<Image> {
        let (w, h) = (w.max(1), h.max(1));
        let mut out = vec![0u32; (w * h) as usize];
        let (sx, sy) = (self.width as f32 / w as f32, self.height as f32 / h as f32);
        for y in 0..h {
            let y0 = (y as f32 * sy) as u32;
            let y1 = (y0 + 1).max(((y + 1) as f32 * sy) as u32);
            for x in 0..w {
                let x0 = (x as f32 * sx) as u32;
                let x1 = (x0 + 1).max(((x + 1) as f32 * sx) as u32);
                let (mut a, mut r, mut g, mut b, mut n) = (0u64, 0u64, 0u64, 0u64, 0u64);
                for yy in y0..y1.min(self.height) {
                    for xx in x0..x1.min(self.width) {
                        let c = self.argb[(yy * self.width + xx) as usize];
                        let ca = u64::from(c >> 24);
                        a += ca;
                        r += u64::from((c >> 16) & 0xFF) * ca;
                        g += u64::from((c >> 8) & 0xFF) * ca;
                        b += u64::from(c & 0xFF) * ca;
                        n += 1;
                    }
                }
                if n == 0 || a == 0 {
                    continue;
                }
                out[(y * w + x) as usize] =
                    ((a / n) as u32) << 24 | ((r / a) as u32) << 16 | ((g / a) as u32) << 8 | (b / a) as u32;
            }
        }
        Image::new(w, h, out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaling_averages_only_what_is_visible() {
        // Opaque red beside fully transparent blue: the average stays red, half as opaque.
        let img = Image::new(2, 1, vec![0xFFFF0000, 0x000000FF]);
        let half = img.scaled(1, 1);
        assert_eq!(half.argb[0], 0x7FFF0000);
        assert_eq!(img.rgba(), vec![255, 0, 0, 255, 0, 0, 255, 0]);
    }
}
