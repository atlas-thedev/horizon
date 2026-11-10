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
shutil.copyfile(native/'packaging/native/native_emote_props.rs', client/'native_emote_props.rs')
shutil.copyfile(native/'packaging/native/native_emote_renderer.rs', client/'game/native_emote_renderer.rs')
# Native cosmetics must already be installed, as in the release workflow.
p = client/'game/native_cosmetics.rs'
s = p.read_text(encoding='utf-8')
marker = '// Native emote props share the bounded cosmetic atlas packer'
if marker not in s:
    s += "\n" + (native/'packaging/native/native_emote_atlas.rs').read_text(encoding='utf-8')
p.write_text(s, encoding='utf-8')

def install(path, old, new, marker):
    s = path.read_text(encoding='utf-8')
    if marker not in s:
        if s.count(old) != 1: raise SystemExit(f'Native emotes: expected one renderer hook in {path}')
        s = s.replace(old, new, 1)
        path.write_text(s, encoding='utf-8')

shutil.copyfile(native/'packaging/native/native_emote_preview.rs', client/'game/native_client/emote_props_preview.rs')
p = client/'game/native_client/boxes.rs'
s = p.read_text(encoding='utf-8')
marker = '// Native emote prop texels: participate in the same sorted list as the avatar.'
if marker not in s:
    for old, new in [
        ('    quads: Vec<Quad>,', '    quads: Vec<Quad>,\n    prop_samples: usize,'),
        ('            quads: Vec::with_capacity(4096),', '            quads: Vec::with_capacity(4096),\n            prop_samples: 0,'),
        ('        self.quads.clear();', '        self.quads.clear();\n        self.prop_samples = 0;'),
    ]:
        if s.count(old) != 1: raise SystemExit(f'Native emotes: preview boxes changed: {old}')
        s=s.replace(old,new,1)
    s += "\n" + (native/'packaging/native/native_emote_preview_boxes.rs').read_text(encoding='utf-8')
    p.write_text(s,encoding='utf-8')
install(client/'game/native_client/mod.rs', 'mod emote_wheel;', 'mod emote_wheel;\nmod emote_props_preview;', 'mod emote_props_preview;')
install(client/'game/native_client/preview3d.rs',
        'pub fn draw_wheel(c: &mut Canvas, skin: &Arc<Image>, slim: bool, p: [[f32; 6]; 6], rect: [f32; 4]) {',
        'pub fn draw_wheel(c: &mut Canvas, skin: &Arc<Image>, slim: bool, p: [[f32; 6]; 6], rect: [f32; 4], props: Option<(String, Arc<horizon_client::native_emote_props::PropClip>, u64)>) {',
        'props: Option<(String, Arc<horizon_client::native_emote_props::PropClip>, u64)>')
install(client/'game/native_client/preview3d.rs',
        '    let [rx,ry,rw,rh] = rect;\n    let unit =',
        '    let prop_quads = props.as_ref().and_then(|(_,clip,time)|clip.quads(*time));\n'
        '    if let Some(quads) = &prop_quads {\n'
        '        for q in quads { for point in q.xyz {\n'
        '            let (x,y,z)=(point[0]*16.0,point[1]*16.0-8.0,point[2]*16.0);\n'
        '            let vx=cy*x+sy*z; let vy=cp*y-sp*(-sy*x+cy*z);\n'
        '            bounds[0]=bounds[0].min(vx); bounds[1]=bounds[1].min(vy);\n'
        '            bounds[2]=bounds[2].max(vx); bounds[3]=bounds[3].max(vy);\n'
        '        }}\n'
        '    }\n'
        '    let [rx,ry,rw,rh] = rect;\n    let unit =', 'let prop_quads = props.as_ref()')
