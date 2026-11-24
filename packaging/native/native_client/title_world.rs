//! `TitleWorld`: the title screen's background, you standing in a whole block world, the way the launcher's Home
//! shows it (the same land the launcher builds for this instance: a port of its `worldGen.js`). The same as the Java
//! mod's `TitleWorld`.
//!
//! Nothing of the world is drawn per frame. On a few background threads the land is generated and ray-traced ONCE
//! into a picture the size of the screen (the game's block textures, resource packs included, with its flat shading,
//! corner ambient occlusion, water, plants, clouds, fog and a sky for the time of day), seen from just behind you, so
//! its ground lands right under your feet. Every frame after that the title screen draws just that picture: one
//! textured quad, breathing slowly around your feet. It is baked again only when the clock moves to another sky (day,
//! sunset, night), the resources change or the screen changes size.

use horizon_client::native_ui::{Canvas, Image};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

// ── the land (worldGen.js) ─────────────────────────────────────────────

pub const SIZE: i32 = 144;
pub const HALF: i32 = SIZE / 2;
pub const HEIGHT: i32 = 72;
pub const WATER: i32 = 12;
pub const G0: i32 = WATER + 8;

const AIR: u8 = 0;
const GRASS: u8 = 1;
const DIRT: u8 = 2;
const STONE: u8 = 3;
const SAND: u8 = 4;
const GRAVEL: u8 = 5;
const SNOW: u8 = 6;
const OAK_LOG: u8 = 7;
const OAK_LEAVES: u8 = 8;
const BIRCH_LOG: u8 = 9;
const BIRCH_LEAVES: u8 = 10;
const WATER_BLOCK: u8 = 11;

const fn opaque(b: u8) -> bool {
    matches!(b, GRASS | DIRT | STONE | SAND | GRAVEL | SNOW | OAK_LOG | BIRCH_LOG)
}
const fn leaves(b: u8) -> bool {
    matches!(b, OAK_LEAVES | BIRCH_LEAVES)
}
/// What darkens a corner (ambient occlusion): any full cube, leaves included.
const fn occludes(b: u8) -> u32 {
    if opaque(b) || leaves(b) { 1 } else { 0 }
}

// tiles, in the launcher's atlas order, then water
const T_GRASS_TOP: usize = 0;
const T_GRASS_SIDE: usize = 1;
const T_DIRT: usize = 2;
const T_STONE: usize = 3;
const T_SAND: usize = 4;
const T_GRAVEL: usize = 5;
const T_SNOW: usize = 6;
const T_OAK_LOG: usize = 7;
const T_OAK_LOG_TOP: usize = 8;
const T_OAK_LEAVES: usize = 9;
const T_BIRCH_LOG: usize = 10;
const T_BIRCH_LOG_TOP: usize = 11;
const T_BIRCH_LEAVES: usize = 12;
const T_SHORT_GRASS: usize = 13;
const T_POPPY: usize = 14;
const T_DANDELION: usize = 15;
const T_CORNFLOWER: usize = 16;
const T_WATER: usize = 17;
const TILES: usize = 18;

/// [top, side, bottom] tile per block.
fn faces_of(b: u8) -> [usize; 3] {
    match b {
        GRASS => [T_GRASS_TOP, T_GRASS_SIDE, T_DIRT],
        DIRT => [T_DIRT; 3],
        STONE => [T_STONE; 3],
        SAND => [T_SAND; 3],
        GRAVEL => [T_GRAVEL; 3],
        SNOW => [T_SNOW; 3],
        OAK_LOG => [T_OAK_LOG_TOP, T_OAK_LOG, T_OAK_LOG_TOP],
        BIRCH_LOG => [T_BIRCH_LOG_TOP, T_BIRCH_LOG, T_BIRCH_LOG_TOP],
        OAK_LEAVES => [T_OAK_LEAVES; 3],
        _ => [T_BIRCH_LEAVES; 3],
    }
}

fn hash2(x: i32, z: i32, seed: i32) -> f64 {
    let mut h = x.wrapping_mul(374761393).wrapping_add(z.wrapping_mul(668265263)).wrapping_add(seed.wrapping_mul(1442695041));
    h = (h ^ ((h as u32) >> 13) as i32).wrapping_mul(1274126177);
    ((h ^ ((h as u32) >> 16) as i32) as u32) as f64 / 4294967296.0
}

fn noise(x: f64, z: f64, seed: i32) -> f64 {
    let (fx, fz) = (x.floor(), z.floor());
    let (xi, zi) = (fx as i32, fz as i32);
    let (xf, zf) = (x - fx, z - fz);
    let (u, v) = (xf * xf * (3.0 - 2.0 * xf), zf * zf * (3.0 - 2.0 * zf));
    let (a, b) = (hash2(xi, zi, seed), hash2(xi + 1, zi, seed));
    let (c, d) = (hash2(xi, zi + 1, seed), hash2(xi + 1, zi + 1, seed));
    a + (b - a) * u + (c - a) * v + (a - b - c + d) * u * v
}

fn fbm(x: f64, z: f64, seed: i32, octaves: i32) -> f64 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 1.0, 1.0, 0.0);
    for i in 0..octaves {
        sum += noise(x * freq, z * freq, seed + i * 131) * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}

