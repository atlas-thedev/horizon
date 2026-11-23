//! Quality-of-life modules: Stopwatch, Timer and Durability Alert. The Java mod has the same three
//! (`QolModules.java`): same ids, settings and behaviour.

use super::module::*;
use super::setting::Setting;
use horizon_client::native_ui::canvas::clock_ms;
use horizon_client::native_ui::theme;

/// GLFW K and O: free in vanilla's default controls and in Native's.
pub const KEY_K: i32 = 75;
pub const KEY_O: i32 = 79;
/// Holding a module's key this long resets it instead of starting / pausing.
const HOLD_MS: f64 = 600.0;
/// How long "Time's up" stays on screen.
const ALERT_MS: f64 = 10_000.0;

pub fn all() -> Vec<Module> {
    vec![stopwatch(), timer(), durability_alert()]
}

/// "1:05.3", "1:02:07.9"; without tenths "1:05".
pub fn clock(ms: f64, tenths: bool) -> String {
    let ms = ms.max(0.0) as u64;
    let s = ms / 1000;
    let (h, m, sec) = (s / 3600, (s / 60) % 60, s % 60);
    let t = if h > 0 { format!("{h}:{m:02}:{sec:02}") } else { format!("{m}:{sec:02}") };
    if tenths { format!("{t}.{}", (ms % 1000) / 100) } else { t }
}

/// A key that starts / pauses on a tap and resets when held: what happened to it this frame.
#[derive(Default)]
struct TapOrHold {
    pressed_at: Option<f64>,
    held: bool,
}

#[derive(PartialEq, Debug)]
enum Press {
    None,
    Tap,
    Hold,
}

impl TapOrHold {
    fn update(&mut self, down: bool, now: f64) -> Press {
        match (down, self.pressed_at) {
            (true, None) => {
                self.pressed_at = Some(now);
                self.held = false;
                Press::None
            }
            (true, Some(at)) if !self.held && now - at >= HOLD_MS => {
                self.held = true;
                Press::Hold
            }
            (false, Some(_)) => {
                self.pressed_at = None;
                if self.held { Press::None } else { Press::Tap }
            }
            _ => Press::None,
        }
    }
}

fn key_down(m: &ModuleState, g: &GameView) -> bool {
    let key = m.setting("key").map_or(-1, Setting::as_key);
    g.playing && !g.sample && g.is_down(key)
}

// ── stopwatch ───────────────────────────────────────────────────────────

fn stopwatch() -> Module {
    Module::hud(
        "stopwatch",
        "Stopwatch",
        "Times anything: tap its key to start and stop, hold it to reset.",
        theme::I_TIMER,
        false,
        0.0,
        0.4,
        vec![
            Setting::key("key", "Start / stop key (hold: reset)", KEY_K),
            Setting::bool("tenths", "Show tenths of a second", true),
        ],
        Box::new(Stopwatch::default()),
    )
}

#[derive(Default)]
struct Stopwatch {
    key: TapOrHold,
    since: f64,
    banked: f64,
    running: bool,
}

impl Stopwatch {
    fn elapsed(&self, now: f64) -> f64 {
        self.banked + if self.running { now - self.since } else { 0.0 }
    }
}

impl Behaviour for Stopwatch {
    fn frame(&mut self, m: &ModuleState, g: &GameView) {
        let now = clock_ms();
        match self.key.update(key_down(m, g), now) {
            Press::Tap if self.running => {
                self.banked += now - self.since;
                self.running = false;
            }
            Press::Tap => {
                self.since = now;
                self.running = true;
            }
            Press::Hold => {
                self.running = false;
                self.banked = 0.0;
            }
            Press::None => {}
        }
    }

    fn lines(&mut self, m: &ModuleState, g: &GameView) -> Vec<Line> {
        let ms = if g.sample { 83_400.0 } else { self.elapsed(clock_ms()) };
        vec![Line::new("Stopwatch", clock(ms, m.bool("tenths")))]
    }
}

// ── timer ───────────────────────────────────────────────────────────────

fn timer() -> Module {
    Module::hud(
        "timer",
        "Timer",
        "Counts down from the time you set and tells you when it is up: tap its key to start or pause, hold it to reset.",
        theme::I_BELL,
        false,
        0.0,
        0.46,
        vec![
            Setting::num("minutes", "Minutes", 5.0, 0.0, 180.0, 1.0, " min"),
            Setting::num("seconds", "Seconds", 0.0, 0.0, 55.0, 5.0, " s"),
            Setting::key("key", "Start / pause key (hold: reset)", KEY_O),
        ],
        Box::new(Timer::default()),
    )
}

#[derive(Default)]
struct Timer {
    key: TapOrHold,
    /// When it reaches zero while running.
    ends_at: Option<f64>,
    /// Time left while paused; `None` reset.
    left: Option<f64>,
    done_at: Option<f64>,
}

impl Timer {
    fn length(m: &ModuleState) -> f64 {
        (f64::from(m.num("minutes")) * 60.0 + f64::from(m.num("seconds"))) * 1000.0
    }

