//! `TextField`: single-line text input with caret, selection, clipboard and
//! horizontal scrolling. Text is kept as characters, so a caret is an index.
#![allow(dead_code)]

use super::ui::{self, Ui};
use horizon_client::native_ui::{Canvas, font, theme};

const KEY_A: i32 = 65;
const KEY_C: i32 = 67;
const KEY_V: i32 = 86;
const KEY_X: i32 = 88;
const KEY_INSERT: i32 = 260;

pub struct TextField {
    pub id: String,
    text: Vec<char>,
    pub caret: usize,
    pub anchor: usize,
    scroll_x: f32,
    blink_start: f64,
    dragging: bool,
    last_press_at: f64,
    last_press_index: i64,
    clicks: u32,
    pub max_length: usize,
    /// Ctrl+V was pressed this frame; `paste_miss` = the text clipboard was empty (maybe a picture is there).
    pub paste_key: bool,
    pub paste_miss: bool,
    /// No box of its own: the owner draws it (the Relay composer).
    pub bare: bool,
}

impl TextField {
    pub fn new(id: &str) -> Self {
        TextField {
            id: id.to_owned(),
            text: Vec::new(),
            caret: 0,
            anchor: 0,
            scroll_x: 0.0,
            blink_start: 0.0,
            dragging: false,
            last_press_at: 0.0,
            last_press_index: -1000,
            clicks: 0,
            max_length: 2000,
            paste_key: false,
            paste_miss: false,
            bare: false,
        }
    }

    pub fn value(&self) -> String {
        self.text.iter().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn len(&self) -> usize {
        self.text.len()
    }

    pub fn set(&mut self, value: &str) {
        self.text = value.chars().collect();
        self.caret = self.text.len();
        self.anchor = self.caret;
    }

    /// Replace an address with clipboard text, without accepting controls or exceeding the field limit.
    pub fn paste_replace(&mut self, text: &str) {
        let cleaned = clean(text);
        let trimmed = cleaned.trim();
        if trimmed.is_empty() { return; }
        self.set("");
        self.insert(trimmed);
    }

    pub fn clear(&mut self) {
        self.set("");
    }

    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.caret = self.text.len();
    }

    /// Types text as if it was entered on the keyboard.
    pub fn type_str(&mut self, s: &str) {
        self.insert(s);
    }

    pub fn focused(&self, ui: &Ui) -> bool {
        ui.focus.as_deref() == Some(self.id.as_str())
    }

    pub fn focus(&mut self, ui: &mut Ui) {
        ui.focus = Some(self.id.clone());
        self.blink_start = ui.now;
    }

    fn slice(&self, to: usize) -> String {
        self.text[..to.min(self.text.len())].iter().collect()
    }

    /// Draws and edits the field. True when Enter was pressed while focused.
    pub fn draw(&mut self, ui: &mut Ui, c: &mut Canvas, x: f32, y: f32, w: f32, h: f32, placeholder: &str, size: f32) -> bool {
        let over = ui.hover(c, x, y, w, h);
        if over {
            ui.cursor_text = true;
        }
        if ui.pressed && over {
            self.focus(ui);
            ui.focus_claimed = true;
        }
        let focused = self.focused(ui);
        self.paste_key = false;
        self.paste_miss = false;
        let mut submit = false;
        let pad = 12.0;
        let inner_w = w - pad * 2.0;
        if focused {
            submit = self.edit(ui);
        }
        if ui.pressed && over {
            let at = self.index_at(c, ui.mx - (x + pad) + self.scroll_x, size);
            self.clicks = if ui.now - self.last_press_at < 400.0 && (at as i64 - self.last_press_index).abs() <= 1 {
                self.clicks + 1
            } else {
                1
            };
            self.last_press_at = ui.now;
            self.last_press_index = at as i64;
            self.dragging = true;
            if self.clicks == 2 {
                self.select_word(at);
            } else if self.clicks >= 3 {
                self.select_all();
            } else {
                self.caret = at;
                self.anchor = at;
            }
            self.blink_start = ui.now;
        } else if self.dragging && ui.down && focused && self.clicks == 1 {
            // Drag-select: the caret follows the pointer, the anchor stays where the press landed.
            self.caret = self.index_at(c, ui.mx - (x + pad) + self.scroll_x, size);
            self.blink_start = ui.now;
        }
        if !ui.down {
            self.dragging = false;
        }

        let fw = ui.anim_on(&format!("{}#f", self.id), focused, 14.0);
        if !self.bare {
            c.round(x, y, w, h, 10.0, theme::mix(0xCC0B0C10, 0xF00E1014, fw));
            c.outline(
                x,
                y,
                w,
                h,
                10.0,
                1.0,
                theme::mix(if over { theme::HAIRLINE_STRONG } else { theme::HAIRLINE }, 0x47FFFFFF, fw),
            );
        }
        let lh = c.line_height(font::REGULAR, size);
        let ty = y + (h - lh) / 2.0;
        let s = self.value();
        let caret_x = c.text_width(font::REGULAR, size, &self.slice(self.caret));
        if caret_x - self.scroll_x > inner_w - 2.0 {
            self.scroll_x = caret_x - inner_w + 2.0;
        }
        if caret_x - self.scroll_x < 0.0 {
            self.scroll_x = caret_x;
        }
        let full = c.text_width(font::REGULAR, size, &s);
        if full - self.scroll_x < inner_w && self.scroll_x > 0.0 {
            self.scroll_x = (full - inner_w).max(0.0);
        }
        c.push_clip(x + pad - 1.0, y, inner_w + 2.0, h);
        if self.caret != self.anchor {
            let (a, b) = (self.caret.min(self.anchor), self.caret.max(self.anchor));
            let sx0 = c.text_width(font::REGULAR, size, &self.slice(a)) - self.scroll_x;
            let sx1 = c.text_width(font::REGULAR, size, &self.slice(b)) - self.scroll_x;
            c.fill(x + pad + sx0, ty + 1.0, sx1 - sx0, lh - 2.0, 0x66A1A1AA);
        }
        if s.is_empty() && !placeholder.is_empty() {
            let p = c.ellipsize(font::REGULAR, size, placeholder, inner_w);
            c.text(font::REGULAR, size, &p, x + pad, ty, theme::TEXT_MUTED);
        } else {
            c.text(font::REGULAR, size, &s, x + pad - self.scroll_x, ty, theme::TEXT_STRONG);
        }
        if focused && (((ui.now - self.blink_start) / 530.0) as i64) % 2 == 0 {
            c.fill(x + pad + caret_x - self.scroll_x, ty + 2.0, 1.2, lh - 4.0, theme::TEXT_STRONG);
        }
        c.pop_clip();
        submit
    }

