//! The in-game side of Native Relay: friends with live presence, DMs and
//! groups. Talks to the Native API with the game-only ticket the launcher
//! hands the game (`/v1/mod/friends`, `/v1/social/relay/*` and the
//! `/v1/mod/stream` event stream with `?relay=1`). All network work runs on
//! background threads; the UI reads a snapshot.
#![allow(dead_code)]

use super::net::{self, HttpError};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{BufRead, BufReader};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Toast key that opens Relay on the pending friend requests.
pub const REQUESTS_KEY: &str = "home:pending";

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    NoAccount,
    Connecting,
    Online,
    Offline,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Friend {
    pub id: String,
    pub name: String,
    pub nickname: Option<String>,
    pub status: String,
    pub activity: String,
    pub server_address: String,
    pub skin: Option<String>,
    pub online: bool,
    pub unread: i32,
    pub pinned: bool,
    /// When they were last online (ms since epoch), 0 when unknown.
    pub last_seen: i64,
    /// When the last message in this chat was sent, 0 when there is none: the list is ordered by it.
    pub last_msg: i64,
}

impl Friend {
    /// Presence dot colour: green online, amber idle/away, red do-not-disturb, grey offline.
    pub fn dot(&self) -> u32 {
        if !self.online {
            return 0xFF52525B;
        }
        match self.status.as_str() {
            "idle" | "away" => 0xFFF59E0B,
            "dnd" | "busy" => 0xFFEF4444,
            _ => 0xFF22C55E,
        }
    }

    /// One-line status: activity, else Online / Idle / Do not disturb / Offline.
    pub fn line(&self) -> String {
        if !self.online {
            return if self.last_seen > 0 { format!("Last seen {}", ago(self.last_seen)) } else { "Offline".to_owned() };
        }
        if let Some(a) = nice_activity(&self.activity, &self.status) {
            return a;
        }
        match self.status.as_str() {
            "idle" | "away" => "Idle".to_owned(),
            "dnd" | "busy" => "Do not disturb".to_owned(),
            _ => "Online".to_owned(),
        }
    }

    pub fn display(&self) -> &str {
        match &self.nickname {
            Some(n) if !n.is_empty() => n,
            _ => &self.name,
        }
    }
}

/// The launcher writes activities like "In-game: Hypixel", "In-game: Singleplayer" or "In-game: Menus": shown here as
/// "Playing on Hypixel", "Playing Singleplayer", "In the menus". `None` when there is nothing to say.
pub fn nice_activity(activity: &str, status: &str) -> Option<String> {
    let a = activity.trim();
    let lower = a.to_lowercase();
    let mut rest: Option<String> = None;
    for prefix in ["in-game:", "ingame:", "in game:", "in-game -", "in-game", "ingame", "in game"] {
        if lower.starts_with(prefix) {
            rest = Some(a[prefix.len()..].trim_start_matches(|c: char| c.is_whitespace() || c == ':' || c == '-').trim().to_owned());
            break;
        }
    }
    let in_game = rest.is_some() || status == "in-game";
    let rest = match rest {
        Some(r) => r,
        None => {
            if !a.is_empty() && status != "in-menus" {
                return Some(if a.eq_ignore_ascii_case("In Launcher") { "In the launcher".to_owned() } else { a.to_owned() });
            }
            String::new()
        }
    };
    let r = rest.to_lowercase();
    if status == "in-menus" || r == "menus" || r == "menu" || r == "in the menus" {
        return Some("In the menus".to_owned());
    }
    if !in_game {
        return None;
    }
    Some(match r.as_str() {
        "" | "minecraft" => "Playing Minecraft".to_owned(),
        "singleplayer" => "Playing Singleplayer".to_owned(),
        "multiplayer" => "Playing multiplayer".to_owned(),
        "realms" => "Playing on Realms".to_owned(),
        _ => format!("Playing on {rest}"),
    })
}

/// "just now", "5 min ago", "3 h ago", "2 d ago", else the date.
pub fn ago(at: i64) -> String {
    let s = ((now_ms() - at) / 1000).max(0);
    match s {
        0..=59 => "just now".to_owned(),
        60..=3599 => format!("{} min ago", s / 60),
        3600..=86_399 => format!("{} h ago", s / 3600),
        86_400..=604_799 => format!("{} d ago", s / 86_400),
        _ => date(at),
    }
}

/// "5 Nov 2026".
pub fn date(at_ms: i64) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let (y, m, d) = civil(at_ms.div_euclid(1000).div_euclid(86_400));
    format!("{d} {} {y}", MONTHS[(m - 1) as usize])
}

/// Year, month and day of a day count since 1970-01-01.
pub fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub last_text: Option<String>,
    pub last_sender: Option<String>,
    pub last_at: i64,
    pub unread: i32,
    pub members: i32,
    /// (id, name)
    pub member_list: Vec<(String, String)>,
    /// Your role here: "owner", "admin" or "member" (owners and admins can delete anyone's message).
    pub role: String,
    pub pinned: bool,
}

impl Group {
    pub fn can_moderate(&self) -> bool {
        self.role == "owner" || self.role == "admin"
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reply {
    pub id: String,
    pub sender_name: String,
    pub content: String,
    pub media: bool,
    pub deleted: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Message {
    pub id: String,
    pub sender_id: String,
    pub sender_name: String,
    pub content: String,
    pub created_at: i64,
    pub pending: bool,
    pub failed: bool,
    pub system: bool,
    pub deleted: bool,
    /// Attached picture (own upload or GIF), `None` for plain text.
    pub media_url: Option<String>,
    pub media_name: Option<String>,
    /// Locally pasted picture shown while it uploads.
    pub local_key: Option<String>,
    pub reply: Option<Reply>,
    /// Reactions: (user id, emoji).
    pub reactions: Vec<(String, String)>,
    pub edited: bool,
    pub read: bool,
}

/// What a reply to `m` quotes.
pub fn quote(m: &Message) -> Reply {
    Reply {
        id: m.id.clone(),
        sender_name: m.sender_name.clone(),
        content: if m.deleted { String::new() } else { m.content.clone() },
        media: m.media_url.is_some() || m.local_key.is_some(),
        deleted: m.deleted,
    }
}

/// One chat (a DM or a group) and the messages loaded so far, oldest first.
#[derive(Clone, Debug, Default)]
pub struct Conversation {
    /// `dm:<friendId>` or `g:<groupId>`
    pub key: String,
    pub messages: Vec<Message>,
    pub loading: bool,
    pub loaded: bool,
    /// True once synced with the live server this session (disk hydration sets `loaded` immediately while `synced` stays false).
    pub synced: bool,
    pub has_more: bool,
    pub error: Option<String>,
}

impl Conversation {
    pub fn is_group(&self) -> bool {
        self.key.starts_with("g:")
    }

    pub fn target_id(&self) -> &str {
        self.key.split_once(':').map_or("", |p| p.1)
    }
}

/// Something to pop up in game (a new message while the chat is closed).
#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    pub conv_key: Option<String>,
    pub title: String,
    pub body: String,
    pub avatar_name: String,
    pub skin: Option<String>,
    pub at: i64,
}

/// A friend request: received (they asked you) or sent (you asked them).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Request {
    pub id: String,
    pub user_id: String,
    pub name: String,
    pub skin: Option<String>,
    pub created_at: i64,
}

/// A player found by "Add friend" (or blocked).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Person {
    pub id: String,
    pub name: String,
    pub skin: Option<String>,
}

/// The public Native profile shown in the side panel.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Profile {
    pub name: String,
    pub bio: Option<String>,
    pub badges: Vec<String>,
    pub joined_at: i64,
    pub friends: i32,
    pub premium: bool,
}

struct Shared {
    link: Link,
    me_id: Option<String>,
    me_name: Option<String>,
    me_skin: Option<String>,
    friends: Vec<Friend>,
    groups: Vec<Group>,
    requests: i32,
    received: Vec<Request>,
    sent: Vec<Request>,
    requests_loaded: bool,
    blocked: Vec<Person>,
    blocked_loaded: bool,
    found: Vec<Person>,
    found_for: String,
    searching: bool,
    note: Option<(String, bool, Instant)>,
    profiles: HashMap<String, Profile>,
    profile_loading: HashSet<String>,
    groups_loaded: bool,
    friends_loaded: bool,
    conversations: HashMap<String, Conversation>,
    typing: HashMap<String, Instant>,
    notices: VecDeque<Notice>,
    viewing: Option<String>,
    cleared: Option<HashMap<String, i64>>,
    last_typing_sent: Option<Instant>,
    started: bool,
    group_creating: bool,
    group_create_error: Option<String>,
    created_group_key: Option<String>,
    disk_loaded_account: Option<String>,
    disk_hydrated: HashSet<String>,
}

fn shared() -> &'static Mutex<Shared> {
    static S: OnceLock<Mutex<Shared>> = OnceLock::new();
    S.get_or_init(|| {
        Mutex::new(Shared {
            link: Link::Connecting,
            me_id: None,
            me_name: None,
            me_skin: None,
            friends: Vec::new(),
            groups: Vec::new(),
            requests: 0,
            received: Vec::new(),
            sent: Vec::new(),
            requests_loaded: false,
            blocked: Vec::new(),
            blocked_loaded: false,
            found: Vec::new(),
            found_for: String::new(),
            searching: false,
            note: None,
            profiles: HashMap::new(),
            profile_loading: HashSet::new(),
            groups_loaded: false,
            friends_loaded: false,
            conversations: HashMap::new(),
            typing: HashMap::new(),
            notices: VecDeque::new(),
            viewing: None,
            cleared: None,
            last_typing_sent: None,
            started: false,
            group_creating: false,
            group_create_error: None,
            created_group_key: None,
            disk_loaded_account: None,
            disk_hydrated: HashSet::new(),
        })
    })
}

fn lock() -> MutexGuard<'static, Shared> {
    shared().lock().unwrap_or_else(|e| e.into_inner())
}

