# Native Client edition of Horizon

This fork builds the experimental Rust engine of the [Native Client](https://playnative.net) launcher.
It is based on [Horizon](https://github.com/horizon-rust/horizon), which is released under the MIT
License. Horizon's `LICENSE` is kept unchanged in this repository, as the license requires.

> Copyright (c) 2026 Ludovic Marie and the Horizon contributors

What this fork changes (and nothing else):

- the player-facing name "Horizon" becomes "Native" (menus, boot screen, window title, logs),
  applied by `tools/native/rebrand.py`;
- the Windows executable icon and the game window icon are Minecraft's own Java Edition icon (26.3), fetched
  from Mojang's asset servers and checked against pinned SHA-1 hashes by `tools/native/make_vanilla_icon.py`
  (the launch screen's mark is still the Native one, `packaging/native/icon64.png.b64`);
- the boot screen is the launcher's own launch screen (black, the Native mark, "Native Client" and the
  sweeping bar, no console wall), drawn both by the early Windows paint and by the game's renderer, and the
  window opens at Minecraft's 854 x 480 instead of 1280 x 720, applied by `tools/native/boot_screen.py`
  with `packaging/native/boot_paint.rs` and `packaging/native/native_boot.rs`. The launcher still passes
  the size, fullscreen and reduced-motion choices of its own settings. Each change names the exact upstream
  text it replaces, and the build fails when upstream changed it;
- every Native player's skin and cape shows in game, as with the Java Native Client mod: the skin directory
  (`/v1/skins/directory`, then the `/v1/skins/stream` events) is ported to Rust in
  `packaging/native/native_skins.rs` and fed into the game's own skin lookup by `tools/native/native_skins.py`.
  Same identity rule as the mod: premium UUIDs only take a look linked to exactly that account, offline players
  match by name. Animated capes play frame by frame in the cape's skin slot;
- the Native logo (or the Native+ mark) in front of Native players' names on name tags and in the tab list, from
  the mod's own logo bitmaps (`packaging/native/native_marks.rs`, `tools/native/native_marks.py`);
- Native 3D cosmetics: the mod's NCM models become the game's own cosmetic items and draw beside Horizon's
  (`packaging/native/native_cosmetics.rs`, `tools/native/native_cosmetics.py`);
- the Native Client interface, ported from the Java mod: its drawing layer (`native_ui`), the HUD modules
  (FPS, CPS, Keystrokes, Coordinates, status effects, armour, crosshair, Zoom, Freelook, Time and Weather
  Changer, Scoreboard, Boss Bar, Now Playing, Discord RPC and the rest), the HUD editor, the Native menu
  (mods, cosmetics wardrobe, settings, screenshots, music), the Relay chat (friends, groups, GIFs,
  pictures, reactions) with message pop-ups, the pause menu and a music player
  (`packaging/native/native_client`, `native_ui`, `tools/native/native_ui.py`, `native_tweaks.py` and
  `native_client.py`). Each engine hook (`native_ui::tweaks`) is one line next to the code it overrides, and is
  off until a module asks for it. Not ported yet: the Native title screen and server list, and voice chat;
- `.github/workflows/native-build.yml` builds the newest upstream Nightly with those changes and publishes
  the client as the release `native-<date>-<commit>`; `.github/workflows/native-check.yml` compiles the same
  changes on every pull request.

Crate names, identifiers, the release-signing code and the game's own assets are upstream's. Minecraft is
a trademark of Mojang AB; this is not an official Minecraft product.
