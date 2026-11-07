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
/// Bigger than this (the atlas's Native area, `AREA`) and a texture is scaled down to fit (the faces' UVs are
/// fractions of the texture, so they still fit).
const SLOT: u32 = AREA;

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

impl Armor {
    fn bit(self) -> u8 {
        match self {
            Armor::None => 0,
            Armor::Head => 1,
            Armor::Chest => 2,
            Armor::Legs => 4,
            Armor::Feet => 8,
        }
    }
}

/// When an item shows: it depends on the armour slots in `mask`, and shows while exactly `state` of them are worn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rule {
    mask: u8,
    state: u8,
}

impl Rule {
    /// Whether the item shows on `m`, by the armour it wears.
    pub fn applies(self, m: &Mob) -> bool {
        let worn = [Armor::Head, Armor::Chest, Armor::Legs, Armor::Feet]
            .into_iter()
            .filter(|a| a.worn(m))
            .fold(0, |bits, a| bits | a.bit());
        worn & self.mask == self.state
    }
}

/// A Native cosmetic ready to draw: the game's items, each for one combination of worn armour.
#[derive(Debug)]
pub struct Native {
    pub items: Vec<(Rule, Arc<Item>)>,
    /// `CosmeticModel.coversHead`: a hood, helmet or mask the skin's hat layer and the helmet must not poke through.
    pub covers_head: bool,
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
    /// With `armor`: the part moves by this (pixels) while the slot is worn, instead of hiding.
    push: Option<[f32; 3]>,
    /// 0 any, 1 left only, 2 right only.
    side: u8,
    /// `layer: "glow"` (or `"emissive"`, or `glow: true`): drawn at full brightness, like the Java mod's eyes layer.
    glow: bool,
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

/// `CosmeticModel`'s armour rule: the slot, and for a `push` rule the offset (pixels) the part moves by while
/// that slot is worn (else the part hides).
fn armor(o: &serde_json::Value) -> (Armor, Option<[f32; 3]>) {
    let Some(a) = o.get("armor").filter(|a| a.is_object()) else {
        return (Armor::None, None);
    };
    let push = (text(a, "mode", "hide") == "push").then(|| clamp_all(floats(a, "offset", [0.0; 3]), 8.0));
    let slot = match text(a, "slot", "none").to_ascii_lowercase().replace(['_', '-'], "").as_str() {
        "head" | "helmet" => Armor::Head,
        "chest" => Armor::Chest,
        "legs" => Armor::Legs,
        "feet" | "boots" => Armor::Feet,
        _ => Armor::None,
    };
    (slot, push)
}

fn anim(o: &serde_json::Value) -> Option<Anim> {
    let kind = text(o, "type", "").to_ascii_lowercase();
    let motion = match kind.as_str() {
        "spin" => Some(Motion::Spin),
        "swing" | "flap" => Some(Motion::Swing),
        "bob" => Some(Motion::Bob),
        // Blinking (shown for part of each cycle): no motion, the part goes in an item that hides on cue.
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
        amplitude: num(o, "amplitude", if motion.is_none() { 0.5 } else { 10.0 }, 360.0),
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
            armor: armor(o).0,
            push: armor(o).1,
            side: match text(o, "side", "") {
                "left" => 1,
                "right" => 2,
                _ => 0,
            },
            glow: matches!(
                text(o, "layer", if o.get("glow").and_then(serde_json::Value::as_bool) == Some(true) { "glow" } else { "cutout" }),
                "glow" | "emissive"
            ),
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
    /// layer and the helmet under such a hood; so does the game ([`set_hooded`](horizon_client::native_skins::set_hooded)).
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
        let roots: Vec<&NcmPart> = self.roots.iter().filter(|r| r.side == 0 || r.side == side).collect();
        // The armour slots any part reacts to; the model is baked once for each combination of them worn: a part
        // under a hiding rule is left out, one under a push rule moves (the Java mod's `armorHide` / `armorOffset`).
        let mask = roots.iter().fold(0, |bits, r| bits | armor_slots(r));
        // Each blinking part (with what hangs on it) is an item of its own, hidden on cue, as the Java mod
        // skips the part and its children; glowing parts are one too, drawn at full brightness (the Java
        // mod's eyes layer).
        let mut gates = vec![(None, Vec::new())];
        for root in &roots {
            blink_gates(root, &[], &mut gates);
        }
        let mut items = Vec::new();
        for worn in (0..16u8).filter(|w| w & !mask == 0) {
            for (gate, blinks) in &gates {
                for glow in [false, true] {
                    let layer = Layer { glow, gate: *gate, worn, tagged: mask != 0 };
                    let item = self.item(id, &roots, texture, layer);
                    if item.vertices == 0 {
                        continue;
                    }
                    if !blinks.is_empty() {
                        blink_registry().insert(item.id.clone(), blinks.clone());
                    }
                    items.push((Rule { mask, state: worn }, Arc::new(item)));
                }
            }
        }
        Native { items, covers_head: self.covers_head() }
    }

    fn item(&self, id: &str, roots: &[&NcmPart], texture: &Texture, layer: Layer) -> Item {
        const REST: horizon_cosmetics::Pose = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        let main = roots.first().map_or(Bone::Head, |r| r.bone);
        let mut animations = Vec::new();
        let mut on_bone: Vec<(Bone, Vec<Part>)> = Vec::new();
        for root in roots {
            let part = bake(root, self.texture, &mut animations, layer, None);
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
            id: format!(
                "native:{id}{}{}",
                format!(
                    "{}{}",
                    if layer.tagged { format!("#a{}", layer.worn) } else { String::new() },
                    layer.gate.map(|g| format!("#b{g}")).unwrap_or_default()
                ),
                if layer.glow { GLOW } else { "" }
            ),
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

/// The id ending of an item holding a model's glowing parts.
pub const GLOW: &str = "#glow";

/// The light a cosmetic item is drawn with: full brightness for the glowing parts.
pub fn light_of(item: &Item, light: u32) -> u32 {
    if item.id.starts_with("native:") && item.id.ends_with(GLOW) { horizon_client::mobs::render::FULL_BRIGHT } else { light }
}

/// Which faces an item holds: the glowing parts' or the others', of the parts whose nearest blinking part
/// (themselves or a parent) is `gate`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Layer {
    glow: bool,
    gate: Option<usize>,
    /// The armour slots worn ([`Armor::bit`]s) this item is baked for.
    worn: u8,
    /// Whether the model has armour rules (then the worn slots are part of the item's id).
    tagged: bool,
}

/// The armour slots this part or its children react to.
fn armor_slots(p: &NcmPart) -> u8 {
    p.children.iter().fold(p.armor.bit(), |bits, c| bits | armor_slots(c))
}

/// A blink channel: cycles per second, the part of each cycle the part shows (0..1), phase (cycles).
type Blink = [f32; 3];

fn blinks_of(p: &NcmPart) -> impl Iterator<Item = Blink> + '_ {
    p.anims.iter().filter(|a| a.motion.is_none()).map(|a| [a.speed, a.amplitude, a.phase])
}

/// Every blinking part under `p`, with the channels it and its parents blink by.
fn blink_gates(p: &NcmPart, above: &[Blink], out: &mut Vec<(Option<usize>, Vec<Blink>)>) {
    let mut chain = above.to_vec();
    let own: Vec<Blink> = blinks_of(p).collect();
    if !own.is_empty() {
        chain.extend(own);
        out.push((Some(p.index), chain.clone()));
    }
    for c in &p.children {
        blink_gates(c, &chain, out);
    }
}

fn blink_registry() -> std::sync::MutexGuard<'static, HashMap<String, Vec<Blink>>> {
    static BLINKS: OnceLock<Mutex<HashMap<String, Vec<Blink>>>> = OnceLock::new();
    BLINKS.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

/// `CosmeticPose.pose`'s blink: a channel hides its part once `(speed × time + phase) mod 1` reaches the
/// amplitude. False while any channel of this item's blinking part (or of a part it hangs on) hides it.
pub fn shown(item: &Item, ticks: f32) -> bool {
    if !item.id.starts_with("native:") || !item.id.contains("#b") {
        return true;
    }
    let time = ticks / 20.0;
    match blink_registry().get(&item.id) {
        Some(blinks) => blinks.iter().all(|[speed, amplitude, phase]| (speed * time + phase).rem_euclid(1.0) < *amplitude),
        None => true,
    }
}

/// A part as the game's item has it, its animations added to `animations`; only the faces of the parts on
/// `layer`, every part keeping its place so children move with their parents.
fn bake(p: &NcmPart, texture: [f32; 2], animations: &mut Vec<Animation>, layer: Layer, gate: Option<usize>) -> Part {
    let gate = if blinks_of(p).next().is_some() { Some(p.index) } else { gate };
    let name = format!("n{}", p.index);
    // The armour rule: the slot is worn, so the part (and what hangs on it) hides, or moves aside.
    let mut push = [0.0; 3];
    if p.armor.bit() & layer.worn != 0 {
        match p.push {
            Some(offset) => push = offset,
            None => return Part { name, pose: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0], faces: Vec::new(), children: Vec::new() },
        }
    }
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
    let [x, y, z] = [p.pivot[0] + push[0], p.pivot[1] + push[1], p.pivot[2] + push[2]];
    Part {
        name,
        pose: [x, y, z, rx, ry, rz, 1.0, 1.0, 1.0],
        faces: if p.glow == layer.glow && gate == layer.gate {
            p.cubes.iter().flat_map(|c| box_faces(c, texture)).collect()
        } else {
            Vec::new()
        },
        children: p.children.iter().map(|c| bake(c, texture, animations, layer, gate)).collect(),
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

/// A texture as it goes into the atlas: as it is, or (only when bigger than the Native area) shrunk to fit,
/// every texel the average of the ones it covers.
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
        let (y0, y1) = (y * height / h, ((y + 1) * height / h).max(y * height / h + 1).min(height));
        for x in 0..w {
            let (x0, x1) = (x * width / w, ((x + 1) * width / w).max(x * width / w + 1).min(width));
            let mut sum = [0u32; 4];
            let mut n = 0;
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let i = ((sy * width + sx) * 4) as usize;
                    for c in 0..4 {
                        sum[c] += u32::from(rgba[i + c]);
                    }
                    n += 1;
                }
            }
            out.extend(sum.iter().map(|s| (s / n.max(1)) as u8));
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
            horizon_client::native_skins::set_hooded(m.id, false);
            return Vec::new();
        }
        let shown = own || self.settings.extras.effective_others() == horizon_cosmetics::prefs::Others::Shown;
        let mut out = Vec::new();
        let mut hooded = false;
        for cosmetic in &refs {
            if let Some(native) = get(&base, cosmetic) {
                hooded |= shown && native.covers_head;
                out.extend(
                    native
                        .items
                        .iter()
                        .filter(|(rule, _)| rule.applies(m))
                        .map(|(_, item)| item.clone()),
                );
            }
        }
        // A hood, helmet or mask replaces the skin's hat layer and the helmet (the Java mod's `HeadCover`).
        horizon_client::native_skins::set_hooded(m.id, hooded);
        out
    }
}

