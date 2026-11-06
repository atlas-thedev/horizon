//! `Ui`: the immediate-mode UI context. Each frame the views describe what is
//! on screen and get back what the player did (hover, click, typing). Widget
//! state that must survive between frames (animations, scroll offsets) is
//! kept in small maps keyed by a string id.
#![allow(dead_code)]

use horizon_client::native_ui::{Canvas, font, theme};
use std::collections::HashMap;
use std::time::Instant;

pub const KEY_ESCAPE: i32 = 256;
pub const KEY_ENTER: i32 = 257;
pub const KEY_TAB: i32 = 258;
pub const KEY_BACKSPACE: i32 = 259;
pub const KEY_DELETE: i32 = 261;
pub const KEY_RIGHT: i32 = 262;
pub const KEY_LEFT: i32 = 263;
pub const KEY_DOWN: i32 = 264;
pub const KEY_UP: i32 = 265;
pub const KEY_PAGE_UP: i32 = 266;
pub const KEY_PAGE_DOWN: i32 = 267;
pub const KEY_HOME: i32 = 268;
pub const KEY_END: i32 = 269;
pub const KEY_KP_ENTER: i32 = 335;
pub const KEY_RIGHT_SHIFT: i32 = 344;
pub const MOD_SHIFT: i32 = 1;
pub const MOD_CONTROL: i32 = 2;
pub const MOD_ALT: i32 = 4;
pub const MOD_SUPER: i32 = 8;

pub const BTN_GLASS: u8 = 0;
pub const BTN_PRIMARY: u8 = 1;
pub const BTN_GHOST: u8 = 2;
pub const BTN_DANGER: u8 = 3;

/// One input event, in physical pixels (`Input.Event`).
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Move { x: f64, y: f64 },
    /// `button` 0 left, 1 right; `pressed` false for a release.
    Button { button: u8, pressed: bool },
    Scroll { y: f64 },
    /// A GLFW key code and modifier bits; `pressed` false for a release.
    Key { code: i32, mods: i32, pressed: bool },
    Char(char),
}

pub struct Ui {
    pub mx: f32,
    pub my: f32,
    /// Where the pointer is, in physical pixels (kept between frames).
    px: f64,
    py: f64,
    pub down: bool,
    pub pressed: bool,
    pub released: bool,
    pub right_pressed: bool,
    pub scroll: f32,
    pub chars: Vec<char>,
    pub keys: Vec<(i32, i32)>,
    pub dt: f32,
    last: Option<Instant>,
    /// The id the mouse went down on.
    pub active: Option<String>,
    /// The focused text field.
    pub focus: Option<String>,
    anims: HashMap<String, f32>,
    /// Cleared while a modal layer is open so the content beneath does not react.
    pub interactive: bool,
    pub cursor_text: bool,
    pub cursor_hand: bool,
    pub focus_claimed: bool,
    held_keys: u32,
    pending_tooltip: Option<String>,
}

impl Default for Ui {
    fn default() -> Self {
        Ui {
            mx: -1.0,
            my: -1.0,
            px: -1.0,
            py: -1.0,
            down: false,
            pressed: false,
            released: false,
            right_pressed: false,
            scroll: 0.0,
            chars: Vec::new(),
            keys: Vec::new(),
            dt: 0.016,
            last: None,
            active: None,
            focus: None,
            anims: HashMap::new(),
            interactive: true,
            cursor_text: false,
            cursor_hand: false,
            focus_claimed: false,
            held_keys: 0,
            pending_tooltip: None,
        }
    }
}

