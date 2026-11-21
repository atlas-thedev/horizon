//! Small helpers the views share: word wrapping (`TextLayout`), the red
//! count badge and empty-state card (`TitleView`), and the local time.
#![allow(dead_code)]

use horizon_client::native_ui::{Canvas, font, theme};

/// Word wrapping in logical units (`TextLayout.wrap`).
pub fn wrap(c: &mut Canvas, face: usize, size: f32, text: &str, max_w: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        wrap_paragraph(c, face, size, para, max_w, &mut lines);
    }
    lines
}

fn wrap_paragraph(c: &mut Canvas, face: usize, size: f32, text: &str, max_w: f32, out: &mut Vec<String>) {
    if text.is_empty() {
        out.push(String::new());
        return;
    }
    let mut line = String::new();
    let mut line_w = 0.0;
    let space = c.text_width(face, size, " ");
    for word in text.split(' ') {
        let ww = c.text_width(face, size, word);
        if !line.is_empty() && line_w + space + ww > max_w {
            out.push(std::mem::take(&mut line));
            line_w = 0.0;
        }
        if ww > max_w {
            // Break a long word by characters.
            for ch in word.chars() {
                let cw = c.text_width(face, size, ch.encode_utf8(&mut [0; 4]));
                if line_w + cw > max_w && !line.is_empty() {
                    out.push(std::mem::take(&mut line));
                    line_w = 0.0;
                }
                line.push(ch);
                line_w += cw;
            }
            continue;
        }
        if !line.is_empty() {
            line.push(' ');
            line_w += space;
        }
        line.push_str(word);
        line_w += ww;
    }
    out.push(line);
}

/// A red count badge whose right edge is at `right_x`, centred on `cy` (`TitleView.badge`).
pub fn badge(c: &mut Canvas, right_x: f32, cy: f32, n: i32) {
    let s = if n > 99 { "99+".to_owned() } else { n.to_string() };
    let tw = c.text_width(font::BOLD, 10.5, &s);
    let bw = (tw + 10.0).max(18.0);
    c.round(right_x - bw, cy - 9.0, bw, 18.0, 9.0, theme::DANGER);
    let lh = c.line_height(font::BOLD, 10.5);
    c.text(font::BOLD, 10.5, &s, right_x - bw + (bw - tw) / 2.0, cy - lh / 2.0, 0xFFFFFFFF);
}

/// An icon, a title and a wrapped line, centred in the box (`TitleView.empty`).
#[allow(clippy::too_many_arguments)]
pub fn empty(c: &mut Canvas, x: f32, y: f32, w: f32, h: f32, icon: u32, title: &str, body: &str) {
    let cy = y + h / 2.0 - 34.0;
    c.round(x + w / 2.0 - 22.0, cy - 22.0, 44.0, 44.0, 12.0, theme::SUBTLE);
    c.icon(icon, 20.0, x + w / 2.0, cy, theme::TEXT_SECONDARY);
    let tw = c.text_width(font::SEMIBOLD, 13.5, title);
    c.text(font::SEMIBOLD, 13.5, title, x + (w - tw) / 2.0, cy + 32.0, theme::TEXT_STRONG);
    let mut ly = cy + 54.0;
    for line in wrap(c, font::REGULAR, 11.5, body, (w - 48.0).min(280.0)) {
        let lw = c.text_width(font::REGULAR, 11.5, &line);
        c.text(font::REGULAR, 11.5, &line, x + (w - lw) / 2.0, ly, theme::TEXT_MUTED);
        ly += 17.0;
    }
}

/// Opens an http(s) link in the default browser (anything else is ignored).
pub fn open_url(url: &str) {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return;
    }
    let url = url.to_owned();
    let _ = std::thread::Builder::new().name("Native open url".into()).spawn(move || {
        let _ = if cfg!(windows) {
            std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", &url]).spawn()
        } else if cfg!(target_os = "macos") {
            std::process::Command::new("open").arg(&url).spawn()
        } else {
            std::process::Command::new("xdg-open").arg(&url).spawn()
        };
    });
}

/// The local time zone's offset from UTC in ms: `NATIVE_UTC_OFFSET` (seconds) when the launcher sets it, else UTC.
pub fn utc_offset_ms() -> i64 {
    std::env::var("NATIVE_UTC_OFFSET").ok().and_then(|v| v.parse::<i64>().ok()).unwrap_or(0) * 1000
}

fn local(t: i64) -> i64 {
    t + utc_offset_ms()
}

/// "18:44".
pub fn hhmm(t: i64) -> String {
    let secs = local(t).div_euclid(1000).rem_euclid(86_400);
    format!("{:02}:{:02}", secs / 3600, secs % 3600 / 60)
}

/// Days since 1970-01-01 in local time.
pub fn day_of(t: i64) -> i64 {
    local(t).div_euclid(86_400_000)
}

pub fn same_day(a: i64, b: i64) -> bool {
    day_of(a) == day_of(b)
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const MONTHS_LONG: [&str; 12] =
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const WEEKDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

/// "3 Jun".
pub fn short_day(t: i64) -> String {
    let (_, m, d) = super::relay::civil(day_of(t));
    format!("{d} {}", MONTHS[(m - 1) as usize])
}

/// "18:44" today, "Yesterday", else "3 Jun".
pub fn short_time(t: i64, now: i64) -> String {
    if t <= 0 {
        String::new()
    } else if same_day(t, now) {
        hhmm(t)
    } else if same_day(t, now - 86_400_000) {
        "Yesterday".to_owned()
    } else {
        short_day(t)
    }
}

/// "Today", "Yesterday" or "Wednesday, 3 June".
pub fn day_label(t: i64, now: i64) -> String {
    if same_day(t, now) {
        return "Today".to_owned();
    }
    if same_day(t, now - 86_400_000) {
        return "Yesterday".to_owned();
    }
    let days = day_of(t);
    let (_, m, d) = super::relay::civil(days);
    format!("{}, {d} {}", WEEKDAYS[(days + 4).rem_euclid(7) as usize], MONTHS_LONG[(m - 1) as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_read_like_the_chat_shows_them() {
        // 2026-11-05 14:03 UTC, a Thursday.
        let t = 1_793_887_380_000;
        assert_eq!(hhmm(t), "14:03");
        assert_eq!(short_day(t), "5 Nov");
        assert_eq!(day_label(t, t + 86_400_000 * 3), "Thursday, 5 November");
        assert_eq!(day_label(t, t), "Today");
        assert_eq!(day_label(t, t + 86_400_000), "Yesterday");
        assert_eq!(short_time(t, t), "14:03");
        assert_eq!(short_time(t, t + 86_400_000), "Yesterday");
        assert_eq!(short_time(0, t), "");
    }

    #[test]
    fn long_words_break_by_characters() {
        let mut c = horizon_client::native_ui::canvas();
        c.begin(100, 100, 1.0);
        let lines = wrap(&mut c, font::REGULAR, 12.0, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa bb", 30.0);
        assert!(lines.len() > 2, "{lines:?}");
        assert!(wrap(&mut c, font::REGULAR, 12.0, "", 30.0) == vec![String::new()]);
    }
}
