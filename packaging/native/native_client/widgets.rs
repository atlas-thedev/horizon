//! `Widgets`: setting widgets in the launcher look: slider, segmented choice,
//! colour swatches, keybind button, the pixel editor, and the labelled row
//! that picks one for a [`Setting`].
#![allow(dead_code)]

use super::keys;
use super::setting::{self, Kind, Setting};
use super::textfield::TextField;
use super::ui::{self, Ui};
use horizon_client::native_ui::theme;
use horizon_client::native_ui::{Canvas, font};
use std::collections::HashMap;

pub const SWATCHES: [u32; 9] = [
    0xFFFFFFFF, 0xFFD9A6DA, 0xFF60A5FA, 0xFF22C55E, 0xFFF59E0B, 0xFFEF4444, 0xFFA78BFA, 0xFF2DD4BF, 0xFF000000,
];
pub const ROW: f32 = 40.0;
const PIX_CELL: f32 = 13.0;

/// The dropdown that is unfolded under its row, by setting id.
static OPEN_DEVICE: std::sync::Mutex<Option<&'static str>> = std::sync::Mutex::new(None);

fn open_device() -> Option<&'static str> {
    *OPEN_DEVICE.lock().unwrap_or_else(|e| e.into_inner())
}

fn set_open_device(id: Option<&'static str>) {
    *OPEN_DEVICE.lock().unwrap_or_else(|e| e.into_inner()) = id;
}

/// Widget state that outlives a frame.
#[derive(Default)]
pub struct Widgets {
    /// Id of the keybind button waiting for a key.
    pub listening: Option<String>,
    /// While drawing on a pixel grid: true paints, false erases.
    pix_paint: bool,
    pix_last: Option<(usize, usize)>,
    fields: HashMap<String, TextField>,
}

impl Setting {
    pub fn pixel_get(&self, x: usize, y: usize) -> bool {
        match &self.kind {
            Kind::Pixels { size, grid, .. } => x < *size && y < *size && grid[y * size + x],
            _ => false,
        }
    }

    /// Sets a cell (and its mirror images when mirroring).
    pub fn pixel_set(&mut self, x: usize, y: usize, on: bool) {
        if let Kind::Pixels { size, grid, mirror, .. } = &mut self.kind {
            if x >= *size || y >= *size {
                return;
            }
            let m = *size - 1;
            grid[y * *size + x] = on;
            if *mirror {
                grid[y * *size + (m - x)] = on;
                grid[(m - y) * *size + x] = on;
                grid[(m - y) * *size + (m - x)] = on;
            }
        }
    }

    pub fn pixel_empty(&self) -> bool {
        matches!(&self.kind, Kind::Pixels { grid, .. } if !grid.iter().any(|b| *b))
    }

    pub fn pixel_clear(&mut self) {
        if let Kind::Pixels { grid, .. } = &mut self.kind {
            grid.iter_mut().for_each(|b| *b = false);
        }
    }

    /// Which saved drawing the grid matches.
    pub fn pixel_current(&self) -> Option<usize> {
        match &self.kind {
            Kind::Pixels { grid, saved, .. } => saved.iter().position(|s| s == grid),
            _ => None,
        }
    }

    pub fn pixel_save_new(&mut self) {
        if self.pixel_current().is_some() {
            return;
        }
        if let Kind::Pixels { grid, saved, .. } = &mut self.kind {
            saved.push(grid.clone());
        }
    }

    pub fn pixel_use(&mut self, i: usize) {
        if let Kind::Pixels { grid, saved, .. } = &mut self.kind
            && let Some(s) = saved.get(i)
        {
            *grid = s.clone();
        }
    }

    pub fn pixel_delete(&mut self, i: usize) {
        if let Kind::Pixels { saved, .. } = &mut self.kind
            && i < saved.len()
        {
            saved.remove(i);
        }
    }
}

impl Widgets {
    /// Esc while a keybind listens: cancels it. True when consumed.
    pub fn escape(&mut self) -> bool {
        if open_device().is_some() {
            set_open_device(None);
            return true;
        }
        self.listening.take().is_some()
    }