impl Ui {
    /// Takes this frame's events; `scale` is the canvas's (logical units to pixels).
    pub fn begin(&mut self, events: &[Event], scale: f32) {
        let now = Instant::now();
        self.dt = self.last.map_or(0.016, |l| now.duration_since(l).as_secs_f32().min(0.1));
        self.last = Some(now);
        self.pressed = false;
        self.released = false;
        self.right_pressed = false;
        self.scroll = 0.0;
        self.chars.clear();
        self.keys.clear();
        self.cursor_text = false;
        self.cursor_hand = false;
        self.interactive = true;
        for e in events {
            match e {
                Event::Move { x, y } => {
                    self.px = *x;
                    self.py = *y;
                }
                Event::Button { button: 0, pressed: true } => {
                    self.down = true;
                    self.pressed = true;
                }
                Event::Button { button: 0, pressed: false } => {
                    self.down = false;
                    self.released = true;
                }
                Event::Button { button: 1, pressed: true } => self.right_pressed = true,
                Event::Button { .. } => {}
                Event::Scroll { y } => self.scroll += *y as f32,
                Event::Key { code, mods, pressed } => {
                    self.track_modifier(*code, *pressed);
                    if *pressed {
                        // The modifier keys seen going down are always right: add them to what the event carried.
                        self.keys.push((*code, mods | self.held_mods()));
                    }
                }
                Event::Char(c) => {
                    if *c as u32 >= 32 {
                        // Typing text means Ctrl / Super are not really down (a missed key-up would leave them stuck).
                        self.held_keys &= !(0x22 | 0x88);
                    }
                    self.chars.push(*c);
                }
            }
        }
        self.mx = (self.px / f64::from(scale)) as f32;
        self.my = (self.py / f64::from(scale)) as f32;
        if self.pressed {
            self.active = None;
        }
        self.focus_claimed = false;
    }

    /// One bit per GLFW code 340 - 347 (left shift, ctrl, alt, super, then the right ones).
    fn track_modifier(&mut self, code: i32, down: bool) {
        if !(340..=347).contains(&code) {
            return;
        }
        let bit = 1 << (code - 340);
        if down {
            self.held_keys |= bit;
        } else {
            self.held_keys &= !bit;
        }
    }

    /// Shift / Control / Alt / Super bits for the modifier keys held right now.
    pub fn held_mods(&self) -> i32 {
        let mut m = 0;
        if self.held_keys & 0x11 != 0 {
            m |= MOD_SHIFT;
        }
        if self.held_keys & 0x22 != 0 {
            m |= MOD_CONTROL;
        }
        if self.held_keys & 0x44 != 0 {
            m |= MOD_ALT;
        }
        if self.held_keys & 0x88 != 0 {
            m |= MOD_SUPER;
        }
        m
    }

    /// Forgets which modifier keys were down (their key-ups never arrived).
    pub fn reset_keys(&mut self) {
        self.held_keys = 0;
    }

    /// After the views drew: forgets the pressed widget once the button is up.
    pub fn end(&mut self) {
        if self.pressed && !self.focus_claimed {
            self.focus = None;
        }
        if !self.down && !self.pressed {
            self.active = None;
        }
    }

    pub fn hover(&self, c: &Canvas, x: f32, y: f32, w: f32, h: f32) -> bool {
        self.interactive
            && self.mx >= x
            && self.mx < x + w
            && self.my >= y
            && self.my < y + h
            && c.in_clip(self.mx, self.my)
    }

    /// A clickable region: true on the frame the button is released over it after being pressed on it.
    pub fn clicked(&mut self, c: &Canvas, id: &str, x: f32, y: f32, w: f32, h: f32) -> bool {
        let over = self.hover(c, x, y, w, h);
        if over {
            self.cursor_hand = true;
        }
        if over && self.pressed {
            self.active = Some(id.to_owned());
        }
        over && self.released && self.active.as_deref() == Some(id)
    }

    pub fn is_pressing(&self, id: &str) -> bool {
        self.down && self.active.as_deref() == Some(id)
    }

    pub fn key(&self, key: i32) -> bool {
        self.keys.iter().any(|k| k.0 == key)
    }

    /// Eases a value towards `target`; `speed` ~ 1 / seconds.
    pub fn anim(&mut self, id: &str, target: f32, speed: f32) -> f32 {
        let k = 1.0 - (-speed * self.dt).exp();
        let v = self.anims.entry(id.to_owned()).or_insert(target);
        *v += (target - *v) * k;
        if (target - *v).abs() < 0.001 {
            *v = target;
        }
        *v
    }

