//! Native Client 3D cosmetics (hats, glasses, back items, shoes, hand items):
//! the port of native-mod's `CosmeticModel` (the NCM format) and
//! `CosmeticLibrary`. Each worn model is turned into the game's own cosmetic
//! [`Item`], so it is posed on the wearer's bones, animated and drawn by
//! `cosmetics_geometry` exactly like a Horizon item.
//!
//! An NCM model is Minecraft-style boxes with box UV on parts of the player
//! model, in model pixels (y down) relative to the vanilla part it rides, as
//! Horizon's items are. What the game's item cannot say is kept apart here:
//! parts that hide while an armour slot is worn become an item of their own,
//! and a part made for one hand only shows when that hand is the one worn.
//!
//! `tools/native/native_cosmetics.py` copies this file to
//! `crates/horizon-client/src/game/native_cosmetics.rs`.

use super::Game;
use horizon_client::mobs::Mob;
use horizon_cosmetics::{Animation, Axis, Bone, Face, Item, Motion, Part, Slot, Texture};
use std::collections::HashMap;
use std::io::Read;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// `CosmeticLibrary.MAX_MODEL_BYTES` and `MAX_TEXTURE_BYTES`.
const MAX_MODEL_BYTES: u64 = 1024 * 1024;
const MAX_TEXTURE_BYTES: u64 = 2 * 1024 * 1024;
/// `CosmeticModel`'s limits.
const MAX_PARTS: usize = 512;
const MAX_CUBES: usize = 2048;
const MAX_DEPTH: usize = 8;
const MAX_ANIMS: usize = 8;
/// A model or texture that failed is asked for again after this long.
const RETRY: Duration = Duration::from_secs(120);
/// The game's item textures fit one 64×64 skin slot; a larger NCM texture is
/// scaled down (the faces' UVs are fractions of the texture, so they still fit).
const SLOT: u32 = 64;

/// The armour slot a part reacts to: hidden while that slot is filled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Armor {
    None,
    Head,
    Chest,
    Legs,
    Feet,
}

impl Armor {
    fn worn(self, m: &Mob) -> bool {
        match self {
            Armor::None => false,
            Armor::Head => m.extra.head.is_some(),
            Armor::Chest => m.extra.chest.is_some(),
            Armor::Legs => m.extra.legs.is_some(),
            Armor::Feet => m.extra.feet.is_some(),
        }
    }
}

/// A Native cosmetic ready to draw: one game item per armour rule.
#[derive(Debug)]
pub struct Native {
    pub items: Vec<(Armor, Arc<Item>)>,
}

#[derive(Debug)]
struct Cube {
    origin: [f32; 3],
    size: [f32; 3],
    uv: [f32; 2],
    inflate: f32,
    mirror: bool,
}

#[derive(Debug)]
struct Anim {
    motion: Option<Motion>,
    axis: Axis,
    speed: f32,
    amplitude: f32,
    phase: f32,
}

#[derive(Debug)]
struct NcmPart {
    index: usize,
    bone: Bone,
    pivot: [f32; 3],
    /// Degrees.
    rotation: [f32; 3],
    cubes: Vec<Cube>,
    anims: Vec<Anim>,
    children: Vec<NcmPart>,
    armor: Armor,
    /// 0 any, 1 left only, 2 right only.
    side: u8,
}

/// A parsed NCM model.
#[derive(Debug)]
pub struct Model {
    texture: [f32; 2],
    roots: Vec<NcmPart>,
}

fn text<'a>(o: &'a serde_json::Value, key: &str, fallback: &'a str) -> &'a str {
    o.get(key).and_then(serde_json::Value::as_str).unwrap_or(fallback)
}

fn num(o: &serde_json::Value, key: &str, fallback: f32, limit: f32) -> f32 {
    match o.get(key).and_then(serde_json::Value::as_f64) {
        Some(v) if v.is_finite() => (v as f32).clamp(-limit, limit),
        _ => fallback,
    }
}