    /// Horizontal slider for a number; true while the value changed this frame.
    #[allow(clippy::too_many_arguments)]
    pub fn slider(&mut self, ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, w: f32, s: &mut Setting) -> bool {
        let Kind::Num { value, min, max, .. } = s.kind else {
            return false;
        };
        let h = 18.0;
        let over = ui.hover(c, x - 6.0, y, w + 12.0, h);
        ui.clicked(c, id, x - 6.0, y, w + 12.0, h);
        let drag = ui.is_pressing(id);
        let mut changed = false;
        if drag {
            let t = ((ui.mx - x) / w).clamp(0.0, 1.0);
            s.set_num(min + t * (max - min));
            changed = (s.as_f32() - value).abs() > f32::EPSILON;
        }
        let t = (s.as_f32() - min) / (max - min);
        let shown = ui.anim(&format!("{id}#v"), t, if drag { 40.0 } else { 18.0 });
        let hv = ui.anim_on(&format!("{id}#h"), over || drag, 14.0);
        let ty = y + h / 2.0 - 2.0;
        c.round(x, ty, w, 4.0, 2.0, theme::SUBTLE_HOVER);
        c.round(x, ty, (w * shown).max(4.0), 4.0, 2.0, 0xFFF4F4F5);
        let kr = 7.0 + hv * 1.5;
        let kx = x + w * shown;
        c.circle(kx, y + h / 2.0, kr + 3.0, theme::alpha(0xFFFFFFFF, 0.08 * hv));
        c.circle(kx, y + h / 2.0, kr, 0xFFFFFFFF);
        if over || drag {
            ui.cursor_hand = true;
        }
        changed
    }

    /// A pill group; returns the (possibly new) selected index.
    #[allow(clippy::too_many_arguments)]
    pub fn segmented(ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, w: f32, h: f32, options: &[&str], value: usize) -> usize {
        c.round(x, y, w, h, h / 2.0, theme::SUBTLE);
        c.outline(x, y, w, h, h / 2.0, 1.0, theme::HAIRLINE);
        let fs = 11.5;
        // Segments are sized to their text, so a long option is never cut off.
        let mut seg: Vec<f32> = options.iter().map(|o| c.text_width(font::SEMIBOLD, fs, o) + 20.0).collect();
        let total: f32 = seg.iter().sum();
        let inner = w - 4.0;
        let k = if total > 0.0 { inner / total } else { 1.0 };
        let mut sx = vec![x + 2.0];
        for s in seg.iter_mut() {
            *s *= k;
            sx.push(sx[sx.len() - 1] + *s);
        }
        let n = options.len();
        if n == 0 {
            return value;
        }
        let sel = ui.anim(&format!("{id}#s"), value as f32, 18.0);
        let lo = (sel.floor().max(0.0) as usize).min(n - 1);
        let hi = (lo + 1).min(n - 1);
        let t = sel - lo as f32;
        let px = sx[lo] + (sx[hi] - sx[lo]) * t;
        let pw = seg[lo] + (seg[hi] - seg[lo]) * t;
        c.round(px, y + 2.0, pw, h - 4.0, (h - 4.0) / 2.0, 0xFFF4F4F5);
        let mut out = value;
        for i in 0..n {
            let over = ui.hover(c, sx[i], y, seg[i], h);
            if ui.clicked(c, &format!("{id}:{i}"), sx[i], y, seg[i], h) {
                out = i;
            }
            let on = (1.0 - (sel - i as f32).abs()).max(0.0);
            let fg = theme::mix(if over { theme::TEXT_STRONG } else { theme::TEXT_SECONDARY }, theme::SOLID_FG, on);
            let label = c.ellipsize(font::SEMIBOLD, fs, options[i], seg[i] - 4.0);
            let tw = c.text_width(font::SEMIBOLD, fs, &label);
            let lh = c.line_height(font::SEMIBOLD, fs);
            c.text(font::SEMIBOLD, fs, &label, sx[i] + (seg[i] - tw) / 2.0, y + (h - lh) / 2.0, fg);
        }
        out
    }

    pub fn swatches_width() -> f32 {
        SWATCHES.len() as f32 * 22.0 - 4.0
    }

