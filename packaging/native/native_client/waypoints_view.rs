//! The Waypoints tab of the Native menu + the floating "Create A Waypoint" / "Edit Waypoint" modal
//! (mirrors the Java mod's `WaypointsView`).

use super::keys;
use super::menu::{CARD, CARD_HOVER};
use super::scroll::Scroll;
use super::textfield::TextField;
use super::ui::{self, Ui};
use super::waypoints::{self, Point};
use super::waypoints_module;
use horizon_client::native_ui::canvas::clock_ms;
use horizon_client::native_ui::{Canvas, font, theme};

const ROW: f32 = 56.0;
const MODAL_BG: u32 = 0xEE0B0C10;
const CHECK_BLUE: u32 = 0xFF2563EB;
const HUE_STOPS: [u32; 7] = [0xFFFF0000, 0xFFFFFF00, 0xFF00FF00, 0xFF00FFFF, 0xFF0000FF, 0xFFFF00FF, 0xFFFF0000];

pub struct WaypointsView {
    scroll: Scroll,
    name: TextField,
    coord_x: TextField,
    coord_y: TextField,
    coord_z: TextField,
    hex_field: TextField,

    modal_open: bool,
    quick_modal: bool,
    close_host_pending: bool,
    focus_name_pending: bool,
    editing: Option<u64>,
    modal_world: Option<String>,
    modal_dim: String,
    modal_icon: usize,

    hue: f32,
    sat: f32,
    val: f32,
    alpha: f32,
    chroma: bool,
    chroma_speed: i32,
    chroma_type: usize,

    confirm_delete: Option<u64>,
    all_dims: bool,
    /// The world shown when you are not in one.
    browse: Option<String>,
    notice: Option<(String, f64)>,
}

impl WaypointsView {
    pub fn new() -> Self {
        let mut name = TextField::new("waypoints:name");
        name.max_length = 32;
        let mut coord_x = TextField::new("waypoints:x");
        coord_x.max_length = 12;
        let mut coord_y = TextField::new("waypoints:y");
        coord_y.max_length = 12;
        let mut coord_z = TextField::new("waypoints:z");
        coord_z.max_length = 12;
        let mut hex_field = TextField::new("waypoints:hex");
        hex_field.max_length = 9;
        WaypointsView {
            scroll: Scroll::new("waypoints:list"),
            name,
            coord_x,
            coord_y,
            coord_z,
            hex_field,
            modal_open: false,
            quick_modal: false,
            close_host_pending: false,
            focus_name_pending: false,
            editing: None,
            modal_world: None,
            modal_dim: waypoints::OVERWORLD.to_owned(),
            modal_icon: 0,
            hue: 0.35,
            sat: 0.78,
            val: 0.65,
            alpha: 1.0,
            chroma: false,
            chroma_speed: 40,
            chroma_type: 0,
            confirm_delete: None,
            all_dims: true,
            browse: None,
            notice: None,
        }
    }

    pub fn opened(&mut self) {
        if !self.quick_modal {
            self.modal_open = false;
            self.editing = None;
        }
        self.confirm_delete = None;
    }

    pub fn open_quick_create(&mut self, pos: [f64; 3], dim: &str) {
        let here = waypoints::here().clone();
        let Some(world) = here.world else { return };
        let dim = if dim.is_empty() {
            if here.dim.is_empty() { waypoints::OVERWORLD } else { &here.dim }
        } else {
            dim
        };
        self.open_create_state(&world, dim, pos, true);
    }

    pub fn cancel_quick_create(&mut self) {
        if self.quick_modal {
            self.quick_modal = false;
            self.modal_open = false;
            self.editing = None;
            self.close_host_pending = false;
        }
    }

    pub fn is_quick_modal(&self) -> bool {
        self.modal_open && self.quick_modal
    }

    pub fn consume_close_host(&mut self) -> bool {
        let r = self.close_host_pending;
        self.close_host_pending = false;
        r
    }

    pub fn escape(&mut self, ui: &mut Ui) -> bool {
        if self.modal_open {
            let was_quick = self.quick_modal;
            self.modal_open = false;
            self.quick_modal = false;
            self.editing = None;
            ui.focus = None;
            if was_quick {
                self.close_host_pending = true;
            }
            return true;
        }
        self.confirm_delete.take().is_some()
    }

    /// Draws the floating "Create A Waypoint" modal over the live world. Returns true when it should close.
    pub fn draw_quick_modal(&mut self, ui: &mut Ui, c: &mut Canvas, w_all: f32, h_all: f32) -> bool {
        if !self.modal_open || !self.quick_modal {
            return true;
        }
        c.fill(0.0, 0.0, w_all, h_all, 0x58000000);
        if self.draw_modal_card(ui, c, 0.0, 0.0, w_all, h_all) {
            self.modal_open = false;
            self.quick_modal = false;
            self.editing = None;
            ui.focus = None;
            return true;
        }
        false
    }

