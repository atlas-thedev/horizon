//! `PlayerPreview`: the player's own model in 3D (skin, cape, worn cosmetics)
//! for the wardrobe. Drag to turn it.
#![allow(dead_code)]

use super::boxes::Boxes;
use super::net::{self, Pic};
use super::ui::Ui;
use super::wardrobe::{self, Item, Locker};
use horizon_client::native_ui::{Canvas, Image, theme};
use horizon_cosmetics::Bone;
use std::sync::Arc;

pub struct PlayerPreview {
    b: Boxes,
    yaw: f32,
    pitch: f32,
    vel: f32,
    last_mx: f32,
    last_drag: f64,
    slim: bool,
    arm_swing: f32,
    time: f32,
    white: Arc<Image>,
}

impl Default for PlayerPreview {
    fn default() -> Self {
        Self::new()
    }
}

/// The skin the Native directory has for `name`: its image once loaded, whether it is still coming, and the arm width.
fn skin_of(name: &str) -> (Option<Arc<Image>>, bool, Option<bool>) {
    let (hash, base, slim) = {
        let dir = horizon_client::native_skins::directory();
        match dir.entry(name) {
            Some(e) => (e.skin.clone(), dir.texture_base().to_owned(), Some(e.slim)),
            None => (None, String::new(), None),
        }
    };
    let Some(hash) = hash else { return (None, false, slim) };
    let base = if base.is_empty() { wardrobe::base() } else { base };
    match net::image(&net::hash_url(&base, &hash), 2 * 1024 * 1024) {
        Pic::Ready(img) => (Some(img), false, slim),
        Pic::Loading => (None, true, slim),
        Pic::Failed => (None, false, slim),
    }
}

fn image_of(url: Option<&str>) -> Option<Arc<Image>> {
    match net::image(url?, 4 * 1024 * 1024) {
        Pic::Ready(img) => Some(img),
        _ => None,
    }
}

impl PlayerPreview {
    pub fn new() -> Self {
        PlayerPreview {
            b: Boxes::new(),
            yaw: -0.45,
            pitch: -0.12,
            vel: 0.0,
            last_mx: 0.0,
            last_drag: 0.0,
            slim: false,
            arm_swing: 0.0,
            time: 0.0,
            white: Image::new(2, 2, vec![0xFFFFFFFF; 4]),
        }
    }

    pub fn reset_view(&mut self) {
        self.yaw = -0.45;
        self.vel = 0.0;
    }

    /// push + move into a vanilla bone's space (pivot and idle pose).
    fn bone(&mut self, bone: Bone) {
        self.b.push();
        self.bone_transform(bone);
    }

    fn bone_transform(&mut self, bone: Bone) {
        let slim_up = if self.slim { 2.5 } else { 2.0 };
        match bone {
            Bone::RightArm => {
                self.b.translate(-5.0, slim_up, 0.0);
                self.b.rotate(self.arm_swing, 0.0, 0.06 + self.arm_swing);
            }
            Bone::LeftArm => {
                self.b.translate(5.0, slim_up, 0.0);
                self.b.rotate(-self.arm_swing, 0.0, -0.06 - self.arm_swing);
            }
            Bone::RightLeg => self.b.translate(-1.9, 12.0, 0.0),
            Bone::LeftLeg => self.b.translate(1.9, 12.0, 0.0),
            _ => {}
        }
    }