fn smooth(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// JavaScript's `Math.round`: halves go up.
fn js_round(v: f64) -> f64 {
    (v + 0.5).floor()
}

/// A stable number for a world name (one world per instance), the same as the launcher's `seedOf`.
pub fn seed_of(text: &str) -> i32 {
    let s = if text.is_empty() { "native" } else { text };
    let mut h: u32 = 2166136261;
    for ch in s.chars() {
        let mut units = [0u16; 2];
        let first = ch.encode_utf16(&mut units)[0]; // the first UTF-16 unit, like charCodeAt(0)
        h = (h ^ u32::from(first)).wrapping_mul(16777619);
    }
    (h % 1000003) as i32
}

/// The generated land: blocks, column heights and the plant (tile + 1, 0 = none) standing on each column.
pub struct Land {
    pub seed: i32,
    pub blocks: Vec<u8>,
    pub heights: Vec<i16>,
    plants: Vec<u8>,
}

impl Land {
    fn at(x: i32, y: i32, z: i32) -> usize {
        (((y * SIZE) + (z + HALF)) * SIZE + (x + HALF)) as usize
    }

    #[inline]
    fn get(&self, x: i32, y: i32, z: i32) -> u8 {
        if !(-HALF..HALF).contains(&x) || !(-HALF..HALF).contains(&z) || !(0..HEIGHT).contains(&y) {
            return AIR;
        }
        self.blocks[Self::at(x, y, z)]
    }

    fn set(&mut self, x: i32, y: i32, z: i32, b: u8) {
        if (-HALF..HALF).contains(&x) && (-HALF..HALF).contains(&z) && (0..HEIGHT).contains(&y) {
            self.blocks[Self::at(x, y, z)] = b;
        }
    }

    fn top(&self, x: i32, z: i32) -> i32 {
        i32::from(self.heights[((z + HALF) * SIZE + (x + HALF)) as usize])
    }
}

pub fn generate(seed: i32) -> Land {
    let mut land = Land {
        seed,
        blocks: vec![AIR; (SIZE * SIZE * HEIGHT) as usize],
        heights: vec![0; (SIZE * SIZE) as usize],
        plants: vec![0; (SIZE * SIZE) as usize],
    };
    for z in -HALF..HALF {
        for x in -HALF..HALF {
            let (xf, zf) = (f64::from(x), f64::from(z));
            let d = (xf + 0.5).hypot(zf + 0.5);
            // lowlands with lakes and rolling hills
            let mut h = f64::from(WATER) - 3.0 + fbm(xf / 34.0, zf / 34.0, seed, 4) * 14.0
                + (fbm(xf / 11.0, zf / 11.0, seed + 7, 3) - 0.5) * 4.0;
            // hills and mountains rise towards the horizon
            h += smooth(28.0, 66.0, d) * fbm(xf / 24.0, zf / 24.0, seed + 3, 4).powf(1.3) * 38.0;
            // the hill in the middle: a flat top, its sides stepping down unevenly
            let edge = 5.5 + (fbm(xf / 9.0, zf / 9.0, seed + 13, 2) - 0.5) * 3.0;
            let hill = if d < edge {
                f64::from(G0)
            } else {
                f64::from(G0) - (d - edge) * (0.75 + fbm(xf / 7.0, zf / 7.0, seed + 17, 2) * 0.6)
            };
            h = h.max(hill);
            let top = js_round(h).min(f64::from(HEIGHT - 12)).max(1.0) as i32;
            land.heights[((z + HALF) * SIZE + (x + HALF)) as usize] = top as i16;
            let beach = top <= WATER + 1;
            let peak = top > G0 + 20 + (hash2(x, z, seed + 9) * 3.0).floor() as i32;
            for y in 0..top {
                let mut b = if y < top - 4 { STONE } else { DIRT };
                if y == top - 1 {
                    b = if top < WATER {
                        if fbm(xf / 6.0, zf / 6.0, seed + 5, 2) > 0.55 { GRAVEL } else { SAND }
                    } else if beach {
                        SAND
                    } else if peak {
                        SNOW
                    } else {
                        GRASS
                    };
                } else if beach && y >= top - 3 {
                    b = SAND;
                }
                if peak && top - y > 4 && y >= top - 6 {
                    b = STONE;
                }
                land.blocks[Land::at(x, y, z)] = b;
            }
            for y in top..WATER {
                land.blocks[Land::at(x, y, z)] = WATER_BLOCK;
            }
        }
    }
    // trees: oak and birch, kept off the hill in the middle so the view stays open
    for z in -HALF + 3..HALF - 3 {
        for x in -HALF + 3..HALF - 3 {
            let (xf, zf) = (f64::from(x), f64::from(z));
            if (xf + 0.5).hypot(zf + 0.5) < 20.0 {
                continue;
            }
            let f = fbm(xf / 30.0, zf / 30.0, seed + 21, 3);
            let chance = if f > 0.55 { 0.05 } else if f > 0.42 { 0.014 } else { 0.003 };
            if hash2(x, z, seed + 31) > chance {
                continue;
            }
            let top = land.top(x, z);
            if land.get(x, top - 1, z) != GRASS {
                continue;
            }
            let crowded = (-3..=3).any(|dz| (-3..=3).any(|dx| matches!(land.get(x + dx, top, z + dz), OAK_LOG | BIRCH_LOG)));
            if crowded {
                continue;
            }
            let birch = fbm(xf / 18.0, zf / 18.0, seed + 41, 2) > 0.6;
            let (log, leaf) = if birch { (BIRCH_LOG, BIRCH_LEAVES) } else { (OAK_LOG, OAK_LEAVES) };
            let trunk = 4 + (hash2(x, z, seed + 51) * 3.0).floor() as i32;
            land.set(x, top - 1, z, DIRT);
            for y in 0..trunk {
                land.set(x, top + y, z, log);
            }
            let crown = top + trunk;
            for y in crown - 3..=crown {
                let r: i32 = if y >= crown - 1 { 1 } else { 2 };
                for dz in -r..=r {
                    for dx in -r..=r {
                        let corner = dx.abs() == r && dz.abs() == r;
                        if corner && (y == crown || hash2(x + dx, z + dz, seed + y) > 0.5) {
                            continue;
                        }
                        if y == crown && dx != 0 && dz != 0 {
                            continue;
                        }
                        if land.get(x + dx, y, z + dz) == AIR {
                            land.set(x + dx, y, z + dz, leaf);
                        }
                    }
                }
            }
        }
    }
    // grass and flowers on grass tops
    for z in -HALF + 1..HALF - 1 {
        for x in -HALF + 1..HALF - 1 {
            let d = (f64::from(x) + 0.5).hypot(f64::from(z) + 0.5);
            if d < 2.6 || (z > 0 && d < 7.0) || d > 58.0 {
                continue;
            }
            let top = land.top(x, z);
            if land.get(x, top - 1, z) != GRASS || land.get(x, top, z) != AIR {
                continue;
            }
            let r = hash2(x, z, seed + 61);
            let tile = if r < 0.16 {
                T_SHORT_GRASS
            } else if r < 0.175 {
                T_POPPY
            } else if r < 0.19 {
                T_DANDELION
            } else if r < 0.197 {
                T_CORNFLOWER
            } else {
                continue;
            };
            land.plants[((z + HALF) * SIZE + (x + HALF)) as usize] = tile as u8 + 1;
        }
    }
    land
}

/// The ground you stand on for the title screen: the hill in the middle of the land made a wide, flat meadow, its
/// edge stepping down into the land around, grass and flowers on it but none right at your feet, and no trees close
/// by to hide the view. Changes the land in place.
pub fn stand_on(mut land: Land) -> Land {
    const MEADOW: f64 = 8.5;
    const CLEARING: f64 = 34.0;
    let seed = land.seed;
    // the trees nearest you go, so the view opens onto the forests and mountains further out
    for z in -HALF..HALF {
        for x in -HALF..HALF {
            let top = land.top(x, z);
            let trunk = land.get(x, top, z);
            if !matches!(trunk, OAK_LOG | BIRCH_LOG) || (f64::from(x) + 0.5).hypot(f64::from(z) + 0.5) >= CLEARING {
                continue;
            }
            for y in top..(top + 9).min(HEIGHT) {
                for dz in -2..=2 {
                    for dx in -2..=2 {
                        let b = land.get(x + dx, y, z + dz);
                        if matches!(b, OAK_LOG | BIRCH_LOG) || leaves(b) {
                            land.set(x + dx, y, z + dz, AIR);
                        }
                    }
                }
            }
            land.set(x, top - 1, z, GRASS);
        }
    }
    for z in -13..=13 {
        for x in -13..=13 {
            let d = (f64::from(x) + 0.5).hypot(f64::from(z) + 0.5);
            let old = land.top(x, z);
            let top = if d < MEADOW {
                G0
            } else {
                let t = old.max(G0 - ((d - MEADOW) * 1.2).ceil() as i32);
                if t == old {
                    continue;
                }
                t
            };
            for y in 0..HEIGHT {
                let b = land.get(x, y, z);
                if y < top - 4 {
                    if b == AIR || b == WATER_BLOCK {
                        land.set(x, y, z, STONE);
                    }
                } else if y < top - 1 {
                    land.set(x, y, z, DIRT);
                } else if y == top - 1 {
                    land.set(x, y, z, GRASS);
                } else if y < WATER && top < WATER {
                    land.set(x, y, z, WATER_BLOCK);
                } else if !leaves(b) && b != OAK_LOG && b != BIRCH_LOG {
                    land.set(x, y, z, AIR);
                }
            }
            let i = ((z + HALF) * SIZE + (x + HALF)) as usize;
            land.heights[i] = top as i16;
            let r = hash2(x, z, seed + 61);
            land.plants[i] = if d < 2.6 {
                0
            } else if r < 0.12 {
                T_SHORT_GRASS as u8 + 1
            } else if r < 0.135 {
                T_POPPY as u8 + 1
            } else if r < 0.15 {
                T_DANDELION as u8 + 1
            } else if r < 0.16 {
                T_CORNFLOWER as u8 + 1
            } else {
                0
            };
        }
    }
    land
}

// ── the sky (the player's clock) ───────────────────────────────────────

pub struct Sky {
    pub id: &'static str,
    top: u32,
    horizon: u32,
    fog: u32,
    light: f32,
    tint: [f32; 3],
    sun: bool,
    sun_up: f64,
    glow: u32,
    disc: u32,
}

pub const NIGHT: Sky = Sky { id: "night", top: 0x050816, horizon: 0x18223F, fog: 0x18223F, light: 0.34, tint: [0.75, 0.82, 1.0], sun: false, sun_up: 0.14, glow: 0xB8C8FF, disc: 0xDFE6F5 };
pub const SUNSET: Sky = Sky { id: "sunset", top: 0x33477F, horizon: 0xF0A165, fog: 0xE7A77C, light: 0.86, tint: [1.0, 0.9, 0.82], sun: true, sun_up: 0.06, glow: 0xFFA060, disc: 0xFFD9A1 };
pub const DAY: Sky = Sky { id: "day", top: 0x6F9FF2, horizon: 0xC4DBFF, fog: 0xC4DBFF, light: 1.0, tint: [1.0, 1.0, 1.0], sun: true, sun_up: 0.17, glow: 0xFFF4C4, disc: 0xFFF8D8 };

/// The sky for the player's clock, the same hours as the launcher's Home.
pub fn sky_at(hour: u32, minute: u32) -> &'static Sky {
    let h = f64::from(hour) + f64::from(minute) / 60.0;
    if !(5.0..20.5).contains(&h) {
        &NIGHT
    } else if !(7.0..18.0).contains(&h) {
        &SUNSET
    } else {
        &DAY
    }
}

