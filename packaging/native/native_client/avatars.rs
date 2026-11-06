//! `Avatars`: player faces (skin face + hat layer, 8x8) fetched from the
//! Native texture store (or Mojang's, for a linked account), with Steve's
//! face as the fallback.
#![allow(dead_code)]

use super::net;
use super::relay;
use horizon_client::native_ui::{Canvas, Image};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

enum Face {
    Loading,
    Ready(Arc<Image>),
    None,
}

fn faces() -> &'static Mutex<HashMap<String, Face>> {
    static FACES: OnceLock<Mutex<HashMap<String, Face>>> = OnceLock::new();
    FACES.get_or_init(Default::default)
}

fn key_of(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)) | 1 << 63
}

/// The signed-in player's own skin reference (store hash or "mj:uuid"), `None` when it is not known yet.
pub fn own(name: &str) -> Option<String> {
    if !name.is_empty()
        && let Some(hash) = horizon_client::native_skins::directory().entry(name).and_then(|e| e.skin.clone())
    {
        return Some(hash);
    }
    if let Some(skin) = relay::snapshot().me_skin {
        return Some(skin);
    }
    net::account().and_then(|a| a.uuid).map(|u| format!("mj:{}", u.replace('-', "").to_lowercase()))
}

/// Steve's face, drawn.
fn steve() -> Arc<Image> {
    static STEVE: OnceLock<Arc<Image>> = OnceLock::new();
    STEVE
        .get_or_init(|| {
            let (h, s, w, e, b, m) = (0xFF4A3322u32, 0xFFB98B66u32, 0xFFFFFFFFu32, 0xFF4B3A8Cu32, 0xFF6B4A33u32, 0xFF8A5A3Bu32);
            #[rustfmt::skip]
            let px = vec![
                h, h, h, h, h, h, h, h,
                h, h, h, h, h, h, h, h,
                h, s, s, s, s, s, s, h,
                s, s, s, s, s, s, s, s,
                s, w, e, s, s, e, w, s,
                s, s, s, s, s, s, s, s,
                s, s, b, b, b, b, s, s,
                s, s, b, m, m, b, s, s,
            ];
            pixel_art(Image::new(8, 8, px))
        })
        .clone()
}

fn pixel_art(mut img: Arc<Image>) -> Arc<Image> {
    if let Some(i) = Arc::get_mut(&mut img) {
        i.linear = false;
    }
    img
}

