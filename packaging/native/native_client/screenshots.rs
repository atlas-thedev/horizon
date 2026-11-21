//! `ScreenshotsView`: the Screenshots page of the Native menu: every picture
//! in the game's screenshots folder as a grid of thumbnails (newest first), a
//! viewer for one picture, and open-folder / copy-path / delete. Pictures are
//! decoded off the render thread, only for the rows on screen.
#![allow(dead_code)]

use super::menu::empty;
use super::scroll::Scroll;
use super::ui::{self, Ui};
use horizon_client::native_ui::{Canvas, Image, font, icons, theme};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;

const MAX_SHOTS: usize = 600;

/// One screenshot file.
#[derive(Clone, Debug, PartialEq)]
pub struct Shot {
    pub path: PathBuf,
    pub name: String,
    pub when: String,
    pub size: String,
    pub modified: u64,
}

impl Shot {
    fn new(path: PathBuf, modified: u64, bytes: u64) -> Shot {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let size = if bytes >= 1_048_576 {
            format!("{:.1} MB", bytes as f64 / 1_048_576.0)
        } else {
            format!("{} KB", (bytes / 1024).max(1))
        };
        Shot { when: date_time(modified), path, name, size, modified }
    }

    fn key(&self) -> String {
        format!("{}@{}", self.name, self.modified)
    }
}

/// "5 Nov 2026, 14:03" (UTC, or `NATIVE_UTC_OFFSET` seconds from it).
fn date_time(unix_ms: u64) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let offset: i64 = std::env::var("NATIVE_UTC_OFFSET").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    let secs = (unix_ms / 1000) as i64 + offset;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{day} {} {year}, {:02}:{:02}", MONTHS[(month - 1) as usize], rem / 3600, rem % 3600 / 60)
}

/// Files of the screenshots folder, newest first.
pub fn list(dir: &Path) -> Vec<Shot> {
    let Ok(read) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<Shot> = read
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let name = path.file_name()?.to_string_lossy().to_lowercase();
            if !(name.ends_with(".png") || name.ends_with(".jpg") || name.ends_with(".jpeg")) {
                return None;
            }
            let meta = e.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            let modified = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_millis() as u64;
            Some(Shot::new(path, modified, meta.len()))
        })
        .collect();
    out.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.name.cmp(&b.name)));
    out.truncate(MAX_SHOTS);
    out
}

#[derive(Default)]
struct Shared {
    shots: Option<Vec<Shot>>,
    thumbs: Vec<(String, Arc<Image>)>,
    big: Option<(String, Option<Arc<Image>>)>,
}

pub struct Screenshots {
    scroll: Scroll,
    shared: Arc<Mutex<Shared>>,
    shots: Vec<Shot>,
    loaded: bool,
    scanned_at: f64,
    thumbs: HashMap<String, Arc<Image>>,
    pending: HashSet<String>,
    open: Option<Shot>,
    big: Option<Arc<Image>>,
    big_loading: bool,
    confirm: Option<String>,
    note: Option<(String, f64)>,
    dir: Option<PathBuf>,
}

impl Default for Screenshots {
    fn default() -> Self {
        Self::new()
    }
}

impl Screenshots {
    pub fn new() -> Self {
        Screenshots {
            scroll: Scroll::new("shots"),
            shared: Arc::default(),
            shots: Vec::new(),
            loaded: false,
            scanned_at: 0.0,
            thumbs: HashMap::new(),
            pending: HashSet::new(),
            open: None,
            big: None,
            big_loading: false,
            confirm: None,
            note: None,
            dir: None,
        }
    }

    /// The page was opened (or the menu came back): rescan the folder.
    pub fn refresh(&mut self, now: f64) {
        self.scanned_at = now;
        let (shared, dir) = (self.shared.clone(), self.dir.clone());
        std::thread::Builder::new()
            .name("Native screenshots".into())
            .spawn(move || {
                let shots = dir.map(|d| list(&d)).unwrap_or_default();
                shared.lock().unwrap_or_else(|e| e.into_inner()).shots = Some(shots);
            })
            .ok();
    }