// ── block textures ─────────────────────────────────────────────────────

const NAMES: [&[&str]; TILES] = [
    &["block/grass_block_top"],
    &["block/grass_block_side"],
    &["block/dirt"],
    &["block/stone"],
    &["block/sand"],
    &["block/gravel"],
    &["block/snow"],
    &["block/oak_log"],
    &["block/oak_log_top"],
    &["block/oak_leaves"],
    &["block/birch_log"],
    &["block/birch_log_top"],
    &["block/birch_leaves"],
    &["block/short_grass", "block/grass"],
    &["block/poppy"],
    &["block/dandelion"],
    &["block/cornflower"],
    &["block/water_still"],
];
/// Plains colours: the game tints these grey textures by biome.
const GRASS_TINT: u32 = 0x91BD59;
const FOLIAGE_TINT: u32 = 0x77AB2F;
const BIRCH_TINT: u32 = 0x80A755;
const WATER_TINT: u32 = 0x3F76E4;
/// Without the game's textures: a speckled colour per tile (plants: nothing rather than a square).
const FALLBACK: [u32; TILES] = [
    0x70A848, 0x866043, 0x866043, 0x7D7D7D, 0xDBCFA3, 0x827C7A, 0xF0FBFB, 0x675030, 0xA0824F, 0x488024, 0xD8D7D2,
    0xC4B076, 0x6E9646, 0, 0, 0, 0, 0x2F5CC8,
];

pub type Tiles = Vec<[u32; 256]>;

fn mul(a: u32, b: u32) -> u32 {
    let ch = |s: u32| ((a >> s) & 255) * ((b >> s) & 255) / 255;
    (a & 0xFF000000) | ch(16) << 16 | ch(8) << 8 | ch(0)
}

fn clamp255(v: f64) -> u32 {
    v.clamp(0.0, 255.0) as u32
}

fn speckle(px: &mut [u32; 256], rgb: u32, salt: i32) {
    if rgb == 0 {
        return; // transparent
    }
    for (i, p) in px.iter_mut().enumerate() {
        let k = 1.0 + (hash2(i as i32 & 15, i as i32 >> 4, 977 + salt) - 0.5) * 0.35;
        let c = |s: u32| clamp255(f64::from((rgb >> s) & 255) * k);
        *p = 0xFF000000 | c(16) << 16 | c(8) << 8 | c(0);
    }
}

