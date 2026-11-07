//! A small GIF decoder (87a / 89a, animated): each frame comes out already
//! composed on the logical screen, as ARGB, with its delay. Chat pictures and
//! the GIF picker use it.
#![allow(dead_code)]

/// Frames a chat picture may have, and the pixels all of them together may take.
const MAX_FRAMES: usize = 150;
const MAX_PIXELS: usize = 48 * 1024 * 1024;

pub struct Gif {
    pub width: u32,
    pub height: u32,
    /// (ARGB pixels of the whole picture, how long it shows in ms)
    pub frames: Vec<(Vec<u32>, u32)>,
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Option<u8> {
        let b = *self.data.get(self.at)?;
        self.at += 1;
        Some(b)
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from(self.byte()?) | u16::from(self.byte()?) << 8)
    }

    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.data.get(self.at..self.at + n)?;
        self.at += n;
        Some(s)
    }

    /// The data sub-blocks up to the terminator, joined.
    fn blocks(&mut self) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        loop {
            let n = self.byte()? as usize;
            if n == 0 {
                return Some(out);
            }
            out.extend_from_slice(self.take(n)?);
        }
    }
}

fn palette(r: &mut Reader, size: usize) -> Option<Vec<u32>> {
    let raw = r.take(size * 3)?;
    Some(raw.chunks_exact(3).map(|c| 0xFF000000 | u32::from(c[0]) << 16 | u32::from(c[1]) << 8 | u32::from(c[2])).collect())
}

/// LZW as GIF has it: codes of growing width, least significant bit first.
fn lzw(min: u8, data: &[u8], pixels: usize) -> Vec<u8> {
    let min = min.clamp(1, 11) as u32;
    let (clear, end) = (1u32 << min, (1u32 << min) + 1);
    let mut prefix = vec![0u16; 4096];
    let mut suffix = vec![0u8; 4096];
    let mut length = vec![0u16; 4096];
    for i in 0..clear as usize {
        suffix[i] = i as u8;
        length[i] = 1;
    }
    let (mut size, mut next, mut prev) = (min + 1, end + 1, None::<u32>);
    let mut out = Vec::with_capacity(pixels);
    let (mut bits, mut have) = (0u32, 0u32);
    let mut input = data.iter();
    let mut scratch = Vec::new();
    'outer: loop {
        while have < size {
            let Some(b) = input.next() else { break 'outer };
            bits |= u32::from(*b) << have;
            have += 8;
        }
        let code = bits & ((1 << size) - 1);
        bits >>= size;
        have -= size;
        if code == clear {
            size = min + 1;
            next = end + 1;
            prev = None;
            continue;
        }
        if code == end {
            break;
        }
        // The string for this code (a code not in the table yet is the previous string plus its first byte).
        scratch.clear();
        let known = code < next;
        let start = if known { Some(code) } else { prev };
        let Some(mut at) = start else { break };
        if !known && prev.is_none() {
            break;
        }
        loop {
            scratch.push(suffix[at as usize]);
            if length[at as usize] <= 1 {
                break;
            }
            at = u32::from(prefix[at as usize]);
        }
        scratch.reverse();
        let first = scratch[0];
        if !known {
            scratch.push(first);
        }
        out.extend_from_slice(&scratch);
        if let Some(p) = prev
            && next < 4096
        {
            prefix[next as usize] = p as u16;
            suffix[next as usize] = first;
            length[next as usize] = length[p as usize] + 1;
            next += 1;
            if next == (1 << size) && size < 12 {
                size += 1;
            }
        }
        prev = Some(code);
        if out.len() >= pixels {
            break;
        }
    }
    out.truncate(pixels);
    out
}

