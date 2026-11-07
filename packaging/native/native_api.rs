// Native Client session and friend presence for the Native edition of the
// engine.
//
// The launcher trades the account's Native session for a game-only ticket
// (`nmt1.…`) and writes it to `<game dir>/.native/session.json`, the same
// hand-off the Java mod reads. This module checks that ticket against
// `/v1/mod/me`, remembers the account for the title screen, and reports what
// the player is doing to `/v1/mod/presence` (the same endpoint the Java mod
// uses) so friends see it in Relay.
//
// Everything network-facing runs on its own daemon thread; the game only
// reads `native_account` from the render thread. Paths are fully qualified
// because this file is included inside `discord_presence`, which already
// imports several of the same names.

const DEFAULT_API: &str = "https://api.playnative.net";
const FILE_LIMIT: u64 = 16 * 1024;
const SESSION_POLL: std::time::Duration = std::time::Duration::from_secs(15);
const VERIFY_EVERY: std::time::Duration = std::time::Duration::from_secs(600);
const PRESENCE_EVERY: std::time::Duration = std::time::Duration::from_secs(30);

struct NativeShared {
    status: Status,
    stop: bool,
    started: bool,
    account: Option<String>,
}

static NATIVE: std::sync::Mutex<NativeShared> = std::sync::Mutex::new(NativeShared {
    status: Status::Menus,
    stop: false,
    started: false,
    account: None,
});
static NATIVE_WAKE: std::sync::Condvar = std::sync::Condvar::new();

fn native_lock() -> std::sync::MutexGuard<'static, NativeShared> {
    NATIVE.lock().unwrap_or_else(|e| e.into_inner())
}

/// The Native account the game ticket belongs to, once it has been checked.
/// `None` in guest mode, before the check finishes, or after Native rejected
/// the ticket.
pub fn native_account() -> Option<String> {
    native_lock().account.clone()
}

/// Called from [`start`] before Discord's early returns: the Native session
/// works whether or not Rich Presence is turned on.
pub fn native_start() {
    let mut shared = native_lock();
    if shared.started {
        return;
    }
    shared.started = true;
    drop(shared);
    match std::thread::Builder::new()
        .name("Native session".into())
        .spawn(native_run)
    {
        Ok(_) => {}
        Err(e) => log::warn!("Native: session thread unavailable: {e}"),
    }
    // Native players' skins: the directory is fetched from launch, not when the first player shows up, so a
    // world opens with everyone's skin already known.
    drop(crate::native_skins::directory());
}

/// What the player does now (the same [`Status`] the game sends Discord).
pub fn native_set(status: Status) {
    let mut shared = native_lock();
    if shared.status != status {
        shared.status = status;
        NATIVE_WAKE.notify_all();
    }
}

/// On quit: clears the friend presence. The thread is a daemon, so a slow API
/// never holds the game open.
pub fn native_stop() {
    let mut shared = native_lock();
    shared.stop = true;
    NATIVE_WAKE.notify_all();
}

fn native_wait(limit: std::time::Duration) {
    let shared = native_lock();
    let _ = NATIVE_WAKE.wait_timeout(shared, limit);
}

struct NativeSession {
    ticket: String,
    api: String,
}

fn native_session_path() -> Option<std::path::PathBuf> {
    if let Some(dir) = std::env::var_os("NATIVE_GAME_DIR") {
        return Some(std::path::PathBuf::from(dir).join(".native").join("session.json"));
    }
    std::env::current_dir()
        .ok()
        .map(|dir| dir.join(".native").join("session.json"))
}

/// Only Native over https (or loopback http for development) may receive the
/// ticket, so a tampered session file cannot point the game at another host.
fn native_api_allowed(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    let parsed = lower
        .strip_prefix("http://")
        .map(|rest| (rest, true))
        .or_else(|| lower.strip_prefix("https://").map(|rest| (rest, false)));
    let Some((rest, plain)) = parsed else {
        return false;
    };
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

fn read_native_session() -> Option<NativeSession> {
    let path = native_session_path()?;
    if std::fs::metadata(&path).ok()?.len() > FILE_LIMIT {
        return None;
    }
    let text = std::fs::read_to_string(&path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let ticket = value.get("ticket")?.as_str()?.trim().to_string();
    if !ticket.starts_with("nmt1.") {
        return None;
    }
    let api = value
        .get("api")
        .and_then(|api| api.as_str())
        .unwrap_or(DEFAULT_API)
        .trim()
        .trim_end_matches('/')
        .to_string();
    if !native_api_allowed(&api) {
        log::warn!("Native: ignoring a session file for {api}");
        return None;
    }
    Some(NativeSession { ticket, api })
}

fn native_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(8))
        .timeout_read(std::time::Duration::from_secs(10))
        .user_agent("NativeClient (Rust engine)")
        .build()
}

