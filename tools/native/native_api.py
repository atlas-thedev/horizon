#!/usr/bin/env python3
"""Native Client session and friend presence for a Horizon source tree (MIT licensed, see NATIVE.md).

Usage: native_api.py <checkout> <native-checkout>

Adds crates/horizon-client/src/native_api.rs (the launcher's game ticket is checked against
/v1/mod/me and the player's activity is reported to /v1/mod/presence) and hooks it into the
existing Discord presence module, which already knows when the player is in a menu or a world.

Every change names the exact text it replaces and the build FAILS when upstream changed it,
instead of shipping a client that silently has no Native account. Safe to run again.
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
        raise SystemExit(f'native_api: expected one {old[:70]!r} in {path.relative_to(root)}, '
                         f'found {text.count(old)}. Upstream changed; update tools/native/native_api.py.')
    path.write_text(text.replace(old, new), encoding='utf-8')


def include(path, name):
    text = path.read_text(encoding='utf-8')
    if f'include!("{name}");' not in text:
        path.write_text(text.rstrip('\n') + f'\n\ninclude!("{name}");\n', encoding='utf-8')


shutil.copyfile(native / 'packaging/native/native_api.rs', CLIENT / 'native_api.rs')

presence = CLIENT / 'discord_presence.rs'
patch(presence, 'pub fn start(enabled: bool, automated: bool) {',
      'pub fn start(enabled: bool, automated: bool) {\n    if !automated {\n        native_start();\n    }')
patch(presence, 'pub fn set(status: Status) {',
      'pub fn set(status: Status) {\n    native_set(status);')
patch(presence, 'pub fn stop() {',
      'pub fn stop() {\n    native_stop();')
include(presence, 'native_api.rs')

print('native_api: Native session and presence applied')