fn floats<const N: usize>(o: &serde_json::Value, key: &str, fallback: [f32; N]) -> [f32; N] {
    let mut out = fallback;
    if let Some(list) = o.get(key).and_then(serde_json::Value::as_array) {
        for (i, v) in list.iter().take(N).enumerate() {
            if let Some(v) = v.as_f64().filter(|v| v.is_finite()) {
                out[i] = v as f32;
            }
        }
    }
    out
}

fn clamp_all<const N: usize>(values: [f32; N], limit: f32) -> [f32; N] {
    values.map(|v| v.clamp(-limit, limit))
}

fn bone(name: &str) -> Bone {
    match name.to_ascii_lowercase().replace('_', "").as_str() {
        "body" | "back" | "torso" => Bone::Body,
        "rightarm" => Bone::RightArm,
        "leftarm" => Bone::LeftArm,
        "rightleg" | "rightfoot" => Bone::RightLeg,
        "leftleg" | "leftfoot" => Bone::LeftLeg,
        _ => Bone::Head,
    }
}

/// `CosmeticModel.slot` for the armour rule. A `push` rule (the part moves
/// instead of hiding) has no counterpart in the game's items: such parts stay.
fn armor(o: &serde_json::Value) -> Armor {
    let Some(a) = o.get("armor").filter(|a| a.is_object()) else {
        return Armor::None;
    };
    if text(a, "mode", "hide") == "push" {
        return Armor::None;
    }
    match text(a, "slot", "none").to_ascii_lowercase().replace(['_', '-'], "").as_str() {
        "head" | "helmet" => Armor::Head,
        "chest" => Armor::Chest,
        "legs" => Armor::Legs,
        "feet" | "boots" => Armor::Feet,
        _ => Armor::None,
    }
}

fn anim(o: &serde_json::Value) -> Option<Anim> {
    let kind = text(o, "type", "").to_ascii_lowercase();
    let motion = match kind.as_str() {
        "spin" => Some(Motion::Spin),
        "swing" | "flap" => Some(Motion::Swing),
        "bob" => Some(Motion::Bob),
        // Blinking (showing a part part of the time) has no counterpart: the part stays shown.
        "blink" => None,
        _ => return None,
    };
    let axis = match text(o, "axis", "y") {
        "x" => Axis::X,
        "z" => Axis::Z,
        _ => Axis::Y,
    };
    Some(Anim {
        motion,
        axis,
        speed: num(o, "speed", if motion == Some(Motion::Spin) { 90.0 } else { 1.0 }, 4000.0),
        amplitude: num(o, "amplitude", 10.0, 360.0),
        phase: num(o, "phase", 0.0, 1000.0),
    })
}

fn parts(
    list: &[serde_json::Value],
    inherited: Option<Bone>,
    depth: usize,
    count: &mut (usize, usize),
) -> Result<Vec<NcmPart>, String> {
    if depth > MAX_DEPTH {
        return Err("model nests too deep".into());
    }
    let mut out = Vec::new();
    for o in list.iter().filter(|o| o.is_object()) {
        if count.0 >= MAX_PARTS {
            return Err("too many parts".into());
        }
        let index = count.0;
        count.0 += 1;
        let attach = inherited.unwrap_or_else(|| bone(text(o, "attach", "head")));
        let mut cubes = Vec::new();
        for c in o.get("cubes").and_then(serde_json::Value::as_array).into_iter().flatten() {
            if !c.is_object() {
                continue;
            }
            count.1 += 1;
            if count.1 > MAX_CUBES {
                return Err("too many cubes".into());
            }
            let uv = floats(c, "uv", [0.0, 0.0]);
            cubes.push(Cube {
                origin: clamp_all(floats(c, "origin", [0.0; 3]), 64.0),
                size: floats(c, "size", [1.0; 3]).map(|s| s.clamp(0.0, 64.0)),
                uv: uv.map(|v| v.clamp(0.0, 2048.0).trunc()),
                inflate: num(c, "inflate", 0.0, 4.0).max(-2.0),
                mirror: c.get("mirror").and_then(serde_json::Value::as_bool).unwrap_or(false),
            });
        }
        let anims = o
            .get("anim")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(anim)
            .take(MAX_ANIMS)
            .collect();
        let children = match o.get("children").and_then(serde_json::Value::as_array) {
            Some(list) => parts(list, Some(attach), depth + 1, count)?,
            None => Vec::new(),
        };
        out.push(NcmPart {
            index,
            bone: attach,
            pivot: clamp_all(floats(o, "pivot", [0.0; 3]), 64.0),
            rotation: clamp_all(floats(o, "rotation", [0.0; 3]), 360.0),
            cubes,
            anims,
            children,
            armor: armor(o),
            side: match text(o, "side", "") {
                "left" => 1,
                "right" => 2,
                _ => 0,
            },
        });
    }
    Ok(out)
}