    /// Preset colours; true when one was picked.
    pub fn swatches(ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, s: &mut Setting) -> bool {
        let cur = s.as_color();
        let mut changed = false;
        for (i, sw) in SWATCHES.iter().enumerate() {
            let sx = x + i as f32 * 22.0;
            let sel = (sw | 0xFF000000) == (cur | 0xFF000000);
            let over = ui.hover(c, sx, y, 18.0, 18.0);
            if ui.clicked(c, &format!("{id}:{i}"), sx, y, 18.0, 18.0) {
                let now = s.as_color();
                s.set_color(if now & 0xFF000000 == 0 { *sw } else { (now & 0xFF000000) | (sw & 0xFFFFFF) });
                changed = true;
            }
            let hv = ui.anim_on(&format!("{id}:{i}#h"), over || sel, 16.0);
            if hv > 0.01 {
                c.circle(sx + 9.0, y + 9.0, 9.0 + 2.0 * hv, theme::alpha(0xFFFFFFFF, if sel { 0.9 } else { 0.25 * hv }));
                c.circle(sx + 9.0, y + 9.0, 9.0 + 0.5 * hv, 0xFF08090C);
            }
            c.circle(sx + 9.0, y + 9.0, 7.6, 0x33FFFFFF);
            c.circle(sx + 9.0, y + 9.0, 7.0, *sw);
        }
        changed
    }

    /// Keybind button; returns the new GLFW code (or the same one). Backspace unbinds (-1), Esc cancels.
    #[allow(clippy::too_many_arguments)]
    pub fn keybind(&mut self, ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, w: f32, h: f32, value: i32) -> i32 {
        let mut wait = self.listening.as_deref() == Some(id);
        let mut out = value;
        if wait {
            for (code, _) in ui.keys.clone() {
                if code == ui::KEY_ESCAPE {
                    continue;
                }
                out = if code == ui::KEY_BACKSPACE { -1 } else { code };
                self.listening = None;
                wait = false;
                break;
            }
        }
        let over = ui.hover(c, x, y, w, h);
        if ui.clicked(c, id, x, y, w, h) {
            self.listening = if wait { None } else { Some(id.to_owned()) };
            wait = !wait;
        }
        let hv = ui.anim_on(&format!("{id}#h"), over, 14.0);
        let lv = ui.anim_on(&format!("{id}#l"), wait, 12.0);
        c.round(x, y, w, h, 8.0, theme::mix(theme::mix(0xB808090C, 0xD80E1014, hv), 0x26D9A6DA, lv));
        c.outline(
            x,
            y,
            w,
            h,
            8.0,
            1.0,
            theme::mix(
                theme::mix(theme::HAIRLINE_STRONG, theme::BORDER_HOVER, hv),
                theme::alpha(theme::ACCENT, 0.7),
                lv,
            ),
        );
        let fs = 11.5;
        let t = if wait { "Press a key\u{2026}".to_owned() } else { keys::name(out) };
        let t = c.ellipsize(font::SEMIBOLD, fs, &t, w - 12.0);
        let fg = if wait {
            theme::alpha(theme::ACCENT, 0.6 + 0.4 * (0.5 + 0.5 * (ui.now / 220.0).sin()) as f32)
        } else {
            theme::TEXT_STRONG
        };
        let tw = c.text_width(font::SEMIBOLD, fs, &t);
        let lh = c.line_height(font::SEMIBOLD, fs);
        c.text(font::SEMIBOLD, fs, &t, x + (w - tw) / 2.0, y + (h - lh) / 2.0, fg);
        out
    }

    /// Height a setting row takes.
    pub fn row_height(s: &Setting) -> f32 {
        match &s.kind {
            Kind::Binding { .. } => ROW + 36.0,
            Kind::Pixels { size, .. } => ROW + PIX_CELL * *size as f32 + 18.0,
            Kind::Device { options, .. } if open_device() == Some(s.id) => ROW + 30.0 * (1 + (options.0)().len()) as f32 + 8.0,
            _ => ROW,
        }
    }