// ── the atlas's Native area ──────────────────────────────────────────────

/// One area of the atlas (`native/cosmetics`) holds every Native cosmetic texture at its own size, packed in
/// rows (shelves). Most NCM textures are long strips (wings are 2032×126): a square slot made them a fraction of
/// their width and they looked speckled. When the area is full it starts over: textures go back in as they are
/// drawn.
pub const AREA: u32 = 2048;

#[derive(Clone, Debug, PartialEq)]
struct Placed {
    /// The texture (its model's id and size): the items of one model (plain, glowing, blinking) share it.
    key: String,
    /// The atlas sprites naming it, one per item.
    locations: Vec<String>,
    rect: [u32; 4],
    drawn: Instant,
}

#[derive(Default)]
struct Packer {
    /// The atlas the area belongs to (`HudAtlas::revision`): a rebuilt atlas starts with it empty.
    revision: u64,
    placed: Vec<Placed>,
    /// Shelves: (top, height, used width).
    shelves: Vec<(u32, u32, u32)>,
}

/// Where a texture went: its place in the area, whether its pixels must be uploaded (else they are there
/// already), and the sprites that left the area.
#[derive(Debug, PartialEq)]
struct Placement {
    rect: [u32; 4],
    upload: bool,
    evicted: Vec<String>,
}

