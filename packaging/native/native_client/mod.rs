//! Native Client in game: the Rust port of native-mod's UI runtime
//! (`UiRuntime`, `Game`, `HudOverlay`) and its modules. It draws on the Native
//! UI canvas (`horizon_client::native_ui`) once a frame, from `Game::hud`, and
//! sees every window event first, from `handle_window_event`.
//!
//! `tools/native/native_client.py` copies this folder to
//! `crates/horizon-client/src/game/native_client/`. As a child of `game` it
//! reads the game's own state directly; its own state lives on the game's
//! thread.

mod discord_module;
mod emote_wheel;
mod extra_modules;
mod gameplay_modules;
mod ads;
mod avatars;
mod boxes;
mod chat_media;
mod common;
mod gif;
mod hud_editor;
mod hud_modules;
mod keys;
mod locker;
mod menu;
mod music;
mod music_modules;
mod music_view;
mod module;
mod modules;
mod net;
mod pause_view;
mod overlay_modules;
mod preview3d;
mod relay;
mod relay_chat;
mod relay_view;
mod previews;
mod screenshots;
mod scroll;
mod servers_dialogs;
mod servers_view;
mod setting;
mod status;
mod status_modules;
mod textfield;
mod title_view;
mod toasts;
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
    /// Max durability per preview stack key, for title-screen previews.
    sample_max: HashMap<u64, u32>,
    /// Effect ids by `status_modules::effect_key`, for the icons.
    effect_names: HashMap<u64, String>,
    /// GLFW codes of the keys and mouse buttons held, as the window events told.
    held: std::collections::HashSet<i32>,
    /// The game's own toggle-sprint and toggle-sneak options before Toggle Sprint took them over.
    /// The game's own toggle-sprint and toggle-sneak options before Toggle Sprint took them over.
    toggles_before: Option<(bool, bool)>,
    /// Freelook put the camera behind the player (it goes back to first person when Freelook ends).
    freelook_switched: bool,
    /// The Relay chat, and whether it is what the Native screen shows.
    relay: relay_view::RelayView,
    pause: pause_view::PauseView,
    pause_was: bool,
    relay_open: bool,
    emote_wheel: emote_wheel::Wheel,
    /// The Native title: its view, what is open over it (0 nothing, 1 the menu, 2 Relay, 3 the HUD editor), whether
    /// it was drawn last frame and what it asked of the game.
    title: title_view::TitleView,
    title_overlay: u8,
    title_was: bool,
    title_pending: Option<title_view::TitleAct>,
    /// The Native multiplayer screen, and whether the game's own was asked for (until the title shows again).
    servers: servers_view::ServersView,
    servers_dialog: servers_dialogs::ServerDialog,
    servers_classic: bool,
    /// The chat to open the Relay screen on (from a toast).
    relay_select: Option<String>,
    toasts: toasts::Toasts,
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
            music::init(game_dir().as_deref());
            music::start();
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
            sample_max: sample_max_damage(),
            effect_names: sample_effect_names(),
            held: std::collections::HashSet::new(),
            toggles_before: None,
            freelook_switched: false,
            relay: relay_view::RelayView::new(),
            pause: pause_view::PauseView,
            pause_was: false,
            relay_open: false,
            emote_wheel: emote_wheel::Wheel::default(),
            title: title_view::TitleView::default(),
            title_overlay: 0,
            title_was: false,
            title_pending: None,
            servers: servers_view::ServersView::default(),
            servers_dialog: servers_dialogs::ServerDialog::default(),
            servers_classic: false,
            relay_select: None,
            toasts: toasts::Toasts::default(),
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

