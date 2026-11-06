#![allow(dead_code)] // The rest of the port (the menu, the editor) reads these.
//! `Module` and `HudModule`: a Native client feature the player can switch on
//! and tune, and the ones drawn on the HUD. A HUD module's place is kept as
//! a fraction of the free screen space (0..1 on each axis), so it stays glued
//! to the same edge when the window is resized. Text modules only implement
//! [`Behaviour::lines`]; graphical ones override `measure` and `paint`.

use super::setting::Setting;
use crate::native_ui::{Canvas, font, theme};

pub const HUD: &str = "HUD";
pub const MECHANIC: &str = "Mechanic";
pub const VISUAL: &str = "Visual";
pub const PERFORMANCE: &str = "Performance";
pub const SETTINGS: &str = "Settings";

pub const MARGIN: f32 = 6.0;
pub const STYLE_CARD: usize = 0;
pub const STYLE_TEXT: usize = 1;
pub const STYLE_BRACKETS: usize = 2;
const FONT: f32 = 12.0;

/// What the modules read about the game this frame (the mod's `Game`).
#[derive(Clone, Debug, Default)]
pub struct GameView {
    pub in_world: bool,
    /// A made-up game for previews in the menu.
    pub sample: bool,
    pub fps: i32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
    /// Blocks per second.
    pub speed: f32,
    pub ping: i32,
    pub server: Option<String>,
    pub cps_left: u32,
    pub cps_right: u32,
    pub mem_used: u64,
    pub mem_max: u64,
    /// Key names of the controls (W, A, S, D…), as bound.
    pub keys: [Option<String>; 9],
    pub down: [bool; 9],
    pub playing: bool,
    pub hud_allowed: bool,
    pub vitals: Vitals,
    /// The world's time of day in ticks (0 - 23999), the game time and the dimension id.
    pub day_time: i64,
    pub game_time: i64,
    pub dimension: String,
}

pub const CONTROLS: [&str; 9] = ["forward", "left", "back", "right", "jump", "sneak", "sprint", "attack", "use"];
pub const FORWARD: usize = 0;
pub const LEFT: usize = 1;
pub const BACK: usize = 2;
pub const RIGHT: usize = 3;
pub const JUMP: usize = 4;
pub const ATTACK: usize = 7;
pub const USE: usize = 8;

impl GameView {
    /// `Game.facing`: the compass direction faced (S, SW, W, NW, N, NE, E, SE).
    pub fn facing(&self) -> &'static str {
        const NAMES: [&str; 8] = ["S", "SW", "W", "NW", "N", "NE", "E", "SE"];
        let f = (((self.yaw % 360.0) + 360.0) % 360.0 / 45.0).round() as usize & 7;
        NAMES[f]
    }

    /// `Game.sample`: what the menu previews show.
    pub fn sample() -> GameView {
        GameView {
            in_world: true,
            sample: true,
            fps: 144,
            x: 128.0,
            y: 64.0,
            z: -256.0,
            yaw: 45.0,
            speed: 4.3,
            ping: 24,
            server: Some("play.example.net".into()),
            cps_left: 8,
            cps_right: 3,
            mem_used: 2 << 30,
            mem_max: 4 << 30,
            playing: true,
            hud_allowed: true,
            vitals: Vitals::sample(),
            day_time: 6000,
            game_time: 123_456,
            dimension: "minecraft:overworld".to_owned(),
            ..GameView::default()
        }
    }
}

/// A HUD module's place and shared look ("Appearance").
pub struct Hud {
    pub fx: f32,
    pub fy: f32,
    def_x: f32,
    def_y: f32,
    pub appearance: Vec<Setting>,
}

/// The index of each shared appearance setting in [`Hud::appearance`], before any are dropped.
pub const SCALE: &str = "scale";
pub const STYLE: &str = "style";
pub const COLOR: &str = "color";
pub const OPACITY: &str = "opacity";
pub const SHADOW: &str = "shadow";
pub const LABELS: &str = "labels";