/// 16 x 16 ARGB tiles: the first frame of each texture, biome-tinted where the game tints it. `texture` reads one of
/// the game's textures by name (the resource packs in use).
pub fn load_tiles(texture: &dyn Fn(&str) -> Option<Arc<Image>>) -> Tiles {
    let find = |names: &[&str]| names.iter().find_map(|n| texture(n).filter(|i| i.width > 0 && i.height > 0));
    let mut out: Tiles = vec![[0u32; 256]; TILES];
    let mut have_side = false;
    for t in 0..TILES {
        let px = &mut out[t];
        let Some(src) = find(NAMES[t]) else {
            speckle(px, FALLBACK[t], t as i32);
            continue;
        };
        have_side |= t == T_GRASS_SIDE;
        let frame = src.width.min(src.height) as usize; // animated textures are vertical strips
        let w = src.width as usize;
        for y in 0..16 {
            for x in 0..16 {
                px[y * 16 + x] = src.argb.get((y * frame / 16) * w + x * frame / 16).copied().unwrap_or(0);
            }
        }
        let tint = match t {
            T_GRASS_TOP | T_SHORT_GRASS => GRASS_TINT,
            T_OAK_LEAVES => FOLIAGE_TINT,
            T_BIRCH_LEAVES => BIRCH_TINT,
            T_WATER => WATER_TINT,
            _ => continue,
        };
        for p in px.iter_mut() {
            *p = (*p & 0xFF000000) | (mul(*p, tint) & 0xFFFFFF);
        }
    }
    // the grass side: dirt and grass with the biome-tinted overlay over its top
    if have_side {
        if let Some(overlay) = find(&["block/grass_block_side_overlay"]) {
            let of = overlay.width.min(overlay.height) as usize;
            let ow = overlay.width as usize;
            for y in 0..16 {
                for x in 0..16 {
                    let o = overlay.argb.get((y * of / 16) * ow + x * of / 16).copied().unwrap_or(0);
                    if o >> 24 > 128 {
                        out[T_GRASS_SIDE][y * 16 + x] = mul(o, GRASS_TINT) | 0xFF000000;
                    }
                }
            }
        }
    } else {
        let top = out[T_GRASS_TOP];
        out[T_GRASS_SIDE][..48].copy_from_slice(&top[..48]);
    }
    out
}

// ── the view ───────────────────────────────────────────────────────────

/// How much of the world the picture shows from top to bottom: a close, zoomed-in look.
const VFOV: f64 = 38.0 * std::f64::consts::PI / 180.0;
/// The camera tilts down a little onto you, the same as the player preview (`title_view`).
pub const PITCH: f64 = 0.12;
/// Where your feet are, as a fraction of the screen height.
pub const FEET: f32 = 0.6;
const PLAYER_BLOCKS: f64 = 1.8;
const FOG_NEAR: f64 = 34.0;
const FOG_FAR: f64 = 80.0;
/// No plants this close to the camera: a blade of grass would fill half the screen.
const PLANTS_FROM: f64 = 3.5;
const CLOUD_CELL: f64 = 12.0;
const CLOUD_N: i32 = 34;
const CLOUD_Y: f64 = G0 as f64 + 34.0;
const AO: [f64; 4] = [0.5, 0.68, 0.84, 1.0];
const PLANT_INSET: f64 = 0.15;

/// How tall your preview is drawn on a screen this tall (UI units): the title screen's own sizing.
pub fn player_height(screen_h: f32) -> f32 {
    32.0 * (screen_h / 84.0).min(9.5)
}

/// The camera: you stand on the flat top of the hill in the middle of the land, the camera a few blocks behind and
/// above you, placed so that ground lands right under your feet at the size you are drawn.
pub struct View {
    pub w: usize,
    pub h: usize,
    f: f64,
    pub origin: [f64; 3],
    fwd: [f64; 3],
    right: [f64; 3],
    up: [f64; 3],
    yaw: f64,
}

impl View {
    /// Facing the direction with the best view.
    pub fn new(land: &Land, w: usize, h: usize, feet: f64, player: f64) -> View {
        View::facing(w, h, feet, player, best_yaw(land, w, h, feet, player))
    }

    fn facing(w: usize, h: usize, feet: f64, player: f64, yaw: f64) -> View {
        let f = (h as f64 / 2.0) / (VFOV / 2.0).tan();
        let (cp, sp, cy, sy) = (PITCH.cos(), PITCH.sin(), yaw.cos(), yaw.sin());
        let fwd = [sy * cp, -sp, cy * cp];
        let right = [cy, 0.0, -sy];
        // up = forward x right
        let up = [fwd[1] * right[2], fwd[2] * right[0] - fwd[0] * right[2], -fwd[1] * right[0]];
        // the ray through your feet, and how deep they are for your size on screen
        let k = (0.5 - feet) * h as f64 / f;
        let depth = PLAYER_BLOCKS * f / (player * h as f64).max(1.0);
        let origin = [
            0.5 - (fwd[0] + up[0] * k) * depth,
            f64::from(G0) - (fwd[1] + up[1] * k) * depth,
            0.5 - (fwd[2] + up[2] * k) * depth,
        ];
        View { w, h, f, origin, fwd, right, up, yaw }
    }

    /// The unit ray through a point of the picture (pixels, from the top left).
    fn ray(&self, px: f64, py: f64) -> [f64; 3] {
        let (a, b) = ((px - self.w as f64 / 2.0) / self.f, (self.h as f64 / 2.0 - py) / self.f);
        let d = [
            self.fwd[0] + self.right[0] * a + self.up[0] * b,
            self.fwd[1] + self.up[1] * b,
            self.fwd[2] + self.right[2] * a + self.up[2] * b,
        ];
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        [d[0] / l, d[1] / l, d[2] / l]
    }
}

/// The direction with the best view: about half of the picture's upper part open sky (not a wall of trees, not an
/// empty plain), mountains in the distance breaking the tie.
fn best_yaw(land: &Land, w: usize, h: usize, feet: f64, player: f64) -> f64 {
    let (mut best, mut best_score) = (0.0, f64::MIN);
    for i in 0..48 {
        let yaw = 2.0 * std::f64::consts::PI * f64::from(i) / 48.0;
        let v = View::facing(w, h, feet, player, yaw);
        let (mut open, mut all) = (0, 0);
        for row in 0..10 {
            let py = (f64::from(row) + 0.5) / 10.0 * v.h as f64 * 0.45;
            for col in 0..24 {
                let px = (f64::from(col) + 0.5) / 24.0 * v.w as f64;
                all += 1;
                if !hits_land(land, v.origin, v.ray(px, py)) {
                    open += 1;
                }
            }
        }
        let mut mountains = 0i32;
        for d in (44..=66).step_by(2) {
            let d = f64::from(d);
            let (x, z) = ((0.5 + yaw.sin() * d).floor() as i32, (0.5 + yaw.cos() * d).floor() as i32);
            if (-HALF..HALF).contains(&x) && (-HALF..HALF).contains(&z) {
                mountains = mountains.max(land.top(x, z));
            }
        }
        let score = -(f64::from(open) / f64::from(all) - 0.5).abs() + f64::from(mountains) * 0.003;
        if score > best_score {
            best_score = score;
            best = yaw;
        }
    }
    best
}

