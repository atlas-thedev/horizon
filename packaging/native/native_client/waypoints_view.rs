//! The Waypoints tab of the Native menu (the Java mod's `WaypointsView`): this world's waypoints, nearest first, with
//! their coordinates and distance. Add one where you stand or from coordinates on the clipboard; rename, recolour,
//! pick an icon, hide, copy or delete. Outside a world it browses the waypoints of the worlds you have been to.
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
const EDIT: f32 = 128.0;

pub struct WaypointsView {
    scroll: Scroll,
    name: TextField,
    editing: Option<u64>,
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
        WaypointsView {
            scroll: Scroll::new("waypoints:list"),
            name,
            editing: None,
            confirm_delete: None,
            all_dims: true,
            browse: None,
            notice: None,
        }
    }

    pub fn opened(&mut self) {
        self.editing = None;
        self.confirm_delete = None;
    }

    pub fn escape(&mut self, ui: &mut Ui) -> bool {
        if self.editing.take().is_some() {
            ui.focus = None;
            return true;
        }
        self.confirm_delete.take().is_some()
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

        // top: where you are, and the add buttons
        let title = if in_world {
            format!("{}  \u{B7}  {}", here.label.clone().unwrap_or_else(|| "This world".into()), waypoints::dimension_name(&dim))
        } else {
            "Not in a world".into()
        };
        c.text(font::SEMIBOLD, 14.0, &title, x, y + 2.0, theme::TEXT_STRONG);
        let hint = if in_world {
            format!("Press {key} to add one where you stand, hold it to mark the block you look at.")
        } else {
            "Join a world to add waypoints. These are the ones you saved before.".into()
        };
        c.text(font::MEDIUM, 11.5, &hint, x, y + 24.0, theme::TEXT_MUTED);
        let mut bx = x + w;
        if let Some(wk) = here.world.as_deref() {
            bx -= 118.0;
            if ui.button(c, "waypoints:add", bx, y, 118.0, 34.0, "Add here", theme::I_PLUS, ui::BTN_PRIMARY) {
                self.add_here(wk, &dim, here.pos);
            }
            bx -= 158.0;
            if ui.button(c, "waypoints:paste", bx, y, 150.0, 34.0, "Paste coordinates", theme::I_MAP_PIN, ui::BTN_GLASS) {
                // the clipboard is read for every click on this page (Menu::wants_clipboard)
                let clip = ui.clip_in.clone();
                self.paste(wk, &dim, here.pos, &clip);
            }
        }
        let mut ty = y + 50.0;

        // worlds to browse (outside a world) / dimension filter (inside one)
        if !in_world {
            let mut cx = x;
            for k in &worlds {
                let label = world_name(k);
                let cw = c.text_width(font::MEDIUM, 11.5, &label) + 26.0;
                if cx + cw > x + w {
                    break;
                }
                if chip(ui, c, &format!("waypoints:world:{k}"), cx, ty, cw, &label, world.as_deref() == Some(k)) {
                    self.browse = Some(k.clone());
                    self.editing = None;
                }
                cx += cw + 6.0;
            }
            if worlds.is_empty() {
                empty(c, x, ty + 30.0, w, "No waypoints yet", &format!("Join a world and press {key} to mark a place."));
                self.notices(c, x, y, w, h);
                return;
            }
        } else {
            if chip(ui, c, "waypoints:dims:all", x, ty, 84.0, "All", self.all_dims) {
                self.all_dims = true;
            }
            let d = waypoints::dimension_name(&dim);
            let dw = c.text_width(font::MEDIUM, 11.5, &d) + 26.0;
            if chip(ui, c, "waypoints:dims:here", x + 90.0, ty, dw, &d, !self.all_dims) {
                self.all_dims = false;
            }
        }
        ty += 38.0;
        c.fill(x, ty - 8.0, w, 1.0, theme::HAIRLINE);

        // the list
        let Some(world) = world else {
            self.notices(c, x, y, w, h);
            return;
        };
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
            self.notices(c, x, y, w, h);
            return;
        }
        let off = self.scroll.begin(ui, c, x, ty, w + 6.0, lh);
        let mut ry = ty - off;
        let mut remove = None;
        for p in &rows {
            let rh = if self.editing == Some(p.id) { ROW + EDIT } else { ROW };
            if ry + rh >= ty && ry <= ty + lh && self.row(ui, c, &world, p, x, ry, w, in_world.then_some(here.pos), &dim) {
                remove = Some(p.id);
            }
            ry += rh + 8.0;
        }
        self.scroll.end(ui, c, ry + off - ty);
        if let Some(id) = remove {
            if self.editing == Some(id) {
                self.editing = None;
            }
            waypoints::store().remove(&world, id);
            self.confirm_delete = None;
        }
        self.notices(c, x, y, w, h);
    }

    /// One waypoint; true when its delete was confirmed.
    #[allow(clippy::too_many_arguments)]
    fn row(&mut self, ui: &mut Ui, c: &mut Canvas, world: &str, p: &Point, x: f32, y: f32, w: f32, me: Option<[f64; 3]>, dim: &str) -> bool {
        let id = format!("waypoints:row:{}", p.id);
        let open = self.editing == Some(p.id);
        let h = if open { ROW + EDIT } else { ROW };
        let over = ui.hover(c, x, y, w, ROW);
        c.round(x, y, w, h, 12.0, if over || open { CARD_HOVER } else { CARD });
        c.outline(x, y, w, h, 12.0, 1.0, if open { theme::HAIRLINE_STRONG } else { theme::HAIRLINE });
        let cy = y + ROW / 2.0;
        c.push_alpha(if p.visible { 1.0 } else { 0.45 });
        c.circle(x + 28.0, cy, 15.0, theme::alpha(p.color, 0.95));
        c.circle(x + 28.0, cy, 12.5, 0xFF101114);
        c.icon(waypoints::ICONS[p.icon.min(waypoints::ICONS.len() - 1)], 14.0, x + 28.0, cy, p.color);
        let tx = x + 54.0;
        let name = c.ellipsize(font::SEMIBOLD, 13.0, &p.name, w - 260.0);
        c.text(font::SEMIBOLD, 13.0, &name, tx, y + 10.0, theme::TEXT_STRONG);
        let mut sub = format!("{} {} {}  \u{B7}  {}", p.x, p.y, p.z, waypoints::dimension_name(&p.dim));
        if let Some(me) = me
            && let Some(at) = waypoints::place_in(p, dim, true)
        {
            let d = ((at[0] - me[0]).powi(2) + (at[1] - me[1]).powi(2) + (at[2] - me[2]).powi(2)).sqrt();
            sub.push_str(&format!("  \u{B7}  {}", waypoints::distance(d)));
            if p.dim != dim {
                sub.push_str(&format!(" (as {} {} {})", waypoints::dimension_name(dim), at[0].floor() as i64, at[2].floor() as i64));
            }
        }
        let sub = c.ellipsize(font::MEDIUM, 11.0, &sub, w - 220.0);
        c.text(font::MEDIUM, 11.0, &sub, tx, y + 30.0, theme::TEXT_MUTED);
        c.pop_alpha();

        // actions, right to left
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
                if open {
                    self.editing = None;
                } else {
                    self.editing = Some(p.id);
                    self.name.set(&p.name);
                    self.name.select_all();
                    self.name.focus(ui);
                }
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
        if open {
            self.editor(ui, c, world, p, x + 16.0, y + ROW + 4.0, w - 32.0);
        }
        deleted
    }

    /// Name, colour and icon of the waypoint being edited.
    #[allow(clippy::too_many_arguments)]
    fn editor(&mut self, ui: &mut Ui, c: &mut Canvas, world: &str, p: &Point, x: f32, y: f32, w: f32) {
        c.fill(x, y - 4.0, w, 1.0, theme::HAIRLINE);
        c.text(font::MEDIUM, 11.0, "Name", x, y + 8.0, theme::TEXT_MUTED);
        if self.name.draw(ui, c, x + 60.0, y, 320.0f32.min(w - 60.0), 32.0, "Waypoint name", 12.0) {
            self.editing = None;
            ui.focus = None;
            return;
        }
        let typed = self.name.value().trim().to_owned();
        if !typed.is_empty() && typed != p.name {
            waypoints::store().edit(world, p.id, |q| q.name = typed);
        }
        let sy = y + 46.0;
        c.text(font::MEDIUM, 11.0, "Colour", x, sy + 6.0, theme::TEXT_MUTED);
        let sx = x + 60.0;
        for (i, col) in waypoints::COLORS.iter().copied().enumerate() {
            let (cx, cy) = (sx + i as f32 * 30.0 + 12.0, sy + 12.0);
            let over = ui.hover(c, cx - 12.0, cy - 12.0, 24.0, 24.0);
            if p.color == col {
                c.circle(cx, cy, 12.0, 0xFFFFFFFF);
            }
            c.circle(cx, cy, if p.color != col && over { 11.0 } else { 10.0 }, col);
            if ui.clicked(c, &format!("waypoints:col:{i}"), cx - 12.0, cy - 12.0, 24.0, 24.0) {
                waypoints::store().edit(world, p.id, |q| q.color = col);
            }
        }
        let iy = sy + 40.0;
        c.text(font::MEDIUM, 11.0, "Icon", x, iy + 6.0, theme::TEXT_MUTED);
        for (i, icon) in waypoints::ICONS.iter().copied().enumerate() {
            let (ix, s) = (sx + i as f32 * 32.0, 28.0);
            let sel = p.icon == i;
            let over = ui.hover(c, ix, iy - 2.0, s, s);
            c.round(ix, iy - 2.0, s, s, 8.0, if sel { theme::alpha(p.color, 0.25) } else if over { theme::SUBTLE_HOVER } else { 0 });
            if sel {
                c.outline(ix, iy - 2.0, s, s, 8.0, 1.0, theme::alpha(p.color, 0.8));
            }
            c.icon(icon, 14.0, ix + s / 2.0, iy - 2.0 + s / 2.0, if sel { p.color } else { theme::TEXT_SECONDARY });
            if ui.clicked(c, &format!("waypoints:icon:{i}"), ix, iy - 2.0, s, s) {
                waypoints::store().edit(world, p.id, |q| q.icon = i);
            }
        }
    }

    fn add_here(&mut self, world: &str, dim: &str, me: [f64; 3]) {
        let mut s = waypoints::store();
        let name = s.next_name(world, "Waypoint");
        let color = s.next_color(world);
        let ok = s.add(world, Point::new(&name, me[0].floor() as i32, me[1].floor() as i32, me[2].floor() as i32, dim, color, 0));
        drop(s);
        self.say(if ok { format!("Added {name}") } else { format!("This world has {} waypoints already", waypoints::MAX_PER_WORLD) });
    }

    /// Coordinates from the clipboard ("120 64 -300", "x: 120 z: -300"), in this dimension.
    fn paste(&mut self, world: &str, dim: &str, me: [f64; 3], text: &str) {
        let Some((x, y, z)) = waypoints::parse_coords(text) else {
            self.say("No coordinates on the clipboard (copy something like 120 64 -300)".into());
            return;
        };
        let y = y.unwrap_or(me[1].floor() as i32);
        let mut s = waypoints::store();
        // a leading word ("Base 120 64 -300", what Copy gives) names it
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
