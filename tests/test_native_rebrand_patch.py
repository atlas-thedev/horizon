"""Branding must preserve source lookup markers used by the hook audit."""
import contextlib
import io
import runpy
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / 'tools/native/rebrand.py'


class RebrandSourceMarkers(unittest.TestCase):
    def test_hook_section_remains_discoverable_after_rebranding(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            relative = 'crates/horizon-client/src/game/native_hooks.rs'
            source = root / relative
            source.parent.mkdir(parents=True)
            source.write_text('''/// The hook points. Each does nothing in Horizon.
impl Game<'_> {
    pub(super) fn native_tick(&self) {}
}
/// Server-level helpers for native mods.
const TITLE: &str = "Horizon";
const URL: &str = "https://playhorizon.dev";
const MARKER: &str = "/// The hook points. Each does nothing in Horizon.";
const END: &str = "/// Server-level helpers for native mods.";
const BYTE_MARKER: &[u8] = b"// Horizon source marker";
''', encoding='utf-8')

            def apply():
                with patch.object(sys, 'argv', [str(SCRIPT), str(root)]), \
                        patch('subprocess.check_output', return_value=relative + '\n'), \
                        contextlib.redirect_stdout(io.StringIO()):
                    runpy.run_path(str(SCRIPT), run_name='__main__')

            apply()
            result = source.read_text(encoding='utf-8')
            # Mirror the hook catalog audit: its embedded marker must still
            # find the actual declarations, rather than finding itself.
            marker = result.split('const MARKER: &str = "', 1)[1].split('";', 1)[0]
            end = result.split('const END: &str = "', 1)[1].split('";', 1)[0]
            section = result.split(marker)[1].split(end)[0]
            self.assertIn('pub(super) fn native_tick', section)
            self.assertIn('const TITLE: &str = "Native";', result)
            self.assertIn('"https://playhorizon.dev"', result)
            self.assertIn('b"// Horizon source marker"', result)
            apply()
            self.assertEqual(source.read_text(encoding='utf-8'), result)


if __name__ == '__main__':
    unittest.main()