impl Packer {
    /// Room for a `w`×`h` texture in the shelves as they are.
    fn fit(&mut self, w: u32, h: u32) -> Option<[u32; 2]> {
        // The best shelf: tall enough, wide enough, wasting the least height.
        let best = self
            .shelves
            .iter()
            .enumerate()
            .filter(|(_, (_, sh, used))| *sh >= h && AREA - used >= w)
            .min_by_key(|(_, (_, sh, _))| *sh - h)
            .map(|(i, _)| i);
        if let Some(i) = best {
            let (top, _, used) = self.shelves[i];
            self.shelves[i].2 += w;
            return Some([used, top]);
        }
        let bottom = self.shelves.last().map_or(0, |(top, sh, _)| top + sh);
        if bottom + h > AREA || w > AREA {
            return None;
        }
        self.shelves.push((bottom, h, w));
        Some([0, bottom])
    }

    /// A place for texture `key` drawn through sprite `location`: where it is, or new room. When there is none
    /// the area starts over (every sprite in it is named, to be taken out of the atlas and put back when drawn).
    fn place(&mut self, key: &str, location: &str, w: u32, h: u32, now: Instant) -> Option<Placement> {
        if let Some(p) = self.placed.iter_mut().find(|p| p.key == key) {
            if !p.locations.iter().any(|l| l == location) {
                p.locations.push(location.to_owned());
            }
            p.drawn = now;
            return Some(Placement { rect: p.rect, upload: false, evicted: Vec::new() });
        }
        let mut evicted = Vec::new();
        let spot = match self.fit(w, h) {
            Some(spot) => spot,
            None => {
                evicted = std::mem::take(&mut self.placed).into_iter().flat_map(|p| p.locations).collect();
                self.shelves.clear();
                self.fit(w, h)?
            }
        };
        let rect = [spot[0], spot[1], w, h];
        self.placed.push(Placed { key: key.to_owned(), locations: vec![location.to_owned()], rect, drawn: now });
        Some(Placement { rect, upload: true, evicted })
    }
}

