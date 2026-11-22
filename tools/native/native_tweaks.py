#!/usr/bin/env python3
"""Native Client's hold on the engine for a Horizon source tree (MIT licensed, see NATIVE.md).

Usage: native_tweaks.py <checkout> <native-checkout>

The Native modules (Zoom, Fullbright, Low Fire, Damage Tint, Block Overlay, Time and Weather
Changer, Crosshair, Freelook, No Hurt Cam, Hitbox Colours, Lag Free) hand what they want to
`native_ui::tweaks` (packaging/native/native_ui/tweaks.rs, copied by native_ui.py). This puts the
reads into the engine, each one a line next to the code it overrides. Every tweak is `None` or off
until a module is switched on, so the game is vanilla otherwise. native_ui.py must run first.

Every change names the exact text it replaces and the build FAILS when upstream changed it,
instead of shipping a client whose modules silently do nothing. Safe to run again.
"""
import pathlib
import sys
import os as _os, sys as _sys
_sys.path.insert(0, _os.path.dirname(_os.path.abspath(__file__)))  # also when run through runpy (the tests)
from native_moved import relocate  # noqa: E402

root = pathlib.Path(sys.argv[1]).resolve()
CRATES = root / 'crates'
RENDER = CRATES / 'horizon-client-render/src'
CLIENT = CRATES / 'horizon-client/src'


def patch(path, old, new):
    """Replace `old` (exactly once) with `new`; fine if it was done already."""
    path = relocate(path, old, new)
    text = path.read_text(encoding='utf-8')
    if new in text:
        return
    if text.count(old) != 1:
        raise SystemExit(f'native_tweaks: expected one {old[:70]!r} in {path.relative_to(root)}, '
                         f'found {text.count(old)}. Upstream changed; update tools/native/native_tweaks.py.')
    path.write_text(text.replace(old, new), encoding='utf-8')


def patch_any(*tries):
    """The first of several (path, old, new) whose `old` (or `new`) is there: one place upstream moved code from or
    to, by Nightly. The last one reports when none is."""
    for path, old, new in tries[:-1]:
        path = relocate(path, old, new)
        text = path.read_text(encoding='utf-8') if path.is_file() else ''
        if new in text or text.count(old) == 1:
            return patch(path, old, new)
    return patch(*tries[-1])


if not (RENDER / 'native_ui/tweaks.rs').is_file():
    raise SystemExit('native_tweaks: run native_ui.py first.')

# Fullbright: the module's brightness over the slider's, read every frame (never saved to options.txt); at full
# strength every light level is drawn at full brightness (Java's gamma 16).
patch(RENDER / 'gfx.rs',
      '            (self.gamma - frame.darkness.0).max(0.0),\n',
      '            (crate::native_ui::tweaks::gamma(self.gamma) - frame.darkness.0).max(0.0),\n')
patch_any((RENDER / 'gfx.rs',
           '        let key = LightmapKey::new(\n'
           '            sky,\n'
           '            darkness,\n'
           '            gamma,\n'
           '            night_vision,\n'
           '            flicker,\n'
           '            boss_darken,\n'
           '            self.fullbright,\n'
           '        );\n'
           '        if self.lightmap_uploaded && self.lightmap_key == Some(key) {\n'
           '            return;\n'
           '        }\n'
           '        let mut pixels = [255u8; 16 * 16 * 4];\n'
           '        if !self.fullbright {\n',
           '        let fullbright = self.fullbright || crate::native_ui::tweaks::fullbright();\n'
           '        let key = LightmapKey::new(\n'
           '            sky,\n'
           '            darkness,\n'
           '            gamma,\n'
           '            night_vision,\n'
           '            flicker,\n'
           '            boss_darken,\n'
           '            fullbright,\n'
           '        );\n'
           '        if self.lightmap_uploaded && self.lightmap_key == Some(key) {\n'
           '            return;\n'
           '        }\n'
           '        let mut pixels = [255u8; 16 * 16 * 4];\n'
           '        if !fullbright {\n'),
          (RENDER / 'gfx.rs',
           '        if self.fullbright {\n'
           '            pixels.fill(255);\n',
           '        if self.fullbright || crate::native_ui::tweaks::fullbright() {\n'
           '            pixels.fill(255);\n'))

