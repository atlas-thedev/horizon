//! `Ads`: the sponsored cards of the Native title screen (the same ads as the
//! launcher's Home): `.native/ads.json` from the launcher, else Native's
//! `/v1/site/ads`. Loaded once a session, off the render thread.
#![allow(dead_code)]

use super::net;
use crate::native_ui::Image;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

const MAX_IMAGE: u64 = 4 * 1024 * 1024;
/// Rounded top corners baked into the banner, as a share of its width (card radius / card width).
const CORNER: f32 = 16.0 / 340.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Button {
    pub label: String,
    /// "server" (join it) or "url" (open it).
    pub action: String,
    pub value: String,
}

impl Button {
    pub fn server(&self) -> bool {
        self.action == "server"
    }
}

#[derive(Clone)]
pub struct Ad {
    pub id: String,
    pub title: String,
    pub body: String,
    pub tag: String,
    pub url: String,
    pub buttons: Vec<Button>,
    pub image: Arc<Image>,
}

impl Ad {
    /// The main button (clicking the banner does the same).
    pub fn primary(&self) -> Option<Button> {
        match self.buttons.first() {
            Some(b) => Some(b.clone()),
            None if !self.url.is_empty() => Some(Button { label: String::new(), action: "url".into(), value: self.url.clone() }),
            None => None,
        }
    }
}

#[derive(Default)]
struct State {
    ads: Vec<Ad>,
    loading: bool,
    /// `None`: loaded for this session.
    retry_at: Option<Instant>,
    started: bool,
}

fn state() -> MutexGuard<'static, State> {
    static S: OnceLock<Mutex<State>> = OnceLock::new();
    S.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

/// Live ads (empty until loaded; loading starts on the first call).
pub fn list(game_dir: Option<PathBuf>) -> Vec<Ad> {
    let mut s = state();
    let due = !s.started || s.retry_at.is_some_and(|t| Instant::now() >= t);
    if !s.loading && due {
        s.loading = true;
        s.started = true;
        let _ = std::thread::Builder::new().name("Native ads".into()).spawn(move || {
            let dir = game_dir.map(|d| d.join(".native"));
            let found = dir.as_deref().and_then(from_launcher).filter(|l| !l.is_empty()).map(Ok).unwrap_or_else(|| from_api(dir.as_deref().map(|d| d.join("ads"))));
            let mut s = state();
            match found {
                Ok(ads) => {
                    s.ads = ads;
                    s.retry_at = None;
                }
                Err(()) => s.retry_at = Some(Instant::now() + Duration::from_secs(120)),
            }
            s.loading = false;
        });
    }
    s.ads.clone()
}

fn text(o: &Value, key: &str) -> String {
    o.get(key).and_then(Value::as_str).unwrap_or("").to_owned()
}

fn cut(s: String, n: usize) -> String {
    s.chars().take(n).collect()
}

fn read(path: &Path) -> Option<Arc<Image>> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > MAX_IMAGE {
        return None;
    }
    Image::decode(&std::fs::read(path).ok()?)
}

/// `ads.json` from the launcher; `None` when there is none.
fn from_launcher(dir: &Path) -> Option<Vec<Ad>> {
    let file = dir.join("ads.json");
    if std::fs::metadata(&file).ok()?.len() > 512 * 1024 {
        return None;
    }
    let root: Value = serde_json::from_slice(&std::fs::read(&file).ok()?).ok()?;
    let player = Some(text(&root, "player")).filter(|p| !p.is_empty()).and_then(|p| read(Path::new(&p)));
    let mut out = Vec::new();
    for o in root.get("ads").and_then(Value::as_array).into_iter().flatten().filter(|o| o.is_object()) {
        let file = text(o, "file");
        let banner = if file.is_empty() { None } else { read(Path::new(&file)) };
        let with_player = o.get("player").and_then(Value::as_bool).unwrap_or(false);
        if let Some(ad) = parse(o, banner, if with_player { player.clone() } else { None }) {
            out.push(ad);
        }
    }
    Some(out)
}

fn hash(url: &str) -> String {
    format!("{:016x}", url.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)))
}

fn from_api(cache: Option<PathBuf>) -> Result<Vec<Ad>, ()> {
    let root = net::get_json(&format!("{}/v1/site/ads", net::api()), None).map_err(|_| ())?;
    let mut out = Vec::new();
    for o in root.get("ads").and_then(Value::as_array).into_iter().flatten().filter(|o| o.is_object()).take(20) {
        let url = text(o, "image");
        if !url.starts_with("https://") {
            continue;
        }
        let file = cache.as_ref().map(|c| c.join(format!("{}.img", hash(&url))));
        let bytes = match file.as_ref().and_then(|f| std::fs::read(f).ok()) {
            Some(b) => Some(b),
            None => net::get_bytes(&url, None, MAX_IMAGE).ok().inspect(|b| {
                if let (Some(f), Some(c)) = (&file, &cache) {
                    let _ = std::fs::create_dir_all(c);
                    let _ = std::fs::write(f, b);
                }
            }),
        };
        if let Some(ad) = parse(o, bytes.and_then(|b| Image::decode(&b)), None) {
            out.push(ad);
        }
    }
    Ok(out)
}