pub fn decode(bytes: &[u8]) -> Option<Gif> {
    if !(bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) {
        return None;
    }
    let mut r = Reader { data: bytes, at: 6 };
    let (width, height) = (u32::from(r.u16()?), u32::from(r.u16()?));
    if width == 0 || height == 0 || (width * height) as usize > 4096 * 4096 {
        return None;
    }
    let packed = r.byte()?;
    let _background = r.byte()?;
    let _aspect = r.byte()?;
    let global = if packed & 0x80 != 0 { Some(palette(&mut r, 2usize << (packed & 7))?) } else { None };
    let (w, h) = (width as usize, height as usize);
    let mut canvas = vec![0u32; w * h];
    let mut frames: Vec<(Vec<u32>, u32)> = Vec::new();
    // The graphic control extension that applies to the next picture: delay, transparent index, disposal.
    let (mut delay, mut transparent, mut disposal) = (100u32, None::<u8>, 0u8);
    loop {
        match r.byte()? {
            0x21 => {
                let label = r.byte()?;
                let data = r.blocks()?;
                if label == 0xF9 && data.len() >= 4 {
                    disposal = (data[0] >> 2) & 7;
                    transparent = (data[0] & 1 != 0).then_some(data[3]);
                    let cs = u32::from(data[1]) | u32::from(data[2]) << 8;
                    // Browsers show a 0 or 1 cs delay as 10 cs.
                    delay = if cs <= 1 { 100 } else { cs * 10 };
                }
            }
            0x2C => {
                let (left, top) = (r.u16()? as usize, r.u16()? as usize);
                let (fw, fh) = (r.u16()? as usize, r.u16()? as usize);
                let flags = r.byte()?;
                let local = if flags & 0x80 != 0 { Some(palette(&mut r, 2usize << (flags & 7))?) } else { None };
                let min = r.byte()?;
                let data = r.blocks()?;
                let colors = local.as_ref().or(global.as_ref())?;
                let indices = lzw(min, &data, fw * fh);
                let saved = (disposal == 3).then(|| canvas.clone());
                // Interlaced frames store their rows in four passes.
                let rows: Vec<usize> = if flags & 0x40 != 0 {
                    let mut order = Vec::with_capacity(fh);
                    for (start, step) in [(0, 8), (4, 8), (2, 4), (1, 2)] {
                        order.extend((start..fh).step_by(step));
                    }
                    order
                } else {
                    (0..fh).collect()
                };
                for (i, row) in rows.iter().enumerate() {
                    let y = top + row;
                    if y >= h {
                        continue;
                    }
                    for x in 0..fw {
                        let px = left + x;
                        let Some(&index) = indices.get(i * fw + x) else { break };
                        if px >= w || Some(index) == transparent {
                            continue;
                        }
                        if let Some(c) = colors.get(index as usize) {
                            canvas[y * w + px] = *c;
                        }
                    }
                }
                frames.push((canvas.clone(), delay));
                if frames.len() >= MAX_FRAMES || frames.len() * w * h > MAX_PIXELS {
                    break;
                }
                match disposal {
                    2 => {
                        for row in top..(top + fh).min(h) {
                            let end = (left + fw).min(w);
                            if left < end {
                                canvas[row * w + left..row * w + end].fill(0);
                            }
                        }
                    }
                    3 => {
                        if let Some(s) = saved {
                            canvas = s;
                        }
                    }
                    _ => {}
                }
                (delay, transparent, disposal) = (100, None, 0);
            }
            _ => break, // The trailer, or something that is not a GIF.
        }
    }
    (!frames.is_empty()).then_some(Gif { width, height, frames })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2x2 GIF: red, green / blue, white, written by hand.
    const TINY: &[u8] = &[
        0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x02, 0x00, 0x02, 0x00, 0xF1, 0x00, 0x00, // header, screen, 4-colour table follows
        0xFF, 0x00, 0x00, 0x00, 0xFF, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, // red green blue white
        0x2C, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x02, 0x00, 0x00, // image descriptor
        0x02, 0x03, 0x44, 0x34, 0x05, 0x00, // LZW: clear, 0 1 2 3, end
        0x3B,
    ];

    #[test]
    fn it_decodes_a_hand_written_gif() {
        let gif = decode(TINY).expect("decodes");
        assert_eq!((gif.width, gif.height, gif.frames.len()), (2, 2, 1));
        assert_eq!(gif.frames[0].0, vec![0xFFFF0000, 0xFF00FF00, 0xFF0000FF, 0xFFFFFFFF]);
    }

    #[test]
    fn it_refuses_what_is_not_a_gif() {
        assert!(decode(b"PNG").is_none());
        assert!(decode(&TINY[..20]).is_none());
    }

    #[test]
    fn lzw_handles_the_end_and_clear_codes() {
        // min 2: clear = 4, end = 5. Codes 4, 0, 1, 5 in 3-bit codes: 100 000 001 101 -> bytes (LSB first).
        let bits = [4u32, 0, 1, 5];
        let (mut acc, mut n, mut out) = (0u32, 0, Vec::new());
        for c in bits {
            acc |= c << n;
            n += 3;
            while n >= 8 {
                out.push(acc as u8);
                acc >>= 8;
                n -= 8;
            }
        }
        if n > 0 {
            out.push(acc as u8);
        }
        assert_eq!(lzw(2, &out, 2), vec![0, 1]);
    }
}
