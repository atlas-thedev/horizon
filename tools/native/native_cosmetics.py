#!/usr/bin/env python3
"""Native Client 3D cosmetics for a Horizon source tree (MIT licensed, see NATIVE.md).

Usage: native_cosmetics.py <checkout> <native-checkout>

Adds crates/horizon-client/src/game/native_cosmetics.rs (native-mod's NCM models, turned into the
game's own cosmetic items) and draws them in cosmetics_geometry beside Horizon's items. Native
items show whether or not the account owns a Horizon cosmetic, as in the Java mod. native_skins.py
must run first: the skin directory says what each player wears.

Every change names the exact text it replaces and the build FAILS when upstream changed it,
instead of shipping a client that silently shows no Native cosmetics. Safe to run again.
"""
import pathlib
import shutil
import sys

root = pathlib.Path(sys.argv[1]).resolve()
native = pathlib.Path(sys.argv[2]).resolve()
CLIENT = root / 'crates/horizon-client/src'


def patch(path, old, new):
    """Replace `old` (exactly once) with `new`; fine if it was done already."""
    text = path.read_text(encoding='utf-8')
    if new in text:
        return
    if text.count(old) != 1:
        raise SystemExit(f'native_cosmetics: expected one {old[:70]!r} in {path.relative_to(root)}, '
                         f'found {text.count(old)}. Upstream changed; update tools/native/native_cosmetics.py.')
    path.write_text(text.replace(old, new), encoding='utf-8')


if not (CLIENT / 'native_skins.rs').exists():
    raise SystemExit('native_cosmetics: run native_skins.py first.')

shutil.copyfile(native / 'packaging/native/native_cosmetics.rs', CLIENT / 'game/native_cosmetics.rs')
patch(CLIENT / 'game.rs', 'mod cosmetics;\n', 'mod cosmetics;\nmod native_cosmetics;\n')

COSMETICS = CLIENT / 'game/cosmetics.rs'
# What the avatar wears: Horizon's items, then Native's.
patch(COSMETICS,
      '        let worn = self.worn(m, own)?;\n'
      '        if horizon_client::mobs::trader::is_invisible(m) {\n',
      '        let native = self.native_cosmetics_worn(m, own);\n'
      '        let worn = match self.worn(m, own) {\n'
      '            Some(worn) => worn,\n'
      '            None if !native.is_empty() => Arc::from(Vec::new()),\n'
      '            None => return None,\n'
      '        };\n'
      '        if horizon_client::mobs::trader::is_invisible(m) {\n')
patch(COSMETICS, '        for item in worn.iter() {\n',
      '        for item in worn.iter().chain(native.iter()) {\n'
      '            // A blinking part is off for part of its cycle (the Java mod skips it).\n'
      '            if !super::native_cosmetics::shown(item, ticks) {\n'
      '                continue;\n'
      '            }\n'
      '            let light = super::native_cosmetics::light_of(item, light);\n')
# Native items are not Horizon's "others' cosmetics", which show only to a Horizon owner.
patch(COSMETICS, '            if !bodies || covered(m, item.slot) {\n',
      '            if (!bodies && !item.id.starts_with("native:")) || covered(m, item.slot) {\n')

# Native cosmetics keep their texture's resolution: one 2048×2048 area of the atlas, each texture packed at its own size.
RENDER = root / 'crates/horizon-client-render/src'
patch(RENDER / 'hud.rs',
      '        for i in 0..SKIN_SLOTS {\n'
      '            images.push((format!("horizon/skin{i}"), 64, 64, vec![0; 64 * 64 * 4]));\n'
      '        }\n',
      '        for i in 0..SKIN_SLOTS {\n'
      '            images.push((format!("horizon/skin{i}"), 64, 64, vec![0; 64 * 64 * 4]));\n'
      '        }\n'
      '        // Native cosmetic textures (game/native_cosmetics.rs), packed at their own size.\n'
      '        images.push(("native/cosmetics".into(), 2048, 2048, vec![0; 2048 * 2048 * 4]));\n')
