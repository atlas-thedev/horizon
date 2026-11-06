//! Native Client skins and capes for the Native edition of the engine: the
//! Rust port of native-mod's `SkinDirectory` and `SkinSync`
//! (`xyz.nativelaunch.core`), so every Native player's skin and cape shows
//! here as it does in the Java mod.
//!
//! One snapshot of `/v1/skins/directory`, then the `/v1/skins/stream`
//! Server-Sent Events, on a daemon thread that reconnects with back-off. The
//! game reads the directory from the render thread through [`apply`], which
//! only takes a read lock: nothing here ever waits on the network.
//!
//! `tools/native/native_skins.py` copies this file to
//! `crates/horizon-client/src/native_skins.rs` and calls [`apply`] from
//! `update_remote_skins`, where every player's textures are chosen.

use std::collections::HashMap;
use std::io::{BufRead, Read};
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, Instant};

use crate::skins::Textures;

const DEFAULT_API: &str = "https://api.playnative.net";
/// `SkinEntry.MAX_COSMETICS`.
pub const MAX_COSMETICS: usize = 8;
/// `Http.MAX_JSON_BYTES`: the directory is far below this.
const MAX_JSON: u64 = 16 << 20;
/// A stream that stayed up this long counts as healthy: only then is the
/// retry delay reset (`SkinSync.HEALTHY_STREAM_NANOS`).
const HEALTHY_STREAM: Duration = Duration::from_secs(30);
const SESSION_LIMIT: u64 = 16 * 1024;

/// An animated cape: frames stacked top to bottom in one strip.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Animation {
    /// The strip's texture hash (in an [`Entry`]) or its full URL (in a [`Look`]).
    pub strip: String,
    pub frames: u32,
    pub fps: u32,
}

impl Animation {
    /// `SkinEntry`'s rule: a strip, more than one frame and a speed, or nothing.
    /// Frames are capped at 256 and the speed at 60.
    fn new(strip: Option<String>, frames: i64, fps: i64) -> Option<Self> {
        let strip = strip?;
        (frames > 1 && fps > 0).then(|| Self {
            strip,
            frames: frames.min(256) as u32,
            fps: fps.min(60) as u32,
        })
    }
}

/// One worn 3D cosmetic (`CosmeticRef`): the store item and its model (NCM
/// JSON) and texture (PNG) hashes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CosmeticRef {
    pub id: String,
    pub model: String,
    pub texture: String,
    /// Store slot (`hats`, `glasses`, `back`, `shoes`), if the server said.
    pub slot: Option<String>,
    /// The hand the player picked: 1 left, 2 right, 0 the item's default.
    pub side: u8,
}

impl CosmeticRef {
    /// Wings, jetpacks and backpacks take the cape's place.
    pub fn is_back_item(&self) -> bool {
        self.slot.as_deref() == Some("back")
    }
}

/// The premium block (`"p"`): the Native name is also the linked premium
/// name, so a real premium session keeps its Mojang skin and only wears this cape.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Premium {
    pub cape: Option<String>,
    pub animation: Option<Animation>,
}

/// One row of the skin directory (`SkinEntry`): hashes, not URLs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub slim: bool,
    pub skin: Option<String>,
    /// Always the first frame when the cape is animated.
    pub cape: Option<String>,
    /// Dashless lowercase Minecraft UUID of the linked premium account.
    pub minecraft_uuid: Option<String>,
    pub revision: i64,
    pub animation: Option<Animation>,
    pub premium: Option<Premium>,
    pub cosmetics: Vec<CosmeticRef>,
    /// A Native+ member.
    pub plus: bool,
}

impl Entry {
    /// A back item is worn: it replaces the cape.
    pub fn hides_cape(&self) -> bool {
        self.cosmetics.iter().any(CosmeticRef::is_back_item)
    }

    fn is_empty(&self) -> bool {
        self.skin.is_none()
            && self.cape.is_none()
            && self.premium.as_ref().is_none_or(|p| p.cape.is_none())
            && self.cosmetics.is_empty()
    }
}

