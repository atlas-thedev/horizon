// Native Client: HD capes. `tools/native/native_cosmetics.py` copies this file next to the render crate's skins.rs
// and includes it there.
//
// A skin slot is 64x64 and `process_cape` squeezes a larger cape into its top 64x32: a 512x256 Native cape (or a frame
// of an animated one) lost 63 of every 64 texels and looked blurry and blocky. The full picture is kept here, by its
// atlas location ("capes/<digest>"), and the game puts it into the atlas's Native area at its own size; the 64x32
// copy in the slot is what shows when that area has no room.

/// A cape's full-size picture.
#[derive(Clone)]
pub struct HdCape {
    pub width: u32,
    pub height: u32,
    pub rgba: std::sync::Arc<Vec<u8>>,
    /// Changes whenever the picture does (a new frame of an animated cape): the game uploads it again.
    pub generation: u64,
}

/// The largest HD cape kept: wider ones are halved (box filter) until they fit.
const HD_CAPE_MAX: (u32, u32) = (1024, 512);
/// HD capes kept at once (oldest first out).
const HD_CAPES_KEPT: usize = 48;

fn hd_capes() -> std::sync::MutexGuard<'static, (u64, std::collections::VecDeque<(String, HdCape)>)> {
    static STORE: std::sync::OnceLock<std::sync::Mutex<(u64, std::collections::VecDeque<(String, HdCape)>)>> =
        std::sync::OnceLock::new();
    STORE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

/// Keeps the full picture of a cape larger than 64x32 for `location`; smaller ones fit their slot as they are.
pub fn set_hd_cape(location: &str, width: u32, height: u32, rgba: &[u8]) {
    if (width <= 64 && height <= 32) || width == 0 || height == 0 || rgba.len() < (width * height * 4) as usize {
        return;
    }
    let (mut w, mut h, mut px) = (width, height, rgba[..(width * height * 4) as usize].to_vec());
    while w > HD_CAPE_MAX.0 || h > HD_CAPE_MAX.1 {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut out = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let at = |sx: u32, sy: u32| u32::from(px[((sy.min(h - 1) * w + sx.min(w - 1)) * 4 + c) as usize]);
                    let sum = at(2 * x, 2 * y) + at(2 * x + 1, 2 * y) + at(2 * x, 2 * y + 1) + at(2 * x + 1, 2 * y + 1);
                    out[((y * nw + x) * 4 + c) as usize] = (sum / 4) as u8;
                }
            }
        }
        (w, h, px) = (nw, nh, out);
    }
    let mut store = hd_capes();
    store.0 += 1;
    let generation = store.0;
    store.1.retain(|(l, _)| l != location);
    store.1.push_back((location.to_owned(), HdCape { width: w, height: h, rgba: std::sync::Arc::new(px), generation }));
    while store.1.len() > HD_CAPES_KEPT {
        store.1.pop_front();
    }
}

/// The full picture of the cape at `location`, when it is larger than a skin slot holds.
pub fn hd_cape(location: &str) -> Option<HdCape> {
    hd_capes().1.iter().find(|(l, _)| l == location).map(|(_, c)| c.clone())
}

#[cfg(test)]
mod native_hd_cape_tests {
    use super::*;

    #[test]
    fn hd_capes_keep_their_size_and_small_ones_are_not_kept() {
        set_hd_cape("capes/small", 64, 32, &[1; 64 * 32 * 4]);
        assert!(hd_cape("capes/small").is_none());
        set_hd_cape("capes/hd", 512, 256, &[2; 512 * 256 * 4]);
        let hd = hd_cape("capes/hd").unwrap();
        assert_eq!((hd.width, hd.height), (512, 256));
        set_hd_cape("capes/huge", 4096, 2048, &[3; 4096 * 2048 * 4]);
        let huge = hd_cape("capes/huge").unwrap();
        assert_eq!((huge.width, huge.height), (1024, 512));
        assert!(huge.rgba.iter().all(|v| *v == 3));
        // a new frame is a new generation
        let before = hd_cape("capes/hd").unwrap().generation;
        set_hd_cape("capes/hd", 512, 256, &[4; 512 * 256 * 4]);
        assert!(hd_cape("capes/hd").unwrap().generation > before);
    }
}
