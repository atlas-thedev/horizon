#!/usr/bin/env python3
"""Install Native emotes after native_client.py on the pinned upstream tree."""
import json
import shutil
import sys
from pathlib import Path
native = Path(__file__).resolve().parents[2]
root = Path(sys.argv[1]).resolve() if len(sys.argv)>1 else Path.cwd()
client = root/'crates/horizon-client/src'
shutil.copyfile(native/'packaging/native/native_emotes.rs', client/'native_emotes.rs')
p = client/'client_lib.rs'
s = p.read_text()
if 'pub mod native_emotes;' not in s:
    s += '\npub mod native_emotes;\n'
p.write_text(s)
p = client/'mobs/anim.rs'
s = p.read_text()
old = '''    let Some(playing) = &m.emote else {
        return vanilla_setup(m, s, p, w_anims);
    };'''
new = '''    let native_rest = ["head","body","right_arm","left_arm","right_leg","left_leg"].map(|n| p.has(n).then(||*p.get(n)));
    let Some(playing) = &m.emote else {
        vanilla_setup(m, s, p, w_anims);
        crate::native_emotes::apply(m.id, native_rest, p);
        return;
    };'''
if new not in s:
    if old not in s: raise SystemExit('Native emotes: upstream animation hook changed')
    s = s.replace(old,new,1).replace('    emote(playing, s.age, rest, p);','    emote(playing, s.age, rest, p);\n    crate::native_emotes::apply(m.id, native_rest, p);',1)
p.write_text(s)
p = root/'crates/horizon-client/assets/horizon/lang/en_us.json'
labels=json.loads(p.read_text())
labels.update({'native.emotes.key':'Emote wheel (hold)','native.emotes.title':'Emotes','native.emotes.empty':'Empty slot','native.emotes.signin':'Sign in through Native Client to use emotes.','native.emotes.hint':'Release to play · Scroll for pages · Esc to cancel'})
p.write_text(json.dumps(labels,ensure_ascii=False,indent=2)+'\n')
print('native_emotes: authenticated wheel and player animation applied')