fn packer() -> std::sync::MutexGuard<'static, Packer> {
    static PACKER: OnceLock<Mutex<Packer>> = OnceLock::new();
    PACKER.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

/// A Native item's texture was drawn this frame.
pub fn drawn(location: &str) {
    let mut p = packer();
    if let Some(placed) = p.placed.iter_mut().find(|p| p.locations.iter().any(|l| l == location)) {
        placed.drawn = Instant::now();
    }
}

/// The size of the atlas region an item's texture fills, in texels: a Native texture has a region of exactly its
/// own size, a Horizon item the top-left of a 64×64 skin slot.
pub fn slot_width(item: &Item) -> f32 {
    if item.id.starts_with("native:") { item.texture.width.max(1) as f32 } else { 64.0 }
}

pub fn slot_height(item: &Item) -> f32 {
    if item.id.starts_with("native:") { item.texture.height.max(1) as f32 } else { 64.0 }
}

/// The texture an item draws: its model's (the id before any `#` ending) at its size.
fn texture_key(item: &Item) -> String {
    let model = item.id.split('#').next().unwrap_or(&item.id);
    format!("{model}@{}x{}", item.texture.width, item.texture.height)
}

impl Game<'_> {
    /// Puts a Native item's texture into the atlas's Native area, at its own resolution.
    pub(super) fn store_native_cosmetic(&mut self, location: &str, item: &Item) {
        let t = &item.texture;
        let (w, h) = (t.width.clamp(1, AREA), t.height.clamp(1, AREA));
        let placed = {
            let mut p = packer();
            if p.revision != self.gui.atlas.revision {
                *p = Packer { revision: self.gui.atlas.revision, ..Packer::default() };
            }
            p.place(&texture_key(item), location, w, h, Instant::now())
        };
        let Some(placed) = placed else { return };
        for old in placed.evicted {
            self.gui.atlas.sprites.remove(&old);
        }
        let Some(area) = self.gui.atlas.sprites.get("native/cosmetics").copied() else { return };
        let (aw, ah) = (self.gui.atlas.width as f32, self.gui.atlas.height as f32);
        let (ax, ay) = ((area.u0 * aw).round() as u32, (area.v0 * ah).round() as u32);
        let rect = placed.rect;
        if placed.upload {
            let mut rgba = Vec::with_capacity((w * h * 4) as usize);
            for y in 0..h as usize {
                let start = y * t.width as usize * 4;
                rgba.extend_from_slice(&t.rgba[start..start + w as usize * 4]);
            }
            self.map_patches.push(horizon_client::font::Patch { x: ax + rect[0], y: ay + rect[1], width: w, height: h, rgba });
        }
        let mut sprite = area;
        sprite.u0 = (ax + rect[0]) as f32 / aw;
        sprite.v0 = (ay + rect[1]) as f32 / ah;
        sprite.u1 = (ax + rect[0] + w) as f32 / aw;
        sprite.v1 = (ay + rect[1] + h) as f32 / ah;
        self.gui.atlas.sprites.insert(location.to_owned(), sprite);
    }
}

