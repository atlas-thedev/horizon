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

root = pathlib.Path(sys.argv[1]).resolve()
CRATES = root / 'crates'
RENDER = CRATES / 'horizon-client-render/src'
CLIENT = CRATES / 'horizon-client/src'


def patch(path, old, new):
    """Replace `old` (exactly once) with `new`; fine if it was done already."""
    text = path.read_text(encoding='utf-8')
    if new in text:
        return
    if text.count(old) != 1:
        raise SystemExit(f'native_tweaks: expected one {old[:70]!r} in {path.relative_to(root)}, '
                         f'found {text.count(old)}. Upstream changed; update tools/native/native_tweaks.py.')
    path.write_text(text.replace(old, new), encoding='utf-8')


if not (RENDER / 'native_ui/tweaks.rs').is_file():
    raise SystemExit('native_tweaks: run native_ui.py first.')

# Fullbright: the module's brightness over the slider's, read every frame (never saved to options.txt); at full
# strength every light level is drawn at full brightness (Java's gamma 16).
patch(RENDER / 'gfx.rs',
      '            (self.gamma - frame.darkness.0).max(0.0),\n',
      '            (crate::native_ui::tweaks::gamma(self.gamma) - frame.darkness.0).max(0.0),\n')
patch(RENDER / 'gfx.rs',
      '        if self.fullbright {\n'
      '            pixels.fill(255);\n',
      '        if self.fullbright || crate::native_ui::tweaks::fullbright() {\n'
      '            pixels.fill(255);\n')

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
patch(CLIENT / 'game/render_tick.rs',
      '            fire_lower: f32::from(self.settings.extras.pvp.tweaks.fire_lower) / 100.0,\n',
      '            fire_lower: horizon_client::native_ui::tweaks::fire_lower(\n'
      '                f32::from(self.settings.extras.pvp.tweaks.fire_lower) / 100.0,\n'
      '            ),\n')

# Block Overlay and Damage Tint: the block outline and the hurt flash the renderer reads each frame.
patch(CLIENT / 'game.rs',
      '            pvp: self.settings.extras.pvp.frame(),\n',
      '            pvp: horizon_client::native_ui::tweaks::frame(self.settings.extras.pvp.frame()),\n')

# Time Changer, Weather Changer and the Crosshair: what PvP & HUD already does for the same things.
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

# Zoom and Freelook: the hooks native mods use.
HOOKS = CLIENT / 'game/native_hooks.rs'
patch(HOOKS,
      '        mod_api::dispatch_zoom(self, mod_api::mods(), zoom);\n',
      '        mod_api::dispatch_zoom(self, mod_api::mods(), zoom);\n'
      '        if zoom.is_none()\n'
      '            && let Some(z) = horizon_client::native_ui::tweaks::zoom()\n'
      '        {\n'
      '            *zoom = Some(NativeZoom {\n'
      '                factor: z.factor,\n'
      '                smooth: z.smooth,\n'
      '                turn: z.turn,\n'
      '            });\n'
      '        }\n')
patch(HOOKS,
      '        mod_api::dispatch_camera_pose(self, mod_api::mods(), partial, position, yaw, pitch);\n',
      '        mod_api::dispatch_camera_pose(self, mod_api::mods(), partial, position, yaw, pitch);\n'
      '        if let Some((turn, tilt)) = horizon_client::native_ui::tweaks::get().freelook {\n'
      '            *yaw += turn;\n'
      '            *pitch = (*pitch + tilt).clamp(-90.0, 90.0);\n'
      '        }\n')
# Freelook: the mouse turns the camera, not the player.
patch(CLIENT / 'game.rs',
      '            self.mouse.on_move(0.0, 0.0, x, y, true, true);\n',
      '            if !horizon_client::native_ui::tweaks::freelook_turn(x, y) {\n'
      '                self.mouse.on_move(0.0, 0.0, x, y, true, true);\n'
      '            }\n')

# Lag Free: the frame rate while the window is in the background (a hook that may only lower it).
patch(HOOKS,
      '        mod_api::dispatch_framerate_limit(\n'
      '            self,\n'
      '            mod_api::mods(),\n'
      '            focused,\n'
      '            minimized,\n'
      '            in_world,\n'
      '            limit,\n'
      '        );\n',
      '        mod_api::dispatch_framerate_limit(\n'
      '            self,\n'
      '            mod_api::mods(),\n'
      '            focused,\n'
      '            minimized,\n'
      '            in_world,\n'
      '            limit,\n'
      '        );\n'
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

# Discord RPC: the module switches the game's Rich Presence off, and words what a server shows as.
patch(CLIENT / 'game/discord.rs',
      '        if !self.settings.extras.discord_presence {\n',
      '        if !self.settings.extras.discord_presence || horizon_client::native_ui::tweaks::get().discord_off {\n')
patch(CLIENT / 'discord_presence.rs',
      '    let mut a = json!({"details": details, "assets": assets});\n',
      '    let mut a = json!({"details": details, "assets": assets});\n'
      '    if let Status::World { play: Play::Multiplayer, .. } = status\n'
      '        && let Some(phrase) = crate::native_ui::tweaks::discord_phrase()\n'
      '    {\n'
      '        a["details"] = phrase.into();\n'
      '    }\n')

# Custom pause menu: Native draws it, so the game's own is left out while it is on.
patch(CLIENT / 'game.rs',
      '            Screen::Paused if self.pause_menu => self.draw_pause_menu(&mut batch),\n',
      '            Screen::Paused if self.pause_menu && horizon_client::native_ui::tweaks::get().custom_pause => {}\n'
      '            Screen::Paused if self.pause_menu => self.draw_pause_menu(&mut batch),\n')

# Discord: Native's own application (as the Java mod's presence uses), its logo and its words, not Horizon's.
# The logo is the Native Discord icon (the Native Cape's look), hosted on the Native API: Discord loads image URLs directly.
ICON = "https://api.playnative.net/v1/site/media/0b3a3c4adde07695da039ecd.png"
DISCORD = CLIENT / 'discord_presence.rs'
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

print('native_tweaks: Native Client engine hooks applied')
