//! Wheel and direct-slot controls, saved in the Java mod's module format.
use super::{
    module::*,
    modules::Modules,
    setting::{Kind, Setting},
    uiconfig::UiConfig,
};
use horizon_client::{lang, native_ui::theme};

const SLOTS: [(&str, &str); 8] = [
    ("slot1", "native.emotes.slot1"),
    ("slot2", "native.emotes.slot2"),
    ("slot3", "native.emotes.slot3"),
    ("slot4", "native.emotes.slot4"),
    ("slot5", "native.emotes.slot5"),
    ("slot6", "native.emotes.slot6"),
    ("slot7", "native.emotes.slot7"),
    ("slot8", "native.emotes.slot8"),
];

pub fn all(legacy_wheel: i32) -> Vec<Module> {
    let mut wheel = Setting::key("wheel", "native.emotes.key", 66);
    wheel.set_key(legacy_wheel);
    let mut settings = vec![wheel];
    settings.extend(
        SLOTS
            .iter()
            .enumerate()
            .map(|(i, (id, label))| Setting::chord(id, label, 49 + i as i32, 4)),
    );
    vec![Module::new(
        "emotes",
        "native.emotes.title",
        "native.emotes.description",
        SETTINGS,
        theme::I_SMILE,
        true,
        settings,
        None,
        Box::new(Emotes),
    )]
}

struct Emotes;
impl Behaviour for Emotes {
    fn has_preview(&self) -> bool {
        false
    }
}

pub fn wheel_key(modules: &Modules) -> i32 {
    modules
        .get("emotes")
        .filter(|m| m.state.enabled)
        .and_then(|m| m.state.setting("wheel"))
        .map_or(-1, Setting::as_key)
}

pub fn conflict(modules: &Modules, binding: &Setting, cfg: &UiConfig) -> Option<String> {
    let Kind::Binding { key, modifiers } = binding.kind else {
        return None;
    };
    if key < 0 {
        return None;
    }
    let reserved = if modifiers & 6 == 0 && key == cfg.menu_key {
        Some("menu")
    } else if modifiers == 0 && key == cfg.relay_key {
        Some("relay")
    } else if modifiers & 6 == 0
        && [cfg.music_play_key, cfg.music_next_key, cfg.music_prev_key].contains(&key)
    {
        Some("music")
    } else if modifiers == 0 && [86, 78].contains(&key) {
        Some("voice")
    } else {
        None
    };
    if let Some(reserved) = reserved {
        return Some(lang::get(&format!("native.emotes.reserved.{reserved}")));
    }
    for module in &modules.all {
        for other in module.state.all_settings() {
            if std::ptr::eq(binding, other) {
                continue;
            }
            if matches!(other.kind, Kind::Binding { key: k, modifiers: m } if k == key && m == modifiers)
                || matches!(other.kind, Kind::Key { value, .. } if value == key)
            {
                return Some(format!(
                    "{}: {} / {}",
                    lang::get("native.emotes.conflict"),
                    label(module.state.name),
                    label(other.label)
                ));
            }
        }
    }
    None
}

pub fn quick_slot(modules: &Modules, cfg: &UiConfig, key: i32, modifiers: i32) -> Option<usize> {
    let module = modules.get("emotes").filter(|m| m.state.enabled)?;
    SLOTS.iter().enumerate().find_map(|(i, (id, _))| {
        let binding = module.state.setting(id)?;
        if matches!(binding.kind, Kind::Binding { key: k, modifiers: m } if k >= 0 && k == key && m == modifiers & 15)
            && conflict(modules, binding, cfg).is_none() { Some(i) } else { None }
    })
}

#[derive(Debug, PartialEq)]
pub enum Input {
    Wheel,
    Slot { index: usize, play: bool },
}

