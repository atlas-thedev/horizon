#![allow(dead_code)] // The rest of the port (the menu, the editor) reads these.
//! `Setting`: one option of a module, shown in its settings panel and saved
//! to `config/native-modules.json` in the Java mod's format, so a player's
//! choices read the same in both editions.

use serde_json::Value;

/// The live list a dropdown setting offers.
#[derive(Clone, Copy, Debug)]
pub struct DeviceList(pub fn() -> Vec<String>);

impl PartialEq for DeviceList {
    fn eq(&self, other: &Self) -> bool {
        self.0 as usize == other.0 as usize
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Bool { value: bool, def: bool },
    Text { value: String, def: String, placeholder: String, max_length: usize },
    Num { value: f32, def: f32, min: f32, max: f32, step: f32, suffix: &'static str },
    /// ARGB.
    Color { value: u32, def: u32 },
    Choice { value: usize, def: usize, options: &'static [&'static str], shorts: Option<&'static [&'static str]> },
    /// A GLFW key code, -1 for none.
    Key { value: i32, def: i32 },
    /// A small pixel drawing (the custom crosshair) and the player's saved drawings.
    Pixels { size: usize, grid: Vec<bool>, def: Vec<bool>, saved: Vec<Vec<bool>>, mirror: bool },
    /// A choice from a live list (microphones, speakers, the counted item). "" is `empty_label`.
    Device { value: String, options: DeviceList, empty_label: &'static str },
    /// `ModuleBinding`: a module's toggle chord, a GLFW key and SHIFT 1 / CONTROL 2 / ALT 4; unbound by default.
    Binding { key: i32, modifiers: i32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Setting {
    pub id: &'static str,
    pub label: &'static str,
    pub kind: Kind,
    /// Kept at its default and not offered in the menu (the module still reads it).
    pub fixed: bool,
    binding_default: (i32, i32),
}

impl Setting {
    pub fn bool(id: &'static str, label: &'static str, def: bool) -> Self {
        Self::new(id, label, Kind::Bool { value: def, def })
    }
    pub fn num(id: &'static str, label: &'static str, def: f32, min: f32, max: f32, step: f32, suffix: &'static str) -> Self {
        Self::new(id, label, Kind::Num { value: def, def, min, max, step, suffix })
    }
    pub fn color(id: &'static str, label: &'static str, def: u32) -> Self {
        Self::new(id, label, Kind::Color { value: def, def })
    }
    pub fn choice(id: &'static str, label: &'static str, def: usize, options: &'static [&'static str]) -> Self {
        Self::new(id, label, Kind::Choice { value: def, def, options, shorts: None })
    }
    pub fn key(id: &'static str, label: &'static str, def: i32) -> Self {
        Self::new(id, label, Kind::Key { value: def, def })
    }
    pub fn text(id: &'static str, label: &'static str, def: &str, placeholder: &str, max_length: usize) -> Self {
        Self::new(
            id,
            label,
            Kind::Text { value: def.to_owned(), def: def.to_owned(), placeholder: placeholder.to_owned(), max_length },
        )
    }
    pub fn pixels(id: &'static str, label: &'static str, size: usize, def: Vec<bool>) -> Self {
        Self::new(id, label, Kind::Pixels { size, grid: def.clone(), def, saved: Vec::new(), mirror: true })
    }
    /// A choice from a live list, shown as a dropdown.
    pub fn device(id: &'static str, label: &'static str, options: fn() -> Vec<String>, empty_label: &'static str) -> Self {
        Self::new(id, label, Kind::Device { value: String::new(), options: DeviceList(options), empty_label })
    }
    /// A module's toggle keybind, unbound.
    pub fn binding() -> Self {
        Self::new("toggleKey", "Toggle keybind", Kind::Binding { key: -1, modifiers: 0 })
    }
    pub fn chord(id: &'static str, label: &'static str, key: i32, modifiers: i32) -> Self {
        let mut s = Self::new(id, label, Kind::Binding { key, modifiers });
        s.binding_default = (key, modifiers);
        s
    }
    fn new(id: &'static str, label: &'static str, kind: Kind) -> Self {
        Setting { id, label, kind, fixed: false, binding_default: (-1, 0) }
    }
    /// Not offered in the menu.
    pub fn fixed(mut self) -> Self {
        self.fixed = true;
        self
    }
    /// A different starting value (and reset target) for a shared choice.
    pub fn initial(mut self, v: usize) -> Self {
        if let Kind::Choice { value, def, .. } = &mut self.kind {
            *value = v;
            *def = v;
        }
        self
    }
    pub fn shorts(mut self, s: &'static [&'static str]) -> Self {
        if let Kind::Choice { shorts, .. } = &mut self.kind {
            *shorts = Some(s);
        }
        self
    }

    pub fn as_bool(&self) -> bool {
        matches!(self.kind, Kind::Bool { value: true, .. })
    }
    pub fn as_f32(&self) -> f32 {
        match self.kind {
            Kind::Num { value, .. } => value,
            _ => 0.0,
        }
    }
    pub fn as_color(&self) -> u32 {
        match self.kind {
            Kind::Color { value, .. } => value,
            _ => 0xFFFFFFFF,
        }
    }
    pub fn as_choice(&self) -> usize {
        match self.kind {
            Kind::Choice { value, .. } => value,
            _ => 0,
        }
    }
    pub fn as_key(&self) -> i32 {
        match self.kind {
            Kind::Key { value, .. } => value,
            _ => -1,
        }
    }
    pub fn as_text(&self) -> &str {
        match &self.kind {
            Kind::Text { value, .. } => value,
            _ => "",
        }
    }

    pub fn set_bool(&mut self, v: bool) {
        if let Kind::Bool { value, .. } = &mut self.kind {
            *value = v;
        }
    }
    /// `Setting.Num.set`: clamped, then snapped to the step.
    pub fn set_num(&mut self, v: f32) {
        if let Kind::Num { value, min, max, step, .. } = &mut self.kind {
            let v = v.clamp(*min, *max);
            *value = (v / *step).round() * *step;
        }
    }
    pub fn set_color(&mut self, v: u32) {
        if let Kind::Color { value, .. } = &mut self.kind {
            *value = v;
        }
    }
    pub fn set_choice(&mut self, v: usize) {
        if let Kind::Choice { value, options, .. } = &mut self.kind {
            *value = v.min(options.len().saturating_sub(1));
        }
    }
    pub fn set_key(&mut self, v: i32) {
        if let Kind::Key { value, .. } = &mut self.kind {
            *value = v;
        }
    }
    pub fn set_text(&mut self, v: &str) {
        if let Kind::Text { value, max_length, .. } = &mut self.kind {
            *value = if *max_length > 0 { v.chars().take(*max_length).collect() } else { v.to_owned() };
        }
    }

    /// `Setting.Num.text`: the value as the menu shows it.
    pub fn num_text(&self) -> String {
        match self.kind {
            Kind::Num { value, step, suffix, .. } => {
                let n = if step >= 1.0 {
                    value.round().to_string()
                } else if step >= 0.1 {
                    format!("{value:.1}")
                } else {
                    format!("{value:.2}")
                };
                format!("{n}{suffix}")
            }
            _ => String::new(),
        }
    }

    /// Back to the value the setting ships with.
    pub fn reset(&mut self) {
        match &mut self.kind {
            Kind::Bool { value, def } => *value = *def,
            Kind::Text { value, def, .. } => *value = def.clone(),
            Kind::Num { value, def, .. } => *value = *def,
            Kind::Color { value, def } => *value = *def,
            Kind::Choice { value, def, .. } => *value = *def,
            Kind::Key { value, def } => *value = *def,
            Kind::Pixels { grid, def, .. } => *grid = def.clone(),
            Kind::Device { value, .. } => value.clear(),
            Kind::Binding { key, modifiers } => (*key, *modifiers) = self.binding_default,
        }
    }

    /// True while the setting holds its shipped value.
    pub fn is_default(&self) -> bool {
        match &self.kind {
            Kind::Bool { value, def } => value == def,
            Kind::Text { value, def, .. } => value == def,
            Kind::Num { value, def, .. } => (value - def).abs() < 1e-4,
            Kind::Color { value, def } => value == def,
            Kind::Choice { value, def, .. } => value == def,
            Kind::Key { value, def } => value == def,
            Kind::Pixels { grid, def, .. } => grid == def,
            Kind::Device { value, .. } => value.is_empty(),
            Kind::Binding { key, modifiers } => (*key, *modifiers) == self.binding_default,
        }
    }

    fn bits(grid: &[bool]) -> String {
        grid.iter().map(|b| if *b { '1' } else { '0' }).collect()
    }

    fn unbits(text: &str, size: usize) -> Option<Vec<bool>> {
        (text.len() == size * size && text.bytes().all(|b| b == b'0' || b == b'1'))
            .then(|| text.bytes().map(|b| b == b'1').collect())
    }

    pub fn save(&self) -> Value {
        match &self.kind {
            Kind::Bool { value, .. } => Value::Bool(*value),
            Kind::Text { value, .. } => Value::String(value.clone()),
            Kind::Num { value, .. } => serde_json::json!(value),
            Kind::Color { value, .. } => Value::String(format!("#{value:08X}")),
            Kind::Choice { value, options, .. } => Value::String(options[*value].to_owned()),
            Kind::Key { value, .. } => serde_json::json!(value),
            Kind::Device { value, .. } => Value::String(value.clone()),
            Kind::Pixels { grid, saved, .. } => serde_json::json!({
                "grid": Self::bits(grid),
                "saved": saved.iter().map(|g| Self::bits(g)).collect::<Vec<_>>(),
            }),
            Kind::Binding { key, modifiers } => serde_json::json!({"key": key, "modifiers": modifiers}),
        }
    }

    /// Reads a saved value; anything that does not parse keeps the current one.
    pub fn load(&mut self, e: &Value) {
        match self.kind {
            Kind::Text { .. } => return self.set_text(e.as_str().unwrap_or("")),
            Kind::Num { .. } => {
                if let Some(v) = e.as_f64() {
                    self.set_num(v as f32);
                }
                return;
            }
            _ => {}
        }
        match &mut self.kind {
            Kind::Bool { value, .. } => {
                if let Some(b) = e.as_bool() {
                    *value = b;
                }
            }
            Kind::Text { .. } | Kind::Num { .. } => {}
            Kind::Color { value, .. } => {
                if let Some(s) = e.as_str() {
                    let s = s.replace('#', "");
                    let s = if s.len() == 6 { format!("FF{s}") } else { s };
                    if let Ok(v) = u32::from_str_radix(&s, 16) {
                        *value = v;
                    }
                }
            }
            Kind::Choice { value, options, .. } => {
                if let Some(s) = e.as_str()
                    && let Some(i) = options.iter().position(|o| *o == s)
                {
                    *value = i;
                }
            }
            Kind::Key { value, .. } => {
                if let Some(v) = e.as_i64() {
                    *value = v as i32;
                }
            }
            Kind::Device { value, .. } => {
                if let Some(s) = e.as_str() {
                    *value = s.to_owned();
                }
            }
            Kind::Pixels { size, grid, saved, .. } => {
                if let Some(g) = e.get("grid").and_then(Value::as_str).and_then(|g| Self::unbits(g, *size)) {
                    *grid = g;
                }
                if let Some(list) = e.get("saved").and_then(Value::as_array) {
                    *saved = list.iter().filter_map(|v| v.as_str().and_then(|g| Self::unbits(g, *size))).collect();
                }
            }
            Kind::Binding { key, modifiers } => {
                (*key, *modifiers) = (-1, 0);
                let k = e.get("key").and_then(Value::as_i64);
                let m = e.get("modifiers").and_then(Value::as_i64);
                if let (Some(k), Some(m)) = (k, m)
                    && bindable(k as i32)
                    && m & !7 == 0
                {
                    (*key, *modifiers) = (k as i32, m as i32);
                }
            }
        }
    }
}

/// `ModuleBinding.bindable`: keys a toggle chord may use (not Escape, Backspace or Delete).
pub fn bindable(code: i32) -> bool {
    ((32..=96).contains(&code)
        || (161..=162).contains(&code)
        || (256..=269).contains(&code)
        || (280..=284).contains(&code)
        || (290..=314).contains(&code)
        || (320..=336).contains(&code)
        || code == 348)
        && code != 256
        && code != 259
        && code != 261
}

/// `Setting.Pixels.plus`: a plus sign with a gap in the middle, as the starting drawing.
pub fn plus(size: usize, arm: usize, gap: usize) -> Vec<bool> {
    let mut g = vec![false; size * size];
    let c = size / 2;
    for i in gap + 1..=gap + arm {
        if c + i >= size || i > c {
            break;
        }
        g[c * size + c + i] = true;
        g[c * size + c - i] = true;
        g[(c + i) * size + c] = true;
        g[(c - i) * size + c] = true;
    }
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip_in_the_java_format() {
        let mut c = Setting::color("color", "Colour", 0xFFFFFFFF);
        c.set_color(0x80112233);
        assert_eq!(c.save(), Value::String("#80112233".into()));
        let mut back = Setting::color("color", "Colour", 0xFFFFFFFF);
        back.load(&Value::String("#112233".into()));
        assert_eq!(back.as_color(), 0xFF112233);

        let mut n = Setting::num("scale", "Size", 1.0, 0.5, 2.5, 0.05, "x");
        n.set_num(9.0);
        assert_eq!(n.as_f32(), 2.5);
        n.set_num(1.234);
        assert!((n.as_f32() - 1.25).abs() < 1e-4);
        assert_eq!(n.num_text(), "1.25x");

        let mut ch = Setting::choice("style", "Style", 0, &["Card", "Text", "Brackets"]);
        ch.load(&Value::String("Brackets".into()));
        assert_eq!(ch.as_choice(), 2);
        assert_eq!(ch.save(), Value::String("Brackets".into()));
        ch.load(&Value::String("Nope".into()));
        assert_eq!(ch.as_choice(), 2);
        assert!(!ch.is_default());
        ch.reset();
        assert!(ch.is_default());
    }

    #[test]
    fn a_pixel_drawing_saves_as_bits() {
        let mut p = Setting::pixels("pixels", "Crosshair", 5, plus(5, 1, 0));
        let saved = p.save();
        assert_eq!(saved["grid"], "0000000100010100010000000");
        p.reset();
        p.load(&serde_json::json!({"grid": "1111111111111111111111111", "saved": ["0000000000000000000000000"]}));
        assert!(matches!(&p.kind, Kind::Pixels { grid, saved, .. } if grid.iter().all(|b| *b) && saved.len() == 1));
    }
}
