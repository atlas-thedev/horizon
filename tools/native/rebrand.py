#!/usr/bin/env python3
"""Native Client rebrand of a Horizon source tree (MIT licensed, see NATIVE.md).

Usage: rebrand.py <checkout>

Renames the player-facing "Horizon" to "Native" in the client's string literals and
language files, and sets the window titles and the boot-screen tagline. Crate names,
identifiers, URLs, server addresses and the LICENSE are left alone. The icon and the
boot-screen layout are not touched. Safe to run again on the same tree.
"""
import pathlib
import re
import subprocess
import sys

root = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else '.').resolve()
files = subprocess.check_output(['git', 'ls-files'], cwd=root, text=True).split('\n')

WORD = re.compile(r"(?<![A-Za-z0-9_./-])Horizon(?![A-Za-z0-9_]|\.dev|-rust)")
STR = re.compile(r'b?"(?:[^"\\\n]|\\.)*"')
# Upstream references, protocol keys and test fixtures keep the original name.
KEEP = ('://', 'PlayHorizon', 'playhorizon', 'Horizon Official', 'Horizon Version',
        'Horizon.Bunchy', 'Horizon Studio', 'Horizon Lab', 'HorizonLab')
RUST_DIRS = (
    'crates/horizon-client/src', 'crates/horizon-client-render/src',
    'crates/horizon-client-core/src', 'crates/horizon-client-input/src',
    'crates/horizon-client-lib/src', 'crates/horizon-prepare/src',
    'crates/horizon-theme/src', 'crates/horizon-assets/src',
)
LANG_DIR = 'crates/horizon-client/assets/horizon/lang/'


def fix_string(match):
    text = match.group(0)
    if any(k in text for k in KEEP):
        return text
    return WORD.sub('Native', text)


changed = 0
for name in files:
    path = root / name
    if not path.is_file():
        continue
    is_rust = name.endswith('.rs') and name.startswith(RUST_DIRS) and '/tests/' not in name
    is_lang = name.startswith(LANG_DIR) and name.endswith('.json')
    if not (is_rust or is_lang):
        continue
    old = path.read_text(encoding='utf-8')
    new = STR.sub(fix_string, old)
    if name == 'crates/horizon-client/src/ui_theme.rs':
        new = new.replace('"Rust based minecraft client"', '"Native Client (Rust engine)"')
    if name in ('crates/horizon-client/src/game.rs', 'crates/horizon-client/src/game/no_resources.rs',
                'crates/horizon-client/src/game/split/windows.rs'):
        new = new.replace('.with_title("Native")', '.with_title("Native Client")')
        new = new.replace('.with_title(format!("Native {key}"))', '.with_title(format!("Native Client {key}"))')
    if new != old:
        path.write_text(new, encoding='utf-8')
        changed += 1
print(f'rebrand: {changed} files changed')
