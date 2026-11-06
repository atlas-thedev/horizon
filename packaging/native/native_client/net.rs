//! `Session` and `Http`: the game's sign-in to Native, shared by Relay, the
//! locker and the music page, and the little HTTP layer they use. The launcher
//! writes a short-lived game ticket to `.native/session.json` and keeps it
//! fresh while the game runs. This checks the ticket, retries when the
//! network is down, notices a refreshed ticket on disk, and reports a rejected
//! one instead of silently dropping to guest mode.
#![allow(dead_code)]

use horizon_client::native_ui::Image;
use serde_json::Value;
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

pub const DEFAULT_API: &str = "https://api.playnative.net";
const WATCH: Duration = Duration::from_secs(15);
const RECHECK: Duration = Duration::from_secs(600);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// No ticket from the launcher (started outside the Native Client, or an offline account).
    Guest,
    /// Checking the ticket with Native.
    Checking,
    SignedIn,
    /// Native refused the ticket and the launcher has not written a new one yet.
    Expired,
    /// Native could not be reached; retrying.
    Offline,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Account {
    pub id: Option<String>,
    pub name: Option<String>,
    pub uuid: Option<String>,
    pub model: Option<String>,
}

struct SessionState {
    status: Status,
    ticket: Option<String>,
    account: Option<Account>,
    api: String,
    dir: Option<PathBuf>,
    stamp: Option<SystemTime>,
    verified_at: Option<Instant>,
    next_try: Instant,
    failures: u32,
    started: bool,
}

fn state() -> &'static (Mutex<SessionState>, Condvar) {
    static STATE: OnceLock<(Mutex<SessionState>, Condvar)> = OnceLock::new();
    STATE.get_or_init(|| {
        (
            Mutex::new(SessionState {
                status: Status::Guest,
                ticket: None,
                account: None,
                api: DEFAULT_API.to_owned(),
                dir: None,
                stamp: None,
                verified_at: None,
                next_try: Instant::now(),
                failures: 0,
                started: false,
            }),
            Condvar::new(),
        )
    })
}

fn lock() -> std::sync::MutexGuard<'static, SessionState> {
    state().0.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn status() -> Status {
    lock().status
}

pub fn signed_in() -> bool {
    status() == Status::SignedIn
}

/// The ticket to send, or `None` when there is none worth sending (guest or rejected).
pub fn ticket() -> Option<String> {
    let s = lock();
    match s.status {
        Status::Guest | Status::Expired => None,
        _ => s.ticket.clone(),
    }
}

pub fn api() -> String {
    lock().api.clone()
}

/// The Native account the ticket belongs to (`None` until it has been checked once).
pub fn account() -> Option<Account> {
    lock().account.clone()
}

/// Only Native over https (or loopback http for development) may receive the ticket.
pub fn api_allowed(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    let parsed = lower
        .strip_prefix("http://")
        .map(|rest| (rest, true))
        .or_else(|| lower.strip_prefix("https://").map(|rest| (rest, false)));
    let Some((rest, plain)) = parsed else { return false };
    let authority = rest.split('/').next().unwrap_or("");
    if authority.contains('@') {
        return false;
    }
    let host = authority.split(':').next().unwrap_or("");
    let loopback = host == "localhost" || host == "127.0.0.1";
    if plain {
        return loopback;
    }
    loopback || host == "playnative.net" || host.ends_with(".playnative.net")
}

struct Handoff {
    ticket: String,
    api: String,
}

fn session_file(dir: &std::path::Path) -> PathBuf {
    let file = dir.join(".native").join("session.json");
    if file.is_file() { file } else { dir.join(".noctra").join("session.json") }
}

fn read_handoff(dir: &std::path::Path) -> Option<Handoff> {
    let file = session_file(dir);
    if std::fs::metadata(&file).ok()?.len() > 16 * 1024 {
        return None;
    }
    let value: Value = serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()?;
    let ticket = value.get("ticket")?.as_str()?.trim().to_owned();
    if !ticket.starts_with("nmt1.") {
        return None;
    }
    let api = value.get("api").and_then(Value::as_str).unwrap_or(DEFAULT_API).trim().trim_end_matches('/').to_owned();
    api_allowed(&api).then_some(Handoff { ticket, api })
}

