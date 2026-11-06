//! The Native logo in front of the name of every player who plays from
//! Native, on name tags and in the tab list: the port of native-mod's
//! `NameTags`. The logo is a glyph, as in the Java mod: U+10FFFD (Native) or
//! U+10FFFC (Native+), added to the default and uniform fonts from the mod's
//! own bitmaps (`native_logo.png`, `native_logo_plus.png`, embedded here), so
//! it draws like any other character.
//!
//! This crate is below the one that holds the skin directory
//! (`horizon_client::native_skins`), so the directory registers its lookup
//! with [`set_lookup`] when it starts. Until then no name is marked.

use crate::text::Text;
use std::sync::OnceLock;

/// U+10FFFD: the last code point of the last private-use plane. Server
/// resource packs put rank icons at U+E000 and up, which this never meets.
pub const GLYPH: char = '\u{10FFFD}';
/// U+10FFFC: the Native+ mark (the N with a gold plus).
pub const GLYPH_PLUS: char = '\u{10FFFC}';

const LOGO: &[u8] = include_bytes!("native_logo.png");
const LOGO_PLUS: &[u8] = include_bytes!("native_logo_plus.png");

/// Whether a player plays from Native: `None` when not, `Some(plus)` when
/// they do (`plus`: a Native+ member).
pub type Lookup = fn(&str, Option<u128>) -> Option<bool>;

static LOOKUP: OnceLock<Lookup> = OnceLock::new();

/// Called once by the skin directory.
pub fn set_lookup(lookup: Lookup) {
    let _ = LOOKUP.set(lookup);
}

/// The mark for a player, if they play from Native.
pub fn mark(name: &str, id: Option<u128>) -> Option<char> {
    let lookup = LOOKUP.get()?;
    lookup(name, id).map(|plus| if plus { GLYPH_PLUS } else { GLYPH })
}

/// `NameTags.decorate`: the logo and a space, then the name as shown.
pub fn marked(shown: Text, name: &str, id: Option<u128>) -> Text {
    match mark(name, id) {
        Some(glyph) => Text::literal(format!("{glyph} ")).append(shown),
        None => shown,
    }
}

/// The mod's `assets/minecraft/font/default.json` (and `uniform.json`)
/// providers, put first in those fonts: they only name these two glyphs.
pub fn font_providers() -> Vec<serde_json::Value> {
    [("native:font/logo.png", GLYPH), ("native:font/logo_plus.png", GLYPH_PLUS)]
        .into_iter()
        .map(|(file, glyph)| {
            serde_json::json!({
                "type": "bitmap",
                "file": file,
                "ascent": 7,
                "height": 8,
                "chars": [glyph.to_string()],
            })
        })
        .collect()
}

/// The pixels of a sheet named by [`font_providers`], or `None` for any other file.
pub fn sheet(file: &str) -> Option<crate::Result<(u32, u32, Vec<u8>)>> {
    match file {
        "native:font/logo.png" => Some(crate::hud::decode_bytes(LOGO)),
        "native:font/logo_plus.png" => Some(crate::hud::decode_bytes(LOGO_PLUS)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_logos_decode_as_square_sheets() {
        for file in ["native:font/logo.png", "native:font/logo_plus.png"] {
            let (w, h, rgba) = sheet(file).unwrap().unwrap();
            assert_eq!(w, h);
            assert_eq!(rgba.len(), (w * h * 4) as usize);
        }
        assert!(sheet("minecraft:font/ascii.png").is_none());
    }

    #[test]
    fn providers_name_one_glyph_each() {
        let providers = font_providers();
        assert_eq!(providers.len(), 2);
        assert_eq!(providers[0]["chars"][0], GLYPH.to_string());
        assert_eq!(providers[1]["chars"][0], GLYPH_PLUS.to_string());
    }
}