# Entity models posed on the CPU where Intel's Gen9 graphics draw GPU-posed ones with holes (tweaks.rs).
patch(RENDER / 'gfx.rs',
      '        let entity_skin = entity_skin::EntitySkin::new(\n'
      '            &device,\n'
      '            animate::SpriteAnimator::compute(&adapter, &device),\n'
      '        );\n',
      '        let entity_skin = entity_skin::EntitySkin::new(\n'
      '            &device,\n'
      '            animate::SpriteAnimator::compute(&adapter, &device)\n'
      '                && !crate::native_ui::tweaks::skip_gpu_skinning(&adapter.get_info()),\n'
      '        );\n')

# No Hurt Cam: the damage tilt of the camera.
patch(RENDER / 'camera.rs',
      'pub fn bob_hurt(state: &BobState, damage_tilt_strength: f64, pose: &mut Mat4) {\n',
      'pub fn bob_hurt(state: &BobState, damage_tilt_strength: f64, pose: &mut Mat4) {\n'
      '    if crate::native_ui::tweaks::no_hurt_cam() {\n'
      '        return;\n'
      '    }\n')

# Low Fire.
PVP = CLIENT / 'game/entities/pvp.rs'
if not PVP.is_file():
    PVP = CLIENT / 'game/extras/pvp.rs'
patch_any((PVP,
           '        f32::from(self.settings.extras.pvp.tweaks.fire_lower) / 100.0\n',
           '        horizon_client::native_ui::tweaks::fire_lower(\n'
           '            f32::from(self.settings.extras.pvp.tweaks.fire_lower) / 100.0,\n'
           '        )\n'),
          (CLIENT / 'game/render_tick.rs',
           '            fire_lower: f32::from(self.settings.extras.pvp.tweaks.fire_lower) / 100.0,\n',
           '            fire_lower: horizon_client::native_ui::tweaks::fire_lower(\n'
           '                f32::from(self.settings.extras.pvp.tweaks.fire_lower) / 100.0,\n'
           '            ),\n'))

# Block Overlay and Damage Tint: the block outline and the hurt flash the renderer reads each frame.
patch_any((PVP,
           '    pub(in crate::game) fn pvp_render_frame(&self) -> Frame {\n'
           '        self.settings.extras.pvp.frame()\n',
           '    pub(in crate::game) fn pvp_render_frame(&self) -> Frame {\n'
           '        horizon_client::native_ui::tweaks::frame(self.settings.extras.pvp.frame())\n'),
          (CLIENT / 'game.rs',
           '            pvp: self.settings.extras.pvp.frame(),\n',
           '            pvp: horizon_client::native_ui::tweaks::frame(self.settings.extras.pvp.frame()),\n'))

# Time Changer, Weather Changer and the Crosshair: what PvP & HUD already does for the same things.
# (game/pvp.rs became game/extras/pvp.rs on 2026-11-13, its methods `pub(in crate::game)`.)
_patch = patch
if PVP.is_file():
    def patch(path, old, new):
        _patch(path, old.replace('pub(super) fn', 'pub(in crate::game) fn'),
               new.replace('pub(super) fn', 'pub(in crate::game) fn'))
else:
    PVP = CLIENT / 'game/pvp.rs'
patch(PVP,
      '        horizon_client::pvp::view_day_time(self.world.day_time, fixed.filter(|_| self.own_view()))\n',
      '        horizon_client::native_ui::tweaks::view_time(\n'
      '            self.world.day_time,\n'
      '            horizon_client::pvp::view_day_time(self.world.day_time, fixed.filter(|_| self.own_view())),\n'
      '        )\n')