fn stamp(dir: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(session_file(dir)).ok()?.modified().ok()
}

fn adopt(s: &mut SessionState, handoff: Option<Handoff>) {
    match handoff {
        None => {
            s.ticket = None;
            s.status = Status::Guest;
        }
        Some(h) => {
            s.ticket = Some(h.ticket);
            s.api = h.api;
            s.status = Status::Checking;
            s.failures = 0;
            s.next_try = Instant::now();
        }
    }
}

fn wake() {
    state().1.notify_all();
}

/// Starts checking the hand-off the launcher left (idempotent).
pub fn start(dir: Option<PathBuf>) {
    let Some(dir) = dir else { return };
    {
        let mut s = lock();
        if s.started {
            return;
        }
        s.started = true;
        s.stamp = stamp(&dir);
        let handoff = read_handoff(&dir);
        adopt(&mut s, handoff);
        s.dir = Some(dir);
    }
    let _ = std::thread::Builder::new().name("Native session".into()).spawn(run);
}

/// Called by anything that got HTTP 401 with `used`. Looks for a newer ticket from the launcher first;
/// only when there is none is the session marked expired.
pub fn rejected(used: &str) {
    let mut s = lock();
    if s.ticket.as_deref() != Some(used) {
        return; // An older ticket: the current one is already being handled.
    }
    let fresh = s.dir.as_deref().and_then(read_handoff);
    s.stamp = s.dir.as_deref().and_then(stamp);
    match fresh {
        Some(h) if h.ticket != used => adopt(&mut s, Some(h)),
        _ => {
            if s.status != Status::Expired {
                s.status = Status::Expired;
            }
        }
    }
    drop(s);
    wake();
}

/// Called by anything whose request failed for network reasons while signed in.
pub fn unreachable() {
    let mut s = lock();
    if s.status == Status::SignedIn && s.verified_at.is_none_or(|v| v.elapsed() > Duration::from_secs(30)) {
        s.status = Status::Checking;
        s.next_try = Instant::now();
        drop(s);
        wake();
    }
}

/// "Try again" in the UI.
pub fn retry() {
    let mut s = lock();
    let fresh = s.dir.as_deref().and_then(read_handoff);
    s.stamp = s.dir.as_deref().and_then(stamp);
    if let Some(h) = fresh
        && (s.ticket.as_deref() != Some(h.ticket.as_str()) || s.status != Status::Expired)
    {
        adopt(&mut s, Some(h));
    }
    s.failures = 0;
    s.next_try = Instant::now();
    drop(s);
    wake();
}

pub fn lock_title() -> &'static str {
    match status() {
        Status::Checking => "Signing in\u{2026}",
        Status::Expired => "Your Native sign-in expired",
        Status::Offline => "Can't reach Native",
        _ => "Not signed in",
    }
}

pub fn lock_body() -> &'static str {
    match status() {
        Status::Checking => "Connecting this game to your Native account.",
        Status::Expired => "Sign in again in the Native Client. This page unlocks by itself once you do.",
        Status::Offline => "Check your connection. Retrying automatically.",
        _ => "Start Minecraft from the Native Client with a Native or Microsoft account to use this.",
    }
}

fn run() {
    loop {
        {
            let mut s = lock();
            // The launcher rewrote session.json (a fresh ticket, another account, or signed out).
            if let Some(dir) = s.dir.clone() {
                let now = stamp(&dir);
                if now != s.stamp {
                    s.stamp = now;
                    let fresh = read_handoff(&dir);
                    if fresh.as_ref().map(|h| h.ticket.as_str()) != s.ticket.as_deref() {
                        adopt(&mut s, fresh);
                    }
                }
            }
            let due = match s.status {
                Status::Checking | Status::Offline => true,
                Status::SignedIn => s.verified_at.is_none_or(|v| v.elapsed() > RECHECK),
                _ => false,
            };
            if due && Instant::now() >= s.next_try {
                let (used, api) = (s.ticket.clone(), s.api.clone());
                drop(s);
                if let Some(used) = used {
                    verify(&used, &api);
                }
            }
        }
        let guard = lock();
        let wait = if guard.status == Status::Checking { Duration::from_secs(1) } else { WATCH };
        let _ = state().1.wait_timeout(guard, wait);
    }
}