/// A server address an ad may join.
fn server_like(v: &str) -> bool {
    let (host, port) = v.split_once(':').map_or((v, None), |(h, p)| (h, Some(p)));
    !host.is_empty()
        && host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        && port.is_none_or(|p| (2..=5).contains(&p.len()) && p.chars().all(|c| c.is_ascii_digit()))
}

fn parse(o: &Value, banner: Option<Arc<Image>>, player: Option<Arc<Image>>) -> Option<Ad> {
    let banner = banner?;
    let url = text(o, "url");
    let mut buttons = Vec::new();
    if let Some(list) = o.get("buttons").and_then(Value::as_array) {
        for b in list.iter().filter(|b| b.is_object()) {
            if buttons.len() >= 2 {
                break;
            }
            let label = cut(text(b, "label"), 20);
            let action = if text(b, "action") == "server" { "server" } else { "url" };
            let value = text(b, "value");
            let ok = if action == "server" { server_like(&value) } else { value.starts_with("https://") };
            if !label.is_empty() && ok {
                buttons.push(Button { label, action: action.into(), value });
            }
        }
    } else if !text(o, "cta").is_empty() && url.starts_with("https://") {
        buttons.push(Button { label: cut(text(o, "cta"), 20), action: "url".into(), value: url.clone() });
    }
    Some(Ad {
        id: text(o, "id"),
        title: cut(text(o, "title"), 60),
        body: cut(text(o, "body"), 140),
        tag: cut(text(o, "tag"), 20),
        url: if url.starts_with("https://") { url } else { String::new() },
        buttons,
        image: compose(&banner, player.as_deref()),
    })
}

fn over(s: u32, d: u32) -> u32 {
    let (sa, da) = ((s >> 24) as f32 / 255.0, (d >> 24) as f32 / 255.0);
    let oa = sa + da * (1.0 - sa);
    if oa <= 0.0 {
        return 0;
    }
    let ch = |shift: u32| -> u32 { ((((s >> shift) & 255) as f32 * sa + ((d >> shift) & 255) as f32 * da * (1.0 - sa)) / oa).round() as u32 };
    ((oa * 255.0).round() as u32) << 24 | ch(16) << 16 | ch(8) << 8 | ch(0)
}

/// The banner with the player's skin drawn in like the launcher does, and its top corners rounded.
fn compose(banner: &Image, player: Option<&Image>) -> Arc<Image> {
    let (mut w, mut h) = (banner.width as usize, banner.height as usize);
    let mut px = banner.argb.clone();
    if let Some(p) = player.filter(|p| p.height > 0) {
        let ph = (h as f32 * 1.12).round() as usize;
        let pw = ((ph as f32 * p.width as f32 / p.height as f32).round() as usize).max(1);
        let scaled = p.scaled(pw as u32, ph as u32);
        let left = (w as f32 - w as f32 * 0.025).round() as isize - pw as isize;
        let top = (h as f32 + h as f32 * 0.028).round() as isize - ph as isize;
        for y in top.max(0)..(h as isize).min(top + ph as isize) {
            for x in left.max(0)..(w as isize).min(left + pw as isize) {
                let s = scaled.argb[((y - top) as usize) * pw + (x - left) as usize];
                if s >> 24 != 0 {
                    let i = y as usize * w + x as usize;
                    px[i] = over(s, px[i]);
                }
            }
        }
    }
    if w > 760 {
        let small = Image::new(w as u32, h as u32, px).scaled(760, (760.0 * h as f32 / w as f32).round() as u32);
        w = small.width as usize;
        h = small.height as usize;
        px = small.argb.clone();
    }
    let r = (w as f32 * CORNER).max(2.0);
    let ri = r.ceil() as usize;
    for y in 0..h.min(ri) {
        for x in 0..ri.min(w) {
            let (dx, dy) = (r - x as f32 - 0.5, r - y as f32 - 0.5);
            let d = (dx * dx + dy * dy).sqrt() - r;
            let cover = if d <= -0.5 { 1.0 } else if d >= 0.5 { 0.0 } else { 0.5 - d };
            if cover >= 1.0 {
                continue;
            }
            for i in [y * w + x, y * w + (w - 1 - x)] {
                let a = ((px[i] >> 24) as f32 * cover).round() as u32;
                px[i] = a << 24 | (px[i] & 0xFFFFFF);
            }
        }
    }
    Image::new(w as u32, h as u32, px)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_server_addresses_and_https_links_make_buttons() {
        assert!(server_like("play.example.net"));
        assert!(server_like("mc.example.net:25565"));
        assert!(!server_like("evil.example.net/path"));
        assert!(!server_like("a:1"));
        let o: Value = serde_json::json!({"id":"x","title":"T","buttons":[
            {"label":"Join","action":"server","value":"play.example.net"},
            {"label":"Site","action":"url","value":"http://insecure.example"}]});
        let ad = parse(&o, Some(Image::new(4, 4, vec![0xFFFFFFFF; 16])), None).unwrap();
        assert_eq!(ad.buttons.len(), 1);
        assert!(ad.primary().unwrap().server());
    }

    #[test]
    fn the_banner_gets_round_top_corners() {
        let img = compose(&Image::new(340, 140, vec![0xFFFFFFFF; 340 * 140]), None);
        assert_eq!(img.argb[0] >> 24, 0);
        assert_eq!(img.argb[100 * 340 + 170], 0xFFFFFFFF);
    }
}
