//! `MusicView`: the Music page of the Native menu: your own songs from the
//! music folder every instance shares. Playlists (its sub-folders) on the left
//! like the Mods categories; on the right the song playing with its controls,
//! then a searchable list of songs. Same cards, hairlines and white-on-black
//! controls as the rest of the menu.

use super::menu::empty;
use super::music::{self, Track};
use super::scroll::Scroll;
use super::setting::Setting;
use super::textfield::TextField;
use super::ui::{self, Ui};
use super::uiconfig::UiConfig;
use super::widgets::Widgets;
use crate::native_ui::{Canvas, font, icons, theme};
use std::sync::Arc;

const CARD: u32 = 0xB808090C;
const CARD_HOVER: u32 = 0xD80E1014;

pub struct MusicView {
    list: Scroll,
    side: Scroll,
    search: TextField,
    widgets: Widgets,
    volume: Setting,
    bars: [f32; 30],
    seek_drag: Option<f32>,
}

impl Default for MusicView {
    fn default() -> Self {
        Self::new()
    }
}

impl MusicView {
    pub fn new() -> Self {
        let mut search = TextField::new("music:search");
        search.max_length = 40;
        MusicView {
            list: Scroll::new("music-list"),
            side: Scroll::new("music-side"),
            search,
            widgets: Widgets::default(),
            volume: Setting::num("musicvol", "Volume", 70.0, 0.0, 100.0, 1.0, "%"),
            bars: [0.0; 30],
            seek_drag: None,
        }
    }