/// What the UI draws from: friends, groups, requests and the connection, copied under one short lock.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub link: Link,
    pub me_id: Option<String>,
    pub me_name: Option<String>,
    pub me_skin: Option<String>,
    pub friends: Vec<Friend>,
    pub groups: Vec<Group>,
    pub requests: i32,
    pub received: Vec<Request>,
    pub sent: Vec<Request>,
    pub requests_loaded: bool,
    pub blocked: Vec<Person>,
    pub blocked_loaded: bool,
    pub found: Vec<Person>,
    pub found_for: String,
    pub searching: bool,
    pub note: Option<(String, bool)>,
    pub groups_loaded: bool,
    pub friends_loaded: bool,
    pub viewing: Option<String>,
}

pub fn snapshot() -> Snapshot {
    let s = lock();
    Snapshot {
        link: s.link,
        me_id: s.me_id.clone(),
        me_name: s.me_name.clone(),
        me_skin: s.me_skin.clone(),
        friends: s.friends.clone(),
        groups: s.groups.clone(),
        requests: s.requests,
        received: s.received.clone(),
        sent: s.sent.clone(),
        requests_loaded: s.requests_loaded,
        blocked: s.blocked.clone(),
        blocked_loaded: s.blocked_loaded,
        found: s.found.clone(),
        found_for: s.found_for.clone(),
        searching: s.searching,
        note: s.note.as_ref().filter(|n| n.2.elapsed() < Duration::from_secs(6)).map(|n| (n.0.clone(), n.1)),
        groups_loaded: s.groups_loaded,
        friends_loaded: s.friends_loaded,
        viewing: s.viewing.clone(),
    }
}

/// Starts Relay (idempotent). It always runs: it waits for a sign-in, so a ticket that arrives (or comes back)
/// mid-game connects Relay.
pub fn start() {
    {
        let mut s = lock();
        if s.started {
            return;
        }
        s.started = true;
        if net::ticket().is_none() {
            s.link = Link::NoAccount;
        }
    }
    ensure_disk_loaded();
    let _ = std::thread::Builder::new().name("Native relay stream".into()).spawn(stream_loop);
}

/// The unread chats' total, not counting the one being viewed.
pub fn unread_total() -> i32 {
    let s = lock();
    let mut n = 0;
    for f in &s.friends {
        if s.viewing.as_deref() != Some(format!("dm:{}", f.id).as_str()) {
            n += f.unread;
        }
    }
    for g in &s.groups {
        if s.viewing.as_deref() != Some(format!("g:{}", g.id).as_str()) {
            n += g.unread;
        }
    }
    n
}

pub fn conversation(key: &str) -> Conversation {
    lock().conversations.get(key).cloned().unwrap_or_else(|| Conversation { key: key.to_owned(), ..Conversation::default() })
}

/// The newest message of a chat that was ever opened (the list reads it).
pub fn peek_last(key: &str) -> Option<Message> {
    lock().conversations.get(key).and_then(|c| c.messages.last().cloned())
}

pub fn friend(id: &str) -> Option<Friend> {
    lock().friends.iter().find(|f| f.id == id).cloned()
}

pub fn group(id: &str) -> Option<Group> {
    lock().groups.iter().find(|g| g.id == id).cloned()
}

/// Names typing in a conversation right now.
pub fn typing(key: &str) -> Vec<String> {
    let s = lock();
    let now = Instant::now();
    s.typing
        .iter()
        .filter(|(k, until)| **until > now && k.starts_with(&format!("{key}|")))
        .map(|(k, _)| name_of(&s, &k[key.len() + 1..], None))
        .collect()
}

fn name_of(s: &Shared, user_id: &str, fallback: Option<&str>) -> String {
    if s.me_id.as_deref() == Some(user_id) {
        return s.me_name.clone().unwrap_or_else(|| "You".to_owned());
    }
    if let Some(f) = s.friends.iter().find(|f| f.id == user_id) {
        return f.display().to_owned();
    }
    for g in &s.groups {
        if let Some(m) = g.member_list.iter().find(|m| m.0 == user_id) {
            return m.1.clone();
        }
    }
    fallback.unwrap_or("Someone").to_owned()
}

/// Takes the pop-ups waiting to be shown.
pub fn take_notices() -> Vec<Notice> {
    lock().notices.drain(..).collect()
}

/// A card from another part of Native (voice: missed call, declined, call ended).
pub fn push_notice(n: Notice) {
    lock().notices.push_back(n);
}

/// The signed-in Native account's id, without copying the whole snapshot.
pub fn me_id() -> Option<String> {
    lock().me_id.clone()
}

/// The signed-in account's skin reference (a texture hash, or "mj:<uuid>").
pub fn me_skin() -> Option<String> {
    lock().me_skin.clone()
}

pub fn set_viewing(key: Option<&str>) {
    lock().viewing = key.map(str::to_owned);
}

pub fn note_text() -> Option<(String, bool)> {
    lock().note.as_ref().filter(|n| n.2.elapsed() < Duration::from_secs(6)).map(|n| (n.0.clone(), n.1))
}

fn note(text: &str, error: bool) {
    lock().note = Some((text.to_owned(), error, Instant::now()));
}

// ── parsing ──────────────────────────────────────────────────────────────

fn str_of(o: &Value, k: &str) -> Option<String> {
    o.get(k).and_then(Value::as_str).map(str::to_owned)
}

fn bool_of(o: &Value, k: &str) -> bool {
    match o.get(k) {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        _ => false,
    }
}

fn int_of(o: &Value, k: &str) -> i64 {
    o.get(k).and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64))).unwrap_or(0)
}

/// ".../csl/textures/<sha256>" -> "<sha256>" (the skin reference avatars use), else `None`.
pub fn hash_of(url: Option<&str>) -> Option<String> {
    super::wardrobe::hash_of(url)
}

/// Native skin hash, else "mj:<uuid>" for a linked Mojang account, else `None`.
fn skin_ref(native: Option<String>, mojang: Option<String>) -> Option<String> {
    match native {
        Some(s) if !s.is_empty() => Some(s),
        _ => mojang.filter(|u| !u.is_empty()).map(|u| format!("mj:{}", u.replace('-', "").to_lowercase())),
    }
}

fn parse_reactions(list: &Value) -> Vec<(String, String)> {
    list.as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| Some((str_of(e, "userId").unwrap_or_default(), str_of(e, "reaction")?)))
        .collect()
}

fn media_allowed(url: &str, api: &str) -> bool {
    url.contains("/v1/social/media/")
        || url.starts_with(api)
        || (url.contains("giphy.com/") && (url.starts_with("https://") || url.starts_with("http://")))
        || url.contains(".nativelaunch.net/")
}

fn parse_message(s: &Shared, m: &Value) -> Message {
    let sender = str_of(m, "senderId").unwrap_or_default();
    let deleted = bool_of(m, "isDeleted");
    let media_url = if deleted { None } else { str_of(m, "mediaUrl") };
    let media_name = str_of(m, "mediaName");
    let mut content = str_of(m, "content");
    if content.as_deref().is_none_or(str::is_empty) && !deleted {
        content = Some(match &media_name {
            Some(n) => format!("[{n}]"),
            None if media_url.is_some() || bool_of(m, "isMedia") => "[attachment]".to_owned(),
            None => String::new(),
        });
    }
    let _ = media_allowed(media_url.as_deref().unwrap_or(""), &net::api());
    let mut out = Message {
        id: str_of(m, "id").unwrap_or_default(),
        sender_name: name_of(s, &sender, str_of(m, "senderName").as_deref()),
        sender_id: sender,
        content: if deleted { "Message deleted".to_owned() } else { content.unwrap_or_default() },
        created_at: int_of(m, "createdAt"),
        system: bool_of(m, "isSystem"),
        deleted,
        media_url,
        media_name,
        edited: !deleted && int_of(m, "editedAt") > 0,
        read: bool_of(m, "isRead"),
        ..Message::default()
    };
    if !deleted && let Some(r) = m.get("reactions") {
        out.reactions = parse_reactions(r);
    }
    if let Some(r) = m.get("reply").filter(|r| r.is_object()) {
        let gone = bool_of(r, "deleted");
        out.reply = Some(Reply {
            id: str_of(r, "id").unwrap_or_default(),
            sender_name: name_of(s, &str_of(r, "senderId").unwrap_or_default(), str_of(r, "senderName").as_deref()),
            content: if gone { String::new() } else { str_of(r, "content").unwrap_or_default() },
            media: !gone && (r.get("mediaName").is_some_and(|v| !v.is_null()) || r.get("mediaUrl").is_some_and(|v| !v.is_null())),
            deleted: gone,
        });
    }
    out
}

fn parse_messages(s: &Shared, list: Option<&Value>) -> Vec<Message> {
    list.and_then(Value::as_array).into_iter().flatten().filter(|e| e.is_object()).map(|m| parse_message(s, m)).collect()
}

fn parse_group(g: &Value) -> Group {
    let last = g.get("lastMessage").filter(|l| l.is_object());
    let members = g
        .get("members")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|m| {
            (
                str_of(m, "id").unwrap_or_default(),
                str_of(m, "name").or_else(|| str_of(m, "username")).unwrap_or_else(|| "Member".to_owned()),
            )
        })
        .collect();
    Group {
        id: str_of(g, "id").unwrap_or_default(),
        name: str_of(g, "name").unwrap_or_default(),
        last_text: last.and_then(|l| str_of(l, "content")),
        last_sender: last.and_then(|l| str_of(l, "senderName")),
        last_at: last.map_or(0, |l| int_of(l, "createdAt")),
        unread: int_of(g, "unreadCount") as i32,
        members: int_of(g, "memberCount") as i32,
        member_list: members,
        role: str_of(g, "role").unwrap_or_else(|| "member".to_owned()),
        pinned: bool_of(g, "pinned"),
    }
}

/// Pinned first, then the group with the newest message.
fn sort_groups(list: &mut [Group]) {
    list.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.last_at.cmp(&a.last_at)));
}

fn sort_friends(list: &mut [Friend]) {
    list.sort_by(|a, b| {
        b.pinned
            .cmp(&a.pinned)
            .then(b.last_msg.cmp(&a.last_msg))
            .then(b.online.cmp(&a.online))
            .then((b.unread > 0).cmp(&(a.unread > 0)))
            .then_with(|| a.display().to_lowercase().cmp(&b.display().to_lowercase()))
    });
}

// ── HTTP on the account's behalf ─────────────────────────────────────────

fn full(path: &str) -> String {
    format!("{}{}", net::api(), path)
}

