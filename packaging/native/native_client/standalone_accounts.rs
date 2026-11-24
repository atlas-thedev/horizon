//! In-game sign-in and local profile storage when there is no launcher connection.

use super::{game_dir, net, title_accounts::tr};
use horizon_protocol::crypto::{offline_uuid, uuid_string};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const CLIENT: &str = "499c8d36-be2a-4231-9ebd-ef291b7bb64c";
const MS: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0";
const MC: &str = "https://api.minecraftservices.com";
const ACCOUNT_LIMIT: usize = 128;
const FILE_LIMIT: u64 = 262_144;

#[derive(Default)]
pub struct Accounts {
    profiles: Option<Vec<Value>>,
    credentials: HashMap<String, Credentials>,
    microsoft: Option<DeviceCode>,
    current: Option<String>,
    prepared: Option<(String, Option<String>)>,
}

struct Credentials {
    native: Option<String>,
    minecraft: Option<(String, Instant)>,
}

struct DeviceCode {
    secret: String,
    expires: Instant,
    next_poll: Instant,
    interval: Duration,
}

impl Accounts {
    pub fn exchange(&mut self, request: &Value) -> Value {
        match self.handle(request) {
            Ok(mut reply) => {
                reply["ok"] = true.into();
                reply
            }
            Err(key) => json!({"ok":false,"error":tr(key)}),
        }
    }