impl Hud {
    pub fn new(x: f32, y: f32) -> Self {
        Hud {
            fx: x,
            fy: y,
            def_x: x,
            def_y: y,
            appearance: vec![
                Setting::num(SCALE, "Size", 1.0, 0.5, 2.5, 0.05, "x"),
                Setting::choice(STYLE, "Style", STYLE_CARD, &["Card", "Text", "Brackets"]),
                Setting::color(COLOR, "Colour", 0xFFFFFFFF),
                Setting::num(OPACITY, "Background", 55.0, 0.0, 100.0, 5.0, "%").fixed(),
                // Not offered: always on.
                Setting::bool(SHADOW, "Text shadow", true).fixed(),
                Setting::bool(LABELS, "Show labels", true).fixed(),
            ],
        }
    }

    pub fn get(&self, id: &str) -> Option<&Setting> {
        self.appearance.iter().find(|s| s.id == id)
    }
    pub fn get_mut(&mut self, id: &str) -> Option<&mut Setting> {
        self.appearance.iter_mut().find(|s| s.id == id)
    }
    /// Hides shared appearance settings a module does not use (they keep their defaults).
    pub fn drop_setting(&mut self, id: &str) {
        if let Some(s) = self.get_mut(id) {
            s.fixed = true;
        }
    }
    pub fn scale(&self) -> f32 {
        self.get(SCALE).map_or(1.0, Setting::as_f32)
    }
    pub fn style(&self) -> usize {
        self.get(STYLE).map_or(STYLE_CARD, Setting::as_choice)
    }
    pub fn color(&self) -> u32 {
        self.get(COLOR).map_or(0xFFFFFFFF, Setting::as_color)
    }
    pub fn opacity(&self) -> f32 {
        self.get(OPACITY).map_or(55.0, Setting::as_f32)
    }
    pub fn shadow(&self) -> bool {
        self.get(SHADOW).is_none_or(Setting::as_bool)
    }
    pub fn labels(&self) -> bool {
        self.get(LABELS).is_none_or(Setting::as_bool)
    }
    pub fn reset_position(&mut self) {
        self.fx = self.def_x;
        self.fy = self.def_y;
    }

    /// `HudModule.pos`: screen position (logical px) for an anchor fraction, a screen side and the module size.
    pub fn pos(f: f32, screen: f32, size: f32) -> f32 {
        MARGIN + f * (screen - size - 2.0 * MARGIN).max(0.0)
    }

    /// `HudModule.frac`: the inverse of [`Hud::pos`].
    pub fn frac(p: f32, screen: f32, size: f32) -> f32 {
        let free = screen - size - 2.0 * MARGIN;
        if free <= 0.0 { 0.0 } else { ((p - MARGIN) / free).clamp(0.0, 1.0) }
    }

    /// Glass card behind a module (Card style).
    pub fn background(&self, c: &mut Canvas, x: f32, y: f32, w: f32, h: f32, s: f32) {
        let a = self.opacity() / 100.0;
        if a <= 0.01 {
            return;
        }
        c.round(x, y, w, h, 6.0 * s, theme::alpha(0xFF08090C, a));
        c.outline(x, y, w, h, 6.0 * s, 1.0, theme::alpha(theme::HAIRLINE, (a * 1.6).min(1.0)));
    }

    /// Corner brackets around a module (Brackets style).
    pub fn brackets(&self, c: &mut Canvas, x: f32, y: f32, w: f32, h: f32, s: f32) {
        let (t, l) = (s.max(1.0), 5.0 * s);
        let col = theme::alpha(self.color(), 0.85);
        for (bx, by, bw, bh) in [
            (x, y, l, t),
            (x, y, t, l),
            (x + w - l, y, l, t),
            (x + w - t, y, t, l),
            (x, y + h - t, l, t),
            (x, y + h - l, t, l),
            (x + w - l, y + h - t, l, t),
            (x + w - t, y + h - l, t, l),
        ] {
            c.fill(bx, by, bw, bh, col);
        }
    }

