//! `mc_font`: crisp built-in 8x8 Mojangles-proportioned bitmap font (printable ASCII 32..=126 + common MOTD
//! symbols `»`, `«`, `·`, `•`, `–`, `—`, `°`, `★`, `✦`, `│`, `┃`, `▪`, `◆`, `♦`, `▲`, `▼`), authentic vanilla
//! advance widths (space = 4 px, glyph width + 1 px), authentic vanilla bold (1 px double-strike + 1 px extra
//! advance), quarter-brightness drop shadow, and fallback to `Canvas::text` for rare Unicode glyphs so nothing
//! ever renders as `?`.
#![allow(dead_code)]

use horizon_client::native_ui::{Canvas, Image, font, theme};
use std::sync::{Arc, OnceLock};

struct BuiltinFont {
    sheet: Arc<Image>,
    adv: [u8; 256],
}

fn builtin() -> &'static BuiltinFont {
    static FONT: OnceLock<BuiltinFont> = OnceLock::new();
    FONT.get_or_init(build_builtin)
}

/// Maps extra MOTD / HUD symbols to custom slots in the built-in 16x16 sheet (indices 128..140).
fn extra_slot(ch: char) -> Option<usize> {
    match ch {
        '\u{00BB}' => Some(128),              // »
        '\u{00AB}' => Some(129),              // «
        '\u{00B7}' => Some(130),              // ·
        '\u{2022}' => Some(131),              // •
        '\u{2013}' | '\u{2014}' => Some(132), // – —
        '\u{00B0}' => Some(133),              // °
        '\u{2605}' | '\u{2726}' => Some(134), // ★ ✦
        '\u{2502}' | '\u{2503}' => Some(135), // │ ┃
        '\u{25AA}' | '\u{25A0}' => Some(136), // ▪ ■
        '\u{25C6}' | '\u{2666}' => Some(137), // ◆ ♦
        '\u{25B2}' => Some(138),              // ▲
        '\u{25BC}' => Some(139),              // ▼
        _ => None,
    }
}

fn is_ascii_printable(ch: char) -> bool {
    let code = ch as u32;
    (32..127).contains(&code)
}

/// Logical px per Minecraft font pixel snapped to crisp screen pixels for a target logical height (e.g. 10.5 px).
pub fn snap_px(c: &Canvas, target_height: f32) -> f32 {
    let k = ((target_height / 8.0) * c.scale).round().max(1.0);
    k / c.scale
}

/// Height of one line: 8 font pixels (vanilla draws text 8 px tall on a 9 px line).
pub fn height(px: f32) -> f32 {
    8.0 * px
}

/// Vanilla drop-shadow colour: quarter RGB brightness, preserving alpha.
pub fn shadow_color(argb: u32) -> u32 {
    let a = argb & 0xFF00_0000;
    let r = ((argb >> 16) & 0xFF) / 4;
    let g = ((argb >> 8) & 0xFF) / 4;
    let b = (argb & 0xFF) / 4;
    a | (r << 16) | (g << 8) | b
}

/// Width of `t` in logical px when one font pixel is `px` logical px, with bold & Unicode fallback support.
pub fn width(c: &mut Canvas, t: &str, px: f32, bold: bool) -> f32 {
    if t.is_empty() {
        return 0.0;
    }
    let f = builtin();
    let mut w = 0.0;
    let fb_size = 8.5 * px;
    let fb_face = if bold { font::BOLD } else { font::MEDIUM };
    let mut buf = [0u8; 4];
    for ch in t.chars() {
        if is_ascii_printable(ch) {
            let a = f32::from(f.adv[ch as usize]);
            let extra = if bold && ch != ' ' { 1.0 } else { 0.0 };
            w += (a + extra) * px;
        } else if let Some(slot) = extra_slot(ch) {
            let a = f32::from(f.adv[slot]);
            w += (a + if bold { 1.0 } else { 0.0 }) * px;
        } else {
            w += c.text_width(fb_face, fb_size, ch.encode_utf8(&mut buf));
        }
    }
    w
}