    pub fn draw(&mut self, ui: &mut Ui, c: &mut Canvas, x: f32, y: f32, w: f32, h: f32) {
        let here = waypoints::here().clone();
        let in_world = here.world.is_some();
        let worlds = waypoints::store().worlds();
        let world = here.world.clone().or_else(|| {
            self.browse.clone().filter(|b| worlds.contains(b)).or_else(|| worlds.first().cloned())
        });
        let key = keys::name(waypoints_module::add_key());
        let dim = if here.dim.is_empty() { waypoints::OVERWORLD.to_owned() } else { here.dim.clone() };

        let prev_interactive = ui.interactive;
        if self.modal_open {
            ui.interactive = false;
        }

        // Top: where you are, and the quick-action buttons
        let title = if in_world {
            format!("{}  \u{B7}  {}", here.label.clone().unwrap_or_else(|| "This world".into()), waypoints::dimension_name(&dim))
        } else {
            "Not in a world".into()
        };
        c.text(font::SEMIBOLD, 14.0, &title, x, y + 2.0, theme::TEXT_STRONG);
        let hint = if in_world {
            format!("Press {key} to create a waypoint where you stand, hold it to mark the block you look at.")
        } else {
            "Join a world to add waypoints. These are the ones you saved before.".into()
        };
        c.text(font::MEDIUM, 11.5, &hint, x, y + 24.0, theme::TEXT_MUTED);
        let mut bx = x + w;
        if let Some(wk) = here.world.as_deref() {
            bx -= 114.0;
            if ui.button(c, "waypoints:add", bx, y, 114.0, 34.0, "Add here", theme::I_PLUS, ui::BTN_PRIMARY) {
                self.open_create_state(wk, &dim, here.pos, false);
                self.name.focus(ui);
                self.name.select_all();
                self.focus_name_pending = false;
            }
            bx -= 154.0;
            if ui.button(c, "waypoints:paste", bx, y, 146.0, 34.0, "Paste coords", theme::I_MAP_PIN, ui::BTN_GLASS) {
                let clip = ui.clip_in.clone();
                self.paste(wk, &dim, here.pos, &clip);
            }
        }
        let mut ty = y + 50.0;

        let world_points = world.as_deref().map(|wk| waypoints::store().of(wk)).unwrap_or_default();
        let death_count = world_points.iter().filter(|p| p.death).count();

        // Worlds to browse (outside a world) / dimension filter (inside one)
        if !in_world {
            let mut cx = x;
            for k in &worlds {
                let label = world_name(k);
                let cw = c.text_width(font::MEDIUM, 11.5, &label) + 26.0;
                if cx + cw > x + w - if death_count > 0 { 132.0 } else { 0.0 } {
                    break;
                }
                if chip(ui, c, &format!("waypoints:world:{k}"), cx, ty, cw, &label, world.as_deref() == Some(k)) {
                    self.browse = Some(k.clone());
                    self.modal_open = false;
                    self.editing = None;
                }
                cx += cw + 6.0;
            }
            if worlds.is_empty() {
                empty(c, x, ty + 30.0, w, "No waypoints yet", &format!("Join a world and press {key} to mark a place."));
                ui.interactive = prev_interactive;
                self.notices(c, x, y, w, h);
                return;
            }
        } else {
            if chip(ui, c, "waypoints:dims:all", x, ty, 78.0, "All", self.all_dims) {
                self.all_dims = true;
            }
            let d = waypoints::dimension_name(&dim);
            let dw = c.text_width(font::MEDIUM, 11.5, &d) + 26.0;
            if chip(ui, c, "waypoints:dims:here", x + 84.0, ty, dw, &d, !self.all_dims) {
                self.all_dims = false;
            }
        }
        if death_count > 0
            && let Some(wk) = world.as_deref()
        {
            let label = if death_count == 1 { "Clear death".to_owned() } else { format!("Clear deaths ({death_count})") };
            let bw = (c.text_width(font::SEMIBOLD, 11.0, &label) + 34.0).max(114.0);
            if ui.button(c, "waypoints:clear-deaths", x + w - bw, ty, bw, 28.0, &label, theme::I_TRASH, ui::BTN_GLASS) {
                let ids: Vec<u64> = world_points.iter().filter(|p| p.death).map(|p| p.id).collect();
                let mut s = waypoints::store();
                for id in ids {
                    s.remove(wk, id);
                }
                drop(s);
                self.say("Cleared death points".into());
            }
        }
        ty += 38.0;
        c.fill(x, ty - 8.0, w, 1.0, theme::HAIRLINE);

        // The list
        if let Some(world) = world {
            let mut rows: Vec<Point> = waypoints::store()
                .of(&world)
                .into_iter()
                .filter(|p| !in_world || self.all_dims || p.dim == dim)
                .collect();
            if in_world {
                rows.sort_by(|a, b| sq_dist(a, here.pos, &dim).total_cmp(&sq_dist(b, here.pos, &dim)));
            }
            let lh = y + h - ty;
            if rows.is_empty() {
                let sub = if in_world { format!("Press {key} or Add here to mark where you stand.") } else { String::new() };
                empty(c, x, ty + 20.0, w, if in_world { "No waypoints here yet" } else { "No waypoints" }, &sub);
            } else {
                let off = self.scroll.begin(ui, c, x, ty, w + 6.0, lh);
                let mut ry = ty - off;
                let mut remove = None;
                for p in &rows {
                    if ry + ROW >= ty && ry <= ty + lh && self.row(ui, c, &world, p, x, ry, w, in_world.then_some(here.pos), &dim) {
                        remove = Some(p.id);
                    }
                    ry += ROW + 8.0;
                }
                self.scroll.end(ui, c, ry + off - ty);
                if let Some(id) = remove {
                    if self.editing == Some(id) {
                        self.editing = None;
                        self.modal_open = false;
                    }
                    waypoints::store().remove(&world, id);
                    self.confirm_delete = None;
                }
            }
        }

        ui.interactive = prev_interactive;
        if self.modal_open {
            c.round(x - 16.0, y - 16.0, w + 32.0, h + 32.0, 12.0, 0x99000000);
            if self.draw_modal_card(ui, c, x, y, w, h) {
                self.modal_open = false;
                self.quick_modal = false;
                self.editing = None;
                ui.focus = None;
            }
        }
        self.notices(c, x, y, w, h);
    }

