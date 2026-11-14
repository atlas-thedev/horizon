#!/usr/bin/env python3
"""Native Client session and friend presence for a Horizon source tree (MIT licensed, see NATIVE.md).

Usage: native_api.py <checkout> <native-checkout>

Adds crates/horizon-client/src/native_api.rs (the launcher's game ticket is checked against
/v1/mod/me and the player's activity is reported to /v1/mod/presence) and hooks it into the
existing Discord presence module, which already knows when the player is in a menu or a world. Since upstream
moved Discord presence into the horizon-discord crate (2026-11-09), the session is a module of its own
(packaging/native/native_presence.rs, installed under the old name) on the same platform hooks.

Every change names the exact text it replaces and the build FAILS when upstream changed it,
instead of shipping a client that silently has no Native account. Safe to run again.
"""
import pathlib
import shutil
import sys
import os as _os, sys as _sys
_sys.path.insert(0, _os.path.dirname(_os.path.abspath(__file__)))  # also when run through runpy (the tests)
from native_moved import relocate  # noqa: E402

root = pathlib.Path(sys.argv[1]).resolve()
native = pathlib.Path(sys.argv[2]).resolve()
CLIENT = root / 'crates/horizon-client/src'


def patch(path, old, new):
    """Replace `old` (exactly once) with `new`; fine if it was done already."""
    path = relocate(path, old, new)
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
SHIM = native / 'packaging/native/native_presence.rs'
if presence.is_file() and 'pub use horizon_hooks::Play;' not in presence.read_text(encoding='utf-8'):
    # The engine's own Discord presence (Nightlies up to 2026-11-08): the session rides on its start/set/stop.
    patch(presence, 'pub fn start(enabled: bool, automated: bool) {',
          'pub fn start(enabled: bool, automated: bool) {\n    if !automated {\n        native_start();\n    }')
    patch(presence, 'pub fn set(status: Status) {',
          'pub fn set(status: Status) {\n    native_set(status);')
    patch(presence, 'pub fn stop() {',
          'pub fn stop() {\n    native_stop();')
    include(presence, 'native_api.rs')
elif (root / 'crates/horizon-discord').is_dir():
    # Discord presence is the horizon-discord crate now, on the platform hooks (upstream 01343707a): the session gets
    # a module of its own under the old name, on the same hooks, registered by the engine with its own parts.
    shutil.copyfile(SHIM, presence)
    patch(CLIENT / 'client_lib.rs', 'pub mod dialog_screen;\n',
          'pub mod dialog_screen;\n'
          '/// Native: the Native session and friend presence (the name it had inside the engine\'s Discord presence).\n'
          'pub mod discord_presence;\n')
    patch(CLIENT / 'entry.rs', '    game::player_hooks::register();\n',
          '    game::player_hooks::register();\n'
          '    if let Err(e) = horizon_hooks::register(&horizon_client::discord_presence::NATIVE_HOOKS) {\n'
          '        log::warn!("Native: the session did not register: {e}");\n'
          '    }\n')
else:
    raise SystemExit('native_api: no discord_presence.rs and no horizon-discord crate; upstream moved Discord again. '
                     'Update tools/native/native_api.py.')

print('native_api: Native session and presence applied')