/// Whether a ray meets the land (any solid block or leaves) before the fog hides it.
fn hits_land(land: &Land, o: [f64; 3], d: [f64; 3]) -> bool {
    let (mut x, mut y, mut z) = (o[0].floor() as i32, o[1].floor() as i32, o[2].floor() as i32);
    let step = |v: f64| if v > 0.0 { 1 } else { -1 };
    let (sx, sy, sz) = (step(d[0]), step(d[1]), step(d[2]));
    let delta = |v: f64| if v == 0.0 { f64::MAX } else { (1.0 / v).abs() };
    let (tdx, tdy, tdz) = (delta(d[0]), delta(d[1]), delta(d[2]));
    let first = |v: f64, o: f64, i: i32, td: f64| {
        if v == 0.0 { f64::MAX } else if v > 0.0 { (f64::from(i) + 1.0 - o) * td } else { (o - f64::from(i)) * td }
    };
    let (mut tmx, mut tmy, mut tmz) = (first(d[0], o[0], x, tdx), first(d[1], o[1], y, tdy), first(d[2], o[2], z, tdz));
    let mut t = 0.0;
    while t < FOG_FAR {
        if (y >= HEIGHT && sy > 0) || y < 0 {
            return y < 0;
        }
        if (x < -HALF && sx < 0) || (x >= HALF && sx > 0) || (z < -HALF && sz < 0) || (z >= HALF && sz > 0) {
            return false;
        }
        let b = land.get(x, y, z);
        if opaque(b) || leaves(b) {
            return true;
        }
        if tmx <= tmy && tmx <= tmz {
            x += sx;
            t = tmx;
            tmx += tdx;
        } else if tmy <= tmz {
            y += sy;
            t = tmy;
            tmy += tdy;
        } else {
            z += sz;
            t = tmz;
            tmz += tdz;
        }
    }
    false
}

/// Faces (the launcher's order): +x, -x, +y, -y, +z, -z; their side tile, light and corners.
const NORMALS: [[i32; 3]; 6] = [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]];
const SIDE: [usize; 6] = [1, 1, 0, 2, 1, 1];
const SHADE: [f64; 6] = [0.6, 0.6, 1.0, 0.5, 0.8, 0.8];
const CORNERS: [[[i32; 3]; 4]; 6] = [
    [[1, 0, 1], [1, 0, 0], [1, 1, 0], [1, 1, 1]],
    [[0, 0, 0], [0, 0, 1], [0, 1, 1], [0, 1, 0]],
    [[0, 1, 1], [1, 1, 1], [1, 1, 0], [0, 1, 0]],
    [[0, 0, 0], [1, 0, 0], [1, 0, 1], [0, 0, 1]],
    [[0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]],
    [[1, 0, 0], [0, 0, 0], [0, 1, 0], [1, 1, 0]],
];

/// [face][corner][3 neighbours]: the blocks in front of a face that darken a corner (side, side, diagonal).
fn ao_at() -> &'static [[[[i32; 3]; 3]; 4]; 6] {
    static AO_AT: OnceLock<[[[[i32; 3]; 3]; 4]; 6]> = OnceLock::new();
    AO_AT.get_or_init(|| {
        let mut out = [[[[0; 3]; 3]; 4]; 6];
        for f in 0..6 {
            let n = NORMALS[f];
            let axes: Vec<usize> = (0..3).filter(|&a| n[a] == 0).collect();
            for k in 0..4 {
                let c = CORNERS[f][k];
                let (mut o1, mut o2, mut o3) = (n, n, n);
                let s0 = if c[axes[0]] != 0 { 1 } else { -1 };
                let s1 = if c[axes[1]] != 0 { 1 } else { -1 };
                o1[axes[0]] += s0;
                o2[axes[1]] += s1;
                o3[axes[0]] += s0;
                o3[axes[1]] += s1;
                out[f][k] = [o1, o2, o3];
            }
        }
        out
    })
}

fn ch(rgb: u32, shift: u32) -> f64 {
    f64::from((rgb >> shift) & 255) / 255.0
}

fn texel(v: f64) -> usize {
    ((v * 16.0) as i64).clamp(0, 15) as usize
}

fn unit(v: f64) -> f64 {
    v.clamp(0.0, 0.999999)
}

pub fn mix(a: u32, b: u32, t: f32) -> u32 {
    if t <= 0.0 {
        return a;
    }
    if t >= 1.0 {
        return b;
    }
    let c = |s: u32| {
        let (x, y) = (((a >> s) & 255) as f32, ((b >> s) & 255) as f32);
        ((x + (y - x) * t).round() as u32).min(255)
    };
    0xFF000000 | c(16) << 16 | c(8) << 8 | c(0)
}

/// Traces the picture: one ray per pixel. Each bake thread has its own Baker; what they read is fixed.
struct Baker<'a> {
    land: &'a Land,
    tiles: &'a Tiles,
    sky: &'a Sky,
    view: &'a View,
    sun: [f64; 3],
    right: [f64; 3],
    up: [f64; 3],
    lit: [f64; 3],
}

impl<'a> Baker<'a> {
    fn new(land: &'a Land, tiles: &'a Tiles, sky: &'a Sky, view: &'a View) -> Self {
        // the sun (or moon) a little right of the middle, low enough to be in the picture
        let (yaw, up) = (view.yaw + 0.35, sky.sun_up);
        let sun = [yaw.sin() * up.cos(), up.sin(), yaw.cos() * up.cos()];
        // the sun's square: right = (y axis x sun), up = sun x right
        let rl = sun[2].hypot(sun[0]);
        let right = [sun[2] / rl, 0.0, -sun[0] / rl];
        let u = [sun[1] * right[2], sun[2] * right[0] - sun[0] * right[2], -sun[1] * right[0]];
        let ul = (u[0] * u[0] + u[1] * u[1] + u[2] * u[2]).sqrt();
        let l = f64::from(sky.light);
        Baker {
            land,
            tiles,
            sky,
            view,
            sun,
            right,
            up: [u[0] / ul, u[1] / ul, u[2] / ul],
            lit: [f64::from(sky.tint[0]) * l, f64::from(sky.tint[1]) * l, f64::from(sky.tint[2]) * l],
        }
    }

    /// One row of the picture.
    fn row(&self, out: &mut [u32], y: usize) {
        for (x, px) in out.iter_mut().enumerate() {
            *px = self.trace(self.view.ray(x as f64 + 0.5, y as f64 + 0.5));
        }
    }