    /// Esc: clears the search first. True when it was consumed.
    pub fn escape(&mut self, ui: &mut Ui) -> bool {
        if self.search.focused(ui) {
            ui.focus = None;
            if !self.search.is_empty() {
                self.search.clear();
            }
            return true;
        }
        false
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(&mut self, ui: &mut Ui, c: &mut Canvas, cfg: &mut UiConfig, x: f32, y: f32, w: f32, h: f32) {
        let wide = w >= 620.0;
        let cw = if wide { 196.0f32.min(w * 0.24) } else { 0.0 };
        if wide {
            self.library(ui, c, cfg, x, y, cw, h);
            c.fill(x + cw + 12.0, y, 1.0, h, theme::HAIRLINE);
        }
        let (gx, gw) = if wide { (x + cw + 26.0, w - cw - 26.0) } else { (x, w) };
        let card_h = 112.0;
        self.now_playing(ui, c, cfg, gx, y, gw, card_h);

        // Toolbar: search, how many songs, volume.
        let (ty, th) = (y + card_h + 12.0, 34.0);
        let vw = 120.0f32.min(gw * 0.22);
        let vx = gx + gw - vw;
        let held = ui.is_pressing("music:vol");
        if !held {
            self.volume.set_num((cfg.music_volume * 100.0).round());
        }
        let vol = self.volume.as_f32();
        c.icon(
            if vol <= 0.0 { theme::I_VOLUME_OFF } else if vol < 50.0 { theme::I_VOLUME_LOW } else { theme::I_VOLUME },
            15.0,
            vx - 16.0,
            ty + th / 2.0,
            theme::TEXT_SECONDARY,
        );
        if self.widgets.slider(ui, c, "music:vol", vx, ty + (th - 18.0) / 2.0, vw, &mut self.volume) {
            cfg.music_volume = self.volume.as_f32() / 100.0;
            music::set_volume(cfg.music_volume); // Heard while dragging.
        }
        if ui.released && ui.active.as_deref() == Some("music:vol") {
            cfg.save(); // Saved once the slider is let go.
        }
        let sw = (vx - 40.0 - gx - 120.0).clamp(140.0, 320.0);
        self.search_box(ui, c, gx, ty, sw, th);
        let songs = self.filtered();
        let err = music::error();
        let library = music::library();
        let info = if let Some(e) = &err {
            e.clone()
        } else if library.is_empty() {
            if music::scanning() { "Looking for songs...".to_owned() } else { "No songs yet".to_owned() }
        } else {
            format!("{} {}", songs.len(), if songs.len() == 1 { "song" } else { "songs" })
        };
        let iw = vx - 30.0 - (gx + sw + 14.0);
        if iw > 40.0 {
            let shown = c.ellipsize(font::MEDIUM, 11.5, &info, iw);
            let lh = c.line_height(font::MEDIUM, 11.5);
            c.text(font::MEDIUM, 11.5, &shown, gx + sw + 14.0, ty + (th - lh) / 2.0, if err.is_some() { theme::DANGER } else { theme::TEXT_MUTED });
        }

        // Songs.
        let mut ly = ty + th + 10.0;
        let mut lh = y + h - ly;
        if !wide {
            ly = self.playlist_chips(ui, c, gx, ly, gw);
            lh = y + h - ly;
        }
        if library.is_empty() {
            self.empty_library(ui, c, gx, ly, gw, lh);
        } else if songs.is_empty() {
            empty(c, gx, ly, gw, lh, theme::I_SEARCH, "Nothing matches", "Try another word, or pick All songs.");
        } else {
            self.song_list(ui, c, &songs, gx, ly, gw, lh);
        }
    }

    // ── left column: library ─────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn library(&mut self, ui: &mut Ui, c: &mut Canvas, cfg: &mut UiConfig, x: f32, y: f32, w: f32, h: f32) {
        c.text(font::SEMIBOLD, 10.0, "LIBRARY", x + 6.0, y + 2.0, theme::TEXT_MUTED);
        let lists = music::playlists();
        let all = music::library();
        let chosen = music::playlist();
        let foot_h = 104.0;
        let (ry0, rh) = (y + 24.0, 38.0);
        let off = self.side.begin(ui, c, x - 6.0, ry0, w + 12.0, h - 24.0 - foot_h);
        let mut ry = ry0 - off;
        ry = self.library_row(ui, c, "music:pl:*", x, ry, w, rh, theme::I_LIST_MUSIC, "All songs", all.len(), chosen.is_none(), None) + 4.0;
        for p in lists.iter() {
            let n = all.iter().filter(|t| t.playlist.as_deref() == Some(p)).count();
            ry = self.library_row(ui, c, &format!("music:pl:{p}"), x, ry, w, rh, theme::I_FOLDER_OPEN, p, n, chosen.as_deref() == Some(p), Some(p.clone())) + 4.0;
        }
        self.side.end(ui, c, ry + off - ry0);

        // Folder, refresh, quieter in calls.
        let fy = y + h - foot_h + 10.0;
        c.fill(x, fy - 10.0, w, 1.0, theme::HAIRLINE);
        if ui.button(c, "music:folder", x, fy, w - 38.0, 32.0, "Music folder", theme::I_FOLDER_OPEN, ui::BTN_GHOST) {
            music::open_folder();
        }
        if ui.icon_button(c, "music:refresh", x + w - 32.0, fy, 32.0, theme::I_REFRESH_CW, Some("Look for new songs")) {
            music::rescan_soon();
        }
        let qy = fy + 44.0;
        let lh = c.line_height(font::MEDIUM, 11.5);
        c.text(font::MEDIUM, 11.5, "Quieter in calls", x + 2.0, qy + 10.0 - lh / 2.0, theme::TEXT_SECONDARY);
        if ui.toggle(c, "music:duck", x + w - 34.0, qy, cfg.music_duck) {
            cfg.music_duck = !cfg.music_duck;
            cfg.save();
        }
        let hint = c.ellipsize(font::REGULAR, 10.0, "MP3, OGG, WAV \u{b7} all instances", w);
        c.text(font::REGULAR, 10.0, &hint, x + 2.0, qy + 30.0, theme::TEXT_MUTED);
    }

