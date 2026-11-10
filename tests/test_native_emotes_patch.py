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
            # Match the upstream hooks after native_client.py and native_cosmetics.py.
            files = {
                'mobs/render.rs': '    let mut body = m.previous_body_yaw + wrap(m.body_yaw - m.previous_body_yaw) * alpha;\n',
                'game.rs': 'mod native_cosmetics;\n    fn render_frame(&mut self) -> Result<bool> {\n',
                'game/native_cosmetics.rs': '// cosmetics source\n',
                'game/creatures.rs': '            let layers = horizon_client::mobs::render::layers_shown(mob);\n',
                'game/perspective.rs': '        if horizon_client::mobs::render::layers_shown(m) {\n',
                'game/native_client/mod.rs': 'mod emote_wheel;\n',
                'game/native_client/boxes.rs': '    quads: Vec<Quad>,\n            quads: Vec::with_capacity(4096),\n        self.quads.clear();\n',
                'game/native_client/preview3d.rs': 'pub fn draw_wheel(c: &mut Canvas, skin: &Arc<Image>, slim: bool, p: [[f32; 6]; 6], rect: [f32; 4]) {\n    let [rx,ry,rw,rh] = rect;\n    let unit = 0;\n    b.end(c);\n}\n',
                'game/native_client/emote_wheel.rs': '                preview3d::draw_wheel(c, &skin, slim, pose, rect);\n',
            }
            for name, content in files.items():
                path = client / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(content, encoding='utf-8')
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
            self.assertEqual(labels['native.emotes.hint'], 'Click / 1–8 to play · Scroll / ← → for pages · Esc to close')
            self.assertEqual(labels['native.emotes.cancel'], 'Cancel')
            self.assertIn('// 日本語 · café', (client / 'mobs/anim.rs').read_text(encoding='utf-8'))
            render = (client / 'mobs/render.rs').read_text(encoding='utf-8')
            self.assertIn('if avatar(m) && crate::native_emotes::is_playing(m.id)', render)
            self.assertIn('facing_yaw(m.previous_head_yaw, m.head_yaw, alpha)', render)
            before = {p: p.read_bytes() for p in root.rglob('*') if p.is_file()}
            apply()
            self.assertEqual(before, {p: p.read_bytes() for p in before})

if __name__ == '__main__':
    unittest.main()
