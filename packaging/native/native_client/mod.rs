//! Native Client in game: the Rust port of native-mod's UI runtime
//! (`UiRuntime`, `Game`, `HudOverlay`) and its modules. It draws on the Native
//! UI canvas (`horizon_client::native_ui`) once a frame, from `Game::hud`, and
//! sees every window event first, from `handle_window_event`.
//!
//! `tools/native/native_client.py` copies this folder to
//! `crates/horizon-client/src/game/native_client/`. As a child of `game` it
//! reads the game's own state directly; its own state lives on the game's
//! thread.

mod hud_modules;
mod module;
mod modules;
mod setting;

use super::{Game, Screen};
use horizon_client::hud::HudBatch;
use horizon_client::native_ui::{self, Canvas};
use module::{GameView, Hud};
use modules::Modules;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use winit::event::{ElementState, MouseButton, WindowEvent};

/// `UiConfig.scale`: the player's UI size on top of the window's.
const CONFIG_SCALE: f32 = 1.0;

struct State {
    canvas: Canvas,
    modules: Modules,
    /// Click times in the last second (`Game.click`).
    clicks: [VecDeque<Instant>; 2],
    /// The last position and when, for the speed read-out.
    moved: Option<([f64; 3], Instant)>,
    speed: f32,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|cell| {
        let mut slot = cell.borrow_mut();
        let state = slot.get_or_insert_with(|| State {
            canvas: native_ui::canvas(),
            modules: Modules::new(game_dir()),
            clicks: [VecDeque::new(), VecDeque::new()],
            moved: None,
            speed: 0.0,
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
    fn native_view(&self, state: &mut State) -> GameView {
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
        GameView {
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
                && matches!(self.screen, Screen::Playing | Screen::Chat)
                && !self.ui.hide_gui
                && !self.debug,
        }
    }

    /// Once a frame, over the game's HUD and screens: the Native UI.
    pub(super) fn native_ui_frame(&mut self, batch: &mut HudBatch) {
        with_state(|state| {
            let g = self.native_view(state);
            let fb = [(batch.width * batch.scale) as u32, (batch.height * batch.scale) as u32];
            let scale = (fb[0] as f32 / 1280.0).min(fb[1] as f32 / 760.0).max(0.75) * CONFIG_SCALE;
            for m in &mut state.modules.all {
                if m.state.enabled {
                    m.behaviour.frame(&m.state, &g);
                }
            }
            state.modules.tick();
            if !g.hud_allowed {
                return;
            }
            let c = &mut state.canvas;
            c.begin(fb[0], fb[1], scale);
            let (w, h) = (c.width(), c.height());
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
            c.finish(batch);
        });
    }

    /// Every window event, before the game handles it. True when the Native
    /// UI took it (the game then never sees it).
    pub(super) fn native_window_event(&mut self, event: &WindowEvent) -> bool {
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