    #[allow(clippy::too_many_arguments)]
    fn library_row(&mut self, ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, w: f32, h: f32, icon: u32, label: &str, count: usize, on: bool, playlist: Option<String>) -> f32 {
        let over = ui.hover(c, x, y, w, h);
        if ui.clicked(c, id, x, y, w, h) && !on {
            music::set_playlist(playlist);
            self.list.reset();
        }
        let sel = ui.anim_on(&format!("{id}#on"), on, 16.0);
        let hv = ui.anim_on(&format!("{id}#h"), over, 14.0);
        c.round(x, y, w, h, 10.0, theme::mix(theme::alpha(theme::SUBTLE_HOVER, hv * 0.7), 0x16FFFFFF, sel));
        if sel > 0.01 {
            c.outline(x, y, w, h, 10.0, 1.0, theme::alpha(theme::HAIRLINE_STRONG, sel));
            c.round(x - 5.0, y + (h - 18.0 * sel) / 2.0, 3.0, 18.0 * sel, 1.5, theme::alpha(theme::WHITE, sel));
        }
        let fg = theme::mix(theme::mix(theme::TEXT_SECONDARY, theme::TEXT_STRONG, hv), theme::TEXT_STRONG, sel);
        c.icon(icon, 15.0, x + 22.0, y + h / 2.0, fg);
        let n = count.to_string();
        let nw = c.text_width(font::MEDIUM, 10.5, &n);
        let name = c.ellipsize(font::SEMIBOLD, 12.5, label, w - 40.0 - nw - 30.0);
        let lh = c.line_height(font::SEMIBOLD, 12.5);
        c.text(font::SEMIBOLD, 12.5, &name, x + 40.0, y + (h - lh) / 2.0, fg);
        c.round(x + w - nw - 22.0, y + (h - 20.0) / 2.0, nw + 12.0, 20.0, 10.0, theme::mix(theme::SUBTLE, 0xFFF4F4F5, sel));
        let nh = c.line_height(font::MEDIUM, 10.5);
        c.text(font::MEDIUM, 10.5, &n, x + w - nw - 16.0, y + (h - nh) / 2.0, theme::mix(theme::TEXT_MUTED, theme::SOLID_FG, sel));
        y + h
    }

    /// Narrow pages: the playlists as a row of chips above the songs. Returns where the songs start.
    fn playlist_chips(&mut self, ui: &mut Ui, c: &mut Canvas, x: f32, y: f32, w: f32) -> f32 {
        let lists = music::playlists();
        if lists.is_empty() {
            return y;
        }
        let chosen = music::playlist();
        let mut cx = self.chip(ui, c, "music:chip:*", x, y, "All songs", chosen.is_none(), None);
        for p in lists.iter() {
            if cx > x + w - 60.0 {
                break;
            }
            cx = self.chip(ui, c, &format!("music:chip:{p}"), cx, y, p, chosen.as_deref() == Some(p), Some(p.clone()));
        }
        y + 36.0
    }

    #[allow(clippy::too_many_arguments)]
    fn chip(&mut self, ui: &mut Ui, c: &mut Canvas, id: &str, x: f32, y: f32, label: &str, on: bool, playlist: Option<String>) -> f32 {
        let text = c.ellipsize(font::SEMIBOLD, 11.5, label, 140.0);
        let (w, h) = (c.text_width(font::SEMIBOLD, 11.5, &text) + 24.0, 26.0);
        let over = ui.hover(c, x, y, w, h);
        c.round(x, y, w, h, h / 2.0, if on { 0xFFF4F4F5 } else if over { theme::SUBTLE_HOVER } else { theme::SUBTLE });
        let lh = c.line_height(font::SEMIBOLD, 11.5);
        c.text(font::SEMIBOLD, 11.5, &text, x + 12.0, y + (h - lh) / 2.0, if on { theme::SOLID_FG } else { theme::TEXT_SECONDARY });
        if ui.clicked(c, id, x, y, w, h) && !on {
            music::set_playlist(playlist);
            self.list.reset();
        }
        x + w + 8.0
    }

    // ── the song playing ─────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn now_playing(&mut self, ui: &mut Ui, c: &mut Canvas, cfg: &mut UiConfig, x: f32, y: f32, w: f32, h: f32) {
        c.round(x, y, w, h, 12.0, CARD);
        c.outline(x, y, w, h, 12.0, 1.0, theme::HAIRLINE);
        let t = music::current();
        let on = music::playing();
        let cs = h - 24.0;
        cover_art(c, t.as_ref(), x + 12.0, y + 12.0, cs, 10.0);

