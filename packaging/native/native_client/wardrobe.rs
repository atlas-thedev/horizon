//! `Wardrobe`: the player's Native locker inside the game: the store
//! catalogue, what they own and wear, and equipping. Every request runs on a
//! background thread; the menu only reads the snapshot.
#![allow(dead_code)]

use super::net::{self, HttpError};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// (no glasses: the store stopped selling them on 2026-11-14)
pub const SLOTS: [&str; 7] = ["capes", "hats", "back", "shoes", "hand", "auras", "balloons"];
pub const SLOT_NAMES: [&str; 7] = ["Capes", "Hats", "Backs", "Shoes", "Hand Items", "Auras", "Balloons"];
const BACK: &str = "back";
const CAPES: &str = "capes";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Item {
    pub id: String,
    pub name: Option<String>,
    pub kind: Option<String>,
    pub slot: Option<String>,
    pub still_url: Option<String>,
    pub model_url: Option<String>,
    pub texture_url: Option<String>,
    pub dye_url: Option<String>,
    /// Animated capes: frames stacked top to bottom in one strip.
    pub strip_url: Option<String>,
    pub frames: u32,
    pub fps: f32,
    /// Made with a Native partner (e.g. Aurex): its name, PNG logo and colour.
    pub partner_name: Option<String>,
    pub partner_logo: Option<String>,
    pub partner_accent: u32,
    /// Store metadata for "More in the Native Store" try-on & dye swatches.
    pub paid: bool,
    pub exclusive: bool,
    pub dyeable: bool,
    pub price: f32,
    pub sale_price: f32,
    pub dye_default: Option<String>,
    pub dye_colors: Vec<String>,
}

impl Item {
    pub fn animated(&self) -> bool {
        self.strip_url.is_some() && self.frames > 1 && self.fps > 0.0
    }

    /// The strip frame showing at `seconds`.
    pub fn frame(&self, seconds: f64) -> usize {
        ((seconds * f64::from(self.fps)) as usize) % (self.frames.max(1) as usize)
    }

    pub fn is_cape(&self) -> bool {
        self.kind.as_deref() != Some("cosmetic")
    }

    /// Slot key: "capes" for capes.
    pub fn group(&self) -> &str {
        if self.is_cape() { CAPES } else { self.slot.as_deref().unwrap_or("hats") }
    }

    pub fn model_hash(&self) -> Option<String> {
        hash_of(self.model_url.as_deref())
    }

    pub fn texture_hash(&self) -> Option<String> {
        hash_of(self.texture_url.as_deref())
    }

    pub fn display(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }

    pub fn is_free_claimable(&self) -> bool {
        !self.paid && !self.exclusive
    }

    pub fn price_label(&self) -> String {
        if self.exclusive {
            "Limited".to_owned()
        } else if !self.paid {
            "Free".to_owned()
        } else {
            let p = if self.sale_price > 0.0 { self.sale_price } else { self.price };
            format!("${p:.2}")
        }
    }
}

/// A saved skin from the player's Native profile history (`/v1/profiles/<name>`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SavedSkin {
    pub id: String,
    pub name: String,
    pub url: String,
    pub hash: Option<String>,
    pub model: String,
    pub last_seen: i64,
}

impl SavedSkin {
    pub fn is_slim(&self) -> bool {
        self.model.eq_ignore_ascii_case("slim")
    }
}

/// The 64-hex file name at the end of a texture URL.
pub fn hash_of(url: Option<&str>) -> Option<String> {
    let url = url?;
    let path = url.split('?').next().unwrap_or(url);
    let name = path.rsplit('/').next()?;
    (name.len() == 64 && name.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))).then(|| name.to_owned())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Idle,
    Loading,
    Ready,
    Error,
    NoAccount,
}

#[derive(Clone, Debug)]
pub struct Locker {
    pub state: State,
    pub error: Option<String>,
    pub catalog: Vec<Item>,
    pub owned: HashSet<String>,
    /// Store cape worn.
    pub equipped: Option<String>,
    pub wearing: HashMap<String, String>,
    pub sides: HashMap<String, i32>,
    pub dyes: HashMap<String, String>,
    /// Item (or slot) id with a request in flight.
    pub busy: Option<String>,
    /// Saved skins from `/v1/profiles/<name>` + local disk cache.
    pub saved_skins: Vec<SavedSkin>,
    pub skins_loading: bool,
    pub active_skin_hash: Option<String>,
    pub skin_busy: Option<String>,
    pub skin_notice: Option<String>,
    skins_for_user: Option<String>,
    skins_loaded_at: Option<Instant>,
    loaded_at: Option<Instant>,
}