    /// One waypoint row; true when its delete was confirmed.
    #[allow(clippy::too_many_arguments)]
    fn row(&mut self, ui: &mut Ui, c: &mut Canvas, world: &str, p: &Point, x: f32, y: f32, w: f32, me: Option<[f64; 3]>, dim: &str) -> bool {
        let id = format!("waypoints:row:{}", p.id);
        let over = ui.hover(c, x, y, w, ROW);
        c.round(x, y, w, ROW, 10.0, if over { CARD_HOVER } else { CARD });
        c.outline(x, y, w, ROW, 10.0, 1.0, theme::HAIRLINE);
        let cy = y + ROW / 2.0;
        let col = waypoints::live_color(p, ui.now);
        c.push_alpha(if p.visible { 1.0 } else { 0.45 });
        c.round(x + 14.0, cy - 14.0, 28.0, 28.0, 7.0, theme::alpha(col, 0.20));
        c.outline(x + 14.0, cy - 14.0, 28.0, 28.0, 7.0, 1.2, theme::alpha(col, 0.92));
        c.icon(waypoints::ICONS[p.icon.min(waypoints::ICONS.len() - 1)], 13.0, x + 28.0, cy, col);
        let tx = x + 54.0;
        let name = c.ellipsize(font::SEMIBOLD, 13.0, &p.name, w - 240.0);
        c.text(font::SEMIBOLD, 13.0, &name, tx, y + 10.0, theme::TEXT_STRONG);
        let mut sub = format!("({}, {}, {})  \u{B7}  {}", p.x, p.y, p.z, waypoints::dimension_name(&p.dim));
        if p.chroma {
            sub.push_str("  \u{B7}  Chroma");
        }
        if let Some(me) = me
            && let Some(at) = waypoints::place_in(p, dim, true)
        {
            let d = ((at[0] - me[0]).powi(2) + (at[1] - me[1]).powi(2) + (at[2] - me[2]).powi(2)).sqrt();
            sub.push_str(&format!("  \u{B7}  {}", waypoints::distance(d)));
            if p.dim != dim {
                sub.push_str(&format!(" (as {} {} {})", waypoints::dimension_name(dim), at[0].floor() as i64, at[2].floor() as i64));
            }
        }
        let sub = c.ellipsize(font::MEDIUM, 11.0, &sub, w - 210.0);
        c.text(font::MEDIUM, 11.0, &sub, tx, y + 30.0, theme::TEXT_MUTED);
        c.pop_alpha();

        // Actions, right to left
        let bs = 32.0;
        let by = cy - bs / 2.0;
        let mut deleted = false;
        if self.confirm_delete == Some(p.id) {
            let dw = 92.0;
            let mut bx = x + w - 12.0 - dw;
            if ui.button(c, &format!("{id}:really"), bx, by, dw, bs, "Delete", theme::I_TRASH, ui::BTN_DANGER) {
                deleted = true;
            }
            bx -= bs + 6.0;
            if ui.button(c, &format!("{id}:keep"), bx, by, bs, bs, "", theme::I_X, ui::BTN_GHOST) {
                self.confirm_delete = None;
            }
        } else {
            let mut bx = x + w - 12.0 - bs;
            if ui.icon_button(c, &format!("{id}:delete"), bx, by, bs, theme::I_TRASH, Some("Delete")) {
                self.confirm_delete = Some(p.id);
            }
            bx -= bs + 4.0;
            if ui.icon_button(c, &format!("{id}:edit"), bx, by, bs, theme::I_PENCIL, Some("Edit")) {
                self.open_edit_state(ui, world, p);
            }
            bx -= bs + 4.0;
            if ui.icon_button(c, &format!("{id}:copy"), bx, by, bs, theme::I_LIST, Some("Copy name and coordinates")) {
                let text = waypoints::describe(p);
                ui.clip_out = Some(text.clone());
                self.say(format!("Copied \"{text}\""));
            }
            bx -= bs + 4.0;
            let eye = if p.visible { theme::I_EYE } else { theme::I_EYE_OFF };
            let tip = if p.visible { "Hide in the world" } else { "Show in the world" };
            if ui.icon_button(c, &format!("{id}:eye"), bx, by, bs, eye, Some(tip)) {
                waypoints::store().edit(world, p.id, |q| q.visible = !q.visible);
            }
        }
        deleted
    }