    pub fn text_w(&self, c: &mut Canvas, s: f32, t: &str) -> f32 {
        c.text_width(font::SEMIBOLD, FONT * s, t)
    }
    pub fn line_h(&self, c: &mut Canvas, s: f32) -> f32 {
        c.line_height(font::SEMIBOLD, FONT * s)
    }

    /// Text at the module's size with the optional drop shadow; returns the x after it.
    pub fn text(&self, c: &mut Canvas, s: f32, t: &str, x: f32, y: f32, argb: u32) -> f32 {
        self.text_face(c, font::SEMIBOLD, FONT * s, t, x, y, argb)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn text_face(&self, c: &mut Canvas, face: usize, size: f32, t: &str, x: f32, y: f32, argb: u32) -> f32 {
        if self.shadow() {
            let o = (size / 12.0).max(1.0) / c.scale * c.scale.max(1.0);
            c.text(face, size, t, x + o, y + o, theme::alpha(0xFF000000, 0.55 * ((argb >> 24) as f32 / 255.0)));
        }
        c.text(face, size, t, x, y, argb);
        x + c.text_width(face, size, t)
    }
}

/// One line of a text module: its label and its value.
#[derive(Clone, Debug, Default)]
pub struct Line {
    pub label: Option<String>,
    pub value: String,
}

impl Line {
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Line {
            label: Some(label.into()),
            value: value.into(),
        }
    }
    pub fn value(value: impl Into<String>) -> Self {
        Line {
            label: None,
            value: value.into(),
        }
    }
}

/// What a module does. Every method has the Java mod's default.
pub trait Behaviour: Send {
    /// Text modules: label / value pairs, one per line.
    fn lines(&mut self, _m: &ModuleState, _g: &GameView) -> Vec<Line> {
        Vec::new()
    }
    /// True when the value is written before the label ("144 FPS").
    fn value_first(&self) -> bool {
        false
    }
    /// Size in logical px at the module's scale; `None` measures [`Behaviour::lines`].
    fn measure(&mut self, _m: &ModuleState, _c: &mut Canvas, _g: &GameView) -> Option<[f32; 2]> {
        None
    }
    /// Draws the module; false draws [`Behaviour::lines`] the standard way.
    fn paint(&mut self, _m: &ModuleState, _c: &mut Canvas, _x: f32, _y: f32, _g: &GameView) -> bool {
        false
    }
    /// Shown on the HUD right now.
    fn visible(&self, _m: &ModuleState, _g: &GameView) -> bool {
        true
    }
    /// Called every frame while enabled.
    fn frame(&mut self, _m: &ModuleState, _g: &GameView) {}
    /// A setting that does nothing right now (given the others) and is left out of the menu.
    fn hidden(&self, _m: &ModuleState, _id: &str) -> bool {
        false
    }
    /// The module was switched on or off (`Module.onEnable` / `onDisable`).
    fn toggled(&mut self, _on: bool) {}
    /// True when the module reads the player's health, effects or items (`Module.needs`).
    fn needs_vitals(&self) -> bool {
        false
    }
}

/// A module's switch, settings and place, without its behaviour (so a
/// behaviour can read them while it runs).
pub struct ModuleState {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub category: &'static str,
    pub icon: u32,
    pub enabled: bool,
    pub favorite: bool,
    pub default_enabled: bool,
    pub toggle_key: Setting,
    pub settings: Vec<Setting>,
    pub hud: Option<Hud>,
}