fn locker() -> &'static Mutex<Locker> {
    static LOCKER: std::sync::OnceLock<Mutex<Locker>> = std::sync::OnceLock::new();
    LOCKER.get_or_init(|| {
        Mutex::new(Locker {
            state: State::Idle,
            error: None,
            catalog: Vec::new(),
            owned: HashSet::new(),
            equipped: None,
            wearing: HashMap::new(),
            sides: HashMap::new(),
            dyes: HashMap::new(),
            busy: None,
            saved_skins: Vec::new(),
            skins_loading: false,
            active_skin_hash: None,
            skin_busy: None,
            skin_notice: None,
            skins_for_user: None,
            skins_loaded_at: None,
            loaded_at: None,
        })
    })
}

fn lock() -> MutexGuard<'static, Locker> {
    locker().lock().unwrap_or_else(|e| e.into_inner())
}

/// A copy of the locker for the frame to read.
pub fn snapshot() -> Locker {
    lock().clone()
}

/// Serialises the requests, so a quick second tap never races the first.
fn io_lock() -> MutexGuard<'static, ()> {
    static IO: Mutex<()> = Mutex::new(());
    IO.lock().unwrap_or_else(|e| e.into_inner())
}

/// Where the store's textures are: `{api}/csl/textures/`.
pub fn base() -> String {
    format!("{}/csl/textures/", net::api())
}

fn text(o: &Value, k: &str) -> Option<String> {
    o.get(k).and_then(Value::as_str).map(str::to_owned)
}

/// Capes and cosmetics only. The store also sells emotes (kind "emote") and bundles: they have no slot, so they landed
/// among the capes as empty cards. Emotes have their own page in the Native menu.
fn wearable(o: &Value) -> bool {
    o.is_object() && matches!(o.get("kind").and_then(Value::as_str), None | Some("cape") | Some("cosmetic"))
}

fn parse_item(o: &Value) -> Item {
    let partner = o.get("partner").filter(|p| p.is_object());
    let accent = partner
        .and_then(|p| text(p, "accent"))
        .filter(|a| a.len() == 7 && a.starts_with('#') && a[1..].bytes().all(|b| b.is_ascii_hexdigit()))
        .and_then(|a| u32::from_str_radix(&a[1..], 16).ok())
        .map_or(0xFF8B5CF6, |v| 0xFF000000 | v);
    let dye_colors = o
        .get("dyeColors")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(|hex| {
            let h = hex.trim_start_matches('#');
            (h.len() == 6 && h.bytes().all(|b| b.is_ascii_hexdigit())).then(|| format!("#{}", h.to_ascii_lowercase()))
        })
        .collect();
    Item {
        id: text(o, "id").unwrap_or_default(),
        name: text(o, "name"),
        kind: text(o, "kind"),
        slot: text(o, "slot"),
        still_url: text(o, "stillUrl"),
        model_url: text(o, "modelUrl"),
        texture_url: text(o, "textureUrl"),
        dye_url: text(o, "dyeUrl"),
        strip_url: text(o, "stripUrl"),
        frames: o.get("frames").and_then(Value::as_u64).unwrap_or(1) as u32,
        fps: o.get("fps").and_then(Value::as_f64).unwrap_or(0.0) as f32,
        partner_name: partner.and_then(|p| text(p, "name")),
        partner_logo: partner.and_then(|p| text(p, "logoPng")),
        partner_accent: accent,
        paid: o.get("paid").and_then(Value::as_bool).unwrap_or(false),
        exclusive: o.get("exclusive").and_then(Value::as_bool).unwrap_or(false),
        dyeable: o.get("dyeable").and_then(Value::as_bool).unwrap_or(false),
        price: o.get("price").and_then(Value::as_f64).unwrap_or(0.0) as f32,
        sale_price: o.get("salePrice").and_then(Value::as_f64).unwrap_or(0.0) as f32,
        dye_default: text(o, "dyeDefault"),
        dye_colors,
    }
}

fn string_map(o: &Value, key: &str) -> HashMap<String, String> {
    o.get(key)
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
        .collect()
}