    fn open_create_state(&mut self, world: &str, dim: &str, pos: [f64; 3], quick: bool) {
        self.modal_open = true;
        self.quick_modal = quick;
        self.close_host_pending = false;
        self.editing = None;
        self.modal_world = Some(world.to_owned());
        self.modal_dim = if dim.is_empty() { waypoints::OVERWORLD.to_owned() } else { dim.to_owned() };
        self.modal_icon = 0;
        self.name.set("");
        self.coord_x.set(&format_coord(pos[0]));
        self.coord_y.set(&format_coord(pos[1]));
        self.coord_z.set(&format_coord(pos[2]));
        let col = waypoints::store().next_color(world);
        self.load_color_into_picker(col, false, 40, 0);
        self.focus_name_pending = true;
    }

    fn open_edit_state(&mut self, ui: &mut Ui, world: &str, p: &Point) {
        self.modal_open = true;
        self.quick_modal = false;
        self.close_host_pending = false;
        self.editing = Some(p.id);
        self.modal_world = Some(world.to_owned());
        self.modal_dim = p.dim.clone();
        self.modal_icon = p.icon;
        self.name.set(&p.name);
        self.coord_x.set(&p.x.to_string());
        self.coord_y.set(&p.y.to_string());
        self.coord_z.set(&p.z.to_string());
        self.load_color_into_picker(p.color, p.chroma, p.chroma_speed, p.chroma_type);
        self.name.focus(ui);
        self.name.select_all();
        self.focus_name_pending = false;
    }

    fn load_color_into_picker(&mut self, argb: u32, chroma: bool, speed: i32, ctype: usize) {
        let [h, s, v] = waypoints::argb_to_hsv(argb);
        self.hue = h;
        self.sat = s;
        self.val = v;
        let a = ((argb >> 24) & 0xFF) as u8;
        self.alpha = f32::from(if a == 0 { 255 } else { a }) / 255.0;
        self.chroma = chroma;
        self.chroma_speed = speed.clamp(1, 100);
        self.chroma_type = ctype.min(waypoints::CHROMA_TYPES.len() - 1);
        self.sync_hex_from_picker();
    }

    fn picker_color(&self) -> u32 {
        let a = (self.alpha * 255.0).round().clamp(0.0, 255.0) as u8;
        waypoints::hsv_to_argb(self.hue, self.sat, self.val, a)
    }

    fn sync_hex_from_picker(&mut self) {
        self.hex_field.set(&format!("#{:08x}", self.picker_color()));
    }

    fn preview_chroma_color(&self, now_ms: f64, base_argb: u32) -> u32 {
        let mut tmp = Point::new("", 0, 0, 0, &self.modal_dim, base_argb, 0);
        tmp.chroma = true;
        tmp.chroma_speed = self.chroma_speed;
        tmp.chroma_type = self.chroma_type;
        waypoints::live_color(&tmp, now_ms)
    }

