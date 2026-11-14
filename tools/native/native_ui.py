#!/usr/bin/env python3
"""Native Client UI drawing layer for a Horizon source tree (MIT licensed, see NATIVE.md).

Usage: native_ui.py <checkout> <native-checkout>

Adds crates/horizon-client-render/src/native_ui/ (the port of native-mod's ui/gfx: atlas, shapes,
fonts, images, icons, canvas, theme) with the mod's assets decoded from their base64 sources, and
gives the HUD pass Native texture pages (packaging/native/native_pages.rs): a run of HUD vertices
marked Blend::Native(n) is drawn with the HUD's own pipeline from Native page n (0 the UI atlas,
the others whole images such as backgrounds).

Every change names the exact text it replaces and the build FAILS when upstream changed it,
instead of shipping a client that silently draws no Native UI. Safe to run again.
"""
import base64
import pathlib
import shutil
import sys
import os as _os, sys as _sys
_sys.path.insert(0, _os.path.dirname(_os.path.abspath(__file__)))  # also when run through runpy (the tests)
from native_moved import relocate  # noqa: E402

root = pathlib.Path(sys.argv[1]).resolve()
native = pathlib.Path(sys.argv[2]).resolve()
RENDER = root / 'crates/horizon-client-render/src'


def patch(path, old, new):
    """Replace `old` (exactly once) with `new`; fine if it was done already."""
    path = relocate(path, old, new)
    text = path.read_text(encoding='utf-8')
    if new in text:
        return
    if text.count(old) != 1:
        raise SystemExit(f'native_ui: expected one {old[:70]!r} in {path.relative_to(root)}, '
                         f'found {text.count(old)}. Upstream changed; update tools/native/native_ui.py.')
    path.write_text(text.replace(old, new), encoding='utf-8')


def insert_after(path, anchor, addition):
    patch(path, anchor, anchor + addition)


# The drawing layer, its assets decoded (the mod keeps them as base64 text too).
source = native / 'packaging/native/native_ui'
target = RENDER / 'native_ui'
if target.exists():
    shutil.rmtree(target)
shutil.copytree(source, target, ignore=shutil.ignore_patterns('assets'))
(target / 'assets').mkdir()
for f in (source / 'assets').iterdir():
    if f.name.endswith('.b64'):
        (target / 'assets' / f.name[:-4]).write_bytes(base64.b64decode(f.read_text(encoding='ascii')))
    else:
        shutil.copyfile(f, target / 'assets' / f.name)
insert_after(RENDER / 'lib.rs', 'pub mod font;\n', 'pub mod native_ui;\n')
CLIENT_LIB = root / 'crates/horizon-client/src/client_lib.rs'
insert_after(CLIENT_LIB, 'pub mod discord_presence;\n', 'pub use horizon_client_render::native_ui;\n')

# Native texture pages in the HUD pass.
shutil.copyfile(native / 'packaging/native/native_pages.rs', RENDER / 'native_pages.rs')
HUD = RENDER / 'hud.rs'
patch(HUD, '    CitGlint(u8),\n}\n',
      '    CitGlint(u8),\n'
      '    /// Drawn from a Native UI texture page (`native_pages.rs`): 0 the atlas, else an image.\n'
      '    Native(u16),\n}\n')
patch(HUD, '    pub inline_profiles: Vec<serde_json::Value>,\n}\n',
      '    pub inline_profiles: Vec<serde_json::Value>,\n'
      '    /// The Native UI pages\' new pixels this frame (`native_pages.rs`).\n'
      '    pub native: Vec<NativeUpload>,\n}\n')
patch(HUD, '            inline_profiles: Vec::new(),\n        }\n    }\n',
      '            inline_profiles: Vec::new(),\n            native: Vec::new(),\n        }\n    }\n')
# The one test that spells a batch out field by field.
patch(HUD, '                inline_profiles: Vec::new(),\n            };\n',
      '                inline_profiles: Vec::new(),\n                native: Vec::new(),\n            };\n')
patch(HUD, '    pub zoom: f32,\n}\nimpl HudRenderer {\n',
      '    pub zoom: f32,\n    native: NativePages,\n}\nimpl HudRenderer {\n')
# First in the literal: `layout` and `uniform` move into their own fields after it.
# (Since 2026-11-13 `pictures: HashMap::new()` comes first there; `native` still goes before the fields that move.)
# Since 2026-11-13 the HUD layout has a 7th binding (the armour glint texture); the pages' groups must name it too.
ARMOR = 'Some(glint_armor_view)' if 'glint_armor_view: &wgpu::TextureView' in HUD.read_text(encoding='utf-8') else 'None'
patch(HUD, '            atlas_texture: None,\n',
      '            native: NativePages::new(&layout, &uniform, &sampler, &linear_sampler, glint_view, glint_sampler, ' + ARMOR + '),\n'
      '            atlas_texture: None,\n')
insert_after(HUD, '        self.upload_patches(uploads, encoder, &batch.patches);\n',
             '        self.native.upload(device, uploads, encoder, &batch.native);\n')
# Since 2026-11-13 each HUD pipeline has a plain and a coverage variant (gfx/hud_coverage.rs).
ALPHA = ('self.pipeline.get(coverage.is_some())'
         if 'use crate::gfx::hud_coverage::{self, Pipeline};' in HUD.read_text(encoding='utf-8') else '&self.pipeline')
patch(HUD, '                    Blend::CitGlint(blend) => {\n                        if let Some(cit) = &self.cit_glint {\n',
      '                    Blend::Native(page) => {\n'
      '                        if let Some(group) = self.native.group(*page) {\n'
      '                            pass.set_bind_group(0, group, &[]);\n'
      f'                            pass.set_pipeline({ALPHA});\n'
      '                            pass.draw(range.clone(), 0..1);\n'
      '                            pass.set_bind_group(0, &self.group, &[]);\n'
      '                        } else {\n'
      '                            // A page this renderer never got: the UI uploads its pages again.\n'
      '                            NATIVE_PAGES_LOST.store(true, std::sync::atomic::Ordering::Relaxed);\n'
      '                        }\n'
      '                        continue;\n'
      '                    }\n'
      '                    Blend::CitGlint(blend) => {\n                        if let Some(cit) = &self.cit_glint {\n')
text = HUD.read_text(encoding='utf-8')
if 'include!("native_pages.rs");' not in text:
    HUD.write_text(text.rstrip('\n') + '\n\ninclude!("native_pages.rs");\n', encoding='utf-8')

print('native_ui: Native UI drawing layer and texture pages applied')