    /// Draws the model into the box. `hover` is the item being tried on; `bare` a slot group shown empty
    /// ("Nothing"; "capes" hides the cape).
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&mut self, ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, w: f32, h: f32, l: &Locker, who: &str, hover: Option<&Item>, bare: Option<&str>) {
        let over = ui.hover(c, x, y, w, h);
        ui.clicked(c, id, x, y, w, h);
        if ui.is_pressing(id) {
            if ui.pressed {
                self.last_mx = ui.mx;
            }
            let dx = ui.mx - self.last_mx;
            self.last_mx = ui.mx;
            // Grab-and-turn: dragging right turns the model's front to the right.
            self.yaw -= dx * 0.012;
            self.vel = -dx * 0.012 / ui.dt.max(0.004);
            self.last_drag = ui.now;
        } else {
            self.yaw += self.vel * ui.dt;
            self.vel *= (-5.0 * ui.dt).exp();
        }
        if over {
            ui.cursor_hand = true;
        }
        self.time += ui.dt;
        if ui.now - self.last_drag > 2500.0 {
            self.yaw += (self.time * 0.7).sin() * 0.0025;
        }
        // Floor glow.
        let cx = x + w / 2.0;
        let scale = (h / 50.0).min(w / 30.0);
        let feet = y + h / 2.0 + 19.0 * scale;
        c.shadow(cx - 9.0 * scale, feet - 1.5 * scale, 18.0 * scale, 3.0 * scale, 1.5 * scale, 10.0, 0x66000000);
        // The skin.
        let (skin, pending, slim) = skin_of(who);
        if let Some(slim) = slim {
            self.slim = slim;
        }
        if pending {
            // The skin is still on its way: a skeleton of the model instead of a grey body that turns into the skin.
            for (bx, by, bw, bh) in [(-4.0, -32.0, 8.0, 8.0), (-4.0, -24.0, 8.0, 12.0), (-8.0, -24.0, 4.0, 12.0), (4.0, -24.0, 4.0, 12.0), (-4.0, -12.0, 4.0, 12.0), (0.0, -12.0, 4.0, 12.0)] {
                c.skeleton(cx + bx * scale, feet + by * scale, bw * scale, bh * scale, 2.0 * scale);
            }
            return;
        }
        // The cape: the store cape worn (or the one being hovered).
        let mut cape: Option<Arc<Image>> = None;
        let (mut cape_frames, mut cape_frame) = (1u32, 0u32);
        let cape_item = match hover {
            Some(i) if i.is_cape() => Some(i.clone()),
            _ => wardrobe::item(l, l.equipped.as_deref()),
        };
        if bare != Some("capes")
            && let Some(item) = &cape_item
        {
            cape = image_of(item.still_url.as_deref());
            if item.animated()
                && let Some(strip) = image_of(item.strip_url.as_deref())
            {
                // The animated strip: the cube below picks the current frame.
                cape = Some(strip);
                cape_frames = item.frames;
                cape_frame = item.frame(ui.now / 1000.0) as u32;
            }
        }
        // A back item (wings, blades, backpacks) takes the cape's place, like in game.
        let hover_back = hover.is_some_and(|h| !h.is_cape() && h.group() == "back");
        let back_worn = bare != Some("back") && (l.wearing.contains_key("back") || hover_back);
        if back_worn && !hover.is_some_and(Item::is_cape) {
            cape = None;
        }
        // The cosmetics.
        let mut worn: Vec<Item> = Vec::new();
        for (slot, id) in &l.wearing {
            if Some(slot.as_str()) == bare {
                continue;
            }
            let item = match hover {
                Some(h) if !h.is_cape() && h.group() == slot => Some(h.clone()),
                _ => wardrobe::item(l, Some(id)),
            };
            worn.extend(item);
        }
        if let Some(h) = hover
            && !h.is_cape()
            && !l.wearing.contains_key(h.group())
        {
            worn.push(h.clone());
        }
        let models: Vec<(Item, Arc<super::super::native_cosmetics::Model>, Arc<Image>)> = worn
            .into_iter()
            .filter_map(|item| {
                let model = super::super::native_cosmetics::preview_model(&wardrobe::base(), &item.model_hash()?)?;
                let tex = image_of(wardrobe::texture_url(l, &item).as_deref())?;
                Some((item, model, tex))
            })
            .collect();

        self.b.begin(cx, y + h / 2.0 + 3.0 * scale, scale, self.yaw, self.pitch);
        self.b.translate(0.0, -8.0, 0.0);
        self.arm_swing = (self.time * 1.6).sin() * 0.04;
        let legacy = skin.as_ref().is_some_and(|s| s.height * 2 == s.width);
        let tex_h = if legacy { 32 } else { 64 };
        let (tint, tint_alt) = if skin.is_some() { (0xFFFFFFFF, 0xFFFFFFFF) } else { (0xFF8A8F98, 0xFFA3A8B1) };
        let s: Arc<Image> = skin.clone().unwrap_or_else(|| self.white.clone());
        let (tw, th) = if skin.is_some() { (64, tex_h) } else { (2, 2) };
        let has_skin = skin.is_some();
        let hides_hat = models.iter().any(|(_, m, _)| m.hides_hat());
        // Head.
        self.bone(Bone::Head);
        self.part(&s, tw, th, [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], [0.0, 0.0], false, tint, tint_alt);
        if has_skin && !hides_hat {
            // A hood replaces the hat layer, like in game.
            self.b.cube(&s, tw, th, [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], [32.0, 0.0], 0.5, false, tint);
        }
        self.b.pop();
        // Body.
        self.bone(Bone::Body);
        self.part(&s, tw, th, [-4.0, 0.0, -2.0], [8.0, 12.0, 4.0], [16.0, 16.0], false, tint, tint);
        if has_skin && !legacy {
            self.b.cube(&s, tw, th, [-4.0, 0.0, -2.0], [8.0, 12.0, 4.0], [16.0, 32.0], 0.25, false, tint);
        }
        if let Some(cape) = &cape {
            self.b.push();
            self.b.translate(0.0, 0.0, 2.0);
            self.b.rotate(0.1 + (self.time * 1.2).sin() * 0.03, 0.0, 0.0);
            self.b.rotate(0.0, std::f32::consts::PI, 0.0);
            self.b.cube(cape, 64, 32 * cape_frames, [-5.0, 0.0, -1.0], [10.0, 16.0, 1.0], [0.0, 32.0 * cape_frame as f32], 0.0, false, 0xFFFFFFFF);
            self.b.pop();
        }
        self.b.pop();
        let aw = if self.slim { 3.0 } else { 4.0 };
        // Right arm.
        self.bone(Bone::RightArm);
        self.part(&s, tw, th, [-aw + 1.0, -2.0, -2.0], [aw, 12.0, 4.0], [40.0, 16.0], false, tint, tint);
        if has_skin && !legacy {
            self.b.cube(&s, tw, th, [-aw + 1.0, -2.0, -2.0], [aw, 12.0, 4.0], [40.0, 32.0], 0.25, false, tint);
        }
        self.b.pop();
        // Left arm.
        self.bone(Bone::LeftArm);
        if legacy {
            self.part(&s, tw, th, [-1.0, -2.0, -2.0], [aw, 12.0, 4.0], [40.0, 16.0], true, tint, tint);
        } else {
            self.part(&s, tw, th, [-1.0, -2.0, -2.0], [aw, 12.0, 4.0], [32.0, 48.0], false, tint, tint);
            if has_skin {
                self.b.cube(&s, tw, th, [-1.0, -2.0, -2.0], [aw, 12.0, 4.0], [48.0, 48.0], 0.25, false, tint);
            }
        }
        self.b.pop();
        // Legs.
        self.bone(Bone::RightLeg);
        self.part(&s, tw, th, [-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [0.0, 16.0], false, tint, tint);
        if has_skin && !legacy {
            self.b.cube(&s, tw, th, [-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [0.0, 32.0], 0.25, false, tint);
        }
        self.b.pop();
        self.bone(Bone::LeftLeg);
        if legacy {
            self.part(&s, tw, th, [-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [0.0, 16.0], true, tint, tint);
        } else {
            self.part(&s, tw, th, [-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [16.0, 48.0], false, tint, tint);
            if has_skin {
                self.b.cube(&s, tw, th, [-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [0.0, 48.0], 0.25, false, tint);
            }
        }
        self.b.pop();
        // The cosmetics, posed on the bones.
        let time = f64::from(self.time);
        for (item, model, tex) in &models {
            let side = l.sides.get(item.group()).copied().unwrap_or(0).clamp(0, 2) as u8;
            let [mw, mh] = model.texture_size();
            let (mw, mh) = (mw as u32, mh as u32);
            let b = &mut self.b;
            let slim_up = if self.slim { 2.5 } else { 2.0 };
            let arm_swing = self.arm_swing;
            model.walk(side, time, &mut |step| match step {
                super::super::native_cosmetics::Walk::Push => b.push(),
                super::super::native_cosmetics::Walk::Pop => b.pop(),
                super::super::native_cosmetics::Walk::Root(bone) => match bone {
                    Bone::RightArm => {
                        b.translate(-5.0, slim_up, 0.0);
                        b.rotate(arm_swing, 0.0, 0.06 + arm_swing);
                    }
                    Bone::LeftArm => {
                        b.translate(5.0, slim_up, 0.0);
                        b.rotate(-arm_swing, 0.0, -0.06 - arm_swing);
                    }
                    Bone::RightLeg => b.translate(-1.9, 12.0, 0.0),
                    Bone::LeftLeg => b.translate(1.9, 12.0, 0.0),
                    _ => {}
                },
                super::super::native_cosmetics::Walk::Transform { pos, rot } => {
                    b.translate(pos[0], pos[1], pos[2]);
                    b.rotate(rot[0], rot[1], rot[2]);
                }
                super::super::native_cosmetics::Walk::Cubes(cubes) => {
                    // Like vanilla's no-cull cosmetic layers.
                    b.two_sided = true;
                    for cube in cubes {
                        b.cube(tex, mw, mh, cube.origin, cube.size, cube.uv, cube.inflate, cube.mirror, 0xFFFFFFFF);
                    }
                    b.two_sided = false;
                }
            });
        }
        self.b.end(c);
        let _ = theme::TEXT;
    }

    #[allow(clippy::too_many_arguments)]
    fn part(&mut self, s: &Arc<Image>, tw: u32, th: u32, o: [f32; 3], size: [f32; 3], uv: [f32; 2], mirror: bool, tint: u32, fallback: u32) {
        if Arc::ptr_eq(s, &self.white) {
            self.b.cube(s, 2, 2, o, size, [0.0, 0.0], 0.0, false, fallback);
        } else {
            self.b.cube(s, tw, th, o, size, uv, 0.0, mirror, tint);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_preview_draws_a_plain_body_while_nothing_is_known() {
        let mut c = horizon_client::native_ui::canvas();
        c.begin(600, 600, 1.0);
        let mut ui = Ui::default();
        ui.begin(&[], 1.0);
        let l = wardrobe::snapshot();
        let mut p = PlayerPreview::new();
        p.draw(&mut ui, &mut c, "t", 0.0, 0.0, 300.0, 400.0, &l, "Nobody", None, None);
        let mut batch = horizon_client::hud::HudBatch::new([600, 600]);
        c.finish(&mut batch);
        assert!(!batch.vertices.is_empty());
    }
}