    fn draw_modal_card(&mut self, ui: &mut Ui, c: &mut Canvas, bx: f32, by: f32, bw: f32, bh: f32) -> bool {
        if self.focus_name_pending {
            self.name.focus(ui);
            self.focus_name_pending = false;
        }
        let mw = (bw - 24.0).min(434.0);
        let mh = 354.0;
        let mx = bx + (bw - mw) / 2.0;
        let my = by + (bh - mh) / 2.0;

        // Dark glass modal card
        c.round(mx, my, mw, mh, 12.0, MODAL_BG);
        c.outline(mx, my, mw, mh, 12.0, 1.0, theme::HAIRLINE_STRONG);

        // Header
        let heading = if self.editing.is_none() { "Create A Waypoint" } else { "Edit Waypoint" };
        let hw = c.text_width(font::SEMIBOLD, 15.0, heading);
        c.text(font::SEMIBOLD, 15.0, heading, mx + (mw - hw) / 2.0, my + 14.0, theme::TEXT_STRONG);
        c.fill(mx + 18.0, my + 44.0, mw - 36.0, 1.0, theme::HAIRLINE);

        let ix = mx + 20.0;
        let iw = mw - 40.0;
        let mut submit = false;

        // Row 1: Name
        if self.name.draw(ui, c, ix, my + 56.0, iw, 32.0, "Waypoint name", 12.5) {
            submit = true;
        }

        // Row 2: X / Y / Z coordinates
        let cy = my + 96.0;
        let gap = 8.0;
        let cw = (iw - gap * 2.0) / 3.0;
        if self.coord_x.draw(ui, c, ix, cy, cw, 30.0, "X", 12.0) { submit = true; }
        draw_axis_tag(c, ix, cy, cw, 30.0, "X");
        if self.coord_y.draw(ui, c, ix + cw + gap, cy, cw, 30.0, "Y", 12.0) { submit = true; }
        draw_axis_tag(c, ix + cw + gap, cy, cw, 30.0, "Y");
        if self.coord_z.draw(ui, c, ix + (cw + gap) * 2.0, cy, cw, 30.0, "Z", 12.0) { submit = true; }
        draw_axis_tag(c, ix + (cw + gap) * 2.0, cy, cw, 30.0, "Z");

        // Row 3: Color header (label left, swatch + hex right)
        let col_y = my + 138.0;
        c.text(font::SEMIBOLD, 12.0, "Color", ix, col_y + 4.0, theme::TEXT_STRONG);
        let current_argb = self.picker_color();
        let preview_argb = if self.chroma { self.preview_chroma_color(ui.now, current_argb) } else { current_argb };
        let hex_w = 92.0;
        let hex_x = ix + iw - hex_w;
        let sw_x = hex_x - 24.0;
        draw_checker(c, sw_x, col_y + 2.0, 18.0, 18.0, 4.5);
        c.round(sw_x, col_y + 2.0, 18.0, 18.0, 4.0, preview_argb);
        c.outline(sw_x, col_y + 2.0, 18.0, 18.0, 4.0, 1.0, 0x66FFFFFF);
        let hex_focused_before = self.hex_field.focused(ui);
        let hex_before = self.hex_field.value();
        if self.hex_field.draw(ui, c, hex_x, col_y - 1.0, hex_w, 24.0, "#aarrggbb", 11.0) {
            submit = true;
        }
        let hex_after = self.hex_field.value();
        if self.hex_field.focused(ui) && hex_after != hex_before {
            if let Some(parsed) = parse_hex_argb(&hex_after) {
                let [h, s, v] = waypoints::argb_to_hsv(parsed);
                self.hue = h;
                self.sat = s;
                self.val = v;
                self.alpha = ((parsed >> 24) & 0xFF) as f32 / 255.0;
            }
        } else if hex_focused_before && !self.hex_field.focused(ui) {
            self.sync_hex_from_picker();
        }

        // Row 4: Color picker body
        let py = my + 168.0;
        let ph = 122.0;

        // Left column: Chroma, Speed, Type
        let lx = ix;
        let lw = 104.0;
        c.text(font::MEDIUM, 11.5, "Chroma", lx, py + 4.0, theme::TEXT_SECONDARY);
        let (chk_x, chk_y) = (lx + lw - 18.0, py + 2.0);
        let over_chk = ui.hover(c, chk_x - 4.0, chk_y - 2.0, 24.0, 22.0);
        c.round(chk_x, chk_y, 18.0, 18.0, 4.0, if self.chroma { CHECK_BLUE } else if over_chk { 0xDD1A1D26 } else { 0xCC111319 });
        c.outline(chk_x, chk_y, 18.0, 18.0, 4.0, 1.0, if self.chroma { 0x6693C5FD } else { theme::HAIRLINE_STRONG });
        if self.chroma {
            c.icon(theme::I_CHECK, 11.0, chk_x + 9.0, chk_y + 9.0, 0xFFFFFFFF);
        }
        if ui.clicked(c, "wp:chroma", lx, py, lw, 22.0) {
            self.chroma = !self.chroma;
        }

        // Speed slider (1..100)
        let spd_y = py + 34.0;
        c.text(font::MEDIUM, 11.0, "Speed", lx, spd_y, theme::TEXT_MUTED);
        let spd_str = self.chroma_speed.to_string();
        let spd_w = c.text_width(font::SEMIBOLD, 11.0, &spd_str);
        c.text(font::SEMIBOLD, 11.0, &spd_str, lx + lw - spd_w, spd_y, theme::TEXT_STRONG);
        let sl_y = spd_y + 19.0;
        if ui.hover(c, lx, sl_y - 5.0, lw, 16.0) {
            ui.cursor_hand = true;
            if ui.pressed {
                ui.active = Some("wp:speed".to_owned());
            }
        }
        if ui.active.as_deref() == Some("wp:speed") && ui.down {
            let t = ((ui.mx - lx) / lw).clamp(0.0_f32, 1.0_f32);
            self.chroma_speed = (1.0_f32 + t * 99.0_f32).round().clamp(1.0, 100.0) as i32;
        }
        let spd_frac = (self.chroma_speed - 1) as f32 / 99.0;
        c.round(lx, sl_y, lw, 4.0, 2.0, 0xFF23262F);
        c.round(lx, sl_y, (lw * spd_frac).max(4.0), 4.0, 2.0, 0xFF3B82F6);
        c.circle(lx + lw * spd_frac, sl_y + 2.0, 5.5, 0xFFFFFFFF);
        c.circle(lx + lw * spd_frac, sl_y + 2.0, 3.5, 0xFF3B82F6);

        // Type selector (< Wave >)
        let typ_y = py + 76.0;
        c.text(font::MEDIUM, 11.0, "Type", lx, typ_y, theme::TEXT_MUTED);
        let (pill_y, pill_h) = (typ_y + 18.0, 26.0);
        c.round(lx, pill_y, lw, pill_h, 6.0, 0xCC111319);
        c.outline(lx, pill_y, lw, pill_h, 6.0, 1.0, theme::HAIRLINE);
        if ui.clicked(c, "wp:type:prev", lx, pill_y, 22.0, pill_h) {
            self.chroma_type = (self.chroma_type + waypoints::CHROMA_TYPES.len() - 1) % waypoints::CHROMA_TYPES.len();
        }
        if ui.clicked(c, "wp:type:next", lx + lw - 22.0, pill_y, 22.0, pill_h) {
            self.chroma_type = (self.chroma_type + 1) % waypoints::CHROMA_TYPES.len();
        }
        c.icon(
            theme::I_CHEVRON_LEFT,
            11.0,
            lx + 11.0,
            pill_y + pill_h / 2.0,
            if ui.hover(c, lx, pill_y, 22.0, pill_h) { theme::TEXT_STRONG } else { theme::TEXT_MUTED },
        );
        c.icon(
            theme::I_CHEVRON_RIGHT,
            11.0,
            lx + lw - 11.0,
            pill_y + pill_h / 2.0,
            if ui.hover(c, lx + lw - 22.0, pill_y, 22.0, pill_h) { theme::TEXT_STRONG } else { theme::TEXT_MUTED },
        );
        let type_label = waypoints::CHROMA_TYPES[self.chroma_type];
        let tlw = c.text_width(font::SEMIBOLD, 11.0, type_label);
        let tlh = c.line_height(font::SEMIBOLD, 11.0);
        c.text(font::SEMIBOLD, 11.0, type_label, lx + (lw - tlw) / 2.0, pill_y + (pill_h - tlh) / 2.0, theme::TEXT_STRONG);

        // Right-anchored layout for Presets, Alpha, Hue, and 2D SV box
        let (pre_size, pre_gap) = (17.0f32, 4.0f32);
        let pre_w = pre_size * 2.0 + pre_gap;
        let pre_x = ix + iw - pre_w;
        let bar_w = 14.0f32;
        let alp_x = pre_x - 10.0 - bar_w;
        let hue_x = alp_x - 8.0 - bar_w;
        let sv_x = lx + lw + 12.0;
        let sv_w = hue_x - 8.0 - sv_x;

        let mut picker_changed = false;

        // 2D Saturation x Value gradient box
        if ui.hover(c, sv_x, py, sv_w, ph) {
            ui.cursor_hand = true;
            if ui.pressed {
                ui.active = Some("wp:sv".to_owned());
            }
        }
        if ui.active.as_deref() == Some("wp:sv") && ui.down {
            self.sat = ((ui.mx - sv_x) / sv_w).clamp(0.0, 1.0);
            self.val = 1.0 - ((ui.my - py) / ph).clamp(0.0, 1.0);
            picker_changed = true;
        }
        let pure_hue = waypoints::hsv_to_argb(self.hue, 1.0, 1.0, 255);
        c.gradient_h(sv_x, py, sv_w, ph, 0xFFFFFFFF, pure_hue);
        c.gradient_v(sv_x, py, sv_w, ph, 0x00000000, 0xFF000000);
        c.outline(sv_x, py, sv_w, ph, 2.0, 1.0, 0xAAFFFFFF);
        let cur_x = sv_x + self.sat * sv_w;
        let cur_y = py + (1.0 - self.val) * ph;
        c.circle(cur_x, cur_y, 5.5, 0xCC000000);
        c.circle(cur_x, cur_y, 4.3, 0xFFFFFFFF);
        c.circle(cur_x, cur_y, 2.7, waypoints::hsv_to_argb(self.hue, self.sat, self.val, 255));

        // Vertical Rainbow Hue bar
        if ui.hover(c, hue_x - 2.0, py, bar_w + 4.0, ph) {
            ui.cursor_hand = true;
            if ui.pressed {
                ui.active = Some("wp:hue".to_owned());
            }
        }
        if ui.active.as_deref() == Some("wp:hue") && ui.down {
            self.hue = ((ui.my - py) / ph).clamp(0.0, 0.9999);
            picker_changed = true;
        }
        let seg_h = ph / 6.0;
        for i in 0..6 {
            c.gradient_v(hue_x, py + i as f32 * seg_h, bar_w, seg_h + 0.5, HUE_STOPS[i], HUE_STOPS[i + 1]);
        }
        c.outline(hue_x, py, bar_w, ph, 2.0, 1.0, theme::HAIRLINE_STRONG);
        let hy = py + self.hue * ph;
        c.round(hue_x - 2.0, hy - 2.5, bar_w + 4.0, 5.0, 2.0, 0xFFFFFFFF);
        c.outline(hue_x - 2.0, hy - 2.5, bar_w + 4.0, 5.0, 2.0, 1.0, 0xCC000000);

        // Vertical Alpha checkerboard + gradient bar
        if ui.hover(c, alp_x - 2.0, py, bar_w + 4.0, ph) {
            ui.cursor_hand = true;
            if ui.pressed {
                ui.active = Some("wp:alpha".to_owned());
            }
        }
        if ui.active.as_deref() == Some("wp:alpha") && ui.down {
            self.alpha = 1.0 - ((ui.my - py) / ph).clamp(0.0, 1.0);
            picker_changed = true;
        }
        draw_checker(c, alp_x, py, bar_w, ph, 4.5);
        let opaque_rgb = waypoints::hsv_to_argb(self.hue, self.sat, self.val, 255);
        c.gradient_v(alp_x, py, bar_w, ph, opaque_rgb, opaque_rgb & 0x00FF_FFFF);
        c.outline(alp_x, py, bar_w, ph, 2.0, 1.0, theme::HAIRLINE_STRONG);
        let ay = py + (1.0 - self.alpha) * ph;
        c.round(alp_x - 2.0, ay - 2.5, bar_w + 4.0, 5.0, 2.0, 0xFFFFFFFF);
        c.outline(alp_x - 2.0, ay - 2.5, bar_w + 4.0, 5.0, 2.0, 1.0, 0xCC000000);

        // 2x6 Preset swatches
        let cur_rgb = self.picker_color() & 0x00FF_FFFF;
        for (i, col) in waypoints::PRESET_COLORS.iter().copied().enumerate() {
            let col_idx = (i % 2) as f32;
            let row_idx = (i / 2) as f32;
            let sx = pre_x + col_idx * (pre_size + pre_gap);
            let sy = py + row_idx * (pre_size + pre_gap);
            let over = ui.hover(c, sx, sy, pre_size, pre_size);
            let sel = !self.chroma && (col & 0x00FF_FFFF) == cur_rgb;
            c.round(sx, sy, pre_size, pre_size, 4.0, col);
            if sel {
                c.outline(sx - 1.0, sy - 1.0, pre_size + 2.0, pre_size + 2.0, 4.5, 1.5, 0xFFFFFFFF);
            } else if over {
                c.outline(sx, sy, pre_size, pre_size, 4.0, 1.0, 0xBBFFFFFF);
            }
            if ui.clicked(c, &format!("wp:preset:{i}"), sx, sy, pre_size, pre_size) {
                let [h, s, v] = waypoints::argb_to_hsv(col);
                self.hue = h;
                self.sat = s;
                self.val = v;
                self.alpha = 1.0;
                self.chroma = false;
                picker_changed = true;
            }
        }

        if picker_changed {
            self.sync_hex_from_picker();
        }

        // Footer hairline + Back (left) + SAVE (right)
        c.fill(mx + 18.0, my + 300.0, mw - 36.0, 1.0, theme::HAIRLINE);
        let foot_y = my + 311.0;
        if ui.button(c, "wp:back", ix, foot_y, 44.0, 30.0, "", theme::I_ARROW_LEFT, ui::BTN_GLASS) {
            return true;
        }
        if ui.button(c, "wp:save", ix + iw - 86.0, foot_y, 86.0, 30.0, "SAVE", 0, ui::BTN_PRIMARY) || submit {
            self.save_modal();
            return true;
        }
        false
    }

