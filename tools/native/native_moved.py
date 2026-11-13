"""Upstream splits big files (game.rs into game/*.rs, ...). A patch anchor that left the file a script names is looked
for in that file's own folder (`game.rs` -> `game/`): when exactly one file there holds it, the patch goes there.
Anything else (gone, or in several files) still fails the build, so nothing is patched in the wrong place."""
import pathlib


def relocate(path, old, done):
    """The file to patch: `path` when it holds `old` (or `done`, the patched text), else the one file under
    `path`'s folder that does. `path` itself when none or several do (the caller then reports it)."""
    path = pathlib.Path(path)
    text = path.read_text(encoding='utf-8') if path.is_file() else ''
    if done in text or text.count(old) == 1:
        return path
    folder = path.with_suffix('')
    if not folder.is_dir():
        return path
    found = []
    for f in sorted(folder.rglob('*.rs')):
        t = f.read_text(encoding='utf-8')
        if done in t:
            return f
        if t.count(old) == 1:
            found.append(f)
        elif t.count(old) > 1:
            return path
    return found[0] if len(found) == 1 else path