fn verify(used: &str, api: &str) {
    let result = get_json(&format!("{api}/v1/mod/me"), Some(used));
    let mut s = lock();
    if s.ticket.as_deref() != Some(used) {
        return; // Replaced while we were asking.
    }
    match result {
        Ok(root) => {
            let a = root.get("account");
            let text = |k: &str| a.and_then(|a| a.get(k)).and_then(Value::as_str).map(str::to_owned);
            s.account = Some(Account { id: text("id"), name: text("name"), uuid: text("uuid"), model: text("model") });
            s.verified_at = Some(Instant::now());
            s.failures = 0;
            s.status = Status::SignedIn;
        }
        Err(HttpError::Unauthorized) => {
            drop(s);
            rejected(used);
        }
        Err(_) => {
            s.failures += 1;
            // 5 s, 10 s, 20 s ... capped at a minute.
            let back = (5u64 << (s.failures - 1).min(4)).min(60);
            s.next_try = Instant::now() + Duration::from_secs(back);
            if s.status != Status::SignedIn || s.failures > 1 {
                s.status = Status::Offline;
            }
        }
    }
}

// ── HTTP ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum HttpError {
    Unauthorized,
    Status(u16),
    TooLarge,
    Other(String),
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpError::Unauthorized => write!(f, "401"),
            HttpError::Status(c) => write!(f, "HTTP {c}"),
            HttpError::TooLarge => write!(f, "too large"),
            HttpError::Other(e) => write!(f, "{e}"),
        }
    }
}

fn agent(read: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(8))
        .timeout_read(read)
        .user_agent("NativeClient (Rust engine)")
        .build()
}

fn finish(result: Result<ureq::Response, ureq::Error>, limit: u64) -> Result<Vec<u8>, HttpError> {
    match result {
        Ok(response) => {
            let mut body = Vec::new();
            response
                .into_reader()
                .take(limit + 1)
                .read_to_end(&mut body)
                .map_err(|e| HttpError::Other(e.to_string()))?;
            if body.len() as u64 > limit { Err(HttpError::TooLarge) } else { Ok(body) }
        }
        Err(ureq::Error::Status(401, _)) => Err(HttpError::Unauthorized),
        Err(ureq::Error::Status(code, _)) => Err(HttpError::Status(code)),
        Err(e) => Err(HttpError::Other(e.to_string())),
    }
}

fn with_auth(request: ureq::Request, ticket: Option<&str>) -> ureq::Request {
    match ticket {
        Some(t) => request.set("Authorization", &format!("Bearer {t}")),
        None => request,
    }
}

/// GET `url` (with the ticket when given), at most `limit` bytes.
pub fn get_bytes(url: &str, ticket: Option<&str>, limit: u64) -> Result<Vec<u8>, HttpError> {
    finish(with_auth(agent(Duration::from_secs(15)).get(url), ticket).call(), limit)
}

pub fn get_json(url: &str, ticket: Option<&str>) -> Result<Value, HttpError> {
    let body = get_bytes(url, ticket, 8 * 1024 * 1024)?;
    serde_json::from_slice(&body).map_err(|e| HttpError::Other(e.to_string()))
}

pub fn post_json(url: &str, ticket: Option<&str>, body: &Value) -> Result<Value, HttpError> {
    let request = with_auth(agent(Duration::from_secs(15)).post(url), ticket).set("Content-Type", "application/json");
    let bytes = finish(request.send_string(&body.to_string()), 8 * 1024 * 1024)?;
    if bytes.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_slice(&bytes).map_err(|e| HttpError::Other(e.to_string()))
}

pub fn delete(url: &str, ticket: Option<&str>) -> Result<(), HttpError> {
    finish(with_auth(agent(Duration::from_secs(15)).delete(url), ticket).call(), 1024 * 1024).map(|_| ())
}

