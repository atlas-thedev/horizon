//! `Modules`: every client module, and `config/native-modules.json` in the
//! game folder (`{id: {enabled, favorite?, x?, y?, settings{...}}}`), the
//! Java mod's file and format.

use super::module::Module;
use serde_json::{Map, Value};
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub struct Modules {
    pub all: Vec<Module>,
    file: Option<PathBuf>,
    dirty: Option<Instant>,
}

/// Changes are written this long after the last one.
const SAVE_AFTER: Duration = Duration::from_millis(800);

impl Modules {
    pub fn new(game_dir: Option<PathBuf>) -> Self {
        let mut all = Vec::new();
        all.extend(super::hud_modules::all());
        all.extend(super::status_modules::all());
        all.extend(super::extra_modules::all());
        all.extend(super::gameplay_modules::all());
        all.extend(super::gameplay_modules::late());
        all.extend(super::overlay_modules::all());
        all.extend(super::discord_module::all());
        all.extend(super::music_modules::all());
        all.extend(super::waypoints_module::all());
        all.extend(super::emotes_module::all(super::uiconfig::UiConfig::load(game_dir.as_deref()).emote_key));
        // The order the Java mod registers them in: it is the order of the menu's grid.
        const ORDER: [&str; 45] = [
            "fps", "cps", "keystrokes", "coords", "direction", "clock", "memory", "ping", "server", "speed", "scoreboard", "bossbar",
            "togglesprint", "zoom", "fullbright", "bgfps", "effects", "armor", "inventory", "crosshair", "hitbox-colours",
            "critical-colours", "watermark", "nether-coords", "moon-phase", "day-night", "totem-counter", "item-counter", "playtime",
            "reach", "combo", "cooldowns", "block-overlay", "low-fire", "damage-tint", "freelook", "no-hurt-cam", "motion-blur",
            "time-changer", "weather-changer", "voice", "voice-overlay", "now-playing", "discord-rpc", "emotes",
        ];
        let rank = |id: &str| ORDER.iter().position(|o| *o == id).unwrap_or(ORDER.len());
        all.sort_by_key(|m| rank(m.state.id));
        let mut modules = Modules {
            all,
            file: game_dir.map(|d| d.join("config").join("native-modules.json")),
            dirty: None,
        };
        modules.load();
        modules
    }

    #[allow(dead_code)]
    pub fn get(&self, id: &str) -> Option<&Module> {
        self.all.iter().find(|m| m.state.id == id)
    }

    #[allow(dead_code)]
    pub fn get_mut(&mut self, id: &str) -> Option<&mut Module> {
        self.all.iter_mut().find(|m| m.state.id == id)
    }

    /// Something changed: written to disk shortly after.
    /// The wheel turned by `dy` in the world: true when an enabled module took it.
    pub fn on_scroll(&mut self, held: &std::collections::HashSet<i32>, dy: f64) -> bool {
        let g = super::module::GameView { keys_down: held.iter().copied().collect(), playing: true, ..Default::default() };
        let mut taken = false;
        for m in &mut self.all {
            if m.state.enabled && m.behaviour.on_scroll(&m.state, &g, dy) {
                taken = true;
            }
        }
        taken
    }

    pub fn enabled(&self, id: &str) -> bool {
        self.all.iter().any(|m| m.state.id == id && m.state.enabled)
    }

    /// True when an enabled module reads the player's health, effects or items.
    pub fn needs_vitals(&self) -> bool {
        self.all.iter().any(|m| m.state.enabled && m.behaviour.needs_vitals())
    }

    pub fn changed(&mut self) {
        if self.dirty.is_none() {
            self.dirty = Some(Instant::now());
        }
    }

    /// Once a frame: saves pending changes after a short quiet period.
    pub fn tick(&mut self) {
        if self.dirty.is_some_and(|at| at.elapsed() > SAVE_AFTER) {
            self.save();
        }
    }

    pub fn reset_positions(&mut self) {
        for m in &mut self.all {
            if let Some(h) = m.state.hud.as_mut() {
                h.reset_position();
            }
        }
        self.changed();
    }

    fn load(&mut self) {
        let Some(file) = &self.file else { return };
        let Ok(text) = std::fs::read_to_string(file) else { return };
        let root: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                log::warn!("Native: could not read {} ({e}), using defaults", file.display());
                return;
            }
        };
        for m in &mut self.all {
            let Some(o) = root.get(m.state.id).filter(|o| o.is_object()) else {
                continue;
            };
            if let Some(on) = o.get("enabled").and_then(Value::as_bool) {
                m.state.enabled = on;
            }
            // Old configurations have no toggle binding.
            m.state.toggle_key.reset();
            if let Some(f) = o.get("favorite").and_then(Value::as_bool) {
                m.state.favorite = f;
            }
            if let Some(h) = m.state.hud.as_mut() {
                if let Some(x) = o.get("x").and_then(Value::as_f64) {
                    h.fx = (x as f32).clamp(0.0, 1.0);
                }
                if let Some(y) = o.get("y").and_then(Value::as_f64) {
                    h.fy = (y as f32).clamp(0.0, 1.0);
                }
            }
            if let Some(s) = o.get("settings").and_then(Value::as_object) {
                for st in m.state.all_settings_mut() {
                    if let Some(v) = s.get(st.id) {
                        st.load(v);
                    }
                }
            }
        }
    }

    pub fn save(&mut self) {
        self.dirty = None;
        let Some(file) = &self.file else { return };
        let mut root = Map::new();
        for m in &self.all {
            let mut o = Map::new();
            o.insert("enabled".into(), Value::Bool(m.state.enabled));
            if m.state.favorite {
                o.insert("favorite".into(), Value::Bool(true));
            }
            if let Some(h) = &m.state.hud {
                o.insert("x".into(), serde_json::json!(h.fx));
                o.insert("y".into(), serde_json::json!(h.fy));
            }
            let settings: Map<String, Value> = m.state.all_settings().map(|s| (s.id.to_owned(), s.save())).collect();
            o.insert("settings".into(), Value::Object(settings));
            root.insert(m.state.id.to_owned(), Value::Object(o));
        }
        let text = serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_default();
        let write = || -> std::io::Result<()> {
            std::fs::create_dir_all(file.parent().unwrap_or(file))?;
            let temp = file.with_extension("json.tmp");
            std::fs::write(&temp, text.as_bytes())?;
            std::fs::rename(&temp, file)
        };
        if let Err(e) = write() {
            log::warn!("Native: could not save {} ({e})", file.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_survive_a_save_and_a_load() {
        let dir = std::env::temp_dir().join(format!("native-modules-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut m = Modules::new(Some(dir.clone()));
        // The Java mod's defaults: FPS, Keystrokes, Scoreboard, Boss Bar, Lag Free, the watermark, Now Playing and Discord RPC on.
        let on: Vec<_> = m.all.iter().filter(|m| m.state.enabled).map(|m| m.state.id).collect();
        assert_eq!(on, vec!["fps", "keystrokes", "scoreboard", "bossbar", "bgfps", "watermark", "now-playing", "discord-rpc", "emotes"]);
        let fps = m.get_mut("fps").unwrap();
        fps.state.enabled = false;
        fps.state.hud.as_mut().unwrap().fx = 0.75;
        m.save();
        let text = std::fs::read_to_string(dir.join("config/native-modules.json")).unwrap();
        assert!(text.contains("\"style\": \"Card\""));
        let back = Modules::new(Some(dir.clone()));
        let fps = back.get("fps").unwrap();
        assert!(!fps.state.enabled);
        assert_eq!(fps.state.hud.as_ref().unwrap().fx, 0.75);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
