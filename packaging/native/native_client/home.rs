//! The title screen's live content from `/v1/mod/home` (the Java mod's `Home`): the sale / announcement banner,
//! title promo cards, quests with the player's progress, the coin balance and the Discord link. Loading runs on
//! background threads; the title only reads the last snapshot. While playing on a server, `tick` pings quest
//! progress once a minute.

use super::net;
use serde_json::{Value, json};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const REFRESH_MS: f64 = 60_000.0;
const LINK_POLL_MS: f64 = 4_000.0;
const PING_MS: f64 = 60_000.0;

#[derive(Clone, Debug, Default)]
pub struct Banner {
    pub kind: String,
    pub text: String,
    pub action: String,
    pub url: String,
    pub ends_at: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Button {
    pub label: String,
    pub action: String,
    pub value: String,
}

#[derive(Clone, Debug, Default)]
pub struct Promo {
    pub id: String,
    pub title: String,
    pub body: String,
    pub tag: String,
    pub image: String,
    pub accent: u32,
    pub buttons: Vec<Button>,
}

#[derive(Clone, Debug, Default)]
pub struct Reward {
    pub kind_of_reward: String,
    pub name: String,
    pub image: String,
    pub item_kind: String,
    pub amount: i64,
    pub owned: bool,
}

impl Reward {
    pub fn coins(&self) -> bool {
        self.kind_of_reward == "coins"
    }
}

#[derive(Clone, Debug, Default)]
pub struct Quest {
    pub id: String,
    pub title: String,
    pub description: String,
    pub pick: String,
    pub goal: String,
    pub goal_minutes: i64,
    pub servers: Vec<String>,
    pub rewards: Vec<Reward>,
    pub minutes: f64,
    pub state: String,
    pub choice: i64,
    pub ends_at: f64,
}

impl Quest {
    pub fn discord(&self) -> bool {
        self.goal == "discord"
    }
    pub fn pick_one(&self) -> bool {
        self.pick == "one" && self.rewards.len() > 1
    }
}

#[derive(Clone, Debug, Default)]
pub struct Doc {
    pub banner: Option<Banner>,
    pub promos: Vec<Promo>,
    pub quests: Vec<Quest>,
    pub coins_enabled: bool,
    pub coins_name: String,
    pub coins: i64,
    pub signed_in: bool,
    pub discord_linked: bool,
    pub discord_name: String,
    pub discord_invite: String,
    pub store_url: String,
}

#[derive(Default)]
struct State {
    doc: Option<Doc>,
    loading: bool,
    loaded_at: f64,
    failed_at: f64,
    skew: f64,
    link_code: Option<String>,
    link_expires: f64,
    link_busy: bool,
    link_error: Option<String>,
    claim_busy: Option<String>,
    claim_message: Option<String>,
    claim_at: f64,
    ping_at: f64,
    pinging: bool,
}

fn state() -> &'static Mutex<State> {
    static S: OnceLock<Mutex<State>> = OnceLock::new();
    S.get_or_init(Default::default)
}

fn lock() -> std::sync::MutexGuard<'static, State> {
    state().lock().unwrap_or_else(|e| e.into_inner())
}

fn now_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64)
}

/// The last loaded document; refreshes in the background when due.
pub fn get() -> Option<Doc> {
    let now = now_ms();
    let due = {
        let s = lock();
        let every = if s.link_code.is_some() && now < s.link_expires { LINK_POLL_MS } else { REFRESH_MS };
        !s.loading && now - s.loaded_at > every && now - s.failed_at > 15_000.0
    };
    if due {
        refresh();
    }
    lock().doc.clone()
}

/// Server time now (ms), so countdowns are right even when the PC clock is off.
pub fn server_now() -> f64 {
    now_ms() + lock().skew
}

pub fn refresh() {
    {
        let mut s = lock();
        if s.loading {
            return;
        }
        s.loading = true;
    }
    let _ = std::thread::Builder::new().name("Native home".into()).spawn(|| {
        let result = net::get_json(&format!("{}/v1/mod/home", net::api()), net::ticket().as_deref());
        let mut s = lock();
        s.loading = false;
        match result {
            Ok(v) => {
                apply(&mut s, &v);
                s.loaded_at = now_ms();
            }
            Err(_) => s.failed_at = now_ms(),
        }
    });
}

