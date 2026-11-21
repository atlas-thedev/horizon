"""Find precise upstream patch anchors after Rust modules move or split."""
import pathlib


def game_sources(client):
    """Place Native modules where either generation of the game discovers them."""
    game = client / 'game'
    if '/game_modules.rs' in (client / 'game.rs').read_text(encoding='utf-8'):
        game = game / 'native'
        game.mkdir(exist_ok=True)
        (game / '.game-area').touch()
    return game


def relocate(path, old, done):
    """Use the named file or a unique anchor in the split or relocated module."""
    path = pathlib.Path(path)
    text = path.read_text(encoding='utf-8') if path.is_file() else ''
    if done in text or text.count(old) == 1:
        return path
    folder = path.with_suffix('')
    if not folder.is_dir() and not path.is_file():
        # For example, game/remote.rs became game/network/remote/mod.rs.
        folder = path.parent
    if not folder.is_dir():
        return path
    found = []
    for f in sorted(folder.rglob('*.rs')):
        t = f.read_text(encoding='utf-8')
        if done in t or t.count(old) == 1:
            found.append(f)
        elif t.count(old) > 1:
            return path
    return found[0] if len(found) == 1 else path
