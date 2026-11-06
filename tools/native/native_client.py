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

GAME = CLIENT / 'game.rs'
patch(GAME, 'mod cosmetics;\n', 'mod cosmetics;\nmod native_client;\n')
# Over the HUD, the screens and the toasts; under the debug overlay, as in the Java mod.
patch(GAME, '        self.draw_debug_overlay(&mut batch, size);\n',
      '        self.native_ui_frame(&mut batch);\n        self.draw_debug_overlay(&mut batch, size);\n')
# Window events reach the Native UI first; what it takes, the game never sees.
TRACE = '        // Who caused it, before the event is handled: the trace wants the\n'
patch(GAME, TRACE, '        if self.native_window_event(&event) {\n            return;\n        }\n' + TRACE)

print('native_client: Native modules applied')