/// Only the core game endpoints can say the ticket itself is bad; a 401 from the friend endpoints
/// (requests, search...) means "not allowed here" and must not lock Relay.
fn check_unauthorized(path: &str, e: &HttpError, used: &Option<String>) {
    if *e == HttpError::Unauthorized
        && (path.starts_with("/v1/mod/") || path.starts_with("/v1/social/relay/"))
        && let Some(t) = used
    {
        net::rejected(t);
    }
}

fn get(path: &str) -> Result<Value, HttpError> {
    let ticket = net::ticket();
    if ticket.is_none() {
        return Err(HttpError::Other("not signed in".into()));
    }
    let r = net::get_json(&full(path), ticket.as_deref());
    if let Err(e) = &r {
        check_unauthorized(path, e, &ticket);
    }
    r
}

fn post(path: &str, body: &Value) -> Result<Value, HttpError> {
    let ticket = net::ticket();
    if ticket.is_none() {
        return Err(HttpError::Other("not signed in".into()));
    }
    let r = net::post_json(&full(path), ticket.as_deref(), body);
    if let Err(e) = &r {
        check_unauthorized(path, e, &ticket);
    }
    r
}

/// The server's own error message ("Already friends."), else the fallback.
fn error_text(e: &HttpError, fallback: &str) -> String {
    match e {
        HttpError::Status(_, Some(m)) if !m.is_empty() => m.clone(),
        _ => fallback.to_owned(),
    }
}