    fn trace(&self, d: [f64; 3]) -> u32 {
        let [ox, oy, oz] = self.view.origin;
        let [dx, dy, dz] = d;
        let (mut x, mut y, mut z) = (ox.floor() as i32, oy.floor() as i32, oz.floor() as i32);
        let (sx, sy, sz) = (if dx > 0.0 { 1 } else { -1 }, if dy > 0.0 { 1 } else { -1 }, if dz > 0.0 { 1 } else { -1 });
        let delta = |v: f64| if v == 0.0 { f64::MAX } else { (1.0 / v).abs() };
        let (tdx, tdy, tdz) = (delta(dx), delta(dy), delta(dz));
        let first = |v: f64, o: f64, i: i32, td: f64| {
            if v == 0.0 { f64::MAX } else if v > 0.0 { (f64::from(i) + 1.0 - o) * td } else { (o - f64::from(i)) * td }
        };
        let (mut tmx, mut tmy, mut tmz) = (first(dx, ox, x, tdx), first(dy, oy, y, tdy), first(dz, oz, z, tdz));
        let mut t = 0.0;
        let mut axis = 1;
        let mut prev_leaves = false;
        let mut wet: Option<(u32, f64)> = None;
        while t < FOG_FAR {
            if (y >= HEIGHT && sy > 0) || y < 0 {
                break;
            }
            if (x < -HALF && sx < 0) || (x >= HALF && sx > 0) || (z < -HALF && sz < 0) || (z >= HALF && sz > 0) {
                break;
            }
            let next = tmx.min(tmy).min(tmz);
            let b = self.land.get(x, y, z);
            if b == WATER_BLOCK {
                prev_leaves = false;
                if wet.is_none() {
                    let surface = self.land.get(x, y + 1, z) != WATER_BLOCK;
                    let mut ts = if surface && dy < 0.0 { (f64::from(y) + 0.88 - oy) / dy } else { -1.0 };
                    if !surface || ts < t {
                        ts = t; // entered below the surface
                    }
                    if ts <= next {
                        wet = Some((self.water_at(ox + dx * ts, oz + dz * ts), ts));
                    }
                }
            } else if b != AIR {
                let leafy = leaves(b);
                if !(leafy && prev_leaves) {
                    let face = match axis {
                        0 => if sx > 0 { 1 } else { 0 },
                        1 => if sy > 0 { 3 } else { 2 },
                        _ => if sz > 0 { 5 } else { 4 },
                    };
                    let hit = [ox + dx * t - f64::from(x), oy + dy * t - f64::from(y), oz + dz * t - f64::from(z)];
                    if let Some(c) = self.block(b, x, y, z, face, hit) {
                        return self.finish(c, t, wet);
                    }
                }
                prev_leaves = leafy;
            } else {
                prev_leaves = false;
                if (-HALF..HALF).contains(&x) && (-HALF..HALF).contains(&z) && y >= 1 && self.land.top(x, z) == y {
                    let plant = self.land.plants[((z + HALF) * SIZE + (x + HALF)) as usize];
                    if plant > 0
                        && next > PLANTS_FROM
                        && let Some((c, at)) = self.plant(usize::from(plant - 1), x, y, z, d, t, next)
                    {
                        return self.finish(c, at, wet);
                    }
                }
            }
            if tmx <= tmy && tmx <= tmz {
                x += sx;
                t = tmx;
                tmx += tdx;
                axis = 0;
            } else if tmy <= tmz {
                y += sy;
                t = tmy;
                tmy += tdy;
                axis = 1;
            } else {
                z += sz;
                t = tmz;
                tmz += tdz;
                axis = 2;
            }
        }
        let s = self.sky_color(d);
        match wet {
            Some((water, at)) => mix(self.fogged(s, FOG_FAR), self.fogged(water, at), 0.8),
            None => s,
        }
    }

    /// A lit texel of a block face, `None` where a leaf texel lets the ray through.
    fn block(&self, b: u8, x: i32, y: i32, z: i32, face: usize, hit: [f64; 3]) -> Option<u32> {
        let (lx, ly, lz) = (unit(hit[0]), unit(hit[1]), unit(hit[2]));
        let (u, v) = match face {
            0 => (1.0 - lz, ly),
            1 => (lz, ly),
            2 => (lx, 1.0 - lz),
            3 => (lx, lz),
            4 => (lx, ly),
            _ => (1.0 - lx, ly),
        };
        let mut c = self.tiles[faces_of(b)[SIDE[face]]][texel(1.0 - v) * 16 + texel(u)];
        if c >> 24 < 128 {
            if leaves(b) {
                return None;
            }
            c |= 0xFF000000;
        }
        // the corners' ambient occlusion, blended across the face
        let corner = |k: usize| {
            let o = &ao_at()[face][k];
            let occ = |n: &[i32; 3]| occludes(self.land.get(x + n[0], y + n[1], z + n[2]));
            let (a, s) = (occ(&o[0]), occ(&o[1]));
            AO[if a == 1 && s == 1 { 0 } else { (3 - (a + s + occ(&o[2]))) as usize }]
        };
        let light = (1.0 - u) * (1.0 - v) * corner(0) + u * (1.0 - v) * corner(1) + u * v * corner(2) + (1.0 - u) * v * corner(3);
        Some(self.lit(c, SHADE[face] * light))
    }

    /// The nearer of a plant's two crossed planes that the ray hits within [t0, t1]: its colour and distance.
    #[allow(clippy::too_many_arguments)]
    fn plant(&self, tile: usize, x: i32, y: i32, z: i32, d: [f64; 3], t0: f64, t1: f64) -> Option<(u32, f64)> {
        let k = PLANT_INSET;
        let [ox, oy, oz] = self.view.origin;
        let (px, pz) = (ox - f64::from(x), oz - f64::from(z));
        let mut best: Option<(u32, f64)> = None;
        for plane in 0..2 {
            // plane 0: lx = lz, plane 1: lx + lz = 1
            let den = if plane == 0 { d[0] - d[2] } else { d[0] + d[2] };
            if den.abs() < 1e-9 {
                continue;
            }
            let t = if plane == 0 { -(px - pz) / den } else { (1.0 - px - pz) / den };
            if t < t0 - 1e-9 || t > t1 + 1e-9 || best.is_some_and(|(_, b)| t >= b) {
                continue;
            }
            let (lx, ly) = (px + d[0] * t, oy + d[1] * t - f64::from(y));
            if lx < k || lx > 1.0 - k || !(0.0..=1.0).contains(&ly) {
                continue;
            }
            let u = if plane == 0 { (lx - k) / (1.0 - 2.0 * k) } else { ((1.0 - k) - lx) / (1.0 - 2.0 * k) };
            let c = self.tiles[tile][texel(1.0 - ly) * 16 + texel(u)];
            if c >> 24 < 128 {
                continue;
            }
            best = Some((self.lit(c, 0.92 + 0.08 * ly), t));
        }
        best
    }

    fn water_at(&self, wx: f64, wz: f64) -> u32 {
        let (u, v) = (wx - wx.floor(), wz - wz.floor());
        self.lit(self.tiles[T_WATER][texel(1.0 - v) * 16 + texel(u)] | 0xFF000000, 1.0)
    }