    /// The pixel editor under its label: the grid on the left (click or drag
    /// to draw, right-click to erase), saved drawings and actions on the right.
    fn pixels_row(&mut self, ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, w: f32, p: &mut Setting) -> bool {
        let Kind::Pixels { size, .. } = p.kind else {
            return false;
        };
        let n = size;
        let (cell, gs) = (PIX_CELL, PIX_CELL * n as f32);
        let (gx, gy) = (x, y + ROW);
        let mut changed = false;
        c.round(gx - 4.0, gy - 4.0, gs + 8.0, gs + 8.0, 8.0, 0xFF0B0C10);
        c.outline(gx - 4.0, gy - 4.0, gs + 8.0, gs + 8.0, 8.0, 1.0, theme::HAIRLINE);
        let mid = n / 2;
        for j in 0..n {
            for i in 0..n {
                let (cx, cy) = (gx + i as f32 * cell, gy + j as f32 * cell);
                let on = p.pixel_get(i, j);
                let bg = if i == mid || j == mid { 0x14FFFFFF } else { 0x08FFFFFF };
                c.fill(cx + 0.5, cy + 0.5, cell - 1.0, cell - 1.0, if on { 0xFFF4F4F5 } else { bg });
            }
        }
        c.outline(gx + mid as f32 * cell - 1.0, gy + mid as f32 * cell - 1.0, cell + 1.0, cell + 1.0, 2.0, 1.0, 0x66B46CFF);
        let gid = format!("{id}#grid");
        ui.clicked(c, &gid, gx, gy, gs, gs);
        let over_grid = ui.hover(c, gx, gy, gs, gs);
        let hx = ((ui.mx - gx) / cell).floor();
        let hy = ((ui.my - gy) / cell).floor();
        if over_grid && hx >= 0.0 && hy >= 0.0 && (hx as usize) < n && (hy as usize) < n {
            let (hx, hy) = (hx as usize, hy as usize);
            c.outline(gx + hx as f32 * cell, gy + hy as f32 * cell, cell, cell, 2.0, 1.0, 0xAAFFFFFF);
            if ui.pressed {
                self.pix_paint = !p.pixel_get(hx, hy);
                self.pix_last = None;
            }
            if ui.is_pressing(&gid) && self.pix_last != Some((hx, hy)) {
                p.pixel_set(hx, hy, self.pix_paint);
                self.pix_last = Some((hx, hy));
                changed = true;
            }
            if ui.right_pressed && p.pixel_get(hx, hy) {
                p.pixel_set(hx, hy, false);
                changed = true;
            }
        }
        if !ui.down {
            self.pix_last = None;
        }

        // Right side: actions and saved drawings.
        let rx = gx + gs + 20.0;
        let rw = (x + w - rx).max(120.0);
        let mut by = gy - 2.0;
        let bh = 28.0;
        let half = (rw - 8.0) / 2.0;
        if ui.button(c, &format!("{id}#save"), rx, by, half, bh, "Save as new", theme::I_PLUS, ui::BTN_PRIMARY) && !p.pixel_empty() {
            p.pixel_save_new();
            changed = true;
        }
        let mirror = matches!(p.kind, Kind::Pixels { mirror: true, .. });
        if ui.button(
            c,
            &format!("{id}#mirror"),
            rx + half + 8.0,
            by,
            half,
            bh,
            if mirror { "Mirror on" } else { "Mirror off" },
            theme::I_GRID,
            if mirror { ui::BTN_GLASS } else { ui::BTN_GHOST },
        ) && let Kind::Pixels { mirror, .. } = &mut p.kind
        {
            *mirror = !*mirror;
        }
        by += bh + 8.0;
        if ui.button(c, &format!("{id}#clear"), rx, by, half, bh, "Clear", theme::I_BRUSH, ui::BTN_GHOST) {
            p.pixel_clear();
            changed = true;
        }
        let cur = p.pixel_current();
        if let Some(cur) = cur
            && ui.button(c, &format!("{id}#del"), rx + half + 8.0, by, half, bh, "Delete", theme::I_TRASH, ui::BTN_DANGER)
        {
            p.pixel_delete(cur);
            changed = true;
        }
        by += bh + 14.0;
        let saved: Vec<Vec<bool>> = match &p.kind {
            Kind::Pixels { saved, .. } => saved.clone(),
            _ => Vec::new(),
        };
        c.text(
            font::SEMIBOLD,
            10.5,
            if saved.is_empty() { "SAVED CROSSHAIRS  \u{b7}  none yet" } else { "SAVED CROSSHAIRS" },
            rx,
            by,
            theme::TEXT_MUTED,
        );
        by += 18.0;
        let t = 40.0;
        let mut tx = rx;
        for (k, g) in saved.iter().enumerate() {
            if tx + t > x + w + 0.5 {
                tx = rx;
                by += t + 6.0;
            }
            if by + t > gy + gs + 6.0 {
                break; // More than fit: the newest stay reachable by deleting older ones.
            }
            let sel = Some(k) == cur;
            let ov = ui.hover(c, tx, by, t, t);
            c.round(tx, by, t, t, 7.0, if sel { 0x33B46CFF } else if ov { 0x1FFFFFFF } else { 0x12FFFFFF });
            c.outline(tx, by, t, t, 7.0, 1.0, if sel { 0xFFB46CFF } else { theme::HAIRLINE });
            let pc = (t - 8.0) / n as f32;
            for j in 0..n {
                for i in 0..n {
                    if g[j * n + i] {
                        c.fill(tx + 4.0 + i as f32 * pc, by + 4.0 + j as f32 * pc, pc, pc, 0xFFF4F4F5);
                    }
                }
            }
            if ui.clicked(c, &format!("{id}#use{k}"), tx, by, t, t) {
                p.pixel_use(k);
                changed = true;
            }
            tx += t + 6.0;
        }
        c.text(
            font::REGULAR,
            10.0,
            "Drag to draw \u{b7} right-click erases \u{b7} the grid is your crosshair",
            gx - 4.0,
            gy + gs + 6.0,
            theme::TEXT_MUTED,
        );
        changed
    }

