//! Native Client in game: the Rust port of native-mod's UI runtime
//! (`UiRuntime`, `Game`, `HudOverlay`) and its modules. It draws on the Native
//! UI canvas (`horizon_client::native_ui`) once a frame, from `Game::hud`, and
//! sees every window event first, from `handle_window_event`.
//!
//! `tools/native/native_client.py` copies this folder to
//! `crates/horizon-client/src/game/native_client/`. As a child of `game` it
//! reads the game's own state directly; its own state lives on the game's
//! thread.

mod extra_modules;
mod gameplay_modules;
mod avatars;
mod boxes;
mod chat_media;
mod gif;
mod hud_editor;
mod hud_modules;
mod keys;
mod locker;
mod menu;
mod module;
mod modules;
mod net;
mod preview3d;
mod relay;
mod previews;
mod screenshots;
mod scroll;
mod setting;
mod status;
mod status_modules;
mod textfield;
mod ui;
mod uiconfig;
mod wardrobe;
mod widgets;

use super::{Game, ItemStack, Screen};
use std::collections::HashMap;
use horizon_client::hud::HudBatch;
use horizon_client::native_ui::{self, Canvas};
use hud_editor::HudEditor;
use menu::Menu;
use module::{GameView, Hud};
use modules::Modules;
use ui::{Event, Ui};
use uiconfig::UiConfig;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::PhysicalKey;

/// The id the Native menu opens its own screen under (`native_mod_screen_open`).
const MOD_ID: &str = "native-client";

struct State {
    canvas: Canvas,
    modules: Modules,
    /// Click times in the last second (`Game.click`).
    clicks: [VecDeque<Instant>; 2],
    /// The last position and when, for the speed read-out.
    moved: Option<([f64; 3], Instant)>,
    speed: f32,
    ui: Ui,
    menu: Menu,
    cfg: UiConfig,
    /// Input events since the last frame, for the Native UI.
    events: Vec<Event>,
    was_open: bool,
    editor: HudEditor,
    /// True while the HUD editor (with the hub) is up; else the menu.
    editing: bool,
    /// The signed-in player's name, read when the screen opens.
    who: String,
    /// The stacks the status modules show this frame (`StackView::key` is an index).
    items: Vec<ItemStack>,
    /// The made-up stacks of previews, by key.
    sample_items: HashMap<u64, ItemStack>,
    /// Effect ids by `status_modules::effect_key`, for the icons.
    effect_names: HashMap<u64, String>,
    /// GLFW codes of the keys and mouse buttons held, as the window events told.
    held: std::collections::HashSet<i32>,
    /// The game's own toggle-sprint and toggle-sneak options before Toggle Sprint took them over.
    toggles_before: Option<(bool, bool)>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|cell| {
        let mut slot = cell.borrow_mut();
        let state = slot.get_or_insert_with(|| {
            net::start(game_dir());
            relay::start();
            State {
            canvas: native_ui::canvas(),
            modules: Modules::new(game_dir()),
            clicks: [VecDeque::new(), VecDeque::new()],
            moved: None,
            speed: 0.0,
            ui: Ui::default(),
            menu: Menu::new_in(game_dir()),
            cfg: UiConfig::load(game_dir().as_deref()),
            events: Vec::new(),
            was_open: false,
            editor: HudEditor::new(),
            editing: true,
            who: String::new(),
            items: Vec::new(),
            sample_items: sample_stacks(),
            effect_names: sample_effect_names(),
            held: std::collections::HashSet::new(),
            toggles_before: None,
            }
        });
        f(state)
    })
}

/// The instance folder: where the launcher put the game, else the working folder.
fn game_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("NATIVE_GAME_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
}

/// `Keys.name`, from a binding's key name (`key.keyboard.w` -> `W`).
fn key_label(name: &str) -> Option<String> {
    let last = name.rsplit('.').next()?;
    if name == "key.keyboard.unknown" || last.is_empty() {
        return None;
    }
    let mut chars = last.chars();
    let first = chars.next()?.to_uppercase().collect::<String>();
    Some(format!("{first}{}", chars.as_str()))
}

