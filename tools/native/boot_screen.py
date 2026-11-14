#!/usr/bin/env python3
"""Native Client launch screen and window size for a Horizon source tree (MIT licensed, see NATIVE.md).

Usage: boot_screen.py <checkout> <native-checkout>

Run after rebrand.py and the icon step. It
  * replaces Horizon's boot screens (the early GDI paint and the wgpu one) with the Native launch screen: the
    launcher's boot splash with the Native mark, "Native Client" and a sweeping bar, no console wall;
  * starts the window at Minecraft's 854 x 480 instead of 1280 x 720 (the launcher passes its own size anyway).

Every change names the exact text it replaces and the build FAILS when upstream changed that text, instead of
shipping a client that quietly still shows Horizon's screen. Safe to run again on the same tree.
"""
import pathlib
import re
import subprocess
import sys
import os as _os, sys as _sys
_sys.path.insert(0, _os.path.dirname(_os.path.abspath(__file__)))  # also when run through runpy (the tests)
from native_moved import relocate  # noqa: E402

root = pathlib.Path(sys.argv[1]).resolve()
native = pathlib.Path(sys.argv[2]).resolve()
CLIENT = root / 'crates/horizon-client/src'
THREADS = root / 'crates/horizon-threads/src'


def patch(path, old, new):
    """Replace `old` (exactly once) with `new`; fine if it was done already, an error if the anchor is gone."""
    path = relocate(path, old, new)
    text = path.read_text(encoding='utf-8')
    if new in text:
        return
    if text.count(old) != 1:
        raise SystemExit(f'boot_screen: expected one {old[:70]!r} in {path.relative_to(root)}, found {text.count(old)}. '
                         'Upstream changed; update tools/native/boot_screen.py.')
    path.write_text(text.replace(old, new), encoding='utf-8')


def patch_re(path, pattern, repl):
    """Like patch(), for text that must match a pattern; `repl` gets the match and returns the new text."""
    text = path.read_text(encoding='utf-8')
    found = list(re.finditer(pattern, text))
    if len(found) == 0 and re.search(DONE_PAINT, text):
        return
    if len(found) != 1:
        raise SystemExit(f'boot_screen: expected one match of {pattern[:70]!r} in {path.relative_to(root)}, found {len(found)}. '
                         'Upstream changed; update tools/native/boot_screen.py.')
    path.write_text(text[:found[0].start()] + repl(found[0]) + text[found[0].end():], encoding='utf-8')


# What the game's boot paint thread looks like once patched.
DONE_PAINT = r'&\[\],\s*\);\s*std::thread::sleep\(std::time::Duration::from_millis\(33\)\);'


# ---- the mark, made from the (Native) window icon ----
icon = root / 'crates/horizon-theme/icons/horizon-64.png'
subprocess.check_call([sys.executable, str(native / 'tools/native/make_mark.py'), str(icon),
                       str(CLIENT / 'native_mark.bin'), str(THREADS / 'native_mark.bin')])

# ---- the early GDI screen (Windows) ----
paint = THREADS / 'boot_paint.rs'
# Upstream made the fonts 'static on 2026-11-13 (AddFontMemResourceEx keeps them): either signature is the same paint.
PAINTS = ('pub fn paint(hwnd: isize, semibold: &[u8], mono: &[u8], logs: &[String])',
          "pub fn paint(hwnd: isize, semibold: &'static [u8], mono: &'static [u8], logs: &[String])")
if not any(sig in paint.read_text(encoding='utf-8') for sig in PAINTS) \
        and 'native_mark.bin' not in paint.read_text(encoding='utf-8'):
    raise SystemExit('boot_screen: boot_paint::paint changed upstream; update packaging/native/boot_paint.rs.')
paint.write_text((native / 'packaging/native/boot_paint.rs').read_text(encoding='utf-8'), encoding='utf-8')

# It repaints about 30 times a second now (300 ms would make the bar jump) and needs no console lines.
# (the repaint loop moved from game.rs to game/loading_screen.rs upstream on 2026-11-13)
REPAINT = r'&horizon_client::logging::recent_lines\(\),\s*\);\s*std::thread::sleep'
patch_re(next((f for f in (CLIENT / 'game/loading_screen.rs', CLIENT / 'game.rs')
               if f.is_file() and (re.search(REPAINT, f.read_text(encoding='utf-8'))
                                   or re.search(DONE_PAINT, f.read_text(encoding='utf-8')))), CLIENT / 'game.rs'),
         r'&horizon_client::logging::recent_lines\(\),(\s*\);\s*std::thread::sleep\(std::time::Duration::from_millis\()300(\)\);)',
         lambda m: '&[],' + m.group(1) + '33' + m.group(2))

# ---- the wgpu screen ----
(CLIENT / 'native_boot.rs').write_text((native / 'packaging/native/native_boot.rs').read_text(encoding='utf-8'),
                                       encoding='utf-8')
theme = CLIENT / 'ui_theme.rs'
patch(theme,
      '    wordmark(atlas, batch);\n    draw_boot_log(atlas, batch, logs);\n}',
      '    wordmark(atlas, batch);\n    let _ = logs;\n}')
patch(theme,
      'fn wordmark(atlas: &HudAtlas, batch: &mut HudBatch) {',
      'fn wordmark(atlas: &HudAtlas, batch: &mut HudBatch) {\n    native_launch_screen(atlas, batch);\n}\n'
      '#[allow(dead_code)]\nfn horizon_wordmark(atlas: &HudAtlas, batch: &mut HudBatch) {')
text = theme.read_text(encoding='utf-8')
if 'include!("native_boot.rs");' not in text:
    theme.write_text(text.rstrip('\n') + '\n\ninclude!("native_boot.rs");\n', encoding='utf-8')

# ---- the window opens at Minecraft's default size ----
game = CLIENT / 'game.rs'
patch(game, 'window_size: [1280, 720],', 'window_size: [854, 480],')
patch(game, '.map_or([1280, 720], |o| o.window_size)', '.map_or([854, 480], |o| o.window_size)')
patch(game, 'Window size (default 1280 x 720)', 'Window size (default 854 x 480)')
patch(game, '/// starting size, 1280x720 by default.', '/// starting size, 854x480 (Minecraft\'s) by default.')
print('boot_screen: launch screen and 854x480 window applied')