/// Reads `/v1/store/me` (or an equip reply) into the locker.
fn apply(l: &mut Locker, me: &Value) {
    if let Some(hidden) = me.get("ownedHidden").and_then(Value::as_array) {
        for e in hidden.iter().filter(|e| wearable(e)) {
            let item = parse_item(e);
            if !item.id.is_empty() && !l.catalog.iter().any(|i| i.id == item.id) {
                l.catalog.push(item);
            }
        }
    }
    if let Some(owned) = me.get("owned").and_then(Value::as_array) {
        l.owned = owned
            .iter()
            .filter_map(|e| e.get("id").and_then(Value::as_str).or_else(|| e.as_str()))
            .map(str::to_owned)
            .collect();
    }
    l.equipped = text(me, "equipped");
    l.wearing = string_map(me, "wearing");
    l.sides = me
        .get("sides")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(k, v)| {
            let side = v.as_i64().map(|n| n as i32).unwrap_or_else(|| match v.as_str() {
                Some(s) if s.starts_with('l') => 1,
                Some(s) if s.starts_with('r') => 2,
                _ => 0,
            });
            (k.clone(), side)
        })
        .collect();
    l.dyes = string_map(me, "dyes");
    save_dyes(&l.dyes);
}

fn dyes_file() -> Option<std::path::PathBuf> {
    super::game_dir().map(|d| d.join(".native").join("dyes.json"))
}

fn save_dyes(dyes: &HashMap<String, String>) {
    if cfg!(test) {
        return;
    }
    let Some(file) = dyes_file() else { return };
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(file, json!(dyes).to_string());
}

/// Loads (or reloads when older than 30 s) the catalogue and the locker.
pub fn refresh(force: bool) {
    {
        let mut l = lock();
        let fresh = l.loaded_at.is_some_and(|t| t.elapsed() < Duration::from_secs(30));
        if l.state == State::Loading || (!force && l.state == State::Ready && fresh) {
            return;
        }
        if l.state != State::Ready {
            l.state = State::Loading;
        }
    }
    let _ = std::thread::Builder::new().name("Native wardrobe".into()).spawn(|| {
        let _io = io_lock();
        let api = net::api();
        let result = (|| -> Result<(), HttpError> {
            let catalog = net::get_json(&format!("{api}/v1/store/catalog"), None)?;
            let items: Vec<Item> = catalog
                .get("items")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|e| wearable(e))
                .map(parse_item)
                .collect();
            lock().catalog = items;
            let Some(ticket) = net::ticket().filter(|_| net::signed_in()) else {
                // Guest, expired or still checking: the menu shows why and retries.
                lock().state = State::NoAccount;
                return Ok(());
            };
            let me = net::get_json(&format!("{api}/v1/store/me"), Some(&ticket))?;
            let mut l = lock();
            apply(&mut l, &me);
            l.loaded_at = Some(Instant::now());
            l.error = None;
            l.state = State::Ready;
            Ok(())
        })();
        if let Err(e) = result {
            let mut l = lock();
            if e == HttpError::Unauthorized {
                if let Some(t) = net::ticket() {
                    net::rejected(&t);
                }
                l.state = State::NoAccount;
                return;
            }
            net::unreachable();
            l.error = Some("Could not load your locker.".to_owned());
            if l.state != State::Ready {
                l.state = State::Error;
            }
        }
    });
}

/// Owned items in one slot group, in catalogue order.
pub fn owned_in(l: &Locker, group: &str) -> Vec<Item> {
    l.catalog.iter().filter(|i| l.owned.contains(&i.id) && i.group() == group).cloned().collect()
}

/// Unowned store items in one slot group ("More in the Native Store").
pub fn not_owned_in(l: &Locker, group: &str) -> Vec<Item> {
    l.catalog.iter().filter(|i| !l.owned.contains(&i.id) && i.group() == group).cloned().collect()
}

/// Total owned wearable items across all slots.
pub fn owned_total(l: &Locker) -> usize {
    l.catalog.iter().filter(|i| l.owned.contains(&i.id)).count()
}

pub fn item(l: &Locker, id: Option<&str>) -> Option<Item> {
    let id = id?;
    l.catalog.iter().find(|i| i.id == id).cloned()
}