/// What to show for a player (`SkinOverride`): full texture URLs. `None`
/// fields leave the game's own texture alone.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Look {
    pub skin: Option<String>,
    pub cape: Option<String>,
    pub slim: bool,
    pub animation: Option<Animation>,
    /// A back item is worn: no cape at all, not even Mojang's.
    pub hide_cape: bool,
}

/// The in-memory copy of every skin and cape published on Native.
#[derive(Debug)]
pub struct Directory {
    entries: HashMap<String, Entry>,
    texture_base: String,
    epoch: i64,
    revision: i64,
}

impl Default for Directory {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            texture_base: String::new(),
            epoch: -1,
            revision: -1,
        }
    }
}

/// The version nibble of a UUID (3 for the name-based ids offline players get).
fn uuid_version(id: u128) -> u8 {
    ((id >> 76) & 0xf) as u8
}

fn plain(id: u128) -> String {
    format!("{id:032x}")
}

fn key(name: &str) -> String {
    name.to_lowercase()
}

impl Directory {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `?epoch=…&since=…`: what the server needs to send only what changed.
    fn query(&self) -> String {
        format!("?epoch={}&since={}", self.epoch, self.revision)
    }

    /// Everything replaced (first sync, server restart, too far behind).
    fn replace_all(&mut self, epoch: i64, revision: i64, base: String, all: Vec<Entry>) {
        self.entries = all
            .into_iter()
            .filter(|e| !e.is_empty())
            .map(|e| (key(&e.name), e))
            .collect();
        self.texture_base = base;
        self.epoch = epoch;
        self.revision = revision;
    }

    /// One change: an entry with nothing in it removes the player.
    fn apply_entry(&mut self, entry: Entry) {
        let revision = entry.revision;
        if entry.is_empty() {
            self.entries.remove(&key(&entry.name));
        } else {
            self.entries.insert(key(&entry.name), entry);
        }
        self.revision = self.revision.max(revision);
    }

    /// The directory row of a player by name (any case).
    pub fn entry(&self, name: &str) -> Option<&Entry> {
        self.entries.get(&key(name))
    }

    /// The row only when it belongs to this player: premium players carry a
    /// random (version 4) UUID, so a row applies to them only when its
    /// account is linked to exactly that UUID, and nobody takes over a real
    /// player's look by registering the same name. Offline players (version
    /// 3, name-based) and the signed-in account itself match by name.
    fn owned(&self, name: &str, id: Option<u128>, self_name: Option<&str>) -> Option<&Entry> {
        let entry = self.entry(name)?;
        let is_self = self_name.is_some_and(|mine| key(mine) == key(name));
        match id {
            Some(id) if uuid_version(id) != 3 && !is_self => {
                (entry.minecraft_uuid.as_deref() == Some(plain(id).as_str())).then_some(entry)
            }
            _ => Some(entry),
        }
    }

    /// `SkinDirectory.find`. `premium_session`: the game already holds
    /// Mojang-signed textures for this player. When the Native name is also
    /// the linked premium name, such a player keeps the Mojang skin and only
    /// wears the cape picked for the premium name.
    pub fn find(
        &self,
        name: &str,
        id: Option<u128>,
        premium_session: bool,
        self_name: Option<&str>,
    ) -> Option<Look> {
        if name.is_empty() || self.texture_base.is_empty() {
            return None;
        }
        let entry = self.owned(name, id, self_name)?;
        let url = |hash: &Option<String>| hash.as_ref().map(|h| format!("{}{h}", self.texture_base));
        let animated = |a: &Option<Animation>| {
            a.as_ref().map(|a| Animation {
                strip: format!("{}{}", self.texture_base, a.strip),
                ..a.clone()
            })
        };
        let hide_cape = entry.hides_cape();
        if premium_session
            && let Some(premium) = &entry.premium
            && id.is_some_and(|id| uuid_version(id) != 3)
        {
            if hide_cape {
                return Some(Look {
                    hide_cape: true,
                    ..Look::default()
                });
            }
            premium.cape.as_ref()?;
            return Some(Look {
                cape: url(&premium.cape),
                animation: animated(&premium.animation),
                ..Look::default()
            });
        }
        Some(Look {
            skin: url(&entry.skin),
            cape: url(&entry.cape),
            slim: entry.slim,
            animation: animated(&entry.animation),
            hide_cape,
        })
    }

