//! `UiConfig`: `config/native-ui.json`, the Java mod's file and field names.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct UiConfig {
    pub custom_title: bool,
    /// The Native multiplayer screen instead of vanilla's.
    pub custom_servers: bool,
    pub custom_pause: bool,
    pub notifications: bool,
    pub discord_rpc: Option<bool>,
    pub discord_hide_server: Option<bool>,
    pub discord_hidden_text: Option<String>,
    /// Show friends in Native Relay what server or world you are playing.
    pub share_activity: bool,
    /// GLFW key that opens the chat in game (default Y).
    pub relay_key: i32,
    pub emote_key: i32,
    pub scale: f32,
    /// GLFW key that opens the Native menu in game (default Right Shift).
    pub menu_key: i32,
    pub music_volume: f32,
    pub music_shuffle: bool,
    /// 0 off, 1 all, 2 one.
    pub music_repeat: i32,
    pub music_duck: bool,
    pub music_play_key: i32,
    pub music_next_key: i32,
    pub music_prev_key: i32,
    #[serde(skip)]
    file: Option<PathBuf>,
}

impl Default for UiConfig {
    fn default() -> Self {
        UiConfig {
            custom_title: true,
            custom_servers: true,
            custom_pause: true,
            notifications: true,
            discord_rpc: None,
            discord_hide_server: None,
            discord_hidden_text: None,
            share_activity: true,
            relay_key: 89,
            emote_key: 66,
            scale: 1.0,
            menu_key: 344,
            music_volume: 0.7,
            music_shuffle: false,
            music_repeat: 1,
            music_duck: true,
            music_play_key: -1,
            music_next_key: -1,
            music_prev_key: -1,
            file: None,
        }
    }
}

impl UiConfig {
    pub fn load(game_dir: Option<&Path>) -> Self {
        let file = game_dir.map(|d| d.join("config").join("native-ui.json"));
        let mut cfg: UiConfig = file
            .as_ref()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        if !(0.5..=2.5).contains(&cfg.scale) {
            cfg.scale = 1.0;
        }
        if !(0.0..=1.0).contains(&cfg.music_volume) {
            cfg.music_volume = 0.7;
        }
        if !(0..=2).contains(&cfg.music_repeat) {
            cfg.music_repeat = 1;
        }
        cfg.file = file;
        cfg
    }

    pub fn save(&self) {
        let Some(file) = &self.file else { return };
        let Ok(text) = serde_json::to_string_pretty(self) else { return };
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = file.with_extension("json.tmp");
        if std::fs::write(&tmp, text).is_ok() && std::fs::rename(&tmp, file).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_reads_the_java_mods_file() {
        let cfg: UiConfig = serde_json::from_str(r#"{"menuKey": 345, "scale": 1.25, "customTitle": false, "other": 1}"#).unwrap();
        assert_eq!(cfg.menu_key, 345);
        assert_eq!(cfg.relay_key, 89);
        assert!(!cfg.custom_title);
        let text = serde_json::to_string(&cfg).unwrap();
        assert!(text.contains("\"menuKey\":345") && text.contains("\"shareActivity\":true"));
    }
}