impl Game<'_> {
    /// `Game.update`: what the modules read this frame.
    fn native_view(&mut self, state: &mut State) -> GameView {
        let player = self.native_player();
        let now = Instant::now();
        for q in &mut state.clicks {
            while q.front().is_some_and(|t| now.duration_since(*t) > Duration::from_secs(1)) {
                q.pop_front();
            }
        }
        if let Some(p) = &player {
            let at = [p.x, p.y, p.z];
            if let Some((last, when)) = state.moved {
                let dt = now.duration_since(when).as_secs_f32();
                if dt >= 0.05 {
                    let d = ((at[0] - last[0]).powi(2) + (at[2] - last[2]).powi(2)).sqrt() as f32;
                    // Smoothed, so the number does not flicker with every frame's step.
                    state.speed += (d / dt - state.speed) * 0.5;
                    state.moved = Some((at, now));
                }
            } else {
                state.moved = Some((at, now));
            }
        } else {
            state.moved = None;
            state.speed = 0.0;
        }
        let bindings = ["key.forward", "key.left", "key.back", "key.right", "key.jump", "key.sneak", "key.sprint"];
        let mut keys: [Option<String>; 9] = Default::default();
        let mut down = [false; 9];
        for (i, name) in bindings.iter().enumerate() {
            let bound = self.settings.keys.get(name);
            keys[i] = key_label(bound);
            down[i] = horizon_client::keybinds::key_code(bound).is_some_and(|code| self.keys.contains(&code));
        }
        down[module::ATTACK] = self.left;
        down[module::USE] = self.right;
        let in_world = player.is_some() && self.title.is_none();
        let (mem_used, mem_max) = horizon_client::debug_overlay::ProcessMemory::read()
            .map_or((0, 0), |m| (m.used, m.max));
        state.items.clear();
        let vitals = if in_world && state.modules.needs_vitals() {
            self.native_vitals(&mut state.items, &mut state.effect_names)
        } else {
            module::Vitals::default()
        };
        let clock = self.native_clock();
        GameView {
            vitals,
            focused: self.focused,
            idle_ms: self.last_input.elapsed().as_secs_f64() * 1000.0,
            keys_down: state.held.iter().copied().collect(),
            sprint_toggled: self.toggles.is_down(1),
            sneak_toggled: self.toggles.is_down(0),
            target_living: in_world && self.crosshair_living() == Some(true),
            reach: if in_world { self.native_reach() } else { None },
            cooldowns: if in_world && state.modules.enabled("cooldowns") { self.native_cooldowns() } else { Vec::new() },
            day_time: clock.as_ref().map_or(0, |c| c.day_time),
            game_time: clock.as_ref().map_or(0, |c| c.game_time),
            dimension: player.as_ref().map(|p| p.dimension.to_string()).unwrap_or_default(),
            in_world,
            sample: false,
            fps: self.fps.round() as i32,
            x: player.as_ref().map_or(0.0, |p| p.x),
            y: player.as_ref().map_or(0.0, |p| p.y),
            z: player.as_ref().map_or(0.0, |p| p.z),
            yaw: player.as_ref().map_or(0.0, |p| p.yaw),
            pitch: player.as_ref().map_or(0.0, |p| p.pitch),
            speed: state.speed,
            ping: self.own_latency().unwrap_or(-1),
            server: self.server_address().map(str::to_owned),
            cps_left: state.clicks[0].len() as u32,
            cps_right: state.clicks[1].len() as u32,
            mem_used,
            mem_max,
            keys,
            down,
            playing: in_world && self.screen == Screen::Playing,
            hud_allowed: in_world
                && matches!(self.screen, Screen::Playing | Screen::Chat | Screen::Mod)
                && !self.ui.hide_gui
                && !self.debug,
        }
    }

    /// Once a frame, over the game's HUD and screens: the Native UI.
    pub(super) fn native_ui_frame(&mut self, batch: &mut HudBatch) {
        use horizon_client::clipboard::Clipboard;
        let open = self.native_mod_screen() == Some(MOD_ID);
        // The clipboard is read only when a paste shortcut came in.
        let clip = if open && with_state(|s| s.wants_paste()) {
            self.clipboard.get()
        } else {
            String::new()
        };
        let (action, copied) = with_state(|state| {
            let g = self.native_view(state);
            let fb = [(batch.width * batch.scale) as u32, (batch.height * batch.scale) as u32];
            let scale = (fb[0] as f32 / 1280.0).min(fb[1] as f32 / 760.0).max(0.75) * state.cfg.scale;
            for m in &mut state.modules.all {
                if m.state.enabled {
                    m.behaviour.frame(&m.state, &g);
                }
            }
            state.modules.tick();
            extra_modules::tick(g.in_world);
            // Toggle Sprint hands the game's own toggle options its choice, and puts them back when it is off.
            let tw = native_ui::tweaks::get();
            match (tw.toggle_sprint || tw.toggle_sneak, state.toggles_before) {
                (true, None) => state.toggles_before = Some((self.settings.toggle_sprint, self.settings.toggle_crouch)),
                (false, Some((sprint, crouch))) => {
                    self.settings.toggle_sprint = sprint;
                    self.settings.toggle_crouch = crouch;
                    state.toggles_before = None;
                }
                _ => {}
            }
            if let Some((sprint, crouch)) = state.toggles_before {
                self.settings.toggle_sprint = tw.toggle_sprint || sprint;
                self.settings.toggle_crouch = tw.toggle_sneak || crouch;
            }
            let mut action = menu::Action::None;
            let mut copied = None;
            if !g.hud_allowed && !open {
                state.events.clear();
                state.was_open = false;
                return (action, copied);
            }
            let c = &mut state.canvas;
            c.begin(fb[0], fb[1], scale);
            let (w, h) = (c.width(), c.height());
            // The modules that draw over the game without a box of their own (the crosshair).
            let now = native_ui::canvas::clock_ms();
            if g.hud_allowed {
                for m in &mut state.modules.all {
                    if m.state.enabled && m.state.hud.is_none() && m.behaviour.overlays(&m.state, &g) {
                        m.behaviour.overlay(&m.state, c, &g, now, false);
                    }
                }
            }
            // `HudOverlay.draw`.
            for m in &mut state.modules.all {
                if !m.state.enabled || m.state.hud.is_none() || !m.behaviour.visible(&m.state, &g) {
                    continue;
                }
                let size = m.measure(c, &g);
                let Some(hud) = m.state.hud.as_ref() else { continue };
                let (x, y) = (Hud::pos(hud.fx, w, size[0]), Hud::pos(hud.fy, h, size[1]));
                m.paint(c, x, y, &g);
            }
            if open {
                let events = std::mem::take(&mut state.events);
                state.ui.clip_in = clip;
                state.ui.begin(&events, scale);
                if !state.was_open {
                    state.menu.opened(state.ui.now);
                    state.editor.opened(state.ui.now);
                    state.was_open = true;
                }
                if state.editing {
                    action = state.editor.draw(&mut state.ui, c, &mut state.modules, &g, g.in_world);
                } else {
                    let who = Some(state.who.as_str()).filter(|n| !n.is_empty());
                    action = state.menu.draw(&mut state.ui, c, &mut state.modules, &mut state.cfg, who, g.in_world);
                }
                state.ui.tooltips(c);
                state.ui.end();
                copied = state.ui.clip_out.take();
            } else {
                state.was_open = false;
                state.events.clear();
            }
            let (items, samples, effects) = (&state.items, &state.sample_items, &state.effect_names);
            c.finish_with(batch, &mut |b, d| self.native_host_draw(b, d, scale, items, samples, effects));
            state.apply(&action);
            (action, copied)
        });
        if let Some(text) = copied {
            self.clipboard.set(&text);
        }
        if action == menu::Action::Close {
            self.native_mod_screen_close(MOD_ID);
        }
    }

    /// Every window event, before the game handles it. True when the Native
    /// UI took it (the game then never sees it).
    pub(super) fn native_window_event(&mut self, event: &WindowEvent) -> bool {
        self.native_track_input(event);
        if self.native_mod_screen() == Some(MOD_ID) {
            return self.native_menu_event(event);
        }
        // The menu key opens the menu from the world.
        if let WindowEvent::KeyboardInput { event: key, .. } = event
            && key.state == ElementState::Pressed
            && !key.repeat
            && let PhysicalKey::Code(code) = key.physical_key
            && self.screen == Screen::Playing
            && self.title.is_none()
            && keys::glfw(code) == Some(with_state(|s| s.cfg.menu_key))
            && self.native_mod_screen_open(MOD_ID)
        {
            // The key opens the HUD editor with the hub in the middle.
            let who = self.options.player_name();
            with_state(|s| {
                s.events.clear();
                s.was_open = false;
                s.editing = true;
                s.who = who;
            });
            return true;
        }
        // The wheel turns a module (Zoom) before it turns the hotbar.
        if let WindowEvent::MouseWheel { delta, .. } = event
            && self.screen == Screen::Playing
        {
            let dy = match delta {
                MouseScrollDelta::LineDelta(_, y) => f64::from(*y),
                MouseScrollDelta::PixelDelta(p) => p.y / 40.0,
            };
            if with_state(|s| s.modules.on_scroll(&s.held, dy)) {
                return true;
            }
        }
        if let WindowEvent::MouseInput {
            state: ElementState::Pressed,
            button,
            ..
        } = event
            && self.screen == Screen::Playing
        {
            let side = match button {
                MouseButton::Left => Some(0),
                MouseButton::Right => Some(1),
                _ => None,
            };
            if let Some(side) = side {
                with_state(|state| state.clicks[side].push_back(Instant::now()));
            }
        }
        false
    }
}

