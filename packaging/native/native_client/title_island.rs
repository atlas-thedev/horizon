//! `TitleIsland`: the title screen's floating island, a few real blocks (grass, dirt, stone, a bush, flowers)
//! the player stands on, drawn with the game's own block textures (resource packs included) in the same
//! orthographic view as `PlayerPreview`, so the feet land on the grass. One quad per block face (back faces
//! culled, sorted back to front), one texture for all of it. The same as the Java mod's `TitleIsland`.
//!
//! Model units are pixels (16 per block) with y pointing down; the player's feet are at y = 16, on top of block
//! (0, 0, 0).

use horizon_client::native_ui::{Canvas, Image};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

const GRASS: u8 = 1;
const DIRT: u8 = 2;
const STONE: u8 = 3;
const LEAVES: u8 = 5;

const T_GRASS_TOP: usize = 0;
const T_GRASS_SIDE: usize = 1;
const T_DIRT: usize = 2;
const T_STONE: usize = 3;
const T_LEAVES: usize = 4;
const T_SHORT_GRASS: usize = 5;
const T_POPPY: usize = 6;
const T_DANDELION: usize = 7;
const T_CORNFLOWER: usize = 8;
const TILES: usize = 9;
const SIZE: usize = 16;

const NAMES: [&[&str]; TILES] = [
    &["block/grass_block_top"],
    &["block/grass_block_side"],
    &["block/dirt"],
    &["block/stone"],
    &["block/oak_leaves"],
    &["block/short_grass", "block/grass"],
    &["block/poppy"],
    &["block/dandelion"],
    &["block/cornflower"],
];
/// Plains colours: the game tints these grey textures by biome.
const GRASS_TINT: u32 = 0xFF91BD59;
const FOLIAGE_TINT: u32 = 0xFF77AB2F;
const FALLBACK: [u32; TILES] = [0xFF6FA548, 0xFF866043, 0xFF866043, 0xFF7D7D7D, 0xFF4C8A2A, 0, 0, 0, 0];

struct Island {
    blocks: Vec<(u8, i32, i32, i32)>,
    at: HashMap<(i32, i32, i32), u8>,
    /// (tile, i, j, k): a crossed-plane plant standing on the block under it.
    plants: Vec<(usize, i32, i32, i32)>,
}

fn island() -> &'static Island {
    static ISLAND: OnceLock<Island> = OnceLock::new();
    ISLAND.get_or_init(|| {
        let mut is = Island { blocks: Vec::new(), at: HashMap::new(), plants: Vec::new() };
        let put = |is: &mut Island, b: u8, i: i32, j: i32, k: i32| {
            is.at.insert((i, j, k), b);
            is.blocks.push((b, i, j, k));
        };
        // a small floating island, sized to the player: a 3 x 3 grass top, dirt and stone tapering under it
        for i in -1..=1 {
            for k in -1..=1 {
                put(&mut is, GRASS, i, 0, k);
                if i.abs() + k.abs() <= 1 {
                    put(&mut is, DIRT, i, 1, k);
                }
            }
        }
        put(&mut is, DIRT, 1, 1, 1);
        put(&mut is, STONE, 0, 2, 0);
        put(&mut is, STONE, 1, 2, 0);
        put(&mut is, STONE, 0, 3, 0);
        // a leafy bush at the back left corner, flowers and grass around the player
        put(&mut is, LEAVES, -1, -1, 1);
        is.plants.extend([
            (T_POPPY, 1, -1, -1),
            (T_DANDELION, 1, -1, 1),
            (T_CORNFLOWER, -1, -1, -1),
            (T_SHORT_GRASS, 0, -1, 1),
            (T_SHORT_GRASS, -1, -1, 0),
            (T_SHORT_GRASS, 1, -1, 0),
        ]);
        is
    })
}

fn mul(a: u32, b: u32) -> u32 {
    let ch = |s: u32| ((a >> s) & 255) * ((b >> s) & 255) / 255;
    (a & 0xFF000000) | ch(16) << 16 | ch(8) << 8 | ch(0)
}

