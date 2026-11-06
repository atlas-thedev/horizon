#!/usr/bin/env python3
"""Native Client skins and capes for a Horizon source tree (MIT licensed, see NATIVE.md).

Usage: native_skins.py <checkout> <native-checkout>

Adds crates/horizon-client/src/native_skins.rs (the Native skin directory, kept current from
/v1/skins/directory and /v1/skins/stream, as the Java mod does) and calls it from
update_remote_skins, where every player's skin and cape is chosen. native_api.py must run first:
the module asks it which Native account is signed in.

Every change names the exact text it replaces and the build FAILS when upstream changed it,
instead of shipping a client that silently shows no Native skins. Safe to run again.
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
        raise SystemExit(f'native_skins: expected one {old[:70]!r} in {path.relative_to(root)}, '
                         f'found {text.count(old)}. Upstream changed; update tools/native/native_skins.py.')
    path.write_text(text.replace(old, new), encoding='utf-8')


api = CLIENT / 'native_api.rs'
if not api.exists() or 'fn native_account()' not in api.read_text(encoding='utf-8'):
    raise SystemExit('native_skins: run native_api.py first (native_skins.rs needs native_account).')

shutil.copyfile(native / 'packaging/native/native_skins.rs', CLIENT / 'native_skins.rs')

patch(CLIENT / 'client_lib.rs', 'pub mod discord_presence;\n',
      'pub mod discord_presence;\npub mod native_skins;\n')

ANCHOR = '        let mut resolved: Vec<(u128, String)> = Vec::new();\n'
patch(CLIENT / 'game/remote.rs', ANCHOR,
      '        // Native Client: every Native player\'s skin and cape (native_skins.rs).\n'
      '        if let Some(r) = self.remote.as_ref() {\n'
      '            let mut players: Vec<(u128, String)> =\n'
      '                r.tab_players.values().map(|p| (p.uuid, p.name.clone())).collect();\n'
      '            if !r.profile_name.is_empty() && !players.iter().any(|(u, _)| *u == local) {\n'
      '                players.push((local, r.profile_name.clone()));\n'
      '            }\n'
      '            horizon_client::native_skins::apply(&mut entries, players);\n'
      '        }\n' + ANCHOR)

print('native_skins: Native skins and capes applied')