impl Game<'_> {
    /// `McBridge.vitals`: the player's health, effects and items for the status modules.
    fn native_vitals(&mut self, items: &mut Vec<ItemStack>, effect_names: &mut HashMap<u64, String>) -> module::Vitals {
        let mut v = module::Vitals::default();
        if let Some(p) = &mut self.player {
            v.health = p.health.health() as f32;
            v.max_health = p.attribute("minecraft:max_health").unwrap_or(20.0) as f32;
            v.absorption = p.health.absorption() as f32;
            v.armor_value = p
                .attribute("minecraft:armor")
                .or_else(|_| p.attribute("armor"))
                .unwrap_or(0.0) as i32;
            v.food = p.health.food.food as i32;
            v.saturation = p.health.food.saturation as f32;
        }
        for e in &self.potions.effects.list {
            let id = e.id.trim_start_matches("minecraft:").to_owned();
            effect_names.insert(status_modules::effect_key(&id), id.clone());
            v.effects.push(module::EffectView {
                id,
                amp: e.amplifier,
                ticks: e.duration.max(0),
                ambient: e.ambient,
                infinite: e.duration == -1,
            });
        }
        for (i, stack) in self.armor.iter().enumerate() {
            v.armor[i] = stack.as_ref().map(|s| self.native_stack(s, items));
        }
        for (i, stack) in self.inventory.iter().enumerate().take(36) {
            v.inv[i] = stack.as_ref().map(|s| self.native_stack(s, items));
        }
        v.selected = self.selected;
        v.hand = self.inventory.get(self.selected).and_then(Option::as_ref).map(|s| self.native_stack(s, items));
        v.offhand = self.offhand.as_ref().map(|s| self.native_stack(s, items));
        v
    }

    fn native_stack(&self, stack: &ItemStack, items: &mut Vec<ItemStack>) -> module::StackView {
        let max = self.items().get(&stack.item).map_or(0, |i| i.max_damage);
        items.push(stack.clone());
        module::StackView {
            id: stack.item.trim_start_matches("minecraft:").to_owned(),
            count: stack.count,
            damage: stack.damage,
            max,
            key: (items.len() - 1) as u64,
        }
    }

    /// The distance to the entity under the crosshair (`McBridge.reach`).
    fn native_reach(&self) -> Option<f64> {
        [
            self.painting_hit().map(|h| h.1),
            self.frame_hit().map(|h| h.1),
            self.armor_stand_hit().map(|h| h.1),
            self.cushion_hit().map(|h| h.1),
            self.boat_hit().map(|h| h.1),
            self.cart_hit().map(|h| h.1),
            self.interaction_hit().map(|h| h.1),
            self.dragon_hit().map(|h| f64::from(h.1)),
            self.mob_hit().map(|h| h.1),
            self.redirectable_hit().map(|h| h.1),
        ]
        .into_iter()
        .flatten()
        .filter(|d| self.before_block(*d))
        .reduce(f64::min)
    }

    /// The items on cooldown and how much of it is left (`McBridge.cooldowns`).
    fn native_cooldowns(&self) -> Vec<(String, f32)> {
        let partial = self.clock.fraction() as f32;
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for stack in self.inventory.iter().flatten().chain(self.offhand.iter()) {
            let info = self.items().get(&stack.item);
            let group = horizon_client::cooldowns::group(stack, info.and_then(|i| i.use_cooldown.as_ref()));
            if seen.insert(group.clone()) {
                let left = self.cooldowns.percent(&group, partial);
                if left > 0.0 && out.len() < 8 {
                    out.push((stack.item.trim_start_matches("minecraft:").to_owned(), left));
                }
            }
        }
        out
    }

    /// The game draws what the canvas asked it to (`Canvas::host`): an item icon or an effect icon,
    /// at the canvas's place and size.
    fn native_host_draw(
        &self,
        batch: &mut HudBatch,
        d: &native_ui::canvas::HostDraw,
        canvas_scale: f32,
        items: &[ItemStack],
        samples: &HashMap<u64, ItemStack>,
        effects: &HashMap<u64, String>,
    ) {
        // Canvas units are logical pixels; the batch is in GUI units.
        let k = canvas_scale / batch.scale;
        let (gx, gy, size) = (d.x * k, d.y * k, d.size * k);
        match d.kind {
            module::HOST_ITEM => {
                let stack = if d.key >= module::SAMPLE_KEY { samples.get(&d.key) } else { items.get(d.key as usize) };
                let Some(stack) = stack else { return };
                let start = batch.vertices.len();
                self.draw_item_model(batch, stack, gx, gy);
                batch.scale_since(start, gx, gy, size / 16.0, size / 16.0);
            }
            module::HOST_ITEM_ID => {
                let Some(id) = module::item_for_key(d.key) else { return };
                let stack = ItemStack { item: std::sync::Arc::from(id.as_str()), count: 1, damage: 0, extra: None };
                let start = batch.vertices.len();
                self.draw_item_model(batch, &stack, gx, gy);
                batch.scale_since(start, gx, gy, size / 16.0, size / 16.0);
            }
            module::HOST_EFFECT => {
                let Some(id) = effects.get(&d.key) else { return };
                let draws = [super::hud_layout::GuiDraw::Sprite {
                    sprite: std::borrow::Cow::Owned(horizon_client_core::potions::sprite(id)),
                    x: gx.round() as i32,
                    y: gy.round() as i32,
                    w: size.round() as i32,
                    h: size.round() as i32,
                    region: None,
                    color: super::hud_layout::WHITE,
                    blend: super::hud_layout::Blend::Textured,
                }];
                super::hud_layout::paint(batch, &self.gui.atlas, &draws);
            }
            _ => {}
        }
    }
}

