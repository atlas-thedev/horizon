//! The banner under the classic title screen's logo: Native Client's
//! "EXPERIMENTAL" with the Rust cog in front of it.
//!
//! Vanilla's `textures/gui/title/edition.png` reads "JAVA EDITION"; Horizon
//! never ships Mojang's art, so this banner is rebuilt at load time with its
//! own glyphs (rectangles in this file), drawn the way the texture's letters
//! are built (a face with darker sides and a light bottom edge, a dark
//! extrusion under it, a black outline and a soft rim) in the shades sampled
//! from the player's own texture. Only vanilla 26.3's exact texture is
//! rebuilt: a resource pack's edition.png stays as its author drew it.

use std::collections::HashMap;
use std::ops::Range;

/// The atlas sprite.
pub const SPRITE: &str = "gui/title/edition";
const WIDTH: usize = 512;
const HEIGHT: usize = 64;
/// FNV-1a over the size and RGBA pixels of 26.3's edition.png.
const VANILLA: u64 = 0x4249_0538_6dfb_181f;
/// Where the letters put their faces, and how deep they are drawn.
const FACE_TOP: usize = 4;
const EXTRUSION: usize = 8;
const OUTLINE: usize = 3;
/// A letter is 5x7 cells of this many texels; letters are one gap apart.
const CELL_W: usize = 6;
const CELL_H: usize = 5;
const LETTER_GAP: usize = 6;
/// The cog in front of the word: its box and the space after it.
const COG_W: usize = 40;
const COG_H: usize = 39;
const COG_GAP: usize = 12;

const WORD: &str = "EXPERIMENTAL";

/// 5x7 letter bitmaps, one string per row.
fn letter(c: char) -> [&'static str; 7] {
    match c {
        'E' => ["#####", "#....", "#....", "####.", "#....", "#....", "#####"],
        'X' => ["#...#", "#...#", ".#.#.", "..#..", ".#.#.", "#...#", "#...#"],
        'P' => ["####.", "#...#", "#...#", "####.", "#....", "#....", "#...."],
        'R' => ["####.", "#...#", "#...#", "####.", "#.#..", "#..#.", "#...#"],
        'I' => ["#####", "..#..", "..#..", "..#..", "..#..", "..#..", "#####"],
        'M' => ["#...#", "##.##", "#.#.#", "#.#.#", "#...#", "#...#", "#...#"],
        'N' => ["#...#", "##..#", "#.#.#", "#..##", "#...#", "#...#", "#...#"],
        'T' => ["#####", "..#..", "..#..", "..#..", "..#..", "..#..", "..#.."],
        'A' => [".###.", "#...#", "#...#", "#####", "#...#", "#...#", "#...#"],
        'L' => ["#....", "#....", "#....", "#....", "#....", "#....", "#####"],
        _ => ["....."; 7],
    }
}

/// The banner rebuilt to read "EXPERIMENTAL", or `None` when the texture is
/// not vanilla's (a resource pack's own).
pub fn rust_edition(width: u32, height: u32, pixels: &[u8]) -> Option<Vec<u8>> {
    (fingerprint(width, height, pixels) == VANILLA).then(|| compose(pixels))
}