    fn save_modal(&mut self) {
        let here = waypoints::here().clone();
        let Some(world) = self.modal_world.clone().or(here.world) else { return };
        let typed_name = self.name.value().trim().to_owned();
        let x = parse_coord(&self.coord_x.value(), here.pos[0].floor() as i32);
        let y = parse_coord(&self.coord_y.value(), here.pos[1].floor() as i32);
        let z = parse_coord(&self.coord_z.value(), here.pos[2].floor() as i32);
        let col = self.picker_color();
        let chroma = self.chroma;
        let speed = self.chroma_speed;
        let ctype = self.chroma_type;

        if let Some(id) = self.editing {
            let mut saved_name = String::new();
            waypoints::store().edit(&world, id, |q| {
                if !typed_name.is_empty() {
                    q.name = typed_name;
                }
                q.x = x;
                q.y = y;
                q.z = z;
                q.color = col;
                q.chroma = chroma;
                q.chroma_speed = speed;
                q.chroma_type = ctype;
                saved_name = q.name.clone();
            });
            if !saved_name.is_empty() {
                self.say(format!("Saved {saved_name}"));
            }
        } else {
            let mut s = waypoints::store();
            let final_name = if typed_name.is_empty() { s.next_name(&world, "Waypoint") } else { typed_name };
            let mut p = Point::new(&final_name, x, y, z, &self.modal_dim, col, self.modal_icon);
            p.chroma = chroma;
            p.chroma_speed = speed;
            p.chroma_type = ctype;
            let ok = s.add(&world, p);
            drop(s);
            self.say(if ok {
                format!("Added {final_name}")
            } else {
                format!("This world has {} waypoints already", waypoints::MAX_PER_WORLD)
            });
        }
    }