    /// `SkinDirectory.plus`: the player is a Native+ member (same identity rule).
    pub fn plus(&self, name: &str, id: Option<u128>, self_name: Option<&str>) -> bool {
        self.owned(name, id, self_name).is_some_and(|e| e.plus)
    }

    /// `SkinDirectory.has`: the player plays from Native.
    pub fn has(&self, name: &str, id: Option<u128>, self_name: Option<&str>) -> bool {
        self.owned(name, id, self_name).is_some()
    }

    /// `SkinDirectory.cosmetics`: the 3D cosmetics a player wears.
    pub fn cosmetics(&self, name: &str, id: Option<u128>, self_name: Option<&str>) -> &[CosmeticRef] {
        self.owned(name, id, self_name)
            .map(|e| e.cosmetics.as_slice())
            .unwrap_or(&[])
    }
}

/// A texture or model hash the directory may name: a URL path segment, never
/// a path of its own.
fn valid_hash(hash: &str) -> bool {
    (1..=128).contains(&hash.len())
        && hash
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn string(o: &serde_json::Value, key: &str) -> Option<String> {
    o.get(key)?.as_str().map(str::to_owned)
}

fn hash(o: &serde_json::Value, key: &str) -> Option<String> {
    string(o, key).filter(|h| valid_hash(h))
}

fn number(o: &serde_json::Value, key: &str, fallback: i64) -> i64 {
    o.get(key).and_then(serde_json::Value::as_i64).unwrap_or(fallback)
}

/// A 1 / true flag of the row (absent = false).
fn flag(o: &serde_json::Value, key: &str) -> bool {
    match o.get(key) {
        Some(serde_json::Value::Bool(b)) => *b,
        Some(v) => v.as_i64().is_some_and(|n| n != 0),
        None => false,
    }
}

/// `"a": {"h": strip hash, "f": frames, "p": fps}`. A malformed animation
/// never costs the player their skin or cape.
fn animation(o: &serde_json::Value) -> Option<Animation> {
    let a = o.get("a").filter(|a| a.is_object())?;
    Animation::new(hash(a, "h"), number(a, "f", 0), number(a, "p", 0))
}

/// `"k": [{"i": item id, "s": slot, "m": model hash, "x": texture hash, "h": hand}]`.
fn cosmetics(o: &serde_json::Value) -> Vec<CosmeticRef> {
    let is_sha256 =
        |h: &str| h.len() == 64 && h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    let Some(list) = o.get("k").and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|k| {
            let model = string(k, "m").filter(|h| is_sha256(h))?;
            let texture = string(k, "x").filter(|h| is_sha256(h))?;
            let side = match string(k, "h").as_deref() {
                Some("l") => 1,
                Some("r") => 2,
                _ => 0,
            };
            Some(CosmeticRef {
                id: string(k, "i").unwrap_or_else(|| model[..12].to_owned()),
                model,
                texture,
                slot: string(k, "s"),
                side,
            })
        })
        .take(MAX_COSMETICS)
        .collect()
}

/// `SkinSync.entry`: one row of the directory or of a `skin` event.
pub fn parse_entry(o: &serde_json::Value) -> Option<Entry> {
    let name = string(o, "n").filter(|n| !n.is_empty())?;
    let cape = hash(o, "c");
    let premium = o.get("p").filter(|p| p.is_object()).map(|p| {
        let cape = hash(p, "c");
        Premium {
            animation: cape.as_ref().and_then(|_| animation(p)),
            cape,
        }
    });
    Some(Entry {
        name,
        slim: string(o, "m").as_deref() == Some("slim"),
        skin: hash(o, "s"),
        animation: cape.as_ref().and_then(|_| animation(o)),
        cape,
        minecraft_uuid: string(o, "u").map(|u| u.replace('-', "").to_ascii_lowercase()),
        revision: number(o, "r", 0),
        premium,
        cosmetics: cosmetics(o),
        plus: flag(o, "x"),
    })
}