patch(PVP,
      '    pub(super) fn view_weather(&self, partial: f32) -> (f32, f32) {\n',
      '    pub(super) fn view_weather(&self, partial: f32) -> (f32, f32) {\n'
      '        if let Some(w) = horizon_client::native_ui::tweaks::weather()\n'
      '            && self.can_have_weather()\n'
      '        {\n'
      '            return w;\n'
      '        }\n')
patch(PVP,
      '    pub(super) fn custom_crosshair(&self) -> bool {\n',
      '    pub(super) fn custom_crosshair(&self) -> bool {\n'
      '        if horizon_client::native_ui::tweaks::crosshair() {\n'
      '            return true;\n'
      '        }\n')
patch(PVP,
      '    pub(super) fn draw_custom_crosshair(&mut self, batch: &mut HudBatch, w: i32, h: i32) {\n',
      '    pub(super) fn draw_custom_crosshair(&mut self, batch: &mut HudBatch, w: i32, h: i32) {\n'
      '        if horizon_client::native_ui::tweaks::crosshair() {\n'
      '            return;\n'
      '        }\n')
patch = _patch

# Zoom and Freelook: the hooks native mods use.
HOOKS = CLIENT / 'game/native_hooks.rs'
ZOOM = '        mod_api::dispatch_zoom(self, mod_api::mods(), zoom);\n'
ZOOM_TYPE = 'NativeZoom'
POSE = '        mod_api::dispatch_camera_pose(self, mod_api::mods(), partial, position, yaw, pitch);\n'
if HOOKS.with_suffix('').is_dir() and not HOOKS.is_file():
    ZOOM = '        self.fire_zoom(zoom);\n'
    ZOOM_TYPE = 'mod_api::Zoom'
    POSE = '        self.fire_camera_pose(partial, position, yaw, pitch);\n'
patch(HOOKS, ZOOM, ZOOM +
      '        if zoom.is_none()\n'
      '            && let Some(z) = horizon_client::native_ui::tweaks::zoom()\n'
      '        {\n'
      f'            *zoom = Some({ZOOM_TYPE} {{\n'
      '                factor: z.factor,\n'
      '                smooth: z.smooth,\n'
      '                turn: z.turn,\n'
      '            });\n'
      '        }\n')
patch(HOOKS, POSE, POSE +
      '        if let Some((turn, tilt)) = horizon_client::native_ui::tweaks::get().freelook {\n'
      '            *yaw += turn;\n'
      '            *pitch = (*pitch + tilt).clamp(-90.0, 90.0);\n'
      '        }\n')
# Freelook: the mouse turns the camera, not the player.
FREELOOK = ('            self.mouse.on_move(0.0, 0.0, x, y, true, true);\n',
            '            if !horizon_client::native_ui::tweaks::freelook_turn(x, y) {\n'
            '                self.mouse.on_move(0.0, 0.0, x, y, true, true);\n'
            '            }\n')
# (game/window_handler.rs since 2026-11-13; game/repro.rs has a deeper copy for replays, which stays.)
patch_any((CLIENT / 'game/runtime/window_handler.rs', *FREELOOK),
          (CLIENT / 'game/window_handler.rs', *FREELOOK), (CLIENT / 'game.rs', *FREELOOK))

# Lag Free: the frame rate while the window is in the background (a hook that may only lower it).
LIMIT = ('        mod_api::dispatch_framerate_limit(\n'
         '            self,\n'
         '            mod_api::mods(),\n'
         '            focused,\n'
         '            minimized,\n'
         '            in_world,\n'
         '            limit,\n'
         '        );\n')
if not HOOKS.is_file():
    LIMIT = '        self.fire_framerate_limit(focused, minimized, in_world, limit);\n'
patch(HOOKS, LIMIT, LIMIT +
      '        if let Some(cap) = horizon_client::native_ui::tweaks::get().background_fps {\n'
      '            let cap = cap.max(1) as i32;\n'
      '            if *limit < 1 || cap < *limit {\n'
      '                *limit = cap;\n'
      '            }\n'
      '        }\n')