    /// The dropdown of a [`Kind::Device`]: the current choice on the right, the list unfolded under it.
    /// Returns whether the choice changed and the left edge of the control.
    #[allow(clippy::too_many_arguments)]
    fn device_row(ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, w: f32, s: &mut Setting, label_w: f32) -> (bool, f32) {
        let Kind::Device { value, options, empty_label } = s.kind.clone() else {
            return (false, x + w);
        };
        let key = s.id;
        let bw = (w - label_w - 40.0).clamp(120.0, 240.0);
        let bh = 28.0;
        let (bx, by) = (x + w - bw, y + (ROW - bh) / 2.0);
        let open = open_device() == Some(key);
        let list = (options.0)();
        let current = if value.is_empty() { empty_label.to_owned() } else { value.clone() };
        let missing = !value.is_empty() && !list.contains(&value);
        let over = ui.hover(c, bx, by, bw, bh);
        c.round(bx, by, bw, bh, 8.0, if over || open { 0x24FFFFFF } else { 0x14FFFFFF });
        c.outline(bx, by, bw, bh, 8.0, 1.0, if open { 0x40FFFFFF } else { theme::HAIRLINE });
        let shown = if missing { format!("{current} (unplugged)") } else { current };
        let shown = c.ellipsize(font::SEMIBOLD, 11.5, &shown, bw - 34.0);
        let lh = c.line_height(font::SEMIBOLD, 11.5);
        c.text(font::SEMIBOLD, 11.5, &shown, bx + 10.0, by + (bh - lh) / 2.0, if missing { theme::DANGER } else { theme::TEXT_STRONG });
        c.icon(if open { theme::I_CHEVRON_UP } else { theme::I_CHEVRON_DOWN }, 12.0, bx + bw - 14.0, by + bh / 2.0, theme::TEXT_MUTED);
        if ui.clicked(c, id, bx, by, bw, bh) {
            set_open_device(if open { None } else { Some(key) });
        }
        if open {
            let mut ly = y + ROW;
            let entries = std::iter::once(String::new()).chain(list.iter().cloned());
            for (i, name) in entries.enumerate() {
                let shown = if name.is_empty() { empty_label.to_owned() } else { name.clone() };
                let sel = value == name;
                let hover = ui.hover(c, x, ly, w, 28.0);
                if hover || sel {
                    c.round(x, ly, w, 28.0, 7.0, if sel { 0x1F22C55E } else { 0x14FFFFFF });
                }
                if sel {
                    c.icon(theme::I_CHECK, 12.0, x + 14.0, ly + 14.0, theme::ONLINE);
                }
                let t = c.ellipsize(font::MEDIUM, 12.0, &shown, w - 40.0);
                let th = c.line_height(font::MEDIUM, 12.0);
                c.text(font::MEDIUM, 12.0, &t, x + 28.0, ly + (28.0 - th) / 2.0, if sel { theme::TEXT_STRONG } else { theme::TEXT });
                if ui.clicked(c, &format!("{id}#{i}"), x, ly, w, 28.0) {
                    if let Kind::Device { value, .. } = &mut s.kind {
                        *value = name;
                    }
                    set_open_device(None);
                    return (true, x + w);
                }
                ly += 30.0;
            }
        }
        (false, bx)
    }