/// Standard base64 with padding.
fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn enc(s: &str) -> String {
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

fn spawn(job: impl FnOnce() + Send + 'static) {
    let _ = std::thread::Builder::new().name("Native relay".into()).spawn(job);
}

fn messages_path(c: &Conversation) -> String {
    if c.is_group() {
        format!("/v1/social/relay/groups/{}/messages", enc(c.target_id()))
    } else {
        format!("/v1/social/relay/dm/{}/messages", enc(c.target_id()))
    }
}

fn action_path(group: bool, id: &str, action: &str) -> String {
    format!("{}{}/{action}", if group { "/v1/social/relay/messages/" } else { "/v1/social/relay/dm-messages/" }, enc(id))
}

fn pref_path(key: &str, what: &str) -> String {
    match key.strip_prefix("g:") {
        Some(g) => format!("/v1/social/relay/groups/{}/{what}", enc(g)),
        None => format!("/v1/social/relay/dm/{}/{what}", enc(key.strip_prefix("dm:").unwrap_or(key))),
    }
}

/// The latest page plus what only we have: older pages already loaded, pending / failed sends, newer live rows.
pub fn merge_latest(have: &[Message], page: Vec<Message>) -> Vec<Message> {
    if have.is_empty() {
        return page;
    }
    let ids: HashSet<&str> = page.iter().map(|m| m.id.as_str()).collect();
    let first = page.first().map_or(i64::MIN, |m| m.created_at);
    let last = page.last().map_or(i64::MIN, |m| m.created_at);
    let mut out: Vec<Message> = have
        .iter()
        .filter(|m| !page.is_empty() && m.created_at < first && !ids.contains(m.id.as_str()) && !m.pending && !m.failed)
        .cloned()
        .collect();
    let tail: Vec<Message> = have
        .iter()
        .filter(|m| !ids.contains(m.id.as_str()) && (m.pending || m.failed || (!page.is_empty() && m.created_at > last)))
        .cloned()
        .collect();
    out.extend(page);
    out.extend(tail);
    out
}

// ── actions (called from the render thread; never block) ─────────────────

pub fn refresh() {
    if net::ticket().is_none() {
        return;
    }
    spawn(load_friends);
    spawn(load_groups);
    spawn(load_requests);
    spawn(load_blocked);
}

fn with_conv<R>(key: &str, f: impl FnOnce(&mut Shared, &mut Conversation) -> R) -> R {
    let mut s = lock();
    let mut c = s.conversations.remove(key).unwrap_or_else(|| Conversation { key: key.to_owned(), ..Conversation::default() });
    let r = f(&mut s, &mut c);
    s.conversations.insert(key.to_owned(), c);
    r
}

fn clear_unread(s: &mut Shared, key: &str) {
    if let Some(id) = key.strip_prefix("dm:") {
        for f in s.friends.iter_mut().filter(|f| f.id == id) {
            f.unread = 0;
        }
    } else if let Some(id) = key.strip_prefix("g:") {
        for g in s.groups.iter_mut().filter(|g| g.id == id) {
            g.unread = 0;
        }
    }
}

/// Opens a conversation: hydrates instantly from disk cache, then fetches the latest page and marks it read.
pub fn open(key: &str) {
    ensure_disk_loaded();
    hydrate_conversation(key);
    let skip = {
        let mut s = lock();
        s.viewing = Some(key.to_owned());
        clear_unread(&mut s, key);
        s.notices.retain(|n| n.conv_key.as_deref() != Some(key));
        net::ticket().is_none() || s.conversations.get(key).is_some_and(|c| c.loading)
    };
    mark_read_later(key);
    if skip {
        return;
    }
    with_conv(key, |_, c| c.loading = true);
    let key = key.to_owned();
    spawn(move || {
        let path = format!("{}?limit=60", messages_path(&conversation(&key)));
        let result = get(&path);
        let mut to_warm = Vec::new();
        let mut should_save = false;
        with_conv(&key, |s, c| {
            match result {
                Ok(page) => {
                    let latest = parse_messages(s, page.get("messages"));
                    let more = bool_of(&page, "hasMore");
                    let kept_older = !latest.is_empty() && c.messages.first().is_some_and(|m| m.created_at < latest[0].created_at);
                    c.has_more = if kept_older { c.has_more } else { more };
                    c.messages = merge_latest(&c.messages, latest);
                    c.loaded = true;
                    c.synced = true;
                    c.error = None;
                    to_warm = c.messages.clone();
                    should_save = true;
                }
                Err(_) => {
                    if c.messages.is_empty() {
                        c.error = Some("Could not load messages.".to_owned());
                    }
                }
            }
            c.loading = false;
        });
        if should_save {
            warm_media(&to_warm);
            save_conversation_async(&key);
            save_index_async();
        }
    });
}

/// Loads the page before the oldest loaded message.
pub fn load_older(key: &str) {
    let ready = with_conv(key, |_, c| {
        if net::ticket().is_none() || c.loading || !c.has_more || c.messages.is_empty() {
            return None;
        }
        c.loading = true;
        Some((messages_path(c), c.messages[0].created_at))
    });
    let Some((path, before)) = ready else { return };
    let key = key.to_owned();
    spawn(move || {
        let result = get(&format!("{path}?limit=60&markRead=0&before={before}"));
        let mut should_save = false;
        let mut to_warm = Vec::new();
        with_conv(&key, |s, c| {
            match result {
                Ok(page) => {
                    let older = parse_messages(s, page.get("messages"));
                    let have: HashSet<String> = c.messages.iter().map(|m| m.id.clone()).collect();
                    let mut merged: Vec<Message> = older.into_iter().filter(|m| !have.contains(&m.id)).collect();
                    to_warm = merged.clone();
                    merged.append(&mut c.messages);
                    c.messages = merged;
                    c.has_more = bool_of(&page, "hasMore");
                    should_save = true;
                }
                Err(_) => c.error = Some("Could not load older messages.".to_owned()),
            }
            c.loading = false;
        });
        if should_save {
            warm_media(&to_warm);
            save_conversation_async(&key);
        }
    });
}

fn add_pending(key: &str, m: Message) {
    with_conv(key, |_, c| c.messages.push(m));
}

/// Puts the server's copy where the pending one was.
fn replace_pending(key: &str, temp_id: &str, mut sent: Message) {
    let media_url = sent.media_url.clone();
    with_conv(key, |_, c| {
        let have = c.messages.iter().any(|m| m.id == sent.id);
        let mut next = Vec::with_capacity(c.messages.len());
        let mut placed = false;
        for m in c.messages.drain(..) {
            if m.id == temp_id {
                if !have {
                    if sent.local_key.is_none() {
                        sent.local_key = m.local_key.clone();
                    }
                    next.push(sent.clone());
                    placed = true;
                }
                continue;
            }
            next.push(m);
        }
        if !have && !placed {
            next.push(sent);
        }
        c.messages = next;
        c.loaded = true;
    });
    if let Some(u) = media_url {
        let _ = super::chat_media::get(&u, &net::api());
    }
    save_conversation_async(key);
    save_index_async();
}

fn fail(key: &str, temp_id: &str) {
    with_conv(key, |_, c| {
        for m in c.messages.iter_mut().filter(|m| m.id == temp_id) {
            m.pending = false;
            m.failed = true;
        }
    });
}

/// A message was just sent or received in this chat: it moves to the top of the list (below pinned ones).
fn bump(s: &mut Shared, key: &str, at: i64) {
    if let Some(gid) = key.strip_prefix("g:") {
        for g in s.groups.iter_mut().filter(|g| g.id == gid && g.last_at < at) {
            g.last_at = at;
        }
        sort_groups(&mut s.groups);
    } else if let Some(fid) = key.strip_prefix("dm:") {
        for f in s.friends.iter_mut().filter(|f| f.id == fid) {
            f.last_msg = f.last_msg.max(at);
        }
        sort_friends(&mut s.friends);
    }
}

/// Sends text, as an answer to `reply_to` when given.
pub fn send(key: &str, text: &str, reply_to: Option<&Message>) {
    let content = text.trim().to_owned();
    if content.is_empty() || net::ticket().is_none() {
        return;
    }
    let temp_id = format!("local-{}", now_ms() * 1000 + (std::process::id() as i64 % 1000));
    let (me_id, me_name) = {
        let s = lock();
        (s.me_id.clone().unwrap_or_default(), s.me_name.clone().unwrap_or_default())
    };
    let mut pending = Message {
        id: temp_id.clone(),
        sender_id: me_id,
        sender_name: me_name,
        content: content.clone(),
        created_at: now_ms(),
        pending: true,
        ..Message::default()
    };
    if let Some(r) = reply_to.filter(|r| !r.id.is_empty() && !r.id.starts_with("local-")) {
        pending.reply = Some(quote(r));
    }
    let reply_id = pending.reply.as_ref().map(|r| r.id.clone());
    let at = pending.created_at;
    add_pending(key, pending);
    bump(&mut lock(), key, at);
    let key = key.to_owned();
    spawn(move || {
        let mut body = json!({ "content": content });
        if let Some(id) = reply_id {
            body["replyTo"] = json!(id);
        }
        let path = messages_path(&conversation(&key));
        match post(&path, &body) {
            Ok(res) => {
                let sent = parse_message(&lock(), res.get("message").unwrap_or(&Value::Null));
                replace_pending(&key, &temp_id, sent);
            }
            Err(_) => fail(&key, &temp_id),
        }
    });
}

/// Sends a GIF picked from the picker (Giphy URL).
pub fn send_gif(key: &str, url: &str, title: &str) {
    if net::ticket().is_none() || url.is_empty() {
        return;
    }
    let temp_id = format!("local-{}", now_ms() * 1000);
    let name = if title.is_empty() { "GIF.gif".to_owned() } else { title.to_owned() };
    let (me_id, me_name) = {
        let s = lock();
        (s.me_id.clone().unwrap_or_default(), s.me_name.clone().unwrap_or_default())
    };
    add_pending(
        key,
        Message {
            id: temp_id.clone(),
            sender_id: me_id,
            sender_name: me_name,
            created_at: now_ms(),
            pending: true,
            media_url: Some(url.to_owned()),
            media_name: Some(name.clone()),
            ..Message::default()
        },
    );
    bump(&mut lock(), key, now_ms());
    let (key, url) = (key.to_owned(), url.to_owned());
    spawn(move || {
        let body = json!({ "content": "", "mediaUrl": url, "mediaName": name, "mediaKind": "image", "isMedia": true });
        let path = messages_path(&conversation(&key));
        match post(&path, &body) {
            Ok(res) => {
                let sent = parse_message(&lock(), res.get("message").unwrap_or(&Value::Null));
                replace_pending(&key, &temp_id, sent);
            }
            Err(_) => fail(&key, &temp_id),
        }
    });
}

fn change(key: &str, id: &str, with: impl FnOnce(&Message) -> Message) {
    let changed = with_conv(key, |_, c| {
        if let Some(m) = c.messages.iter_mut().find(|m| m.id == id) {
            *m = with(m);
            true
        } else {
            false
        }
    });
    if changed {
        save_conversation_async(key);
    }
}

/// Toggles your reaction on a message (the server flips it); shows at once, then takes the server's list.
pub fn react(key: &str, message_id: &str, emoji: &str) {
    if net::ticket().is_none() || message_id.starts_with("local-") || message_id.is_empty() {
        return;
    }
    let me = lock().me_id.clone().unwrap_or_default();
    change(key, message_id, |m| {
        let mut next = m.clone();
        let had = next.reactions.iter().any(|r| r.0 == me && r.1 == emoji);
        next.reactions.retain(|r| !(r.0 == me && r.1 == emoji));
        if !had {
            next.reactions.push((me.clone(), emoji.to_owned()));
        }
        next
    });
    let (key, id, emoji) = (key.to_owned(), message_id.to_owned(), emoji.to_owned());
    spawn(move || {
        let group = key.starts_with("g:");
        if let Ok(res) = post(&action_path(group, &id, "react"), &json!({ "reaction": emoji }))
            && let Some(list) = res.get("reactions").filter(|r| r.is_array())
        {
            let list = parse_reactions(list);
            change(&key, &id, |m| Message { reactions: list.clone(), ..m.clone() });
        }
    });
}

/// Changes the text of one of your messages.
pub fn edit(key: &str, message_id: &str, text: &str) {
    let content = text.trim().to_owned();
    if net::ticket().is_none() || content.is_empty() || message_id.is_empty() || message_id.starts_with("local-") {
        return;
    }
    let (key, id) = (key.to_owned(), message_id.to_owned());
    spawn(move || {
        let group = key.starts_with("g:");
        match post(&action_path(group, &id, "edit"), &json!({ "content": content })) {
            Ok(res) => {
                let updated = parse_message(&lock(), res.get("message").unwrap_or(&Value::Null));
                change(&key, &id, |_| updated.clone());
            }
            Err(_) => with_conv(&key, |_, c| c.error = Some("Could not edit that message.".to_owned())),
        }
    });
}

/// Deletes a message (yours, or anyone's when you are a group admin). It stays as "This message was deleted".
pub fn delete(key: &str, message_id: &str) {
    if net::ticket().is_none() || message_id.is_empty() {
        return;
    }
    if message_id.starts_with("local-") {
        // Never sent: just drop it.
        with_conv(key, |_, c| c.messages.retain(|m| m.id != message_id));
        return;
    }
    let (key, id) = (key.to_owned(), message_id.to_owned());
    spawn(move || {
        let group = key.starts_with("g:");
        match post(&action_path(group, &id, "delete"), &json!({})) {
            Ok(res) => {
                let updated = parse_message(&lock(), res.get("message").unwrap_or(&Value::Null));
                change(&key, &id, |_| updated.clone());
            }
            Err(_) => with_conv(&key, |_, c| c.error = Some("Could not delete that message.".to_owned())),
        }
    });
}

/// Tells the other side we are typing, at most every 3 s.
pub fn typing_ping(key: &str) {
    {
        let mut s = lock();
        if net::ticket().is_none() || s.last_typing_sent.is_some_and(|t| t.elapsed() < Duration::from_secs(3)) {
            return;
        }
        s.last_typing_sent = Some(Instant::now());
    }
    let path = pref_path(key, "typing");
    spawn(move || {
        let _ = post(&path, &json!({ "isTyping": true }));
    });
}

fn send_prefs(key: &str, body: Value) {
    if net::ticket().is_none() {
        return;
    }
    let path = pref_path(key, "prefs");
    spawn(move || {
        let _ = post(&path, &body);
    });
}

/// Pins / unpins a chat (synced with the launcher).
pub fn set_pinned(key: &str, pinned: bool) {
    {
        let mut s = lock();
        if let Some(gid) = key.strip_prefix("g:") {
            for g in s.groups.iter_mut().filter(|g| g.id == gid) {
                g.pinned = pinned;
            }
            sort_groups(&mut s.groups);
        } else if let Some(fid) = key.strip_prefix("dm:") {
            for f in s.friends.iter_mut().filter(|f| f.id == fid) {
                f.pinned = pinned;
            }
            sort_friends(&mut s.friends);
        }
    }
    send_prefs(key, json!({ "pinned": pinned }));
    save_index_async();
}

/// Sets (or, when empty, removes) the nickname of a friend.
pub fn set_nickname(friend_id: &str, nickname: &str) {
    let nick = nickname.trim();
    {
        let mut s = lock();
        for f in s.friends.iter_mut().filter(|f| f.id == friend_id) {
            f.nickname = (!nick.is_empty()).then(|| nick.to_owned());
        }
        sort_friends(&mut s.friends);
    }
    send_prefs(&format!("dm:{friend_id}"), json!({ "nickname": nick }));
    save_index_async();
}

pub fn group_creating() -> bool {
    lock().group_creating
}

pub fn group_create_error() -> Option<String> {
    lock().group_create_error.clone()
}

pub fn clear_group_create_error() {
    lock().group_create_error = None;
}

pub fn take_created_group() -> Option<String> {
    lock().created_group_key.take()
}

/// Creates a new Relay group from in-game (POST /v1/social/relay/groups).
pub fn create_group(name: &str, member_ids: Vec<String>) {
    let trimmed = name.trim().to_owned();
    if trimmed.is_empty() {
        lock().group_create_error = Some("Enter a group name.".to_owned());
        return;
    }
    if net::ticket().is_none() {
        lock().group_create_error = Some("Sign in to Native to create a group.".to_owned());
        return;
    }
    {
        let mut s = lock();
        if s.group_creating {
            return;
        }
        s.group_creating = true;
        s.group_create_error = None;
    }
    spawn(move || {
        let body = json!({ "name": trimmed, "memberIds": member_ids });
        match post("/v1/social/relay/groups", &body) {
            Ok(o) => {
                let mut s = lock();
                s.group_creating = false;
                if let Some(go) = o.get("group") {
                    let g = parse_group(go);
                    let key = format!("g:{}", g.id);
                    s.groups.retain(|cur| cur.id != g.id);
                    s.groups.push(g);
                    sort_groups(&mut s.groups);
                    s.groups_loaded = true;
                    s.created_group_key = Some(key);
                    drop(s);
                    save_index_async();
                } else {
                    s.group_create_error = Some("Could not create group.".to_owned());
                }
            }
            Err(e) => {
                let mut s = lock();
                s.group_creating = false;
                s.group_create_error = Some(match e {
                    HttpError::Status(_, Some(m)) if !m.is_empty() => m,
                    _ => "Could not create group.".to_owned(),
                });
            }
        }
    });
}

fn cleared_file() -> Option<std::path::PathBuf> {
    super::game_dir().map(|d| d.join("config").join("native-relay-cleared.json"))
}

fn cleared_map(s: &mut Shared) -> &mut HashMap<String, i64> {
    s.cleared.get_or_insert_with(|| {
        cleared_file()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v.as_object().map(|o| o.iter().filter_map(|(k, v)| Some((k.clone(), v.as_i64()?))).collect()))
            .unwrap_or_default()
    })
}

/// Messages at or before this time are hidden in the chat (0 = none).
pub fn cleared_at(key: &str) -> i64 {
    let mut s = lock();
    let me = s.me_id.clone().unwrap_or_default();
    cleared_map(&mut s).get(&format!("{me}|{key}")).copied().unwrap_or(0)
}

/// "Delete chat": hides the history up to now on this device (the launcher's Clear chat does the same).
pub fn clear_chat(key: &str) {
    let mut s = lock();
    let me = s.me_id.clone().unwrap_or_default();
    // +1 s: tolerate small clock drift to the server.
    cleared_map(&mut s).insert(format!("{me}|{key}"), now_ms() + 1000);
    let body = json!(cleared_map(&mut s));
    drop(s);
    if let Some(f) = cleared_file() {
        if let Some(dir) = f.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(f, body.to_string());
    }
}

// ── friends: requests, search, remove, block, profiles ───────────────────

