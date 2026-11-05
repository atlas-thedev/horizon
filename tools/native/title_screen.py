#!/usr/bin/env python3
"""Native Client title screen for a Horizon source tree (MIT licensed, see NATIVE.md).

Usage: title_screen.py <checkout> <native-checkout>

Draws the "NATIVE CLIENT" wordmark above the classic logo and the signed-in account line
under it. The account comes from native_api.py, which must run first.

Every change names the exact text it replaces and the build FAILS when upstream changed it,
instead of shipping a client that silently has no Native title. Safe to run again.
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
        raise SystemExit(f'title_screen: expected one {old[:70]!r} in {path.relative_to(root)}, '
                         f'found {text.count(old)}. Upstream changed; update tools/native/title_screen.py.')
    path.write_text(text.replace(old, new), encoding='utf-8')


shutil.copyfile(native / 'packaging/native/title_native.rs', CLIENT / 'title_native.rs')

title = CLIENT / 'title.rs'
patch(title, 'fn draw_logo(atlas: &HudAtlas, batch: &mut HudBatch, w: f32) {',
      'fn draw_logo(atlas: &HudAtlas, batch: &mut HudBatch, w: f32) {\n    native_title_branding(atlas, batch, w);')
text = title.read_text(encoding='utf-8')
if 'include!("title_native.rs");' not in text:
    title.write_text(text.rstrip('\n') + '\n\ninclude!("title_native.rs");\n', encoding='utf-8')

print('title_screen: Native title branding applied')