impl Model {
    /// `CosmeticModel.parse`: checks the format and its limits.
    pub fn parse(bytes: &[u8]) -> Result<Model, String> {
        let root: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        if !root.is_object() {
            return Err("model is not an object".into());
        }
        let format = root.get("format").and_then(serde_json::Value::as_i64).unwrap_or(1);
        if format != 1 {
            return Err(format!("unsupported model format {format}"));
        }
        let texture = floats(&root, "texture", [64.0, 64.0]).map(f32::trunc);
        if texture.iter().any(|t| !(1.0..=2048.0).contains(t)) {
            return Err("bad texture size".into());
        }
        let list = root
            .get("parts")
            .and_then(serde_json::Value::as_array)
            .ok_or("model has no parts")?;
        let roots = parts(list, None, 0, &mut (0, 0))?;
        if roots.is_empty() {
            return Err("model has no parts".into());
        }
        Ok(Model { texture, roots })
    }

    /// `CosmeticModel.defaultSide`: the side of the first side-flagged root.
    fn default_side(&self) -> u8 {
        self.roots.iter().map(|r| r.side).find(|s| *s != 0).unwrap_or(0)
    }

    /// `CosmeticModel.coversHead`: a head root has a head-sized box the head's
    /// centre (0, -4, 0) sits inside, at rest. The Java mod hides the skin's hat
    /// layer and the helmet under such a hood; the game does not do that yet.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn covers_head(&self) -> bool {
        self.roots
            .iter()
            .filter(|r| r.bone == Bone::Head)
            .any(|r| wraps(r, glam::Mat4::IDENTITY))
    }

    /// The game's items for this model as `side` (1 left, 2 right, 0 the
    /// model's own) wears it, its texture already scaled to fit a skin slot.
    pub fn items(&self, id: &str, side: u8, texture: &Texture) -> Native {
        let side = if side != 0 { side } else { self.default_side() };
        let mut groups: Vec<(Armor, Vec<&NcmPart>)> = Vec::new();
        for root in self.roots.iter().filter(|r| r.side == 0 || r.side == side) {
            match groups.iter_mut().find(|(a, _)| *a == root.armor) {
                Some((_, list)) => list.push(root),
                None => groups.push((root.armor, vec![root])),
            }
        }
        let items = groups
            .into_iter()
            .map(|(armor, roots)| (armor, Arc::new(self.item(id, &roots, texture))))
            .collect();
        Native { items }
    }

    fn item(&self, id: &str, roots: &[&NcmPart], texture: &Texture) -> Item {
        const REST: horizon_cosmetics::Pose = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        let main = roots.first().map_or(Bone::Head, |r| r.bone);
        let mut animations = Vec::new();
        let mut on_bone: Vec<(Bone, Vec<Part>)> = Vec::new();
        for root in roots {
            let part = bake(root, self.texture, &mut animations);
            match on_bone.iter_mut().find(|(b, _)| *b == root.bone) {
                Some((_, list)) => list.push(part),
                None => on_bone.push((root.bone, vec![part])),
            }
        }
        let mut root = Part {
            name: "cosmetic".into(),
            pose: REST,
            faces: Vec::new(),
            children: Vec::new(),
        };
        let mut limbs = Vec::new();
        for (b, children) in on_bone {
            if b == main {
                root.children = children;
            } else {
                limbs.push((
                    b,
                    Part {
                        name: format!("cosmetic_{}", b.part()),
                        pose: REST,
                        faces: Vec::new(),
                        children,
                    },
                ));
            }
        }
        let vertices = count(&root) + limbs.iter().map(|(_, l)| count(l)).sum::<usize>();
        Item {
            id: format!("native:{id}"),
            // Never covered by the game's own rule: NCM parts carry their armour rules.
            slot: Slot::Shoulder,
            bone: main,
            root,
            texture: texture.clone(),
            animations,
            vertices,
            limbs,
            tag: None,
        }
    }
}