patch(COSMETICS,
      '                self.store_skin(location, slot_image(&item));\n',
      '                if item.id.starts_with("native:") {\n'
      '                    self.store_native_cosmetic(&location, &item);\n'
      '                } else {\n'
      '                    self.store_skin(location, slot_image(&item));\n'
      '                }\n')
patch(COSMETICS,
      '                item.texture.width as f32 / 64.0,\n'
      '                item.texture.height as f32 / 64.0,\n',
      '                item.texture.width as f32 / super::native_cosmetics::slot_width(item),\n'
      '                item.texture.height as f32 / super::native_cosmetics::slot_height(item),\n')
patch(COSMETICS,
      '                self.skin_downloads.borrow_mut().drawn(&b.location);\n',
      '                self.skin_downloads.borrow_mut().drawn(&b.location);\n'
      '                super::native_cosmetics::drawn(&b.location);\n')

# HD capes (native_hd_capes.rs): the full picture of a cape larger than 64x32 is kept beside the skin slot's 64x32
# copy and drawn from the atlas's Native area at its own size.
shutil.copyfile(native / 'packaging/native/native_hd_capes.rs', RENDER / 'native_hd_capes.rs')
SKINS = RENDER / 'skins.rs'
text = SKINS.read_text(encoding='utf-8')
if 'include!("native_hd_capes.rs");' not in text:
    SKINS.write_text(text.rstrip('\n') + '\n\ninclude!("native_hd_capes.rs");\n', encoding='utf-8')
patch(SKINS, '                Kind::Cape => process_cape(w, h, &rgba),\n',
      '                Kind::Cape => {\n'
      '                    set_hd_cape(&format!("capes/{digest}"), w, h, &rgba);\n'
      '                    process_cape(w, h, &rgba)\n'
      '                }\n')
patch(SKINS, '            let (w, h, rgba) = crate::hud::decode_bytes(&std::fs::read(&file).ok()?).ok()?;\n'
             '            process(w, h, &rgba)\n',
      '            let (w, h, rgba) = crate::hud::decode_bytes(&std::fs::read(&file).ok()?).ok()?;\n'
      '            if location.starts_with("capes/") {\n'
      '                set_hd_cape(location, w, h, &rgba);\n'
      '            }\n'
      '            process(w, h, &rgba)\n')
patch(CLIENT / 'game/remote.rs',
      '        if location.starts_with("capes/") {\n'
      '            rect.v1 = (rect.v0 + rect.v1) / 2.0;\n'
      '        }\n'
      '        self.gui.atlas.sprites.insert(location, rect);\n',
      '        if location.starts_with("capes/") {\n'
      '            rect.v1 = (rect.v0 + rect.v1) / 2.0;\n'
      '        }\n'
      '        let cape = location.starts_with("capes/");\n'
      '        self.gui.atlas.sprites.insert(location.clone(), rect);\n'
      '        if cape {\n'
      '            self.store_native_hd_cape(&location);\n'
      '        }\n')

# Under a hood (native_skins.rs `hooded`) the helmet and the head item are not drawn either.
ARMOR_STANDS = CLIENT / 'game/armor_stands.rs'
patch(ARMOR_STANDS,
      '        self.stack_armor_layers(&self.armor)\n',
      '        let mut armor = self.armor.clone();\n'
      '        if self.player_model.as_ref().is_some_and(|p| horizon_client::native_skins::hooded(p.id)) {\n'
      '            armor[0] = None;\n'
      '        }\n'
      '        self.stack_armor_layers(&armor)\n')
patch(ARMOR_STANDS,
      '                (e.head.as_deref(), e.armor[0].as_ref()),\n',
      '                (\n'
      '                    e.head.as_deref().filter(|_| !horizon_client::native_skins::hooded(m.id)),\n'
      '                    e.armor[0].as_ref(),\n'
      '                ),\n')
patch(CLIENT / 'game/mob_blocks.rs',
      '        // The layer also draws on an invisible mob.\n',
      '        if horizon_client::native_skins::hooded(mob.id) {\n'
      '            return;\n'
      '        }\n'
      '        // The layer also draws on an invisible mob.\n')

print('native_cosmetics: Native cosmetics applied')
