#!/usr/bin/env python3
"""Native Client modules in game for a Horizon source tree (MIT licensed, see NATIVE.md).

Usage: native_client.py <checkout> <native-checkout>

Adds crates/horizon-client/src/game/native_client/ (the port of native-mod's UI runtime and its
modules: FPS, CPS, Keystrokes, Coordinates, Direction, Clock, Memory, Ping, Server IP, Speed, the
Native watermark, saved to config/native-modules.json like the Java mod), draws it once a frame
from Game::hud and lets it see window events first. native_ui.py must run first.

Every change names the exact text it replaces and the build FAILS when upstream changed it,
instead of shipping a client that silently has no Native modules. Safe to run again.
"""
import pathlib
import shutil
import sys

root = pathlib.Path(sys.argv[1]).resolve()
native = pathlib.Path(sys.argv[2]).resolve()
CLIENT = root / 'crates/horizon-client/src'


def patch(path, old, new):
    """Replace `old` (exactly once) with `new`; fine if it was done already."""
    text = path.read_text(encoding='utf-8')
    if new in text:
        return
    if text.count(old) != 1:
        raise SystemExit(f'native_client: expected one {old[:70]!r} in {path.relative_to(root)}, '
                         f'found {text.count(old)}. Upstream changed; update tools/native/native_client.py.')
    path.write_text(text.replace(old, new), encoding='utf-8')


if not (root / 'crates/horizon-client-render/src/native_ui').is_dir():
    raise SystemExit('native_client: run native_ui.py first.')

target = CLIENT / 'game/native_client'
if target.exists():
    shutil.rmtree(target)
shutil.copytree(native / 'packaging/native/native_client', target)

# Native Music decodes songs with symphonia and plays them through cpal. The lock file learns the new
# packages when the build resolves them, so the builds run without --locked.
patch(root / 'crates/horizon-client/Cargo.toml',
      'log = "0.4"\n',
      'log = "0.4"\n'
      '# Native Music: the songs\n'
      'symphonia = { version = "0.5", default-features = false, features = ["mp3", "ogg", "vorbis", "wav", "pcm"] }\n'
      'cpal = "0.16"\n')

GAME = CLIENT / 'game.rs'
patch(GAME, 'mod cosmetics;\n', 'mod cosmetics;\nmod native_client;\n')
# Over the HUD, the screens and the toasts; under the debug overlay, as in the Java mod.
patch(GAME, '        self.draw_debug_overlay(&mut batch, size);\n',
      '        self.native_ui_frame(&mut batch);\n        self.draw_debug_overlay(&mut batch, size);\n')
# Window events reach the Native UI first; what it takes, the game never sees.
TRACE = '        // Who caused it, before the event is handled: the trace wants the\n'
patch(GAME, TRACE, '        if self.native_window_event(&event) {\n            return;\n        }\n' + TRACE)

# The Native title screen: its page instead of the title's own (the world lists, the server list and the options
# stay the game's), on the boot menu and on the title a world returns to. What it asks for goes through the
# game's own title actions.
MENU = CLIENT / 'game/startup_menu.rs'
patch(MENU,
      '            title.draw_with_items(\n'
      '                &gui.atlas,\n'
      '                items,\n'
      '                batch,\n'
      '                self.menu.cursor,\n'
      '                self.settings.file.bool("hideSplashTexts"),\n'
      '                self.settings.file.bool("hideServerAddress"),\n'
      '            );\n',
      '            if super::native_client::native_title_active(title) {\n'
      '                let who = self.options.as_ref().map_or_else(|| "Player".into(), |o| o.player_name());\n'
      '                super::native_client::native_title_frame(batch, title.allows_multiplayer, &who);\n'
      '            } else {\n'
      '                title.draw_with_items(\n'
      '                    &gui.atlas,\n'
      '                    items,\n'
      '                    batch,\n'
      '                    self.menu.cursor,\n'
      '                    self.settings.file.bool("hideSplashTexts"),\n'
      '                    self.settings.file.bool("hideServerAddress"),\n'
      '                );\n'
      '            }\n')
MENU_FOCUS = "        // The real mouse takes the pointer back from the keyboard's focus.\n"
patch(MENU, MENU_FOCUS,
      '        if let Some(act) = super::native_client::native_title_take()\n'
      '            && let Some(title) = self.menu.title.as_mut()\n'
      '            && let Some(action) = super::native_client::native_title_apply(title, act)\n'
      '        {\n'
      '            self.menu_action(event_loop, action);\n'
      '        }\n'
      '        if !self.menu.options_open\n'
      '            && self.menu.win.is_none()\n'
      '            && self.menu.pending.is_none()\n'
      '            && self.menu.title.as_ref().is_some_and(super::native_client::native_title_active)\n'
      '            && super::native_client::native_title_event(&event, self.menu.modifiers)\n'
      '        {\n'
      '            return Ok(());\n'
      '        }\n' + MENU_FOCUS)
patch(GAME,
      '            if let Some(title) = &self.title {\n'
      '                title.draw_with_items(\n'
      '                    &self.gui.atlas,\n'
      '                    Some(&self.pack.item_renderer),\n'
      '                    &mut batch,\n'
      '                    self.cursor,\n'
      '                    self.settings.file.bool("hideSplashTexts"),\n'
      '                    self.settings.file.bool("hideServerAddress"),\n'
      '                );\n'
      '            }\n',
      '            if let Some(title) = &self.title {\n'
      '                if native_client::native_title_active(title) {\n'
      '                    let who = self.options.player_name();\n'
      '                    native_client::native_title_frame(&mut batch, title.allows_multiplayer, &who);\n'
      '                } else {\n'
      '                    title.draw_with_items(\n'
      '                        &self.gui.atlas,\n'
      '                        Some(&self.pack.item_renderer),\n'
      '                        &mut batch,\n'
      '                        self.cursor,\n'
      '                        self.settings.file.bool("hideSplashTexts"),\n'
      '                        self.settings.file.bool("hideServerAddress"),\n'
      '                    );\n'
      '                }\n'
      '            }\n')
patch(GAME,
      '        if self.native_window_event(&event) {\n'
      '            return;\n'
      '        }\n',
      '        if let Some(act) = native_client::native_title_take()\n'
      '            && let Some(title) = self.title.as_mut()\n'
      '            && let Some(action) = native_client::native_title_apply(title, act)\n'
      '        {\n'
      '            let _ = self.title_action(event_loop, action);\n'
      '        }\n'
      '        if self.title.as_ref().is_some_and(native_client::native_title_active)\n'
      '            && native_client::native_title_event(&event, self.modifiers)\n'
      '        {\n'
      '            return;\n'
      '        }\n'
      '        if self.native_window_event(&event) {\n'
      '            return;\n'
      '        }\n')

print('native_client: Native modules applied')