# Hitbox Colours: the F3+B boxes' white body and blue look direction.
GIZMOS = CLIENT / 'gizmos.rs'
patch(GIZMOS,
      '        self.cuboid(min, max, WHITE, STROKE_WIDTH);\n',
      '        self.cuboid(min, max, crate::native_ui::tweaks::hitbox(WHITE), STROKE_WIDTH);\n')
patch(GIZMOS,
      '        self.arrow(eye, end, BLUE, STROKE_WIDTH);\n',
      '        self.arrow(eye, end, crate::native_ui::tweaks::hitbox(BLUE), STROKE_WIDTH);\n')

# Scoreboard and Boss Bar: the sidebar and the bars the game is about to draw pass through Native, which
# remembers them, moves and scales them, or (for its own look) leaves them out and draws them itself.
SCORE = CLIENT / 'game/scoreboard.rs'
patch(SCORE,
      '        let draws = hud_overlays::sidebar(\n',
      '        let draws = horizon_client::native_ui::tweaks::sidebar_draws(\n'
      '            batch.width as i32,\n'
      '            batch.height as i32,\n'
      '            hud_overlays::sidebar(\n')
patch(SCORE,
      '            self.settings.text_background(0.4),\n'
      '        );\n',
      '            self.settings.text_background(0.4),\n'
      '            ),\n'
      '        );\n')
DRAGON = CLIENT / 'game/dragon.rs'
if not DRAGON.is_file():
    DRAGON = CLIENT / 'game/mobs/dragon.rs'
    BOSS = '        hud_overlays::boss_overlay(w, h, &bars, &self.gui.atlas.font, |index, at| {\n'
    patch(DRAGON, BOSS, '        let draws = ' + BOSS.lstrip())
    patch(DRAGON, '            !cancel\n        })\n    }\n    /// The dragon fight',
          '            !cancel\n        });\n'
          '        horizon_client::native_ui::tweaks::bars_draws(w, h, &bars, draws)\n'
          '    }\n    /// The dragon fight')
else:
    patch(DRAGON,
          '        let draws = hud_overlays::boss_overlay(\n',
          '        let draws = horizon_client::native_ui::tweaks::bars_draws(\n'
          '            batch.width as i32,\n'
          '            batch.height as i32,\n'
          '            &bars,\n'
          '            hud_overlays::boss_overlay(\n')
    patch(DRAGON,
          '            &bars,\n'
          '            &self.gui.atlas.font,\n'
          '        );\n',
          '            &bars,\n'
          '            &self.gui.atlas.font,\n'
          '            ),\n'
          '        );\n')

# Discord RPC: the module switches the game's Rich Presence off, and its custom status is the top line everywhere
# (menus, singleplayer, servers, hosting), with what the player does moved under it, as in the Java mod.
# Up to Nightly 2026-11-08 the presence is the engine's discord_presence.rs; since then it is the horizon-discord crate
# (upstream 01343707a), which reaches the Native UI's switches through horizon-client-render.
DISCORD = CRATES / 'horizon-discord/src/lib.rs'
if DISCORD.is_file():
    TWEAKS = 'horizon_client_render::native_ui::tweaks'
    patch(CRATES / 'horizon-discord/Cargo.toml',
          '[dependencies]\n',
          '[dependencies]\n'
          '# Native: the Discord RPC module\'s switch and custom status (native_ui::tweaks).\n'
          'horizon-client-render = { path = "../horizon-client-render" }\n')
    patch(DISCORD,
          '        if stop || wanted.status == Status::Off {\n',
          '        // Native: the Discord RPC module off clears the activity too; it is looked at again every 2 s.\n'
          f'        let native_off = {TWEAKS}::get().discord_off;\n'
          '        if stop || wanted.status == Status::Off || native_off {\n')
    patch(DISCORD,
          '            std::thread::park();\n'
          '            continue;\n',
          '            if native_off {\n'
          '                std::thread::park_timeout(Duration::from_secs(2));\n'
          '            } else {\n'
          '                std::thread::park();\n'
          '            }\n'
          '            continue;\n')
