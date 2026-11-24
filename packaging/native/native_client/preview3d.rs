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
    /// Something drawn under the player in the same view (the title screen's island): `(c, cx, cy, scale, yaw,
    /// pitch)`, model y = 16 under the feet.
    pub stage: Option<fn(&mut Canvas, f32, f32, f32, f32, f32)>,
    /// Optional foreground pass drawn after the player (plants/decor closer to the camera than the player).
    pub stage_front: Option<fn(&mut Canvas, f32, f32, f32, f32, f32)>,
    /// A Minecraft nametag over the head (the title screen).
    pub nametag: Option<String>,
    /// Optional zoom multiplier and pause toggle for the Locker stage controls.
    pub zoom: f32,
    pub paused: bool,
    /// Optional temporary skin override (when previewing or switching a saved skin in Locker).
    pub override_skin: Option<(Arc<Image>, bool)>,
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
            stage: None,
            stage_front: None,
            nametag: None,
            zoom: 1.0,
            paused: false,
            override_skin: None,
        }
    }

    /// The camera's tilt: negative looks up at the model (the wardrobe), positive down onto it (the title island).
    pub fn set_pitch(&mut self, pitch: f32) {
        self.pitch = pitch;
    }

    pub fn reset_view(&mut self) {
        self.yaw = -0.45;
        self.vel = 0.0;
        self.zoom = 1.0;
    }

    pub fn face_back(&mut self) {
        self.yaw = std::f32::consts::PI - 0.35;
        self.vel = 0.0;
    }

    pub fn face_front(&mut self) {
        self.yaw = -0.45;
        self.vel = 0.0;
    }

    pub fn zoom_by(&mut self, factor: f32) {
        self.zoom = (self.zoom * factor).clamp(0.65, 1.65);
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
        if !self.paused {
            self.time += ui.dt;
            if ui.now - self.last_drag > 2500.0 {
                self.yaw += (self.time * 0.7).sin() * 0.0025;
            }
        }
        // Floor glow.
        let cx = x + w / 2.0;
        let scale = (h / 50.0).min(w / 30.0) * self.zoom.max(0.5);
        let mut feet = y + h / 2.0 + 19.0 * scale;
        if let Some(stage) = self.stage {
            stage(c, cx, y + h / 2.0 + 3.0 * scale, scale, self.yaw, self.pitch);
            feet = y + h / 2.0 + 3.0 * scale + 16.0 * self.pitch.cos() * scale;
        }
        c.shadow(cx - 9.0 * scale, feet - 1.5 * scale, 18.0 * scale, 3.0 * scale, 1.5 * scale, 10.0, 0x66000000);
        // The skin.
        let (skin, pending, slim) = if let Some((ref img, s)) = self.override_skin {
            (Some(img.clone()), false, Some(s))
        } else {
            skin_of(who)
        };
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
        if let Some(stage_front) = self.stage_front {
            stage_front(c, cx, y + h / 2.0 + 3.0 * scale, scale, self.yaw, self.pitch);
        }
        if let Some(name) = self.nametag.as_deref().filter(|n| !n.is_empty()) {
            // Over the head like in game: model y = -16 is the top of the head, the tag sits a little above it.
            let bottom = y + h / 2.0 + 3.0 * scale + (-16.0 - 7.0) * self.pitch.cos() * scale;
            name_tag(c, name, cx, bottom, (scale * 0.28).max(1.5));
        }
        let _ = theme::TEXT;
    }

    #[allow(clippy::too_many_arguments)]
    fn part(&mut self, s: &Arc<Image>, tw: u32, th: u32, o: [f32; 3], size: [f32; 3], uv: [f32; 2], mirror: bool, tint: u32, fallback: u32) {
        if Arc::ptr_eq(s, &self.white) {
            // The plain body: declare a texture exactly as big as this cube's unwrapped faces, so every face samples
            // inside the 2 x 2 white image (with a 2 x 2 size the faces' regions fell outside it and drew nothing).
            let (fw, fh) = ((2.0 * (size[0] + size[2])).ceil() as u32, (size[1] + size[2]).ceil() as u32);
            self.b.cube(s, fw.max(1), fh.max(1), o, size, [0.0, 0.0], 0.0, false, fallback);
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
        let mut batch = horizon_client::hud::HudBatch::new([600, 600], horizon_client::hud::GuiScale { requested: 0, unicode: false });
        c.finish(&mut batch);
        assert!(!batch.vertices.is_empty());
    }

    /// The Cosmetics tab inside the menu: a clip around the pane, the preview at the pane's size, a guest name.
    #[test]
    fn the_preview_draws_a_body_in_the_locker_pane_for_a_guest() {
        let mut c = horizon_client::native_ui::canvas();
        c.begin(1280, 720, 1.0);
        let mut ui = Ui::default();
        ui.begin(&[], 1.0);
        let l = wardrobe::snapshot();
        let mut p = PlayerPreview::new();
        let (x, y, w, h) = (856.0, 130.0, 254.0, 512.0);
        c.push_clip(x + 1.0, y + 52.0, w - 2.0, h - 52.0 - 92.0);
        let before = { let mut b = horizon_client::hud::HudBatch::new([1280, 720], horizon_client::hud::GuiScale { requested: 0, unicode: false }); c.finish(&mut b); b.vertices.len() };
        c.begin(1280, 720, 1.0);
        c.push_clip(x + 1.0, y + 52.0, w - 2.0, h - 52.0 - 92.0);
        p.draw(&mut ui, &mut c, "menu:preview", x, y + 56.0, w, h - 56.0 - 98.0, &l, "NativeDev", None, None);
        c.pop_clip();
        let mut batch = horizon_client::hud::HudBatch::new([1280, 720], horizon_client::hud::GuiScale { requested: 0, unicode: false });
        c.finish(&mut batch);
        eprintln!("preview vertices: {} (empty canvas: {before})", batch.vertices.len());
        assert!(batch.vertices.len() > before + 100, "the body is drawn");
    }
}

