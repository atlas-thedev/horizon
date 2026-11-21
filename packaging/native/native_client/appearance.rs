use super::{
    textfield::TextField,
    ui::{self, Ui},
};
use crate::native_ui::{Canvas, font, theme};
use std::path::Path;

const LABELS: [&str; theme::KEYS.len()] = [
    "Background",
    "Surface",
    "Panel",
    "Panel hover",
    "Panel pressed",
    "Border",
    "Strong border",
    "Focus border",
    "Overlay",
    "Hover overlay",
    "Body text",
    "Headings",
    "Secondary text",
    "Muted text",
    "Accent",
    "Success",
    "Warning",
    "Error",
    "Accent button text",
];

fn path(dir: &Path) -> std::path::PathBuf {
    dir.join("config/native-theme.json")
}

fn parse(value: &str) -> Option<u32> {
    let value = value.trim().trim_start_matches('#');
    match value.len() {
        6 => u32::from_str_radix(value, 16)
            .ok()
            .map(|rgba| rgba | 0xFF00_0000),
        8 => u32::from_str_radix(value, 16).ok(),
        _ => None,
    }
}

pub fn load(dir: Option<&Path>) {
    let mut colors = theme::DEFAULT;
    if let Some(value) = dir
        .and_then(|dir| std::fs::read_to_string(path(dir)).ok())
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
    {
        for (i, key) in theme::KEYS.iter().enumerate() {
            if let Some(color) = value[key].as_str().and_then(parse) {
                colors[i] = color;
            }
        }
    }
    theme::set_colors(colors);
}

fn save(colors: &[u32; theme::KEYS.len()]) -> std::io::Result<()> {
    let Some(dir) = super::game_dir() else {
        return Err(std::io::ErrorKind::NotFound.into());
    };
    let file = path(&dir);
    std::fs::create_dir_all(file.parent().unwrap())?;
    let value = theme::KEYS
        .iter()
        .zip(colors)
        .map(|(&key, color)| (key, format!("#{color:08X}")))
        .collect::<std::collections::BTreeMap<_, _>>();
    let staged = file.with_extension("json.tmp");
    std::fs::write(&staged, serde_json::to_vec_pretty(&value)?)?;
    if let Err(error) = std::fs::rename(&staged, &file) {
        let _ = std::fs::remove_file(staged);
        return Err(error);
    }
    Ok(())
}

pub struct Appearance {
    value: TextField,
    role: usize,
    initialized: bool,
    error: Option<&'static str>,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            value: TextField::new("appearance:hex"),
            role: 0,
            initialized: false,
            error: None,
        }
    }
}

impl Appearance {
    fn commit(&mut self, colors: [u32; theme::KEYS.len()]) {
        theme::set_colors(colors);
        self.value.set(&format!("#{:08X}", colors[self.role]));
        self.error = save(&colors).err().map(|_| "The theme could not be saved.");
    }

    fn preset(&mut self, index: usize) {
        let mut colors = theme::DEFAULT;
        match index {
            1 => {
                colors[..5]
                    .copy_from_slice(&[0xFF061017, 0xFF0B1A23, 0xFF102531, 0xFF1A3642, 0xFF224855]);
                colors[13] = 0xFF8CA7B3;
                colors[14] = 0xFF6DDFCB;
            }
            2 => {
                colors[..5]
                    .copy_from_slice(&[0xFF170D10, 0xFF201318, 0xFF2B1A20, 0xFF40242B, 0xFF56313A]);
                colors[13] = 0xFFBB9690;
                colors[14] = 0xFFFFAD7C;
            }
            3 => {
                colors = [
                    0xFFE5E8ED, 0xFFF4F5F8, 0xFFFBFCFE, 0xFFE9EDF3, 0xFFC9D5E1, 0x24161E2A,
                    0x45161E2A, 0x76161E2A, 0x16161E2A, 0x27161E2A, 0xFF263344, 0xFF151E29,
                    0xFF435367, 0xFF586879, 0xFF235C9A, 0xFF176E47, 0xFF965709, 0xFFAF283D,
                    0xFFFFFFFF,
                ]
            }
            _ => {}
        }
        self.commit(colors);
    }