    pub fn anim_on(&mut self, id: &str, on: bool, speed: f32) -> f32 {
        self.anim(id, if on { 1.0 } else { 0.0 }, speed)
    }

    pub fn set_anim(&mut self, id: &str, value: f32) {
        self.anims.insert(id.to_owned(), value);
    }

    // ── common widgets ───────────────────────────────────────────────────

    /// A launcher-style button; true when clicked.
    #[allow(clippy::too_many_arguments)]
    pub fn button(&mut self, c: &mut Canvas, id: &str, x: f32, y: f32, w: f32, h: f32, label: &str, icon: u32, style: u8) -> bool {
        let over = self.hover(c, x, y, w, h);
        let click = self.clicked(c, id, x, y, w, h);
        let hv = self.anim_on(&format!("{id}#h"), over, 14.0);
        let pressing = self.is_pressing(id);
        let pr = self.anim_on(&format!("{id}#p"), pressing, 20.0);
        let r = (h / 2.0).min(10.0);
        let (bg, border, fg) = match style {
            BTN_PRIMARY => {
                let bg = theme::mix(theme::mix(0xFFF4F4F5, 0xFFFFFFFF, hv), 0xFFD4D4D8, pr);
                (bg, 0, theme::SOLID_FG)
            }
            BTN_GHOST => (
                theme::alpha(theme::SUBTLE_HOVER, hv),
                0,
                theme::mix(theme::TEXT_SECONDARY, theme::TEXT_STRONG, hv),
            ),
            BTN_DANGER => (theme::mix(0x26EF4444, 0x40EF4444, hv), 0x4DEF4444, 0xFFFCA5A5),
            _ => {
                let bg = theme::mix(theme::mix(0xB808090C, 0xD80E1014, hv), 0xE014171D, pr);
                (
                    bg,
                    theme::mix(theme::HAIRLINE, theme::BORDER_HOVER, hv),
                    theme::mix(theme::TEXT, theme::TEXT_STRONG, hv),
                )
            }
        };
        c.round(x, y, w, h, r, bg);
        if border != 0 {
            c.outline(x, y, w, h, r, 1.0, border);
        }
        let size = if h >= 40.0 { 14.0 } else { 12.5 };
        if icon != 0 && label.is_empty() {
            // Icon only: dead centre.
            c.icon(icon, size + 3.0, x + w / 2.0, y + h / 2.0, fg);
            return click;
        }
        let tw = c.text_width(font::SEMIBOLD, size, label);
        let icon_w = if icon != 0 { size + 4.0 + if label.is_empty() { -4.0 } else { 6.0 } } else { 0.0 };
        let tx = x + (w - (tw + icon_w)) / 2.0;
        if icon != 0 {
            c.icon(icon, size + 3.0, tx + (size + 4.0) / 2.0, y + h / 2.0, fg);
        }
        if !label.is_empty() {
            let lh = c.line_height(font::SEMIBOLD, size);
            c.text(font::SEMIBOLD, size, label, tx + icon_w, y + (h - lh) / 2.0, fg);
        }
        click
    }

    /// A square icon-only button.
    #[allow(clippy::too_many_arguments)]
    pub fn icon_button(&mut self, c: &mut Canvas, id: &str, x: f32, y: f32, size: f32, icon: u32, tooltip: Option<&str>) -> bool {
        let over = self.hover(c, x, y, size, size);
        let click = self.clicked(c, id, x, y, size, size);
        let hv = self.anim_on(&format!("{id}#h"), over, 16.0);
        c.round(x, y, size, size, (size / 2.0).min(8.0), theme::alpha(theme::SUBTLE_HOVER, hv));
        c.icon(icon, size * 0.5, x + size / 2.0, y + size / 2.0, theme::mix(theme::TEXT_MUTED, theme::TEXT_STRONG, hv));
        if over && let Some(t) = tooltip {
            self.pending_tooltip = Some(t.to_owned());
        }
        click
    }