fn count(part: &Part) -> usize {
    part.faces.len() * 4 + part.children.iter().map(count).sum::<usize>()
}

/// The vanilla `ModelPart` matrix of a part's rest pose, in pixels:
/// translate by the pivot, then rotate z, y, x.
fn rest_matrix(p: &NcmPart) -> glam::Mat4 {
    let [rx, ry, rz] = p.rotation.map(f32::to_radians);
    glam::Mat4::from_translation(glam::Vec3::from(p.pivot))
        * glam::Mat4::from_quat(glam::Quat::from_euler(glam::EulerRot::ZYX, rz, ry, rx))
}

fn wraps(part: &NcmPart, parent: glam::Mat4) -> bool {
    let m = parent * rest_matrix(part);
    let head = m.inverse().transform_point3(glam::Vec3::new(0.0, -4.0, 0.0));
    let inside = part.cubes.iter().any(|c| {
        let e = c.inflate;
        let big = c.size.iter().all(|s| s + 2.0 * e >= 8.0);
        let within = (0..3).all(|i| head[i] >= c.origin[i] - e && head[i] <= c.origin[i] + c.size[i] + e);
        big && within
    });
    inside || part.children.iter().any(|c| wraps(c, m))
}

/// A part as the game's item has it, its animations added to `animations`.
fn bake(p: &NcmPart, texture: [f32; 2], animations: &mut Vec<Animation>) -> Part {
    let name = format!("n{}", p.index);
    for a in &p.anims {
        let Some(motion) = a.motion else { continue };
        if a.speed == 0.0 {
            continue;
        }
        // NCM speeds are per second (spin: degrees, the others: cycles); the
        // game's periods are ticks per cycle.
        let period = match motion {
            Motion::Spin => 20.0 * 360.0 / a.speed,
            Motion::Swing | Motion::Bob => 20.0 / a.speed,
        };
        animations.push(Animation {
            part: name.clone(),
            motion,
            axis: a.axis,
            amplitude: if motion == Motion::Spin { 0.0 } else { a.amplitude },
            period,
            phase: a.phase,
        });
    }
    let [rx, ry, rz] = p.rotation.map(f32::to_radians);
    let [x, y, z] = p.pivot;
    Part {
        name,
        pose: [x, y, z, rx, ry, rz, 1.0, 1.0, 1.0],
        faces: p.cubes.iter().flat_map(|c| box_faces(c, texture)).collect(),
        children: p.children.iter().map(|c| bake(c, texture, animations)).collect(),
    }
}

