//! `Theme`: colours and icon codepoints taken from the Native launcher (dark,
//! monochrome, soft hairlines). Colours are ARGB, as in the Java mod.
#![allow(dead_code)]

pub const PAGE: u32 = 0xFF000000;
pub const SURFACE: u32 = 0xFF050608;
pub const PANEL: u32 = 0xFF08090C;
pub const PANEL_HOVER: u32 = 0xFF0E1014;
pub const PANEL_PRESSED: u32 = 0xFF14171D;
/// 8 %.
pub const HAIRLINE: u32 = 0x14FFFFFF;
/// 16 %.
pub const HAIRLINE_STRONG: u32 = 0x29FFFFFF;
pub const BORDER_HOVER: u32 = 0x47FFFFFF;
/// 6 %.
pub const SUBTLE: u32 = 0x0FFFFFFF;
/// 12 %.
pub const SUBTLE_HOVER: u32 = 0x1FFFFFFF;
pub const TEXT: u32 = 0xFFD2D2D2;
pub const TEXT_STRONG: u32 = 0xFFFFFFFF;
pub const TEXT_SECONDARY: u32 = 0xFFA1A1AA;
pub const TEXT_MUTED: u32 = 0xFF75717A;
pub const ACCENT: u32 = 0xFFD9A6DA;
pub const ONLINE: u32 = 0xFF22C55E;
pub const IDLE: u32 = 0xFFF59E0B;
pub const DANGER: u32 = 0xFFEF4444;
pub const WHITE: u32 = 0xFFFFFFFF;
pub const SOLID_FG: u32 = 0xFF09090B;