fn request_list(list: Option<&Value>) -> Vec<Request> {
    list.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|r| Request {
            id: str_of(r, "id").unwrap_or_default(),
            user_id: str_of(r, "userId").unwrap_or_default(),
            name: str_of(r, "name").unwrap_or_default(),
            skin: hash_of(str_of(r, "skinUrl").as_deref()),
            created_at: int_of(r, "createdAt"),
        })
        .collect()
}

pub fn load_requests() {
    if net::ticket().is_none() {
        return;
    }
    if let Ok(o) = get("/v1/social/requests") {
        let r = o.get("requests");
        let mut s = lock();
        s.received = request_list(r.and_then(|r| r.get("received")));
        s.sent = request_list(r.and_then(|r| r.get("sent")));
        s.requests = s.received.len() as i32;
        s.requests_loaded = true;
    }
}

fn person_list(list: Option<&Value>) -> Vec<Person> {
    list.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|u| Person {
            id: str_of(u, "id").or_else(|| str_of(u, "userId")).unwrap_or_default(),
            name: str_of(u, "name").or_else(|| str_of(u, "username")).unwrap_or_default(),
            skin: hash_of(str_of(u, "skinUrl").as_deref()),
        })
        .collect()
}

pub fn load_blocked() {
    if net::ticket().is_none() {
        return;
    }
    if let Ok(o) = get("/v1/social/blocked") {
        let mut s = lock();
        s.blocked = person_list(o.get("blocked"));
        s.blocked_loaded = true;
    }
}

pub fn unblock(user_id: &str) {
    if user_id.is_empty() || net::ticket().is_none() {
        return;
    }
    lock().blocked.retain(|b| b.id != user_id);
    let id = user_id.to_owned();
    spawn(move || {
        if let Err(e) = post("/v1/social/unblock", &json!({ "targetId": id })) {
            note(&error_text(&e, "Could not unblock that player."), true);
        }
        load_blocked();
    });
}

/// Players whose Native name contains `query` (2+ characters).
pub fn search(query: &str) {
    let q = query.trim().to_owned();
    {
        let mut s = lock();
        if q.chars().count() < 2 || net::ticket().is_none() {
            s.found.clear();
            s.found_for = q;
            return;
        }
        if q == s.found_for || s.searching {
            return;
        }
        s.searching = true;
        s.found_for = q.clone();
    }
    spawn(move || {
        if let Ok(o) = get(&format!("/v1/social/search?q={}", enc(&q))) {
            let mut s = lock();
            if s.found_for == q {
                s.found = person_list(o.get("results"));
            }
        }
        lock().searching = false;
    });
}

pub fn send_request(username: &str) {
    let name = username.trim().to_owned();
    if name.is_empty() || net::ticket().is_none() {
        return;
    }
    spawn(move || {
        match post("/v1/social/requests/send", &json!({ "username": name })) {
            Ok(res) => {
                let mutual = bool_of(&res, "mutual");
                note(&if mutual { format!("You and {name} are now friends.") } else { format!("Friend request sent to {name}.") }, false);
            }
            Err(e) => note(&error_text(&e, "Could not send the request."), true),
        }
        load_requests();
        load_friends();
    });
}

/// `action`: accept | decline | cancel
pub fn respond(request_id: &str, action: &str) {
    if request_id.is_empty() || net::ticket().is_none() {
        return;
    }
    {
        // Optimistic: drop it from the list straight away.
        let mut s = lock();
        s.received.retain(|r| r.id != request_id);
        s.sent.retain(|r| r.id != request_id);
        s.requests = s.received.len() as i32;
    }
    let (id, action) = (request_id.to_owned(), action.to_owned());
    spawn(move || {
        if let Err(e) = post("/v1/social/requests/respond", &json!({ "requestId": id, "action": action })) {
            note(&error_text(&e, "That didn't work. Try again."), true);
        }
        load_requests();
        load_friends();
    });
}

pub fn remove_friend(friend_id: &str) {
    lock().friends.retain(|f| f.id != friend_id);
    let id = friend_id.to_owned();
    spawn(move || {
        let ticket = net::ticket();
        if let Err(e) = net::delete(&full(&format!("/v1/social/friends/{}", enc(&id))), ticket.as_deref()) {
            note(&error_text(&e, "Could not remove that friend."), true);
        }
        load_friends();
    });
}

pub fn block(user_id: &str) {
    lock().friends.retain(|f| f.id != user_id);
    let id = user_id.to_owned();
    spawn(move || {
        match post("/v1/social/block", &json!({ "targetId": id })) {
            Ok(_) => note("Blocked. They can't message you or send requests.", false),
            Err(e) => note(&error_text(&e, "Could not block that player."), true),
        }
        load_friends();
        load_requests();
        load_blocked();
    });
}

/// The public profile for the side panel; `None` while it loads (fetched once per name).
pub fn profile(name: &str) -> Option<Profile> {
    if name.is_empty() {
        return None;
    }
    let key = name.to_lowercase();
    {
        let mut s = lock();
        if let Some(p) = s.profiles.get(&key) {
            return Some(p.clone());
        }
        if !s.profile_loading.insert(key.clone()) {
            return None;
        }
    }
    let name = name.to_owned();
    spawn(move || {
        let fallback = Profile { name: name.clone(), ..Profile::default() };
        let p = get(&format!("/v1/profiles/{}", enc(&name)))
            .ok()
            .and_then(|o| o.get("profile").cloned())
            .map(|o| {
                let badges = o
                    .get("badges")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|b| if b.is_object() { str_of(b, "name") } else { b.as_str().map(str::to_owned) })
                    .collect();
                let friends = match o.get("friends") {
                    Some(Value::Array(a)) => a.len() as i32,
                    Some(Value::Number(n)) => n.as_i64().unwrap_or(0) as i32,
                    Some(f @ Value::Object(_)) => int_of(f, "count") as i32,
                    _ => 0,
                };
                Profile { name: str_of(&o, "name").unwrap_or_else(|| name.clone()), bio: str_of(&o, "bio"), badges, joined_at: int_of(&o, "joinedAt"), friends, premium: bool_of(&o, "premium") }
            })
            .unwrap_or(fallback);
        let mut s = lock();
        s.profiles.insert(key.clone(), p);
        s.profile_loading.remove(&key);
    });
    None
}

/// GIF search through the Native API (Giphy proxy). Blocking. Each entry: (url, preview, title).
pub fn gifs(query: &str) -> Result<Vec<(String, String, String)>, HttpError> {
    let o = get(&format!("/v1/social/relay/gifs?limit=30&q={}", enc(query.trim())))?;
    Ok(o.get("gifs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|g| {
            let url = str_of(g, "url")?;
            let preview = str_of(g, "preview").unwrap_or_else(|| url.clone());
            Some((url, preview, str_of(g, "title").unwrap_or_default()))
        })
        .collect())
}

/// Uploads a picture (PNG/JPEG/GIF/WebP bytes) and posts it with a caption.
pub fn send_image(key: &str, bytes: Vec<u8>, mime: &str, caption: &str, reply_to: Option<&Message>) {
    if net::ticket().is_none() || bytes.is_empty() || bytes.len() > 8 * 1024 * 1024 {
        return;
    }
    let text = caption.trim().to_owned();
    let temp_id = format!("local-{}", now_ms() * 1000);
    let ext = match mime {
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => "png",
    };
    let name = format!("image-{}.{ext}", now_ms());
    let (me_id, me_name) = {
        let s = lock();
        (s.me_id.clone().unwrap_or_default(), s.me_name.clone().unwrap_or_default())
    };
    super::chat_media::put_local(&temp_id, bytes.clone());
    let mut pending = Message {
        id: temp_id.clone(),
        sender_id: me_id,
        sender_name: me_name,
        content: text.clone(),
        created_at: now_ms(),
        pending: true,
        media_name: Some(name.clone()),
        local_key: Some(temp_id.clone()),
        ..Message::default()
    };
    let reply_id = reply_to.filter(|r| !r.id.is_empty() && !r.id.starts_with("local-")).map(|r| {
        pending.reply = Some(quote(r));
        r.id.clone()
    });
    add_pending(key, pending);
    bump(&mut lock(), key, now_ms());
    let (key, mime) = (key.to_owned(), mime.to_owned());
    spawn(move || {
        let data = format!("data:{mime};base64,{}", base64(&bytes));
        let result = post("/v1/social/relay/upload", &json!({ "data": data, "name": name })).and_then(|res| {
            let url = str_of(&res, "url").ok_or_else(|| HttpError::Other("no url".into()))?;
            super::chat_media::remember(&url, &net::api(), bytes.clone());
            let mut body = json!({ "content": text, "mediaUrl": url, "mediaName": name, "mediaKind": "image", "isMedia": true });
            if let Some(id) = &reply_id {
                body["replyTo"] = json!(id);
            }
            post(&messages_path(&conversation(&key)), &body)
        });
        match result {
            Ok(res) => {
                let sent = parse_message(&lock(), res.get("message").unwrap_or(&Value::Null));
                replace_pending(&key, &temp_id, sent);
            }
            Err(_) => fail(&key, &temp_id),
        }
    });
}

fn mark_read_later(key: &str) {
    let key = key.to_owned();
    spawn(move || {
        let c = conversation(&key);
        if c.is_group() {
            let _ = post(&format!("/v1/social/relay/groups/{}/read", enc(c.target_id())), &json!({}));
        } else {
            let _ = get(&format!("{}?limit=1", messages_path(&c)));
        }
    });
}

// ── loading ──────────────────────────────────────────────────────────────