pub fn is_worn(l: &Locker, i: &Item) -> bool {
    if i.is_cape() {
        l.equipped.as_deref() == Some(i.id.as_str())
    } else {
        l.wearing.get(i.group()).is_some_and(|w| *w == i.id)
    }
}

/// Takes off whatever is worn in a slot group ("capes", "hats", ...).
pub fn unequip_slot(group: &str) {
    let worn = {
        let l = lock();
        if l.busy.is_some() {
            return;
        }
        if group == CAPES { item(&l, l.equipped.as_deref()) } else { item(&l, l.wearing.get(group).map(String::as_str)) }
    };
    if let Some(w) = worn {
        toggle(&w);
    }
}

/// Claims a free unowned store item and puts it on straight away.
pub fn claim_and_wear(i: &Item) {
    if !i.is_free_claimable() {
        return;
    }
    {
        let mut l = lock();
        if l.busy.is_some() {
            return;
        }
        l.busy = Some(i.id.clone());
    }
    let id = i.id.clone();
    let _ = std::thread::Builder::new().name("Native claim".into()).spawn(move || {
        let _io = io_lock();
        let api = net::api();
        let ticket = net::ticket();
        let res = (|| -> Result<(), HttpError> {
            net::post_json(&format!("{api}/v1/store/claim"), ticket.as_deref(), &json!({ "itemId": id }))?;
            lock().owned.insert(id.clone());
            let reply = net::post_json(&format!("{api}/v1/store/equip"), ticket.as_deref(), &json!({ "itemId": id }))?;
            let mut l = lock();
            apply(&mut l, &reply);
            l.error = None;
            Ok(())
        })();
        if let Err(e) = res {
            if e == HttpError::Unauthorized {
                if let Some(t) = &ticket {
                    net::rejected(t);
                }
                lock().state = State::NoAccount;
            } else {
                lock().error = Some("Could not add that item. Try again.".to_owned());
            }
        }
        lock().busy = None;
    });
}

/// Picks the side (`1` = left, `2` = right) for a sided cosmetic slot ("hand" or "balloons").
pub fn set_side(slot: &str, side: i32) {
    {
        let mut l = lock();
        if l.busy.is_some() {
            return;
        }
        l.sides.insert(slot.to_owned(), side);
        l.busy = Some(format!("side:{slot}"));
    }
    let body = json!({ "slot": slot, "itemId": Value::Null, "side": if side == 1 { "left" } else { "right" } });
    post(body, None);
}

/// Saves a dye hex (`#rrggbb`) or `None` (default) for a dyeable cosmetic.
pub fn set_dye(i: &Item, color: Option<&str>) {
    if !i.dyeable {
        return;
    }
    let next_hex = color
        .filter(|c| !i.dye_default.as_deref().is_some_and(|d| d.eq_ignore_ascii_case(c)))
        .map(str::to_ascii_lowercase);
    {
        let mut l = lock();
        if l.busy.is_some() {
            return;
        }
        if let Some(ref hex) = next_hex {
            l.dyes.insert(i.id.clone(), hex.clone());
        } else {
            l.dyes.remove(&i.id);
        }
        save_dyes(&l.dyes);
        l.busy = Some(format!("dye:{}", i.id));
    }
    let id = i.id.clone();
    let _ = std::thread::Builder::new().name("Native dye".into()).spawn(move || {
        let _io = io_lock();
        let api = net::api();
        let ticket = net::ticket();
        let body = json!({ "itemId": id, "color": next_hex });
        match net::post_json(&format!("{api}/v1/store/dye"), ticket.as_deref(), &body) {
            Ok(reply) => {
                let mut l = lock();
                if reply.get("dyes").is_some_and(Value::is_object) {
                    l.dyes = string_map(&reply, "dyes");
                    save_dyes(&l.dyes);
                }
                l.error = None;
            }
            Err(_) => {
                lock().error = Some("Could not save colour.".to_owned());
            }
        }
        lock().busy = None;
    });
}

// ── Saved skins (/v1/profiles/<name> + local cache) ──────────────────────

fn skins_cache_file(user: &str) -> Option<std::path::PathBuf> {
    if cfg!(test) {
        return None;
    }
    let safe: String = user.to_ascii_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
    super::game_dir().map(|d| d.join(".native").join(format!("locker-skins-{safe}.json")))
}