/// A request whose reply is read as it comes (Server-Sent Events).
pub fn open_stream(url: &str, ticket: Option<&str>) -> Result<Box<dyn Read + Send + Sync>, HttpError> {
    let request = with_auth(agent(Duration::from_secs(70)).get(url), ticket).set("Accept", "text/event-stream");
    match request.call() {
        Ok(response) => Ok(Box::new(response.into_reader())),
        Err(ureq::Error::Status(401, _)) => Err(HttpError::Unauthorized),
        Err(ureq::Error::Status(code, _)) => Err(HttpError::Status(code)),
        Err(e) => Err(HttpError::Other(e.to_string())),
    }
}

// ── images ───────────────────────────────────────────────────────────────

#[derive(Clone)]
pub enum Pic {
    Loading,
    Ready(Arc<Image>),
    Failed,
}

fn pics() -> &'static Mutex<HashMap<String, Pic>> {
    static PICS: OnceLock<Mutex<HashMap<String, Pic>>> = OnceLock::new();
    PICS.get_or_init(Default::default)
}

static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

/// The picture at `url`, fetched on a background thread the first time it is asked for (at most four at once).
pub fn image(url: &str, limit: u64) -> Pic {
    if url.is_empty() {
        return Pic::Failed;
    }
    {
        let mut cache = pics().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(p) = cache.get(url) {
            return p.clone();
        }
        if IN_FLIGHT.load(Ordering::Relaxed) >= 4 {
            return Pic::Loading;
        }
        if cache.len() > 400 {
            cache.retain(|_, p| matches!(p, Pic::Loading));
        }
        cache.insert(url.to_owned(), Pic::Loading);
    }
    IN_FLIGHT.fetch_add(1, Ordering::Relaxed);
    let url = url.to_owned();
    let spawned = std::thread::Builder::new().name("Native image".into()).spawn({
        let url = url.clone();
        move || {
            let result = get_bytes(&url, None, limit).ok().and_then(|b| Image::decode(&b));
            let pic = result.map_or(Pic::Failed, Pic::Ready);
            pics().lock().unwrap_or_else(|e| e.into_inner()).insert(url, pic);
            IN_FLIGHT.fetch_sub(1, Ordering::Relaxed);
        }
    });
    if spawned.is_err() {
        IN_FLIGHT.fetch_sub(1, Ordering::Relaxed);
        pics().lock().unwrap_or_else(|e| e.into_inner()).remove(&url);
    }
    Pic::Loading
}

/// Forget what failed, so the next ask tries again.
pub fn forget_failed() {
    pics().lock().unwrap_or_else(|e| e.into_inner()).retain(|_, p| !matches!(p, Pic::Failed));
}

/// `{base}/{hash}`, the texture directory's way to name a file.
pub fn hash_url(base: &str, hash: &str) -> String {
    format!("{}/{}", base.trim_end_matches('/'), hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_native_may_get_the_ticket() {
        assert!(api_allowed("https://api.playnative.net"));
        assert!(api_allowed("http://localhost:8080"));
        assert!(!api_allowed("http://api.playnative.net"));
        assert!(!api_allowed("https://playnative.net.evil.com"));
        assert!(!api_allowed("https://user@api.playnative.net"));
    }

    #[test]
    fn a_hand_off_needs_a_native_ticket() {
        let dir = std::env::temp_dir().join(format!("native-handoff-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".native")).unwrap();
        std::fs::write(dir.join(".native/session.json"), r#"{"ticket":"nmt1.abc","api":"https://api.playnative.net/"}"#).unwrap();
        let h = read_handoff(&dir).unwrap();
        assert_eq!((h.ticket.as_str(), h.api.as_str()), ("nmt1.abc", "https://api.playnative.net"));
        std::fs::write(dir.join(".native/session.json"), r#"{"ticket":"other"}"#).unwrap();
        assert!(read_handoff(&dir).is_none());
        std::fs::write(dir.join(".native/session.json"), r#"{"ticket":"nmt1.x","api":"https://evil.example"}"#).unwrap();
        assert!(read_handoff(&dir).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hash_urls_join_cleanly() {
        assert_eq!(hash_url("https://t.example/csl/", "abc"), "https://t.example/csl/abc");
    }
}