    fn lit(&self, c: u32, light: f64) -> u32 {
        let r = clamp255(f64::from((c >> 16) & 255) * light * self.lit[0]);
        let g = clamp255(f64::from((c >> 8) & 255) * light * self.lit[1]);
        let b = clamp255(f64::from(c & 255) * light * self.lit[2]);
        0xFF000000 | r << 16 | g << 8 | b
    }

    fn finish(&self, c: u32, t: f64, wet: Option<(u32, f64)>) -> u32 {
        let out = self.fogged(c, t);
        match wet {
            Some((water, at)) => mix(out, self.fogged(water, at), 0.8),
            None => out,
        }
    }

    fn fogged(&self, c: u32, t: f64) -> u32 {
        let f = ((t - FOG_NEAR) / (FOG_FAR - FOG_NEAR)).clamp(0.0, 1.0) as f32;
        mix(c, 0xFF000000 | self.sky.fog, f)
    }

    /// The sky: the clock's gradient, a glow around the sun, square stars at night, the sun or moon, clouds.
    fn sky_color(&self, d: [f64; 3]) -> u32 {
        let [dx, dy, dz] = d;
        let sky = self.sky;
        let (from, to, k) = if dy < 0.0 { (sky.horizon, sky.fog, smooth(0.0, -0.08, dy)) } else { (sky.horizon, sky.top, smooth(0.0, 0.3, dy)) };
        let mut rgb = [16, 8, 0].map(|s| ch(from, s) + (ch(to, s) - ch(from, s)) * k);
        let toward = (dx * self.sun[0] + dy * self.sun[1] + dz * self.sun[2]).max(0.0);
        let t2 = toward * toward;
        let mut glow = (t2 * t2 * t2 * 0.32 + toward.powi(48) * 0.4) * if sky.sun { 1.0 } else { 0.3 };
        // the halo: a soft disc of light around the sun (or moon)
        let ang = toward.min(1.0).acos();
        if toward > 0.0 && ang < 0.18 {
            glow += 0.55 * (1.0 - ang / 0.18);
        }
        for (i, s) in [16, 8, 0].into_iter().enumerate() {
            rgb[i] += ch(sky.glow, s) * glow;
        }
        // the game's flat clouds overhead, seen from below, thinning out into the distance
        let mut cloud_k = 0.0f32;
        if dy > 0.02 {
            let [ox, oy, oz] = self.view.origin;
            let tc = (CLOUD_Y - oy) / dy;
            let (px, pz) = (ox + dx * tc, oz + dz * tc);
            let (i, j) = ((px / CLOUD_CELL).floor() as i32 + CLOUD_N / 2, (pz / CLOUD_CELL).floor() as i32 + CLOUD_N / 2);
            if (0..CLOUD_N).contains(&i) && (0..CLOUD_N).contains(&j) && noise(f64::from(i) / 3.2, f64::from(j) / 3.2, self.land.seed + 77) > 0.6 {
                cloud_k = 0.72 * (1.0 - smooth(60.0, 240.0, tc)) as f32;
            }
        }
        if !sky.sun && cloud_k < 0.3 {
            let (cx, cy, cz) = ((dx * 150.0).floor() as i32, (dy * 150.0).floor() as i32, (dz * 150.0).floor() as i32);
            if hash2(cx.wrapping_mul(7919).wrapping_add(cz), cy, 4242) > 0.9965 {
                let s = smooth(0.04, 0.3, dy) * (0.5 + 0.5 * hash2(cx, cz, cy + 17));
                rgb.iter_mut().for_each(|c| *c += s);
            }
        }
        let mut out = 0xFF000000 | clamp255(rgb[0] * 255.0) << 16 | clamp255(rgb[1] * 255.0) << 8 | clamp255(rgb[2] * 255.0);
        // the square sun or moon, its edges level with the horizon
        if toward > 0.5 {
            let a = (dx * self.right[0] + dz * self.right[2]) / toward;
            let c = (dx * self.up[0] + dy * self.up[1] + dz * self.up[2]) / toward;
            let half = if sky.sun { 0.044 } else { 0.032 };
            if a.abs() < half && c.abs() < half {
                out = 0xFF000000 | sky.disc;
            }
        }
        if cloud_k > 0.0 {
            let cl = f64::from(sky.light);
            let cloud = 0xFF000000
                | clamp255(cl.max(0.3) * 0.95 * 255.0) << 16
                | clamp255((cl * 0.97).max(0.3) * 0.95 * 255.0) << 8
                | clamp255((cl * 0.95).max(0.35) * 0.97 * 255.0);
            out = mix(out, cloud, cloud_k);
        }
        out
    }
}

/// The picture for a framebuffer this big: its own size, at most 1080 pixels tall (it is soft-scaled up).
pub fn picture_size(fb_w: i32, fb_h: i32) -> (usize, usize) {
    let k = (1080.0 / f64::from(fb_h.max(1))).min(1.0);
    let w = (f64::from(fb_w) * k).round().clamp(16.0, 4096.0) as usize;
    let h = (f64::from(fb_h) * k).round().max(16.0) as usize;
    (w, h)
}

/// Bakes a picture on `threads` threads (the background bake and the tests).
pub fn bake(land: &Land, tiles: &Tiles, sky: &Sky, view: &View, threads: usize) -> Arc<Image> {
    let (w, h) = (view.w, view.h);
    let mut px = vec![0u32; w * h];
    let next = AtomicUsize::new(0);
    const BAND: usize = 8;
    {
        let bands: Vec<Mutex<&mut [u32]>> = px.chunks_mut(w * BAND).map(Mutex::new).collect();
        std::thread::scope(|scope| {
            for _ in 0..threads.max(1) {
                scope.spawn(|| {
                    let baker = Baker::new(land, tiles, sky, view);
                    loop {
                        let band = next.fetch_add(1, Ordering::Relaxed);
                        let Some(slot) = bands.get(band) else { break };
                        let mut rows = slot.lock().unwrap_or_else(|e| e.into_inner());
                        for (i, row) in rows.chunks_mut(w).enumerate() {
                            baker.row(row, band * BAND + i);
                        }
                    }
                });
            }
        });
    }
    // soft when scaled up (images are linear by default)
    Image::new(w as u32, h as u32, px)
}

// ── on the title screen ────────────────────────────────────────────────

/// The world breathes: a slow zoom of a few percent around your feet, so they stay planted on the ground.
const BREATH: f64 = 0.03;
const BREATH_S: f64 = 28.0;
const FADE_MS: f64 = 700.0;

#[derive(Default)]
struct Shown {
    img: Option<Arc<Image>>,
    previous: Option<Arc<Image>>,
    key: String,
    baking: Option<String>,
    failed: Option<String>,
    at: Option<Instant>,
    land: Option<Arc<Land>>,
}

fn shown() -> &'static Mutex<Shown> {
    static SHOWN: OnceLock<Mutex<Shown>> = OnceLock::new();
    SHOWN.get_or_init(Mutex::default)
}