/// The made-up stacks of previews, by key (`Vitals::sample` hands out the keys).
fn sample_stacks() -> HashMap<u64, ItemStack> {
    let v = module::Vitals::sample();
    let mut out = HashMap::new();
    let all = v.armor.iter().chain(v.inv.iter()).chain([&v.hand, &v.offhand]).flatten();
    for s in all {
        out.insert(
            s.key,
            ItemStack {
                item: std::sync::Arc::from(s.id.as_str()),
                count: s.count,
                damage: s.damage,
                extra: None,
            },
        );
    }
    out
}

/// The effect ids of previews, by key.
fn sample_effect_names() -> HashMap<u64, String> {
    module::Vitals::sample()
        .effects
        .into_iter()
        .map(|e| (status_modules::effect_key(&e.id), e.id))
        .collect()
}

impl Game<'_> {
    /// Keeps the held keys and mouse buttons (GLFW codes) the modules read.
    fn native_track_input(&mut self, event: &WindowEvent) {
        match event {
            WindowEvent::KeyboardInput { event: key, .. } => {
                if let PhysicalKey::Code(code) = key.physical_key
                    && let Some(g) = keys::glfw(code)
                {
                    let down = key.state == ElementState::Pressed;
                    with_state(|s| {
                        if down {
                            s.held.insert(g);
                        } else {
                            s.held.remove(&g);
                        }
                    });
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let code = match button {
                    MouseButton::Left => 0,
                    MouseButton::Right => 1,
                    MouseButton::Middle => 2,
                    MouseButton::Back => 3,
                    MouseButton::Forward => 4,
                    MouseButton::Other(n) => i32::from(*n) + 2,
                };
                let down = *state == ElementState::Pressed;
                with_state(|s| {
                    if down {
                        s.held.insert(code);
                    } else {
                        s.held.remove(&code);
                    }
                });
            }
            WindowEvent::Focused(false) => with_state(|s| s.held.clear()),
            _ => {}
        }
    }
}