        // Controls on the right: shuffle, previous, play, next, repeat.
        let (ctrl_w, ccy) = (196.0f32, y + 40.0);
        let ccx = x + w - 16.0 - ctrl_w / 2.0;
        if control(ui, c, "music:shuffle", ccx - 78.0, ccy, 30.0, theme::I_SHUFFLE, cfg.music_shuffle, if cfg.music_shuffle { "Shuffle on" } else { "Shuffle off" }) {
            cfg.music_shuffle = !cfg.music_shuffle;
            cfg.save();
        }
        if control(ui, c, "music:prev", ccx - 40.0, ccy, 32.0, theme::I_SKIP_BACK, false, "Previous") {
            music::previous();
        }
        let pr = 19.0;
        let p_over = ui.hover(c, ccx - pr, ccy - pr, pr * 2.0, pr * 2.0);
        let press = ui.anim_on("music:play#p", ui.is_pressing("music:play"), 22.0);
        let ph = ui.anim_on("music:play#h", p_over, 14.0);
        c.circle(ccx, ccy, pr * (1.0 - 0.06 * press), theme::mix(0xFFF4F4F5, 0xFFFFFFFF, ph));
        c.icon(if on { theme::I_PAUSE } else { theme::I_PLAY }, 16.0, ccx + if on { 0.0 } else { 1.5 }, ccy, theme::SOLID_FG);
        if ui.clicked(c, "music:play", ccx - pr, ccy - pr, pr * 2.0, pr * 2.0) {
            music::toggle();
        }
        if p_over {
            ui.cursor_hand = true;
            ui.tip(if on { "Pause" } else { "Play" });
        }
        if control(ui, c, "music:next", ccx + 40.0, ccy, 32.0, theme::I_SKIP_FORWARD, false, "Next") {
            music::next();
        }
        let rep = cfg.music_repeat;
        let tip = match rep {
            0 => "Repeat off",
            1 => "Repeat all songs",
            _ => "Repeat this song",
        };
        if control(ui, c, "music:repeat", ccx + 78.0, ccy, 30.0, if rep == 2 { theme::I_REPEAT_ONE } else { theme::I_REPEAT }, rep != 0, tip) {
            cfg.music_repeat = (rep + 1) % 3;
            cfg.save();
        }

        // Title, artist and the sound bars.
        let tx = x + 12.0 + cs + 16.0;
        let tw = ccx - ctrl_w / 2.0 - 16.0 - tx;
        let status = if t.is_none() { "NOTHING PLAYING" } else if on { "NOW PLAYING" } else { "PAUSED" };
        c.text(font::SEMIBOLD, 10.0, status, tx, y + 14.0, theme::TEXT_MUTED);
        let title = t.as_ref().map_or("Pick a song".to_owned(), |t| t.title());
        let title = c.ellipsize(font::SEMIBOLD, 16.0, &title, tw);
        c.text(font::SEMIBOLD, 16.0, &title, tx, y + 29.0, theme::TEXT_STRONG);
        let sub = match &t {
            None => "Your songs play here, in menus and in game".to_owned(),
            Some(t) => match (t.album(), t.artist()) {
                (Some(al), Some(_)) => format!("{} \u{b7} {al}", t.subtitle()),
                _ => t.subtitle(),
            },
        };
        let sub = c.ellipsize(font::REGULAR, 12.0, &sub, tw);
        c.text(font::REGULAR, 12.0, &sub, tx, y + 50.0, theme::TEXT_SECONDARY);
        if tw > 60.0 && t.is_some() {
            music::bars(&mut self.bars);
            sound_bars(c, &self.bars, tx, y + 66.0, tw.min(220.0), 14.0, if on { 0.55 } else { 0.18 });
        }