fn tile(t: usize) -> Vec<u32> {
    let src = NAMES[t].iter().find_map(|n| super::game_texture(n));
    let mut out = vec![FALLBACK[t]; SIZE * SIZE];
    let Some(src) = src else {
        if t == T_GRASS_SIDE {
            out[..SIZE * 4].fill(FALLBACK[T_GRASS_TOP]);
        }
        return out;
    };
    // animated textures are vertical strips: the first frame
    let frame = src.width.min(src.height) as usize;
    let w = src.width as usize;
    for y in 0..SIZE {
        for x in 0..SIZE {
            out[y * SIZE + x] = src.argb.get((y * frame / SIZE) * w + x * frame / SIZE).copied().unwrap_or(0);
        }
    }
    let tint = match t {
        T_GRASS_TOP | T_SHORT_GRASS => GRASS_TINT,
        T_LEAVES => FOLIAGE_TINT,
        _ => 0,
    };
    if tint != 0 {
        for p in &mut out {
            *p = mul(*p, tint);
        }
    }
    if t == T_GRASS_SIDE
        && let Some(overlay) = super::game_texture("block/grass_block_side_overlay")
    {
        // the side: dirt with the biome-tinted overlay over its top
        let of = overlay.width.min(overlay.height) as usize;
        let ow = overlay.width as usize;
        for y in 0..SIZE {
            for x in 0..SIZE {
                let o = overlay.argb.get((y * of / SIZE) * ow + x * of / SIZE).copied().unwrap_or(0);
                if o >> 24 > 128 {
                    out[y * SIZE + x] = mul(o, GRASS_TINT) | 0xFF000000;
                }
            }
        }
    }
    out
}

/// One strip of 16 x 16 tiles, rebuilt when the game's resources change.
fn atlas() -> Arc<Image> {
    static ATLAS: Mutex<Option<(usize, Arc<Image>)>> = Mutex::new(None);
    let generation = super::resources_generation();
    let mut cached = ATLAS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((g, img)) = cached.as_ref()
        && *g == generation
    {
        return img.clone();
    }
    let mut px = vec![0u32; SIZE * TILES * SIZE];
    for t in 0..TILES {
        let tile = tile(t);
        for y in 0..SIZE {
            px[y * SIZE * TILES + t * SIZE..y * SIZE * TILES + (t + 1) * SIZE].copy_from_slice(&tile[y * SIZE..(y + 1) * SIZE]);
        }
    }
    let mut img = Image::new((SIZE * TILES) as u32, SIZE as u32, px);
    if let Some(i) = Arc::get_mut(&mut img) {
        i.linear = false;
    }
    *cached = Some((generation, img.clone()));
    img
}

fn tile_of(block: u8, face: usize) -> usize {
    match block {
        GRASS => match face {
            0 => T_GRASS_TOP,
            1 => T_DIRT,
            _ => T_GRASS_SIDE,
        },
        DIRT => T_DIRT,
        STONE => T_STONE,
        _ => T_LEAVES,
    }
}

/// Face corners (offsets of a block's min corner, in blocks): top, bottom, -z, +z, -x, +x.
const FACE: [[f32; 12]; 6] = [
    [0., 0., 0., 1., 0., 0., 1., 0., 1., 0., 0., 1.],
    [0., 1., 1., 1., 1., 1., 1., 1., 0., 0., 1., 0.],
    [0., 0., 0., 1., 0., 0., 1., 1., 0., 0., 1., 0.],
    [1., 0., 1., 0., 0., 1., 0., 1., 1., 1., 1., 1.],
    [0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 1., 1.],
    [1., 0., 0., 1., 0., 1., 1., 1., 1., 1., 1., 0.],
];
const NORMAL: [[i32; 3]; 6] = [[0, -1, 0], [0, 1, 0], [0, 0, -1], [0, 0, 1], [-1, 0, 0], [1, 0, 0]];
/// The game's flat shading: the top brightest.
const LIGHT: [f32; 6] = [1.0, 0.5, 0.8, 0.8, 0.62, 0.62];

struct Quad {
    xy: [f32; 8],
    uv: [f32; 8],
    z: f32,
    argb: u32,
}

/// Draws the island behind/under the player in the player's projection.
pub fn draw(c: &mut Canvas, cx: f32, cy: f32, scale: f32, yaw: f32, pitch: f32) {
    draw_layer(c, cx, cy, scale, yaw, pitch, false);
}