pub fn input(
    modules: &Modules,
    cfg: &UiConfig,
    key: i32,
    modifiers: i32,
    pressed: bool,
    repeat: bool,
    allowed: bool,
) -> Option<Input> {
    if !allowed {
        return None;
    }
    if key >= 0 && key == wheel_key(modules) && modifiers & 15 == 0 && pressed && !repeat {
        return Some(Input::Wheel);
    }
    quick_slot(modules, cfg, key, modifiers).map(|index| Input::Slot {
        index,
        play: pressed && !repeat,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_chords_are_exact_and_repeats_never_replay() {
        let modules = Modules::new(None);
        let cfg = UiConfig::default();
        for index in 0..8 {
            let key = 49 + index as i32;
            assert_eq!(
                input(&modules, &cfg, key, 4, true, false, true),
                Some(Input::Slot { index, play: true })
            );
            assert_eq!(
                input(&modules, &cfg, key, 4, true, true, true),
                Some(Input::Slot { index, play: false })
            );
            assert_eq!(
                input(&modules, &cfg, key, 4, false, false, true),
                Some(Input::Slot { index, play: false })
            );
            assert_eq!(input(&modules, &cfg, key, 0, true, false, true), None);
            assert_eq!(input(&modules, &cfg, key, 6, true, false, true), None);
            assert_eq!(input(&modules, &cfg, key, 4, true, false, false), None);
        }
        assert_eq!(
            input(&modules, &cfg, 66, 0, true, false, true),
            Some(Input::Wheel)
        );
        assert_eq!(input(&modules, &cfg, 66, 0, true, true, true), None);
        assert_eq!(input(&modules, &cfg, 66, 4, true, false, true), None);
    }

    #[test]
    fn disabled_modules_duplicates_and_native_conflicts_do_not_fire() {
        let mut modules = Modules::new(None);
        let mut cfg = UiConfig::default();
        modules.get_mut("emotes").unwrap().state.enabled = false;
        assert_eq!(input(&modules, &cfg, 66, 0, true, false, true), None);
        assert_eq!(quick_slot(&modules, &cfg, 49, 4), None);
        modules.get_mut("emotes").unwrap().state.enabled = true;
        modules.get_mut("emotes").unwrap().state.settings[2].kind = Kind::Binding {
            key: 49,
            modifiers: 4,
        };
        assert_eq!(quick_slot(&modules, &cfg, 49, 4), None);
        modules.get_mut("emotes").unwrap().state.settings[2].reset();
        modules.get_mut("fps").unwrap().state.toggle_key.kind = Kind::Binding {
            key: 49,
            modifiers: 4,
        };
        assert_eq!(quick_slot(&modules, &cfg, 49, 4), None);
        modules.get_mut("fps").unwrap().state.toggle_key.reset();
        modules.get_mut("emotes").unwrap().state.settings[1].kind = Kind::Binding {
            key: 75,
            modifiers: 0,
        };
        cfg.menu_key = 75;
        assert_eq!(quick_slot(&modules, &cfg, 75, 0), None);
    }

    #[test]
    fn shortcuts_roundtrip_clear_and_reset_without_losing_defaults() {
        let mut original = Setting::chord("slot1", "native.emotes.slot1", 49, 4);
        original.kind = Kind::Binding {
            key: 75,
            modifiers: 3,
        };
        let mut copy = Setting::chord("slot1", "native.emotes.slot1", 49, 4);
        copy.load(&original.save());
        assert_eq!(copy, original);
        original.kind = Kind::Binding {
            key: -1,
            modifiers: 0,
        };
        copy.load(&original.save());
        assert_eq!(copy, original);
        assert!(!copy.is_default());
        copy.reset();
        assert_eq!(
            copy.kind,
            Kind::Binding {
                key: 49,
                modifiers: 4
            }
        );
        assert!(copy.is_default());
    }

    #[test]
    fn legacy_wheel_and_module_shortcuts_survive_save_and_reload() {
        let dir =
            std::env::temp_dir().join(format!("native-emote-controls-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("config")).unwrap();
        std::fs::write(dir.join("config/native-ui.json"), r#"{"emoteKey":75}"#).unwrap();
        let mut modules = Modules::new(Some(dir.clone()));
        assert_eq!(wheel_key(&modules), 75);
        let m = modules.get_mut("emotes").unwrap();
        m.state.settings[0].set_key(76);
        m.state.settings[1].kind = Kind::Binding {
            key: 80,
            modifiers: 3,
        };
        m.state.settings[2].kind = Kind::Binding {
            key: -1,
            modifiers: 0,
        };
        modules.save();
        let loaded = Modules::new(Some(dir.clone()));
        assert_eq!(wheel_key(&loaded), 76);
        assert_eq!(quick_slot(&loaded, &UiConfig::default(), 80, 3), Some(0));
        assert_eq!(quick_slot(&loaded, &UiConfig::default(), 50, 4), None);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