#[cfg(test)]
mod slot_tests {
    use super::*;

    #[test]
    fn strips_are_packed_at_their_own_size() {
        let now = Instant::now();
        let mut p = Packer::default();
        let a = p.place("wings", "w1", 2032, 126, now).unwrap();
        let b = p.place("tails", "t1", 300, 17, now).unwrap();
        let c = p.place("hat", "h1", 64, 64, now).unwrap();
        assert_eq!(a.rect, [0, 0, 2032, 126]);
        assert!(a.upload && b.upload && c.upload);
        // A short texture shares a shelf with room for it, else gets one of its own.
        assert!(b.rect[1] >= 126 || b.rect[0] >= 2032);
        assert_ne!(b.rect, c.rect);
        // The glowing item of the same model draws the same pixels: nothing new to upload.
        let again = p.place("wings", "w1-glow", 2032, 126, now).unwrap();
        assert_eq!((again.rect, again.upload), (a.rect, false));
    }

    #[test]
    fn a_full_area_starts_over() {
        let t0 = Instant::now();
        let mut p = Packer::default();
        for i in 0..16 {
            p.place(&format!("old{i}"), &format!("s{i}"), 2048, 128, t0).unwrap();
        }
        p.place("old0", "s0b", 2048, 128, t0).unwrap();
        let fresh = p.place("new", "n", 2048, 128, t0 + Duration::from_secs(2)).unwrap();
        assert_eq!(fresh.rect, [0, 0, 2048, 128]);
        assert_eq!(fresh.evicted.len(), 17);
    }

