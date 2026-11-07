//! `DiscordRpcModule`: the switch and the words for the game's Discord Rich
//! Presence. The presence itself is the game's own (it never shows a server
//! address); this module turns it on and off and lets the player choose what
//! is shown in place of "Multiplayer".

use super::module::*;
use super::setting::Setting;
use crate::native_ui::{theme, tweaks};

pub fn all() -> Vec<Module> {
    vec![Module::new(
        "discord-rpc",
        "Discord RPC",
        "Share your Minecraft status on Discord with optional server address masking.",
        SETTINGS,
        theme::I_GAMEPAD,
        true,
        vec![
            Setting::bool("hideserver", "Hide server address", false),
            Setting::text("customtext", "Custom phrase", "", "Playing Multiplayer", 48),
        ],
        None,
        Box::new(DiscordRpc),
    )]
}

/// One short line: no control characters, single spaces, at most 48 characters.
pub fn clean_phrase(value: &str) -> String {
    let spaced: String = value.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let one = spaced.split_whitespace().collect::<Vec<_>>().join(" ");
    one.chars().take(48).collect::<String>().trim().to_owned()
}

struct DiscordRpc;

impl Behaviour for DiscordRpc {
    fn frame(&mut self, m: &ModuleState, _g: &GameView) {
        let phrase = m.setting("customtext").map(|s| clean_phrase(s.as_text())).unwrap_or_default();
        tweaks::set_discord_phrase(Some(phrase).filter(|p| !p.is_empty()));
    }

    fn toggled(&mut self, on: bool) {
        if !on {
            tweaks::set_discord_phrase(None);
        }
    }

    fn has_preview(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_phrase_is_one_short_clean_line() {
        assert_eq!(clean_phrase("  Playing \n  with\tfriends "), "Playing with friends");
        assert_eq!(clean_phrase(&"x".repeat(80)).chars().count(), 48);
        assert_eq!(clean_phrase("\n\n"), "");
    }

    #[test]
    fn it_is_on_by_default_in_the_settings_page() {
        let m = all().remove(0);
        assert!(m.state.enabled);
        assert_eq!(m.state.category, SETTINGS);
    }
}