    /// Coordinates from the clipboard ("120 64 -300", "x: 120 z: -300"), in this dimension.
    fn paste(&mut self, world: &str, dim: &str, me: [f64; 3], text: &str) {
        let Some((x, y, z)) = waypoints::parse_coords(text) else {
            self.say("No coordinates on the clipboard (copy something like 120 64 -300)".into());
            return;
        };
        let y = y.unwrap_or(me[1].floor() as i32);
        let mut s = waypoints::store();
        let lead = text.trim().split(|ch: char| ch.is_whitespace() || ch == ':' || ch == ',').next().unwrap_or("");
        let name = if lead.len() <= 32
            && lead.chars().next().is_some_and(|ch| ch.is_ascii_alphabetic())
            && lead.chars().all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '\'' || ch == '-')
            && !matches!(lead.to_ascii_lowercase().as_str(), "x" | "y" | "z")
        {
            lead.to_owned()
        } else {
            s.next_name(world, "Waypoint")
        };
        let color = s.next_color(world);
        if s.add(world, Point::new(&name, x, y, z, dim, color, 0)) {
            drop(s);
            self.say(format!("Added {name} at {x} {y} {z}"));
        }
    }

    fn say(&mut self, text: String) {
        self.notice = Some((text, clock_ms()));
    }

    fn notices(&mut self, c: &mut Canvas, x: f32, y: f32, w: f32, h: f32) {
        let Some((text, at)) = self.notice.clone() else { return };
        let age = clock_ms() - at;
        if age > 2500.0 {
            self.notice = None;
            return;
        }
        let a = (age / 150.0).min((2500.0 - age) / 400.0).min(1.0) as f32;
        let tw = c.text_width(font::MEDIUM, 11.5, &text);
        let lh = c.line_height(font::MEDIUM, 11.5);
        let (nx, ny) = (x + (w - tw) / 2.0, y + h - lh - 16.0);
        c.push_alpha(a);
        c.round(nx - 14.0, ny - 8.0, tw + 28.0, lh + 16.0, 10.0, 0xE6101114);
        c.outline(nx - 14.0, ny - 8.0, tw + 28.0, lh + 16.0, 10.0, 1.0, theme::HAIRLINE_STRONG);
        c.text(font::MEDIUM, 11.5, &text, nx, ny, theme::TEXT_STRONG);
        c.pop_alpha();
    }
}

