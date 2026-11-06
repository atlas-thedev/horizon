//! `Status`: small pure helpers shared by the status modules (potion
//! effects, armour, health and food).
#![allow(dead_code)]

const ROMAN: [&str; 11] = ["", "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X"];

/// Level 1 - 10 as roman numerals like the game; anything else as a plain number.
pub fn roman(level: i32) -> String {
    if (1..ROMAN.len() as i32).contains(&level) {
        ROMAN[level as usize].to_owned()
    } else {
        level.to_string()
    }
}

/// Seconds left for a tick count (rounded up, like the game's own timer).
pub fn seconds(ticks: i32) -> i32 {
    ((ticks + 19) / 20).max(0)
}

/// "1:05", "12:00", "1:02:03".
pub fn time(ticks: i32) -> String {
    let s = seconds(ticks);
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// "night_vision" -> "Night Vision", "mymod:bless" -> "Bless".
pub fn title(id: &str) -> String {
    let name = id.rsplit(':').next().unwrap_or(id);
    let mut out = String::with_capacity(name.len());
    let mut up = true;
    for ch in name.chars() {
        if ch == '_' || ch == '.' {
            out.push(' ');
            up = true;
        } else if up {
            out.extend(ch.to_uppercase());
            up = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// The game's own names where they differ from a tidy-up of the id.
pub fn effect_name(id: &str) -> String {
    match id {
        "unluck" | "bad_luck" => "Bad Luck".to_owned(),
        "luck" => "Luck".to_owned(),
        "jump" => "Jump Boost".to_owned(),
        "slow_falling" => "Slow Falling".to_owned(),
        _ => title(id),
    }
}

pub fn harmful(id: &str) -> bool {
    matches!(
        id,
        "slowness"
            | "mining_fatigue"
            | "instant_damage"
            | "nausea"
            | "blindness"
            | "hunger"
            | "weakness"
            | "poison"
            | "wither"
            | "levitation"
            | "unluck"
            | "bad_luck"
            | "darkness"
            | "bad_omen"
            | "infested"
            | "oozing"
            | "weaving"
            | "wind_charged"
            | "raid_omen"
            | "trial_omen"
    )
}

/// Colour of the durability bar and text: green when new, through yellow to red (the game's own scale).
pub fn wear(remaining: f32) -> u32 {
    // Hue 0 (red) .. 120 degrees (green), full saturation and value.
    let h = remaining.clamp(0.0, 1.0) * 2.0; // 0..2 sextants
    let up = (255.0 * h.min(1.0)).round() as u32;
    let down = if h <= 1.0 { 255 } else { (255.0 * (2.0 - h)).round() as u32 };
    0xFF000000 | (down << 16) | (up << 8)
}

/// Hunger points (half drumsticks) and saturation a food item restores; `None` when it is not food.
pub fn food(id: &str) -> Option<(i32, f32)> {
    Some(match id {
        "apple" => (4, 2.4),
        "baked_potato" => (5, 6.0),
        "beef" => (3, 1.8),
        "beetroot" => (1, 1.2),
        "beetroot_soup" => (6, 7.2),
        "bread" => (5, 6.0),
        "carrot" => (3, 3.6),
        "chicken" => (2, 1.2),
        "chorus_fruit" => (4, 2.4),
        "cod" => (2, 0.4),
        "cooked_beef" => (8, 12.8),
        "cooked_chicken" => (6, 7.2),
        "cooked_cod" => (5, 6.0),
        "cooked_mutton" => (6, 9.6),
        "cooked_porkchop" => (8, 12.8),
        "cooked_rabbit" => (5, 6.0),
        "cooked_salmon" => (6, 9.6),
        "cookie" => (2, 0.4),
        "dried_kelp" => (1, 0.6),
        "enchanted_golden_apple" => (4, 9.6),
        "golden_apple" => (4, 9.6),
        "golden_carrot" => (6, 14.4),
        "honey_bottle" => (6, 1.2),
        "melon_slice" => (2, 1.2),
        "mushroom_stew" => (6, 7.2),
        "mutton" => (2, 1.2),
        "poisonous_potato" => (2, 1.2),
        "porkchop" => (3, 1.8),
        "potato" => (1, 0.6),
        "pufferfish" => (1, 0.2),
        "pumpkin_pie" => (8, 4.8),
        "rabbit" => (3, 1.8),
        "rabbit_stew" => (10, 12.0),
        "rotten_flesh" => (4, 0.8),
        "salmon" => (2, 0.4),
        "spider_eye" => (2, 3.2),
        "suspicious_stew" => (6, 7.2),
        "sweet_berries" => (2, 0.4),
        "glow_berries" => (2, 0.4),
        "tropical_fish" => (1, 0.2),
        _ => return None,
    })
}

/// Formats a health value: "20", "7.5".
pub fn hp(v: f32) -> String {
    if (v - v.round()).abs() < 0.05 { format!("{}", v.round() as i32) } else { format!("{v:.1}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effects_read_like_the_game() {
        assert_eq!(roman(2), "II");
        assert_eq!(roman(11), "11");
        assert_eq!(time(20 * 65), "1:05");
        assert_eq!(time(20 * 3723), "1:02:03");
        assert_eq!(effect_name("night_vision"), "Night Vision");
        assert_eq!(effect_name("jump"), "Jump Boost");
        assert_eq!(effect_name("mymod:bless"), "Bless");
        assert!(harmful("poison") && !harmful("speed"));
    }

    #[test]
    fn wear_runs_from_red_to_green() {
        assert_eq!(wear(1.0), 0xFF00FF00);
        assert_eq!(wear(0.0), 0xFFFF0000);
        assert_eq!(wear(0.5), 0xFFFFFF00);
        assert_eq!(hp(7.5), "7.5");
        assert_eq!(hp(20.0), "20");
    }
}