/// `SkinSync.applySnapshot`: a full directory replaces everything, a partial
/// one (`"full": false`) applies only the rows that changed.
pub fn apply_snapshot(directory: &mut Directory, body: &str) -> Result<(), String> {
    let root: serde_json::Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let base = string(&root, "textureBase").unwrap_or_default();
    let full = root.get("full").and_then(serde_json::Value::as_bool).unwrap_or(true);
    let entries: Vec<Entry> = root
        .get("entries")
        .and_then(serde_json::Value::as_array)
        .map(|list| list.iter().filter_map(parse_entry).collect())
        .unwrap_or_default();
    let revision = number(&root, "rev", 0);
    if full {
        directory.replace_all(number(&root, "epoch", -1), revision, base, entries);
    } else {
        if !base.is_empty() {
            directory.texture_base = base;
        }
        for entry in entries {
            directory.apply_entry(entry);
        }
        directory.revision = directory.revision.max(revision);
    }
    Ok(())
}

static DIRECTORY: OnceLock<RwLock<Directory>> = OnceLock::new();

/// The shared directory, filled by the sync thread (started on first use).
pub fn directory() -> std::sync::RwLockReadGuard<'static, Directory> {
    start();
    shared().read().unwrap_or_else(|e| e.into_inner())
}

fn shared() -> &'static RwLock<Directory> {
    DIRECTORY.get_or_init(|| RwLock::new(Directory::default()))
}

fn write() -> std::sync::RwLockWriteGuard<'static, Directory> {
    shared().write().unwrap_or_else(|e| e.into_inner())
}

/// The sync thread, once.
fn start() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        let spawned = std::thread::Builder::new()
            .name("Native skins".into())
            .spawn(|| run(&api()));
        if let Err(e) = spawned {
            log::warn!("Native: skin sync unavailable: {e}");
        }
    });
}

/// Only Native over https (or loopback http for development) is asked for
/// skins, so a tampered session file cannot point the game elsewhere.
fn allowed(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    let parsed = lower
        .strip_prefix("http://")
        .map(|rest| (rest, true))
        .or_else(|| lower.strip_prefix("https://").map(|rest| (rest, false)));
    let Some((rest, plain_http)) = parsed else {
        return false;
    };
    let authority = rest.split('/').next().unwrap_or("");
    if authority.contains('@') {
        return false;
    }
    let host = authority.split(':').next().unwrap_or("");
    let loopback = host == "localhost" || host == "127.0.0.1";
    if plain_http {
        return loopback;
    }
    loopback || host == "playnative.net" || host.ends_with(".playnative.net")
}

/// The API the launcher's hand-off names (`<game dir>/.native/session.json`),
/// else Native's own. A guest without a hand-off still sees everyone's skins.
fn api() -> String {
    let dir = std::env::var_os("NATIVE_GAME_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::current_dir().ok());
    let from_session = dir
        .map(|d| d.join(".native").join("session.json"))
        .filter(|p| std::fs::metadata(p).is_ok_and(|m| m.len() <= SESSION_LIMIT))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|v| string(&v, "api"))
        .map(|api| api.trim().trim_end_matches('/').to_owned())
        .filter(|api| allowed(api));
    from_session.unwrap_or_else(|| DEFAULT_API.to_owned())
}

fn agent(read: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(8))
        .timeout_read(read)
        .user_agent("NativeClient (Rust engine)")
        .build()
}

fn query() -> String {
    shared().read().unwrap_or_else(|e| e.into_inner()).query()
}

/// `SkinSync.fetchDirectory`.
fn fetch_directory(api: &str) -> Result<(), String> {
    let response = agent(Duration::from_secs(15))
        .get(&format!("{api}/v1/skins/directory{}", query()))
        .set("Accept", "application/json")
        .call()
        .map_err(|e| e.to_string())?;
    let mut body = String::new();
    response
        .into_reader()
        .take(MAX_JSON)
        .read_to_string(&mut body)
        .map_err(|e| e.to_string())?;
    let mut directory = write();
    apply_snapshot(&mut directory, &body)?;
    log::info!("Native: skin directory ready, {} players with custom looks", directory.len());
    Ok(())
}