/// The player's skin, reduced to a sensible size for the wheel.
pub fn wheel_skin(name: &str) -> (Arc<Image>, bool) {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static URLS: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    static SMALL: OnceLock<Mutex<HashMap<u64, Arc<Image>>>> = OnceLock::new();
    let (skin, _, directory_slim) = skin_of(name);
    let local = wheel_local_look(name);
    let slim = directory_slim.or_else(|| local.as_ref().map(|(_, slim)| *slim))
        .or_else(|| net::account().and_then(|a| a.model).map(|m| m.eq_ignore_ascii_case("slim"))).unwrap_or(false);
    let skin = skin.or_else(|| {
        let reference = local.as_ref().map(|(hash, _)| hash.clone()).or_else(|| super::avatars::own(name))?;
        let url = if let Some(uuid) = reference.strip_prefix("mj:") {
            let mut map = URLS.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
            if !map.contains_key(uuid) {
                map.insert(uuid.to_string(), None);
                let uuid = uuid.to_string();
                let _ = std::thread::Builder::new().name("Native wheel skin".into()).spawn(move || {
                    let result = super::avatars::mojang_skin_url(&uuid);
                    URLS.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner()).insert(uuid, result);
                });
            }
            map.get(uuid).cloned().flatten()
        } else if reference.len() == 64 && reference.bytes().all(|b| b.is_ascii_hexdigit()) {
            Some(net::hash_url(&wardrobe::base(), &reference))
        } else { None }?;
        match net::image(&url, 2 * 1024 * 1024) { Pic::Ready(img) => Some(img), _ => None }
    });
    let Some(skin) = skin else { return (wheel_steve(), false) };
    if skin.width <= 64 { return (skin, slim); }
    let mut cache = SMALL.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if let Some(small) = cache.get(&skin.id) { return (small.clone(), slim); }
    let height = if skin.height * 2 == skin.width { 32 } else { 64 };
    let mut pixels = vec![0; 64 * height];
    for y in 0..height { for x in 0..64 {
        let sx = x * skin.width as usize / 64;
        let sy = y * skin.height as usize / height;
        pixels[y * 64 + x] = skin.argb[sy * skin.width as usize + sx];
    } }
    let small = Image::new(64, height as u32, pixels);
    if cache.len() > 32 { cache.clear(); }
    cache.insert(skin.id, small.clone());
    (small, slim)
}
fn wheel_local_look(name: &str) -> Option<(String, bool)> {
    static LOCAL: std::sync::OnceLock<Option<(String, String, bool)>> = std::sync::OnceLock::new();
    let look = LOCAL.get_or_init(|| {
        let game = std::env::var_os("NATIVE_GAME_DIR").map(std::path::PathBuf::from).or_else(|| std::env::current_dir().ok())?;
        let root = [game.join(".native/launcher.json"), game.join(".noctra/launcher.json")]
            .into_iter().find_map(|p| std::fs::metadata(&p).ok().filter(|m| m.len() < 16384)
                .and_then(|_| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()))?;
        let local = &root["local"];
        let name = local["name"].as_str()?.to_owned(); let hash = local["skin"].as_str()?.to_owned();
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) { return None; }
        Some((name, hash, local["slim"].as_bool().unwrap_or(false)))
    });
    look.as_ref().filter(|(n,_,_)| n.eq_ignore_ascii_case(name)).map(|(_,h,s)| (h.clone(), *s))
}
fn wheel_steve() -> Arc<Image> {
    use std::sync::OnceLock;
    static STEVE: OnceLock<Arc<Image>> = OnceLock::new();
    STEVE.get_or_init(|| {
        let mut px = vec![0u32; 64 * 64];
        let mut rect = |x: usize, y: usize, w: usize, h: usize, color: u32| {
            for yy in y..y+h { for xx in x..x+w { px[yy*64+xx] = color; } }
        };
        rect(0,0,32,16,0xffa97753); rect(0,0,32,8,0xff4a3322);
        rect(16,16,24,16,0xff2eaaa3); rect(40,16,16,16,0xffa97753);
        rect(0,16,16,16,0xff303b80); rect(16,48,16,16,0xff303b80);
        rect(32,48,16,16,0xffa97753);
        rect(9,11,2,1,0xffffffff); rect(13,11,2,1,0xffffffff);
        rect(10,11,1,1,0xff4b3a8c); rect(13,11,1,1,0xff4b3a8c);
        rect(10,14,4,1,0xff6b4a33);
        Image::new(64,64,px)
    }).clone()
}
/// Takes the clip's sideways and forward/back travel (where the hips go) off every bone, as the launcher's stage does:
/// a preview plays on the spot instead of walking towards and away from the camera. Up and down (jumps) stays.
pub fn on_the_spot(mut p: [[f32; 6]; 6]) -> [[f32; 6]; 6] {
    let (dx, dz) = ((p[4][3] + p[5][3]) * 0.5, (p[4][5] + p[5][5]) * 0.5);
    for bone in &mut p {
        bone[3] -= dx;
        bone[5] -= dz;
    }
    p
}