/// `ModelPart.Cube` as horizon-cosmetics bakes it (its `box_faces`): six
/// faces with box UV in vanilla's order, grown by `inflate`, mirrored in x on
/// request, UVs as fractions of the texture.
fn box_faces(cube: &Cube, [tw, th]: [f32; 2]) -> Vec<Face> {
    let [w, h, d] = cube.size;
    let g = cube.inflate;
    let (mut x0, y0, z0) = (cube.origin[0] - g, cube.origin[1] - g, cube.origin[2] - g);
    let (mut x1, y1, z1) = (
        cube.origin[0] + w + g,
        cube.origin[1] + h + g,
        cube.origin[2] + d + g,
    );
    if cube.mirror {
        std::mem::swap(&mut x0, &mut x1);
    }
    let t0 = [x0, y0, z0];
    let t1 = [x1, y0, z0];
    let t2 = [x1, y1, z0];
    let t3 = [x0, y1, z0];
    let l0 = [x0, y0, z1];
    let l1 = [x1, y0, z1];
    let l2 = [x1, y1, z1];
    let l3 = [x0, y1, z1];
    let [u, v] = cube.uv;
    let (u1, u2, u22) = (u + d, u + d + w, u + d + w + w);
    let (u3, u4) = (u + d + w + d, u + d + w + d + w);
    let (v1, v2) = (v + d, v + d + h);
    let faces: [([[f32; 3]; 4], [f32; 4], [f32; 3]); 6] = [
        ([l1, l0, t0, t1], [u1, v, u2, v1], [0.0, -1.0, 0.0]),
        ([t2, t3, l3, l2], [u2, v1, u22, v], [0.0, 1.0, 0.0]),
        ([t0, l0, l3, t3], [u, v1, u1, v2], [-1.0, 0.0, 0.0]),
        ([t1, t0, t3, t2], [u1, v1, u2, v2], [0.0, 0.0, -1.0]),
        ([l1, t1, t2, l2], [u2, v1, u3, v2], [1.0, 0.0, 0.0]),
        ([l0, l1, l2, l3], [u3, v1, u4, v2], [0.0, 0.0, 1.0]),
    ];
    faces
        .into_iter()
        .map(|(corners, [a, b, c, e], mut normal)| {
            let uvs = [[c, b], [a, b], [a, e], [c, e]];
            let mut vertices: [[f32; 5]; 4] = std::array::from_fn(|i| {
                let [x, y, z] = corners[i];
                [x, y, z, uvs[i][0] / tw, uvs[i][1] / th]
            });
            if cube.mirror {
                vertices.reverse();
                normal[0] = -normal[0];
            }
            Face { normal, vertices }
        })
        .collect()
}

/// A texture scaled down (nearest texel) to fit a 64×64 skin slot, if larger.
pub fn fit_slot(width: u32, height: u32, rgba: &[u8]) -> Texture {
    if width <= SLOT && height <= SLOT {
        return Texture {
            width,
            height,
            rgba: rgba.to_vec(),
        };
    }
    let scale = (SLOT as f32 / width as f32).min(SLOT as f32 / height as f32);
    let (w, h) = (
        ((width as f32 * scale) as u32).max(1),
        ((height as f32 * scale) as u32).max(1),
    );
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let (sx, sy) = (x * width / w, y * height / h);
            let i = ((sy * width + sx) * 4) as usize;
            out.extend_from_slice(&rgba[i..i + 4]);
        }
    }
    Texture {
        width: w,
        height: h,
        rgba: out,
    }
}

enum State {
    Loading,
    Ready(Arc<Native>),
    Failed(Instant),
}

/// Loaded cosmetics by (model hash, texture hash, side): shared by everyone
/// wearing the same files on the same side.
fn library() -> std::sync::MutexGuard<'static, HashMap<(String, String, u8), State>> {
    static LIBRARY: OnceLock<Mutex<HashMap<(String, String, u8), State>>> = OnceLock::new();
    LIBRARY
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// A file by hash: the disk cache in the game folder first, else Native.
fn fetch(base: &str, hash: &str, limit: u64) -> Option<Vec<u8>> {
    let cache = std::env::current_dir().ok()?.join("assets/native").join(hash);
    if let Ok(bytes) = std::fs::read(&cache)
        && !bytes.is_empty()
    {
        return Some(bytes);
    }
    let response = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(8))
        .timeout_read(Duration::from_secs(20))
        .user_agent("NativeClient (Rust engine)")
        .build()
        .get(&format!("{base}{hash}"))
        .call()
        .ok()?;
    let mut body = Vec::new();
    response.into_reader().take(limit + 1).read_to_end(&mut body).ok()?;
    if body.len() as u64 > limit {
        return None;
    }
    let _ = std::fs::create_dir_all(cache.parent()?);
    let _ = std::fs::write(&cache, &body);
    Some(body)
}