fn load_friends() {
    match get("/v1/mod/friends") {
        Ok(o) => {
            let mut s = lock();
            if let Some(a) = o.get("account").filter(|a| a.is_object()) {
                s.me_id = str_of(a, "id");
                s.me_name = str_of(a, "name");
                s.me_skin = skin_ref(str_of(a, "skin"), str_of(a, "minecraftUuid"));
            }
            let viewing = s.viewing.clone();
            let known: HashMap<String, i64> = s.friends.iter().map(|f| (f.id.clone(), f.last_msg)).collect();
            let mut list: Vec<Friend> = o
                .get("friends")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|f| {
                    let id = str_of(f, "id").unwrap_or_default();
                    let unread = if viewing.as_deref() == Some(format!("dm:{id}").as_str()) { 0 } else { int_of(f, "unread") as i32 };
                    Friend {
                        last_msg: int_of(f, "lastMessageAt").max(known.get(&id).copied().unwrap_or(0)),
                        name: str_of(f, "name").unwrap_or_default(),
                        nickname: str_of(f, "nickname").filter(|n| !n.is_empty()),
                        status: str_of(f, "status").unwrap_or_default(),
                        activity: str_of(f, "activity").unwrap_or_default(),
                        server_address: str_of(f, "serverAddress").unwrap_or_default(),
                        skin: skin_ref(str_of(f, "skin"), str_of(f, "mcUuid")),
                        online: bool_of(f, "online"),
                        unread,
                        pinned: bool_of(f, "pinned"),
                        last_seen: int_of(f, "lastSeen"),
                        id,
                    }
                })
                .collect();
            sort_friends(&mut list);
            s.friends = list;
            s.requests = o
                .get("requests")
                .and_then(|r| r.get("received"))
                .and_then(Value::as_array)
                .map_or(0, |a| a.len() as i32);
            s.friends_loaded = true;
            s.link = Link::Online;
            let first: Vec<String> = s.friends.iter().take(5).map(|f| format!("dm:{}", f.id)).collect();
            drop(s);
            ensure_disk_loaded();
            save_index_async();
            for key in first {
                prefetch(&key);
            }
        }
        Err(_) => {
            let mut s = lock();
            if !s.friends_loaded {
                s.link = Link::Offline;
            }
        }
    }
}

fn load_groups() {
    if let Ok(o) = get("/v1/social/relay/groups") {
        let mut s = lock();
        let viewing = s.viewing.clone();
        let mut list: Vec<Group> = o
            .get("groups")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|g| {
                let mut g = parse_group(g);
                if viewing.as_deref() == Some(format!("g:{}", g.id).as_str()) {
                    g.unread = 0;
                }
                g
            })
            .collect();
        sort_groups(&mut list);
        s.groups = list;
        s.groups_loaded = true;
        let first: Vec<String> = s.groups.iter().take(5).map(|g| format!("g:{}", g.id)).collect();
        drop(s);
        save_index_async();
        for key in first {
            prefetch(&key);
        }
    }
}

/// Warms a chat's latest page in the background (doesn't mark it read) so it opens instantly.
fn prefetch(key: &str) {
    hydrate_conversation(key);
    let skip = with_conv(key, |_, c| c.synced || c.loading);
    if skip || net::ticket().is_none() {
        return;
    }
    let key = key.to_owned();
    spawn(move || {
        let c = conversation(&key);
        if c.synced || c.loading {
            return;
        }
        if let Ok(page) = get(&format!("{}?limit=60&markRead=0", messages_path(&c))) {
            let mut to_warm = Vec::new();
            let mut should_save = false;
            with_conv(&key, |s, c| {
                if c.synced {
                    return;
                }
                let latest = parse_messages(s, page.get("messages"));
                let kept_older = !latest.is_empty() && c.messages.first().is_some_and(|m| m.created_at < latest[0].created_at);
                c.has_more = if kept_older { c.has_more } else { bool_of(&page, "hasMore") };
                c.messages = merge_latest(&c.messages, latest);
                c.loaded = true;
                c.synced = true;
                to_warm = c.messages.clone();
                should_save = true;
            });
            if should_save {
                warm_media(&to_warm);
                save_conversation_async(&key);
            }
        }
    });
}

// ── live stream ──────────────────────────────────────────────────────────

fn stream_loop() {
    let mut backoff = Duration::from_secs(2);
    let mut loaded = false;
    loop {
        // Not signed in (guest, checking, expired): wait for the session instead of hammering Native.
        if !net::signed_in() {
            if net::ticket().is_none() {
                lock().link = Link::NoAccount;
            }
            loaded = false;
            std::thread::sleep(Duration::from_millis(1500));
            continue;
        }
        if !loaded {
            loaded = true;
            ensure_disk_loaded();
            {
                let mut s = lock();
                if s.link == Link::NoAccount {
                    s.link = Link::Connecting;
                }
            }
            refresh();
        }
        let started = Instant::now();
        let _ = stream();
        if started.elapsed() > Duration::from_secs(60) {
            backoff = Duration::from_secs(2);
        }
        {
            let mut s = lock();
            if s.link == Link::Online {
                s.link = Link::Connecting;
            }
        }
        std::thread::sleep(backoff);
        backoff = (backoff * 2).min(Duration::from_secs(60));
        spawn(load_friends);
    }
}

fn stream() -> Result<(), HttpError> {
    let ticket = net::ticket();
    let url = full("/v1/mod/stream?relay=1&activity=Playing%20Minecraft");
    let reader = match net::open_stream(&url, ticket.as_deref()) {
        Ok(r) => r,
        Err(HttpError::Unauthorized) => {
            lock().link = Link::NoAccount;
            if let Some(t) = &ticket {
                net::rejected(t);
            }
            return Err(HttpError::Unauthorized);
        }
        Err(e) => return Err(e),
    };
    let mut lines = BufReader::new(reader).lines();
    let (mut kind, mut data) = ("message".to_owned(), String::new());
    while let Some(Ok(line)) = lines.next() {
        if line.is_empty() {
            if !data.is_empty()
                && let Ok(v) = serde_json::from_str::<Value>(&data)
                && v.is_object()
            {
                handle(&kind, &v);
            }
            kind = "message".to_owned();
            data.clear();
        } else if let Some(rest) = line.strip_prefix("event:") {
            kind = rest.trim().to_owned();
        } else if let Some(rest) = line.strip_prefix("data:") {
            data.push_str(rest.trim());
        }
    }
    Ok(())
}

fn handle(kind: &str, p: &Value) {
    match kind {
        "hello" => {
            lock().link = Link::Online;
            refresh();
            super::voice::refresh_calls();
            let reopen: Vec<String> = {
                let s = lock();
                s.conversations.values().filter(|c| c.loaded && s.viewing.as_deref() == Some(c.key.as_str())).map(|c| c.key.clone()).collect()
            };
            for key in reopen {
                open(&key);
            }
        }
        // Nearby players' emotes, pushed live so they start without waiting for a poll.
        "voice:state" | "voice:ring" | "voice:ring-end" | "voice:declined" | "voice:ended" => super::voice::on_event(kind, p),
        "emote:play" => horizon_client::native_emotes::pushed_play(p),
        "emote:stop" => horizon_client::native_emotes::pushed_stop(p),
        "marker:place" => super::markers_module::on_remote_place(p),
        "marker:remove" => super::markers_module::on_remote_remove(p),
        "presence" => {
            let id = str_of(p, "userId").unwrap_or_default();
            let mut s = lock();
            for f in s.friends.iter_mut().filter(|f| f.id == id) {
                let status = str_of(p, "status").unwrap_or_default();
                let on = status != "offline";
                let seen = int_of(p, "lastSeen");
                if !on {
                    f.last_seen = if seen > 0 { seen } else { now_ms() };
                }
                f.online = on;
                f.status = status;
                f.activity = str_of(p, "activity").unwrap_or_default();
                f.server_address = str_of(p, "serverAddress").unwrap_or_default();
            }
            sort_friends(&mut s.friends);
        }
        "friends:changed" | "request:changed" => {
            let (action, actor, actor_name, receiver) =
                (str_of(p, "action"), str_of(p, "actorId"), str_of(p, "actorName"), str_of(p, "receiverId"));
            spawn(move || {
                load_requests();
                load_friends();
                request_notice(action.as_deref(), actor.as_deref(), actor_name.as_deref(), receiver.as_deref());
            });
        }
        "blocks:changed" => spawn(load_blocked),
        "group:created" | "group:updated" | "group:removed" | "group:deleted" => spawn(load_groups),
        "message:new" => {
            if let Some(m) = p.get("message") {
                let sender = str_of(m, "senderId").unwrap_or_default();
                let receiver = str_of(m, "receiverId").unwrap_or_default();
                let me = lock().me_id.clone().unwrap_or_default();
                let other = if sender == me { receiver } else { sender.clone() };
                incoming(&format!("dm:{other}"), m, &sender);
            }
        }
        "group:message" => {
            if let Some(m) = p.get("message") {
                incoming(&format!("g:{}", str_of(p, "groupId").unwrap_or_default()), m, &str_of(m, "senderId").unwrap_or_default());
            }
        }
        "message:updated" | "group:message:updated" => {
            if let Some(m) = p.get("message") {
                let id = str_of(m, "id").unwrap_or_default();
                let mut touched = Vec::new();
                {
                    let mut s = lock();
                    let keys: Vec<String> = s.conversations.keys().cloned().collect();
                    for key in keys {
                        let Some(mut c) = s.conversations.remove(&key) else { continue };
                        if let Some(i) = c.messages.iter().position(|x| x.id == id) {
                            let old = c.messages[i].clone();
                            let mut fresh = parse_message(&s, m);
                            if fresh.local_key.is_none() {
                                fresh.local_key = old.local_key.clone();
                            }
                            fresh.read = fresh.read || old.read;
                            c.messages[i] = fresh;
                            touched.push(key.clone());
                        }
                        s.conversations.insert(key, c);
                    }
                }
                for k in touched {
                    save_conversation_async(&k);
                }
            }
        }
        "message:reaction" => {
            let list = p.get("reactions").map(parse_reactions).unwrap_or_default();
            if let Some(mid) = str_of(p, "messageId") {
                let keys: Vec<String> = lock().conversations.keys().filter(|k| k.starts_with("dm:")).cloned().collect();
                for key in keys {
                    change(&key, &mid, |m| Message { reactions: list.clone(), ..m.clone() });
                }
            }
        }
        "group:message:reaction" => {
            let list = p.get("reactions").map(parse_reactions).unwrap_or_default();
            if let Some(mid) = str_of(p, "messageId") {
                change(&format!("g:{}", str_of(p, "groupId").unwrap_or_default()), &mid, |m| Message { reactions: list.clone(), ..m.clone() });
            }
        }
        "thread:read" => {
            // You read this chat in the launcher (or elsewhere): no unread badge or pending notice for it in game.
            if let Some(tid) = str_of(p, "threadId")
                && str_of(p, "kind").as_deref() == Some("dm")
            {
                let key = format!("dm:{tid}");
                let mut s = lock();
                clear_unread(&mut s, &key);
                s.notices.retain(|n| n.conv_key.as_deref() != Some(key.as_str()));
            }
        }
        "message:read" => {
            let ids: HashSet<String> = p
                .get("messageIds")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|e| e.as_str().map(str::to_owned))
                .collect();
            let mut touched = Vec::new();
            {
                let mut s = lock();
                for c in s.conversations.values_mut().filter(|c| !c.is_group()) {
                    let mut hit = false;
                    for m in c.messages.iter_mut().filter(|m| ids.contains(&m.id)) {
                        if !m.read {
                            m.read = true;
                            hit = true;
                        }
                    }
                    if hit {
                        touched.push(c.key.clone());
                    }
                }
            }
            for k in touched {
                save_conversation_async(&k);
            }
        }
        "typing" => {
            let uid = str_of(p, "userId").unwrap_or_default();
            set_typing(&format!("dm:{uid}"), &uid, bool_of(p, "isTyping"));
        }
        "group:typing" => {
            set_typing(&format!("g:{}", str_of(p, "groupId").unwrap_or_default()), &str_of(p, "userId").unwrap_or_default(), bool_of(p, "isTyping"));
        }
        _ => {}
    }
}