    fn handle(&mut self, request: &Value) -> Result<Value, &'static str> {
        if self.profiles.is_none() {
            self.profiles = Some(load(&file()?));
        }
        match text(request, "action") {
            "list" => Ok(self.list()),
            "add_offline" => {
                let name = text(request, "name");
                if !valid_name(name, 3) {
                    return Err("bad_name");
                }
                let id = format!("offline-{}", name.to_ascii_lowercase());
                self.save(&id, "offline", name, &uuid_string(offline_uuid(name)))?;
                Ok(self.added(&id))
            }
            "native_login" => self.native_login(text(request, "login"), text(request, "password")),
            "microsoft_start" => {
                self.microsoft = None;
                let (_, result) = form(
                    "devicecode",
                    &[
                        ("client_id", CLIENT),
                        ("scope", "XboxLive.signin offline_access"),
                    ],
                )?;
                let now = Instant::now();
                let interval = Duration::from_secs(number(&result, "interval", 5).clamp(1, 900));
                let secret = required(&result, "device_code", "ms_failed")?.to_owned();
                let code = required(&result, "user_code", "ms_failed")?;
                let url = required(&result, "verification_uri", "ms_failed")?;
                self.microsoft = Some(DeviceCode {
                    secret,
                    interval,
                    next_poll: now + interval,
                    expires: now
                        + Duration::from_secs(number(&result, "expires_in", 900).min(1800)),
                });
                Ok(json!({"code":code,"url":url}))
            }
            "microsoft_status" => self.microsoft_status(),
            "microsoft_cancel" => {
                self.microsoft = None;
                Ok(json!({}))
            }
            "switch" => self.prepare(text(request, "account_id")),
            "applied" => {
                let id = text(request, "account_id");
                let Some((_, ticket)) = self.prepared.take().filter(|(prepared, _)| prepared == id)
                else {
                    return Err("switch_failed");
                };
                super::net::use_standalone_ticket(ticket);
                self.current = Some(id.to_owned());
                Ok(self.list())
            }
            _ => Err("switch_failed"),
        }
    }

    fn native_login(&mut self, login: &str, password: &str) -> Result<Value, &'static str> {
        let (status, result) = post(
            &format!("{}/v1/auth/login", net::api()),
            None,
            &json!({
                "login":login,"password":password
            }),
        )?;
        match status {
            200 => {}
            401 if result["premium"] == true => return Err("use_microsoft"),
            401 => return Err("bad_credentials"),
            429 => return Err("rate_limited"),
            _ => return Err("service_failed"),
        }
        let profile = &result["account"];
        let name = required(profile, "name", "service_failed")?;
        let id = format!("native-{}", required(profile, "id", "service_failed")?);
        let token = required(&result, "token", "service_failed")?.to_owned();
        self.save(&id, "native", name, &uuid_string(offline_uuid(name)))?;
        self.credentials.insert(
            id.clone(),
            Credentials {
                native: Some(token),
                minecraft: None,
            },
        );
        Ok(self.added(&id))
    }

    fn microsoft_status(&mut self) -> Result<Value, &'static str> {
        let Some(mut code) = self.microsoft.take() else {
            return Ok(json!({}));
        };
        let now = Instant::now();
        if now >= code.expires {
            return Err("ms_expired");
        }
        if now < code.next_poll {
            self.microsoft = Some(code);
            return Ok(json!({}));
        }
        let (_, response) = form(
            "token",
            &[
                ("client_id", CLIENT),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("device_code", &code.secret),
            ],
        )?;
        match text(&response, "error") {
            "authorization_pending" | "slow_down" => {
                if text(&response, "error") == "slow_down" {
                    code.interval += Duration::from_secs(5);
                }
                code.next_poll = Instant::now() + code.interval;
                self.microsoft = Some(code);
                return Ok(json!({}));
            }
            "expired_token" => return Err("ms_expired"),
            "authorization_declined" | "access_denied" => return Err("ms_declined"),
            _ => {}
        }
        let microsoft_token = required(&response, "access_token", "ms_failed")?;
        let (profile, minecraft) = minecraft_profile(microsoft_token)?;
        let uuid = required(&profile, "id", "ms_failed")?;
        let name = required(&profile, "name", "ms_failed")?;
        let id = format!("microsoft-{}", uuid.replace('-', "").to_ascii_lowercase());
        self.save(&id, "microsoft", name, uuid)?;
        self.credentials.insert(
            id.clone(),
            Credentials {
                native: None,
                minecraft: Some(minecraft),
            },
        );
        Ok(self.added(&id))
    }

    fn prepare(&mut self, id: &str) -> Result<Value, &'static str> {
        let mut grant = self
            .profiles
            .as_ref()
            .and_then(|rows| rows.iter().find(|r| r["id"] == id))
            .cloned()
            .ok_or("switch_failed")?;
        let provider = text(&grant, "type").to_owned();
        if self.needs_sign_in(id, &provider) {
            return Err("sign_in");
        }
        let mut game_ticket = None;
        let mut minecraft_token = String::new();
        if let Some(auth) = self.credentials.get_mut(id) {
            if let Some((token, _)) = &auth.minecraft {
                minecraft_token.clone_from(token);
                if auth.native.is_none()
                    && let Ok((200, result)) = post(
                        &format!("{}/v1/auth/minecraft", net::api()),
                        None,
                        &json!({"minecraftAccessToken":token}),
                    )
                {
                    auth.native = result["token"].as_str().map(str::to_owned);
                }
            }
            if let Some(token) = &auth.native {
                let result = post(
                    &format!("{}/v1/auth/game-ticket", net::api()),
                    Some(token),
                    &json!({}),
                );
                match result {
                    Ok((200, result)) => game_ticket = result["ticket"].as_str().map(str::to_owned),
                    Ok((401, _)) => auth.native = None,
                    _ => {}
                }
            }
        }
        if provider == "native" && game_ticket.is_none() {
            if self
                .credentials
                .get(id)
                .is_some_and(|auth| auth.native.is_none())
            {
                self.credentials.remove(id);
                return Err("sign_in");
            }
            return Err("service_failed");
        }
        self.prepared = Some((id.to_owned(), game_ticket));
        grant["online"] = (provider == "microsoft").into();
        grant["access_token"] = minecraft_token.into();
        Ok(json!({"session":grant}))
    }

    fn needs_sign_in(&self, id: &str, provider: &str) -> bool {
        provider != "offline"
            && self.credentials.get(id).is_none_or(|auth| {
                auth.minecraft
                    .as_ref()
                    .is_some_and(|(_, expires)| Instant::now() >= *expires)
            })
    }

    fn list(&self) -> Value {
        let rows: Vec<_> = self
            .profiles
            .iter()
            .flatten()
            .cloned()
            .map(|mut row| {
                let id = text(&row, "id");
                let current = self.current.as_deref() == Some(id);
                row["signInRequired"] = self.needs_sign_in(id, text(&row, "type")).into();
                row["current"] = current.into();
                row
            })
            .collect();
        json!({"accounts":rows})
    }

    fn added(&self, id: &str) -> Value {
        let mut result = self.list();
        result["addedAccountId"] = id.into();
        result
    }

    fn save(
        &mut self,
        id: &str,
        provider: &str,
        name: &str,
        uuid: &str,
    ) -> Result<(), &'static str> {
        let profile = json!({"id":id,"type":provider,"name":name,"uuid":uuid});
        if !valid_profile(&profile) {
            return Err("switch_failed");
        }
        let mut rows = self.profiles.clone().unwrap_or_default();
        rows.retain(|row| row["id"] != id);
        if rows.len() >= ACCOUNT_LIMIT {
            return Err("local_full");
        }
        rows.push(profile);
        save(&file()?, &rows).map_err(|_| "local_write_failed")?;
        self.profiles = Some(rows);
        Ok(())
    }
}

fn file() -> Result<PathBuf, &'static str> {
    Ok(game_dir()
        .ok_or("local_write_failed")?
        .join(".native")
        .join("standalone-accounts.json"))
}

fn load(file: &Path) -> Vec<Value> {
    let Ok(file) = std::fs::File::open(file) else {
        return Vec::new();
    };
    let reader = std::io::BufReader::new(file).take(FILE_LIMIT);
    let Ok(profiles) = serde_json::from_reader::<_, Vec<Value>>(reader) else {
        return Vec::new();
    };
    profiles
        .into_iter()
        .filter(valid_profile)
        .take(ACCOUNT_LIMIT)
        .map(|row| json!({"id":row["id"],"type":row["type"],"name":row["name"],"uuid":row["uuid"]}))
        .collect()
}