/// Width without a canvas (for pure ASCII/symbol measurements and unit tests).
pub fn width_plain(t: &str, px: f32, bold: bool) -> f32 {
    let f = builtin();
    let mut w = 0.0;
    for ch in t.chars() {
        if is_ascii_printable(ch) {
            let a = f32::from(f.adv[ch as usize]);
            w += (a + if bold && ch != ' ' { 1.0 } else { 0.0 }) * px;
        } else if let Some(slot) = extra_slot(ch) {
            w += (f32::from(f.adv[slot]) + if bold { 1.0 } else { 0.0 }) * px;
        } else {
            w += (6.0 + if bold { 1.0 } else { 0.0 }) * px;
        }
    }
    w
}

/// Draws `t` with its top-left at `(x, y)` with optional vanilla drop shadow (quarter brightness, 1 px down-right)
/// and optional vanilla bold (1 px double-strike + 1 px extra advance). Returns the x coordinate after the text.
#[allow(clippy::too_many_arguments)]
pub fn draw(c: &mut Canvas, t: &str, x: f32, y: f32, px: f32, argb: u32, shadow: bool, bold: bool) -> f32 {
    if t.is_empty() {
        return x;
    }
    if shadow {
        draw_run(c, t, x + px, y + px, px, shadow_color(argb), bold);
    }
    draw_run(c, t, x, y, px, argb, bold)
}

#[allow(clippy::too_many_arguments)]
fn draw_run(c: &mut Canvas, t: &str, mut x: f32, y: f32, px: f32, argb: u32, bold: bool) -> f32 {
    let f = builtin();
    let u = 1.0 / 16.0;
    let cell = 8.0 * px;
    let fb_size = 8.5 * px;
    let fb_face = if bold { font::BOLD } else { font::MEDIUM };
    let mut buf = [0u8; 4];
    for ch in t.chars() {
        if is_ascii_printable(ch) {
            let code = ch as usize;
            if ch != ' ' {
                let u0 = (code % 16) as f32 * u;
                let v0 = (code / 16) as f32 * u;
                c.image(&f.sheet, x, y, cell, cell, [u0, v0, u0 + u, v0 + u], argb);
                if bold {
                    c.image(&f.sheet, x + px, y, cell, cell, [u0, v0, u0 + u, v0 + u], argb);
                }
            }
            x += (f32::from(f.adv[code]) + if bold && ch != ' ' { 1.0 } else { 0.0 }) * px;
        } else if let Some(slot) = extra_slot(ch) {
            let u0 = (slot % 16) as f32 * u;
            let v0 = (slot / 16) as f32 * u;
            c.image(&f.sheet, x, y, cell, cell, [u0, v0, u0 + u, v0 + u], argb);
            if bold {
                c.image(&f.sheet, x + px, y, cell, cell, [u0, v0, u0 + u, v0 + u], argb);
            }
            x += (f32::from(f.adv[slot]) + if bold { 1.0 } else { 0.0 }) * px;
        } else {
            let s = ch.encode_utf8(&mut buf);
            let lh = c.line_height(fb_face, fb_size);
            let fy = y + (cell - lh) * 0.5;
            c.text(fb_face, fb_size, s, x, fy, argb);
            x += c.text_width(fb_face, fb_size, s);
        }
    }
    x
}

// ── built-in Mojangles-proportioned 8x8 bitmap sheet (128x128, 16x16 cells) ──

fn put(px: &mut [u32], ch: usize, rows: &[u8]) {
    let gx = (ch % 16) * 8;
    let gy = (ch / 16) * 8;
    for (r, &bits) in rows.iter().take(8).enumerate() {
        for col in 0..8 {
            if (bits & (0x80 >> col)) != 0 {
                px[(gy + r) * 128 + gx + col] = theme::WHITE;
            }
        }
    }
}