else:
    TWEAKS = 'crate::native_ui::tweaks'
    DISCORD = CLIENT / 'discord_presence.rs'
    patch(CLIENT / 'game/discord.rs',
          '        if !self.settings.extras.discord_presence {\n',
          '        if !self.settings.extras.discord_presence || horizon_client::native_ui::tweaks::get().discord_off {\n')
patch(DISCORD,
      '    let mut a = json!({"details": details, "assets": assets});\n'
      '    if let Some(state) = state {\n'
      '        a["state"] = state.into();\n'
      '    }\n',
      '    let mut a = json!({"details": details, "assets": assets});\n'
      '    if let Some(state) = state {\n'
      '        a["state"] = state.into();\n'
      '    }\n'
      f'    if let Some(status) = {TWEAKS}::discord_phrase() {{\n'
      '        with_status(&mut a, &status);\n'
      '    }\n')
patch(DISCORD,
      '/// The activity Discord shows for `status`; `None` clears it.\n',
      '/// Native: the custom `status` on top and what the player does under it ("Singleplayer · In the Nether").\n'
      '/// Discord refuses a line shorter than two characters, so such a status is left out.\n'
      'fn with_status(a: &mut Value, status: &str) {\n'
      '    let doing = a["details"].as_str().unwrap_or_default().trim_start_matches("Playing ").to_owned();\n'
      '    if status.chars().count() < 2 || doing == status {\n'
      '        return;\n'
      '    }\n'
      '    let line = match a.get("state").and_then(Value::as_str) {\n'
      '        Some(place) => format!("{doing} · {place}"),\n'
      '        None => doing,\n'
      '    };\n'
      '    a["details"] = status.into();\n'
      '    a["state"] = line.into();\n'
      '}\n'
      '\n'
      '/// The activity Discord shows for `status`; `None` clears it.\n')
# An edited status reaches Discord within seconds, not at the next minute's refresh: the thread looks every 2 s.
patch(DISCORD,
      '        let due = sent.is_none_or(|(w, at)| w != wanted || now - at >= REFRESH);\n'
      '        if !due {\n'
      '            let (_, at) = sent.expect("sent");\n'
      '            std::thread::park_timeout(REFRESH - (now - at));\n',
      f'        let phrase = {TWEAKS}::discord_phrase();\n'
      '        let due = sent_phrase != phrase || sent.is_none_or(|(w, at)| w != wanted || now - at >= REFRESH);\n'
      '        if !due {\n'
      '            let (_, at) = sent.expect("sent");\n'
      '            std::thread::park_timeout((REFRESH - (now - at)).min(Duration::from_secs(2)));\n')
patch(DISCORD,
      '    let mut sent: Option<(Wanted, Instant)> = None;\n',
      '    let mut sent: Option<(Wanted, Instant)> = None;\n'
      '    let mut sent_phrase: Option<String> = None;\n')
patch(DISCORD,
      '            Ok(()) => sent = Some((wanted, now)),\n',
      '            Ok(()) => {\n'
      '                sent = Some((wanted, now));\n'
      '                sent_phrase = phrase;\n'
      '            }\n')
patch(DISCORD,
      '    #[test]\n'
      '    fn time_counts_from_entering_the_world() {\n',
      '    #[test]\n'
      '    fn a_custom_status_is_the_top_line_and_keeps_what_the_player_does() {\n'
      '        let mut menus = json!({"details": "In the menus"});\n'
      '        with_status(&mut menus, "Grinding bedwars");\n'
      '        assert_eq!(menus["details"], "Grinding bedwars");\n'
      '        assert_eq!(menus["state"], "In the menus");\n'
      '        let mut single = json!({"details": "Playing Singleplayer", "state": "In the Nether"});\n'
      '        with_status(&mut single, "Speedrunning");\n'
      '        assert_eq!(single["details"], "Speedrunning");\n'
      '        assert_eq!(single["state"], "Singleplayer · In the Nether");\n'
      '        let mut short = json!({"details": "Playing Multiplayer", "state": "In the End"});\n'
      '        with_status(&mut short, "x");\n'
      '        assert_eq!(short["details"], "Playing Multiplayer");\n'
      '    }\n'
      '\n'
      '    #[test]\n'
      '    fn time_counts_from_entering_the_world() {\n')