fn load(base: &str, cosmetic: &horizon_client::native_skins::CosmeticRef) -> Option<Native> {
    let model = Model::parse(&fetch(base, &cosmetic.model, MAX_MODEL_BYTES)?)
        .inspect_err(|e| log::warn!("Native: cosmetic {} is not a model: {e}", cosmetic.id))
        .ok()?;
    let png = fetch(base, &cosmetic.texture, MAX_TEXTURE_BYTES)?;
    let (w, h, rgba) = horizon_client::hud::decode_bytes(&png).ok()?;
    Some(model.items(&cosmetic.id, cosmetic.side, &fit_slot(w, h, &rgba)))
}

/// The cosmetic once its files are loaded; the first ask starts the download.
fn get(base: &str, cosmetic: &horizon_client::native_skins::CosmeticRef) -> Option<Arc<Native>> {
    let key = (cosmetic.model.clone(), cosmetic.texture.clone(), cosmetic.side);
    let mut cache = library();
    match cache.get(&key) {
        Some(State::Ready(native)) => return Some(native.clone()),
        Some(State::Loading) => return None,
        Some(State::Failed(at)) if at.elapsed() < RETRY => return None,
        _ => {}
    }
    cache.insert(key.clone(), State::Loading);
    let (base, cosmetic, done) = (base.to_owned(), cosmetic.clone(), key.clone());
    let spawned = std::thread::Builder::new()
        .name("Native cosmetic".into())
        .spawn(move || {
            let state = match load(&base, &cosmetic) {
                Some(native) => State::Ready(Arc::new(native)),
                None => State::Failed(Instant::now()),
            };
            library().insert(done, state);
        });
    if spawned.is_err() {
        cache.insert(key, State::Failed(Instant::now()));
    }
    None
}

