"""Keep both title-screen hosts connected to Native's clipboard UI."""
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
NATIVE = ROOT / 'packaging/native/native_client'


class NativeMultiplayerClipboardHooks(unittest.TestCase):
    def test_title_hosts_pass_their_own_clipboards(self):
        installer = (ROOT / 'tools/native/native_client.py').read_text(encoding='utf-8')
        self.assertIn('native_title_frame(batch, title, &who, &gui.atlas, items, &mut self.menu.clipboard)', installer)
        self.assertIn('native_title_frame(&mut batch, title, &who, &self.gui.atlas, Some(&self.pack.item_renderer), &mut self.clipboard)', installer)

    def test_title_frame_reads_and_writes_clipboard(self):
        source = (NATIVE / 'mod.rs').read_text(encoding='utf-8')
        frame = source[source.index('pub(super) fn native_title_frame('):source.index('struct TitleItemEnv')]
        self.assertIn('s.wants_paste()', frame)
        self.assertIn('Event::Button { button: 0, pressed: false }', frame)
        self.assertIn('clipboard.get()', frame)
        self.assertIn('state.ui.clip_in = clip;', frame)
        self.assertIn('state.ui.clip_out.take()', frame)
        self.assertIn('clipboard.set(&text)', frame)

    def test_direct_and_saved_addresses_share_the_paste_widget(self):
        source = (NATIVE / 'servers_dialogs.rs').read_text(encoding='utf-8')
        self.assertIn('"play.example.net", true)', source)
        self.assertIn('f.paste_replace(&ui.clip_in)', source)
        self.assertIn('"{}#paste"', source)


if __name__ == '__main__':
    unittest.main()