fn parse_saved_skins(container: &Value) -> Vec<SavedSkin> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let Some(arr) = container.get("skins").and_then(Value::as_array) else { return out };
    for (idx, s) in arr.iter().enumerate() {
        let Some(url) = text(s, "url").filter(|u| !u.is_empty()) else { continue };
        let hash = text(s, "hash").or_else(|| hash_of(Some(&url)));
        let id = text(s, "id").filter(|v| !v.is_empty()).or_else(|| hash.clone()).unwrap_or_else(|| url.clone());
        if !seen.insert(id.clone()) {
            continue;
        }
        let name = text(s, "name").filter(|v| !v.trim().is_empty()).unwrap_or_else(|| format!("Saved skin {}", idx + 1));
        let model = text(s, "model").filter(|v| !v.is_empty()).unwrap_or_else(|| "classic".to_owned());
        let last_seen = s.get("lastSeen").and_then(Value::as_i64).unwrap_or(0);
        out.push(SavedSkin { id, name, url, hash, model, last_seen });
    }
    out
}

fn save_skins_to_disk(user: &str, list: &[SavedSkin], active_hash: Option<&str>) {
    let Some(file) = skins_cache_file(user) else { return };
    let arr: Vec<Value> = list
        .iter()
        .map(|s| json!({ "id": s.id, "name": s.name, "url": s.url, "hash": s.hash, "model": s.model, "lastSeen": s.last_seen }))
        .collect();
    let body = json!({ "activeHash": active_hash, "skins": arr }).to_string();
    if let Some(parent) = file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = file.with_extension("json.tmp");
    if std::fs::write(&tmp, body).is_ok() {
        let _ = std::fs::rename(&tmp, &file);
    }
}