impl State {
    /// What a view asked for: switch between the editor and the menu, jump to a tab or a module.
    fn apply(&mut self, action: &menu::Action) {
        let now = self.ui.now;
        match action {
            menu::Action::EditHud => {
                self.editing = true;
                self.editor.opened(now);
            }
            menu::Action::OpenMenu(tab) => {
                self.editing = false;
                self.menu.opened(now);
                self.menu.open_tab(*tab, now);
            }
            menu::Action::OpenMod(id) => {
                self.editing = false;
                self.menu.opened(now);
                self.menu.open_mod(id, now);
            }
            menu::Action::OpenRelay | menu::Action::Close | menu::Action::None => {}
        }
    }

    /// A paste shortcut is waiting in the input queue.
    fn wants_paste(&self) -> bool {
        self.events.iter().any(|e| {
            matches!(e, Event::Key { code: 86, mods, pressed: true } if mods & (ui::MOD_CONTROL | ui::MOD_SUPER) != 0)
                || matches!(e, Event::Key { code: 260, mods, pressed: true } if mods & ui::MOD_SHIFT != 0)
        })
    }
}

impl Game<'_> {
    /// Window events while the Native menu is open: the menu takes the mouse and
    /// the keyboard. True when it took the event.
    fn native_menu_event(&mut self, event: &WindowEvent) -> bool {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                with_state(|s| s.events.push(Event::Move { x: position.x, y: position.y }));
                true
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let button = match button {
                    MouseButton::Left => 0,
                    MouseButton::Right => 1,
                    _ => return true,
                };
                let pressed = *state == ElementState::Pressed;
                with_state(|s| s.events.push(Event::Button { button, pressed }));
                true
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let y = match delta {
                    MouseScrollDelta::LineDelta(_, y) => f64::from(*y),
                    MouseScrollDelta::PixelDelta(p) => p.y / 40.0,
                };
                with_state(|s| s.events.push(Event::Scroll { y }));
                true
            }
            WindowEvent::KeyboardInput { event: key, .. } => {
                let PhysicalKey::Code(code) = key.physical_key else {
                    return true;
                };
                let Some(glfw) = keys::glfw(code) else {
                    return true;
                };
                let pressed = key.state == ElementState::Pressed;
                let m = self.modifiers;
                let mods = if m.shift_key() { ui::MOD_SHIFT } else { 0 }
                    | if m.control_key() { ui::MOD_CONTROL } else { 0 }
                    | if m.alt_key() { ui::MOD_ALT } else { 0 }
                    | if m.super_key() { ui::MOD_SUPER } else { 0 };
                let close = with_state(|s| {
                    let State { ui, menu, editor, editing, cfg, events, .. } = s;
                    if pressed && glfw == ui::KEY_ESCAPE {
                        return !if *editing { editor.escape(ui) } else { menu.escape(ui) };
                    }
                    // The menu key closes it, unless a field or a keybind is waiting for the key.
                    let listening = if *editing { false } else { menu.widgets.listening.is_some() };
                    if pressed && !key.repeat && glfw == cfg.menu_key && ui.focus.is_none() && !listening {
                        return true;
                    }
                    events.push(Event::Key { code: glfw, mods, pressed });
                    if pressed && mods & (ui::MOD_CONTROL | ui::MOD_SUPER) == 0 {
                        if let Some(text) = &key.text {
                            events.extend(text.chars().map(Event::Char));
                        }
                    }
                    false
                });
                if close {
                    self.native_mod_screen_close(MOD_ID);
                }
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bindings_read_as_key_names() {
        assert_eq!(key_label("key.keyboard.w").as_deref(), Some("W"));
        assert_eq!(key_label("key.keyboard.space").as_deref(), Some("Space"));
        assert_eq!(key_label("key.keyboard.unknown"), None);
    }

    #[test]
    fn the_view_faces_like_the_java_mod() {
        let mut g = GameView::sample();
        g.yaw = 0.0;
        assert_eq!(g.facing(), "S");
        g.yaw = 180.0;
        assert_eq!(g.facing(), "N");
        g.yaw = -90.0;
        assert_eq!(g.facing(), "E");
    }
}
