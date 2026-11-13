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
from native_moved import relocate

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
patch_any((CLIENT / 'game/extras/pvp.rs',
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
patch_any((CLIENT / 'game/extras/pvp.rs',
           '    pub(in crate::game) fn pvp_render_frame(&self) -> Frame {\n'
           '        self.settings.extras.pvp.frame()\n',
           '    pub(in crate::game) fn pvp_render_frame(&self) -> Frame {\n'
           '        horizon_client::native_ui::tweaks::frame(self.settings.extras.pvp.frame())\n'),
          (CLIENT / 'game.rs',
           '            pvp: self.settings.extras.pvp.frame(),\n',
           '            pvp: horizon_client::native_ui::tweaks::frame(self.settings.extras.pvp.frame()),\n'))

# Time Changer, Weather Changer and the Crosshair: what PvP & HUD already does for the same things.
# (game/pvp.rs became game/extras/pvp.rs on 2026-11-13, its methods `pub(in crate::game)`.)
PVP = CLIENT / 'game/extras/pvp.rs'
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
FREELOOK = ('            self.mouse.on_move(0.0, 0.0, x, y, true, true);\n',
            '            if !horizon_client::native_ui::tweaks::freelook_turn(x, y) {\n'
            '                self.mouse.on_move(0.0, 0.0, x, y, true, true);\n'
            '            }\n')
# (game/window_handler.rs since 2026-11-13; game/repro.rs has a deeper copy for replays, which stays.)
patch_any((CLIENT / 'game/window_handler.rs', *FREELOOK), (CLIENT / 'game.rs', *FREELOOK))

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

# Discord: Native's own application (as the Java mod's presence uses), its logo and its words, not Horizon's.
# The logo is the Native Discord icon (the Native Cape's look), hosted on the Native API: Discord loads image URLs directly.
ICON = "https://api.playnative.net/v1/site/media/e3c3ffebf47300fe60c82456.png"
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
