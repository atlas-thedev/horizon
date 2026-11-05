# Native Client edition of Horizon

This fork builds the experimental Rust engine of the [Native Client](https://playnative.net) launcher.
It is based on [Horizon](https://github.com/horizon-rust/horizon), which is released under the MIT
License. Horizon's `LICENSE` is kept unchanged in this repository, as the license requires.

> Copyright (c) 2026 Ludovic Marie and the Horizon contributors

What this fork changes (and nothing else):

- the player-facing name "Horizon" becomes "Native" (menus, boot screen, window title, logs),
  applied by `tools/native/rebrand.py`;
- the Windows executable icon is the Native Client icon (`packaging/native/icon.ico.b64`);
- `.github/workflows/native-build.yml` builds the newest upstream Nightly with those changes and publishes
  the client as the release `native-<date>-<commit>`.

Crate names, identifiers, the release-signing code and the game's own assets are upstream's. Minecraft is
a trademark of Mojang AB; this is not an official Minecraft product.