    pub fn draw(&mut self, ui: &mut Ui, c: &mut Canvas, x: f32, y: f32, w: f32) -> f32 {
        use horizon_client::lang::get_or as tr;
        if !self.initialized {
            self.initialized = true;
            self.value.max_length = 9;
            self.value
                .set(&format!("#{:08X}", theme::colors()[self.role]));
        }
        c.round(x, y, w, 165.0, 13.0, theme::panel());
        c.outline(x, y, w, 165.0, 13.0, 1.0, theme::hairline());
        c.icon(theme::I_PALETTE, 17.0, x + 23.0, y + 24.0, theme::accent());
        c.text(
            font::SEMIBOLD,
            15.0,
            &tr("native.theme.title", "Theme"),
            x + 38.0,
            y + 13.0,
            theme::text_strong(),
        );
        let width = (w - 47.0) / 4.0;
        for (i, name) in ["Native", "Tide", "Ember", "Paper"].iter().enumerate() {
            if ui.button(
                c,
                &format!("appearance:preset:{i}"),
                x + 13.0 + i as f32 * (width + 7.0),
                y + 43.0,
                width,
                30.0,
                name,
                0,
                ui::BTN_GLASS,
            ) {
                self.preset(i);
            }
        }
        let prev = ui.button(
            c,
            "appearance:prev",
            x + 13.0,
            y + 85.0,
            29.0,
            32.0,
            "",
            theme::I_CHEVRON_LEFT,
            ui::BTN_GLASS,
        );
        let next = ui.button(
            c,
            "appearance:next",
            x + 46.0,
            y + 85.0,
            29.0,
            32.0,
            "",
            theme::I_CHEVRON_RIGHT,
            ui::BTN_GLASS,
        );
        if prev || next {
            self.role = (self.role + if next { 1 } else { LABELS.len() - 1 }) % LABELS.len();
            self.value
                .set(&format!("#{:08X}", theme::colors()[self.role]));
            self.error = None;
        }
        c.round(
            x + 82.0,
            y + 88.0,
            23.0,
            25.0,
            5.0,
            theme::colors()[self.role],
        );
        c.outline(
            x + 82.0,
            y + 88.0,
            23.0,
            25.0,
            5.0,
            1.0,
            theme::border_hover(),
        );
        let edit = (w * 0.29).clamp(74.0, 113.0);
        let ex = x + w - edit - 75.0;
        let role = tr(
            &format!("native.theme.role.{}", theme::KEYS[self.role]),
            LABELS[self.role],
        );
        if ex - x > 117.0 {
            let title = c.ellipsize(font::SEMIBOLD, 11.0, &role, ex - x - 117.0);
            c.text(
                font::SEMIBOLD,
                11.0,
                &title,
                x + 111.0,
                y + 94.0,
                theme::text_strong(),
            );
        }
        let enter = self
            .value
            .draw(ui, c, ex, y + 85.0, edit, 32.0, "#AARRGGBB", 11.0);
        if ui.button(
            c,
            "appearance:apply",
            x + w - 69.0,
            y + 85.0,
            56.0,
            32.0,
            &tr("native.theme.apply", "Apply"),
            0,
            ui::BTN_PRIMARY,
        ) || enter
        {
            if let Some(color) = parse(&self.value.value()) {
                let mut colors = theme::colors();
                colors[self.role] = color;
                self.commit(colors);
            } else {
                self.error = Some("Use #RRGGBB or #AARRGGBB.");
            }
        }
        let note = self.error.map(str::to_owned).unwrap_or_else(|| {
            format!(
                "{role}  ·  {} / {}  ·  #RRGGBB or #AARRGGBB",
                self.role + 1,
                LABELS.len()
            )
        });
        let note = c.ellipsize(font::REGULAR, 10.0, &note, w - 26.0);
        c.text(
            font::REGULAR,
            10.0,
            &note,
            x + 13.0,
            y + 130.0,
            if self.error.is_some() {
                theme::danger()
            } else {
                theme::text_muted()
            },
        );
        y + 181.0
    }
}