fn set_typing(conv: &str, uid: &str, on: bool) {
    if uid.is_empty() {
        return;
    }
    let key = format!("{conv}|{uid}");
    let mut s = lock();
    if on {
        s.typing.insert(key, Instant::now() + Duration::from_secs(6));
    } else {
        s.typing.remove(&key);
    }
}

/// A toast for a friend request someone sent you, or one they accepted.
fn request_notice(action: Option<&str>, actor: Option<&str>, actor_name: Option<&str>, receiver: Option<&str>) {
    let (Some(actor), Some(actor_name)) = (actor, actor_name) else { return };
    let mut s = lock();
    if s.me_id.as_deref() == Some(actor) {
        return;
    }
    if action == Some("sent") && receiver.is_some() && receiver == s.me_id.as_deref() {
        let skin = s.received.iter().find(|r| r.user_id == actor).and_then(|r| r.skin.clone());
        s.notices.push_back(Notice {
            conv_key: Some(REQUESTS_KEY.to_owned()),
            title: actor_name.to_owned(),
            body: "Sent you a friend request".to_owned(),
            avatar_name: actor_name.to_owned(),
            skin,
            at: now_ms(),
        });
    } else if action == Some("accepted") {
        let f = s.friends.iter().find(|f| f.id == actor).cloned();
        s.notices.push_back(Notice {
            conv_key: Some(format!("dm:{actor}")),
            title: f.as_ref().map_or_else(|| actor_name.to_owned(), |f| f.display().to_owned()),
            body: "Accepted your friend request".to_owned(),
            avatar_name: actor_name.to_owned(),
            skin: f.and_then(|f| f.skin),
            at: now_ms(),
        });
    }
}

fn incoming(key: &str, m: &Value, sender: &str) {
    hydrate_conversation(key);
    let mut s = lock();
    let mut c = s.conversations.remove(key).unwrap_or_else(|| Conversation { key: key.to_owned(), ..Conversation::default() });
    let msg = parse_message(&s, m);
    s.typing.remove(&format!("{key}|{sender}"));
    let mine = s.me_id.as_deref() == Some(sender);
    if c.messages.iter().any(|x| x.id == msg.id) {
        // Delivered twice (stream + catch-up): already shown and counted.
        s.conversations.insert(key.to_owned(), c);
        return;
    }
    let media = msg.media_url.is_some();
    let mut next = Vec::with_capacity(c.messages.len() + 1);
    let mut replaced = false;
    for x in c.messages.drain(..) {
        // A pending copy of our own message is replaced by the real one, in place (keeps its local picture).
        if !replaced && mine && x.pending && x.content == msg.content && media == (x.media_url.is_some() || x.local_key.is_some()) {
            replaced = true;
            let mut real = msg.clone();
            if real.local_key.is_none() {
                real.local_key = x.local_key.clone();
            }
            next.push(real);
            continue;
        }
        next.push(x);
    }
    if !replaced {
        next.push(msg.clone());
    }
    c.messages = next;
    c.loaded = true;
    s.conversations.insert(key.to_owned(), c);
    let media_url = (!msg.deleted).then(|| msg.media_url.clone()).flatten();
    if mine {
        bump(&mut s, key, msg.created_at);
        drop(s);
        if let Some(u) = media_url {
            let _ = super::chat_media::get(&u, &net::api());
        }
        save_conversation_async(key);
        save_index_async();
        return;
    }
    let watching = s.viewing.as_deref() == Some(key);
    let mut mark_read = false;
    if let Some(gid) = key.strip_prefix("g:") {
        let mut hit: Option<Group> = None;
        for g in s.groups.iter_mut().filter(|g| g.id == gid) {
            g.last_text = Some(msg.content.clone());
            g.last_sender = Some(msg.sender_name.clone());
            g.last_at = msg.created_at;
            g.unread = if watching { 0 } else { g.unread + 1 };
            hit = Some(g.clone());
        }
        sort_groups(&mut s.groups);
        if watching {
            mark_read = true;
        } else if !msg.system {
            let skin = s.friends.iter().find(|f| f.id == sender).and_then(|f| f.skin.clone());
            s.notices.push_back(Notice {
                conv_key: Some(key.to_owned()),
                title: hit.map_or_else(|| "Group".to_owned(), |g| g.name),
                body: format!("{}: {}", msg.sender_name, msg.content),
                avatar_name: msg.sender_name.clone(),
                skin,
                at: now_ms(),
            });
        }
    } else if let Some(fid) = key.strip_prefix("dm:") {
        let mut hit: Option<Friend> = None;
        for f in s.friends.iter_mut().filter(|f| f.id == fid) {
            f.unread = if watching { 0 } else { f.unread + 1 };
            f.last_msg = f.last_msg.max(msg.created_at);
            hit = Some(f.clone());
        }
        sort_friends(&mut s.friends);
        if watching {
            mark_read = true;
        } else {
            s.notices.push_back(Notice {
                conv_key: Some(key.to_owned()),
                title: hit.as_ref().map_or_else(|| msg.sender_name.clone(), |f| f.display().to_owned()),
                body: msg.content.clone(),
                avatar_name: hit.as_ref().map_or_else(|| msg.sender_name.clone(), |f| f.name.clone()),
                skin: hit.and_then(|f| f.skin),
                at: now_ms(),
            });
        }
    }
    drop(s);
    if mark_read {
        mark_read_later(key);
    }
    if let Some(u) = media_url {
        let _ = super::chat_media::get(&u, &net::api());
    }
    save_conversation_async(key);
    save_index_async();
}

// ── local disk cache (instant load of chats, friends, groups & media) ────

const MAX_CACHED_MESSAGES: usize = 200;

fn safe_key(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()
}

fn cache_account_key(s: &Shared) -> Option<String> {
    let raw = s
        .me_id
        .as_deref()
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            net::account().and_then(|a| {
                a.id.filter(|v| !v.is_empty()).or_else(|| a.name.filter(|v| !v.is_empty()))
            })
        })?;
    Some(safe_key(&raw))
}

fn cache_dir_for(account_key: &str) -> Option<std::path::PathBuf> {
    if cfg!(test) {
        return None;
    }
    super::game_dir().map(|d| d.join(".native").join("relay-cache").join(account_key))
}

fn conv_file_name(key: &str) -> String {
    format!("conv-{}.json", safe_key(key))
}

fn warm_media(messages: &[Message]) {
    if cfg!(test) {
        return;
    }
    let api = net::api();
    let mut warmed = 0usize;
    for m in messages.iter().rev() {
        if warmed >= 16 {
            break;
        }
        if !m.deleted
            && let Some(u) = &m.media_url
        {
            let _ = super::chat_media::get(u, &api);
            warmed += 1;
        }
    }
}

fn ensure_disk_loaded() {
    let (acc, dir) = {
        let s = lock();
        let Some(acc) = cache_account_key(&s) else { return };
        if s.disk_loaded_account.as_deref() == Some(acc.as_str()) {
            return;
        }
        let Some(dir) = cache_dir_for(&acc) else { return };
        (acc, dir)
    };
    let idx = dir.join("index.json");
    let text = std::fs::read_to_string(&idx).ok().filter(|t| t.len() < 4 * 1024 * 1024);
    let mut s = lock();
    if s.disk_loaded_account.as_deref() == Some(acc.as_str()) {
        return;
    }
    s.disk_loaded_account = Some(acc);
    let Some(root) = text.and_then(|t| serde_json::from_str::<Value>(&t).ok()) else {
        return;
    };
    if s.me_id.is_none() {
        s.me_id = str_of(&root, "meId");
    }
    if s.me_name.is_none() {
        s.me_name = str_of(&root, "meName");
    }
    if s.me_skin.is_none() {
        s.me_skin = str_of(&root, "meSkin");
    }
    if !s.friends_loaded
        && let Some(arr) = root.get("friends").and_then(Value::as_array)
    {
        let mut list: Vec<Friend> = arr
            .iter()
            .filter_map(|f| {
                let id = str_of(f, "id")?;
                Some(Friend {
                    id,
                    name: str_of(f, "name").unwrap_or_default(),
                    nickname: str_of(f, "nickname").filter(|n| !n.is_empty()),
                    status: "offline".to_owned(),
                    activity: String::new(),
                    server_address: String::new(),
                    skin: str_of(f, "skin"),
                    online: false,
                    unread: int_of(f, "unread") as i32,
                    pinned: bool_of(f, "pinned"),
                    last_seen: int_of(f, "lastSeen"),
                    last_msg: int_of(f, "lastMsg"),
                })
            })
            .collect();
        if !list.is_empty() {
            sort_friends(&mut list);
            s.friends = list;
            s.friends_loaded = true;
        }
    }
    if !s.groups_loaded
        && let Some(arr) = root.get("groups").and_then(Value::as_array)
    {
        let mut list: Vec<Group> = arr
            .iter()
            .filter_map(|g| {
                let id = str_of(g, "id")?;
                let member_list = g
                    .get("members")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|m| Some((str_of(m, "id")?, str_of(m, "name").unwrap_or_else(|| "Member".to_owned()))))
                    .collect();
                Some(Group {
                    id,
                    name: str_of(g, "name").unwrap_or_default(),
                    last_text: str_of(g, "lastText"),
                    last_sender: str_of(g, "lastSender"),
                    last_at: int_of(g, "lastAt"),
                    unread: int_of(g, "unread") as i32,
                    members: int_of(g, "memberCount") as i32,
                    member_list,
                    role: str_of(g, "role").unwrap_or_else(|| "member".to_owned()),
                    pinned: bool_of(g, "pinned"),
                })
            })
            .collect();
        if !list.is_empty() {
            sort_groups(&mut list);
            s.groups = list;
            s.groups_loaded = true;
        }
    }
}

