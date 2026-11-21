"""Guard Native's terrain screen against focus-loss pause/menu overlays."""
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


class NativeLoadingPriority(unittest.TestCase):
    def test_installer_pause_and_keyboard_hooks_are_unique_and_idempotent(self):
        source = (ROOT / 'tools/native/native_client.py').read_text(encoding='utf-8')
        start = source.index('def patch(')
        definition = source[start:source.index('\n\nif not', start)]
        hooks = source[source.index('POINTER = '):source.index('# The Native title screen')]
        patch_loading = compile(definition + '\n' + hooks, 'native_client.py', 'exec')
        sys.path.insert(0, str(ROOT / 'tools/native'))
        self.addCleanup(sys.path.pop, 0)
        from native_moved import relocate

        for replay, moved in [('self.studio_replay.is_some()', False), ('self.replay.playing()', True)]:
            with self.subTest(replay=replay), tempfile.TemporaryDirectory() as tmp:
                client = Path(tmp)
                game = client / 'game.rs'
                game.touch()
                directory = client / ('game/runtime' if moved else 'game')
                directory.mkdir(parents=True)
                pointer = directory / 'pointer_capture.rs'
                pointer.write_text(
                    '        self.has_singleplayer_server()\n            && !self.is_published()\n'
                    '            && self.control.is_none()\n            && self.screen == Screen::Playing\n',
                    encoding='utf-8',
                )
                keys = directory / 'play_keys.rs' if moved else game
                keys.write_text(f'                Screen::Playing => {{\n                    if {replay} && self.captured {{\n', encoding='utf-8')
                scope = {'CLIENT': client, 'GAME': game, 'root': client, 'relocate': relocate}
                exec(patch_loading, scope)
                patched = pointer.read_text(), keys.read_text()
                self.assertEqual(patched[0].count('!self.remote_screen_up()'), 2)
                self.assertEqual(patched[1].count('if self.remote_screen_up()'), 1)
                exec(patch_loading, scope)
                self.assertEqual((pointer.read_text(), keys.read_text()), patched)

    def test_loading_guards_native_render_and_passes_input_to_game(self):
        source = (ROOT / 'packaging/native/native_client/mod.rs').read_text(encoding='utf-8')
        frame = source[source.index('pub(super) fn native_ui_frame'):source.index('pub(super) fn native_window_event')]
        event = source[source.index('pub(super) fn native_window_event'):source.index('fn native_track_input')]
        pause = source[source.index('fn native_pause_active'):source.index('fn native_pause_info')]
        self.assertIn('!self.remote_screen_up() && self.native_mod_screen() == Some(MOD_ID)', frame)
        self.assertIn('!self.remote_screen_up() && self.screen == Screen::Paused', pause)
        self.assertIn('if self.remote_screen_up() {\n            return false;\n        }', event)
        self.assertLess(event.index('self.native_track_input(event);'), event.index('if self.remote_screen_up()'))


if __name__ == '__main__':
    unittest.main()