fn save(path: &Path, rows: &[Value]) -> std::io::Result<()> {
    std::fs::create_dir_all(path.parent().expect("profile directory"))?;
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    let result =
        std::fs::write(&tmp, serde_json::to_vec(rows)?).and_then(|_| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    result
}

fn valid_name(name: &str, minimum: usize) -> bool {
    (minimum..=16).contains(&name.len())
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn valid_profile(row: &Value) -> bool {
    let provider = text(row, "type");
    let id = text(row, "id");
    matches!(provider, "offline" | "native" | "microsoft")
        && id.len() < 160
        && id
            .strip_prefix(provider)
            .is_some_and(|suffix| suffix.starts_with('-'))
        && valid_name(
            text(row, "name"),
            if provider == "microsoft" { 1 } else { 3 },
        )
        && horizon_client::auth::parse_uuid(text(row, "uuid")).is_some()
}

fn minecraft_profile(token: &str) -> Result<(Value, (String, Instant)), &'static str> {
    let xbox_user = |prefix| {
        post(
            "https://user.auth.xboxlive.com/user/authenticate",
            None,
            &json!({
                "Properties":{"AuthMethod":"RPS","SiteName":"user.auth.xboxlive.com","RpsTicket":format!("{prefix}{token}")},
                "RelyingParty":"http://auth.xboxlive.com","TokenType":"JWT",
            }),
        )
    };
    let (status, mut xbox) = xbox_user("t=")?;
    if status == 401 {
        (_, xbox) = xbox_user("d=")?;
    }
    let xbox_token = required(&xbox, "Token", "ms_failed")?;
    let (_, xsts) = post(
        "https://xsts.auth.xboxlive.com/xsts/authorize",
        None,
        &json!({
            "Properties":{"SandboxId":"RETAIL","UserTokens":[xbox_token]},
            "RelyingParty":"rp://api.minecraftservices.com/","TokenType":"JWT",
        }),
    )?;
    let claim = xsts.pointer("/DisplayClaims/xui/0").ok_or("ms_failed")?;
    let identity = format!(
        "XBL3.0 x={};{}",
        required(claim, "uhs", "ms_failed")?,
        required(&xsts, "Token", "ms_failed")?
    );
    let (status, mut session) = post(
        &format!("{MC}/launcher/login"),
        None,
        &json!({"xtoken":identity,"platform":"PC_LAUNCHER"}),
    )?;
    if (400..500).contains(&status) && status != 429 {
        (_, session) = post(
            &format!("{MC}/authentication/login_with_xbox"),
            None,
            &json!({"identityToken":identity}),
        )?;
    }
    let token = required(&session, "access_token", "ms_failed")?;
    let expires =
        Instant::now() + Duration::from_secs(number(&session, "expires_in", 86_400).min(172_800));
    let (status, ownership) = get(&format!("{MC}/entitlements/mcstore"), token)?;
    if status != 200 {
        return Err("ms_failed");
    }
    if ownership["items"].as_array().is_none_or(Vec::is_empty) {
        return Err("ms_needs_game");
    }
    let (status, profile) = get(&format!("{MC}/minecraft/profile"), token)?;
    match status {
        200 => Ok((profile, (token.to_owned(), expires))),
        404 => Err("ms_needs_profile"),
        _ => Err("ms_failed"),
    }
}

fn request(method: &str, url: &str, token: Option<&str>) -> ureq::Request {
    let request = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(15))
        .redirects(0)
        .user_agent("NativeClient (Rust engine)")
        .build()
        .request(method, url)
        .set("Accept", "application/json");
    match token {
        Some(token) => request.set("Authorization", &format!("Bearer {token}")),
        None => request,
    }
}

fn response(result: Result<ureq::Response, ureq::Error>) -> Result<(u16, Value), &'static str> {
    let response = match result {
        Ok(response) | Err(ureq::Error::Status(_, response)) => response,
        Err(ureq::Error::Transport(_)) => return Err("service_unreachable"),
    };
    let status = response.status();
    let body =
        serde_json::from_reader(response.into_reader().take(FILE_LIMIT)).unwrap_or(Value::Null);
    Ok((status, body))
}

fn post(url: &str, token: Option<&str>, value: &Value) -> Result<(u16, Value), &'static str> {
    response(
        request("POST", url, token)
            .set("Content-Type", "application/json")
            .send_string(&value.to_string()),
    )
}

fn get(url: &str, token: &str) -> Result<(u16, Value), &'static str> {
    response(request("GET", url, Some(token)).call())
}

fn form(path: &str, fields: &[(&str, &str)]) -> Result<(u16, Value), &'static str> {
    response(request("POST", &format!("{MS}/{path}"), None).send_form(fields))
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or_default()
}

fn required<'a>(value: &'a Value, key: &str, error: &'static str) -> Result<&'a str, &'static str> {
    value[key]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or(error)
}

fn number(value: &Value, key: &str, fallback: u64) -> u64 {
    value[key].as_u64().unwrap_or(fallback)
}