fn draw_axis_tag(c: &mut Canvas, x: f32, y: f32, w: f32, h: f32, axis: &str) {
    let tw = c.text_width(font::SEMIBOLD, 10.0, axis);
    let lh = c.line_height(font::SEMIBOLD, 10.0);
    c.text(font::SEMIBOLD, 10.0, axis, x + w - tw - 8.0, y + (h - lh) / 2.0, 0x66A1A1AA);
}

fn draw_checker(c: &mut Canvas, x: f32, y: f32, w: f32, h: f32, cell: f32) {
    c.push_clip(x, y, w, h);
    let cols = (w / cell).ceil() as i32;
    let rows = (h / cell).ceil() as i32;
    for r in 0..rows {
        for k in 0..cols {
            let col = if ((r + k) & 1) == 0 { 0xFF3E414A } else { 0xFF23252C };
            c.fill(x + k as f32 * cell, y + r as f32 * cell, cell, cell, col);
        }
    }
    c.pop_clip();
}

fn format_coord(v: f64) -> String {
    let rounded = (v * 100.0).round() / 100.0;
    if (rounded - rounded.round()).abs() < 0.005 {
        format!("{rounded:.1}")
    } else {
        format!("{rounded:.2}")
    }
}

fn parse_coord(s: &str, fallback: i32) -> i32 {
    let t = s.trim();
    if t.is_empty() || t == "-" || t == "+" {
        return fallback;
    }
    t.parse::<f64>().map_or(fallback, |v| v.floor() as i32)
}

fn parse_hex_argb(s: &str) -> Option<u32> {
    let hex = s.trim().trim_start_matches('#');
    if hex.len() != 6 && hex.len() != 8 {
        return None;
    }
    let v = u32::from_str_radix(hex, 16).ok()?;
    Some(if hex.len() == 6 { 0xFF000000 | v } else { v })
}

fn sq_dist(p: &Point, me: [f64; 3], dim: &str) -> f64 {
    waypoints::place_in(p, dim, true).map_or(f64::MAX, |at| (at[0] - me[0]).powi(2) + (at[1] - me[1]).powi(2) + (at[2] - me[2]).powi(2))
}

fn world_name(key: &str) -> String {
    if let Some(s) = key.strip_prefix("server:") {
        return s.to_owned();
    }
    if let Some(n) = key.strip_prefix("world:") {
        return if n.is_empty() { "Singleplayer".into() } else { n.to_owned() };
    }
    if key == "realms" { "Realms".into() } else { key.to_owned() }
}

#[allow(clippy::too_many_arguments)]
fn chip(ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, w: f32, label: &str, on: bool) -> bool {
    let over = ui.hover(c, x, y, w, 28.0);
    c.round(x, y, w, 28.0, 14.0, if on { 0x33FFFFFF } else if over { theme::SUBTLE_HOVER } else { 0x14FFFFFF });
    if on {
        c.outline(x, y, w, 28.0, 14.0, 1.0, theme::HAIRLINE_STRONG);
    }
    let tw = c.text_width(font::MEDIUM, 11.5, label);
    let lh = c.line_height(font::MEDIUM, 11.5);
    c.text(font::MEDIUM, 11.5, label, x + (w - tw) / 2.0, y + (28.0 - lh) / 2.0, if on { theme::TEXT_STRONG } else { theme::TEXT_SECONDARY });
    ui.clicked(c, id, x, y, w, 28.0)
}

fn empty(c: &mut Canvas, x: f32, y: f32, w: f32, title: &str, sub: &str) {
    c.icon(theme::I_MAP_PIN, 30.0, x + w / 2.0, y + 30.0, theme::TEXT_MUTED);
    let tw = c.text_width(font::SEMIBOLD, 14.0, title);
    c.text(font::SEMIBOLD, 14.0, title, x + (w - tw) / 2.0, y + 60.0, theme::TEXT_STRONG);
    let sw = c.text_width(font::MEDIUM, 11.5, sub);
    c.text(font::MEDIUM, 11.5, sub, x + (w - sw) / 2.0, y + 84.0, theme::TEXT_MUTED);
}