    /// Esc: closes the viewer first. True when it was consumed.
    pub fn escape(&mut self) -> bool {
        if self.confirm.take().is_some() {
            return true;
        }
        if self.open.is_some() {
            self.close();
            return true;
        }
        false
    }

    fn close(&mut self) {
        self.open = None;
        self.big = None;
        self.big_loading = false;
        self.confirm = None;
    }

    /// The page was opened again: look at the folder again.
    pub fn stale(&mut self) {
        self.scanned_at = 0.0;
    }

    fn say(&mut self, text: &str, now: f64) {
        self.note = Some((text.to_owned(), now));
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(&mut self, ui: &mut Ui, c: &mut Canvas, game_dir: Option<&Path>, x: f32, y: f32, w: f32, h: f32) {
        let dir = game_dir.map(|d| d.join("screenshots"));
        if dir != self.dir {
            self.dir = dir;
            self.loaded = false;
            self.scanned_at = 0.0;
        }
        let now = ui.now;
        if (!self.loaded && self.scanned_at == 0.0) || (now - self.scanned_at > 6000.0 && self.open.is_none()) {
            self.refresh(now);
        }
        {
            let mut shared = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(shots) = shared.shots.take() {
                self.shots = shots;
                self.loaded = true;
            }
            for (key, img) in shared.thumbs.drain(..) {
                self.pending.remove(&key);
                self.thumbs.insert(key, img);
            }
            if let Some((key, img)) = shared.big.take()
                && self.open.as_ref().is_some_and(|s| s.key() == key)
            {
                self.big = img;
                self.big_loading = false;
            }
        }
        if self.open.is_some() {
            self.viewer(ui, c, x, y, w, h);
            return;
        }
        let tb = 36.0;
        let count = if !self.loaded {
            "Looking for screenshots...".to_owned()
        } else if self.shots.is_empty() {
            "No screenshots yet".to_owned()
        } else {
            format!("{} {}", self.shots.len(), if self.shots.len() == 1 { "screenshot" } else { "screenshots" })
        };
        let lh = c.line_height(font::MEDIUM, 12.0);
        c.text(font::MEDIUM, 12.0, &count, x + 2.0, y + (tb - lh) / 2.0, theme::TEXT_SECONDARY);
        let bw = 130.0;
        let bx = x + w - bw;
        if ui.button(c, "shots:folder", bx, y, bw, 32.0, "Open folder", theme::I_LAYERS, ui::BTN_GHOST) {
            self.open_folder();
        }
        if ui.icon_button(c, "shots:refresh", bx - 38.0, y, 32.0, theme::I_REFRESH, Some("Rescan the folder")) {
            self.refresh(now);
        }
        if let Some((note, at)) = &self.note
            && now - at < 3500.0
        {
            let nw = c.text_width(font::MEDIUM, 11.5, note);
            let nh = c.line_height(font::MEDIUM, 11.5);
            c.text(font::MEDIUM, 11.5, note, bx - 52.0 - nw, y + (tb - nh) / 2.0, theme::ONLINE);
        }
        let (gy, gh) = (y + tb + 8.0, h - tb - 8.0);
        if self.loaded && self.shots.is_empty() {
            empty(c, x, gy, w, gh, icons::CAMERA, "No screenshots yet", "Press F2 in game to take one: it will show up here.");
            return;
        }
        let gap = 14.0;
        let cols = (((w - 6.0 + gap) / (210.0 + gap)) as usize).max(1);
        let cw = (w - 6.0 - gap * (cols - 1) as f32) / cols as f32;
        let ch = cw * 9.0 / 16.0 + 34.0;
        let off = self.scroll.begin(ui, c, x, gy, w + 6.0, gh);
        let rows = self.shots.len().div_ceil(cols);
        let mut picked = None;
        for i in 0..self.shots.len() {
            let cx = x + (i % cols) as f32 * (cw + gap);
            let cy = gy + (i / cols) as f32 * (ch + gap) - off;
            if cy + ch < gy - 2.0 || cy > gy + gh + 2.0 {
                continue;
            }
            if self.card(ui, c, i, cx, cy, cw, ch) {
                picked = Some(i);
            }
        }
        self.scroll.end(ui, c, rows as f32 * (ch + gap));
        if let Some(i) = picked {
            let shot = self.shots[i].clone();
            self.big = None;
            self.confirm = None;
            self.load_big(&shot);
            self.open = Some(shot);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn card(&mut self, ui: &mut Ui, c: &mut Canvas, index: usize, x: f32, y: f32, w: f32, h: f32) -> bool {
        let shot = self.shots[index].clone();
        let id = format!("shots:card:{index}");
        let over = ui.hover(c, x, y, w, h);
        let hv = ui.anim_on(&format!("{id}#h"), over, 14.0);
        c.round(x, y, w, h, 12.0, theme::mix(0xB808090C, 0xD80E1014, hv));
        c.outline(x, y, w, h, 12.0, 1.0, theme::mix(theme::HAIRLINE, theme::HAIRLINE_STRONG, hv));
        let (ix, iy, iw, ih) = (x + 5.0, y + 5.0, w - 10.0, w * 9.0 / 16.0 - 4.0);
        match self.thumbs.get(&shot.key()).cloned() {
            None => {
                self.load_thumb(&shot);
                c.round(ix, iy, iw, ih, 8.0, 0x12FFFFFF);
                c.icon(icons::CAMERA, 18.0, ix + iw / 2.0, iy + ih / 2.0, theme::TEXT_MUTED);
            }
            Some(t) => {
                c.push_clip(ix, iy, iw, ih);
                c.image_cover(&t, ix, iy, iw, ih, 1.0, 0.0, 0.0, 0xFFFFFFFF);
                c.pop_clip();
            }
        }
        let ty = iy + ih + 6.0;
        let name = c.ellipsize(font::MEDIUM, 11.0, &shot.name, w - 18.0);
        c.text(font::MEDIUM, 11.0, &name, x + 10.0, ty, theme::TEXT);
        c.text(font::REGULAR, 10.0, &format!("{}  \u{b7}  {}", shot.when, shot.size), x + 10.0, ty + 15.0, theme::TEXT_MUTED);
        if over {
            ui.cursor_hand = true;
        }
        ui.clicked(c, &id, x, y, w, h)
    }

    fn load_thumb(&mut self, shot: &Shot) {
        let key = shot.key();
        if self.pending.contains(&key) || self.pending.len() >= 6 {
            return;
        }
        self.pending.insert(key.clone());
        let (shared, path) = (self.shared.clone(), shot.path.clone());
        std::thread::Builder::new()
            .name("Native thumbnails".into())
            .spawn(move || {
                let thumb = std::fs::read(&path).ok().and_then(|b| Image::decode(&b)).map(|full| {
                    let tw = 360;
                    if full.width <= tw {
                        full
                    } else {
                        full.scaled(tw, (full.height * tw / full.width.max(1)).max(1))
                    }
                });
                let thumb = thumb.unwrap_or_else(|| Image::new(1, 1, vec![0x22FFFFFF]));
                shared.lock().unwrap_or_else(|e| e.into_inner()).thumbs.push((key, thumb));
            })
            .ok();
    }

    fn load_big(&mut self, shot: &Shot) {
        self.big_loading = true;
        let (shared, path, key) = (self.shared.clone(), shot.path.clone(), shot.key());
        std::thread::Builder::new()
            .name("Native screenshot".into())
            .spawn(move || {
                let img = std::fs::read(&path).ok().and_then(|b| Image::decode(&b)).map(|full| {
                    let max = 1920;
                    if full.width <= max {
                        full
                    } else {
                        full.scaled(max, (full.height * max / full.width.max(1)).max(1))
                    }
                });
                shared.lock().unwrap_or_else(|e| e.into_inner()).big = Some((key, img));
            })
            .ok();
    }

    fn viewer(&mut self, ui: &mut Ui, c: &mut Canvas, x: f32, y: f32, w: f32, h: f32) {
        let Some(shot) = self.open.clone() else { return };
        if ui.button(c, "shots:back", x, y, 92.0, 32.0, "Back", theme::I_ARROW_LEFT, ui::BTN_GHOST) {
            self.close();
            return;
        }
        let name = c.ellipsize(font::SEMIBOLD, 13.0, &shot.name, w - 560.0);
        c.text(font::SEMIBOLD, 13.0, &name, x + 106.0, y + 2.0, theme::TEXT_STRONG);
        c.text(font::REGULAR, 10.5, &format!("{}  \u{b7}  {}", shot.when, shot.size), x + 106.0, y + 19.0, theme::TEXT_MUTED);
        let mut bx = x + w - 112.0;
        if self.confirm.is_some() {
            if ui.button(c, "shots:yes", bx, y, 112.0, 32.0, "Delete it", theme::I_TRASH, ui::BTN_DANGER) {
                self.delete(&shot, ui.now);
                return;
            }
            bx -= 98.0;
            if ui.button(c, "shots:no", bx, y, 90.0, 32.0, "Keep", theme::I_X, ui::BTN_GHOST) {
                self.confirm = None;
            }
            let ask = "Delete this screenshot for good?";
            let aw = c.text_width(font::MEDIUM, 11.5, ask);
            c.text(font::MEDIUM, 11.5, ask, bx - 14.0 - aw, y + 8.0, theme::DANGER);
        } else {
            if ui.button(c, "shots:del", bx, y, 112.0, 32.0, "Delete", theme::I_TRASH, ui::BTN_GHOST) {
                self.confirm = Some(shot.name.clone());
            }
            bx -= 126.0;
            if ui.button(c, "shots:path", bx, y, 118.0, 32.0, "Copy path", theme::I_HASH, ui::BTN_GHOST) {
                ui.clip_out = Some(shot.path.to_string_lossy().into_owned());
                self.say("Path copied", ui.now);
            }
            bx -= 126.0;
            if ui.button(c, "shots:folder2", bx, y, 118.0, 32.0, "Show in folder", theme::I_LAYERS, ui::BTN_GHOST) {
                self.open_folder();
            }
        }
        let (py, ph) = (y + 46.0, h - 46.0);
        c.round(x, py, w, ph, 12.0, 0x66000000);
        c.outline(x, py, w, ph, 12.0, 1.0, theme::HAIRLINE);
        let Some(img) = self.big.clone() else {
            let t = if self.big_loading { "Loading..." } else { "This picture can't be shown" };
            let tw = c.text_width(font::MEDIUM, 12.0, t);
            c.text(font::MEDIUM, 12.0, t, x + (w - tw) / 2.0, py + ph / 2.0 - 8.0, theme::TEXT_MUTED);
            return;
        };
        let pad = 10.0;
        let (aw, ah) = (w - pad * 2.0, ph - pad * 2.0);
        let k = (aw / img.width as f32).min(ah / img.height as f32);
        let (iw, ih) = (img.width as f32 * k, img.height as f32 * k);
        c.image(&img, x + (w - iw) / 2.0, py + (ph - ih) / 2.0, iw, ih, [0.0, 0.0, 1.0, 1.0], 0xFFFFFFFF);
    }

    fn delete(&mut self, shot: &Shot, now: f64) {
        self.confirm = None;
        let ok = std::fs::remove_file(&shot.path).is_ok();
        self.say(if ok { "Deleted" } else { "Could not delete it" }, now);
        self.close();
        self.refresh(now);
    }

    fn open_folder(&self) {
        let Some(dir) = self.dir.clone() else { return };
        std::thread::Builder::new()
            .name("Native open folder".into())
            .spawn(move || {
                let _ = std::fs::create_dir_all(&dir);
                let cmd = if cfg!(windows) {
                    "explorer"
                } else if cfg!(target_os = "macos") {
                    "open"
                } else {
                    "xdg-open"
                };
                let _ = std::process::Command::new(cmd).arg(&dir).spawn();
            })
            .ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_read_like_the_java_mod() {
        // 2026-11-05 14:03:00 UTC.
        assert_eq!(date_time(1_793_887_380_000), "5 Nov 2026, 14:03");
        assert_eq!(date_time(0), "1 Jan 1970, 00:00");
    }

    #[test]
    fn the_folder_lists_pictures_newest_first() {
        let dir = std::env::temp_dir().join(format!("native-shots-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.png"), [0u8; 10]).unwrap();
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        let shots = list(&dir);
        assert_eq!(shots.len(), 1);
        assert_eq!(shots[0].name, "a.png");
        assert_eq!(shots[0].size, "1 KB");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