    /// A pill toggle switch; true when clicked.
    pub fn toggle(&mut self, c: &mut Canvas, id: &str, x: f32, y: f32, on: bool) -> bool {
        let (w, h) = (34.0, 20.0);
        let click = self.clicked(c, id, x, y, w, h);
        let t = self.anim_on(&format!("{id}#t"), on, 14.0);
        c.round(x, y, w, h, h / 2.0, theme::mix(0x33FFFFFF, 0xFFF4F4F5, t));
        let k = h - 6.0;
        c.circle(x + 3.0 + k / 2.0 + t * (w - 6.0 - k), y + h / 2.0, k / 2.0, theme::mix(0xFFD4D4D8, theme::SOLID_FG, t));
        click
    }

    /// Requests a tooltip under the cursor for this frame.
    pub fn tip(&mut self, text: &str) {
        if !text.is_empty() {
            self.pending_tooltip = Some(text.to_owned());
        }
    }

    /// Draws the tooltip requested this frame, if any (call last).
    pub fn tooltips(&mut self, c: &mut Canvas) {
        let Some(tip) = self.pending_tooltip.take() else { return };
        let size = 11.5;
        let w = c.text_width(font::MEDIUM, size, &tip) + 16.0;
        let h = 24.0;
        let x = (self.mx + 12.0).min(c.width() - w - 4.0);
        let y = (self.my + 16.0).min(c.height() - h - 4.0);
        c.shadow(x, y + 2.0, w, h, 6.0, 8.0, 0x66000000);
        c.round(x, y, w, h, 6.0, 0xF0141418);
        c.outline(x, y, w, h, 6.0, 1.0, theme::HAIRLINE_STRONG);
        let lh = c.line_height(font::MEDIUM, size);
        c.text(font::MEDIUM, size, &tip, x + 8.0, y + (h - lh) / 2.0, theme::TEXT);
    }

    /// A spinner of three pulsing dots.
    pub fn dots(&self, c: &mut Canvas, cx: f32, cy: f32, color: u32) {
        let now = horizon_client::native_ui::canvas::clock_ms();
        for i in 0..3 {
            let p = (0.5 + 0.5 * (now / 160.0 - f64::from(i) * 0.9).sin()) as f32;
            c.circle(cx + (i - 1) as f32 * 9.0, cy, 2.6, theme::alpha(color, 0.35 + 0.65 * p));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_is_a_press_and_a_release_on_the_same_widget() {
        let mut c = horizon_client::native_ui::canvas();
        c.begin(200, 200, 1.0);
        let mut ui = Ui::default();
        ui.begin(&[Event::Move { x: 10.0, y: 10.0 }, Event::Button { button: 0, pressed: true }], 1.0);
        assert!(!ui.clicked(&c, "b", 0.0, 0.0, 20.0, 20.0));
        ui.end();
        ui.begin(&[Event::Button { button: 0, pressed: false }], 1.0);
        assert!(ui.clicked(&c, "b", 0.0, 0.0, 20.0, 20.0));
        ui.end();
        // Released over another widget: no click.
        ui.begin(&[Event::Button { button: 0, pressed: true }], 1.0);
        ui.clicked(&c, "b", 0.0, 0.0, 20.0, 20.0);
        ui.end();
        ui.begin(&[Event::Move { x: 50.0, y: 50.0 }, Event::Button { button: 0, pressed: false }], 1.0);
        assert!(!ui.clicked(&c, "b", 0.0, 0.0, 20.0, 20.0));
    }

    #[test]
    fn animations_ease_towards_their_target() {
        let mut ui = Ui::default();
        assert_eq!(ui.anim("a", 0.0, 10.0), 0.0);
        ui.dt = 0.05;
        let v = ui.anim("a", 1.0, 10.0);
        assert!(v > 0.3 && v < 0.5, "{v}");
        ui.held_keys = 0;
        ui.begin(&[Event::Key { code: 341, mods: 0, pressed: true }], 1.0);
        assert_eq!(ui.keys, vec![(341, MOD_CONTROL)]);
    }
}