impl ModuleState {
    pub fn setting(&self, id: &str) -> Option<&Setting> {
        self.settings.iter().find(|s| s.id == id)
    }
    pub fn bool(&self, id: &str) -> bool {
        self.setting(id).is_some_and(Setting::as_bool)
    }
    pub fn num(&self, id: &str) -> f32 {
        self.setting(id).map_or(0.0, Setting::as_f32)
    }
    pub fn choice(&self, id: &str) -> usize {
        self.setting(id).map_or(0, Setting::as_choice)
    }
    pub fn color(&self, id: &str) -> u32 {
        self.setting(id).map_or(0xFFFFFFFF, Setting::as_color)
    }
    /// Every setting, the toggle binding and shared appearance included (what is saved).
    pub fn all_settings(&self) -> impl Iterator<Item = &Setting> {
        std::iter::once(&self.toggle_key)
            .chain(self.settings.iter())
            .chain(self.hud.iter().flat_map(|h| h.appearance.iter()))
    }
    pub fn all_settings_mut(&mut self) -> impl Iterator<Item = &mut Setting> {
        std::iter::once(&mut self.toggle_key)
            .chain(self.settings.iter_mut())
            .chain(self.hud.iter_mut().flat_map(|h| h.appearance.iter_mut()))
    }
}

pub struct Module {
    pub state: ModuleState,
    pub behaviour: Box<dyn Behaviour>,
}

impl Module {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: &'static str,
        name: &'static str,
        description: &'static str,
        category: &'static str,
        icon: u32,
        enabled: bool,
        settings: Vec<Setting>,
        hud: Option<Hud>,
        behaviour: Box<dyn Behaviour>,
    ) -> Self {
        Module {
            state: ModuleState {
                id,
                name,
                description,
                category,
                icon: crate::native_ui::icons::for_module(id, icon),
                enabled,
                favorite: false,
                default_enabled: enabled,
                toggle_key: Setting::binding(),
                settings,
                hud,
            },
            behaviour,
        }
    }

    /// A HUD module in the HUD category.
    #[allow(clippy::too_many_arguments)]
    pub fn hud(
        id: &'static str,
        name: &'static str,
        description: &'static str,
        icon: u32,
        enabled: bool,
        x: f32,
        y: f32,
        settings: Vec<Setting>,
        behaviour: Box<dyn Behaviour>,
    ) -> Self {
        Self::new(id, name, description, HUD, icon, enabled, settings, Some(Hud::new(x, y)), behaviour)
    }

    /// `HudModule.join`: one line as text.
    fn join(&self, line: &Line) -> String {
        let hud = self.state.hud.as_ref();
        let (labels, style) = hud.map_or((true, STYLE_CARD), |h| (h.labels(), h.style()));
        match &line.label {
            None => line.value.clone(),
            Some(l) if l.is_empty() => line.value.clone(),
            Some(l) if line.value.is_empty() => l.clone(),
            Some(_) if !labels => line.value.clone(),
            Some(l) if self.behaviour.value_first() => format!("{} {l}", line.value),
            Some(l) => format!("{l}{}{}", if style == STYLE_BRACKETS { ": " } else { " " }, line.value),
        }
    }

    /// `HudModule.measure`.
    pub fn measure(&mut self, c: &mut Canvas, g: &GameView) -> [f32; 2] {
        if let Some(size) = self.behaviour.measure(&self.state, c, g) {
            return size;
        }
        let lines = self.behaviour.lines(&self.state, g);
        self.measure_lines(c, &lines)
    }

    fn measure_lines(&self, c: &mut Canvas, lines: &[Line]) -> [f32; 2] {
        let Some(hud) = self.state.hud.as_ref() else {
            return [0.0, 0.0];
        };
        let s = hud.scale();
        let brackets = hud.style() == STYLE_BRACKETS;
        let mut w: f32 = 0.0;
        for line in lines {
            let t = self.join(line);
            let t = if brackets { format!("[{t}]") } else { t };
            w = w.max(hud.text_w(c, s, &t));
        }
        let lh = hud.line_h(c, s);
        let card = hud.style() == STYLE_CARD;
        let n = lines.len() as f32;
        [
            w + if card { 16.0 * s } else { 2.0 * s },
            n * lh + (n - 1.0).max(0.0) * 2.0 * s + if card { 10.0 * s } else { 2.0 * s },
        ]
    }

    /// `HudModule.paint`.
    pub fn paint(&mut self, c: &mut Canvas, x: f32, y: f32, g: &GameView) {
        if self.behaviour.paint(&self.state, c, x, y, g) {
            return;
        }
        let lines = self.behaviour.lines(&self.state, g);
        let size = self.measure_lines(c, &lines);
        let value_first = self.behaviour.value_first();
        let Some(hud) = self.state.hud.as_ref() else {
            return;
        };
        let s = hud.scale();
        let style = hud.style();
        let card = style == STYLE_CARD;
        if card {
            hud.background(c, x, y, size[0], size[1], s);
        }
        let lh = hud.line_h(c, s);
        let mut ty = y + if card { 5.0 * s } else { s };
        let col = hud.color();
        let muted = theme::alpha(col, 0.62);
        for line in &lines {
            let mut tx = x + if card { 8.0 * s } else { s };
            let bare = !hud.labels()
                || line.label.as_deref().is_none_or(str::is_empty)
                || line.value.is_empty();
            if style == STYLE_BRACKETS {
                hud.text(c, s, &format!("[{}]", self.join(line)), tx, ty, col);
            } else if bare {
                hud.text(c, s, &self.join(line), tx, ty, col);
            } else if value_first {
                tx = hud.text(c, s, &format!("{} ", line.value), tx, ty, col);
                hud.text(c, s, line.label.as_deref().unwrap_or(""), tx, ty, muted);
            } else {
                tx = hud.text(c, s, &format!("{} ", line.label.as_deref().unwrap_or("")), tx, ty, muted);
                hud.text(c, s, &line.value, tx, ty, col);
            }
            ty += lh + 2.0 * s;
        }
    }
}