fn serialize_message(m: &Message) -> Value {
    let mut o = json!({
        "id": m.id,
        "senderId": m.sender_id,
        "senderName": m.sender_name,
        "content": m.content,
        "createdAt": m.created_at,
    });
    if m.system {
        o["system"] = json!(true);
    }
    if m.deleted {
        o["deleted"] = json!(true);
    }
    if let Some(u) = &m.media_url {
        o["mediaUrl"] = json!(u);
    }
    if let Some(n) = &m.media_name {
        o["mediaName"] = json!(n);
    }
    if m.edited {
        o["edited"] = json!(true);
    }
    if m.read {
        o["read"] = json!(true);
    }
    if let Some(r) = &m.reply {
        o["reply"] = json!({
            "id": r.id,
            "senderName": r.sender_name,
            "content": r.content,
            "media": r.media,
            "deleted": r.deleted,
        });
    }
    if !m.reactions.is_empty() {
        let rx: Vec<Value> = m.reactions.iter().map(|(u, e)| json!({ "u": u, "e": e })).collect();
        o["reactions"] = Value::Array(rx);
    }
    o
}

fn deserialize_message(o: &Value) -> Option<Message> {
    let id = str_of(o, "id").filter(|s| !s.is_empty())?;
    let reply = o.get("reply").filter(|r| r.is_object()).map(|r| Reply {
        id: str_of(r, "id").unwrap_or_default(),
        sender_name: str_of(r, "senderName").unwrap_or_default(),
        content: str_of(r, "content").unwrap_or_default(),
        media: bool_of(r, "media"),
        deleted: bool_of(r, "deleted"),
    });
    let reactions = o
        .get("reactions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|r| Some((str_of(r, "u").unwrap_or_default(), str_of(r, "e")?)))
        .collect();
    Some(Message {
        id,
        sender_id: str_of(o, "senderId").unwrap_or_default(),
        sender_name: str_of(o, "senderName").unwrap_or_default(),
        content: str_of(o, "content").unwrap_or_default(),
        created_at: int_of(o, "createdAt"),
        pending: false,
        failed: false,
        system: bool_of(o, "system"),
        deleted: bool_of(o, "deleted"),
        media_url: str_of(o, "mediaUrl"),
        media_name: str_of(o, "mediaName"),
        local_key: None,
        reply,
        reactions,
        edited: bool_of(o, "edited"),
        read: bool_of(o, "read"),
    })
}

fn hydrate_conversation(key: &str) {
    let file = {
        let mut s = lock();
        if s.disk_hydrated.contains(key) {
            return;
        }
        let Some(acc) = cache_account_key(&s) else { return };
        let Some(dir) = cache_dir_for(&acc) else { return };
        s.disk_hydrated.insert(key.to_owned());
        dir.join(conv_file_name(key))
    };
    let Some(text) = std::fs::read_to_string(&file).ok().filter(|t| t.len() < 8 * 1024 * 1024) else {
        return;
    };
    let Ok(root) = serde_json::from_str::<Value>(&text) else { return };
    let cached: Vec<Message> = root
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(deserialize_message)
        .collect();
    let has_more = bool_of(&root, "hasMore");
    let to_warm = with_conv(key, |_, c| {
        if !c.loaded && !cached.is_empty() {
            c.messages = if c.messages.is_empty() { cached.clone() } else { merge_latest(&cached, c.messages.clone()) };
            c.has_more = has_more;
            c.loaded = true;
            c.error = None;
            return c.messages.clone();
        }
        Vec::new()
    });
    if !to_warm.is_empty() {
        warm_media(&to_warm);
    }
}

fn atomic_write(path: &std::path::Path, data: &[u8]) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, data).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

fn save_index_async() {
    if cfg!(test) {
        return;
    }
    let payload = {
        let s = lock();
        let Some(acc) = cache_account_key(&s) else { return };
        let Some(dir) = cache_dir_for(&acc) else { return };
        let friends: Vec<Value> = s
            .friends
            .iter()
            .map(|f| {
                json!({
                    "id": f.id,
                    "name": f.name,
                    "nickname": f.nickname,
                    "skin": f.skin,
                    "unread": f.unread,
                    "pinned": f.pinned,
                    "lastSeen": f.last_seen,
                    "lastMsg": f.last_msg,
                })
            })
            .collect();
        let groups: Vec<Value> = s
            .groups
            .iter()
            .map(|g| {
                let members: Vec<Value> = g.member_list.iter().map(|(id, name)| json!({ "id": id, "name": name })).collect();
                json!({
                    "id": g.id,
                    "name": g.name,
                    "lastText": g.last_text,
                    "lastSender": g.last_sender,
                    "lastAt": g.last_at,
                    "unread": g.unread,
                    "memberCount": g.members,
                    "role": g.role,
                    "pinned": g.pinned,
                    "members": members,
                })
            })
            .collect();
        let root = json!({
            "version": 1,
            "meId": s.me_id,
            "meName": s.me_name,
            "meSkin": s.me_skin,
            "friends": friends,
            "groups": groups,
        });
        (dir.join("index.json"), root.to_string())
    };
    spawn(move || atomic_write(&payload.0, payload.1.as_bytes()));
}

fn save_conversation_async(key: &str) {
    if cfg!(test) {
        return;
    }
    let payload = {
        let s = lock();
        let Some(acc) = cache_account_key(&s) else { return };
        let Some(dir) = cache_dir_for(&acc) else { return };
        let Some(c) = s.conversations.get(key) else { return };
        let saved: Vec<&Message> = c.messages.iter().filter(|m| !m.pending && !m.failed && !m.id.starts_with("local-")).collect();
        let start = saved.len().saturating_sub(MAX_CACHED_MESSAGES);
        let msgs: Vec<Value> = saved[start..].iter().map(|m| serialize_message(m)).collect();
        let root = json!({
            "version": 1,
            "key": c.key,
            "hasMore": c.has_more,
            "messages": msgs,
        });
        (dir.join(conv_file_name(key)), root.to_string())
    };
    spawn(move || atomic_write(&payload.0, payload.1.as_bytes()));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(id: &str, at: i64) -> Message {
        Message { id: id.into(), created_at: at, ..Message::default() }
    }

    #[test]
    fn activities_read_like_the_launcher_writes_them() {
        assert_eq!(nice_activity("In-game: Hypixel", "in-game").as_deref(), Some("Playing on Hypixel"));
        assert_eq!(nice_activity("In-game: Singleplayer", "in-game").as_deref(), Some("Playing Singleplayer"));
        assert_eq!(nice_activity("In-game: Menus", "in-game").as_deref(), Some("In the menus"));
        assert_eq!(nice_activity("In Launcher", "online").as_deref(), Some("In the launcher"));
        assert_eq!(nice_activity("", "online"), None);
        assert_eq!(nice_activity("", "in-game").as_deref(), Some("Playing Minecraft"));
    }

    #[test]
    fn the_latest_page_keeps_older_pages_and_pending_sends() {
        let have = vec![msg("a", 1), msg("b", 2), msg("c", 3), Message { pending: true, ..msg("local-1", 9) }];
        let page = vec![msg("b", 2), msg("c", 3), msg("d", 4)];
        let ids: Vec<String> = merge_latest(&have, page).into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["a", "b", "c", "d", "local-1"]);
        assert_eq!(merge_latest(&[], vec![msg("x", 1)]).len(), 1);
    }

    #[test]
    fn friends_sort_pinned_then_newest_chat() {
        let mut list = vec![
            Friend { id: "a".into(), name: "Zed".into(), last_msg: 5, ..Friend::default() },
            Friend { id: "b".into(), name: "Amy".into(), last_msg: 9, ..Friend::default() },
            Friend { id: "c".into(), name: "Bob".into(), pinned: true, ..Friend::default() },
        ];
        sort_friends(&mut list);
        let ids: Vec<&str> = list.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(ids, ["c", "b", "a"]);
    }

    #[test]
    fn dates_and_urls() {
        assert_eq!(date(1_793_887_380_000), "5 Nov 2026");
        assert_eq!(enc("a b/ü"), "a%20b%2F%C3%BC");
        assert_eq!(skin_ref(None, Some("AB-CD".into())).as_deref(), Some("mj:abcd"));
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
        assert_eq!(pref_path("g:7", "read"), "/v1/social/relay/groups/7/read");
        assert_eq!(pref_path("dm:9", "typing"), "/v1/social/relay/dm/9/typing");
    }

    #[test]
    fn groups_read_the_servers_json() {
        let g = parse_group(&json!({"id": "g1", "name": "Squad", "memberCount": 3, "unreadCount": 2, "role": "admin",
            "lastMessage": {"content": "hi", "senderName": "Amy", "createdAt": 5},
            "members": [{"id": "u1", "name": "Amy"}, {"id": "u2", "username": "Bob"}]}));
        assert_eq!((g.members, g.unread, g.last_at), (3, 2, 5));
        assert!(g.can_moderate());
        assert_eq!(g.member_list[1].1, "Bob");
    }

    #[test]
    fn cached_messages_round_trip_with_media_replies_and_reactions() {
        let orig = Message {
            id: "m42".into(),
            sender_id: "u1".into(),
            sender_name: "Alex".into(),
            content: "check this gif".into(),
            created_at: 1_793_887_380_000,
            media_url: Some("https://media.giphy.com/media/abc/giphy.gif".into()),
            media_name: Some("cat.gif".into()),
            reply: Some(Reply { id: "m41".into(), sender_name: "Sam".into(), content: "send it".into(), media: false, deleted: false }),
            reactions: vec![("u2".into(), "fire".into())],
            edited: true,
            read: true,
            ..Message::default()
        };
        let raw = serialize_message(&orig);
        let back = deserialize_message(&raw).expect("deserialized");
        assert_eq!(back, orig);
    }
}
