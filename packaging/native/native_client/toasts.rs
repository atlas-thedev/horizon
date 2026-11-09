//! `Toasts`: small message cards in the top-right corner while playing. Purely
//! visual: they never take input. One card per conversation, newest first, at
//! most three, each up for six seconds.
#![allow(dead_code)]

use super::avatars;
use super::relay::{self, Notice};
use horizon_client::native_ui::{Canvas, font, theme};

const LIFE: i64 = 6000;
const FADE: f32 = 260.0;
const MAX: usize = 3;

#[derive(Default)]
pub struct Toasts {
    shown: Vec<(Notice, i64)>,
}

fn ease(t: f32) -> f32 {
    let u = 1.0 - t;
    1.0 - u * u * u
}

impl Toasts {
    /// Conversation of the newest card still on screen, so the chat key can jump straight to it.
    pub fn latest_key(&self) -> Option<String> {
        self.shown.first().and_then(|(n, _)| n.conv_key.clone())
    }

    pub fn active(&self) -> bool {
        !self.shown.is_empty()
    }

    /// Takes the new notices (dropping them when the player turned pop-ups off).
    pub fn take(&mut self, enabled: bool, viewing: Option<&str>, now: i64) {
        for n in relay::take_notices() {
            if !enabled || (n.conv_key.is_some() && n.conv_key.as_deref() == viewing) {
                continue;
            }
            self.shown.retain(|(s, _)| n.conv_key.is_none() || s.conv_key != n.conv_key);
            self.shown.insert(0, (n, now));
            self.shown.truncate(MAX);
        }
        self.shown.retain(|(_, born)| now - born <= LIFE);
    }

    pub fn draw(&mut self, c: &mut Canvas, relay_key: &str, now: i64) {
        if self.shown.is_empty() {
            return;
        }
        let (w, h, gap) = (290.0f32, 66.0f32, 8.0f32);
        let x = c.width() - w - 14.0;
        let mut y = 14.0f32;
        let hint = format!("Press {relay_key} to open chat");
        for (i, (n, born)) in self.shown.iter().enumerate() {
            let age = (now - born) as f32;
            let fin = (age / FADE).min(1.0);
            let fout = ((LIFE as f32 - age) / FADE).clamp(0.0, 1.0);
            let a = ease(fin.min(fout));
            let slide = (1.0 - ease(fin)) * 24.0;
            c.push_alpha(a);
            let cx = x + slide;
            c.shadow(cx, y + 3.0, w, h, 14.0, 14.0, 0x66000000);
            c.round(cx, y, w, h, 14.0, 0xF00A0B0E);
            c.outline(cx, y, w, h, 14.0, 1.0, theme::HAIRLINE_STRONG);
            avatars::draw(c, n.skin.as_deref(), cx + 12.0, y + 12.0, 38.0, 10.0);
            let (tx, tw) = (cx + 60.0, w - 72.0);
            let title = c.ellipsize(font::SEMIBOLD, 13.0, &n.title, tw);
            c.text(font::SEMIBOLD, 13.0, &title, tx, y + 10.0, theme::TEXT_STRONG);
            let body = c.ellipsize(font::REGULAR, 12.0, &n.body.replace('\n', " "), tw);
            c.text(font::REGULAR, 12.0, &body, tx, y + 27.0, theme::TEXT);
            if i == 0 {
                let h2 = if n.conv_key.as_deref() == Some(relay::REQUESTS_KEY) { hint.replace("open chat", "respond") } else { hint.clone() };
                c.text(font::REGULAR, 10.0, &h2, tx, y + 47.0, theme::TEXT_MUTED);
            }
            let left = (1.0 - age / LIFE as f32).max(0.0);
            c.fill(cx + 14.0, y + h - 2.0, (w - 28.0) * left, 1.0, theme::alpha(theme::ACCENT, 0.55));
            c.pop_alpha();
            y += (h + gap) * a;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice(key: &str) -> Notice {
        Notice { conv_key: Some(key.to_owned()), title: "A".into(), body: "hi".into(), avatar_name: String::new(), skin: None, at: 0 }
    }

    #[test]
    fn one_card_per_chat_and_three_at_most() {
        let mut t = Toasts::default();
        for k in ["dm:1", "dm:2", "dm:1", "dm:3", "dm:4"] {
            t.shown.retain(|(s, _)| s.conv_key.as_deref() != Some(k));
            t.shown.insert(0, (notice(k), 0));
            t.shown.truncate(MAX);
        }
        assert_eq!(t.shown.len(), 3);
        assert_eq!(t.latest_key().as_deref(), Some("dm:4"));
    }

    #[test]
    fn cards_leave_after_their_time() {
        let mut t = Toasts::default();
        t.shown.insert(0, (notice("dm:1"), 0));
        t.take(true, None, LIFE + 1);
        assert!(!t.active());
    }
}