    #[test]
    fn glowing_parts_become_a_full_bright_item() {
        let model = Model::parse(
            br#"{"texture":[64,64],"parts":[{"attach":"head","cubes":[{"origin":[0,0,0],"size":[1,1,1],"uv":[0,0]}],
                 "children":[{"layer":"glow","cubes":[{"origin":[0,0,0],"size":[1,1,1],"uv":[8,0]}]}]}]}"#,
        )
        .unwrap();
        let tex = Texture { width: 64, height: 64, rgba: vec![255; 64 * 64 * 4] };
        let items = model.items("halo", 0, &tex).items;
        assert_eq!(items.len(), 2);
        let (plain, glow) = (&items[0].1, &items[1].1);
        assert_eq!(light_of(plain, 7), 7);
        assert_eq!(light_of(glow, 7), horizon_client::mobs::render::FULL_BRIGHT);
        // Each holds only its own faces: one box each.
        assert_eq!((plain.vertices, glow.vertices), (24, 24));
        assert_eq!((slot_width(plain), slot_height(plain)), (64.0, 64.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HAT: &str = r#"{"format":1, "texture":[64,32], "parts":[
        {"id":"cap", "attach":"head", "armor":{"slot":"head","mode":"hide"},
         "cubes":[{"origin":[-4,-9,-4], "size":[8,1,8], "uv":[0,0], "inflate":0.25}],
         "anim":[{"type":"spin","axis":"y","speed":540}],
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
        // The cap hides under a helmet: the model is baked for the head slot empty and worn.
        assert_eq!(native.items.len(), 2);
        let ((open_rule, open), (worn_rule, worn)) = (&native.items[0], &native.items[1]);
        assert_eq!((open_rule.mask, open_rule.state), (Armor::Head.bit(), 0));
        assert_eq!((worn_rule.mask, worn_rule.state), (Armor::Head.bit(), Armor::Head.bit()));
        assert_eq!(open.id, "native:party_hat#a0");
        assert_eq!(open.bone, Bone::Head);
        // The cap's 6 faces, the bobble's 6, the wing's 6 and the shoe's 6; under a helmet the wing and the shoe.
        assert_eq!(open.vertices, 24 * 4);
        assert_eq!(worn.vertices, 12 * 4);
        // The wing rides the body and the shoe the left leg (limbs of the item).
        assert_eq!(open.limbs.len(), 2);
        assert_eq!(open.limbs[0].0, Bone::Body);
        assert_eq!(open.limbs[1].0, Bone::LeftLeg);
        assert!(!model.covers_head());
        assert!(!native.covers_head);
    }

    #[test]
    fn push_rules_move_a_part_while_armour_is_worn() {
        let model = Model::parse(
            br#"{"texture":[64,64],"parts":[{"attach":"body","pivot":[0,2,0],
                 "armor":{"slot":"chest","mode":"push","offset":[0,1,3]},
                 "cubes":[{"origin":[0,0,0],"size":[1,1,1],"uv":[0,0]}],
                 "children":[{"pivot":[1,0,0],"cubes":[{"origin":[0,0,0],"size":[1,1,1],"uv":[8,0]}]}]}]}"#,
        )
        .unwrap();
        let items = model.items("cape", 0, &tex()).items;
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].0.state, Armor::Chest.bit());
        let moved = |i: usize| items[i].1.root.children[0].pose;
        assert_eq!(moved(0)[..3], [0.0, 2.0, 0.0]);
        assert_eq!(moved(1)[..3], [0.0, 3.0, 3.0]);
        // What hangs on the part goes along with it, as it moves through the parent's matrix.
        assert_eq!(items[1].1.vertices, 48);
    }

    #[test]
    fn animations_keep_their_speed() {
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
    fn blinking_parts_hide_for_part_of_each_cycle() {
        // A visor that blinks once a second, shown for the first half; its light hangs on it and blinks with it.
        let model = Model::parse(
            br#"{"texture":[64,64],"parts":[{"attach":"head","cubes":[{"origin":[0,0,0],"size":[1,1,1],"uv":[0,0]}],
                 "children":[{"anim":[{"type":"blink","speed":1,"amplitude":0.5}],
                              "cubes":[{"origin":[0,0,0],"size":[1,1,1],"uv":[8,0]}],
                              "children":[{"cubes":[{"origin":[0,0,0],"size":[1,1,1],"uv":[16,0]}]}]}]}]}"#,
        )
        .unwrap();
        let tex = Texture { width: 64, height: 64, rgba: vec![255; 64 * 64 * 4] };
        let items = model.items("visor", 0, &tex).items;
        assert_eq!(items.len(), 2);
        let (plain, visor) = (&items[0].1, &items[1].1);
        assert_eq!(plain.vertices, 24);
        assert_eq!(visor.vertices, 48);
        assert_eq!(visor.id, "native:visor#b1");
        // Ticks: 20 a second. A quarter second in it shows, three quarters in it is hidden (CosmeticPose).
        assert!(shown(plain, 15.0));
        assert!(shown(visor, 5.0));
        assert!(!shown(visor, 15.0));
        assert!(shown(visor, 25.0));
    }
    #[test]
    fn a_side_only_part_shows_on_its_side() {
        let model = Model::parse(HAT.as_bytes()).unwrap();
        let has_wing = |side| {
            model.items("h", side, &tex()).items.iter().any(|(_, i)| {
                i.limbs.iter().any(|(_, l)| l.children.iter().any(|p| p.name == "n2"))
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
    fn big_textures_shrink_to_a_native_slot() {
        let t = fit_slot(4096, 64, &vec![9; 4096 * 64 * 4]);
        assert_eq!((t.width, t.height), (2048, 32));
        assert_eq!(t.rgba.len(), 2048 * 32 * 4);
        // Shrinking averages: a black and white checker turns grey, not speckled.
        let checker: Vec<u8> = (0..4096 * 2).flat_map(|i| if (i % 4096 + i / 4096) % 2 == 0 { [0, 0, 0, 255] } else { [255, 255, 255, 255] }).collect();
        let grey = fit_slot(4096, 2, &checker);
        assert!(grey.rgba.chunks(4).all(|p| p[0] == 127 && p[3] == 255));
        // Wings (2032×126) keep every texel.
        let kept = fit_slot(2032, 126, &vec![9; 2032 * 126 * 4]);
        assert_eq!((kept.width, kept.height), (2032, 126));
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

// ── the Locker's preview ─────────────────────────────────────────────────

/// A box of a part, for the preview to draw (`CosmeticModel.Cube`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewCube {
    pub origin: [f32; 3],
    pub size: [f32; 3],
    pub uv: [f32; 2],
    pub inflate: f32,
    pub mirror: bool,
}

/// One step of walking a model the way `CosmeticRenderer` does.
#[derive(Debug)]
pub enum Walk<'a> {
    Push,
    Pop,
    /// Into a root part: the bone it hangs on.
    Root(Bone),
    /// The part's pose after its animations: pivot in pixels, rotation in radians.
    Transform { pos: [f32; 3], rot: [f32; 3] },
    Cubes(&'a [PreviewCube]),
}

fn axis_index(axis: Axis) -> usize {
    match axis {
        Axis::X => 0,
        Axis::Y => 1,
        Axis::Z => 2,
    }
}

/// Position in an animation's cycle, 0 to 1 (`CosmeticPose.cycle`).
fn cycle(a: &Anim, time: f64) -> f64 {
    (f64::from(a.speed) * time + f64::from(a.phase)).rem_euclid(1.0)
}

fn walk_part(p: &NcmPart, time: f64, sink: &mut dyn FnMut(Walk)) {
    sink(Walk::Push);
    let mut pos = p.pivot;
    let mut rot = p.rotation.map(f32::to_radians);
    for a in &p.anims {
        let i = axis_index(a.axis);
        match a.motion {
            Some(Motion::Spin) => {
                let degrees = (f64::from(a.speed) * time + f64::from(a.phase) * 360.0).rem_euclid(360.0);
                rot[i] += (degrees as f32).to_radians();
            }
            Some(Motion::Swing) => {
                rot[i] += (f64::from(a.amplitude) * (std::f64::consts::TAU * cycle(a, time)).sin()).to_radians() as f32;
            }
            Some(Motion::Bob) => {
                pos[i] += (f64::from(a.amplitude) * (std::f64::consts::TAU * cycle(a, time)).sin()) as f32;
            }
            None => {}
        }
    }
    sink(Walk::Transform { pos, rot });
    if !p.cubes.is_empty() {
        let cubes: Vec<PreviewCube> = p
            .cubes
            .iter()
            .map(|c| PreviewCube { origin: c.origin, size: c.size, uv: c.uv, inflate: c.inflate, mirror: c.mirror })
            .collect();
        sink(Walk::Cubes(&cubes));
    }
    for child in &p.children {
        walk_part(child, time, sink);
    }
    sink(Walk::Pop);
}

impl Model {
    /// The texture size the model's UVs are on.
    pub fn texture_size(&self) -> [f32; 2] {
        self.texture
    }

    /// Walks the parts `side` wears at `time` seconds, as `CosmeticRenderer.render` does with no armour worn.
    pub fn walk(&self, side: u8, time: f64, sink: &mut dyn FnMut(Walk)) {
        let side = if side != 0 { side } else { self.default_side() };
        for root in self.roots.iter().filter(|r| r.side == 0 || r.side == side) {
            sink(Walk::Push);
            sink(Walk::Root(root.bone));
            walk_part(root, time, sink);
            sink(Walk::Pop);
        }
    }

    /// `CosmeticModel.coversHead`, for the preview (a hood hides the skin's hat layer).
    pub fn hides_hat(&self) -> bool {
        self.roots.iter().filter(|r| r.bone == Bone::Head).any(|r| wraps(r, glam::Mat4::IDENTITY))
    }
}

enum Preview {
    Loading,
    Ready(Arc<Model>),
    Failed(Instant),
}

fn previews() -> &'static Mutex<HashMap<String, Preview>> {
    static MODELS: OnceLock<Mutex<HashMap<String, Preview>>> = OnceLock::new();
    MODELS.get_or_init(Default::default)
}

/// A cosmetic's model for the Locker's preview; the first ask starts the download.
pub fn preview_model(base: &str, hash: &str) -> Option<Arc<Model>> {
    let mut cache = previews().lock().unwrap_or_else(|e| e.into_inner());
    match cache.get(hash) {
        Some(Preview::Ready(m)) => return Some(m.clone()),
        Some(Preview::Loading) => return None,
        Some(Preview::Failed(at)) if at.elapsed() < RETRY => return None,
        _ => {}
    }
    cache.insert(hash.to_owned(), Preview::Loading);
    let (base, hash) = (base.to_owned(), hash.to_owned());
    let spawned = std::thread::Builder::new().name("Native cosmetic model".into()).spawn({
        let (base, hash) = (base.clone(), hash.clone());
        move || {
            let model = fetch(&base, &hash, MAX_MODEL_BYTES).and_then(|b| Model::parse(&b).ok());
            let state = model.map_or_else(|| Preview::Failed(Instant::now()), |m| Preview::Ready(Arc::new(m)));
            previews().lock().unwrap_or_else(|e| e.into_inner()).insert(hash, state);
        }
    });
    if spawned.is_err() {
        cache.insert(hash, Preview::Failed(Instant::now()));
    }
    None
}
