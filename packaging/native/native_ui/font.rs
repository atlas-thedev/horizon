//! `Fonts`, `FontFace`, `Font` and `Glyph`: the faces the UI uses (Poppins
//! like the launcher, the original Native module icons, Lucide navigation)
//! plus system fallbacks for other scripts, rasterised into the atlas at the
//! exact pixel size they are drawn at. The Java mod uses stb_truetype; this
//! reads the same TrueType metrics (em scaling, ascent, descent, line gap)
//! with `ab_glyph`, which the game already uses for its own TTF fonts.

use super::atlas::Atlas;
use ab_glyph::{Font as _, FontArc, PxScale, point};
use std::collections::HashMap;

pub const REGULAR: usize = 0;
pub const MEDIUM: usize = 1;
pub const SEMIBOLD: usize = 2;
pub const BOLD: usize = 3;
pub const ICONS: usize = 4;
pub const MODULE_ICONS: usize = 5;

/// A rasterised glyph in the atlas, in physical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Glyph {
    pub ax: u32,
    pub ay: u32,
    pub w: u32,
    pub h: u32,
    pub xoff: i32,
    pub yoff: i32,
    pub advance: f32,
}

/// One face at one pixel size: its metrics and the glyphs baked so far.
struct Sized {
    face: usize,
    px: u32,
    pub ascent: f32,
    pub descent: f32,
    pub line_height: f32,
    glyphs: HashMap<char, Glyph>,
    advances: HashMap<char, f32>,
    generation: Option<u32>,
}

/// The light gamma lift that keeps small light-on-dark text from looking thin.
fn gamma() -> &'static [u8; 256] {
    static GAMMA: std::sync::OnceLock<[u8; 256]> = std::sync::OnceLock::new();
    GAMMA.get_or_init(|| {
        let mut out = [0u8; 256];
        for (i, v) in out.iter_mut().enumerate() {
            *v = ((i as f32 / 255.0).powf(0.8) * 255.0).round().min(255.0) as u8;
        }
        out
    })
}

/// Scale that maps the em square to `px` pixels (CSS font-size semantics).
fn em_scale(font: &FontArc, px: f32) -> f32 {
    px / font.units_per_em().unwrap_or(1000.0)
}

/// `ab_glyph`'s `PxScale` is the font's height (ascent - descent): the one that gives `px` per em.
fn px_scale(font: &FontArc, px: f32) -> PxScale {
    let height = font.ascent_unscaled() - font.descent_unscaled();
    PxScale::from(px * height / font.units_per_em().unwrap_or(1000.0))
}

pub struct Fonts {
    faces: Vec<FontArc>,
    sized: HashMap<(usize, u32), Sized>,
    fallbacks: Option<Vec<FontArc>>,
}