/// The Music page's keys: play / pause, next, previous. True when `key` is one of them.
fn music_key(cfg: &UiConfig, key: i32) -> bool {
    if key < 0 {
        return false;
    }
    if key == cfg.music_play_key {
        music::toggle();
    } else if key == cfg.music_next_key {
        music::next();
    } else if key == cfg.music_prev_key {
        music::previous();
    } else {
        return false;
    }
    true
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
                && !self.debug
                && !self.native_world_loading(),
        }
    }

    /// A world is being made or its terrain is coming in (the level loading, reconfiguring and world creation
    /// screens): the HUD modules stay off, as the game's own HUD does.
    fn native_world_loading(&self) -> bool {
        self.integrated_starting.is_some()
            || self.pending_entry.is_some()
            || self.pack_reload.is_some()
            || self.server_packs.prompt.is_some()
            || self.code_of_conduct.is_some()
            || self.dialog_open()
            || self.link_prompt.is_some()
            || self.command_prompt.is_some()
            || self.remote.as_ref().is_some_and(|r| !r.joined || r.level_load.is_active() || r.reconfiguring.is_some())
    }

    /// Once a frame, over the game's HUD and screens: the Native UI.
    pub(super) fn native_ui_frame(&mut self, batch: &mut HudBatch) {
        use horizon_client::clipboard::Clipboard;
        let open = self.native_mod_screen() == Some(MOD_ID);
        let paused = self.native_pause_active();
        let pause_info = if paused { Some(self.native_pause_info()) } else { None };
        // The clipboard is read only when a paste shortcut came in.
        let clip = if open && with_state(|s| s.wants_paste()) {
            self.clipboard.get()
        } else {
            String::new()
        };
        let (action, copied, pause_action) = with_state(|state| {
            let g = self.native_view(state);
            let local = self.options.player_name();
            let mut players: Vec<_> = self.remote_player_names().into_iter().map(|(id,n)| (id,n,self.remote_player_uuid(id).map(|u|format!("{u:032x}")))).collect();
            players.push((0,local.clone(),self.remote.as_ref().map(|r|format!("{:032x}",r.profile()))));
            let scope = if !g.in_world { String::new() } else if let Some(server)=self.server_address() { horizon_client::native_emotes::scope(server) } else { horizon_client::native_emotes::scope(&format!("local:{local}")) };
            horizon_client::native_emotes::tick(scope,players,&local,[g.x,g.y,g.z],g.down[4]||g.down[7]||g.down[8]);
            let fb = [(batch.width * batch.scale) as u32, (batch.height * batch.scale) as u32];
            let scale = (fb[0] as f32 / 1280.0).min(fb[1] as f32 / 760.0).max(0.75) * state.cfg.scale;
            for m in &mut state.modules.all {
                if m.state.enabled {
                    m.behaviour.frame(&m.state, &g);
                }
            }
            state.modules.tick();
            music::configure(
                state.cfg.music_volume,
                state.cfg.music_shuffle,
                music::Repeat::from_index(state.cfg.music_repeat),
                1.0,
            );
            let discord_off = state.modules.get("discord-rpc").is_some_and(|m| !m.state.enabled);
            native_ui::tweaks::set(|t| t.discord_off = discord_off);
            let custom_pause = state.cfg.custom_pause;
            native_ui::tweaks::set(|t| t.custom_pause = custom_pause);
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
            // Freelook: behind the player while it looks around, and back to first person after, but only when the view
            // is still the one Freelook chose (a camera picked with F5 meanwhile stays).
            {
                use super::perspective::CameraType;
                if tw.freelook_third && !state.freelook_switched && self.camera_type == CameraType::FirstPerson {
                    self.camera_type = CameraType::ThirdPersonBack;
                    state.freelook_switched = true;
                } else if !tw.freelook_third && state.freelook_switched {
                    if self.camera_type == CameraType::ThirdPersonBack {
                        self.camera_type = CameraType::FirstPerson;
                    }
                    state.freelook_switched = false;
                }
            }
            let mut action = menu::Action::None;
            let mut copied = None;
            let mut pause_action = pause_view::PauseAction::None;
            if !g.hud_allowed && !open && !paused {
                state.events.clear();
                state.was_open = false;
                state.pause_was = false;
                return (action, copied, pause_action);
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
            if g.hud_allowed {
                for m in &mut state.modules.all {
                    if !m.state.enabled || m.state.hud.is_none() || !m.behaviour.visible(&m.state, &g) {
                        continue;
                    }
                    let size = m.measure(c, &g);
                    let Some(hud) = m.state.hud.as_ref() else { continue };
                    let (x, y) = (Hud::pos(hud.fx, w, size[0]), Hud::pos(hud.fy, h, size[1]));
                    m.paint(c, x, y, &g);
                }
            }
            // Message pop-ups (they never take input).
            {
                let now_ms = relay::now_ms();
                let viewing = if state.relay_open && open { relay::snapshot().viewing } else { None };
                state.toasts.take(state.cfg.notifications, viewing.as_deref(), now_ms);
                if !open {
                    let key = keys::name(state.cfg.relay_key);
                    state.toasts.draw(c, &key, now_ms);
                }
            }
            if !paused {
                state.pause_was = false;
            }
            if let (true, Some(info)) = (paused, pause_info.as_ref()) {
                let events = std::mem::take(&mut state.events);
                state.ui.begin(&events, scale);
                if !state.pause_was {
                    state.pause.opened(&mut state.ui);
                    state.pause_was = true;
                }
                pause_action = state.pause.draw(&mut state.ui, c, info);
                state.ui.tooltips(c);
                state.ui.end();
            } else if open {
                let events = std::mem::take(&mut state.events);
                state.ui.clip_in = clip;
                state.ui.begin(&events, scale);
                if !state.was_open {
                    state.menu.opened(state.ui.now);
                    state.editor.opened(state.ui.now);
                    if state.relay_open {
                        state.relay.opened(&mut state.ui, state.relay_select.take().as_deref());
                    }
                    state.was_open = true;
                }
                if state.emote_wheel.open {
                    if state.emote_wheel.draw(&mut state.ui, c) { action = menu::Action::Close; }
                } else if state.relay_open {
                    if state.relay.draw(&mut state.ui, c, &mut state.cfg, g.in_world) == relay_view::RelayAction::Close {
                        action = menu::Action::Close;
                    }
                } else if state.editing {
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
                state.relay_open = false;
            }
            let (items, samples, effects) = (&state.items, &state.sample_items, &state.effect_names);
            c.finish_with(batch, &mut |b, d| self.native_host_draw(b, d, scale, items, samples, effects));
            state.apply(&action);
            (action, copied, pause_action)
        });
        self.native_pause_do(pause_action);
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
        if let WindowEvent::KeyboardInput { event:key, .. }=event
            && let PhysicalKey::Code(code)=key.physical_key {
            let glfw=keys::glfw(code);
            if with_state(|s|s.emote_wheel.open) {
                if glfw==Some(256) && key.state==ElementState::Pressed {
                    with_state(|s| {s.emote_wheel.open=false;s.emote_wheel.selected=None;}); self.native_mod_screen_close(MOD_ID);return true;
                }
                if glfw==Some(with_state(|s|s.cfg.emote_key)) && key.state==ElementState::Released {
                    with_state(|s|s.emote_wheel.release());self.native_mod_screen_close(MOD_ID);return true;
                }
            } else if glfw==Some(with_state(|s|s.cfg.emote_key)) && key.state==ElementState::Pressed && !key.repeat
                && self.screen==Screen::Playing && self.title.is_none() && self.native_mod_screen().is_none() && self.native_mod_screen_open(MOD_ID) {
                with_state(|s|{s.emote_wheel.open=true;s.emote_wheel.selected=None;s.events.clear();s.was_open=false;});return true;
            }
        }
        if self.native_mod_screen() == Some(MOD_ID) {
            return self.native_menu_event(event);
        }
        if self.native_pause_active() {
            return self.native_pause_event(event);
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
                s.relay_open = false;
                s.who = who;
            });
            return true;
        }
        // The Music page's play / next / previous keys, while playing.
        if let WindowEvent::KeyboardInput { event: key, .. } = event
            && key.state == ElementState::Pressed
            && !key.repeat
            && let PhysicalKey::Code(code) = key.physical_key
            && self.screen == Screen::Playing
            && let Some(glfw) = keys::glfw(code)
            && with_state(|s| music_key(&s.cfg, glfw))
        {
            return true;
        }
        // The Relay key opens the chat from the world.
        if let WindowEvent::KeyboardInput { event: key, .. } = event
            && key.state == ElementState::Pressed
            && !key.repeat
            && let PhysicalKey::Code(code) = key.physical_key
            && self.screen == Screen::Playing
            && self.title.is_none()
            && keys::glfw(code) == Some(with_state(|s| s.cfg.relay_key))
            && self.native_mod_screen_open(MOD_ID)
        {
            let latest = with_state(|s| s.toasts.latest_key());
            with_state(|s| {
                s.events.clear();
                s.was_open = false;
                s.relay_open = true;
                s.relay_select = latest;
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
        let full = full_id(&stack.item);
        let max = self
            .items()
            .get(full.as_str())
            .or_else(|| self.items().get(&stack.item))
            .map_or(0, |i| i.max_damage);
        let mut s = stack.clone();
        if !stack.item.contains(':') {
            s.item = std::sync::Arc::from(full.as_str());
        }
        items.push(s);
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
        match d.kind & 0xFF {
            module::HOST_SLOT => {
                let stack = if d.key >= module::SAMPLE_KEY { samples.get(&d.key) } else { items.get(d.key as usize) };
                let Some(stack) = stack else { return };
                let start = batch.vertices.len();
                self.draw_item_model(batch, stack, gx, gy);
                // GuiGraphicsExtractor.itemDecorations: the count in the game's font and the damage bar.
                let mut shown = stack.clone();
                if d.kind & module::SLOT_COUNT == 0 {
                    shown.count = 1;
                }
                if d.kind & module::SLOT_BAR == 0 {
                    shown.damage = 0;
                }
                self.draw_item_decorations(batch, &shown, gx, gy, None);
                batch.scale_since(start, gx, gy, size / 16.0, size / 16.0);
            }
            module::HOST_ITEM => {
                let stack = if d.key >= module::SAMPLE_KEY { samples.get(&d.key) } else { items.get(d.key as usize) };
                let Some(stack) = stack else { return };
                let start = batch.vertices.len();
                self.draw_item_model(batch, stack, gx, gy);
                batch.scale_since(start, gx, gy, size / 16.0, size / 16.0);
            }
            module::HOST_ITEM_ID => {
                let Some(id) = module::item_for_key(d.key) else { return };
                let stack = ItemStack { item: std::sync::Arc::from(full_id(&id).as_str()), count: 1, damage: 0, extra: None };
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
                item: std::sync::Arc::from(full_id(&s.id).as_str()),
                count: s.count,
                damage: s.damage,
                extra: None,
            },
        );
    }
    out
}

/// Max durability of each preview stack by key, for title-screen previews.
fn sample_max_damage() -> HashMap<u64, u32> {
    let v = module::Vitals::sample();
    v.armor
        .iter()
        .chain(v.inv.iter())
        .chain([&v.hand, &v.offhand])
        .flatten()
        .map(|s| (s.key, s.max))
        .collect()
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
            menu::Action::OpenRelay => {
                self.relay_open = true;
                self.relay.opened(&mut self.ui, None);
            }
            menu::Action::Close | menu::Action::None => {}
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
                    let State { ui, menu, editor, editing, cfg, events, relay, relay_open, .. } = s;
                    if pressed && glfw == ui::KEY_ESCAPE {
                        return !if *relay_open {
                            relay.escape(ui)
                        } else if *editing {
                            editor.escape(ui)
                        } else {
                            menu.escape(ui)
                        };
                    }
                    // The Relay key closes the chat again, unless something is being typed.
                    if *relay_open && pressed && !key.repeat && glfw == cfg.relay_key && ui.focus.is_none() {
                        return true;
                    }
                    // The menu key closes it, unless a field or a keybind is waiting for the key.
                    let listening = if *editing { false } else { menu.widgets.listening.is_some() };
                    if pressed && !key.repeat && glfw == cfg.menu_key && ui.focus.is_none() && !listening && !*relay_open {
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

impl Game<'_> {
    /// The pause menu is up and Native draws it (Custom pause menu).
    fn native_pause_active(&self) -> bool {
        self.screen == Screen::Paused && self.pause_menu && with_state(|s| s.cfg.custom_pause)
    }

    fn native_pause_info(&self) -> pause_view::PauseInfo {
        let single = !self.external_server();
        pause_view::PauseInfo {
            title: self.translate("menu.game", "Game Menu"),
            back: self.translate("menu.returnToGame", "Back to Game"),
            advancements: self.translate("gui.advancements", "Advancements"),
            stats: self.translate("gui.stats", "Statistics"),
            options: self.translate("menu.options", "Options..."),
            world: self.translate("options.worldOptions.button", "World Options..."),
            leave: if single {
                self.translate("menu.returnToMenu", "Save and Quit to Title")
            } else {
                self.translate("menu.disconnect", "Disconnect")
            },
            single,
            unread: relay::unread_total(),
        }
    }

    /// What a click on the pause menu asked for: the game's own button of the same name, or Native's screens.
    fn native_pause_do(&mut self, action: pause_view::PauseAction) {
        use pause_view::PauseAction as P;
        let id = match action {
            P::None => return,
            P::Resume => 0,
            P::Advancements => 4,
            P::Stats => 3,
            P::Options => 1,
            P::WorldOptions => 9,
            P::Leave => 2,
            P::Relay | P::Menu | P::Cosmetics => {
                // The pause menu gives way to the Native screen (closing it returns to the game).
                self.pause_focus = None;
                self.screen = Screen::Playing;
                if self.native_mod_screen_open(MOD_ID) {
                    let who = self.options.player_name();
                    with_state(|s| {
                        s.events.clear();
                        s.was_open = true;
                        s.who = who;
                        match action {
                            P::Relay => {
                                s.relay_select = None;
                                s.apply(&menu::Action::OpenRelay);
                            }
                            P::Menu => {
                                s.relay_open = false;
                                s.apply(&menu::Action::OpenMenu(0));
                            }
                            _ => {
                                s.relay_open = false;
                                s.apply(&menu::Action::OpenMenu(1));
                            }
                        }
                    });
                } else {
                    self.screen = Screen::Paused;
                }
                return;
            }
        };
        let _ = self.execute_pause_action(id);
    }

    /// Window events while Native's pause menu is up: it takes the mouse; the keys stay the game's (Esc resumes).
    fn native_pause_event(&mut self, event: &WindowEvent) -> bool {
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
            WindowEvent::MouseWheel { .. } => true,
            WindowEvent::KeyboardInput { event: key, .. } => {
                !matches!(key.physical_key, PhysicalKey::Code(winit::keyboard::KeyCode::Escape))
            }
            _ => false,
        }
    }
}

// ── the Native title screen ──────────────────────────────────────────────

/// The title's own page (not a world list or the options) is the Native one, when it is switched on.
pub(super) fn native_title_active(title: &horizon_client::title::Title) -> bool {
    use horizon_client::title::Mode;
    with_state(|s| match &title.mode {
        Mode::Main => {
            s.servers_classic = false;
            s.servers.closed();
            s.cfg.custom_title
        }
        // The server list itself (Add, Edit, Direct Connection and Delete stay the game's screens).
        Mode::Multiplayer(
            horizon_client::multiplayer::Screen::List { .. }
            | horizon_client::multiplayer::Screen::Edit { .. }
            | horizon_client::multiplayer::Screen::Direct { .. }
            | horizon_client::multiplayer::Screen::ConfirmDelete { .. },
        ) => s.cfg.custom_servers && !s.servers_classic,
        _ => false,
    })
}

/// Draws the Native title (and Relay or the Native menu over it, opened from there), or the Native server list,
/// into `batch`.
pub(super) fn native_title_frame(
    batch: &mut HudBatch,
    title: &horizon_client::title::Title,
    player: &str,
    atlas: &horizon_client::hud::HudAtlas,
    item_renderer: Option<&horizon_client::item_render::ItemRenderer>,
) {
    let allows_multiplayer = title.allows_multiplayer;
    let servers = servers_view::info(title);
    let dialog = servers_dialogs::dialog_info(title);
    with_state(|state| {
        let fb = [(batch.width * batch.scale) as u32, (batch.height * batch.scale) as u32];
        let scale = (fb[0] as f32 / 1280.0).min(fb[1] as f32 / 760.0).max(0.75) * state.cfg.scale;
        let c = &mut state.canvas;
        c.begin(fb[0], fb[1], scale);
        let events = std::mem::take(&mut state.events);
        state.ui.begin(&events, scale);
        if !state.title_was {
            state.title_was = true;
            state.title_overlay = 0;
        }
        let mut deferred = None;
        let info = title_view::TitleInfo { allows_multiplayer, game_dir: game_dir(), player: player.to_owned() };
        let overlay = if dialog.is_some() { 5 } else if servers.is_some() { 4 } else { state.title_overlay };
        if dialog.is_none() {
            state.servers_dialog.closed();
        }
        match overlay {
            5 => {
                state.title.background(c);
                if let Some(info) = dialog.as_ref()
                    && let Some(act) = state.servers_dialog.draw(&mut state.ui, c, info)
                {
                    state.title_pending = Some(act);
                }
            }
            4 => {
                let escape = state.ui.keys.iter().any(|(k, _)| *k == ui::KEY_ESCAPE);
                let mut act = servers.as_ref().and_then(|list| state.servers.draw(&mut state.ui, c, list));
                if escape && act.is_none() {
                    act = state.servers.escape(&mut state.ui);
                }
                if act.is_some() {
                    state.title_pending = act;
                }
            }
            1 => {
                state.title.background(c);
                let who = Some(player).filter(|n| !n.is_empty());
                let action = state.menu.draw(&mut state.ui, c, &mut state.modules, &mut state.cfg, who, false);
                match action {
                    menu::Action::Close => state.title_overlay = 0,
                    menu::Action::OpenRelay => {
                        state.title_overlay = 2;
                        state.relay.opened(&mut state.ui, None);
                    }
                    menu::Action::EditHud => {
                        state.title_overlay = 3;
                        state.editor.opened(state.ui.now);
                    }
                    other => deferred = Some(other),
                }
            }
            2 => {
                state.title.background(c);
                if state.relay.draw(&mut state.ui, c, &mut state.cfg, false) == relay_view::RelayAction::Close {
                    state.title_overlay = 0;
                }
            }
            3 => {
                state.title.background(c);
                let g = module::GameView::sample();
                match state.editor.draw(&mut state.ui, c, &mut state.modules, &g, false) {
                    menu::Action::Close => state.title_overlay = 0,
                    menu::Action::OpenRelay => {
                        state.title_overlay = 2;
                        state.relay.opened(&mut state.ui, None);
                    }
                    menu::Action::OpenMenu(tab) => {
                        state.title_overlay = 1;
                        state.menu.opened(state.ui.now);
                        state.menu.open_tab(tab, state.ui.now);
                    }
                    menu::Action::OpenMod(id) => {
                        state.title_overlay = 1;
                        state.menu.opened(state.ui.now);
                        state.menu.open_mod(&id, state.ui.now);
                    }
                    _ => {}
                }
            }
            _ => match state.title.draw(&mut state.ui, c, &info) {
                Some(title_view::TitleAct::NativeMods) => {
                    state.title_overlay = 1;
                    state.editing = false;
                    state.who = player.to_owned();
                    state.menu.opened(state.ui.now);
                }
                Some(title_view::TitleAct::Relay) => {
                    state.title_overlay = 2;
                    state.relay.opened(&mut state.ui, None);
                }
                Some(title_view::TitleAct::Classic) => {
                    state.cfg.custom_title = false;
                    state.cfg.save();
                }
                Some(other) => state.title_pending = Some(other),
                None => {}
            },
        }
        state.ui.tooltips(c);
        state.ui.end();
        let (samples, sample_max, effects) = (&state.sample_items, &state.sample_max, &state.effect_names);
        c.finish_with(batch, &mut |b, d| {
            title_host_draw(b, d, scale, atlas, item_renderer, samples, sample_max, effects);
        });
        if let Some(action) = deferred {
            state.apply(&action);
        }
    });
}

struct TitleItemEnv<'a>(&'a horizon_client::item_render::ItemRenderer);

impl horizon_assets::item_model::ItemEnv for TitleItemEnv<'_> {
    fn grass_color(&self, temperature: f64, downfall: f64) -> i32 {
        self.0.grass_color(temperature, downfall)
    }
}