/// Draws the above-ground plants and bush closer to the camera than the player, after the player is drawn.
pub fn draw_front(c: &mut Canvas, cx: f32, cy: f32, scale: f32, yaw: f32, pitch: f32) {
    draw_layer(c, cx, cy, scale, yaw, pitch, true);
}

fn draw_layer(c: &mut Canvas, cx: f32, cy: f32, scale: f32, yaw: f32, pitch: f32, front: bool) {
    let (cyw, syw, cp, sp) = (yaw.cos(), yaw.sin(), pitch.cos(), pitch.sin());
    // view = Rx(pitch) * Ry(yaw), the same as Boxes
    let view = [cyw, 0.0, syw, sp * syw, cp, -sp * cyw, -cp * syw, sp, cp * cyw];
    let is = island();
    let tex = atlas();
    let mut quads: Vec<Quad> = Vec::with_capacity(160);
    let mut add = |corners: &[f32; 12], x0: f32, y0: f32, z0: f32, unit: f32, tile: usize, light: f32| {
        let mut q = Quad { xy: [0.0; 8], uv: [0.0; 8], z: 0.0, argb: 0 };
        for v in 0..4 {
            let (x, y, z) = (x0 + corners[v * 3] * unit, y0 + corners[v * 3 + 1] * unit, z0 + corners[v * 3 + 2] * unit);
            q.xy[v * 2] = cx + (view[0] * x + view[1] * y + view[2] * z) * scale;
            q.xy[v * 2 + 1] = cy + (view[3] * x + view[4] * y + view[5] * z) * scale;
            q.z += (view[6] * x + view[7] * y + view[8] * z) / 4.0;
        }
        let (u0, u1) = (tile as f32 / TILES as f32, (tile + 1) as f32 / TILES as f32);
        q.uv = [u0, 0.0, u1, 0.0, u1, 1.0, u0, 1.0];
        let l = (255.0 * light).round() as u32;
        q.argb = 0xFF000000 | l << 16 | l << 8 | l;
        quads.push(q);
    };
    for &(block, i, j, k) in &is.blocks {
        let in_front = j < 0 && (view[6] * i as f32 + view[8] * k as f32) < 0.0;
        if in_front != front {
            continue;
        }
        for f in 0..6 {
            let n = NORMAL[f];
            let nb = is.at.get(&(i + n[0], j + n[1], k + n[2])).copied().unwrap_or(0);
            if nb != 0 && (nb != LEAVES || block == LEAVES) {
                continue; // hidden behind its neighbour (leaves show what is behind them)
            }
            // back faces point away from the camera (larger view z is farther)
            let nz = view[6] * n[0] as f32 + view[7] * n[1] as f32 + view[8] * n[2] as f32;
            if nz > 0.001 {
                continue;
            }
            let (x0, y0, z0) = (i as f32 * 16.0 - 8.0, 16.0 + j as f32 * 16.0, k as f32 * 16.0 - 8.0);
            add(&FACE[f], x0, y0, z0, 16.0, tile_of(block, f), LIGHT[f]);
        }
    }
    for &(t, i, j, k) in &is.plants {
        let in_front = j < 0 && (view[6] * i as f32 + view[8] * k as f32) < 0.0;
        if in_front != front {
            continue;
        }
        // two crossed planes through the block's middle, both sides drawn
        let (x0, y0, z0, s, inset) = (i as f32 * 16.0 - 8.0, 16.0 + j as f32 * 16.0, k as f32 * 16.0 - 8.0, 16.0, 2.4);
        let a = [inset, 0.0, inset, s - inset, 0.0, s - inset, s - inset, s, s - inset, inset, s, inset];
        let d = [s - inset, 0.0, inset, inset, 0.0, s - inset, inset, s, s - inset, s - inset, s, inset];
        add(&a, x0, y0, z0, 1.0, t, 0.95);
        add(&d, x0, y0, z0, 1.0, t, 0.95);
    }
    if quads.is_empty() {
        return;
    }
    // farthest first
    quads.sort_by(|a, b| b.z.partial_cmp(&a.z).unwrap_or(std::cmp::Ordering::Equal));
    for q in &quads {
        c.free_quad(Some(&tex), q.xy, q.uv, q.argb);
    }
}
