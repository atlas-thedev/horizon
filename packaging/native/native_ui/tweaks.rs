//! What Native Client's modules ask of the engine: zoom, brightness, the
//! first-person fire, the hurt flash, the block outline, the time and weather
//! this player sees, the camera. The modules write here once a frame; the
//! engine reads these where `tools/native/native_tweaks.py` hooked it.
//! Everything is off (`None`) until a module is switched on, so the game is
//! vanilla otherwise.

use crate::pvp::Frame;
use std::sync::Mutex;

/// A zoom factor, whether it eases, and how much the mouse turns while zoomed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zoom {
    pub factor: f32,
    pub smooth: bool,
    pub turn: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tweaks {
    /// Fullbright: the brightness slider's value.
    pub gamma: Option<f32>,
    /// Low Fire: how far the fire overlay is moved down, in blocks.
    pub fire_lower: Option<f32>,
    /// Damage Tint: the hurt overlay texel (see `pvp::Hit::texel`).
    pub hurt_texel: Option<u32>,
    /// Block Overlay: the outline's ARGB and width as a multiple of vanilla's.
    pub outline: Option<(u32, f32)>,
    /// Time Changer: the time of day to show, and whether time keeps passing from there.
    pub time: Option<TimeView>,
    /// Weather Changer: the rain and thunder levels to show.
    pub weather: Option<(f32, f32)>,
    /// Crosshair: Native draws its own; the game's is left out.
    pub crosshair: bool,
    pub zoom: Option<Zoom>,
    /// Freelook: the camera's yaw and pitch offsets from where the player faces, in degrees.
    pub freelook: Option<(f32, f32)>,
    pub no_hurt_cam: bool,
    /// Toggle Sprint / Toggle Sneak: latch the sprint and sneak keys.
    pub toggle_sprint: bool,
    pub toggle_sneak: bool,
    /// Freelook: how fast the camera follows the mouse (1 is normal).
    pub freelook_sens: f32,
    /// Hitbox Colours: the body and look-direction ARGB.
    pub hitbox: Option<(u32, u32)>,
    /// Critical Colours: the critical-hit and enchanted-hit ARGB.
    pub critical: Option<(u32, u32)>,
    /// Motion Blur: how much of the last frame stays, per 1/60 s (0 to 1).
    pub motion_blur: Option<f32>,
    /// Lag Free: the frame rate cap while the window is in the background.
    pub background_fps: Option<u32>,
}

/// The time Time Changer shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TimeView {
    /// Always this time of day.
    Fixed(i64),
    /// This time of day now, passing as it does in the world: the offset from the world's time.
    Flow(i64),
}

static TWEAKS: Mutex<Tweaks> = Mutex::new(Tweaks {
    gamma: None,
    fire_lower: None,
    hurt_texel: None,
    outline: None,
    time: None,
    weather: None,
    crosshair: false,
    zoom: None,
    freelook: None,
    no_hurt_cam: false,
    toggle_sprint: false,
    toggle_sneak: false,
    freelook_sens: 1.0,
    hitbox: None,
    critical: None,
    motion_blur: None,
    background_fps: None,
});

/// What the modules want right now.
pub fn get() -> Tweaks {
    *TWEAKS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Changes what the modules want.
pub fn set(f: impl FnOnce(&mut Tweaks)) {
    f(&mut TWEAKS.lock().unwrap_or_else(|e| e.into_inner()));
}

pub fn gamma(base: f32) -> f32 {
    get().gamma.unwrap_or(base)
}

pub fn fire_lower(base: f32) -> f32 {
    get().fire_lower.unwrap_or(base)
}

pub fn no_hurt_cam() -> bool {
    get().no_hurt_cam
}

pub fn crosshair() -> bool {
    get().crosshair
}

pub fn zoom() -> Option<Zoom> {
    get().zoom
}

pub fn weather() -> Option<(f32, f32)> {
    get().weather
}

/// The block outline and hurt flash the renderer reads each frame, with Native's over vanilla's.
pub fn frame(mut f: Frame) -> Frame {
    let t = get();
    if let Some(o) = t.outline {
        f.outline = Some(o);
    }
    if let Some(h) = t.hurt_texel {
        f.hurt_texel = h;
    }
    f
}

/// The time of day this player sees: Time Changer's, else `base` (the world's, or PvP & HUD's).
pub fn view_time(world: i64, base: i64) -> i64 {
    match get().time {
        Some(TimeView::Fixed(t)) => world.div_euclid(24000) * 24000 + t,
        Some(TimeView::Flow(offset)) => world + offset,
        None => base,
    }
}

/// Freelook is on and the mouse moved by `dx`, `dy`: the camera turns, the player does not. True when taken.
pub fn freelook_turn(dx: f64, dy: f64) -> bool {
    let mut t = TWEAKS.lock().unwrap_or_else(|e| e.into_inner());
    let Some((yaw, pitch)) = t.freelook else { return false };
    let k = 0.15 * t.freelook_sens;
    t.freelook = Some((yaw + dx as f32 * k, (pitch + dy as f32 * k).clamp(-180.0, 180.0)));
    true
}

/// The hitbox colour the debug view draws in place of `vanilla` (ARGB): white body, blue look direction.
pub fn hitbox(vanilla: u32) -> u32 {
    match (get().hitbox, vanilla) {
        (Some((body, _)), 0xFFFFFFFF) => 0xFF000000 | (body & 0xFFFFFF),
        (Some((_, dir)), 0xFF0000FF) => 0xFF000000 | (dir & 0xFFFFFF),
        _ => vanilla,
    }
}

/// The critical-hit particle colour as 0..1 channels, when Critical Colours is on.
pub fn critical(enchanted: bool) -> Option<[f32; 3]> {
    let (normal, magic) = get().critical?;
    let argb = if enchanted { magic } else { normal };
    Some([
        ((argb >> 16) & 255) as f32 / 255.0,
        ((argb >> 8) & 255) as f32 / 255.0,
        (argb & 255) as f32 / 255.0,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_changes_until_a_module_asks() {
        // The tweaks are one process-wide value: this test sets and clears its own fields only.
        assert_eq!(gamma(0.5), get().gamma.unwrap_or(0.5));
        set(|t| t.time = Some(TimeView::Fixed(6000)));
        assert_eq!(view_time(24000 * 3 + 100, 7), 24000 * 3 + 6000);
        set(|t| t.time = Some(TimeView::Flow(-500)));
        assert_eq!(view_time(1000, 7), 500);
        set(|t| t.time = None);
        assert_eq!(view_time(1000, 7), 7);
    }

    #[test]
    fn the_hitbox_colours_only_replace_the_two_native_ones() {
        set(|t| t.hitbox = Some((0x112233, 0x445566)));
        assert_eq!(hitbox(0xFFFFFFFF), 0xFF112233);
        assert_eq!(hitbox(0xFF0000FF), 0xFF445566);
        assert_eq!(hitbox(0xFFFF0000), 0xFFFF0000);
        set(|t| t.hitbox = None);
        assert_eq!(hitbox(0xFFFFFFFF), 0xFFFFFFFF);
    }
}