/// `Ok(name)` when Native accepted the ticket, `Err(true)` when it refused it
/// (the launcher must sign in again), `Err(false)` when it could not be
/// reached.
fn native_verify(session: &NativeSession) -> Result<String, bool> {
    let request = native_agent()
        .get(&format!("{}/v1/mod/me", session.api))
        .set("Authorization", &format!("Bearer {}", session.ticket));
    match request.call() {
        Ok(response) => {
            let text = response.into_string().unwrap_or_default();
            let value: serde_json::Value = serde_json::from_str(&text).map_err(|_| false)?;
            Ok(value
                .get("account")
                .and_then(|account| account.get("name"))
                .and_then(|name| name.as_str())
                .unwrap_or("")
                .to_string())
        }
        Err(ureq::Error::Status(401, _)) => Err(true),
        Err(_) => Err(false),
    }
}

fn native_activity(status: Status) -> (Option<&'static str>, bool) {
    match status {
        Status::Off => (None, false),
        Status::Menus => (Some("In the menus"), false),
        Status::World { play, .. } => (
            Some(match play {
                Play::Singleplayer => "Playing singleplayer",
                Play::Multiplayer => "Playing multiplayer",
                Play::Hosting { .. } => "Hosting a world",
            }),
            true,
        ),
    }
}

fn native_push(session: &NativeSession, status: Status) -> bool {
    let (activity, in_game) = native_activity(status);
    let mut body = serde_json::json!({
        "status": if in_game { "in-game" } else { "online" },
    });
    body["activity"] = match activity {
        Some(text) => serde_json::Value::String(text.to_string()),
        None => serde_json::Value::Null,
    };
    let request = native_agent()
        .post(&format!("{}/v1/mod/presence", session.api))
        .set("Authorization", &format!("Bearer {}", session.ticket))
        .set("Content-Type", "application/json");
    request.send_string(&body.to_string()).is_ok()
}

fn native_run() {
    let mut session: Option<NativeSession> = None;
    let mut ticket = String::new();
    let mut rejected = false;
    let mut verified: Option<String> = None;
    let mut next_session = std::time::Instant::now();
    let mut next_verify = std::time::Instant::now();
    let mut next_push = std::time::Instant::now();
    let mut pushed: Option<Status> = None;

    loop {
        let (stop, status) = {
            let shared = native_lock();
            (shared.stop, shared.status)
        };
        if stop {
            break;
        }

        let now = std::time::Instant::now();
        if now >= next_session {
            let fresh = read_native_session();
            let fresh_ticket = fresh.as_ref().map(|s| s.ticket.clone()).unwrap_or_default();
            if fresh_ticket != ticket {
                ticket = fresh_ticket;
                rejected = false;
                verified = None;
                next_verify = now;
                next_push = now;
                pushed = None;
                native_lock().account = None;
                if fresh.is_some() {
                    log::info!("Native: found the launcher's game ticket");
                }
            }
            session = fresh;
            next_session = now + SESSION_POLL;
        }

        if let Some(found) = session.as_ref() {
            if !rejected && (verified.is_none() || now >= next_verify) {
                match native_verify(found) {
                    Ok(name) => {
                        if verified.as_deref() != Some(name.as_str()) {
                            log::info!("Native: signed in as {name}");
                        }
                        verified = Some(name.clone());
                        native_lock().account = Some(name);
                    }
                    Err(is_rejected) => {
                        if is_rejected {
                            log::warn!("Native: the game ticket was refused; waiting for the launcher");
                            rejected = true;
                            verified = None;
                            native_lock().account = None;
                        }
                    }
                }
                next_verify = now + VERIFY_EVERY;
            }
            if verified.is_some() && !rejected && (pushed != Some(status) || now >= next_push) {
                if native_push(found, status) {
                    pushed = Some(status);
                }
                next_push = now + PRESENCE_EVERY;
            }
        }

        native_wait(std::time::Duration::from_secs(1));
    }

    if let (Some(found), Some(_)) = (session.as_ref(), verified.as_ref()) {
        let _ = native_push(found, Status::Off);
    }
}