fn draw_title_item(
    batch: &mut HudBatch,
    atlas: &horizon_client::hud::HudAtlas,
    item_renderer: Option<&horizon_client::item_render::ItemRenderer>,
    stack: &ItemStack,
    x: f32,
    y: f32,
) {
    if let Some(renderer) = item_renderer {
        let state = renderer.resolve(
            &horizon_client::item_render::stack_view(stack),
            horizon_assets::item_model::DisplayContext::Gui,
            None,
            None,
            0,
            &TitleItemEnv(renderer),
        );
        if !state.is_empty() {
            horizon_client::item_render::gui::draw(
                &state,
                renderer,
                &atlas.sprites,
                &horizon_client::item_render::NoHost,
                batch,
                x,
                y,
                0.0,
                0.5,
                0.75,
            );
            return;
        }
    }
    let bare = stack.item.trim_start_matches("minecraft:");
    for candidate in [format!("item/{bare}"), format!("block/{bare}"), format!("block/{bare}_top")] {
        if atlas.sprites.contains_key(candidate.as_str()) {
            batch.sprite_scaled(atlas, &candidate, x, y, 16.0, 16.0, u32::MAX);
            return;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn title_host_draw(
    batch: &mut HudBatch,
    d: &native_ui::canvas::HostDraw,
    canvas_scale: f32,
    atlas: &horizon_client::hud::HudAtlas,
    item_renderer: Option<&horizon_client::item_render::ItemRenderer>,
    samples: &HashMap<u64, ItemStack>,
    sample_max: &HashMap<u64, u32>,
    effects: &HashMap<u64, String>,
) {
    let k = canvas_scale / batch.scale;
    let (gx, gy, size) = (d.x * k, d.y * k, d.size * k);
    match d.kind & 0xFF {
        module::HOST_SLOT => {
            let Some(stack) = samples.get(&d.key) else { return };
            let start = batch.vertices.len();
            draw_title_item(batch, atlas, item_renderer, stack, gx, gy);
            let max_damage = sample_max.get(&d.key).copied().unwrap_or(0) as i32;
            let bar = if d.kind & module::SLOT_BAR != 0 {
                horizon_client::hud_overlays::damage_bar(stack.damage as i32, max_damage)
            } else {
                None
            };
            let decorations = horizon_client::hud_overlays::ItemDecorations {
                count: if d.kind & module::SLOT_COUNT != 0 { stack.count } else { 1 },
                count_text: None,
                bar,
                cooldown: 0.0,
            };
            let mut draws = vec![super::hud_layout::GuiDraw::Push(vec![
                super::hud_layout::PoseOp::Translate(gx, gy),
            ])];
            draws.extend(horizon_client::hud_overlays::item_decorations(0, 0, &decorations, &atlas.font));
            draws.push(super::hud_layout::GuiDraw::Pop);
            super::hud_layout::paint(batch, atlas, &draws);
            batch.scale_since(start, gx, gy, size / 16.0, size / 16.0);
        }
        module::HOST_ITEM => {
            let Some(stack) = samples.get(&d.key) else { return };
            let start = batch.vertices.len();
            draw_title_item(batch, atlas, item_renderer, stack, gx, gy);
            batch.scale_since(start, gx, gy, size / 16.0, size / 16.0);
        }
        module::HOST_ITEM_ID => {
            let Some(id) = module::item_for_key(d.key) else { return };
            let stack = ItemStack { item: std::sync::Arc::from(full_id(&id).as_str()), count: 1, damage: 0, extra: None };
            let start = batch.vertices.len();
            draw_title_item(batch, atlas, item_renderer, &stack, gx, gy);
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
            super::hud_layout::paint(batch, atlas, &draws);
        }
        _ => {}
    }
}

/// What the Native title asked of the game, for the host to do (it needs the event loop).
pub(super) fn native_title_take() -> Option<title_view::TitleAct> {
    with_state(|s| s.title_pending.take())
}

/// Does what the title asked: the title's own pages here, the rest (`Options`, `Quit`, joining a server) back
/// for the host's own title actions.
pub(super) fn native_title_apply(title: &mut horizon_client::title::Title, act: title_view::TitleAct) -> Option<horizon_client::title::Action> {
    use horizon_client::title::{Action, Mode};
    use title_view::TitleAct;
    match act {
        TitleAct::Singleplayer => {
            title.enter_singleplayer();
            None
        }
        TitleAct::Multiplayer => {
            // TitleScreen.init: SafetyScreen first, unless it was turned off.
            title.mode = if title.skip_multiplayer_warning {
                Mode::Multiplayer(horizon_client::multiplayer::Screen::list(&title.saves))
            } else {
                Mode::MultiplayerWarning { check: false }
            };
            None
        }
        TitleAct::Realms => {
            title.mode = Mode::Realms(horizon_client::realms::Screen::new());
            None
        }
        TitleAct::Options => Some(Action::Options),
        TitleAct::Quit => Some(Action::Quit),
        TitleAct::Join(address) => Some(Action::Connect { address, lan: false }),
        TitleAct::ServerButton { id, index } => {
            use horizon_client::multiplayer::Screen;
            if let Mode::Multiplayer(Screen::List { selected, .. }) = &mut title.mode
                && index.is_some()
            {
                *selected = index;
            }
            server_click(title, id)
        }
        // The Native dialogs fill in the game's own screens and press their buttons.
        TitleAct::ServerForm { name, address, packs, done } => {
            use horizon_client::multiplayer::Screen;
            if done
                && let Mode::Multiplayer(Screen::Edit { name: n, address: a, packs: p, .. }) = &mut title.mode
            {
                (*n, *a, *p) = (name, address, packs);
            }
            server_click(title, if done { 50 } else { 51 })
        }
        TitleAct::ServerDirect { address, join } => {
            use horizon_client::multiplayer::Screen;
            if join
                && let Mode::Multiplayer(Screen::Direct { address: a }) = &mut title.mode
            {
                *a = address;
            }
            server_click(title, if join { 52 } else { 51 })
        }
        TitleAct::ServerDelete { yes } => server_click(title, if yes { 58 } else { 59 }),
        TitleAct::ServerMove { index, up } => {
            let saves = title.saves.clone();
            if let Mode::Multiplayer(screen) = &mut title.mode {
                if let horizon_client::multiplayer::Screen::List { selected, .. } = screen {
                    *selected = Some(index);
                }
                screen.arrow(up, true, (0.0, 0.0), &saves);
            }
            None
        }
        TitleAct::ServersClassic => {
            with_state(|s| s.servers_classic = true);
            None
        }
        TitleAct::NativeMods | TitleAct::Relay | TitleAct::Classic => None,
    }
}

/// Window events while the Native title is up: it takes the mouse and the keys. True when it took the event.
pub(super) fn native_title_event(event: &WindowEvent, modifiers: winit::keyboard::ModifiersState) -> bool {
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
            let PhysicalKey::Code(code) = key.physical_key else { return true };
            // Fullscreen stays the window's.
            if code == winit::keyboard::KeyCode::F11 {
                return false;
            }
            let Some(glfw) = keys::glfw(code) else { return true };
            let pressed = key.state == ElementState::Pressed;
            let mods = if modifiers.shift_key() { ui::MOD_SHIFT } else { 0 }
                | if modifiers.control_key() { ui::MOD_CONTROL } else { 0 }
                | if modifiers.alt_key() { ui::MOD_ALT } else { 0 }
                | if modifiers.super_key() { ui::MOD_SUPER } else { 0 };
            with_state(|s| {
                let State { ui, menu, editor, relay, title_overlay, events, .. } = s;
                if pressed && glfw == ui::KEY_ESCAPE {
                    let kept = match *title_overlay {
                        1 => menu.escape(ui),
                        2 => relay.escape(ui),
                        3 => editor.escape(ui),
                        _ => true,
                    };
                    if !kept {
                        *title_overlay = 0;
                    }
                    return;
                }
                events.push(Event::Key { code: glfw, mods, pressed });
                if pressed
                    && mods & (ui::MOD_CONTROL | ui::MOD_SUPER) == 0
                    && let Some(text) = &key.text
                {
                    events.extend(text.chars().map(Event::Char));
                }
            });
            true
        }
        _ => false,
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

/// An item id as the game's registry has it: the modules keep ids without `minecraft:` (as the Java mod does),
/// and the game's item renderer finds nothing for a bare `diamond_sword`, so previews drew no items.
fn full_id(id: &str) -> String {
    if id.contains(':') { id.to_owned() } else { format!("minecraft:{id}") }
}

/// Presses one of the game's multiplayer screen buttons (`multiplayer::Screen::click`) and says what that asks of the
/// host: back to the title, or a server to join.
fn server_click(title: &mut horizon_client::title::Title, id: u8) -> Option<horizon_client::title::Action> {
    use horizon_client::multiplayer::Outcome;
    use horizon_client::title::{Action, Mode};
    let saves = title.saves.clone();
    let outcome = match &mut title.mode {
        Mode::Multiplayer(screen) => screen.click(Some(id), 0.0, 0.0, (0.0, 0.0), &saves),
        _ => return None,
    };
    match outcome {
        Outcome::Back => {
            title.mode = Mode::Main;
            None
        }
        Outcome::Connect { address, lan } => Some(Action::Connect { address, lan }),
        Outcome::None => None,
    }
}

thread_local! {
    static RESTORE_NATIVE: std::cell::Cell<([f64;4], bool)> = const { std::cell::Cell::new(([0.0;4],false)) };
}

/// A visible way back, drawn over the classic title in both startup and return-to-title paths.
pub(super) fn native_title_restore_frame(batch: &mut HudBatch, title: &horizon_client::title::Title) {
    let show = matches!(title.mode, horizon_client::title::Mode::Main) && with_state(|s| !s.cfg.custom_title);
    if !show { RESTORE_NATIVE.with(|r| r.set(([0.0;4],false))); return; }
    with_state(|s| {
        let fb = [(batch.width * batch.scale) as u32, (batch.height * batch.scale) as u32];
        let scale = (fb[0] as f32 / 1280.0).min(fb[1] as f32 / 760.0).max(0.75) * s.cfg.scale;
        let c = &mut s.canvas;
        c.begin(fb[0],fb[1],scale);
        let x=c.width()-152.0;
        let hovered=RESTORE_NATIVE.with(|r| { let (_,hovered)=r.get();r.set(([f64::from(x*scale),12.0*f64::from(scale),140.0*f64::from(scale),32.0*f64::from(scale)],hovered));hovered });
        c.round(x,12.0,140.0,32.0,6.0,if hovered {0xFF303036}else{0xE818181C});
        c.outline(x,12.0,140.0,32.0,6.0,1.0,0xFF606068);
        let label=horizon_client::lang::get("native.ui.restore");
        let tw=c.text_width(native_ui::font::BOLD,12.0,&label);
        c.text(native_ui::font::BOLD,12.0,&label,x+(140.0-tw)/2.0,21.0,0xFFFFFFFF);
        c.finish_with(batch,&mut |_batch,_draw| {});
    });
}

pub(super) fn native_title_restore_event(title: &horizon_client::title::Title,event: &WindowEvent) -> bool {
    if !matches!(title.mode,horizon_client::title::Mode::Main) || with_state(|s| s.cfg.custom_title) { return false; }
    match event {
        WindowEvent::CursorMoved{position,..} => {RESTORE_NATIVE.with(|r| {let (rect,_)=r.get();let [x,y,w,h]=rect;r.set((rect,w>0.0&&position.x>=x&&position.x<=x+w&&position.y>=y&&position.y<=y+h));});false}
        WindowEvent::MouseInput{state:ElementState::Pressed,button:MouseButton::Left,..} if RESTORE_NATIVE.with(|r|r.get().1) => {
            with_state(|s| {s.cfg.custom_title=true;s.cfg.save();s.events.clear();s.title_was=false;});
            RESTORE_NATIVE.with(|r|r.set(([0.0;4],false)));true
        }
        WindowEvent::Focused(false) => {RESTORE_NATIVE.with(|r|r.set(([0.0;4],false)));false}
        _=>false,
    }
}
