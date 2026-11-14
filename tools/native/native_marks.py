#!/usr/bin/env python3
"""Native Client name marks for a Horizon source tree (MIT licensed, see NATIVE.md).

Usage: native_marks.py <checkout> <native-checkout>

The Native logo (or the Native+ mark) in front of every Native player's name on name tags and in
the tab list, as native-mod's NameTags does. Adds crates/horizon-client-render/src/native_marks.rs
and the mod's two logo bitmaps, puts those glyphs first in the default and uniform fonts, and marks
the two names. native_skins.rs registers who plays from Native.

Every change names the exact text it replaces and the build FAILS when upstream changed it,
instead of shipping a client that silently has no Native marks. Safe to run again.
"""
import pathlib
import shutil
import sys
import os as _os, sys as _sys
_sys.path.insert(0, _os.path.dirname(_os.path.abspath(__file__)))  # also when run through runpy (the tests)
from native_moved import relocate  # noqa: E402

root = pathlib.Path(sys.argv[1]).resolve()
native = pathlib.Path(sys.argv[2]).resolve()
RENDER = root / 'crates/horizon-client-render/src'
CLIENT = root / 'crates/horizon-client/src'


def patch(path, old, new):
    """Replace `old` (exactly once) with `new`; fine if it was done already."""
    path = relocate(path, old, new)
    text = path.read_text(encoding='utf-8')
    if new in text:
        return
    if text.count(old) != 1:
        raise SystemExit(f'native_marks: expected one {old[:70]!r} in {path.relative_to(root)}, '
                         f'found {text.count(old)}. Upstream changed; update tools/native/native_marks.py.')
    path.write_text(text.replace(old, new), encoding='utf-8')


for name in ('native_marks.rs', 'native_logo.png', 'native_logo_plus.png'):
    shutil.copyfile(native / 'packaging/native' / name, RENDER / name)

patch(RENDER / 'lib.rs', 'pub mod font;\n', 'pub mod font;\npub mod native_marks;\n')
patch(CLIENT / 'client_lib.rs', 'pub mod discord_presence;\n',
      'pub mod discord_presence;\npub use horizon_client_render::native_marks;\n')

# The logo glyphs, first in the fonts the mod adds them to; their sheets come from the embedded bitmaps.
FONT = RENDER / 'font.rs'
patch(FONT, '        providers(&self.root, id, 0, &mut all);\n',
      '        providers(&self.root, id, 0, &mut all);\n'
      '        if id == DEFAULT || id == "minecraft:uniform" {\n'
      '            all.splice(0..0, crate::native_marks::font_providers());\n'
      '        }\n')
SHEET = '            let sheet = crate::hud::decode(&root.join(&path)).or_else(|first| {\n'
if SHEET in FONT.read_text(encoding='utf-8') or 'crate::native_marks::sheet(file)\n                .unwrap_or_else' \
        in FONT.read_text(encoding='utf-8'):
    # Since 2026-11-13 a sheet the pack lacks is looked for in the default resources too.
    patch(FONT, SHEET,
          '            let sheet = crate::native_marks::sheet(file)\n'
          '                .unwrap_or_else(|| crate::hud::decode(&root.join(&path)))\n'
          '                .or_else(|first| {\n')
else:
    patch(FONT, '        None => match crate::hud::decode(&root.join(resource_path(file, "textures", ""))) {\n',
          '        None => match crate::native_marks::sheet(file)\n'
          '            .unwrap_or_else(|| crate::hud::decode(&root.join(resource_path(file, "textures", ""))))\n'
          '        {\n')

# The tab list row's name (NameTags.tab).
patch(RENDER / 'tab_list.rs', '            let name = name_for_display(p, i.board).resolve_legacy();\n',
      '            let name = crate::native_marks::marked(name_for_display(p, i.board), &p.name, Some(p.uuid))\n'
      '                .resolve_legacy();\n')

# A player's name tag (NameTags.nameplate), after any mod's own text.
NAMES = CLIENT / 'game/names.rs'
LINES = ('            let mut lines = Vec::new();\n'
         '            // EntityRenderer.extractNameTags: the score shows within the\n')
patch(NAMES, LINES,
      '            // Native Client: the Native logo in front of a Native player\'s name (native_marks.rs).\n'
      '            let shown = if player {\n'
      '                let native_id = self.persist.uuids.get(&m.id).map(|u| {\n'
      '                    u.iter().fold(0u128, |id, part| (id << 32) | u128::from(*part as u32))\n'
      '                });\n'
      '                horizon_client::native_marks::marked(shown, &name, native_id)\n'
      '            } else {\n'
      '                shown\n'
      '            };\n' + LINES)

print('native_marks: Native name marks applied')