# Anchor the final b.end specifically to draw_wheel, which is the last function in this file.
p = client/'game/native_client/preview3d.rs'
s = p.read_text(encoding='utf-8')
marker = 'super::emote_props_preview::draw(&mut b,hash,clip,quads);'
if marker not in s:
    start=s.index('pub fn draw_wheel(')
    at=s.index('    b.end(c);',start)
    s=s[:at]+'    if let (Some((hash,clip,_)),Some(quads))=(&props,&prop_quads) {\n        '+marker+'\n    }\n'+s[at:]
    p.write_text(s,encoding='utf-8')
install(client/'game/native_client/emote_wheel.rs',
        '                preview3d::draw_wheel(c, &skin, slim, pose, rect);',
        '                preview3d::draw_wheel(c, &skin, slim, pose, rect, native_emotes::preview_props(hash, if active { Some(ui.now) } else { None }));',
        'preview3d::draw_wheel(c, &skin, slim, pose, rect, native_emotes::preview_props')

install(client/'game.rs', 'mod native_cosmetics;', 'mod native_cosmetics;\nmod native_emote_renderer;', 'mod native_emote_renderer;')
install(client/'game.rs', '    fn render_frame(&mut self) -> Result<bool> {',
        '    fn render_frame(&mut self) -> Result<bool> {\n        self.native_emote_prop_uploads();', 'self.native_emote_prop_uploads();')
install(client/'game/creatures.rs',
        '            let layers = horizon_client::mobs::render::layers_shown(mob);',
        '            let layers = horizon_client::mobs::render::layers_shown(mob);\n'
        '            if is_player && layers && !layers_only && !mob.invisible {\n'
        '                let base = horizon_client::mobs::render::base_transform(mob, alpha as f32, camera);\n'
        '                self.native_emote_props_geometry(mob.id, base, packed, out);\n'
        '            }', 'self.native_emote_props_geometry(mob.id,')
install(client/'game/perspective.rs',
        '        if horizon_client::mobs::render::layers_shown(m) {',
        '        if horizon_client::mobs::render::layers_shown(m) {\n'
        '            if body && !self.own_invisible() && !self.spectator() {\n'
        '                let base = horizon_client::mobs::render::base_transform(m, alpha as f32, camera);\n'
        '                self.native_emote_props_geometry(0, base, packed, out);\n'
        '            }', 'self.native_emote_props_geometry(0,')
p = client/'client_lib.rs'
s = p.read_text(encoding='utf-8')
if 'pub mod native_emotes;' not in s:
    s += '\npub mod native_emotes;\n'
if 'pub mod native_emote_props;' not in s:
    s += '\npub mod native_emote_props;\n'
p.write_text(s, encoding='utf-8')
p = client/'mobs/anim.rs'
s = p.read_text(encoding='utf-8')
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
p.write_text(s, encoding='utf-8')
p = root/'crates/horizon-client/assets/horizon/lang/en_us.json'
labels=json.loads(p.read_text(encoding='utf-8'))
labels.update({'native.emotes.select':'Select an emote','native.emotes.page':'Page','native.emotes.key':'Emote wheel','native.emotes.title':'Emotes','native.emotes.empty':'Empty slot','native.emotes.signin':'Sign in through Native Client to use emotes.','native.emotes.hint':'Click / 1–8 to play · Scroll / ← → for pages · Esc to close'})
labels.update({
    'native.emotes.cancel': 'Cancel',
    'native.emotes.description': 'Open the wheel or play a slot on its current page. Click a shortcut to change it; Backspace clears it.',
    'native.emotes.conflict': 'Conflict',
    'native.emotes.reserved.menu': 'Reserved: Native menu',
    'native.emotes.reserved.relay': 'Reserved: Relay',
    'native.emotes.reserved.music': 'Reserved: music control',
    'native.emotes.reserved.voice': 'Reserved: voice call control',
    **{f'native.emotes.slot{i}': f'Quick slot {i}' for i in range(1, 9)},
})
p.write_text(json.dumps(labels,ensure_ascii=False,indent=2)+'\n', encoding='utf-8')
print('native_emotes: authenticated wheel and player animation applied')