impl Fonts {
    /// The six faces in the mod's order: four Poppins weights, Lucide, the module icons.
    pub fn new(files: [&'static [u8]; 6]) -> Self {
        let mut faces: Vec<Option<FontArc>> = files.iter().map(|b| FontArc::try_from_slice(b).ok()).collect();
        for i in 0..4 {
            if faces[i].is_none() {
                faces[i] = faces[0].clone();
            }
        }
        let regular = faces[0].clone().expect("the Native UI's regular face is missing");
        Fonts {
            faces: faces.into_iter().map(|f| f.unwrap_or_else(|| regular.clone())).collect(),
            sized: HashMap::new(),
            fallbacks: None,
        }
    }

    fn sized(&mut self, face: usize, px: u32) -> &mut Sized {
        let px = px.clamp(4, 200);
        let face = face.min(self.faces.len() - 1);
        let font = &self.faces[face];
        self.sized.entry((face, px)).or_insert_with(|| {
            let s = em_scale(font, px as f32);
            Sized {
                face,
                px,
                ascent: font.ascent_unscaled() * s,
                descent: -font.descent_unscaled() * s,
                line_height: (font.ascent_unscaled() - font.descent_unscaled() + font.line_gap_unscaled()) * s,
                glyphs: HashMap::new(),
                advances: HashMap::new(),
                generation: None,
            }
        })
    }

    /// Ascent, descent and line height in physical pixels.
    pub fn metrics(&mut self, face: usize, px: u32) -> (f32, f32, f32) {
        let s = self.sized(face, px);
        (s.ascent, s.descent, s.line_height)
    }

    /// The face that has `c`: this one, else the first system fallback that does.
    fn face_for(&mut self, face: usize, c: char) -> Option<FontArc> {
        let own = &self.faces[face.min(self.faces.len() - 1)];
        if own.glyph_id(c).0 != 0 {
            return Some(own.clone());
        }
        self.fallbacks
            .get_or_insert_with(load_fallbacks)
            .iter()
            .find(|f| f.glyph_id(c).0 != 0)
            .cloned()
    }

    pub fn advance(&mut self, face: usize, px: u32, c: char) -> f32 {
        let key = (self.sized(face, px).face, self.sized(face, px).px);
        if let Some(a) = self.sized.get(&key).and_then(|s| s.advances.get(&c)) {
            return *a;
        }
        let (face, px) = key;
        let adv = match self.face_for(face, c) {
            Some(f) => f.h_advance_unscaled(f.glyph_id(c)) * em_scale(&f, px as f32),
            None => px as f32 * 0.5,
        };
        if let Some(s) = self.sized.get_mut(&key) {
            s.advances.insert(c, adv);
        }
        adv
    }

    /// Width of `text` in physical pixels.
    pub fn width(&mut self, face: usize, px: u32, text: &str) -> f32 {
        text.chars().map(|c| self.advance(face, px, c)).sum()
    }

    /// The glyph of `c`, baked into the atlas on first use; an empty glyph
    /// (advance only) while the atlas is full, drawn after the next reset.
    pub fn glyph(&mut self, atlas: &mut Atlas, face: usize, px: u32, c: char) -> Glyph {
        let key = {
            let s = self.sized(face, px);
            if s.generation != Some(atlas.generation) {
                s.glyphs.clear();
                s.generation = Some(atlas.generation);
            }
            if let Some(g) = s.glyphs.get(&c) {
                return *g;
            }
            (s.face, s.px)
        };
        let advance = self.advance(key.0, key.1, c);
        let baked = self.rasterise(atlas, key.0, key.1, c, advance);
        match baked {
            Some(g) => {
                if let Some(s) = self.sized.get_mut(&key) {
                    s.glyphs.insert(c, g);
                }
                g
            }
            None => Glyph {
                advance,
                ..Glyph::default()
            },
        }
    }

    fn rasterise(&mut self, atlas: &mut Atlas, face: usize, px: u32, c: char, advance: f32) -> Option<Glyph> {
        let empty = Glyph {
            advance,
            ..Glyph::default()
        };
        let Some(font) = self.face_for(face, c) else {
            return Some(empty);
        };
        if c == ' ' || c == '\t' {
            return Some(empty);
        }
        let glyph = font
            .glyph_id(c)
            .with_scale_and_position(px_scale(&font, px as f32), point(0.0, 0.0));
        let Some(outline) = font.outline_glyph(glyph) else {
            return Some(empty);
        };
        let bounds = outline.px_bounds();
        let (x0, y0) = (bounds.min.x.floor() as i32, bounds.min.y.floor() as i32);
        let w = (bounds.max.x.ceil() as i32 - x0).max(0) as u32;
        let h = (bounds.max.y.ceil() as i32 - y0).max(0) as u32;
        if w == 0 || h == 0 {
            return Some(empty);
        }
        // Atlas full: drawn next frame after the reset.
        let [ax, ay] = atlas.alloc(w, h)?;
        let mut alpha = vec![0u8; (w * h) as usize];
        let lift = gamma();
        outline.draw(|x, y, coverage| {
            if x < w && y < h {
                let a = (coverage.clamp(0.0, 1.0) * 255.0).round() as usize;
                alpha[(y * w + x) as usize] = lift[a];
            }
        });
        atlas.put_alpha(ax, ay, w, h, &alpha, w as usize);
        Some(Glyph {
            ax,
            ay,
            w,
            h,
            xoff: x0,
            yoff: y0,
            advance,
        })
    }
}

/// System faces for scripts Poppins lacks, in the mod's order.
fn load_fallbacks() -> Vec<FontArc> {
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    let win = format!("{windir}\\Fonts\\");
    let candidates = [
        format!("{win}segoeui.ttf"),
        format!("{win}Nirmala.ttf"),
        format!("{win}arial.ttf"),
        format!("{win}msyh.ttc"),
        format!("{win}malgun.ttf"),
        format!("{win}seguisym.ttf"),
        format!("{win}seguiemj.ttf"),
        "/System/Library/Fonts/Supplemental/Arial Unicode.ttf".into(),
        "/Library/Fonts/Arial Unicode.ttf".into(),
        "/System/Library/Fonts/Helvetica.ttc".into(),
        "/System/Library/Fonts/Supplemental/Sinhala Sangam MN.ttc".into(),
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into(),
        "/usr/share/fonts/dejavu/DejaVuSans.ttf".into(),
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf".into(),
        "/usr/share/fonts/noto/NotoSans-Regular.ttf".into(),
        "/usr/share/fonts/truetype/noto/NotoSansSinhala-Regular.ttf".into(),
        "/usr/share/fonts/liberation/LiberationSans-Regular.ttf".into(),
    ];
    candidates
        .iter()
        .filter_map(|path| {
            let meta = std::fs::metadata(path).ok()?;
            if !meta.is_file() || meta.len() > 40 * 1024 * 1024 {
                return None;
            }
            let bytes = std::fs::read(path).ok()?;
            ab_glyph::FontVec::try_from_vec_and_index(bytes, 0).ok().map(FontArc::new)
        })
        .collect()
}