impl Module {
    /// `Module.setEnabled`.
    pub fn set_enabled(&mut self, on: bool) {
        if self.state.enabled != on {
            self.state.enabled = on;
            self.behaviour.toggled(on);
        }
    }

    pub fn is_hud(&self) -> bool {
        self.state.hud.is_some()
    }

    /// True when the module page shows a live preview beside its settings.
    pub fn has_preview(&self) -> bool {
        self.state.category != SETTINGS
    }

    /// `Module.shownSettings`: the module's own settings that currently do something.
    pub fn shown_settings(&self) -> Vec<usize> {
        self.state
            .settings
            .iter()
            .enumerate()
            .filter(|(_, s)| !s.fixed && !self.behaviour.hidden(&self.state, s.id))
            .map(|(i, _)| i)
            .collect()
    }

    /// `HudModule.shownAppearance`: the shared look settings that currently do something.
    pub fn shown_appearance(&self) -> Vec<usize> {
        let Some(hud) = self.state.hud.as_ref() else {
            return Vec::new();
        };
        let card = hud.style() == STYLE_CARD;
        hud.appearance
            .iter()
            .enumerate()
            .filter(|(_, s)| !s.fixed && s.id != SCALE && !(s.id == OPACITY && !card))
            .map(|(i, _)| i)
            .collect()
    }

    /// `Module.resetSettings`.
    pub fn reset_settings(&mut self) {
        for s in self.state.all_settings_mut() {
            s.reset();
        }
        if let Some(h) = self.state.hud.as_mut() {
            h.reset_position();
        }
    }
}

/// One item stack as the HUD modules see it. `key` tells the host which stack to draw
/// (`Canvas::host`), so the modules never hold the game's own type.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StackView {
    pub id: String,
    pub count: u32,
    pub damage: u32,
    pub max: u32,
    pub key: u64,
}

impl StackView {
    /// Remaining durability fraction (1 = new); 1 for things that do not wear.
    pub fn durability(&self) -> f32 {
        if self.max == 0 {
            1.0
        } else {
            ((self.max.saturating_sub(self.damage)) as f32 / self.max as f32).clamp(0.0, 1.0)
        }
    }
}

/// An active potion effect. `id` is the bare vanilla id ("speed"), or "namespace:name" for modded ones.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EffectView {
    pub id: String,
    pub amp: i32,
    pub ticks: i32,
    pub ambient: bool,
    pub infinite: bool,
}

