//! The Native Client UI drawing layer: the Rust port of native-mod's `ui/gfx`
//! (`Atlas`, `Shapes`, `Fonts`, `Image`, `Canvas`, the icons) and `Theme`.
//! Views draw on a [`Canvas`] in logical units; [`Canvas::finish`] hands the
//! frame to the HUD as runs drawn from the Native UI's own texture pages.
//!
//! `tools/native/native_ui.py` copies this folder to
//! `crates/horizon-client-render/src/native_ui/` and decodes the mod's assets
//! (fonts, logo, backgrounds, emoji) into `assets/`.

pub mod atlas;
pub mod canvas;
pub mod font;
pub mod icons;
pub mod image;
pub mod shapes;
pub mod theme;

pub use atlas::Atlas;
pub use canvas::Canvas;
pub use font::Fonts;
pub use image::Image;

/// The atlas side, as the mod's (`new Atlas(2048)`).
pub const ATLAS_SIZE: u32 = 2048;

/// The mod's UI files, decoded from its base64 sources when the game is built.
pub mod assets {
    pub const POPPINS_REGULAR: &[u8] = include_bytes!("assets/Poppins-Regular.ttf");
    pub const POPPINS_MEDIUM: &[u8] = include_bytes!("assets/Poppins-Medium.ttf");
    pub const POPPINS_SEMIBOLD: &[u8] = include_bytes!("assets/Poppins-SemiBold.ttf");
    pub const POPPINS_BOLD: &[u8] = include_bytes!("assets/Poppins-Bold.ttf");
    pub const LUCIDE: &[u8] = include_bytes!("assets/lucide.ttf");
    pub const MODULE_ICONS: &[u8] = include_bytes!("assets/native-modules.ttf");
    pub const LOGO: &[u8] = include_bytes!("assets/logo.png");
    pub const BACKGROUND: &[u8] = include_bytes!("assets/bg1.jpg");
    pub const BACKGROUNDS: &[u8] = include_bytes!("assets/bg1-8.jpg");
    pub const HUD_BACKGROUND: &[u8] = include_bytes!("assets/hudbg.jpg");
    /// The chat's quick reactions, by codepoint.
    pub const EMOJI: [(&str, &[u8]); 8] = [
        ("1f389", include_bytes!("assets/emoji-1f389.png")),
        ("1f44d", include_bytes!("assets/emoji-1f44d.png")),
        ("1f480", include_bytes!("assets/emoji-1f480.png")),
        ("1f525", include_bytes!("assets/emoji-1f525.png")),
        ("1f602", include_bytes!("assets/emoji-1f602.png")),
        ("1f622", include_bytes!("assets/emoji-1f622.png")),
        ("1f62e", include_bytes!("assets/emoji-1f62e.png")),
        ("2764", include_bytes!("assets/emoji-2764.png")),
    ];
}

/// A canvas with the mod's atlas, fonts and logo.
pub fn canvas() -> Canvas {
    let fonts = Fonts::new([
        assets::POPPINS_REGULAR,
        assets::POPPINS_MEDIUM,
        assets::POPPINS_SEMIBOLD,
        assets::POPPINS_BOLD,
        assets::LUCIDE,
        assets::MODULE_ICONS,
    ]);
    Canvas::new(Atlas::new(ATLAS_SIZE), fonts, Image::decode(assets::LOGO))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_file_decodes() {
        for (name, bytes) in [
            ("logo", assets::LOGO),
            ("bg1", assets::BACKGROUND),
            ("bg1-8", assets::BACKGROUNDS),
            ("hudbg", assets::HUD_BACKGROUND),
        ] {
            assert!(Image::decode(bytes).is_some(), "{name} does not decode");
        }
        for (name, bytes) in assets::EMOJI {
            assert!(Image::decode(bytes).is_some(), "emoji {name} does not decode");
        }
    }

    #[test]
    fn text_is_measured_and_drawn_as_quads() {
        let mut c = canvas();
        c.begin(800, 600, 2.0);
        let w = c.text_width(font::REGULAR, 12.0, "Native");
        assert!(w > 20.0 && w < 60.0, "width {w}");
        let advance = c.text(font::SEMIBOLD, 12.0, "Hi", 10.0, 10.0, theme::TEXT);
        assert!(advance > 0.0);
        c.round(0.0, 0.0, 100.0, 40.0, 8.0, theme::PANEL);
        c.icon(theme::I_SEARCH, 16.0, 50.0, 50.0, theme::WHITE);
        c.icon(icons::MODS, 16.0, 80.0, 50.0, theme::WHITE);
        let mut batch = HudBatch::new([800, 600]);
        c.finish(&mut batch);
        assert!(!batch.vertices.is_empty());
        // The first frame uploads the whole atlas page.
        assert_eq!(batch.native[0].page, 0);
        assert_eq!(batch.native[0].rect, [0, 0, ATLAS_SIZE, ATLAS_SIZE]);
        assert!(batch.blends.iter().all(|(_, b)| *b == crate::hud::Blend::Native(0)));
    }

    #[test]
    fn clipping_cuts_quads_and_their_uvs() {
        let mut c = canvas();
        c.begin(100, 100, 1.0);
        c.push_clip(0.0, 0.0, 50.0, 100.0);
        c.fill(40.0, 0.0, 20.0, 10.0, theme::WHITE);
        c.pop_clip();
        let mut batch = HudBatch::new([100, 100]);
        c.finish(&mut batch);
        let xs: Vec<f32> = batch.vertices.iter().map(|v| v.pos[0] * batch.scale).collect();
        assert!(xs.iter().all(|x| *x <= 50.0), "{xs:?}");
    }

    use crate::hud::HudBatch;
}
