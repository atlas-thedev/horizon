//! Native Music on the HUD. The player itself is the Music page of the Native
//! menu (`music_view`).

use super::module::*;
use super::music;
use super::music_view::{cover_art, sound_bars};
use super::setting::Setting;
use crate::native_ui::{Canvas, font, theme};
use std::sync::Arc;

pub fn all() -> Vec<Module> {
    let mut m = Module::hud(
        "now-playing",
        "Now Playing",
        "The song playing from the Music page: cover, title, progress and sound bars.",
        theme::I_AUDIO_LINES,
        true,
        1.0,
        0.13, // Under the watermark's corner.
        vec![
            Setting::bool("bars", "Sound bars", true),
            Setting::bool("cover", "Album cover", true),
            Setting::bool("time", "Song progress", true),
            Setting::bool("hidepaused", "Hide while paused", false),
        ],
        Box::new(NowPlaying { levels: [0.0; 28] }),
    );
    if let Some(h) = m.state.hud.as_mut() {
        h.drop_setting(STYLE);
        h.drop_setting(LABELS);
    }
    vec![m]
}

/// What's playing from the Music page: cover, song, progress and sound bars, in the same dark card as every HUD module.
struct NowPlaying {
    levels: [f32; 28],
}

impl Behaviour for NowPlaying {
    fn visible(&self, m: &ModuleState, _g: &GameView) -> bool {
        music::current().is_some() && (music::playing() || !m.bool("hidepaused"))
    }

    fn measure(&mut self, m: &ModuleState, _c: &mut Canvas, _g: &GameView) -> Option<[f32; 2]> {
        let s = m.hud.as_ref()?.scale();
        Some([224.0 * s, (if m.bool("time") { 72.0 } else { 58.0 }) * s + if m.bool("bars") { 16.0 * s } else { 0.0 }])
    }

    fn paint(&mut self, m: &ModuleState, c: &mut Canvas, x: f32, y: f32, g: &GameView) -> bool {
        let Some(hud) = m.hud.as_ref() else { return true };
        let s = hud.scale();
        let Some([w, h]) = self.measure(m, c, g) else { return true };
        let t: Option<Arc<music::Track>> = music::current();
        let demo = t.is_none(); // The HUD editor / module page before anything played.
        let on = demo || music::playing();
        hud.background(c, x, y, w, h, s);
        let (pad, cs) = (9.0 * s, 40.0 * s);
        let mut tx = x + pad;
        if m.bool("cover") {
            cover_art(c, t.as_ref(), x + pad, y + pad, cs, 6.0 * s);
            tx += cs + 10.0 * s;
        }
        let tw = x + w - pad - tx - 16.0 * s;
        let (title, sub) = match &t {
            Some(t) => (t.title(), t.subtitle()),
            None => ("Your song".to_owned(), "Music".to_owned()),
        };
        let title = c.ellipsize(font::SEMIBOLD, 12.0 * s, &title, tw);
        hud.text_face(c, font::SEMIBOLD, 12.0 * s, &title, tx, y + pad + 3.0 * s, hud.color());
        let sub = c.ellipsize(font::REGULAR, 10.5 * s, &sub, tw);
        c.text(font::REGULAR, 10.5 * s, &sub, tx, y + pad + 21.0 * s, theme::TEXT_MUTED);
        // A small equaliser while it plays, the pause sign while it doesn't.
        let now = crate::native_ui::canvas::clock_ms();
        let (ex, ey) = (x + w - pad - 7.0 * s, y + pad + 10.0 * s);
        if on {
            for b in 0..3 {
                let bh = (3.0 + 7.0 * (0.5 + 0.5 * ((now / (140.0 + f64::from(b) * 45.0) + f64::from(b) * 1.9).sin() as f32))) * s;
                c.round(ex - 5.0 * s + b as f32 * 4.0 * s, ey + 5.0 * s - bh, 2.4 * s, bh, s, hud.color());
            }
        } else {
            c.icon(theme::I_PAUSE, 11.0 * s, ex, ey, theme::TEXT_MUTED);
        }
        let mut by = y + pad + cs + 7.0 * s;
        if m.bool("time") {
            let (pos, dur) = if demo { (83_000, 207_000) } else { (music::position(), music::duration()) };
            let frac = if dur > 0 { (pos as f32 / dur as f32).min(1.0) } else { 0.0 };
            let (l, r) = (music::time(pos), music::time(dur));
            let (lw, rw) = (c.text_width(font::MEDIUM, 9.0 * s, &l), c.text_width(font::MEDIUM, 9.0 * s, &r));
            let (bx, bw) = (x + pad + lw + 7.0 * s, w - pad * 2.0 - lw - rw - 14.0 * s);
            c.text(font::MEDIUM, 9.0 * s, &l, x + pad, by - s, theme::TEXT_MUTED);
            c.text(font::MEDIUM, 9.0 * s, &r, x + w - pad - rw, by - s, theme::TEXT_MUTED);
            c.round(bx, by + 4.0 * s, bw, 3.0 * s, 1.5 * s, theme::SUBTLE_HOVER);
            c.round(bx, by + 4.0 * s, (bw * frac).max(3.0 * s), 3.0 * s, 1.5 * s, hud.color());
            by += 13.0 * s;
        }
        if m.bool("bars") {
            if demo {
                for (i, l) in self.levels.iter_mut().enumerate() {
                    let i = i as f64;
                    *l = 0.25 + 0.55 * ((now / 380.0 + i * 0.55).sin() * (now / 910.0 + i * 0.23).sin()).abs() as f32;
                }
            } else {
                music::bars(&mut self.levels);
            }
            sound_bars(c, &self.levels, x + pad, by, w - pad * 2.0, 12.0 * s, if on { 0.55 } else { 0.18 });
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_card_is_taller_with_progress_and_bars() {
        let mut m = all().remove(0);
        let mut c = crate::native_ui::canvas();
        c.begin(800, 600, 1.0);
        let g = GameView::default();
        let full = m.behaviour.measure(&m.state, &mut c, &g).unwrap();
        m.state.settings.iter_mut().find(|s| s.id == "bars").unwrap().set_bool(false);
        let less = m.behaviour.measure(&m.state, &mut c, &g).unwrap();
        assert!(full[1] > less[1]);
        assert_eq!(full[0], 224.0);
    }

    #[test]
    fn it_is_on_by_default_in_the_hud_category() {
        let m = all().remove(0);
        assert!(m.state.enabled);
        assert_eq!(m.state.category, HUD);
    }
}