/// The view-space box of a posed player as `draw_wheel` projects it (same cubes, yaw and pitch).
fn pose_bounds(p: &[[f32; 6]; 6], slim: bool) -> [f32; 4] {
    let pivots = [[0.,0.,0.],[0.,0.,0.],[-5.,if slim {2.5} else {2.},0.],[5.,if slim {2.5} else {2.},0.],[-1.9,12.,0.],[1.9,12.,0.]];
    let aw: f32 = if slim {3.} else {4.};
    let cubes: [([f32; 3], [f32; 3]); 6] = [([-4.,-8.,-4.],[8.,8.,8.]), ([-4.,0.,-2.],[8.,12.,4.]), ([-aw+1.,-2.,-2.],[aw,12.,4.]),
        ([-1.,-2.,-2.],[aw,12.,4.]), ([-2.,0.,-2.],[4.,12.,4.]), ([-2.,0.,-2.],[4.,12.,4.])];
    let (sy,cy) = (-0.38f32).sin_cos(); let (sp,cp) = (-0.10f32).sin_cos();
    let mut bounds = [f32::INFINITY,f32::INFINITY,f32::NEG_INFINITY,f32::NEG_INFINITY];
    for i in 0..6 {
        let (sx,cx) = p[i][0].sin_cos(); let (syaw,cyaw) = p[i][1].sin_cos(); let (sz,cz) = p[i][2].sin_cos();
        for mask in 0..8 {
            let mut v = [0.;3];
            for j in 0..3 { v[j] = cubes[i].0[j] + if mask & (1<<j) == 0 {-0.5} else {cubes[i].1[j]+0.5}; }
            let yy = cx*v[1]-sx*v[2]; let zz = sx*v[1]+cx*v[2]; let xx = cyaw*v[0]+syaw*zz;
            let x = cz*xx-sz*yy+pivots[i][0]+p[i][3];
            let y = sz*xx+cz*yy+pivots[i][1]+p[i][4]-8.;
            let z = -syaw*v[0]+cyaw*zz+pivots[i][2]+p[i][5];
            let vx = cy*x+sy*z; let vy = cp*y-sp*(-sy*x+cy*z);
            bounds[0]=bounds[0].min(vx); bounds[1]=bounds[1].min(vy);
            bounds[2]=bounds[2].max(vx); bounds[3]=bounds[3].max(vy);
        }
    }
    bounds
}