        // The seek bar along the bottom.
        let (pos, dur) = (music::position(), music::duration());
        let sy = y + h - 22.0;
        let left = match self.seek_drag {
            Some(f) if dur > 0 => music::time((f * dur as f32) as i64),
            _ if t.is_none() => music::time(-1),
            _ => music::time(pos),
        };
        let right = music::time(dur);
        let lw = c.text_width(font::MEDIUM, 10.5, "00:00");
        let sx = tx + lw + 10.0;
        let sw = x + w - 16.0 - lw - 10.0 - sx;
        let over = ui.hover(c, sx - 4.0, sy - 7.0, sw + 8.0, 20.0);
        ui.clicked(c, "music:seek", sx - 4.0, sy - 7.0, sw + 8.0, 20.0);
        let drag = ui.is_pressing("music:seek") && dur > 0;
        if drag {
            self.seek_drag = Some(((ui.mx - sx) / sw).clamp(0.0, 1.0));
        } else if let Some(f) = self.seek_drag.take() {
            music::seek((f * dur as f32) as i64);
        }
        let frac = self.seek_drag.unwrap_or(if dur > 0 { (pos as f32 / dur as f32).min(1.0) } else { 0.0 });
        let hv = ui.anim_on("music:seek#h", over || drag, 14.0);
        c.round(sx, sy, sw, 4.0, 2.0, theme::SUBTLE_HOVER);
        c.round(sx, sy, (sw * frac).max(4.0), 4.0, 2.0, theme::mix(0xFFD4D4D8, 0xFFFFFFFF, hv));
        if hv > 0.01 {
            c.circle(sx + sw * frac, sy + 2.0, 6.0 * hv, 0xFFFFFFFF);
        }
        if over || drag {
            ui.cursor_hand = true;
        }
        let ly = sy + 2.0 - c.line_height(font::MEDIUM, 10.5) / 2.0;
        let lwid = c.text_width(font::MEDIUM, 10.5, &left);
        c.text(font::MEDIUM, 10.5, &left, sx - 10.0 - lwid, ly, theme::TEXT_MUTED);
        c.text(font::MEDIUM, 10.5, &right, sx + sw + 10.0, ly, theme::TEXT_MUTED);
    }

    // ── search + songs ───────────────────────────────────────────────────

    fn search_box(&mut self, ui: &mut Ui, c: &mut Canvas, x: f32, y: f32, w: f32, h: f32) {
        let fa = ui.anim_on("music:search#f", self.search.focused(ui), 14.0);
        c.round(x, y, w, h, h / 2.0, theme::mix(CARD, 0xE00E1014, fa));
        c.outline(x, y, w, h, h / 2.0, 1.0, theme::mix(theme::HAIRLINE_STRONG, theme::BORDER_HOVER, fa));
        c.icon(theme::I_SEARCH, 14.0, x + 18.0, y + h / 2.0, theme::TEXT_MUTED);
        c.push_clip(x + 30.0, y + 1.0, w - 42.0, h - 2.0);
        self.search.draw(ui, c, x + 20.0, y, w - 24.0, h, "Search songs", 12.5);
        c.pop_clip();
    }

    fn filtered(&self) -> Vec<Arc<Track>> {
        let songs = music::visible();
        let q = self.search.value().trim().to_lowercase();
        if q.is_empty() {
            return songs;
        }
        songs
            .into_iter()
            .filter(|t| format!("{} {} {}", t.title(), t.artist().unwrap_or_default(), t.album().unwrap_or_default()).to_lowercase().contains(&q))
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    fn song_list(&mut self, ui: &mut Ui, c: &mut Canvas, songs: &[Arc<Track>], x: f32, y: f32, w: f32, h: f32) {
        let cur = music::current();
        let on = music::playing();
        let (row_h, gap) = (46.0f32, 4.0f32);
        let album_col = w >= 640.0;
        let off = self.list.begin(ui, c, x - 6.0, y, w + 12.0, h);
        let mut ry = y - off;
        for (i, s) in songs.iter().enumerate() {
            let at = ry;
            ry += row_h + gap;
            if at + row_h < y - 2.0 || at > y + h + 2.0 {
                continue; // Off screen.
            }
            let playing_this = cur.as_ref().is_some_and(|c| Arc::ptr_eq(c, s));
            let id = format!("music:row:{}", s.rel);
            let over = ui.hover(c, x, at, w, row_h) && c.in_clip(ui.mx, ui.my);
            let hv = ui.anim_on(&format!("{id}#h"), over, 14.0);
            let sel = ui.anim_on(&format!("{id}#on"), playing_this, 16.0);
            c.round(x, at, w, row_h, 10.0, theme::mix(theme::mix(CARD, CARD_HOVER, hv), 0x16FFFFFF, sel));
            c.outline(x, at, w, row_h, 10.0, 1.0, theme::mix(theme::mix(theme::HAIRLINE, theme::HAIRLINE_STRONG, hv), theme::HAIRLINE_STRONG, sel));
            if sel > 0.01 {
                c.round(x - 5.0, at + (row_h - 18.0 * sel) / 2.0, 3.0, 18.0 * sel, 1.5, theme::alpha(theme::WHITE, sel));
            }
            let (mid, ix) = (at + row_h / 2.0, x + 22.0);
            if playing_this && on {
                for b in 0..3 {
                    let bh = 4.0 + 8.0 * (0.5 + 0.5 * ((ui.now / (150.0 + f64::from(b) * 40.0) + f64::from(b) * 1.7).sin() as f32));
                    c.round(ix - 6.0 + b as f32 * 5.0, mid + 6.0 - bh, 3.0, bh, 1.0, theme::TEXT_STRONG);
                }
            } else if over {
                c.icon(theme::I_PLAY, 12.0, ix, mid, theme::TEXT_STRONG);
            } else {
                let n = (i + 1).to_string();
                let nw = c.text_width(font::MEDIUM, 11.0, &n);
                let nh = c.line_height(font::MEDIUM, 11.0);
                c.text(font::MEDIUM, 11.0, &n, ix - nw / 2.0, mid - nh / 2.0, if playing_this { theme::TEXT_STRONG } else { theme::TEXT_MUTED });
            }
            let th = row_h - 14.0;
            cover_art(c, Some(s), x + 42.0, at + 7.0, th, 6.0);
            let tx = x + 42.0 + th + 12.0;
            let dur = s.duration_ms();
            let d = music::time(dur);
            let dw = if dur > 0 { c.text_width(font::MEDIUM, 11.0, &d) } else { 0.0 };
            let album_x = if album_col { x + w * 0.58 } else { 0.0 };
            let tw = (if album_col { album_x - 16.0 } else { x + w - 18.0 - dw - 12.0 }) - tx;
            let error = s.error();
            let title_color = if playing_this { theme::TEXT_STRONG } else if error.is_some() { theme::TEXT_MUTED } else { theme::TEXT };
            let title = c.ellipsize(font::SEMIBOLD, 12.5, &s.title(), tw);
            c.text(font::SEMIBOLD, 12.5, &title, tx, at + 8.0, title_color);
            let sub = error.clone().or_else(|| s.artist()).unwrap_or_else(|| "Unknown artist".to_owned());
            let sub = c.ellipsize(font::REGULAR, 11.0, &sub, tw);
            c.text(font::REGULAR, 11.0, &sub, tx, at + 25.0, if error.is_some() { theme::DANGER } else { theme::TEXT_MUTED });
            if let (true, Some(album)) = (album_col, s.album()) {
                let aw = x + w - 18.0 - dw - 16.0 - album_x;
                let album = c.ellipsize(font::REGULAR, 11.5, &album, aw);
                let ah = c.line_height(font::REGULAR, 11.5);
                c.text(font::REGULAR, 11.5, &album, album_x, mid - ah / 2.0, theme::TEXT_MUTED);
            }
            if dw > 0.0 {
                let dh = c.line_height(font::MEDIUM, 11.0);
                c.text(font::MEDIUM, 11.0, &d, x + w - 18.0 - dw, mid - dh / 2.0, theme::TEXT_MUTED);
            }
            if ui.clicked(c, &id, x, at, w, row_h) {
                if playing_this { music::toggle() } else { music::play(s) }
            }
            if over {
                ui.cursor_hand = true;
            }
        }
        self.list.end(ui, c, songs.len() as f32 * (row_h + gap));
    }

    #[allow(clippy::too_many_arguments)]
    fn empty_library(&mut self, ui: &mut Ui, c: &mut Canvas, x: f32, y: f32, w: f32, h: f32) {
        if music::scanning() {
            ui.dots(c, x + w / 2.0, y + h / 2.0 - 20.0, theme::TEXT_MUTED);
            return;
        }
        let eh = h.min(220.0);
        empty(c, x, y, w, eh, icons::MUSIC, "No songs yet", "Put MP3, OGG or WAV files in your music folder. Sub-folders become playlists.");
        let by = y + eh / 2.0 + 44.0;
        if let Some(f) = music::folder() {
            let p = c.ellipsize(font::REGULAR, 10.5, &f.to_string_lossy(), w - 40.0);
            let pw = c.text_width(font::REGULAR, 10.5, &p);
            c.text(font::REGULAR, 10.5, &p, x + (w - pw) / 2.0, by, theme::TEXT_MUTED);
        }
        if ui.button(c, "music:folder:big", x + w / 2.0 - 84.0, by + 24.0, 168.0, 34.0, "Open music folder", theme::I_FOLDER_OPEN, ui::BTN_PRIMARY) {
            music::open_folder();
        }
    }
}