# Custom pause menu: Native draws it, so the game's own is left out while it is on.
patch(CLIENT / 'game.rs',
      '            Screen::Paused if self.pause_menu => self.draw_pause_menu(&mut batch),\n',
      '            Screen::Paused if self.pause_menu && horizon_client::native_ui::tweaks::get().custom_pause => {}\n'
      '            Screen::Paused if self.pause_menu => self.draw_pause_menu(&mut batch),\n')

# Potion Effects: hide the vanilla top-right status effect icons while Native's module is on.
HIDE_EFFECTS = ('    pub(super) fn draw_effect_icons(&self, batch: &mut HudBatch) {\n',
                '    pub(super) fn draw_effect_icons(&self, batch: &mut HudBatch) {\n'
                '        if horizon_client::native_ui::tweaks::hide_effects() {\n'
                '            return;\n'
                '        }\n')
patch_any((CLIENT / 'game/items/potions.rs', *HIDE_EFFECTS),
          (CLIENT / 'game/potions.rs', *HIDE_EFFECTS))

# Discord: Native's own application (as the Java mod's presence uses), its logo and its words, not Horizon's.
# Use the same public artwork as the launcher and Java mod.
ICON = "https://playnative.net/brand/discord-icon.png"
patch(DISCORD, 'pub const DISCORD_APP_ID: &str = "1563857609738690560";\n',
      'pub const DISCORD_APP_ID: &str = "1465139441457827972";\n')
# rebrand.py (run first by the release build, not by the checks) has made "Horizon" "Native" here.
ASSETS = '    let mut assets = json!({"large_image": "%s", "large_text": "Native Client"});\n' % ICON
for brand in ('Native', 'Horizon'):
    old = '    let mut assets = json!({"large_image": "horizon", "large_text": "%s"});\n' % brand
    if old in DISCORD.read_text(encoding='utf-8'):
        patch(DISCORD, old, ASSETS)
if ASSETS not in DISCORD.read_text(encoding='utf-8'):
    raise SystemExit('native_tweaks: the Discord assets line changed upstream; update tools/native/native_tweaks.py.')
patch(DISCORD, '                Play::Singleplayer => ("Singleplayer", None),\n',
      '                Play::Singleplayer => ("Playing Singleplayer", None),\n')
patch(DISCORD, '                Play::Multiplayer => ("Multiplayer", None),\n',
      '                Play::Multiplayer => ("Playing Multiplayer", None),\n')
patch(DISCORD, '        assert_eq!(menus["assets"]["large_image"], "horizon");\n',
      '        assert_eq!(menus["assets"]["large_image"], "%s");\n' % ICON)
patch(DISCORD, '        assert_eq!(single["details"], "Singleplayer");\n',
      '        assert_eq!(single["details"], "Playing Singleplayer");\n')
patch(DISCORD, '        assert_eq!(multi["details"], "Multiplayer");\n',
      '        assert_eq!(multi["details"], "Playing Multiplayer");\n')

# Motion Blur: the module only set `tweaks::motion_blur`; nothing drew it. The world goes through world_post's own
# target while it is on, and each frame is blended into a history at the copy to the screen (native_world_blur.rs).
POST = RENDER / 'gfx/world_post.rs'
patch(POST,
      '        if self.scale == 1.0 && !self.antialias && !self.taa_on && !always {\n'
      '            self.target = None;\n'
      '            return None;\n'
      '        }\n',
      "        // Native Motion Blur blends frames in the world's own target: it needs one at any scale.\n"
      '        let always = always || native_blur_ready(device, window);\n'
      '        if self.scale == 1.0 && !self.antialias && !self.taa_on && !always {\n'
      '            self.target = None;\n'
      '            return None;\n'
      '        }\n')