/// The face of a skin: its 8x8 front with the hat layer over it (HD skins are read at their scale).
fn crop(skin: &Image) -> Arc<Image> {
    let s = (skin.width / 64).max(1) as usize;
    let w = skin.width as usize;
    let mut out = vec![0u32; 64];
    for y in 0..8 {
        for x in 0..8 {
            let at = |sx: usize| skin.argb.get((8 * s + y * s) * w + (sx * s + x * s)).copied().unwrap_or(0);
            let base = at(8) | 0xFF000000;
            let hat = at(40);
            out[y * 8 + x] = if hat >> 24 > 128 { hat | 0xFF000000 } else { base };
        }
    }
    pixel_art(Image::new(8, 8, out))
}

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    };
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for c in text.bytes().filter(|c| *c != b'=') {
        acc = acc << 6 | u32::from(val(c)?);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// Skin URL of a Mojang account (session server profile, textures property).
pub fn mojang_skin_url(uuid: &str) -> Option<String> {
    let body = net::get_bytes(&format!("https://sessionserver.mojang.com/session/minecraft/profile/{uuid}"), None, 64 * 1024).ok()?;
    let profile: Value = serde_json::from_slice(&body).ok()?;
    for p in profile.get("properties")?.as_array()? {
        if p.get("name").and_then(Value::as_str) != Some("textures") {
            continue;
        }
        let json = base64_decode(p.get("value")?.as_str()?)?;
        let t: Value = serde_json::from_slice(&json).ok()?;
        let url = t.get("textures")?.get("SKIN")?.get("url")?.as_str()?.to_owned();
        return Some(match url.strip_prefix("http://textures.minecraft.net/") {
            Some(rest) => format!("https://textures.minecraft.net/{rest}"),
            None => url,
        });
    }
    None
}

fn is_hex(s: &str, n: usize) -> bool {
    s.len() == n && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The face for a skin reference; `None` while it loads.
fn face(skin: &str) -> Option<Option<Arc<Image>>> {
    {
        let f = faces().lock().unwrap_or_else(|e| e.into_inner());
        match f.get(skin) {
            Some(Face::Ready(i)) => return Some(Some(i.clone())),
            Some(Face::None) => return Some(None),
            Some(Face::Loading) => return None,
            None => {}
        }
    }
    let mojang = skin.strip_prefix("mj:").filter(|u| is_hex(u, 32)).map(str::to_owned);
    if mojang.is_none() && !is_hex(skin, 64) {
        // Not a texture hash: nothing to load.
        faces().lock().unwrap_or_else(|e| e.into_inner()).insert(skin.to_owned(), Face::None);
        return Some(None);
    }
    faces().lock().unwrap_or_else(|e| e.into_inner()).insert(skin.to_owned(), Face::Loading);
    let skin = skin.to_owned();
    let _ = std::thread::Builder::new().name("Native avatar".into()).spawn(move || {
        let url = match &mojang {
            Some(uuid) => mojang_skin_url(uuid),
            None => Some(net::hash_url(&format!("{}/csl/textures/", net::api()), &skin)),
        };
        let img = url.and_then(|u| net::get_bytes(&u, None, 2 * 1024 * 1024).ok()).and_then(|b| Image::decode(&b));
        let state = match img {
            Some(i) if i.width >= 64 => Face::Ready(crop(&i)),
            _ => Face::None,
        };
        faces().lock().unwrap_or_else(|e| e.into_inner()).insert(skin, state);
    });
    None
}

/// Draws a rounded avatar for a player. `skin` = texture hash or "mj:uuid" (may be `None`).
#[allow(clippy::too_many_arguments)]
pub fn draw(c: &mut Canvas, skin: Option<&str>, x: f32, y: f32, size: f32, radius: f32) {
    let face = match skin.map(face) {
        Some(None) => {
            // Still downloading: a skeleton instead of the letter, so the face doesn't pop from letter to skin.
            c.skeleton(x, y, size, size, radius);
            return;
        }
        Some(Some(f)) => f,
        None => None,
    };
    let (img, key) = match (face, skin) {
        (Some(f), Some(s)) => (f, key_of(s)),
        _ => (steve(), key_of("face:steve")),
    };
    c.round(x, y, size, size, radius, 0xFF1A1B20);
    // Faces are pixel art.
    c.stamp(key, &img, x, y, size, size, false, 0xFFFFFFFF);
    c.outline(x, y, size, size, radius, 1.0, 0x1FFFFFFF);
}

/// A round face (like the launcher's profile avatar): the pixel face cut to a circle with a smooth edge.
pub fn draw_round(c: &mut Canvas, skin: Option<&str>, x: f32, y: f32, size: f32) {
    let face = match skin.map(face) {
        Some(None) => {
            c.skeleton(x, y, size, size, size / 2.0);
            return;
        }
        Some(Some(f)) => f,
        None => None,
    };
    let (img, key) = match (face, skin) {
        (Some(f), Some(s)) => (f, key_of(s)),
        _ => (steve(), key_of("face:steve")),
    };
    let round = round_of(&img, key);
    c.circle(x + size / 2.0, y + size / 2.0, size / 2.0, 0xFF1A1B20);
    c.stamp(key ^ 1, &round, x, y, size, size, true, 0xFFFFFFFF);
}

fn round_of(face: &Image, key: u64) -> Arc<Image> {
    static ROUND: OnceLock<Mutex<HashMap<u64, Arc<Image>>>> = OnceLock::new();
    let mut cache = ROUND.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if cache.len() > 200 {
        cache.clear();
    }
    cache
        .entry(key)
        .or_insert_with(|| {
            let n = 128usize;
            let mut out = vec![0u32; n * n];
            let r = n as f32 / 2.0;
            for y in 0..n {
                for x in 0..n {
                    let d = ((x as f32 + 0.5 - r).powi(2) + (y as f32 + 0.5 - r).powi(2)).sqrt();
                    let a = (r - d).clamp(0.0, 1.0);
                    if a <= 0.0 {
                        continue;
                    }
                    let px = face.argb[(y * face.height as usize / n) * face.width as usize + (x * face.width as usize / n)];
                    let alpha = (((px >> 24) & 0xFF) as f32 * a).round() as u32;
                    out[y * n + x] = alpha << 24 | (px & 0xFFFFFF);
                }
            }
            Image::new(n as u32, n as u32, out)
        })
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_reads_the_session_servers_value() {
        assert_eq!(base64_decode("TWFu").unwrap(), b"Man");
        assert_eq!(base64_decode("TWE=").unwrap(), b"Ma");
        assert!(base64_decode("!!").is_none());
    }

    #[test]
    fn the_face_is_the_front_with_the_hat_over_it() {
        let mut px = vec![0u32; 64 * 64];
        for y in 8..16 {
            for x in 8..16 {
                px[y * 64 + x] = 0xFF112233;
            }
        }
        px[8 * 64 + 40] = 0xFF445566; // One hat texel.
        let face = crop(&Image::new(64, 64, px));
        assert_eq!(face.argb[0], 0xFF445566);
        assert_eq!(face.argb[1], 0xFF112233);
        assert!(!steve().linear);
    }
}
