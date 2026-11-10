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

# A level-loading/connection screen takes precedence over a pause left open by
# focus loss or a transition. Do not pause the integrated server while its
# terrain is still loading, and do not auto-open pause over that screen.
POINTER = CLIENT / 'game/pointer_capture.rs'
patch(POINTER,
      '        self.has_singleplayer_server()\n            && !self.is_published()\n',
      '        self.has_singleplayer_server()\n            && !self.remote_screen_up()\n            && !self.is_published()\n')
patch(POINTER,
      '            && self.control.is_none()\n            && self.screen == Screen::Playing\n            && self.player.is_some()\n',
      '            && self.control.is_none()\n            && self.screen == Screen::Playing\n            && !self.remote_screen_up()\n            && self.player.is_some()\n')

# The shared Escape path is used by keyboard and controller. Do not open a
# pause screen on top of loading; an already-open pause can still be closed.
patch(GAME,
      '                Screen::Playing => {\n                    if self.studio_replay.is_some() && self.captured {\n',
      '                Screen::Playing => {\n                    if self.remote_screen_up() {\n                        return Ok(());\n                    }\n                    if self.studio_replay.is_some() && self.captured {\n')

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
      '                let logo_width = batch.width.ceil();\n'
      '                super::native_client::native_title_background(batch);\n'
      '                if matches!(title.mode, horizon_client::title::Mode::Main) {\n'
      '                    horizon_client::title::draw_logo(&gui.atlas, batch, logo_width);\n'
      '                }\n'
      '                super::native_client::native_title_frame(batch, title, &who, &gui.atlas, items, &mut self.menu.clipboard);\n'
      '            } else {\n'
      '                title.draw_with_items(\n'
      '                    &gui.atlas,\n'
      '                    items,\n'
      '                    batch,\n'
      '                    self.menu.cursor,\n'
      '                    self.settings.file.bool("hideSplashTexts"),\n'
      '                    self.settings.file.bool("hideServerAddress"),\n'
      '                );\n'
      '            }\n'
      '            super::native_client::native_title_restore_frame(batch, title);\n')
MENU_FOCUS = "        // The real mouse takes the pointer back from the keyboard's focus.\n"
patch(MENU, MENU_FOCUS,
      '        if let Some(act) = super::native_client::native_title_take()\n'
      '            && let Some(title) = self.menu.title.as_mut()\n'
      '            && let Some(action) = super::native_client::native_title_apply(title, act)\n'
      '        {\n'
      '            self.menu_action(event_loop, action);\n'
      '        }\n'
      '        if !self.menu.options_open && self.menu.win.is_none() && self.menu.pending.is_none()\n'
      '            && self.menu.title.as_ref().is_some_and(|title| super::native_client::native_title_restore_event(title, &event)) { return Ok(()); }\n'
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
      '                    let logo_width = batch.width.ceil();\n'
      '                    native_client::native_title_background(&mut batch);\n'
      '                    if matches!(title.mode, horizon_client::title::Mode::Main) {\n'
      '                        horizon_client::title::draw_logo(&self.gui.atlas, &mut batch, logo_width);\n'
      '                    }\n'
      '                    native_client::native_title_frame(&mut batch, title, &who, &self.gui.atlas, Some(&self.pack.item_renderer), &mut self.clipboard);\n'
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
      '                native_client::native_title_restore_frame(&mut batch, title);\n'
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
      '        if self.screen != Screen::Options && self.title.as_ref().is_some_and(|title| native_client::native_title_restore_event(title, &event)) { return; }\n'
      '        // Options opened from the title is a screen over it: its input goes to the options, as vanilla.\n'
      '        if self.screen != Screen::Options\n'
      '            && self.title.as_ref().is_some_and(native_client::native_title_active)\n'
      '            && native_client::native_title_event(&event, self.modifiers)\n'
      '        {\n'
      '            return;\n'
      '        }\n'
      '        if self.native_window_event(&event) {\n'
      '            return;\n'
      '        }\n')

# The Native title leaves the game's panorama and Minecraft logo as they are: the logo is drawn by the game.
patch(CLIENT / 'title.rs', '\nfn draw_logo(atlas: &HudAtlas, batch: &mut HudBatch, w: f32) {\n',
      '\npub fn draw_logo(atlas: &HudAtlas, batch: &mut HudBatch, w: f32) {\n')
# The Native server list pings the servers Native features the way the game pings its own list.
patch(CLIENT / 'multiplayer.rs', '\nfn ping_all(servers: &[ServerEntry], pings: &Pings, icons: &Icons, file: &Path) {\n',
      '\npub fn ping_all(servers: &[ServerEntry], pings: &Pings, icons: &Icons, file: &Path) {\n')
patch(CLIENT / 'multiplayer.rs', '\ntype Pings = Arc<Mutex<HashMap<String, Ping>>>;\n',
      '\npub type Pings = Arc<Mutex<HashMap<String, Ping>>>;\n')
patch(CLIENT / 'multiplayer.rs', '\ntype Icons = Arc<Mutex<HashMap<String, Arc<Vec<u8>>>>>;\n',
      '\npub type Icons = Arc<Mutex<HashMap<String, Arc<Vec<u8>>>>>;\n')

# The Windows executable says it is Native Client (Task Manager, the firewall prompt): its version strings, in the
# resource upstream's build.rs already compiles with the icon (also on the Linux cross build) (native_exe.rs).
shutil.copyfile(native / 'packaging/native/native_exe.rs', CLIENT / '../native_exe.rs')
patch(CLIENT / '../build.rs', '        resource.set_icon(&resource_icon);\n',
      '        resource.set_icon(&resource_icon);\n'
      '        for (k, v) in NATIVE_EXE_STRINGS {\n'
      '            resource.set(k, v);\n'
      '        }\n')
build_rs = (CLIENT / '../build.rs')
if 'include!("native_exe.rs");' not in build_rs.read_text(encoding='utf-8'):
    with open(build_rs, 'a', encoding='utf-8') as build:
        build.write('\ninclude!("native_exe.rs");\n')

print('native_client: Native modules applied')

# New Native-only labels remain translation keys.
import json
lang = root / 'crates/horizon-client/assets/horizon/lang/en_us.json'
labels = json.loads(lang.read_text(encoding='utf-8'))
labels['native.ui.restore'] = 'Use Native UI'
lang.write_text(json.dumps(labels, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