fn enc_seg(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Loads saved skins from disk immediately and refreshes from `/v1/profiles/<username>` in the background.
pub fn refresh_skins(username: &str, force: bool) {
    let user = username.trim().to_owned();
    if user.is_empty() {
        return;
    }
    let mut force = force;
    {
        let mut l = lock();
        if !l.skins_for_user.as_deref().is_some_and(|u| u.eq_ignore_ascii_case(&user)) {
            l.skins_for_user = Some(user.clone());
            l.saved_skins.clear();
            if let Some(f) = skins_cache_file(&user)
                && let Ok(txt) = std::fs::read_to_string(&f)
                && let Ok(root) = serde_json::from_str::<Value>(&txt)
            {
                let cached = parse_saved_skins(&root);
                if !cached.is_empty() {
                    l.saved_skins = cached;
                }
                if l.active_skin_hash.is_none() {
                    l.active_skin_hash = text(&root, "activeHash");
                }
            }
            force = true;
        }
        let fresh = l.skins_loaded_at.is_some_and(|t| t.elapsed() < Duration::from_secs(60));
        if l.skins_loading || (!force && fresh) {
            return;
        }
        l.skins_loading = true;
    }
    let _ = std::thread::Builder::new().name("Native skins".into()).spawn(move || {
        let api = net::api();
        if let Ok(root) = net::get_json(&format!("{api}/v1/profiles/{}", enc_seg(&user)), None) {
            let prof = root.get("profile").filter(|p| p.is_object()).unwrap_or(&root);
            let mut list = parse_saved_skins(prof);
            let cur_from_prof = prof
                .get("current")
                .filter(|c| c.is_object())
                .and_then(|c| text(c, "hash").or_else(|| hash_of(text(c, "url").as_deref())));
            let (dir_skin, dir_slim) = {
                let dir = horizon_client::native_skins::directory();
                match dir.entry(&user) {
                    Some(e) => (e.skin.clone(), e.slim),
                    None => (None, false),
                }
            };
            let cur_hash = cur_from_prof.or(dir_skin);
            if let Some(ref ch) = cur_hash
                && !list.iter().any(|s| s.hash.as_deref().is_some_and(|h| h.eq_ignore_ascii_case(ch)))
            {
                list.insert(
                    0,
                    SavedSkin {
                        id: ch.clone(),
                        name: "Current skin".to_owned(),
                        url: format!("{}{ch}", base()),
                        hash: Some(ch.clone()),
                        model: if dir_slim { "slim" } else { "classic" }.to_owned(),
                        last_seen: 0,
                    },
                );
            }
            save_skins_to_disk(&user, &list, cur_hash.as_deref());
            // Warm thumbnails
            for s in list.iter().take(24) {
                let u = match &s.hash {
                    Some(h) => net::hash_url(&base(), h),
                    None => s.url.clone(),
                };
                let _ = net::image(&u, 2 * 1024 * 1024);
            }
            let mut l = lock();
            l.saved_skins = list;
            if cur_hash.is_some() {
                l.active_skin_hash = cur_hash;
            }
            l.skins_loaded_at = Some(Instant::now());
        }
        lock().skins_loading = false;
    });
}

/// Equips a saved skin from the Locker right column (never calls `net::rejected` on failure).
pub fn equip_skin(skin: &SavedSkin, username: &str) {
    {
        let mut l = lock();
        if l.skin_busy.is_some() {
            return;
        }
        l.skin_busy = Some(skin.id.clone());
        l.skin_notice = None;
    }
    let skin = skin.clone();
    let user = username.to_owned();
    let _ = std::thread::Builder::new().name("Native equip skin".into()).spawn(move || {
        let api = net::api();
        let ticket = net::ticket();
        if let (Some(t), Some(h)) = (ticket.as_deref(), skin.hash.as_deref()) {
            let body = json!({ "hash": h, "url": skin.url, "model": if skin.is_slim() { "slim" } else { "default" } });
            let _ = net::post_json(&format!("{api}/v1/skins/equip"), Some(t), &body);
        }
        let mut l = lock();
        if let Some(ref h) = skin.hash {
            l.active_skin_hash = Some(h.clone());
            save_skins_to_disk(&user, &l.saved_skins, Some(h));
        }
        l.skin_notice = Some(format!("Equipped {}", skin.name));
        l.skin_busy = None;
    });
}

/// Puts the item on, or takes it off when it is worn.
pub fn toggle(i: &Item) {
    let swap;
    let (off, body);
    {
        let mut l = lock();
        if l.busy.is_some() {
            return;
        }
        off = is_worn(&l, i);
        body = if off {
            if i.is_cape() { json!({ "itemId": Value::Null }) } else { json!({ "slot": i.group(), "itemId": Value::Null }) }
        } else {
            json!({ "itemId": i.id })
        };
        // Optimistic. Wings / backpacks hang where the cloak does: putting one on takes the cloak off, and a cloak
        // takes the back item off (the server swaps them too).
        let mut s = None;
        if i.is_cape() {
            l.equipped = if off { None } else { Some(i.id.clone()) };
            if !off && l.wearing.remove(BACK).is_some() {
                s = Some(BACK);
            }
        } else {
            if off {
                l.wearing.remove(i.group());
            } else {
                l.wearing.insert(i.group().to_owned(), i.id.clone());
            }
            if !off && i.group() == BACK && l.equipped.take().is_some() {
                s = Some(CAPES);
            }
        }
        swap = s;
        l.busy = Some(i.id.clone());
    }
    post(body, swap);
}

/// `swap`: the slot ("capes" / "back") the server should have emptied; an older server that kept it gets asked once more.
fn post(body: Value, swap: Option<&'static str>) {
    let _ = std::thread::Builder::new().name("Native equip".into()).spawn(move || {
        let _io = io_lock();
        let api = net::api();
        let ticket = net::ticket();
        let result = (|| -> Result<(), HttpError> {
            // A cloak and a back item hang in the same place: take the other one off first, so the server never
            // keeps both (not every server swaps them by itself).
            if let Some(slot) = swap {
                let off = if slot == BACK { json!({ "slot": BACK, "itemId": Value::Null }) } else { json!({ "itemId": Value::Null }) };
                net::post_json(&format!("{api}/v1/store/equip"), ticket.as_deref(), &off)?;
            }
            let reply = net::post_json(&format!("{api}/v1/store/equip"), ticket.as_deref(), &body)?;
            let mut l = lock();
            apply(&mut l, &reply);
            let still_there = match swap {
                Some(CAPES) => l.equipped.is_some(),
                Some(BACK) => l.wearing.contains_key(BACK),
                _ => false,
            };
            drop(l);
            if still_there {
                let off = if swap == Some(BACK) { json!({ "slot": BACK, "itemId": Value::Null }) } else { json!({ "itemId": Value::Null }) };
                let reply = net::post_json(&format!("{api}/v1/store/equip"), ticket.as_deref(), &off)?;
                apply(&mut lock(), &reply);
            }
            lock().error = None;
            Ok(())
        })();
        match result {
            Ok(()) => {}
            Err(HttpError::Unauthorized) => {
                if let Some(t) = &ticket {
                    net::rejected(t);
                }
                lock().state = State::NoAccount;
            }
            Err(e) => {
                let mut l = lock();
                // It worked, the reply was just big: read the locker again.
                if e == HttpError::TooLarge {
                    l.error = None;
                } else {
                    l.error = Some(
                        if matches!(e, HttpError::Status(403, _)) { "Add it to your locker in the launcher first." } else { "Could not change your look. Try again." }
                            .to_owned(),
                    );
                }
                l.loaded_at = None;
                drop(l);
                refresh(true);
            }
        }
        lock().busy = None;
    });
}

/// The dye colour the player gave an item, as a 6-digit hex.
pub fn dye(l: &Locker, i: &Item) -> Option<String> {
    let d = l.dyes.get(&i.id)?;
    let d = d.trim_start_matches('#');
    (d.len() == 6 && d.bytes().all(|b| b.is_ascii_hexdigit())).then(|| d.to_ascii_lowercase())
}

/// The URL of a worn cosmetic's texture, dyed when the player dyed it.
pub fn texture_url(l: &Locker, i: &Item) -> Option<String> {
    if let (Some(dye), Some(url)) = (dye(l, i), i.dye_url.as_deref()) {
        return Some(format!("{url}{dye}"));
    }
    match i.texture_hash() {
        Some(h) => Some(net::hash_url(&base(), &h)),
        None => i.texture_url.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_read_the_stores_json() {
        let hash = "a".repeat(64);
        let i = parse_item(&json!({
            "id": "wings", "name": "Wings", "kind": "cosmetic", "slot": "back",
            "modelUrl": format!("https://x/csl/textures/{hash}"), "textureUrl": format!("https://x/{hash}?v=2"),
            "partner": {"name": "Aurex Hosting", "accent": "#112233"}
        }));
        assert_eq!(i.group(), "back");
        assert!(!i.is_cape());
        assert_eq!(i.model_hash().as_deref(), Some(hash.as_str()));
        assert_eq!(i.texture_hash().as_deref(), Some(hash.as_str()));
        assert_eq!(i.partner_accent, 0xFF112233);
        let cape = parse_item(&json!({"id": "c", "frames": 4, "fps": 8, "stripUrl": "https://x/s"}));
        assert!(cape.is_cape() && cape.animated());
        assert_eq!(cape.frame(0.5), 0);
        assert_eq!(cape.frame(1.0), 0);
        assert_eq!(cape.frame(0.3), 2);
    }

    #[test]
    fn emotes_and_bundles_stay_out_of_the_locker() {
        assert!(wearable(&json!({"id": "c"})) && wearable(&json!({"id": "c", "kind": "cape"})));
        assert!(wearable(&json!({"id": "h", "kind": "cosmetic", "slot": "hats"})));
        assert!(!wearable(&json!({"id": "emote-hi", "kind": "emote"})));
        assert!(!wearable(&json!({"id": "set", "kind": "bundle"})));
    }

    #[test]
    fn the_locker_reads_what_is_owned_and_worn() {
        let mut l = Locker {
            state: State::Ready,
            error: None,
            catalog: vec![Item { id: "a".into(), ..Item::default() }],
            owned: HashSet::new(),
            equipped: None,
            wearing: HashMap::new(),
            sides: HashMap::new(),
            dyes: HashMap::new(),
            busy: None,
            saved_skins: Vec::new(),
            skins_loading: false,
            active_skin_hash: None,
            skin_busy: None,
            skin_notice: None,
            skins_for_user: None,
            skins_loaded_at: None,
            loaded_at: None,
        };
        apply(
            &mut l,
            &json!({"owned": [{"id": "a"}, "b"], "equipped": "a", "wearing": {"hats": "h"}, "sides": {"hats": "left"}, "dyes": {"h": "#FF00AA"}}),
        );
        assert!(l.owned.contains("a") && l.owned.contains("b"));
        assert_eq!(l.sides["hats"], 1);
        let cape = l.catalog[0].clone();
        assert!(is_worn(&l, &cape));
        let hat = Item { id: "h".into(), kind: Some("cosmetic".into()), slot: Some("hats".into()), ..Item::default() };
        assert!(is_worn(&l, &hat));
        assert_eq!(dye(&l, &hat).as_deref(), Some("ff00aa"));
    }
}
