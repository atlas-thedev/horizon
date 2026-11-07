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
    /// Scoreboard and Boss Bar in the Native look: the game's own are left out.
    pub hide_sidebar: bool,
    pub hide_bossbars: bool,
    /// Discord RPC is switched off: the game shows no Rich Presence.
    pub discord_off: bool,
    /// Native draws the pause menu: the game's own is left out.
    pub custom_pause: bool,
    /// Where the game's own sidebar / boss bars go: a shift in GUI px, then a scale.
    pub sidebar_move: Option<[f32; 3]>,
    pub boss_move: Option<[f32; 3]>,
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
    hide_sidebar: false,
    hide_bossbars: false,
    discord_off: false,
    custom_pause: false,
    sidebar_move: None,
    boss_move: None,
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

static PHRASE: Mutex<Option<String>> = Mutex::new(None);

/// Discord RPC: the words to show instead of "Multiplayer" while on a server.
pub fn set_discord_phrase(phrase: Option<String>) {
    *PHRASE.lock().unwrap_or_else(|e| e.into_inner()) = phrase;
}

pub fn discord_phrase() -> Option<String> {
    PHRASE.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

// ── what the game shows on the HUD, for the modules that redraw or move it ──

/// Styled text as runs of (text, ARGB), colour codes already resolved.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rich(pub Vec<(String, u32)>);

impl Rich {
    fn of(text: &crate::text::Text) -> Rich {
        Rich(
            text.spans
                .iter()
                .filter(|s| !s.text.is_empty())
                .map(|s| (s.text.clone(), 0xFF00_0000 | s.style.color.unwrap_or(0xFF_FFFF)))
                .collect(),
        )
    }

    /// Nothing but spaces.
    pub fn is_blank(&self) -> bool {
        self.0.iter().all(|(t, _)| t.trim().is_empty())
    }
}

/// The server sidebar: its title and its rows as (name, score), top first, and where the game draws it
/// (`bounds` = left, top, right, bottom in GUI px; `gui` = the GUI's size).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SidebarView {
    pub title: Rich,
    pub rows: Vec<(Rich, Rich)>,
    pub bounds: [f32; 4],
    pub gui: [f32; 2],
}

/// One boss bar.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BarView {
    pub name: Rich,
    pub progress: f32,
    /// Pink, blue, red, green, yellow, purple, white.
    pub color: usize,
    /// Width of the name in GUI px is not known here; the bar itself is 182 wide.
    pub name_len: usize,
}

/// All the bars, and where the game draws them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BarsView {
    pub bars: Vec<BarView>,
    pub bounds: [f32; 4],
    pub gui: [f32; 2],
}

struct Seen<T> {
    at: Option<std::time::Instant>,
    value: T,
}

static SIDEBAR: Mutex<Seen<SidebarView>> = Mutex::new(Seen {
    at: None,
    value: SidebarView { title: Rich(Vec::new()), rows: Vec::new(), bounds: [0.0; 4], gui: [0.0; 2] },
});
static BARS: Mutex<Seen<BarsView>> = Mutex::new(Seen { at: None, value: BarsView { bars: Vec::new(), bounds: [0.0; 4], gui: [0.0; 2] } });

/// A view the game published within this long ago is on screen.
const FRESH: std::time::Duration = std::time::Duration::from_millis(400);

/// `draws` shifted by (`x`, `y`) GUI px and scaled by `s` about the GUI's origin.
fn placed(draws: Vec<crate::hud_layout::GuiDraw>, m: [f32; 3]) -> Vec<crate::hud_layout::GuiDraw> {
    use crate::hud_layout::{GuiDraw, PoseOp};
    let mut out = Vec::with_capacity(draws.len() + 2);
    out.push(GuiDraw::Push(vec![PoseOp::Translate(m[0], m[1]), PoseOp::Scale(m[2], m[2])]));
    out.extend(draws);
    out.push(GuiDraw::Pop);
    out
}

/// What `Hud.displayScoreboardSidebar` is about to draw, remembered for Scoreboard. Returns what the game
/// should draw: the same, moved and scaled, or nothing when Native draws it.
pub fn sidebar_draws(w: i32, h: i32, draws: Vec<crate::hud_layout::GuiDraw>) -> Vec<crate::hud_layout::GuiDraw> {
    use crate::hud_layout::GuiDraw;
    let mut bounds: Option<[f32; 4]> = None;
    let mut texts = Vec::new();
    for d in &draws {
        match d {
            GuiDraw::Fill { x0, y0, x1, y1, .. } => {
                let b = bounds.get_or_insert([*x0 as f32, *y0 as f32, *x1 as f32, *y1 as f32]);
                *b = [b[0].min(*x0 as f32), b[1].min(*y0 as f32), b[2].max(*x1 as f32), b[3].max(*y1 as f32)];
            }
            GuiDraw::Text { text, .. } => texts.push(Rich::of(text)),
            _ => {}
        }
    }
    let mut texts = texts.into_iter();
    let title = texts.next().unwrap_or_default();
    let mut rows = Vec::new();
    while let Some(name) = texts.next() {
        rows.push((name, texts.next().unwrap_or_default()));
    }
    {
        let mut s = SIDEBAR.lock().unwrap_or_else(|e| e.into_inner());
        s.value = SidebarView { title, rows, bounds: bounds.unwrap_or_default(), gui: [w as f32, h as f32] };
        s.at = Some(std::time::Instant::now());
    }
    let t = get();
    if t.hide_sidebar {
        Vec::new()
    } else if let Some(m) = t.sidebar_move {
        placed(draws, m)
    } else {
        draws
    }
}

/// The sidebar on screen now, `None` when there is none.
pub fn sidebar_view() -> Option<SidebarView> {
    let s = SIDEBAR.lock().unwrap_or_else(|e| e.into_inner());
    s.at.filter(|t| t.elapsed() < FRESH).map(|_| s.value.clone())
}

/// The boss bars `BossHealthOverlay` is about to draw: remembered for Boss Bar, and moved, scaled or left out.
pub fn bars_draws(w: i32, h: i32, bars: &[crate::hud_overlays::BossBar], draws: Vec<crate::hud_layout::GuiDraw>) -> Vec<crate::hud_layout::GuiDraw> {
    // The bars the game fits above a third of the screen (19 px each, the first at 12).
    let (mut shown, mut y) = (0, 12);
    for _ in bars {
        shown += 1;
        y += 19;
        if y >= h / 3 {
            break;
        }
    }
    let views: Vec<BarView> = bars
        .iter()
        .map(|b| BarView { name: Rich::of(&b.name), progress: b.progress, color: b.color, name_len: b.name.plain().chars().count() })
        .collect();
    {
        let mut s = BARS.lock().unwrap_or_else(|e| e.into_inner());
        s.value = BarsView {
            bars: views,
            bounds: [(w / 2 - 91) as f32, 2.0, (w / 2 + 91) as f32, (2 + shown * 19 - 4) as f32],
            gui: [w as f32, h as f32],
        };
        s.at = Some(std::time::Instant::now());
    }
    let t = get();
    if t.hide_bossbars {
        Vec::new()
    } else if let Some(m) = t.boss_move {
        placed(draws, m)
    } else {
        draws
    }
}

/// The boss bars on screen now (none when there are none).
pub fn bars_view() -> BarsView {
    let s = BARS.lock().unwrap_or_else(|e| e.into_inner());
    if s.at.is_some_and(|t| t.elapsed() < FRESH) { s.value.clone() } else { BarsView::default() }
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