text = POST.read_text(encoding='utf-8')
if 'fn native_finish(' not in text:
    TAA = '        self.finish_with(encoder, screen, &taa.show[i], timing);\n'
    # Since 2026-11-13 the TAA path draws its resolve through its own pipeline (sharpen when upscaling).
    SHOWN = '        self.draw_onto(encoder, screen, shown, pipeline, timing);\n'
    if TAA not in text and text.count(SHOWN) == 1:
        text = text.replace(SHOWN, '        let blurred = native_blur(self, encoder, shown);\n'
                                   '        self.draw_onto(encoder, screen, blurred.as_ref().unwrap_or(shown), pipeline, timing);\n')
        TAA = None
    copies = 1 if '    fn taa_frame(&self)' in text else 2
    for old, count in (('            self.finish_with(encoder, screen, &target.group, timing.as_deref_mut());\n', copies),
                       (TAA, 1)):
        if old is None:
            continue
        if text.count(old) != count:
            raise SystemExit(f'native_tweaks: expected {count} {old.strip()!r} in world_post.rs; update tools/native/native_tweaks.py.')
        text = text.replace(old, old.replace('self.finish_with(', 'self.native_finish('))
    text += '\n' + (pathlib.Path(sys.argv[2]).resolve() / 'packaging/native/native_world_blur.rs').read_text(encoding='utf-8')
    POST.write_text(text, encoding='utf-8')

# Resource-pack armour (a server pack's custom armour, a pack's own `<namespace>:equipment/<name>.json`): Java's
# EquipmentAssetManager reads the equipment assets of EVERY namespace and reloads them with the packs. Upstream reads
# only `minecraft` and the content mods' namespaces, at startup only, and packs only those namespaces' entity textures,
# so armour from a pack namespace was invisible on the body (its icon in the slot still showed).
ARMOR = CLIENT / 'game/entities/armor_stands.rs'
upstream_pack_armor = ARMOR.is_file() and 'EquipmentAssetManager reads resolved resources in every namespace' in ARMOR.read_text(encoding='utf-8')
if not upstream_pack_armor:
    patch(CLIENT / 'game/armor_stands.rs',
          '        let namespaces = std::iter::once("minecraft".to_owned())\n'
          '            .chain(horizon_client::mod_content::namespaces(root));\n',
          '        // Native: every namespace of the root, the resource packs\' own too (a server pack\'s armour).\n'
          '        let namespaces = std::iter::once("minecraft".to_owned()).chain(\n'
          '            std::fs::read_dir(root.join("assets"))\n'
          '                .into_iter()\n'
          '                .flatten()\n'
          '                .flatten()\n'
          '                .filter_map(|e| e.file_name().into_string().ok())\n'
          '                .filter(|n| n != "minecraft"),\n'
          '        );\n')
    patch(CLIENT / 'game/armor_stands.rs',
          '        let fresh = Self::load(root, None);\n'
          '        self.trim_overrides = fresh.trim_overrides;\n',
          '        let fresh = Self::load(root, None);\n'
          '        // Native: the packs\' equipment assets change with the packs, like the trims.\n'
          '        self.equipment = fresh.equipment;\n'
          '        self.trim_overrides = fresh.trim_overrides;\n')
    patch(RENDER / 'hud.rs',
          '        let assets = root.join("assets");\n'
          '        // A native mod\'s particle type names its textures `<ns>:particle/<path>`\n',
          '        // Native: every namespace\'s armour layers (`<ns>:entity/equipment/...`), which a resource pack\'s\n'
          '        // equipment assets name; the content mods\' are packed above with all their entity textures.\n'
          '        if folders.contains(&"entity")\n'
          '            && let Ok(all) = fs::read_dir(root.join("assets"))\n'
          '        {\n'
          '            for ns in all.flatten() {\n'
          '                let name = ns.file_name().to_string_lossy().into_owned();\n'
          '                if name != "minecraft" && !namespaces.contains(&name) {\n'
          '                    walk(&ns.path().join("textures/entity/equipment"), &mut files);\n'
          '                }\n'
          '            }\n'
          '        }\n'
          '        let assets = root.join("assets");\n'
          '        // A native mod\'s particle type names its textures `<ns>:particle/<path>`\n')