    /// One labelled setting row (label left, control right). `hint` is the
    /// conflict warning of a toggle binding. True when the value changed.
    pub fn row(&mut self, ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, w: f32, s: &mut Setting, warning: Option<&str>) -> bool {
        let h = ROW;
        let fs = 12.5;
        let display_label = if s.label.starts_with("native.") { horizon_client::lang::get(s.label) } else { s.label.to_owned() };
        let label_w = c.text_width(font::MEDIUM, fs, &display_label);
        let mut changed = false;
        let mut control_x = x + w; // Left edge of the control, so the reset button can sit just before it.
        let label_max = match &s.kind {
            Kind::Bool { .. } => w - 34.0 - 40.0,
            Kind::Num { .. } => w - 70.0 - 46.0 - 36.0,
            Kind::Choice { .. } => w * 0.4,
            Kind::Color { .. } => w - Self::swatches_width() - 40.0,
            Kind::Binding { .. } => w - 160.0 - 40.0,
            Kind::Key { .. } => w - 110.0 - 40.0,
            Kind::Text { .. } => w * 0.42,
            Kind::Device { .. } => w * 0.38,
            Kind::Pixels { .. } => w,
        };
        let label = if label_w > label_max {
            c.ellipsize(font::MEDIUM, fs, &display_label, label_max.max(40.0))
        } else {
            display_label
        };
        let label_w = c.text_width(font::MEDIUM, fs, &label);
        let lh = c.line_height(font::MEDIUM, fs);
        c.text(font::MEDIUM, fs, &label, x, y + (h - lh) / 2.0, theme::TEXT);
        match s.kind.clone() {
            Kind::Pixels { .. } => changed = self.pixels_row(ui, c, id, x, y, w, s),
            Kind::Device { .. } => {
                let (ch, cx) = Self::device_row(ui, c, id, x, y, w, s, label_w);
                changed = ch;
                control_x = cx;
            }
            Kind::Bool { value, .. } => {
                control_x = x + w - 34.0;
                if ui.toggle(c, id, x + w - 34.0, y + (h - 20.0) / 2.0, value) {
                    s.set_bool(!value);
                    changed = true;
                }
            }
            Kind::Num { .. } => {
                let v = s.num_text();
                let vw = 46.0;
                let sw = (w - label_w - vw - 36.0).clamp(70.0, 170.0);
                let tw = c.text_width(font::SEMIBOLD, 11.5, &v);
                let th = c.line_height(font::SEMIBOLD, 11.5);
                c.text(font::SEMIBOLD, 11.5, &v, x + w - tw, y + (h - th) / 2.0, theme::TEXT_SECONDARY);
                changed = self.slider(ui, c, id, x + w - vw - sw, y + (h - 18.0) / 2.0, sw, s);
                control_x = x + w - vw - sw - 6.0;
            }
            Kind::Choice { value, options, shorts, .. } => {
                let avail = (w * 0.6).max(w - label_w - 16.0);
                let mut labels: Vec<&str> = options.to_vec();
                let width_of = |c: &mut Canvas, l: &[&str]| l.iter().map(|o| c.text_width(font::SEMIBOLD, 11.5, o) + 24.0).sum::<f32>();
                let mut sw = width_of(c, &labels);
                if sw > avail
                    && let Some(sh) = shorts
                    && sh.len() == options.len()
                {
                    // The full labels would be cut off: use the short ones.
                    labels = sh.to_vec();
                    sw = width_of(c, &labels);
                }
                let sw = sw.max(90.0).min(avail);
                let v = Self::segmented(ui, c, id, x + w - sw, y + (h - 26.0) / 2.0, sw, 26.0, &labels, value);
                control_x = x + w - sw;
                if v != value {
                    s.set_choice(v);
                    changed = true;
                }
            }
            Kind::Color { .. } => {
                changed = Self::swatches(ui, c, id, x + w - Self::swatches_width(), y + (h - 18.0) / 2.0, s);
                control_x = x + w - Self::swatches_width();
            }
            Kind::Binding { key, modifiers } => {
                let mut new = (key, modifiers);
                if self.listening.as_deref() == Some(id) {
                    for (k, m) in ui.keys.clone() {
                        if k == ui::KEY_ESCAPE {
                            self.listening = None;
                            break;
                        }
                        if k == ui::KEY_BACKSPACE || k == ui::KEY_DELETE {
                            new = (-1, 0);
                            self.listening = None;
                            break;
                        }
                        if setting::bindable(k) && m & ui::MOD_SUPER == 0 {
                            new = (k, m & 7);
                            self.listening = None;
                            break;
                        }
                    }
                    ui.keys.clear();
                }
                if new != (key, modifiers) {
                    s.kind = Kind::Binding { key: new.0, modifiers: new.1 };
                    changed = true;
                }
                control_x = x + w - 160.0;
                let text = if self.listening.as_deref() == Some(id) {
                    "Press a key...".to_owned()
                } else {
                    binding_text(new.0, new.1)
                };
                let text = c.ellipsize(font::SEMIBOLD, 11.5, &text, 140.0);
                if ui.button(c, id, control_x, y + 6.0, 160.0, 28.0, &text, 0, ui::BTN_GLASS) {
                    self.listening = if self.listening.as_deref() == Some(id) { None } else { Some(id.to_owned()) };
                }
                let hint = match warning {
                    None => "Ctrl / Shift / Alt optional. Backspace / Delete clears.".to_owned(),
                    Some(wn) => format!("{wn} (not active)"),
                };
                let hint = c.ellipsize(font::REGULAR, 10.0, &hint, w);
                c.text(font::REGULAR, 10.0, &hint, x, y + ROW, if warning.is_none() { theme::TEXT_MUTED } else { theme::IDLE });
                let note = c.ellipsize(font::REGULAR, 10.0, "Esc cancels. Check Minecraft / other mods for conflicts.", w);
                c.text(font::REGULAR, 10.0, &note, x, y + ROW + 15.0, theme::TEXT_MUTED);
            }
            Kind::Key { value, .. } => {
                let v = self.keybind(ui, c, id, x + w - 110.0, y + (h - 28.0) / 2.0, 110.0, 28.0, value);
                control_x = x + w - 110.0;
                if v != value {
                    s.set_key(v);
                    changed = true;
                }
            }
            Kind::Text { value, placeholder, max_length, .. } => {
                let fw = (w - label_w - 28.0).clamp(120.0, 220.0);
                let fx = x + w - fw;
                let fh = 26.0;
                control_x = fx;
                let field = self.fields.entry(id.to_owned()).or_insert_with(|| {
                    let mut f = TextField::new(id);
                    f.max_length = if max_length > 0 { max_length } else { 2000 };
                    f.set(&value);
                    f
                });
                if !field.focused(ui) && field.value() != value {
                    field.set(&value);
                }
                field.draw(ui, c, fx, y + (h - fh) / 2.0, fw, fh, &placeholder, 11.5);
                let now = field.value();
                if now != value {
                    s.set_text(&now);
                    changed = true;
                }
            }
        }
        // Changed from what it ships with: a small reset button brings it back.
        if !s.is_default() && !ui.is_pressing(id) {
            let rs = 22.0;
            if ui.icon_button(c, &format!("{id}#reset"), control_x - rs - 6.0, y + (h - rs) / 2.0, rs, theme::I_RESET, Some("Reset to default")) {
                s.reset();
                changed = true;
            }
        }
        changed
    }
}