// Lucide icon codepoints.
pub const I_MESSAGE: u32 = 0xE11A;
pub const I_USERS: u32 = 0xE1A4;
pub const I_USER_PLUS: u32 = 0xE1A2;
pub const I_SEND: u32 = 0xE156;
pub const I_SETTINGS: u32 = 0xE158;
pub const I_X: u32 = 0xE1B2;
pub const I_SEARCH: u32 = 0xE155;
pub const I_HASH: u32 = 0xE0F2;
pub const I_CHEVRON_LEFT: u32 = 0xE072;
pub const I_CHEVRON_RIGHT: u32 = 0xE073;
pub const I_GLOBE: u32 = 0xE0EB;
pub const I_GAMEPAD: u32 = 0xE0E2;
pub const I_LOG_OUT: u32 = 0xE112;
pub const I_PLAY: u32 = 0xE140;
pub const I_SERVER: u32 = 0xE157;
pub const I_USER: u32 = 0xE19F;
pub const I_BELL: u32 = 0xE05D;
pub const I_CHECK_CIRCLE: u32 = 0xE226;
pub const I_ARROW_LEFT: u32 = 0xE04C;
pub const I_PLUS: u32 = 0xE141;
pub const I_POWER: u32 = 0xE144;
pub const I_LAYERS: u32 = 0xE104;
pub const I_SPARKLES: u32 = 0xE417;
pub const I_SMILE: u32 = 0xE168;
pub const I_WIFI_OFF: u32 = 0xE1AF;
pub const I_PUZZLE: u32 = 0xE29C;
pub const I_MONITOR: u32 = 0xE121;
pub const I_CLOUD: u32 = 0xE08C;
pub const I_HOME: u32 = 0xE0F8;
pub const I_REFRESH: u32 = 0xE149;
pub const I_REPLY: u32 = 0xE22A;
pub const I_KEYBOARD: u32 = 0xE284;
pub const I_EYE: u32 = 0xE0BE;
pub const I_LOCK: u32 = 0xE10F;
pub const I_LOG_IN: u32 = 0xE111;
pub const I_LOADER: u32 = 0xE10E;
pub const I_MSG_MORE: u32 = 0xE56A;
pub const I_ELLIPSIS: u32 = 0xE0BA;
pub const I_CROWN: u32 = 0xE1D6;
// Added from lucide-static 1.54.0 (0xF100 + index).
pub const I_GAUGE: u32 = 0xF100;
pub const I_CLICK: u32 = 0xF101;
pub const I_COMPASS: u32 = 0xF102;
pub const I_MAP_PIN: u32 = 0xF103;
pub const I_CLOCK: u32 = 0xF104;
pub const I_MEMORY: u32 = 0xF105;
pub const I_SIGNAL: u32 = 0xF106;
pub const I_ZAP: u32 = 0xF107;
pub const I_FOOTPRINTS: u32 = 0xF108;
pub const I_ZOOM: u32 = 0xF109;
pub const I_SUN: u32 = 0xF10A;
pub const I_SHIRT: u32 = 0xF10B;
pub const I_MOVE: u32 = 0xF10C;
pub const I_RESET: u32 = 0xF10D;
pub const I_SLIDERS: u32 = 0xF10E;
pub const I_LAYOUT: u32 = 0xF10F;
pub const I_PALETTE: u32 = 0xF110;
pub const I_CHECK: u32 = 0xF111;
pub const I_EYE_OFF: u32 = 0xF112;
pub const I_MOUSE: u32 = 0xF113;
pub const I_TIMER: u32 = 0xF114;
pub const I_WIFI: u32 = 0xF115;
pub const I_GLASSES: u32 = 0xF116;
pub const I_BACKPACK: u32 = 0xF117;
pub const I_HAND: u32 = 0xF118;
pub const I_HAT: u32 = 0xF119;
pub const I_GRID: u32 = 0xF11A;
pub const I_MAGNET: u32 = 0xF11B;
pub const I_TYPE: u32 = 0xF11C;
pub const I_PERSON: u32 = 0xF11D;
pub const I_ACTIVITY: u32 = 0xF11E;
pub const I_CROSSHAIR: u32 = 0xF11F;
pub const I_FEATHER: u32 = 0xF120;
pub const I_ARROWS_UP: u32 = 0xF121;
pub const I_GRID_3: u32 = 0xF122;
pub const I_BRUSH: u32 = 0xF123;
pub const I_HEART: u32 = 0xF124;
pub const I_STAR: u32 = 0xF125;
pub const I_LIST: u32 = 0xF126;
pub const I_STORE: u32 = 0xF127;
pub const I_BAN: u32 = 0xF128;
pub const I_FLASK: u32 = 0xF129;
pub const I_SHIELD: u32 = 0xF12A;
pub const I_HEART_PULSE: u32 = 0xF12B;
pub const I_MINUS: u32 = 0xF12C;
pub const I_HELP: u32 = 0xF12D;
pub const I_DRUMSTICK: u32 = 0xF12E;
pub const I_SWORD: u32 = 0xF12F;
pub const I_PENCIL: u32 = 0xF130;
pub const I_TRASH: u32 = 0xF131;
pub const I_CHECKS: u32 = 0xF132;
pub const I_REPLY_ARROW: u32 = 0xF133;
pub const I_MIC: u32 = 0xF134;
pub const I_MIC_OFF: u32 = 0xF135;
pub const I_PHONE: u32 = 0xF136;
pub const I_PHONE_OFF: u32 = 0xF137;
pub const I_HEADPHONES: u32 = 0xF138;
pub const I_HEADPHONES_OFF: u32 = 0xF139;
pub const I_VOLUME: u32 = 0xF13A;
pub const I_CHEVRON_DOWN: u32 = 0xF13B;
pub const I_CHEVRON_UP: u32 = 0xF13C;
// Music player.
pub const I_PAUSE: u32 = 0xF13D;
pub const I_SKIP_FORWARD: u32 = 0xF13E;
pub const I_SKIP_BACK: u32 = 0xF13F;
pub const I_SHUFFLE: u32 = 0xF140;
pub const I_REPEAT: u32 = 0xF141;
pub const I_REPEAT_ONE: u32 = 0xF142;
pub const I_MUSIC: u32 = 0xF143;
pub const I_FOLDER_OPEN: u32 = 0xF144;
pub const I_VOLUME_OFF: u32 = 0xF145;
pub const I_LIST_MUSIC: u32 = 0xF146;
pub const I_DISC: u32 = 0xF147;
pub const I_AUDIO_LINES: u32 = 0xF148;
pub const I_VOLUME_LOW: u32 = 0xF149;
pub const I_REFRESH_CW: u32 = 0xF14A;
// Drawn in code (`icons`).
pub const I_NAV_MODS: u32 = super::icons::MODS;
pub const I_NAV_LOCKER: u32 = super::icons::LOCKER;
pub const I_NAV_SETTINGS: u32 = super::icons::SETTINGS;

/// `Canvas.mulAlpha`.
pub fn alpha(argb: u32, a: f32) -> u32 {
    super::canvas::mul_alpha(argb, a)
}

/// `Canvas.lerpColor` with `t` clamped.
pub fn mix(a: u32, b: u32, t: f32) -> u32 {
    super::canvas::lerp_color(a, b, t.clamp(0.0, 1.0))
}

/// Java's `String.hashCode`, so a name keeps the colour it has in the Java mod.
fn java_hash(text: &str) -> i32 {
    text.encode_utf16()
        .fold(0i32, |h, unit| h.wrapping_mul(31).wrapping_add(i32::from(unit)))
}

/// A stable, friendly colour for a name (avatar fallback).
pub fn name_color(name: Option<&str>) -> u32 {
    const PALETTE: [u32; 10] = [
        0xFF6366F1, 0xFF8B5CF6, 0xFFEC4899, 0xFFF43F5E, 0xFFF97316, 0xFFEAB308, 0xFF22C55E, 0xFF14B8A6, 0xFF06B6D4,
        0xFF3B82F6,
    ];
    let h = name.map_or(0, |n| java_hash(&n.to_lowercase()));
    PALETTE[(h % PALETTE.len() as i32).unsigned_abs() as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_hash_like_java() {
        // "abc".hashCode() == 96354; 96354 % 10 == 4.
        assert_eq!(java_hash("abc"), 96354);
        assert_eq!(name_color(Some("ABC")), 0xFFF97316);
        assert_eq!(name_color(None), 0xFF6366F1);
    }
}
