"""The patch must keep Rust sources and include_str! assets UTF-8 on Windows."""
import contextlib
import io
import json
import runpy
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / 'tools/native/native_emotes.py'

class EmotePatchEncoding(unittest.TestCase):
    def test_cp1252_host_preserves_utf8_and_reapplying_is_idempotent(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            client = root / 'crates/horizon-client/src'
            (client / 'mobs').mkdir(parents=True)
            (client / 'client_lib.rs').write_text('// 日本語 · café\n', encoding='utf-8')
            (client / 'mobs/anim.rs').write_text('''// 日本語 · café
    let Some(playing) = &m.emote else {
        return vanilla_setup(m, s, p, w_anims);
    };
    emote(playing, s.age, rest, p);
''', encoding='utf-8')
            lang = root / 'crates/horizon-client/assets/horizon/lang/en_us.json'
            lang.parent.mkdir(parents=True)
            lang.write_text(json.dumps({'existing': '日本語 · café'}, ensure_ascii=False), encoding='utf-8')
            original_open = Path.open
            def windows_open(path, mode='r', buffering=-1, encoding=None, errors=None, newline=None):
                if 'b' not in mode and encoding in (None, 'locale'):
                    encoding = 'cp1252'
                return original_open(path, mode, buffering, encoding, errors, newline)
            def apply():
                with patch.object(sys, 'argv', [str(SCRIPT), str(root)]), patch.object(Path, 'open', windows_open), contextlib.redirect_stdout(io.StringIO()):
                    runpy.run_path(str(SCRIPT), run_name='__main__')
            apply()
            labels = json.loads(lang.read_bytes().decode('utf-8', errors='strict'))
            self.assertEqual(labels['existing'], '日本語 · café')
            self.assertEqual(labels['native.emotes.hint'], 'Click to play · Scroll for pages · Esc to close')
            self.assertIn('// 日本語 · café', (client / 'mobs/anim.rs').read_text(encoding='utf-8'))
            before = {p: p.read_bytes() for p in root.rglob('*') if p.is_file()}
            apply()
            self.assertEqual(before, {p: p.read_bytes() for p in before})

if __name__ == '__main__':
    unittest.main()