fn fingerprint(width: u32, height: u32, pixels: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let bytes = width.to_le_bytes().into_iter().chain(height.to_le_bytes());
    for b in bytes.chain(pixels.iter().copied()) {
        hash = (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

type Rgba = [u8; 4];

fn lum(p: Rgba) -> u32 {
    (u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2])) / 3
}

fn pixel(pixels: &[u8], x: usize, y: usize) -> Rgba {
    let i = (y * WIDTH + x) * 4;
    [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
}

/// The texture's shades: the most common colour in each brightness band.
struct Palette {
    highlight: Rgba,
    face: Rgba,
    rim: Rgba,
    extrusion: Rgba,
    edge: Rgba,
    outline: Rgba,
    soft: Rgba,
}

impl Palette {
    fn sample(pixels: &[u8]) -> Self {
        let mut bands: [HashMap<Rgba, u32>; 7] = Default::default();
        for &p in pixels.as_chunks::<4>().0 {
            let band = match (p[3], lum(p)) {
                (0, _) => continue,
                (255, 215..) => 0,
                (255, 170..) => 1,
                (255, 100..) => 2,
                (255, 45..) => 3,
                (255, 20..) => 4,
                (255, _) => 5,
                _ => 6,
            };
            *bands[band].entry(p).or_default() += 1;
        }
        let mode = |band: &HashMap<Rgba, u32>, fallback: Rgba| {
            band.iter()
                .max_by_key(|&(p, n)| (*n, std::cmp::Reverse(*p)))
                .map_or(fallback, |(p, _)| *p)
        };
        let outline = mode(&bands[5], [0, 0, 0, 255]);
        let face = mode(&bands[1], [200, 200, 200, 255]);
        Self {
            highlight: mode(&bands[0], face),
            face,
            rim: mode(&bands[2], face),
            extrusion: mode(&bands[3], outline),
            edge: mode(&bands[4], outline),
            outline,
            soft: mode(&bands[6], outline),
        }
    }
}

/// Draw faces within `columns` the way the texture's letters are drawn.
fn draw_glyphs(out: &mut [u8], face: &[bool], palette: &Palette, columns: Range<usize>) {
    let at = |grid: &[bool], x: isize, y: isize| {
        (columns.start as isize..columns.end as isize).contains(&x)
            && (0..HEIGHT as isize).contains(&y)
            && grid[y as usize * WIDTH + x as usize]
    };
    let mut body = face.to_vec();
    for y in 0..HEIGHT {
        for x in columns.clone() {
            let (xi, yi) = (x as isize, y as isize);
            body[y * WIDTH + x] |= (1..=EXTRUSION as isize).any(|k| at(face, xi, yi - k));
        }
    }
    let reach = OUTLINE as isize + 1;
    for y in 0..HEIGHT {
        for x in columns.clone() {
            let (xi, yi) = (x as isize, y as isize);
            let colour = if at(face, xi, yi) {
                if !at(face, xi, yi + 1) {
                    palette.highlight
                } else if !at(face, xi - 1, yi) || !at(face, xi + 1, yi) {
                    palette.rim
                } else {
                    palette.face
                }
            } else if at(&body, xi, yi) {
                if at(face, xi, yi - 1) {
                    palette.rim
                } else if !at(&body, xi - 1, yi) || !at(&body, xi + 1, yi) || !at(&body, xi, yi + 1)
                {
                    palette.edge
                } else {
                    palette.extrusion
                }
            } else {
                let distance = (-reach..=reach)
                    .flat_map(|dy| (-reach..=reach).map(move |dx| (dx, dy)))
                    .filter(|&(dx, dy)| at(&body, xi + dx, yi + dy))
                    .map(|(dx, dy)| dx.abs().max(dy.abs()))
                    .min();
                match distance {
                    Some(d) if d <= OUTLINE as isize => palette.outline,
                    Some(_) => palette.soft,
                    None => continue,
                }
            };
            put(out, x, y, colour);
        }
    }
}

/// Paint `p` over what is there unless that is more opaque, or as opaque and
/// brighter (a letter's body wins over a neighbour's outline).
fn put(out: &mut [u8], x: usize, y: usize, p: Rgba) {
    let old = pixel(out, x, y);
    if (p[3], lum(p)) > (old[3], lum(old)) {
        out[(y * WIDTH + x) * 4..][..4].copy_from_slice(&p);
    }
}

/// Total width of the cog, the gap and the word.
fn total_width() -> usize {
    let letters = WORD.len();
    COG_W + COG_GAP + letters * 5 * CELL_W + (letters - 1) * LETTER_GAP
}

/// The Rust cog: a ring with eight teeth around a small "R".
fn cog_faces(face: &mut [bool], left: usize) {
    let (cx, cy) = (COG_W as f32 / 2.0, COG_H as f32 / 2.0);
    let r_glyph = ["##.", "#.#", "##.", "#.#", "#.#"];
    for y in 0..COG_H {
        for x in 0..COG_W {
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            let r = dx.hypot(dy);
            let turn = dy.atan2(dx) / std::f32::consts::TAU * 8.0;
            let tooth = (turn - turn.floor() - 0.5).abs() < 0.2;
            let ring = r <= 16.0 || (r <= 19.5 && tooth);
            if ring && r >= 12.5 {
                face[(FACE_TOP + y) * WIDTH + left + x] = true;
            }
        }
    }
    // The R inside, 3x5 cells of 3x3 texels.
    let (gx, gy) = (left + COG_W / 2 - 5, FACE_TOP + COG_H / 2 - 7);
    for (row, line) in r_glyph.iter().enumerate() {
        for (col, c) in line.chars().enumerate() {
            if c == '#' {
                for y in 0..3 {
                    for x in 0..3 {
                        face[(gy + row * 3 + y) * WIDTH + gx + col * 3 + x] = true;
                    }
                }
            }
        }
    }
}

/// Faces of the cog and the word, centred in the texture.
fn glyph_faces() -> Vec<bool> {
    let mut face = vec![false; WIDTH * HEIGHT];
    let start = (WIDTH - total_width()) / 2;
    cog_faces(&mut face, start);
    let mut left = start + COG_W + COG_GAP;
    for c in WORD.chars() {
        for (row, line) in letter(c).iter().enumerate() {
            for (col, cell) in line.chars().enumerate() {
                if cell == '#' {
                    for y in 0..CELL_H {
                        for x in 0..CELL_W {
                            let (px, py) = (left + col * CELL_W + x, FACE_TOP + row * CELL_H + y);
                            face[py * WIDTH + px] = true;
                        }
                    }
                }
            }
        }
        left += 5 * CELL_W + LETTER_GAP;
    }
    face
}

/// "EXPERIMENTAL" with the cog; `pixels` is a 512x64 RGBA banner laid out as
/// vanilla's, used for its shades.
fn compose(pixels: &[u8]) -> Vec<u8> {
    let palette = Palette::sample(pixels);
    let mut out = vec![0; WIDTH * HEIGHT * 4];
    draw_glyphs(&mut out, &glyph_faces(), &palette, 0..WIDTH);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_and_is_centred() {
        let faces = glyph_faces();
        let used = |x: usize| (0..HEIGHT).any(|y| faces[y * WIDTH + x]);
        let first = (0..WIDTH).find(|&x| used(x)).unwrap();
        let last = (0..WIDTH).rev().find(|&x| used(x)).unwrap();
        assert!(first >= 8 && WIDTH - 1 - last >= 8);
        assert!(first.abs_diff(WIDTH - 1 - last) <= 2);
    }

    #[test]
    fn only_vanilla_is_rebuilt() {
        let source = vec![0; WIDTH * HEIGHT * 4];
        assert!(rust_edition(512, 64, &source).is_none());
    }
}