/// `SkinSync.stream`: Server-Sent Events until the connection ends.
fn stream(api: &str) -> Result<(), String> {
    let response = agent(Duration::from_secs(75))
        .get(&format!("{api}/v1/skins/stream{}", query()))
        .set("Accept", "text/event-stream")
        .call()
        .map_err(|e| e.to_string())?;
    let reader = std::io::BufReader::new(response.into_reader());
    let mut event: Option<String> = None;
    let mut data = String::new();
    for line in reader.lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.is_empty() {
            if let Some(name) = event.take()
                && !data.is_empty()
            {
                handle(api, &name, &data)?;
            }
            data.clear();
        } else if let Some(rest) = line.strip_prefix("event:") {
            event = Some(rest.trim().to_owned());
        } else if let Some(rest) = line.strip_prefix("data:") {
            if data.len() + rest.len() > MAX_JSON as usize {
                return Err("event too large".into());
            }
            data.push_str(rest.trim());
        }
        // ": ping" comments and id: lines need no handling.
    }
    Ok(())
}

/// `SkinSync.handle`: `hello` may ask for a resync, `skin` is one change.
fn handle(api: &str, event: &str, data: &str) -> Result<(), String> {
    let payload: serde_json::Value = serde_json::from_str(data).map_err(|e| e.to_string())?;
    match event {
        "hello" if payload.get("resync").and_then(serde_json::Value::as_bool) == Some(true) => {
            fetch_directory(api)
        }
        "skin" => {
            if let Some(entry) = parse_entry(&payload) {
                log::debug!("Native: skin update for {}", entry.name);
                write().apply_entry(entry);
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// `SkinSync.loop`: snapshot, stream, back off (2 s doubling to 60 s; a
/// stream that dies at once, such as HTTP 429, never retries every 2 s).
fn run(api: &str) {
    let mut backoff = Duration::from_secs(2);
    loop {
        let mut opened: Option<Instant> = None;
        let result = fetch_directory(api).and_then(|()| {
            opened = Some(Instant::now());
            stream(api)
        });
        if let Err(e) = result {
            log::warn!("Native: skin sync interrupted ({e}); retrying");
        }
        if opened.is_some_and(|at| at.elapsed() >= HEALTHY_STREAM) {
            backoff = Duration::from_secs(2);
        }
        std::thread::sleep(backoff);
        backoff = (backoff * 2).min(Duration::from_secs(60));
    }
}

/// Puts each player's Native look into the textures the game is about to
/// resolve (the Java mod's authlib hook). `players` are the known players'
/// UUIDs and profile names; a player with a Native look but no textures of
/// their own (an offline-mode server) gets an entry. Native textures are not
/// signed by Mojang, so a changed entry is marked trusted, as the Java mod
/// marks them `SIGNED`: vanilla would otherwise show other players Steve/Alex.
///
/// Returns the players whose Native cape is animated (full strip URLs), for
/// [`animated_cape`].
pub fn apply(
    entries: &mut Vec<(u128, Textures)>,
    players: impl IntoIterator<Item = (u128, String)>,
) -> Vec<(u128, Animation)> {
    let self_name = crate::discord_presence::native_account();
    apply_to(&directory(), self_name.as_deref(), entries, players)
}

fn apply_to(
    directory: &Directory,
    self_name: Option<&str>,
    entries: &mut Vec<(u128, Textures)>,
    players: impl IntoIterator<Item = (u128, String)>,
) -> Vec<(u128, Animation)> {
    let mut animated = Vec::new();
    if directory.is_empty() {
        return animated;
    }
    for (uuid, name) in players {
        let slot = entries.iter().position(|(u, _)| *u == uuid);
        let premium_session = slot.is_some_and(|i| entries[i].1.signed && entries[i].1.skin.is_some());
        let Some(look) = directory.find(&name, Some(uuid), premium_session, self_name) else {
            continue;
        };
        if look.skin.is_none() && look.cape.is_none() && !look.hide_cape {
            continue;
        }
        let i = slot.unwrap_or_else(|| {
            entries.push((uuid, Textures::default()));
            entries.len() - 1
        });
        let textures = &mut entries[i].1;
        if let Some(skin) = look.skin {
            textures.skin = Some(skin);
            textures.slim = look.slim;
        }
        if look.hide_cape {
            textures.cape = None;
        } else if let Some(cape) = look.cape {
            textures.cape = Some(cape);
            if let Some(animation) = look.animation {
                animated.push((uuid, animation));
            }
        }
        textures.signed = true;
    }
    animated
}

/// An animated cape's frames, each laid out as `skins::process_cape` gives a
/// still cape (64x64 RGBA, the picture in the top half).
enum Strip {
    Loading,
    Ready(Vec<Vec<u8>>),
    Failed(Instant),
}

const STRIP_RETRY: Duration = Duration::from_secs(120);
const MAX_STRIPS: usize = 64;

struct Strips {
    strips: HashMap<String, Strip>,
    /// The frame each cape slot holds now, so a frame is uploaded only when it changes.
    shown: HashMap<String, usize>,
}

fn strips() -> std::sync::MutexGuard<'static, Strips> {
    static STRIPS: OnceLock<std::sync::Mutex<Strips>> = OnceLock::new();
    STRIPS
        .get_or_init(|| {
            std::sync::Mutex::new(Strips {
                strips: HashMap::new(),
                shown: HashMap::new(),
            })
        })
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Splits a strip of `frames` pictures stacked top to bottom into capes.
pub fn split_strip(width: u32, height: u32, rgba: &[u8], frames: u32) -> Option<Vec<Vec<u8>>> {
    let (w, frames) = (width as usize, frames.max(1) as usize);
    let frame_h = height as usize / frames;
    if w == 0 || frame_h == 0 || rgba.len() < w * height as usize * 4 {
        return None;
    }
    (0..frames)
        .map(|i| {
            let start = i * frame_h * w * 4;
            let slice = &rgba[start..start + frame_h * w * 4];
            crate::skins::process_cape(width, frame_h as u32, slice)
        })
        .collect()
}

fn fetch_strip(url: &str, frames: u32) -> Option<Vec<Vec<u8>>> {
    let response = agent(Duration::from_secs(20)).get(url).call().ok()?;
    let mut body = Vec::new();
    response.into_reader().take(MAX_JSON).read_to_end(&mut body).ok()?;
    let (w, h, rgba) = crate::hud::decode_bytes(&body).ok()?;
    split_strip(w, h, &rgba, frames)
}

/// The frame an animation shows `elapsed` after the game started.
pub fn frame_at(animation: &Animation, elapsed: Duration) -> usize {
    let frames = animation.frames.max(1) as u128;
    (elapsed.as_millis() * animation.fps as u128 / 1000 % frames) as usize
}

fn started() -> Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    *START.get_or_init(Instant::now)
}

/// The atlas location of an animated cape and, when the slot needs new
/// pixels (a new frame, or `present` says the slot was lost), the frame to
/// upload with `store_skin`. `None` until the strip is downloaded: the cape's
/// first frame (its still cape) shows meanwhile.
pub fn animated_cape(
    animation: &Animation,
    present: impl Fn(&str) -> bool,
) -> Option<(String, Option<Vec<u8>>)> {
    let location = format!(
        "capes/native-{}",
        crate::skins::texture_hash(&animation.strip).unwrap_or_default()
    );
    let mut strips = strips();
    let ready = match strips.strips.get(&animation.strip) {
        Some(Strip::Ready(frames)) => Some(frames.len()),
        Some(Strip::Loading) => None,
        Some(Strip::Failed(at)) if at.elapsed() < STRIP_RETRY => None,
        _ => {
            if strips.strips.len() >= MAX_STRIPS {
                strips.strips.retain(|_, s| !matches!(s, Strip::Failed(_)));
            }
            if strips.strips.len() < MAX_STRIPS {
                strips.strips.insert(animation.strip.clone(), Strip::Loading);
                let (url, frames) = (animation.strip.clone(), animation.frames);
                let spawned = std::thread::Builder::new()
                    .name("Native cape".into())
                    .spawn(move || {
                        let state = match fetch_strip(&url, frames) {
                            Some(frames) if !frames.is_empty() => Strip::Ready(frames),
                            _ => Strip::Failed(Instant::now()),
                        };
                        strips().strips.insert(url, state);
                    });
                if spawned.is_err() {
                    strips.strips.insert(animation.strip.clone(), Strip::Failed(Instant::now()));
                }
            }
            None
        }
    }?;
    let frame = frame_at(animation, started().elapsed()) % ready;
    let fresh = strips.shown.get(&location) != Some(&frame) || !present(&location);
    let upload = fresh
        .then(|| match strips.strips.get(&animation.strip) {
            Some(Strip::Ready(frames)) => frames.get(frame).cloned(),
            _ => None,
        })
        .flatten();
    if upload.is_some() {
        strips.shown.insert(location.clone(), frame);
    }
    Some((location, upload))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "https://api.playnative.net/csl/textures/";
    // A version 4 (random, premium) and a version 3 (name-based, offline) UUID.
    const PREMIUM: u128 = 0x069a79f4_44e9_4726_a5be_fca90e38aaf5;
    const OFFLINE: u128 = 0x7a6d2b1c_9f3e_3a41_8c2d_5e6f7a8b9c0d;

    fn sha(c: char) -> String {
        std::iter::repeat_n(c, 64).collect()
    }

    fn directory(rows: serde_json::Value) -> Directory {
        let mut d = Directory::default();
        let body = serde_json::json!({"epoch": 7, "rev": 3, "textureBase": BASE, "entries": rows});
        apply_snapshot(&mut d, &body.to_string()).unwrap();
        d
    }

    #[test]
    fn offline_players_match_by_name_and_get_full_urls() {
        let d = directory(serde_json::json!([{"n": "Alice", "m": "slim", "s": sha('a'), "c": sha('b'), "r": 3}]));
        let look = d.find("alice", Some(OFFLINE), false, None).unwrap();
        assert_eq!(look.skin, Some(format!("{BASE}{}", sha('a'))));
        assert_eq!(look.cape, Some(format!("{BASE}{}", sha('b'))));
        assert!(look.slim);
    }

    #[test]
    fn premium_uuid_needs_the_linked_account() {
        let linked = format!("{PREMIUM:032x}");
        let d = directory(serde_json::json!([
            {"n": "Notch", "s": sha('a')},
            {"n": "Linked", "s": sha('c'), "u": linked},
        ]));
        assert!(d.find("Notch", Some(PREMIUM), false, None).is_none());
        assert!(d.find("Linked", Some(PREMIUM), false, None).is_some());
        // The signed-in account itself always matches by name.
        assert!(d.find("Notch", Some(PREMIUM), false, Some("notch")).is_some());
    }

    #[test]
    fn premium_session_keeps_the_mojang_skin_and_wears_the_premium_cape() {
        let linked = format!("{PREMIUM:032x}");
        let d = directory(serde_json::json!([
            {"n": "Steve", "s": sha('a'), "c": sha('b'), "u": linked, "p": {"c": sha('d')}}
        ]));
        let look = d.find("Steve", Some(PREMIUM), true, None).unwrap();
        assert_eq!(look.skin, None);
        assert_eq!(look.cape, Some(format!("{BASE}{}", sha('d'))));
    }

    #[test]
    fn a_back_item_hides_the_cape() {
        let d = directory(serde_json::json!([{
            "n": "Wings", "s": sha('a'), "c": sha('b'),
            "k": [{"i": "angel", "s": "back", "m": sha('e'), "x": sha('f')}]
        }]));
        let mut entries = vec![(OFFLINE, Textures {
            cape: Some("https://textures.minecraft.net/texture/x".into()),
            ..Textures::default()
        })];
        apply_to(&d, None, &mut entries, [(OFFLINE, "Wings".to_owned())]);
        assert_eq!(entries[0].1.cape, None);
        assert_eq!(entries[0].1.skin, Some(format!("{BASE}{}", sha('a'))));
    }

    #[test]
    fn apply_gives_unlisted_players_their_look_and_marks_it_trusted() {
        let d = directory(serde_json::json!([{"n": "Bob", "m": "slim", "s": sha('a'), "c": sha('b')}]));
        let mut entries = Vec::new();
        apply_to(&d, None, &mut entries, [(OFFLINE, "Bob".to_owned()), (PREMIUM, "Bob".to_owned())]);
        // The offline Bob gets the look; the premium UUID is not the linked account.
        assert_eq!(entries.len(), 1);
        let (uuid, t) = &entries[0];
        assert_eq!(*uuid, OFFLINE);
        assert!(t.signed && t.slim);
        assert_eq!(t.cape, Some(format!("{BASE}{}", sha('b'))));
    }

    #[test]
    fn apply_leaves_players_without_a_native_look_alone() {
        let d = directory(serde_json::json!([{"n": "Bob", "s": sha('a')}]));
        let theirs = Textures {
            skin: Some("https://textures.minecraft.net/texture/own".into()),
            ..Textures::default()
        };
        let mut entries = vec![(OFFLINE, theirs.clone())];
        apply_to(&d, None, &mut entries, [(OFFLINE, "Carol".to_owned())]);
        assert_eq!(entries, vec![(OFFLINE, theirs)]);
    }

    #[test]
    fn animated_capes_need_frames_and_speed() {
        let d = directory(serde_json::json!([
            {"n": "Anim", "c": sha('b'), "a": {"h": sha('c'), "f": 400, "p": 90}},
            {"n": "Still", "c": sha('b'), "a": {"h": sha('c'), "f": 1, "p": 10}},
        ]));
        let anim = d.find("Anim", None, false, None).unwrap().animation.unwrap();
        assert_eq!((anim.frames, anim.fps), (256, 60));
        assert_eq!(anim.strip, format!("{BASE}{}", sha('c')));
        assert!(d.find("Still", None, false, None).unwrap().animation.is_none());
    }

    #[test]
    fn partial_snapshot_and_empty_rows() {
        let mut d = directory(serde_json::json!([{"n": "Gone", "s": sha('a')}, {"n": "Kept", "s": sha('a')}]));
        let body = serde_json::json!({"full": false, "rev": 9, "entries": [{"n": "Gone", "r": 9}]});
        apply_snapshot(&mut d, &body.to_string()).unwrap();
        assert!(d.entry("gone").is_none());
        assert!(d.entry("kept").is_some());
        assert_eq!(d.query(), "?epoch=7&since=9");
    }

    #[test]
    fn hashes_that_are_not_url_segments_are_dropped() {
        let d = directory(serde_json::json!([{"n": "Bad", "s": "../../etc", "c": sha('b')}]));
        let look = d.find("Bad", None, false, None).unwrap();
        assert_eq!(look.skin, None);
        assert!(look.cape.is_some());
    }

    #[test]
    fn only_native_hosts_are_trusted() {
        assert!(allowed("https://api.playnative.net"));
        assert!(allowed("http://localhost:8080"));
        assert!(!allowed("http://api.playnative.net"));
        assert!(!allowed("https://playnative.net.evil.com"));
        assert!(!allowed("https://user@api.playnative.net"));
    }

    #[test]
    fn a_strip_splits_into_capes_and_frames_follow_the_clock() {
        // Two 64x32 frames: the first red, the second blue.
        let mut rgba = Vec::new();
        for frame in 0..2 {
            for _ in 0..64 * 32 {
                rgba.extend_from_slice(if frame == 0 { &[255, 0, 0, 255] } else { &[0, 0, 255, 255] });
            }
        }
        let frames = split_strip(64, 64, &rgba, 2).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].len(), 64 * 64 * 4);
        assert_eq!(&frames[0][..4], &[255, 0, 0, 255]);
        assert_eq!(&frames[1][..4], &[0, 0, 255, 255]);
        let animation = Animation { strip: String::new(), frames: 4, fps: 10 };
        assert_eq!(frame_at(&animation, Duration::from_millis(0)), 0);
        assert_eq!(frame_at(&animation, Duration::from_millis(250)), 2);
        assert_eq!(frame_at(&animation, Duration::from_millis(450)), 0);
    }

    #[test]
    fn plus_follows_the_identity_rule() {
        let d = directory(serde_json::json!([{"n": "Gold", "s": sha('a'), "x": 1}]));
        assert!(d.plus("gold", Some(OFFLINE), None));
        assert!(!d.plus("gold", Some(PREMIUM), None));
    }
}
