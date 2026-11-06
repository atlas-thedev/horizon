//! `Wardrobe`: the player's Native locker inside the game: the store
//! catalogue, what they own and wear, and equipping. Every request runs on a
//! background thread; the menu only reads the snapshot.
#![allow(dead_code)]

use super::net::{self, HttpError};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

pub const SLOTS: [&str; 6] = ["capes", "hats", "glasses", "back", "shoes", "hand"];
pub const SLOT_NAMES: [&str; 6] = ["Capes", "Hats", "Glasses", "Wings & Backpacks", "Shoes", "Hand Items"];
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

fn parse_item(o: &Value) -> Item {
    let partner = o.get("partner").filter(|p| p.is_object());
    let accent = partner
        .and_then(|p| text(p, "accent"))
        .filter(|a| a.len() == 7 && a.starts_with('#') && a[1..].bytes().all(|b| b.is_ascii_hexdigit()))
        .and_then(|a| u32::from_str_radix(&a[1..], 16).ok())
        .map_or(0xFF8B5CF6, |v| 0xFF000000 | v);
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
        for e in hidden.iter().filter(|e| e.is_object()) {
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
                .filter(|e| e.is_object())
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
                        if e == HttpError::Status(403) { "Add it to your locker in the launcher first." } else { "Could not change your look. Try again." }
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