    fn index_at(&self, c: &mut Canvas, local_x: f32, size: f32) -> usize {
        let mut acc = 0.0;
        for (i, ch) in self.text.iter().enumerate() {
            let a = c.text_width(font::REGULAR, size, &ch.to_string());
            if acc + a / 2.0 > local_x {
                return i;
            }
            acc += a;
        }
        self.text.len()
    }

    fn edit(&mut self, ui: &mut Ui) -> bool {
        let mut submit = false;
        let chars = std::mem::take(&mut ui.chars);
        for ch in &chars {
            if (*ch as u32) < 32 || *ch as u32 == 127 {
                continue;
            }
            self.insert(&ch.to_string());
        }
        ui.chars = chars;
        let keys = ui.keys.clone();
        for (key, mods) in keys {
            let ctrl = mods & (ui::MOD_CONTROL | ui::MOD_SUPER) != 0;
            let shift = mods & ui::MOD_SHIFT != 0;
            self.blink_start = ui.now;
            let len = self.text.len();
            match key {
                ui::KEY_ENTER | ui::KEY_KP_ENTER => submit = true,
                ui::KEY_BACKSPACE => {
                    if self.caret != self.anchor {
                        self.delete_selection();
                    } else if self.caret > 0 {
                        let from = if ctrl { self.word_left(self.caret) } else { self.caret - 1 };
                        self.text.drain(from..self.caret);
                        self.caret = from;
                        self.anchor = from;
                    }
                }
                ui::KEY_DELETE => {
                    if self.caret != self.anchor {
                        self.delete_selection();
                    } else if self.caret < len {
                        let to = if ctrl { self.word_right(self.caret) } else { self.caret + 1 };
                        self.text.drain(self.caret..to);
                    }
                }
                ui::KEY_LEFT => {
                    let mut to = if self.caret == 0 {
                        0
                    } else if ctrl {
                        self.word_left(self.caret)
                    } else {
                        self.caret - 1
                    };
                    if !shift && self.caret != self.anchor {
                        to = self.caret.min(self.anchor);
                    }
                    self.caret = to;
                    if !shift {
                        self.anchor = to;
                    }
                }
                ui::KEY_RIGHT => {
                    let mut to = if self.caret >= len {
                        len
                    } else if ctrl {
                        self.word_right(self.caret)
                    } else {
                        self.caret + 1
                    };
                    if !shift && self.caret != self.anchor {
                        to = self.caret.max(self.anchor);
                    }
                    self.caret = to;
                    if !shift {
                        self.anchor = to;
                    }
                }
                ui::KEY_HOME => {
                    self.caret = 0;
                    if !shift {
                        self.anchor = 0;
                    }
                }
                ui::KEY_END => {
                    self.caret = len;
                    if !shift {
                        self.anchor = len;
                    }
                }
                KEY_A if ctrl => self.select_all(),
                KEY_C | KEY_X | KEY_INSERT if ctrl && self.caret != self.anchor => {
                    let (a, b) = (self.caret.min(self.anchor), self.caret.max(self.anchor));
                    ui.clip_out = Some(self.text[a..b].iter().collect());
                    if key == KEY_X {
                        self.delete_selection();
                    }
                }
                KEY_V | KEY_INSERT if (ctrl && key == KEY_V) || (shift && key == KEY_INSERT) => {
                    self.paste_key = true;
                    let clip = ui.clip_in.clone();
                    if clip.is_empty() {
                        self.paste_miss = true;
                    }
                    self.insert(&clean(&clip));
                }
                _ => {}
            }
        }
        submit
    }

