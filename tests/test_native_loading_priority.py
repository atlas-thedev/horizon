"""Guard Native's terrain screen against focus-loss pause/menu overlays."""
import ast
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


class NativeLoadingPriority(unittest.TestCase):
    def test_installer_pause_and_keyboard_hooks_are_unique_and_idempotent(self):
        source = (ROOT / 'tools/native/native_client.py').read_text(encoding='utf-8')
        tree = ast.parse(source)
        hooks = []
        for node in tree.body:
            if not (isinstance(node, ast.Expr) and isinstance(node.value, ast.Call)
                    and isinstance(node.value.func, ast.Name) and node.value.func.id == 'patch'):
                continue
            args = node.value.args
            if len(args) != 3 or not isinstance(args[0], ast.Name):
                continue
            try:
                old, new = ast.literal_eval(args[1]), ast.literal_eval(args[2])
            except (ValueError, TypeError):
                continue
            if args[0].id == 'POINTER' or ('self.studio_replay.is_some() && self.captured' in old):
                hooks.append((old, new))
        self.assertEqual(len(hooks), 3)
        for old, new in hooks:
            if 'self.has_singleplayer_server()' in old or 'self.control.is_none()' in old:
                self.assertIn('!self.remote_screen_up()', new)
            else:
                self.assertIn('if self.remote_screen_up()', new)
            self.assertEqual(old.count('self.remote_screen_up()'), 0)
            fixture = 'before\n' + old + 'after\n'
            def apply(text):
                if new in text:
                    return text
                if text.count(old) != 1:
                    raise ValueError('upstream changed')
                return text.replace(old, new)
            patched = apply(fixture)
            self.assertEqual(apply(patched), patched)
            with self.assertRaises(ValueError):
                apply(fixture.replace(old, 'changed'))

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