thread_local! {
    /// The emote the next `draw_wheel` shows (its hash and arm width), set by `fit_emote`.
    static FIT_KEY: std::cell::RefCell<Option<(String, bool)>> = const { std::cell::RefCell::new(None) };
    /// Each emote's framing: the box of its whole clip, so the model keeps one size and place while it plays.
    static FITS: std::cell::RefCell<std::collections::HashMap<(String, bool), [f32; 4]>> = std::cell::RefCell::new(std::collections::HashMap::new());
}

/// The next `draw_wheel` frames emote `hash` by its whole clip instead of the current pose alone. Without it the
/// model was fitted anew every frame and grew and shrank (came forward and back) as the emote played.
pub fn fit_emote(hash: &str, slim: bool) {
    FIT_KEY.with(|k| *k.borrow_mut() = Some((hash.to_owned(), slim)));
}

fn fit(bounds: &mut [f32; 4]) {
    let Some(key) = FIT_KEY.with(|k| k.borrow_mut().take()) else { return };
    FITS.with(|fits| {
        let mut fits = fits.borrow_mut();
        if fits.len() > 512 {
            fits.clear();
        }
        let whole = fits.entry(key.clone()).or_insert_with(|| {
            let mut b = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
            if let Some(duration) = horizon_client::native_emotes::clip_duration(&key.0) {
                for i in 0..=48 {
                    if let Some(p) = horizon_client::native_emotes::preview_pose(&key.0, Some(duration * i as f64 / 48.0 * 0.999)) {
                        let q = pose_bounds(&on_the_spot(p), key.1);
                        b = [b[0].min(q[0]), b[1].min(q[1]), b[2].max(q[2]), b[3].max(q[3])];
                    }
                }
            }
            b
        });
        // props reach further than the body on some frames: the framing only ever grows, so it settles in one loop
        *whole = [whole[0].min(bounds[0]), whole[1].min(bounds[1]), whole[2].max(bounds[2]), whole[3].max(bounds[3])];
        *bounds = *whole;
    });
}