fn start() -> Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    *START.get_or_init(Instant::now)
}

/// The instance's name (its game folder), so this is the same world as the launcher's Home for it.
fn seed_name() -> &'static str {
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(|| {
        super::game_dir()
            .and_then(|d| std::fs::canonicalize(&d).ok().or(Some(d)))
            .and_then(|d| d.file_name().map(|n| n.to_string_lossy().into_owned()))
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "native".into())
    })
}

fn clock_sky() -> &'static Sky {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
    let day = (now + super::common::utc_offset_ms()).rem_euclid(86_400_000) / 60_000;
    sky_at((day / 60) as u32, (day % 60) as u32)
}

/// Starts the bake when the sky, the resources or the screen size asks for a new picture and returns how far the
/// baked world has faded in: 0 = nothing to draw yet (the panorama behind shows), 1 = it covers everything behind it.
pub fn prepare(c: &Canvas, w: f32, h: f32) -> f32 {
    let sky = clock_sky();
    let (pw, ph) = picture_size(c.px(w), c.px(h));
    let player = (player_height(h) / h * 100.0).round() / 100.0;
    let name = seed_name();
    let key = format!("{name}|{}|{}|{pw}x{ph}|{player}", sky.id, super::resources_generation());
    let mut s = shown().lock().unwrap_or_else(|e| e.into_inner());
    if s.key != key && s.baking.is_none() && s.failed.as_deref() != Some(key.as_str()) {
        s.baking = Some(key.clone());
        let land = s.land.clone();
        let spawned = std::thread::Builder::new().name("Native title world".into()).spawn(move || {
            let result = std::panic::catch_unwind(|| {
                let seed = seed_of(name);
                let land = land.filter(|l| l.seed == seed).unwrap_or_else(|| Arc::new(stand_on(generate(seed))));
                let tiles = load_tiles(&|n| super::game_texture(n));
                let threads = std::thread::available_parallelism().map_or(1, |n| n.get().saturating_sub(2).clamp(1, 4));
                let view = View::new(&land, pw, ph, f64::from(FEET), f64::from(player));
                (bake(&land, &tiles, sky, &view, threads), land)
            });
            let mut s = shown().lock().unwrap_or_else(|e| e.into_inner());
            s.baking = None;
            match result {
                Ok((img, land)) => {
                    s.previous = s.img.take();
                    s.img = Some(img);
                    s.at = None;
                    s.key = key;
                    s.land = Some(land);
                }
                Err(_) => s.failed = Some(key),
            }
        });
        if spawned.is_err() {
            s.baking = None;
        }
    }
    if s.img.is_none() {
        return 0.0;
    }
    let at = *s.at.get_or_insert_with(Instant::now);
    if s.previous.is_some() {
        return 1.0; // a new picture fading in over the old one
    }
    (at.elapsed().as_secs_f64() * 1000.0 / FADE_MS).min(1.0) as f32
}

/// Draws the baked world (after [`prepare`]) over the box, a new one fading in over an old one.
pub fn draw(c: &mut Canvas, x: f32, y: f32, w: f32, h: f32) {
    let (img, old, k) = {
        let mut s = shown().lock().unwrap_or_else(|e| e.into_inner());
        let Some(img) = s.img.clone() else { return };
        let k = s.at.map_or(0.0, |at| (at.elapsed().as_secs_f64() * 1000.0 / FADE_MS).min(1.0)) as f32;
        if k >= 1.0 {
            s.previous = None;
        }
        (img, s.previous.clone(), k)
    };
    if let Some(old) = &old {
        picture(c, old, x, y, w, h, 0xFFFFFFFF);
    }
    picture(c, &img, x, y, w, h, horizon_client::native_ui::theme::alpha(0xFFFFFFFF, k));
}

/// The picture over the box, zoomed in and out a little around your feet.
fn picture(c: &mut Canvas, img: &Arc<Image>, x: f32, y: f32, w: f32, h: f32, argb: u32) {
    let phase = start().elapsed().as_secs_f64() / BREATH_S * 2.0 * std::f64::consts::PI;
    let zoom = (1.0 + BREATH * (1.0 - phase.cos()) / 2.0) as f32;
    let (u0, u1) = (0.5 - 0.5 / zoom, 0.5 + 0.5 / zoom);
    let (v0, v1) = (FEET - FEET / zoom, FEET + (1.0 - FEET) / zoom);
    c.image(img, x, y, w, h, [u0, v0, u1, v1], argb);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The launcher's worldGen.js gives these (seedOf, then FNV-1a over the blocks and the heights).
    const LAUNCHER: [(&str, i32, i32, i32); 4] = [
        ("native", 586730, 925421129, -1905062880),
        ("Survival 1.21", 251136, -605929283, 2110113274),
        ("a1b2c3-instance", 702467, -989170605, -1524129885),
        ("\u{dc}n\u{ef}code\u{2713}", 479754, 391309411, 1350093221),
    ];

    #[test]
    fn builds_the_same_land_as_the_launcher() {
        for (name, seed, blocks, heights) in LAUNCHER {
            assert_eq!(seed_of(name), seed, "seed of {name}");
            let land = generate(seed);
            let fnv = |it: &mut dyn Iterator<Item = u32>| it.fold(2166136261u32, |h, v| (h ^ v).wrapping_mul(16777619)) as i32;
            assert_eq!(fnv(&mut land.blocks.iter().map(|&b| u32::from(b))), blocks, "blocks of {name}");
            assert_eq!(fnv(&mut land.heights.iter().map(|&v| u32::from(v as u16))), heights, "heights of {name}");
        }
    }

    #[test]
    fn bakes_you_standing_in_the_world() {
        let land = stand_on(generate(seed_of("native")));
        let tiles = load_tiles(&|_| None);
        let (w, h) = picture_size(960, 540);
        let view = View::new(&land, w, h, f64::from(FEET), f64::from(player_height(540.0) / 540.0));
        // the camera is behind you and above the ground you stand on
        assert!(view.origin[1] > f64::from(G0) && view.origin[1] < f64::from(G0) + 4.0, "camera height {}", view.origin[1]);
        let a = bake(&land, &tiles, &DAY, &view, 4);
        let b = bake(&land, &tiles, &DAY, &view, 1);
        assert_eq!((a.width, a.height), (960, 540));
        assert!(a.argb == b.argb, "the same picture on any number of threads");
        let feet = a.argb[((540.0 * FEET) as usize + 3) * 960 + 480];
        let ch = |c: u32, s: u32| (c >> s) & 255;
        assert!(ch(feet, 8) > ch(feet, 16) && ch(feet, 8) > ch(feet, 0), "grass under your feet: {feet:08x}");
    }
}