/// One round transport button; true when clicked.
#[allow(clippy::too_many_arguments)]
fn control(ui: &mut Ui, c: &mut Canvas, id: &str, cx: f32, cy: f32, size: f32, icon: u32, on: bool, tip: &str) -> bool {
    let over = ui.hover(c, cx - size / 2.0, cy - size / 2.0, size, size);
    let hv = ui.anim_on(&format!("{id}#h"), over, 16.0);
    if hv > 0.01 {
        c.circle(cx, cy, size / 2.0, theme::alpha(theme::SUBTLE_HOVER, hv));
    }
    let fg = if on { theme::TEXT_STRONG } else { theme::mix(theme::TEXT_MUTED, theme::TEXT_STRONG, hv) };
    c.icon(icon, size * 0.5, cx, cy, fg);
    if on {
        c.circle(cx, cy + size / 2.0 - 1.0, 1.8, theme::TEXT_STRONG); // The dot under an option that is on.
    }
    if over {
        ui.cursor_hand = true;
        ui.tip(tip);
    }
    ui.clicked(c, id, cx - size / 2.0, cy - size / 2.0, size, size)
}

/// The cover, or a quiet placeholder with a note while there is none (also used by the Now Playing card).
pub fn cover_art(c: &mut Canvas, t: Option<&Arc<Track>>, x: f32, y: f32, size: f32, radius: f32) {
    if let Some(t) = t
        && let Some(img) = music::cover(t)
    {
        c.stamp(music::cover_key(t), &img, x, y, size, size, true, 0xFFFFFFFF);
        return;
    }
    c.round(x, y, size, size, radius, 0x14FFFFFF);
    c.outline(x, y, size, size, radius, 1.0, theme::HAIRLINE);
    c.icon(icons::MUSIC, (size * 0.42).max(10.0), x + size / 2.0, y + size / 2.0, theme::TEXT_MUTED);
}

/// Monochrome sound bars, rising from the bottom.
pub fn sound_bars(c: &mut Canvas, levels: &[f32], x: f32, y: f32, w: f32, h: f32, alpha: f32) {
    let n = levels.len();
    if n == 0 {
        return;
    }
    let gap = (w / n as f32 * 0.35).max(1.5);
    let bw = (w - gap * (n - 1) as f32) / n as f32;
    for (i, level) in levels.iter().enumerate() {
        let v = level.clamp(0.08, 1.0);
        let bh = (h * v).max(bw);
        c.round(x + i as f32 * (bw + gap), y + h - bh, bw, bh, (bw / 2.0).min(1.5), theme::alpha(0xFFFFFFFF, alpha * (0.45 + 0.55 * v)));
    }
}