# Their tests: upstream's one that wanted pack namespaces unread now wants them read (when it is still there).
LOADING_TESTS = CLIENT / 'game/entity_model_loading_tests.rs'
UNREAD = ('    // Not a content root: other namespaces stay unread, as before.\n'
          '    assert!(ArmorStandAssets::load(&root.0, None).equipment.is_empty());\n')
if LOADING_TESTS.is_file():
    text = LOADING_TESTS.read_text(encoding='utf-8')
    if UNREAD in text:
        text = text.replace(UNREAD, '    // Native: like Java\'s EquipmentAssetManager, every namespace is read, content root or not.\n'
                                    '    assert!(ArmorStandAssets::load(&root.0, None).equipment.contains_key("gems:ruby"));\n')
    if 'fn native_resource_pack_equipment_loads_and_reloads(' not in text:
        text += '''
#[test]
fn native_resource_pack_equipment_loads_and_reloads() {
    let root = FixtureRoot::new();
    let equipment = root.0.join("assets/kimo/equipment");
    fs::create_dir_all(&equipment).unwrap();
    let frost = json!({"layers": {"humanoid": [{"texture": "kimo:frost"}]}});
    fs::write(equipment.join("frost.json"), serde_json::to_vec(&frost).unwrap()).unwrap();
    let mut assets = ArmorStandAssets::load(&root.0, None);
    assert_eq!(
        assets.equipment["kimo:frost"]["humanoid"],
        vec![("kimo:entity/equipment/humanoid/frost".into(), None)]
    );
    // a server pack applied later: its armour is there after the reload
    fs::write(equipment.join("ember.json"), serde_json::to_vec(&frost).unwrap()).unwrap();
    assets.reload_trims(&root.0);
    assert!(assets.equipment.contains_key("kimo:ember"));
}
'''
    LOADING_TESTS.write_text(text, encoding='utf-8')
patch(RENDER / 'hud.rs',
      '    /// A pack\'s particle sprite of any size packs whole, as vanilla\'s particle atlas stitches\n',
      '    #[test]\n'
      '    fn native_resource_pack_armour_packs_under_its_namespace() {\n'
      '        let root = std::env::temp_dir().join(format!("horizon-atlas-pack-armour-{}", std::process::id()));\n'
      '        let _ = fs::remove_dir_all(&root);\n'
      '        let equipment = root.join("assets/kimo/textures/entity/equipment/humanoid");\n'
      '        fs::create_dir_all(&equipment).unwrap();\n'
      '        write_strip(&equipment.join("frost.png"), 64, 32, &[[0, 120, 200, 255]], None);\n'
      '        let other = root.join("assets/kimo/textures/entity/creeper");\n'
      '        fs::create_dir_all(&other).unwrap();\n'
      '        write_strip(&other.join("big.png"), 64, 32, &[[9, 9, 9, 255]], None);\n'
      '        let atlas = HudAtlas::build(&horizon_assets::resource_source::ResourceSource::directory(root.clone())).unwrap();\n'
      '        let rect = atlas.sprites["kimo:entity/equipment/humanoid/frost"];\n'
      '        assert_eq!(texel_at(&atlas, rect, 0, 0), [0, 120, 200, 255]);\n'
      '        // only the armour layers of a namespace that is not a content mod\'s\n'
      '        assert!(!atlas.sprites.contains_key("kimo:entity/creeper/big"));\n'
      '        fs::remove_dir_all(&root).unwrap();\n'
      '    }\n'
      '    /// A pack\'s particle sprite of any size packs whole, as vanilla\'s particle atlas stitches\n')

print('native_tweaks: Native Client engine hooks applied')
