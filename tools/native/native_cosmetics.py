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
from native_moved import relocate

root = pathlib.Path(sys.argv[1]).resolve()
native = pathlib.Path(sys.argv[2]).resolve()
CLIENT = root / 'crates/horizon-client/src'


def patch(path, old, new):
    """Replace `old` (exactly once) with `new`; fine if it was done already."""
    path = relocate(path, old, new)
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

# Items made in code (an NCM model) rather than read from a file: `Item` keeps private fields (its animated parts),
# so it gets a constructor that finds them, and its top, as `parse` does.
patch(root / 'crates/horizon-cosmetics/src/item.rs',
      '\nimpl Item {\n    /// Reads and checks an item file.\n',
      '\nimpl Item {\n'
      '    /// Native Client: an item built from an NCM model (game/native_cosmetics.rs), its animated parts and its\n'
      '    /// top found as `parse` finds them.\n'
      '    #[allow(clippy::too_many_arguments)]\n'
      '    pub fn native(\n'
      '        id: String,\n'
      '        slot: Slot,\n'
      '        bone: Bone,\n'
      '        root: Part,\n'
      '        texture: Texture,\n'
      '        animations: Vec<Animation>,\n'
      '        vertices: usize,\n'
      '        limbs: Vec<(Bone, Part)>,\n'
      '    ) -> Item {\n'
      '        let animated = {\n'
      '            let mut roots = vec![&root];\n'
      '            roots.extend(limbs.iter().map(|(_, l)| l));\n'
      '            animated(&roots, &animations)\n'
      '        };\n'
      '        let item = Item {\n'
      '            id,\n'
      '            slot,\n'
      '            bone,\n'
      '            root,\n'
      '            texture,\n'
      '            animations,\n'
      '            vertices,\n'
      '            limbs,\n'
      '            tag: None,\n'
      '            top: 0.0,\n'
      '            emote: None,\n'
      '            companion: None,\n'
      '            trail: None,\n'
      '            aura: None,\n'
      '            animated,\n'
      '        };\n'
      '        let top = item.extent().top;\n'
      '        Item { top, ..item }\n'
      '    }\n\n'
      '    /// Reads and checks an item file.\n')

COSMETICS = CLIENT / 'game/cosmetics.rs'
PLAYER_HOOKS = CLIENT / 'game/player_hooks.rs'
# Since 2026-11-13 Horizon's cosmetics draw through the player hooks (game/player_hooks.rs, `PlayerDraw`), whose
# sprites are 64×64 skin slots: Native's are drawn from there too, after the crates' layers (native_cosmetics_draw.rs).
HOOKED = PLAYER_HOOKS.is_file() and 'fn player_layers_geometry(' in PLAYER_HOOKS.read_text(encoding='utf-8')
if HOOKED:
    shutil.copyfile(native / 'packaging/native/native_cosmetics_draw.rs', CLIENT / 'game/native_cosmetics_draw.rs')
    hooks_text = PLAYER_HOOKS.read_text(encoding='utf-8')
    if 'include!("native_cosmetics_draw.rs");' not in hooks_text:
        PLAYER_HOOKS.write_text(hooks_text.rstrip('\n') + '\n\ninclude!("native_cosmetics_draw.rs");\n', encoding='utf-8')
    patch(PLAYER_HOOKS,
          '        engine::player_layers(&model, &mut draw);\n        draw.bounds\n',
          '        engine::player_layers(&model, &mut draw);\n        draw.native_cosmetics(false);\n        draw.bounds\n')
    patch(PLAYER_HOOKS,
          '        engine::player_layers(&model, &mut draw);\n    }\n',
          '        engine::player_layers(&model, &mut draw);\n        draw.native_cosmetics(true);\n    }\n')
    patch(PLAYER_HOOKS,
          '        engine::frame(&frame, &mut HookStore(self));\n',
          '        engine::frame(&frame, &mut HookStore(self));\n        self.native_cosmetics_frame();\n')
    # Gear hides a Native item's slot as it hides Horizon's; "other players' cosmetics" is the cosmetics' setting now.
    patch(COSMETICS, '\nfn covered(gear: Gear, slot: Slot) -> bool {\n', '\npub(super) fn covered(gear: Gear, slot: Slot) -> bool {\n')
    cosmetics_text = COSMETICS.read_text(encoding='utf-8')
    if 'fn native_others_shown(' not in cosmetics_text:
        COSMETICS.write_text(cosmetics_text.rstrip('\n') + '\n\n'
                             '/// Native: whether other players\' cosmetics show in full (game/native_cosmetics.rs: a hood replaces\n'
                             '/// the hat layer only then).\n'
                             'pub(super) fn native_others_shown() -> bool {\n'
                             '    service(|s| s.others() == Others::Shown)\n'
                             '}\n', encoding='utf-8')
    NATIVE_FILE = CLIENT / 'game/native_cosmetics.rs'
    OTHERS = 'self.settings.extras.effective_others() == horizon_cosmetics::prefs::Others::Shown'
    native_text = NATIVE_FILE.read_text(encoding='utf-8')
    if native_text.count(OTHERS) != 1:
        raise SystemExit('native_cosmetics: packaging/native/native_cosmetics.rs no longer asks effective_others once; '
                         'update tools/native/native_cosmetics.py.')
    NATIVE_FILE.write_text(native_text.replace(OTHERS, 'super::cosmetics::native_others_shown()'), encoding='utf-8')