fn text(v: &Value, key: &str) -> String {
    match &v[key] {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

fn num(v: &Value, key: &str) -> f64 {
    v[key].as_f64().unwrap_or(0.0)
}

fn color(hex: &str, def: u32) -> u32 {
    let h = hex.trim_start_matches('#');
    if hex.starts_with('#') && h.len() == 6 {
        u32::from_str_radix(h, 16).map_or(def, |c| 0xFF00_0000 | c)
    } else {
        def
    }
}

fn apply(s: &mut State, v: &Value) {
    let server_time = num(v, "serverTime");
    if server_time > 0.0 {
        s.skew = server_time - now_ms();
    }
    let banner = v["banner"].as_object().map(|_| Banner {
        kind: text(&v["banner"], "kind"),
        text: text(&v["banner"], "text"),
        action: text(&v["banner"], "action"),
        url: text(&v["banner"], "url"),
        ends_at: num(&v["banner"], "endsAt"),
    }).filter(|b| !b.text.is_empty());
    let promos = v["promos"].as_array().map(|a| a.iter().map(|p| Promo {
        id: text(p, "id"),
        title: text(p, "title"),
        body: text(p, "body"),
        tag: text(p, "tag"),
        image: text(p, "image"),
        accent: color(&text(p, "accent"), 0xFF7C3AED),
        buttons: p["buttons"].as_array().map(|b| b.iter().take(2).map(|b| Button { label: text(b, "label"), action: text(b, "action"), value: text(b, "value") }).collect()).unwrap_or_default(),
    }).collect()).unwrap_or_default();
    let quests = v["quests"].as_array().map(|a| a.iter().map(|q| Quest {
        id: text(q, "id"),
        title: text(q, "title"),
        description: text(q, "description"),
        pick: text(q, "pick"),
        goal: text(&q["goal"], "type"),
        goal_minutes: num(&q["goal"], "minutes") as i64,
        servers: q["goal"]["servers"].as_array().map(|s| s.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect()).unwrap_or_default(),
        rewards: q["rewards"].as_array().map(|r| r.iter().map(|r| Reward {
            kind_of_reward: text(r, "type"),
            name: text(r, "name"),
            image: text(r, "image"),
            item_kind: text(r, "kind"),
            amount: num(r, "amount") as i64,
            owned: r["owned"].as_bool().unwrap_or(false),
        }).collect()).unwrap_or_default(),
        minutes: num(&q["progress"], "minutes"),
        state: text(q, "state"),
        choice: q["choice"].as_i64().unwrap_or(-1),
        ends_at: num(q, "endsAt"),
    }).collect()).unwrap_or_default();
    let linked = v["discord"]["linked"].as_bool().unwrap_or(false);
    if linked {
        s.link_code = None;
    }
    let store = text(v, "storeUrl");
    s.doc = Some(Doc {
        banner,
        promos,
        quests,
        coins_enabled: v["coins"]["enabled"].as_bool().unwrap_or(false),
        coins_name: text(&v["coins"], "name"),
        coins: v["coins"]["balance"].as_i64().unwrap_or(-1),
        signed_in: v["signedIn"].as_bool().unwrap_or(false),
        discord_linked: linked,
        discord_name: text(&v["discord"], "name"),
        discord_invite: text(&v["discord"], "invite"),
        store_url: if store.is_empty() { "https://playnative.net/store".into() } else { store },
    });
}

/// The Discord link in progress: (code, error, busy).
pub fn link_state() -> (Option<String>, Option<String>, bool) {
    let s = lock();
    let code = s.link_code.clone().filter(|_| now_ms() < s.link_expires);
    (code, s.link_error.clone(), s.link_busy)
}

/// Asks for a Discord link code ("/link CODE"); the doc is polled until the bot confirms it.
pub fn start_link() {
    {
        let mut s = lock();
        if s.link_busy {
            return;
        }
        if net::ticket().is_none() {
            s.link_error = Some("Sign in with Native in the launcher first.".into());
            return;
        }
        s.link_busy = true;
        s.link_error = None;
    }
    let _ = std::thread::Builder::new().name("Native discord link".into()).spawn(|| {
        let result = net::post_json(&format!("{}/v1/mod/discord/link", net::api()), net::ticket().as_deref(), &json!({}));
        {
            let mut s = lock();
            s.link_busy = false;
            match result {
                Ok(v) => {
                    let code = text(&v, "code");
                    if !code.is_empty() {
                        let expires = num(&v, "expiresAt");
                        s.link_expires = if expires > 0.0 { expires - s.skew } else { now_ms() + 15.0 * 60_000.0 };
                        s.link_code = Some(code);
                    }
                }
                Err(net::HttpError::Status(429, _)) => s.link_error = Some("Too many codes. Wait a few minutes.".into()),
                Err(_) => s.link_error = Some("Could not reach Native. Try again.".into()),
            }
        }
        refresh();
    });
}

/// The last claim: (busy quest id, message shown for 6 s).
pub fn claim_state() -> (Option<String>, Option<String>) {
    let s = lock();
    let msg = s.claim_message.clone().filter(|_| now_ms() - s.claim_at < 6_000.0);
    (s.claim_busy.clone(), msg)
}

/// Claims a finished quest (`choice` = the picked reward of a "1 of N" quest).
pub fn claim(quest: &str, choice: Option<usize>) {
    {
        let mut s = lock();
        if s.claim_busy.is_some() || net::ticket().is_none() {
            return;
        }
        s.claim_busy = Some(quest.to_owned());
        s.claim_message = None;
    }
    let quest = quest.to_owned();
    let _ = std::thread::Builder::new().name("Native quest claim".into()).spawn(move || {
        let body = choice.map_or_else(|| json!({}), |c| json!({ "choice": c }));
        let result = net::post_json(&format!("{}/v1/mod/quests/{quest}/claim", net::api()), net::ticket().as_deref(), &body);
        let mut refresh_after = false;
        {
            let mut s = lock();
            match result {
                Ok(v) => {
                    apply(&mut s, &v);
                    let got: Vec<String> = v["granted"].as_array().map(|g| g.iter().map(|x| text(x, "name")).collect()).unwrap_or_default();
                    s.claim_message = Some(if got.is_empty() { "Reward claimed!".into() } else { format!("You got {}!", got.join(", ")) });
                }
                Err(e) => {
                    s.claim_message = Some(match e {
                        net::HttpError::Status(409, _) => "Already claimed, or not finished yet.".into(),
                        net::HttpError::Status(400, _) => "Pick a reward first.".into(),
                        _ => "Could not claim right now. Try again.".into(),
                    });
                    refresh_after = true;
                }
            }
            s.claim_at = now_ms();
            s.claim_busy = None;
        }
        super::wardrobe::refresh(true);
        if refresh_after {
            refresh();
        }
    });
}

/// Every frame: while playing on a server, ping quest progress once a minute (only with a play quest running).
pub fn tick(in_world: bool, server: Option<&str>) {
    let now = now_ms();
    let Some(server) = server.filter(|s| in_world && !s.is_empty()) else { return };
    {
        let mut s = lock();
        if s.pinging || now - s.ping_at < PING_MS || net::ticket().is_none() {
            return;
        }
        s.ping_at = now;
        let wanted = s.doc.as_ref().is_some_and(|d| d.quests.iter().any(|q| !q.discord() && q.state == "active"));
        if !wanted {
            if now - s.loaded_at > 10.0 * REFRESH_MS {
                drop(s);
                refresh();
            }
            return;
        }
        s.pinging = true;
    }
    let server = server.to_owned();
    let _ = std::thread::Builder::new().name("Native quest ping".into()).spawn(move || {
        let result = net::post_json(&format!("{}/v1/mod/quests/ping", net::api()), net::ticket().as_deref(), &json!({ "server": server }));
        lock().pinging = false;
        if let Ok(v) = result {
            let done = v["quests"].as_array().is_some_and(|q| q.iter().any(|x| x["done"].as_bool() == Some(true)));
            if done {
                refresh();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_home_document_parses() {
        let mut s = State::default();
        apply(&mut s, &json!({
            "serverTime": 1, "signedIn": true, "storeUrl": "https://playnative.net/store",
            "banner": { "kind": "sale", "text": "15% off", "endsAt": 5000, "action": "store" },
            "promos": [{ "id": "a", "title": "LINK YOUR DISCORD", "accent": "#7c3aed", "buttons": [{ "label": "Link", "action": "discord-link", "value": "" }] }],
            "quests": [{ "id": "q", "title": "Medal", "pick": "one", "goal": { "type": "play", "minutes": 10, "servers": ["hypixel.net"] },
                "rewards": [{ "type": "coins", "amount": 50, "name": "50 Coins" }, { "type": "item", "name": "Wings", "kind": "cosmetic", "owned": true }],
                "progress": { "minutes": 4.5, "done": false }, "state": "active", "choice": null }],
            "coins": { "enabled": true, "name": "Coins", "balance": 1250 },
            "discord": { "linked": false, "invite": "https://discord.gg/x" }
        }));
        let d = s.doc.unwrap();
        assert_eq!(d.banner.unwrap().ends_at, 5000.0);
        assert_eq!(d.promos[0].accent, 0xFF7C3AED);
        assert_eq!(d.promos[0].buttons[0].action, "discord-link");
        let q = &d.quests[0];
        assert!(q.pick_one() && !q.discord());
        assert_eq!((q.goal_minutes, q.minutes, q.choice), (10, 4.5, -1));
        assert!(q.rewards[0].coins() && q.rewards[1].owned);
        assert_eq!(d.coins, 1250);
    }
}