    fn select_word(&mut self, at: usize) {
        let mut from = at.min(self.text.len());
        let mut to = from;
        while from > 0 && self.text[from - 1] != ' ' {
            from -= 1;
        }
        while to < self.text.len() && self.text[to] != ' ' {
            to += 1;
        }
        self.anchor = from;
        self.caret = to;
    }

    fn insert(&mut self, s: &str) {
        if self.caret != self.anchor {
            self.delete_selection();
        }
        let room = self.max_length.saturating_sub(self.text.len());
        if room == 0 {
            return;
        }
        let add: Vec<char> = s.chars().take(room).collect();
        let n = add.len();
        for (i, ch) in add.into_iter().enumerate() {
            self.text.insert(self.caret + i, ch);
        }
        self.caret += n;
        self.anchor = self.caret;
    }

    fn delete_selection(&mut self) {
        let (a, b) = (self.caret.min(self.anchor), self.caret.max(self.anchor));
        self.text.drain(a..b);
        self.caret = a;
        self.anchor = a;
    }

    fn word_left(&self, from: usize) -> usize {
        let mut i = from;
        while i > 0 && self.text[i - 1] == ' ' {
            i -= 1;
        }
        while i > 0 && self.text[i - 1] != ' ' {
            i -= 1;
        }
        i
    }

    fn word_right(&self, from: usize) -> usize {
        let (mut i, n) = (from, self.text.len());
        while i < n && self.text[i] == ' ' {
            i += 1;
        }
        while i < n && self.text[i] != ' ' {
            i += 1;
        }
        i
    }
}

/// Pasted text on one line, without control characters.
fn clean(s: &str) -> String {
    s.chars()
        .filter_map(|ch| match ch {
            '\n' | '\r' | '\t' => Some(' '),
            c if (c as u32) >= 32 && c as u32 != 127 => Some(c),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_and_deleting_edit_at_the_caret() {
        let mut f = TextField::new("t");
        f.set("hello world");
        f.caret = 5;
        f.anchor = 5;
        f.insert(",");
        assert_eq!(f.value(), "hello, world");
        assert_eq!(f.caret, 6);
        f.select_word(8);
        assert_eq!(f.value()[f.anchor..f.caret].to_string(), "world");
        f.delete_selection();
        assert_eq!(f.value(), "hello, ");
        assert_eq!(f.word_left(7), 0);
    }

    #[test]
    fn clipboard_shortcuts_replace_selection_and_copy_it() {
        let mut f = TextField::new("address");
        let mut ui = Ui::default();
        f.set("old");
        f.select_all();
        ui.clip_in = "play.example.net".into();
        ui.keys = vec![(KEY_V, ui::MOD_CONTROL)];
        f.edit(&mut ui);
        assert_eq!(f.value(), "play.example.net");
        f.select_all();
        ui.keys = vec![(KEY_C, ui::MOD_CONTROL)];
        f.edit(&mut ui);
        assert_eq!(ui.clip_out.as_deref(), Some("play.example.net"));
        ui.clip_in = "new.example".into();
        ui.keys = vec![(KEY_INSERT, ui::MOD_SHIFT)];
        f.edit(&mut ui);
        assert_eq!(f.value(), "new.example");
    }

    #[test]
    fn paste_button_replaces_text_cleans_controls_and_respects_limit() {
        let mut f = TextField::new("address");
        f.max_length = 12;
        f.set("old");
        f.paste_replace("  example.org\r\n");
        assert_eq!(f.value(), "example.org");
        f.paste_replace("  \t ");
        assert_eq!(f.value(), "example.org");
        f.paste_replace("1234567890123456");
        assert_eq!(f.value(), "123456789012");
        assert_eq!(f.caret, 12);
    }

    #[test]
    fn the_length_limit_and_pasted_lines() {
        let mut f = TextField::new("t");
        f.max_length = 4;
        f.insert("abcdefg");
        assert_eq!(f.value(), "abcd");
        assert_eq!(clean("a\nb\tc\u{7}"), "a b c");
    }
}