/// `ModuleBinding.text`: a chord as the player reads it (`Ctrl + Shift + K`).
pub fn binding_text(key: i32, modifiers: i32) -> String {
    if key < 0 {
        return "Not bound".to_owned();
    }
    let mut parts: Vec<String> = Vec::new();
    if modifiers & ui::MOD_CONTROL != 0 {
        parts.push("Ctrl".to_owned());
    }
    if modifiers & ui::MOD_SHIFT != 0 {
        parts.push("Shift".to_owned());
    }
    if modifiers & ui::MOD_ALT != 0 {
        parts.push("Alt".to_owned());
    }
    parts.push(keys::name(key));
    parts.join(" + ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pixel_drawing_mirrors_into_all_four_corners() {
        let mut s = Setting::pixels("p", "P", 5, vec![false; 25]);
        s.pixel_set(0, 1, true);
        assert!(s.pixel_get(0, 1) && s.pixel_get(4, 1) && s.pixel_get(0, 3) && s.pixel_get(4, 3));
        s.pixel_save_new();
        assert_eq!(s.pixel_current(), Some(0));
        s.pixel_clear();
        assert!(s.pixel_empty());
        s.pixel_use(0);
        assert!(!s.pixel_empty());
    }

    #[test]
    fn chords_read_like_the_java_mod() {
        assert_eq!(binding_text(-1, 0), "Not bound");
        assert_eq!(binding_text(75, ui::MOD_CONTROL | ui::MOD_SHIFT), "Ctrl + Shift + K");
    }
}