impl Game<'_> {
    /// The Native cosmetics `m` wears and shows now (armour hides what it
    /// covers): the player's own model when `own`, else another player.
    pub(super) fn native_cosmetics_worn(&self, m: &Mob, own: bool) -> Vec<Arc<Item>> {
        let who = if own {
            self.remote
                .as_ref()
                .map(|r| (r.profile_name.clone(), Some(r.profile())))
        } else if &*m.kind.id == "minecraft:player" {
            self.remote_player_name(m.id)
                .map(|name| (name.to_owned(), self.remote_player_uuid(m.id)))
        } else {
            None
        };
        let Some((name, uuid)) = who.filter(|(n, _)| !n.is_empty()) else {
            return Vec::new();
        };
        let self_name = horizon_client::discord_presence::native_account();
        let (base, refs) = {
            let directory = horizon_client::native_skins::directory();
            (
                directory.texture_base().to_owned(),
                directory.cosmetics(&name, uuid, self_name.as_deref()).to_vec(),
            )
        };
        if base.is_empty() || refs.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for cosmetic in &refs {
            if let Some(native) = get(&base, cosmetic) {
                out.extend(
                    native
                        .items
                        .iter()
                        .filter(|(armor, _)| !armor.worn(m))
                        .map(|(_, item)| item.clone()),
                );
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HAT: &str = r#"{"format":1, "texture":[64,32], "parts":[
        {"id":"cap", "attach":"head", "armor":{"slot":"head","mode":"hide"},
         "cubes":[{"origin":[-4,-9,-4], "size":[8,1,8], "uv":[0,0], "inflate":0.25}],
         "anim":[{"type":"spin","axis":"y","speed":540}, {"type":"blink"}],
         "children":[{"id":"bobble", "cubes":[{"origin":[-1,-11,-1], "size":[2,2,2], "uv":[32,0]}],
                      "anim":[{"type":"bob","speed":2,"amplitude":1}]}]},
        {"id":"wing", "attach":"body", "side":"left", "cubes":[{"origin":[0,0,2], "size":[1,8,6], "uv":[0,10]}]},
        {"id":"shoe", "attach":"left_foot", "cubes":[{"origin":[-2,10,-2], "size":[4,2,4], "uv":[0,20]}]}
    ]}"#;

    fn tex() -> Texture {
        Texture {
            width: 64,
            height: 32,
            rgba: vec![255; 64 * 32 * 4],
        }
    }

    #[test]
    fn armour_rules_become_items_of_their_own() {
        let model = Model::parse(HAT.as_bytes()).unwrap();
        let native = model.items("party_hat", 0, &tex());
        let armors: Vec<_> = native.items.iter().map(|(a, _)| *a).collect();
        assert_eq!(armors, vec![Armor::Head, Armor::None]);
        let (_, hat) = &native.items[0];
        assert_eq!(hat.id, "native:party_hat");
        assert_eq!(hat.bone, Bone::Head);
        // The cap's 6 faces and the bobble's 6.
        assert_eq!(hat.vertices, 12 * 4);
        // The others ride the body (main) and the left leg (a limb).
        let (_, rest) = &native.items[1];
        assert_eq!(rest.bone, Bone::Body);
        assert_eq!(rest.limbs.len(), 1);
        assert_eq!(rest.limbs[0].0, Bone::LeftLeg);
    }

    #[test]
    fn animations_keep_their_speed_and_blinks_are_dropped() {
        let model = Model::parse(HAT.as_bytes()).unwrap();
        let hat = &model.items("h", 0, &tex()).items[0].1;
        assert_eq!(hat.animations.len(), 2);
        let spin = &hat.animations[0];
        assert_eq!(spin.motion, Motion::Spin);
        // 540 degrees a second: one turn in 2/3 s, 40/3 ticks.
        assert!((spin.period - 40.0 / 3.0).abs() < 1e-4);
        let bob = &hat.animations[1];
        assert_eq!((bob.motion, bob.period, bob.amplitude), (Motion::Bob, 10.0, 1.0));
    }

    #[test]
    fn a_side_only_part_shows_on_its_side() {
        let model = Model::parse(HAT.as_bytes()).unwrap();
        let has_wing = |side| {
            model.items("h", side, &tex()).items.iter().any(|(_, i)| {
                i.root.children.iter().any(|p| p.name == "n2")
            })
        };
        // The model's own side is the wing's (left).
        assert!(has_wing(0));
        assert!(has_wing(1));
        assert!(!has_wing(2));
    }

    #[test]
    fn faces_use_fractions_of_the_declared_texture() {
        let model = Model::parse(HAT.as_bytes()).unwrap();
        let hat = &model.items("h", 0, &tex()).items[0].1;
        let cap = &hat.root.children[0];
        // Up face of an 8x1x8 box at uv 0,0: u from 16 to 24 of 64, v from 0 to 8 of 32.
        let up = &cap.faces[1];
        let us: Vec<f32> = up.vertices.iter().map(|v| v[3]).collect();
        let vs: Vec<f32> = up.vertices.iter().map(|v| v[4]).collect();
        assert!(us.iter().all(|u| (16.0 / 64.0..=24.0 / 64.0).contains(u)));
        assert!(vs.iter().all(|v| (0.0..=8.0 / 32.0).contains(v)));
    }

    #[test]
    fn a_hood_covers_the_head_and_a_cap_does_not() {
        let cap = Model::parse(HAT.as_bytes()).unwrap();
        assert!(!cap.covers_head());
        let hood = r#"{"parts":[{"attach":"head","cubes":[{"origin":[-4.5,-8.5,-4.5],"size":[9,9,9],"uv":[0,0]}]}]}"#;
        assert!(Model::parse(hood.as_bytes()).unwrap().covers_head());
    }

    #[test]
    fn big_textures_shrink_to_a_skin_slot() {
        let t = fit_slot(128, 64, &vec![9; 128 * 64 * 4]);
        assert_eq!((t.width, t.height), (64, 32));
        assert_eq!(t.rgba.len(), 64 * 32 * 4);
        let small = fit_slot(32, 32, &vec![1; 32 * 32 * 4]);
        assert_eq!((small.width, small.height), (32, 32));
    }

    #[test]
    fn malformed_models_are_refused() {
        assert!(Model::parse(b"[]").is_err());
        assert!(Model::parse(br#"{"format":2,"parts":[{}]}"#).is_err());
        assert!(Model::parse(br#"{"parts":[]}"#).is_err());
        assert!(Model::parse(br#"{"texture":[0,0],"parts":[{}]}"#).is_err());
    }
}
