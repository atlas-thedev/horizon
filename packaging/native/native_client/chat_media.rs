//! `ChatMedia`: pictures and animated GIFs shown inside chat messages:
//! downloaded off-thread, decoded, cached (newest 80).
#![allow(dead_code)]

use super::gif;
use super::net;
use horizon_client::native_ui::Image;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// A picture is shrunk to this width when it is wider.
const MAX_WIDTH: u32 = 640;

/// One picture: a single frame, or the frames of a GIF with their delays.
pub struct Media {
    pub frames: Vec<Arc<Image>>,
    pub delays: Vec<u32>,
    pub failed: bool,
    pub width: u32,
    pub height: u32,
    total: u32,
}

impl Media {
    fn failed() -> Media {
        Media { frames: Vec::new(), delays: Vec::new(), failed: true, width: 0, height: 0, total: 0 }
    }

    /// The frame showing at `now_ms`.
    pub fn frame(&self, now_ms: f64) -> Option<&Arc<Image>> {
        if self.frames.len() <= 1 || self.total == 0 {
            return self.frames.first();
        }
        let mut t = (now_ms as u64 % u64::from(self.total)) as i64;
        for (i, d) in self.delays.iter().enumerate() {
            t -= i64::from(*d);
            if t < 0 {
                return self.frames.get(i);
            }
        }
        self.frames.first()
    }

    pub fn animated(&self) -> bool {
        self.frames.len() > 1
    }
}

enum Slot {
    Loading,
    Ready(Arc<Media>),
}

struct Cache {
    map: HashMap<String, Slot>,
    order: VecDeque<String>,
}

fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Cache { map: HashMap::new(), order: VecDeque::new() }))
}

fn locals() -> &'static Mutex<HashMap<String, Arc<Media>>> {
    static LOCALS: OnceLock<Mutex<HashMap<String, Arc<Media>>>> = OnceLock::new();
    LOCALS.get_or_init(Default::default)
}

static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

/// Only Native uploads and Giphy are ever fetched.
pub fn allowed(url: &str, api: &str) -> bool {
    if url.contains("/v1/social/media/") || (!api.is_empty() && url.starts_with(api)) {
        return true;
    }
    let https = url.starts_with("https://") || url.starts_with("http://");
    https && (url.contains("giphy.com/") || url.contains(".nativelaunch.net/"))
}

fn decode(bytes: &[u8]) -> Media {
    let shrink = |img: Arc<Image>| -> Arc<Image> {
        if img.width > MAX_WIDTH { img.scaled(MAX_WIDTH, (img.height * MAX_WIDTH / img.width.max(1)).max(1)) } else { img }
    };
    if let Some(g) = gif::decode(bytes) {
        let frames: Vec<Arc<Image>> = g.frames.iter().map(|(px, _)| shrink(Image::new(g.width, g.height, px.clone()))).collect();
        let delays: Vec<u32> = g.frames.iter().map(|f| f.1).collect();
        let (width, height) = frames.first().map_or((0, 0), |f| (f.width, f.height));
        let total = delays.iter().sum();
        return Media { frames, delays, failed: false, width, height, total };
    }
    match Image::decode(bytes) {
        Some(img) => {
            let img = shrink(img);
            Media { width: img.width, height: img.height, frames: vec![img], delays: vec![0], failed: false, total: 0 }
        }
        None => Media::failed(),
    }
}

/// A picture the player just pasted, shown while it uploads.
pub fn put_local(key: &str, bytes: Vec<u8>) {
    let key = key.to_owned();
    let _ = std::thread::Builder::new().name("Native media".into()).spawn(move || {
        let media = decode(&bytes);
        let mut l = locals().lock().unwrap_or_else(|e| e.into_inner());
        if l.len() > 16 {
            l.clear();
        }
        l.insert(key, Arc::new(media));
    });
}

pub fn local(key: &str) -> Option<Arc<Media>> {
    locals().lock().unwrap_or_else(|e| e.into_inner()).get(key).cloned()
}

/// The picture for a URL if it is already cached (or loading); never starts a download.
pub fn peek(url: &str) -> Option<Arc<Media>> {
    match cache().lock().unwrap_or_else(|e| e.into_inner()).map.get(url) {
        Some(Slot::Ready(m)) => Some(m.clone()),
        _ => None,
    }
}

/// The picture for a URL; `None` while it loads (the first call starts it). Never blocks.
pub fn get(url: &str, api: &str) -> Option<Arc<Media>> {
    let mut c = cache().lock().unwrap_or_else(|e| e.into_inner());
    match c.map.get(url) {
        Some(Slot::Ready(m)) => return Some(m.clone()),
        Some(Slot::Loading) => return None,
        None => {}
    }
    if !allowed(url, api) {
        let m = Arc::new(Media::failed());
        c.map.insert(url.to_owned(), Slot::Ready(m.clone()));
        return Some(m);
    }
    if IN_FLIGHT.load(Ordering::Relaxed) >= 3 {
        return None;
    }
    while c.order.len() >= 80 {
        if let Some(old) = c.order.pop_front() {
            c.map.remove(&old);
        }
    }
    c.map.insert(url.to_owned(), Slot::Loading);
    c.order.push_back(url.to_owned());
    drop(c);
    let mut target = url.to_owned();
    if target.contains("giphy.com") {
        target = target.replace("/giphy.gif", "/200w.gif");
    }
    if target.starts_with('/') && !api.is_empty() {
        target = format!("{}{target}", api.trim_end_matches('/'));
    }
    IN_FLIGHT.fetch_add(1, Ordering::Relaxed);
    let url = url.to_owned();
    let spawned = std::thread::Builder::new().name("Native media".into()).spawn({
        let url = url.clone();
        move || {
            let media = net::get_bytes(&target, None, 10 * 1024 * 1024).map(|b| decode(&b)).unwrap_or_else(|_| Media::failed());
            cache().lock().unwrap_or_else(|e| e.into_inner()).map.insert(url, Slot::Ready(Arc::new(media)));
            IN_FLIGHT.fetch_sub(1, Ordering::Relaxed);
        }
    });
    if spawned.is_err() {
        IN_FLIGHT.fetch_sub(1, Ordering::Relaxed);
        cache().lock().unwrap_or_else(|e| e.into_inner()).map.remove(&url);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_native_and_giphy_pictures_are_fetched() {
        let api = "https://api.playnative.net";
        assert!(allowed("https://api.playnative.net/v1/social/media/x.png", api));
        assert!(allowed("https://media2.giphy.com/media/abc/giphy.gif", api));
        assert!(!allowed("https://evil.example/a.png", api));
        assert!(!allowed("ftp://media.giphy.com/a.gif", api));
    }

    #[test]
    fn an_animation_shows_the_frame_for_the_time() {
        let img = || Image::new(1, 1, vec![0xFF000000]);
        let m = Media { frames: vec![img(), img()], delays: vec![100, 300], failed: false, width: 1, height: 1, total: 400 };
        assert!(Arc::ptr_eq(m.frame(50.0).unwrap(), &m.frames[0]));
        assert!(Arc::ptr_eq(m.frame(150.0).unwrap(), &m.frames[1]));
        assert!(Arc::ptr_eq(m.frame(450.0).unwrap(), &m.frames[0]));
        assert!(m.animated());
    }
}