/// Fits the transformed skin cubes to a wheel sector rectangle, including outer layers.
pub fn draw_wheel(c: &mut Canvas, skin: &Arc<Image>, slim: bool, p: [[f32; 6]; 6], rect: [f32; 4]) {
    let p = on_the_spot(p);
    let pivots = [[0.,0.,0.],[0.,0.,0.],[-5.,if slim {2.5} else {2.},0.],[5.,if slim {2.5} else {2.},0.],[-1.9,12.,0.],[1.9,12.,0.]];
    let aw = if slim {3.} else {4.};
    let boxes = [([-4.,-8.,-4.],[8.,8.,8.],[0.,0.],[32.,0.]),
        ([-4.,0.,-2.],[8.,12.,4.],[16.,16.],[16.,32.]),
        ([-aw+1.,-2.,-2.],[aw,12.,4.],[40.,16.],[40.,32.]),
        ([-1.,-2.,-2.],[aw,12.,4.],[32.,48.],[48.,48.]),
        ([-2.,0.,-2.],[4.,12.,4.],[0.,16.],[0.,32.]),
        ([-2.,0.,-2.],[4.,12.,4.],[16.,48.],[0.,48.])];
    let (sy,cy) = (-0.38f32).sin_cos(); let (sp,cp) = (-0.10f32).sin_cos();
    let mut bounds = [f32::INFINITY,f32::INFINITY,f32::NEG_INFINITY,f32::NEG_INFINITY];
    for i in 0..6 {
        let (sx,cx) = p[i][0].sin_cos(); let (syaw,cyaw) = p[i][1].sin_cos(); let (sz,cz) = p[i][2].sin_cos();
        for mask in 0..8 {
            let mut v = [0.;3];
            for j in 0..3 { v[j] = boxes[i].0[j] + if mask & (1<<j) == 0 {-0.5} else {boxes[i].1[j]+0.5}; }
            let yy = cx*v[1]-sx*v[2]; let zz = sx*v[1]+cx*v[2]; let xx = cyaw*v[0]+syaw*zz;
            let x = cz*xx-sz*yy+pivots[i][0]+p[i][3];
            let y = sz*xx+cz*yy+pivots[i][1]+p[i][4]-8.;
            let z = -syaw*v[0]+cyaw*zz+pivots[i][2]+p[i][5];
            let vx = cy*x+sy*z; let vy = cp*y-sp*(-sy*x+cy*z);
            bounds[0]=bounds[0].min(vx); bounds[1]=bounds[1].min(vy);
            bounds[2]=bounds[2].max(vx); bounds[3]=bounds[3].max(vy);
        }
    }
    let [rx,ry,rw,rh] = rect;
    let unit = { fit(&mut bounds); (rw / (bounds[2]-bounds[0]).max(1.)).min(rh / (bounds[3]-bounds[1]).max(1.)) };
    let mut b = Boxes::new();
    b.begin(rx+rw/2.-(bounds[0]+bounds[2])*unit/2., ry+rh/2.-(bounds[1]+bounds[3])*unit/2., unit, -0.38, -0.10);
    b.translate(0.,-8.,0.);
    let legacy = skin.height*2 == skin.width;
    for i in 0..6 {
        b.push(); b.translate(pivots[i][0]+p[i][3],pivots[i][1]+p[i][4],pivots[i][2]+p[i][5]); b.rotate(p[i][0],p[i][1],p[i][2]);
        let uv = if legacy && i == 3 { [40.,16.] } else if legacy && i == 5 { [0.,16.] } else { boxes[i].2 };
        b.cube(skin,64,if legacy {32} else {64},boxes[i].0,boxes[i].1,uv,0.,legacy && (i==3||i==5),0xffffffff);
        if i == 0 || !legacy { b.cube(skin,64,if legacy {32} else {64},boxes[i].0,boxes[i].1,boxes[i].3,if i==0 {0.5} else {0.25},false,0xffffffff); }
        b.pop();
    }
    b.end(c);
}

/// A nametag as the game draws one over a player: the name in the Minecraft font on a dark see-through box, with
/// the Native mark in front. Centred at `cx`, its bottom edge at `bottom`; `unit` is one font pixel.
fn name_tag(c: &mut Canvas, name: &str, cx: f32, bottom: f32, unit: f32) {
    static MARK: std::sync::OnceLock<Option<Arc<Image>>> = std::sync::OnceLock::new();
    let mark = MARK.get_or_init(|| horizon_client::native_ui::Image::decode(horizon_client::native_ui::assets::LOGO)).clone();
    let text_w = super::mc_font::width(c, name, unit, false) - unit;
    let mark_w = if mark.is_some() { 9.0 * unit } else { 0.0 };
    let pad = 2.0 * unit;
    let (w, bh) = (text_w + mark_w + pad * 2.0, 8.0 * unit + pad * 2.0);
    let (x, y) = (cx - w / 2.0, bottom - bh);
    c.fill(x, y, w, bh, 0x40000000);
    let mut tx = x + pad;
    if let Some(m) = mark.as_ref() {
        c.stamp(0x6E61_7469_7665_6D6B, m, tx, y + pad, 8.0 * unit, 8.0 * unit, true, 0xFFFFFFFF);
        tx += mark_w;
    }
    super::mc_font::draw(c, name, tx, y + pad, unit, 0xFFFFFFFF, false, false);
}