else:
    # What the avatar wears: Horizon's items (and an emote's props), then Native's.
    patch(COSMETICS,
          '        let worn = self.worn(m, own);\n',
          '        let worn = self.worn(m, own);\n'
          '        let native = self.native_cosmetics_worn(m, own);\n')
    patch(COSMETICS,
          '        if worn.is_none() && emote.is_none() || horizon_client::mobs::trader::is_invisible(m) {\n',
          '        if worn.is_none() && native.is_empty() && emote.is_none() || horizon_client::mobs::trader::is_invisible(m) {\n')
    patch(COSMETICS,
          '        let worn_items = worn.iter().flat_map(|w| w.iter()).map(|i| (i, None));\n',
          '        let worn_items = worn.iter().flat_map(|w| w.iter()).chain(native.iter()).map(|i| (i, None));\n')
    patch(COSMETICS,
          '        for (item, emoting) in worn_items.chain(emote_items) {\n'
          '            if emoting.is_none() && !bodies {\n',
          '        for (item, emoting) in worn_items.chain(emote_items) {\n'
          '            // A blinking part is off for part of its cycle (the Java mod skips it).\n'
          '            if !super::native_cosmetics::shown(item, ticks) {\n'
          '                continue;\n'
          '            }\n'
          '            let light = super::native_cosmetics::light_of(item, light);\n'
          '            // Native items are not Horizon\'s "others\' cosmetics", which show only to a Horizon owner.\n'
          '            if emoting.is_none() && !bodies && !item.id.starts_with("native:") {\n')

# Native cosmetics keep their texture's resolution: one 2048×2048 area of the atlas, each texture packed at its own size.
RENDER = root / 'crates/horizon-client-render/src'
# (Since 2026-11-13 a blank slot is an empty buffer, f2c108108; the area is blank the same way.)
SLOTS = '        for i in 0..SKIN_SLOTS {\n            images.push((format!("horizon/skin{i}"), 64, 64, %s));\n        }\n'
BLANK = 'Vec::new()' if SLOTS % 'Vec::new()' in (RENDER / 'hud.rs').read_text(encoding='utf-8') else None
patch(RENDER / 'hud.rs', SLOTS % (BLANK or 'vec![0; 64 * 64 * 4]'),
      SLOTS % (BLANK or 'vec![0; 64 * 64 * 4]') +
      '        // Native cosmetic textures (game/native_cosmetics.rs), packed at their own size.\n'
      '        images.push(("native/cosmetics".into(), 2048, 2048, %s));\n' % (BLANK or 'vec![0; 2048 * 2048 * 4]'))
if not HOOKED:
    patch(COSMETICS,
          '                self.store_skin(location, slot_image(&item));\n',
          '                if item.id.starts_with("native:") {\n'
          '                    self.store_native_cosmetic(&location, &item);\n'
          '                } else {\n'
          '                    self.store_skin(location, slot_image(&item));\n'
          '                }\n')
    patch(COSMETICS,
          '            item.texture.width as f32 / 64.0,\n'
          '            item.texture.height as f32 / 64.0,\n',
          '            item.texture.width as f32 / super::native_cosmetics::slot_width(item),\n'
          '            item.texture.height as f32 / super::native_cosmetics::slot_height(item),\n')
    patch(COSMETICS,
          '            self.skin_downloads.borrow_mut().drawn(&b.location);\n',
          '            self.skin_downloads.borrow_mut().drawn(&b.location);\n'
          '            super::native_cosmetics::drawn(&b.location);\n')

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
