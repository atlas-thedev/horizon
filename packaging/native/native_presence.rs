//! Native Client: the Native session and friend presence (`native_api.rs`), for a Horizon whose Discord presence
//! left the engine for the `horizon-discord` crate (upstream, 2026-11-09). `tools/native/native_api.py` installs this
//! file as `discord_presence.rs`: the Native files call the session by that name (`discord_presence::native_account`,
//! `discord_presence::native_emote_auth`), as when it lived inside the engine's Discord presence. It hears the game
//! through the same platform hooks the Discord crate does; the engine's `entry` registers [`NATIVE_HOOKS`].
use horizon_hooks::{Activity, Client, Hooks};

/// How the player is in a world (the hooks' own).
pub use horizon_hooks::Play;

/// What the player does, as the Native presence reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Nothing to show: the presence goes back to plain "online".
    Off,
    Menus,
    World { play: Play },
}

impl From<Activity> for Status {
    fn from(now: Activity) -> Self {
        match now {
            Activity::Menus => Status::Menus,
            Activity::World { play, .. } => Status::World { play },
            // Hidden, and anything a later game says that this file doesn't know yet.
            _ => Status::Off,
        }
    }
}

/// The Native session's hooks, for `horizon_hooks::register`.
pub static NATIVE_HOOKS: NativeHooks = NativeHooks;

pub struct NativeHooks;

impl Hooks for NativeHooks {
    /// The session starts with the game's window, whether or not Discord shows anything; never for an automated run.
    fn client_started(&self, client: &Client) {
        if !client.automated {
            native_start();
        }
    }

    fn activity(&self, now: Activity) {
        native_set(now.into());
    }

    /// On quit: the friend presence is cleared.
    fn shutdown(&self) {
        native_stop();
    }
}

include!("native_api.rs");