/// What `Canvas::host` kinds mean.
pub const HOST_ITEM: u32 = 1;
pub const HOST_EFFECT: u32 = 2;
/// Keys at and above this are the made-up stacks of previews.
pub const SAMPLE_KEY: u64 = u64::MAX - 1024;

/// What the status modules read: the player's health, effects and items.
#[derive(Clone, Debug)]
pub struct Vitals {
    pub health: f32,
    pub max_health: f32,
    pub absorption: f32,
    pub armor_value: i32,
    pub food: i32,
    pub saturation: f32,
    pub effects: Vec<EffectView>,
    /// head, chest, legs, feet
    pub armor: [Option<StackView>; 4],
    /// 0 - 8 hotbar, 9 - 35 main inventory
    pub inv: Vec<Option<StackView>>,
    pub hand: Option<StackView>,
    pub offhand: Option<StackView>,
    pub selected: usize,
    pub target: Option<(String, f32, f32)>,
}

impl Default for Vitals {
    fn default() -> Self {
        Vitals {
            health: 20.0,
            max_health: 20.0,
            absorption: 0.0,
            armor_value: 0,
            food: 20,
            saturation: 5.0,
            effects: Vec::new(),
            armor: Default::default(),
            inv: vec![None; 36],
            hand: None,
            offhand: None,
            selected: 0,
            target: None,
        }
    }
}

impl Vitals {
    /// `Game.fillSample`: believable values for the menu's previews.
    pub fn sample() -> Vitals {
        let now = crate::native_ui::canvas::clock_ms();
        let mut v = Vitals {
            health: 17.0,
            absorption: 4.0,
            armor_value: 15,
            food: 15,
            saturation: 4.2,
            ..Vitals::default()
        };
        let ids = ["speed", "strength", "regeneration", "night_vision", "fire_resistance"];
        let amp = [1, 2, 0, 0, 0];
        let ticks = [20 * 134, 20 * 61, 20 * 7 - (now / 50.0) as i32 % 100, 20 * 480, 20 * 193];
        for i in 0..ids.len() {
            v.effects.push(EffectView { id: ids[i].to_owned(), amp: amp[i], ticks: ticks[i], ambient: false, infinite: false });
        }
        let mut next = 0u64;
        let mut stack = |id: &str, count: u32, damage: u32, max: u32| {
            next += 1;
            Some(StackView { id: id.to_owned(), count, damage, max, key: SAMPLE_KEY + next })
        };
        v.armor = [
            stack("diamond_helmet", 1, 53, 363),
            stack("diamond_chestplate", 1, 120, 528),
            stack("iron_leggings", 1, 31, 225),
            stack("netherite_boots", 1, 17, 481),
        ];
        let items = [
            "diamond_sword", "bow", "golden_apple", "cooked_beef", "ender_pearl", "oak_planks", "cobblestone", "torch", "water_bucket",
            "diamond_pickaxe", "arrow", "oak_log", "iron_ingot", "diamond", "bread", "grass_block", "tnt", "shield", "potion",
            "splash_potion", "lingering_potion", "stone", "leather_chestplate",
        ];
        let counts = [1, 1, 6, 24, 12, 64, 48, 32, 1, 1, 128, 22, 31, 9, 16, 7, 8, 1, 1, 1, 1, 64, 1];
        for (i, id) in items.iter().enumerate() {
            let slot = if i < 9 { i } else { 9 + (i - 9) };
            if slot >= v.inv.len() {
                break;
            }
            let (damage, max) = if i == 0 { (120, 1561) } else if i == 9 { (300, 1561) } else { (0, 0) };
            v.inv[slot] = stack(id, counts[i], damage, max);
        }
        v.hand = stack("diamond_sword", 1, 120, 1561);
        v.offhand = stack("shield", 1, 12, 336);
        v.target = Some(("Zombie".to_owned(), 13.0, 20.0));
        v
    }
}
