#!/usr/bin/env python3
"""Native Client 3D cosmetics for a Horizon source tree (MIT licensed, see NATIVE.md).

Usage: native_cosmetics.py <checkout> <native-checkout>

Adds crates/horizon-client/src/game/native_cosmetics.rs (native-mod's NCM models, turned into the
game's own cosmetic items) and draws them in cosmetics_geometry beside Horizon's items. Native
items show whether or not the account owns a Horizon cosmetic, as in the Java mod. native_skins.py
must run first: the skin directory says what each player wears.

Every change names the exact text it replaces and the build FAILS when upstream changed it,
instead of shipping a client that silently shows no Native cosmetics. Safe to run again.
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
        raise SystemExit(f'native_cosmetics: expected one {old[:70]!r} in {path.relative_to(root)}, '
                         f'found {text.count(old)}. Upstream changed; update tools/native/native_cosmetics.py.')
    path.write_text(text.replace(old, new), encoding='utf-8')


if not (CLIENT / 'native_skins.rs').exists():
    raise SystemExit('native_cosmetics: run native_skins.py first.')

shutil.copyfile(native / 'packaging/native/native_cosmetics.rs', CLIENT / 'game/native_cosmetics.rs')
patch(CLIENT / 'game.rs', 'mod cosmetics;\n', 'mod cosmetics;\nmod native_cosmetics;\n')

COSMETICS = CLIENT / 'game/cosmetics.rs'
# What the avatar wears: Horizon's items, then Native's.
patch(COSMETICS,
      '        let worn = self.worn(m, own)?;\n'
      '        if horizon_client::mobs::trader::is_invisible(m) {\n',
      '        let native = self.native_cosmetics_worn(m, own);\n'
      '        let worn = match self.worn(m, own) {\n'
      '            Some(worn) => worn,\n'
      '            None if !native.is_empty() => Arc::from(Vec::new()),\n'
      '            None => return None,\n'
      '        };\n'
      '        if horizon_client::mobs::trader::is_invisible(m) {\n')
patch(COSMETICS, '        for item in worn.iter() {\n',
      '        for item in worn.iter().chain(native.iter()) {\n')
# Native items are not Horizon's "others' cosmetics", which show only to a Horizon owner.
patch(COSMETICS, '            if !bodies || covered(m, item.slot) {\n',
      '            if (!bodies && !item.id.starts_with("native:")) || covered(m, item.slot) {\n')

print('native_cosmetics: Native cosmetics applied')