    fn remaining(&self, m: &ModuleState, now: f64) -> f64 {
        match (self.ends_at, self.left) {
            (Some(end), _) => (end - now).max(0.0),
            (None, Some(left)) => left,
            (None, None) => Self::length(m),
        }
    }
}

impl Behaviour for Timer {
    fn frame(&mut self, m: &ModuleState, g: &GameView) {
        let now = clock_ms();
        match self.key.update(key_down(m, g), now) {
            Press::Tap => {
                if let Some(end) = self.ends_at.take() {
                    self.left = Some((end - now).max(0.0)); // pause
                } else {
                    let left = self.remaining(m, now);
                    if left > 0.0 {
                        self.ends_at = Some(now + left); // start or carry on
                        self.done_at = None;
                    }
                }
            }
            Press::Hold => {
                self.ends_at = None;
                self.left = None;
                self.done_at = None;
            }
            Press::None => {}
        }
        if self.ends_at.is_some_and(|end| now >= end) {
            self.ends_at = None;
            self.left = Some(0.0);
            self.done_at = Some(now);
        }
    }

    fn lines(&mut self, m: &ModuleState, g: &GameView) -> Vec<Line> {
        let now = clock_ms();
        let value = if g.sample {
            clock(272_000.0, false)
        } else if let Some(done) = self.done_at.filter(|d| now - d < ALERT_MS) {
            // blinks
            if ((now - done) / 500.0) as u64 % 2 == 0 { "Time's up!".to_owned() } else { "0:00".to_owned() }
        } else {
            // 4:59.2 left reads 5:00, as a kitchen timer does
            clock(self.remaining(m, now) + 999.0, false)
        };
        vec![Line::new("Timer", value)]
    }
}

// ── durability alert ────────────────────────────────────────────────────

fn durability_alert() -> Module {
    Module::hud(
        "durability-alert",
        "Durability Alert",
        "Warns you before a tool, weapon or piece of armor breaks.",
        theme::I_SHIELD,
        false,
        0.5,
        0.62,
        vec![
            Setting::num("threshold", "Warn below", 10.0, 1.0, 50.0, 1.0, "%"),
            Setting::bool("armor", "Check worn armor", true),
            Setting::bool("hands", "Check the items in your hands", true),
        ],
        Box::new(DurabilityAlert),
    )
}

struct DurabilityAlert;

/// The worn-out items to show, in the order hand, offhand, head to feet.
fn worn_out<'a>(m: &ModuleState, g: &'a GameView) -> Vec<&'a StackView> {
    let limit = m.num("threshold") / 100.0;
    let hands = [&g.vitals.hand, &g.vitals.offhand].into_iter().filter(|_| m.bool("hands"));
    let armor = g.vitals.armor.iter().filter(|_| m.bool("armor"));
    hands
        .chain(armor)
        .filter_map(Option::as_ref)
        .filter(|s| s.max > 0 && s.durability() <= limit)
        .take(6)
        .collect()
}

/// "minecraft:diamond_pickaxe" -> "Diamond Pickaxe".
pub fn pretty(id: &str) -> String {
    let bare = id.strip_prefix("minecraft:").unwrap_or(id);
    bare.split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

impl Behaviour for DurabilityAlert {
    fn needs_vitals(&self) -> bool {
        true
    }

    fn visible(&self, m: &ModuleState, g: &GameView) -> bool {
        g.sample || !worn_out(m, g).is_empty()
    }

    fn lines(&mut self, m: &ModuleState, g: &GameView) -> Vec<Line> {
        if g.sample {
            return vec![Line::new("Diamond Pickaxe", "6% · 94 left")];
        }
        worn_out(m, g)
            .into_iter()
            .map(|s| {
                let pct = (s.durability() * 100.0).round() as u32;
                Line::new(pretty(&s.id), format!("{pct}% · {} left", s.max.saturating_sub(s.damage)))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_read_like_the_java_mod() {
        assert_eq!(clock(0.0, true), "0:00.0");
        assert_eq!(clock(83_400.0, true), "1:23.4");
        assert_eq!(clock(3_727_900.0, true), "1:02:07.9");
        assert_eq!(clock(65_000.0, false), "1:05");
    }

    #[test]
    fn a_tap_toggles_and_a_hold_resets() {
        let mut k = TapOrHold::default();
        assert_eq!(k.update(true, 0.0), Press::None);
        assert_eq!(k.update(false, 100.0), Press::Tap);
        assert_eq!(k.update(true, 1000.0), Press::None);
        assert_eq!(k.update(true, 1400.0), Press::None);
        assert_eq!(k.update(true, 1700.0), Press::Hold);
        assert_eq!(k.update(true, 2500.0), Press::None); // once per hold
        assert_eq!(k.update(false, 2600.0), Press::None); // releasing a hold is not a tap
    }

    #[test]
    fn item_names_are_readable() {
        assert_eq!(pretty("minecraft:diamond_pickaxe"), "Diamond Pickaxe");
        assert_eq!(pretty("elytra"), "Elytra");
    }
}