/// Renders an upper-body 3D portrait of a Minecraft skin inside `[x, y, w, h]` (for Locker saved skin cards).
pub fn draw_skin_portrait(c: &mut Canvas, skin: &Arc<Image>, slim: bool, x: f32, y: f32, w: f32, h: f32) {
    let legacy = skin.height * 2 == skin.width;
    let th = if legacy { 32 } else { 64 };
    let scale = (w / 18.0).min(h / 21.0);
    let cx = x + w / 2.0;
    let cy = y + h * 0.56;
    let mut b = Boxes::new();
    b.begin(cx, cy, scale, -0.38, -0.08);
    // Head
    b.push();
    b.cube(skin, 64, th, [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], [0.0, 0.0], 0.0, false, 0xFFFFFFFF);
    b.cube(skin, 64, th, [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], [32.0, 0.0], 0.5, false, 0xFFFFFFFF);
    b.pop();
    // Body
    b.push();
    b.cube(skin, 64, th, [-4.0, 0.0, -2.0], [8.0, 12.0, 4.0], [16.0, 16.0], 0.0, false, 0xFFFFFFFF);
    if !legacy {
        b.cube(skin, 64, th, [-4.0, 0.0, -2.0], [8.0, 12.0, 4.0], [16.0, 32.0], 0.25, false, 0xFFFFFFFF);
    }
    b.pop();
    let aw = if slim { 3.0 } else { 4.0 };
    let ay = if slim { 2.5 } else { 2.0 };
    // Right arm
    b.push();
    b.translate(-5.0, ay, 0.0);
    b.rotate(0.0, 0.0, 0.06);
    b.cube(skin, 64, th, [-aw + 1.0, -2.0, -2.0], [aw, 12.0, 4.0], [40.0, 16.0], 0.0, false, 0xFFFFFFFF);
    if !legacy {
        b.cube(skin, 64, th, [-aw + 1.0, -2.0, -2.0], [aw, 12.0, 4.0], [40.0, 32.0], 0.25, false, 0xFFFFFFFF);
    }
    b.pop();
    // Left arm
    b.push();
    b.translate(5.0, ay, 0.0);
    b.rotate(0.0, 0.0, -0.06);
    if legacy {
        b.cube(skin, 64, th, [-1.0, -2.0, -2.0], [aw, 12.0, 4.0], [40.0, 16.0], 0.0, true, 0xFFFFFFFF);
    } else {
        b.cube(skin, 64, th, [-1.0, -2.0, -2.0], [aw, 12.0, 4.0], [32.0, 48.0], 0.0, false, 0xFFFFFFFF);
        b.cube(skin, 64, th, [-1.0, -2.0, -2.0], [aw, 12.0, 4.0], [48.0, 48.0], 0.25, false, 0xFFFFFFFF);
    }
    b.pop();
    // Legs
    b.push();
    b.translate(-1.9, 12.0, 0.0);
    b.cube(skin, 64, th, [-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [0.0, 16.0], 0.0, false, 0xFFFFFFFF);
    if !legacy {
        b.cube(skin, 64, th, [-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [0.0, 32.0], 0.25, false, 0xFFFFFFFF);
    }
    b.pop();
    b.push();
    b.translate(1.9, 12.0, 0.0);
    if legacy {
        b.cube(skin, 64, th, [-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [0.0, 16.0], 0.0, true, 0xFFFFFFFF);
    } else {
        b.cube(skin, 64, th, [-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [16.0, 48.0], 0.0, false, 0xFFFFFFFF);
        b.cube(skin, 64, th, [-2.0, 0.0, -2.0], [4.0, 12.0, 4.0], [0.0, 48.0], 0.25, false, 0xFFFFFFFF);
    }
    b.pop();
    b.end(c);
}