fn build_builtin() -> BuiltinFont {
    let mut px = vec![0u32; 128 * 128];

    // Printable ASCII 33..=126 (top-left aligned in 8x8 cells, Mojangles proportions).
    put(&mut px, b'!' as usize, &[0x80, 0x80, 0x80, 0x80, 0x80, 0x00, 0x80]);
    put(&mut px, b'"' as usize, &[0xA0, 0xA0, 0xA0, 0x00, 0x00, 0x00, 0x00]);
    put(&mut px, b'#' as usize, &[0x50, 0xF8, 0x50, 0x50, 0xF8, 0x50, 0x00]);
    put(&mut px, b'$' as usize, &[0x20, 0x78, 0xA0, 0x70, 0x28, 0xF0, 0x20]);
    put(&mut px, b'%' as usize, &[0xC8, 0xC8, 0x10, 0x20, 0x4C, 0x4C, 0x00]);
    put(&mut px, b'&' as usize, &[0x60, 0x90, 0xA0, 0x40, 0xA8, 0x90, 0x68]);
    put(&mut px, b'\'' as usize, &[0x80, 0x80, 0x80, 0x00, 0x00, 0x00, 0x00]);
    put(&mut px, b'(' as usize, &[0x20, 0x40, 0x80, 0x80, 0x80, 0x40, 0x20]);
    put(&mut px, b')' as usize, &[0x80, 0x40, 0x20, 0x20, 0x20, 0x40, 0x80]);
    put(&mut px, b'*' as usize, &[0x00, 0xA8, 0x70, 0xF8, 0x70, 0xA8, 0x00]);
    put(&mut px, b'+' as usize, &[0x00, 0x20, 0x20, 0xF8, 0x20, 0x20, 0x00]);
    put(&mut px, b',' as usize, &[0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x80]);
    put(&mut px, b'-' as usize, &[0x00, 0x00, 0x00, 0xF8, 0x00, 0x00, 0x00]);
    put(&mut px, b'.' as usize, &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80]);
    put(&mut px, b'/' as usize, &[0x08, 0x10, 0x10, 0x20, 0x40, 0x40, 0x80]);
    put(&mut px, b'0' as usize, &[0x70, 0x88, 0x98, 0xA8, 0xC8, 0x88, 0x70]);
    put(&mut px, b'1' as usize, &[0x20, 0x60, 0x20, 0x20, 0x20, 0x20, 0x70]);
    put(&mut px, b'2' as usize, &[0x70, 0x88, 0x08, 0x30, 0x40, 0x80, 0xF8]);
    put(&mut px, b'3' as usize, &[0x70, 0x88, 0x08, 0x30, 0x08, 0x88, 0x70]);
    put(&mut px, b'4' as usize, &[0x10, 0x30, 0x50, 0x90, 0xF8, 0x10, 0x10]);
    put(&mut px, b'5' as usize, &[0xF8, 0x80, 0xF0, 0x08, 0x08, 0x88, 0x70]);
    put(&mut px, b'6' as usize, &[0x30, 0x40, 0x80, 0xF0, 0x88, 0x88, 0x70]);
    put(&mut px, b'7' as usize, &[0xF8, 0x08, 0x10, 0x20, 0x40, 0x40, 0x40]);
    put(&mut px, b'8' as usize, &[0x70, 0x88, 0x88, 0x70, 0x88, 0x88, 0x70]);
    put(&mut px, b'9' as usize, &[0x70, 0x88, 0x88, 0x78, 0x08, 0x10, 0x60]);
    put(&mut px, b':' as usize, &[0x00, 0x00, 0x80, 0x00, 0x00, 0x80, 0x00]);
    put(&mut px, b';' as usize, &[0x00, 0x00, 0x40, 0x00, 0x00, 0x40, 0x80]);
    put(&mut px, b'<' as usize, &[0x10, 0x20, 0x40, 0x80, 0x40, 0x20, 0x10]);
    put(&mut px, b'=' as usize, &[0x00, 0x00, 0xF8, 0x00, 0xF8, 0x00, 0x00]);
    put(&mut px, b'>' as usize, &[0x80, 0x40, 0x20, 0x10, 0x20, 0x40, 0x80]);
    put(&mut px, b'?' as usize, &[0x70, 0x88, 0x08, 0x10, 0x20, 0x00, 0x20]);
    put(&mut px, b'@' as usize, &[0x70, 0x88, 0xB8, 0xA8, 0xB8, 0x80, 0x78]);
    put(&mut px, b'A' as usize, &[0x70, 0x88, 0x88, 0xF8, 0x88, 0x88, 0x88]);
    put(&mut px, b'B' as usize, &[0xF0, 0x88, 0x88, 0xF0, 0x88, 0x88, 0xF0]);
    put(&mut px, b'C' as usize, &[0x70, 0x88, 0x80, 0x80, 0x80, 0x88, 0x70]);
    put(&mut px, b'D' as usize, &[0xF0, 0x88, 0x88, 0x88, 0x88, 0x88, 0xF0]);
    put(&mut px, b'E' as usize, &[0xF8, 0x80, 0x80, 0xF0, 0x80, 0x80, 0xF8]);
    put(&mut px, b'F' as usize, &[0xF8, 0x80, 0x80, 0xF0, 0x80, 0x80, 0x80]);
    put(&mut px, b'G' as usize, &[0x70, 0x88, 0x80, 0xB8, 0x88, 0x88, 0x78]);
    put(&mut px, b'H' as usize, &[0x88, 0x88, 0x88, 0xF8, 0x88, 0x88, 0x88]);
    put(&mut px, b'I' as usize, &[0xE0, 0x40, 0x40, 0x40, 0x40, 0x40, 0xE0]);
    put(&mut px, b'J' as usize, &[0x38, 0x10, 0x10, 0x10, 0x10, 0x90, 0x60]);
    put(&mut px, b'K' as usize, &[0x88, 0x90, 0xA0, 0xC0, 0xA0, 0x90, 0x88]);
    put(&mut px, b'L' as usize, &[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0xF8]);
    put(&mut px, b'M' as usize, &[0x88, 0xD8, 0xA8, 0xA8, 0x88, 0x88, 0x88]);
    put(&mut px, b'N' as usize, &[0x88, 0xC8, 0xA8, 0x98, 0x88, 0x88, 0x88]);
    put(&mut px, b'O' as usize, &[0x70, 0x88, 0x88, 0x88, 0x88, 0x88, 0x70]);
    put(&mut px, b'P' as usize, &[0xF0, 0x88, 0x88, 0xF0, 0x80, 0x80, 0x80]);
    put(&mut px, b'Q' as usize, &[0x70, 0x88, 0x88, 0x88, 0xA8, 0x90, 0x68]);
    put(&mut px, b'R' as usize, &[0xF0, 0x88, 0x88, 0xF0, 0xA0, 0x90, 0x88]);
    put(&mut px, b'S' as usize, &[0x78, 0x80, 0x80, 0x70, 0x08, 0x08, 0xF0]);
    put(&mut px, b'T' as usize, &[0xF8, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20]);
    put(&mut px, b'U' as usize, &[0x88, 0x88, 0x88, 0x88, 0x88, 0x88, 0x70]);
    put(&mut px, b'V' as usize, &[0x88, 0x88, 0x88, 0x88, 0x50, 0x50, 0x20]);
    put(&mut px, b'W' as usize, &[0x88, 0x88, 0x88, 0xA8, 0xA8, 0xD8, 0x88]);
    put(&mut px, b'X' as usize, &[0x88, 0x88, 0x50, 0x20, 0x50, 0x88, 0x88]);
    put(&mut px, b'Y' as usize, &[0x88, 0x88, 0x50, 0x20, 0x20, 0x20, 0x20]);
    put(&mut px, b'Z' as usize, &[0xF8, 0x08, 0x10, 0x20, 0x40, 0x80, 0xF8]);
    put(&mut px, b'[' as usize, &[0xE0, 0x80, 0x80, 0x80, 0x80, 0x80, 0xE0]);
    put(&mut px, b'\\' as usize, &[0x80, 0x40, 0x40, 0x20, 0x10, 0x10, 0x08]);
    put(&mut px, b']' as usize, &[0xE0, 0x20, 0x20, 0x20, 0x20, 0x20, 0xE0]);
    put(&mut px, b'^' as usize, &[0x20, 0x50, 0x88, 0x00, 0x00, 0x00, 0x00]);
    put(&mut px, b'_' as usize, &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xF8]);
    put(&mut px, b'`' as usize, &[0x80, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00]);
    put(&mut px, b'a' as usize, &[0x00, 0x00, 0x70, 0x08, 0x78, 0x88, 0x78]);
    put(&mut px, b'b' as usize, &[0x80, 0x80, 0xF0, 0x88, 0x88, 0x88, 0xF0]);
    put(&mut px, b'c' as usize, &[0x00, 0x00, 0x78, 0x80, 0x80, 0x80, 0x78]);
    put(&mut px, b'd' as usize, &[0x08, 0x08, 0x78, 0x88, 0x88, 0x88, 0x78]);
    put(&mut px, b'e' as usize, &[0x00, 0x00, 0x70, 0x88, 0xF8, 0x80, 0x78]);
    put(&mut px, b'f' as usize, &[0x30, 0x40, 0xE0, 0x40, 0x40, 0x40, 0x40]);
    put(&mut px, b'g' as usize, &[0x00, 0x00, 0x78, 0x88, 0x88, 0x78, 0x08, 0x70]);
    put(&mut px, b'h' as usize, &[0x80, 0x80, 0xF0, 0x88, 0x88, 0x88, 0x88]);
    put(&mut px, b'i' as usize, &[0x80, 0x00, 0x80, 0x80, 0x80, 0x80, 0x80]);
    put(&mut px, b'j' as usize, &[0x10, 0x00, 0x10, 0x10, 0x10, 0x90, 0x60]);
    put(&mut px, b'k' as usize, &[0x80, 0x80, 0x90, 0xA0, 0xC0, 0xA0, 0x90]);
    put(&mut px, b'l' as usize, &[0xC0, 0x40, 0x40, 0x40, 0x40, 0x40, 0xE0]);
    put(&mut px, b'm' as usize, &[0x00, 0x00, 0xD0, 0xA8, 0xA8, 0xA8, 0xA8]);
    put(&mut px, b'n' as usize, &[0x00, 0x00, 0xF0, 0x88, 0x88, 0x88, 0x88]);
    put(&mut px, b'o' as usize, &[0x00, 0x00, 0x70, 0x88, 0x88, 0x88, 0x70]);
    put(&mut px, b'p' as usize, &[0x00, 0x00, 0xF0, 0x88, 0x88, 0xF0, 0x80, 0x80]);
    put(&mut px, b'q' as usize, &[0x00, 0x00, 0x78, 0x88, 0x88, 0x78, 0x08, 0x08]);
    put(&mut px, b'r' as usize, &[0x00, 0x00, 0xB0, 0xC8, 0x80, 0x80, 0x80]);
    put(&mut px, b's' as usize, &[0x00, 0x00, 0x78, 0x80, 0x70, 0x08, 0xF0]);
    put(&mut px, b't' as usize, &[0x40, 0x40, 0xE0, 0x40, 0x40, 0x40, 0x30]);
    put(&mut px, b'u' as usize, &[0x00, 0x00, 0x88, 0x88, 0x88, 0x88, 0x78]);
    put(&mut px, b'v' as usize, &[0x00, 0x00, 0x88, 0x88, 0x88, 0x50, 0x20]);
    put(&mut px, b'w' as usize, &[0x00, 0x00, 0x88, 0x88, 0xA8, 0xA8, 0x50]);
    put(&mut px, b'x' as usize, &[0x00, 0x00, 0x88, 0x50, 0x20, 0x50, 0x88]);
    put(&mut px, b'y' as usize, &[0x00, 0x00, 0x88, 0x88, 0x88, 0x78, 0x08, 0x70]);
    put(&mut px, b'z' as usize, &[0x00, 0x00, 0xF8, 0x10, 0x20, 0x40, 0xF8]);
    put(&mut px, b'{' as usize, &[0x30, 0x40, 0x40, 0x80, 0x40, 0x40, 0x30]);
    put(&mut px, b'|' as usize, &[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80]);
    put(&mut px, b'}' as usize, &[0xC0, 0x20, 0x20, 0x10, 0x20, 0x20, 0xC0]);
    put(&mut px, b'~' as usize, &[0x00, 0x50, 0xA8, 0x00, 0x00, 0x00, 0x00]);

    // Extra MOTD & HUD symbols (slots 128..=139).
    put(&mut px, 128, &[0x00, 0x90, 0x48, 0x24, 0x48, 0x90, 0x00]); // »
    put(&mut px, 129, &[0x00, 0x24, 0x48, 0x90, 0x48, 0x24, 0x00]); // «
    put(&mut px, 130, &[0x00, 0x00, 0x00, 0x80, 0x00, 0x00, 0x00]); // ·
    put(&mut px, 131, &[0x00, 0x00, 0xC0, 0xC0, 0x00, 0x00, 0x00]); // •
    put(&mut px, 132, &[0x00, 0x00, 0x00, 0xFC, 0x00, 0x00, 0x00]); // – —
    put(&mut px, 133, &[0x60, 0x90, 0x60, 0x00, 0x00, 0x00, 0x00]); // °
    put(&mut px, 134, &[0x20, 0x20, 0xF8, 0x70, 0x50, 0x88, 0x00]); // ★ ✦
    put(&mut px, 135, &[0xC0, 0xC0, 0xC0, 0xC0, 0xC0, 0xC0, 0xC0]); // │ ┃
    put(&mut px, 136, &[0x00, 0x00, 0xE0, 0xE0, 0xE0, 0x00, 0x00]); // ▪ ■
    put(&mut px, 137, &[0x20, 0x70, 0xF8, 0x70, 0x20, 0x00, 0x00]); // ◆ ♦
    put(&mut px, 138, &[0x00, 0x20, 0x70, 0xF8, 0xF8, 0x00, 0x00]); // ▲
    put(&mut px, 139, &[0x00, 0xF8, 0xF8, 0x70, 0x20, 0x00, 0x00]); // ▼

    // Measure authentic vanilla advances: glyph width + 1 px gap; space = 4 px.
    let mut adv = [4u8; 256];
    for ch in 0..256 {
        if ch == b' ' as usize {
            adv[ch] = 4;
            continue;
        }
        let gx = (ch % 16) * 8;
        let gy = (ch / 16) * 8;
        let mut right = -1i32;
        for x in (0..8).rev() {
            let mut hit = false;
            for y in 0..8 {
                if (px[(gy + y) * 128 + gx + x] >> 24) > 16 {
                    hit = true;
                    break;
                }
            }
            if hit {
                right = x as i32;
                break;
            }
        }
        adv[ch] = if right < 0 { 4 } else { (right + 2).clamp(2, 9) as u8 };
    }

    let mut sheet = Image::new(128, 128, px);
    if let Some(img) = Arc::get_mut(&mut sheet) {
        img.linear = false; // crisp nearest-neighbor pixels
    }
    BuiltinFont { sheet, adv }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vanilla_advances_and_bold_match_mc_font() {
        let f = builtin();
        assert!(!f.sheet.linear);
        assert_eq!(f.adv[b' ' as usize], 4);
        assert_eq!(f.adv[b'!' as usize], 2);
        assert_eq!(f.adv[b'A' as usize], 6);
        assert_eq!(width_plain("A B", 1.0, false), 6.0 + 4.0 + 6.0);
        // Bold adds +1 px to non-space characters only.
        assert_eq!(width_plain("A B", 1.0, true), 7.0 + 4.0 + 7.0);
        assert_eq!(shadow_color(0xFF_FF_80_40), 0xFF_3F_20_10);
        assert_eq!(extra_slot('»'), Some(128));
        assert_eq!(extra_slot('◆'), Some(137));
    }
}
