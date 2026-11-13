//! Native Client's launch screen before wgpu's first frame: the renderer's device and pipelines take seconds
//! to build, and winit's window sits white until a frame is presented. This replaces upstream's boot paint
//! (tools/native/boot_screen.py) and draws the launcher's boot splash with GDI: pure black, the Native mark,
//! "Native Client" and a 132x3 bar with a light stretch sweeping along it, faded in the way the launcher
//! fades it in. The game's own `ui_theme::native_launch_screen` draws the same thing once wgpu takes over.
//!
//! Frames are drawn off screen and copied to the window in one go, so repainting at 30 Hz does not flicker.

use std::sync::{Once, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use windows_sys::Win32::Foundation::{COLORREF, HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    AddFontMemResourceEx, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CLEARTYPE_QUALITY,
    CLIP_DEFAULT_PRECIS, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreateSolidBrush,
    DEFAULT_CHARSET, DT_CENTER, DT_NOCLIP, DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject,
    DrawTextW, FF_DONTCARE, FillRect, GetDC, HALFTONE, OUT_DEFAULT_PRECIS, ReleaseDC, SRCCOPY,
    SelectObject, SetBkMode, SetStretchBltMode, SetTextColor, StretchDIBits, TRANSPARENT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect;

/// The Native mark: 64x64, light on black, one byte per pixel (tools/native/make_mark.py).
const MARK: &[u8] = include_bytes!("native_mark.bin");
const MARK_SIDE: usize = 64;

/// GDI's font table is global to the process: the bundled faces register once across every paint.
static FACES: Once = Once::new();
/// The first paint, which the fade-in counts from.
static START: OnceLock<Instant> = OnceLock::new();

/// A grey of `v` (0..255) as GDI's `0x00bbggrr`.
fn gray(v: f32) -> COLORREF {
    let g = v.clamp(0.0, 255.0).round() as u32;
    g | g << 8 | g << 16
}

/// The launcher's `cubic-bezier(0.65, 0, 0.35, 1)`, as an in-out cubic.
fn ease(t: f32) -> f32 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// Paints the client area of the window at `hwnd`: black, the mark, the name and the bar, centred.
///
/// `semibold` and `mono` are the faces' TTF bytes, which must outlive the process: `AddFontMemResourceEx` keeps
/// referencing them. `_logs` is the console's recent lines, which this screen does not show. Repainting is
/// safe and cheap (a background thread calls this about 30 times a second).
pub fn paint(hwnd: isize, semibold: &'static [u8], mono: &'static [u8], _logs: &[String]) {
    let hwnd = hwnd as HWND;
    FACES.call_once(|| {
        for font in [semibold, mono] {
            let mut count = 0u32;
            unsafe {
                AddFontMemResourceEx(
                    font.as_ptr().cast(),
                    font.len() as u32,
                    std::ptr::null(),
                    (&raw mut count).cast_const(),
                );
            }
        }
    });
    // The launcher's reduced-motion setting: no fade-in, a slower bar.
    let calm = std::env::var_os("NATIVE_REDUCED_MOTION").is_some_and(|v| v != "0" && !v.is_empty());
    let seconds = START.get_or_init(Instant::now).elapsed().as_secs_f32();
    let sweep_ms: u128 = if calm { 2400 } else { 1100 };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let phase = (now % sweep_ms) as f32 / sweep_ms as f32;
    unsafe {
        let window = GetDC(hwnd);
        if window.is_null() {
            return;
        }
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        GetClientRect(hwnd, &mut rect);
        let (iw, ih) = (rect.right - rect.left, rect.bottom - rect.top);
        if iw <= 0 || ih <= 0 {
            let _ = ReleaseDC(hwnd, window);
            return;
        }
        let (w, h) = (iw as f32, ih as f32);
        // Off screen: the whole frame is drawn here, then copied to the window.
        let hdc = CreateCompatibleDC(window);
        let canvas = CreateCompatibleBitmap(window, iw, ih);
        let old_canvas = SelectObject(hdc, canvas);
        // The pure black.
        let black = CreateSolidBrush(0);
        FillRect(hdc, &rect, black);
        let _ = DeleteObject(black);
        // The launcher's layout in dp (72 / 22 / 20 / 22 / 3), as physical pixels per dp like the game's `Ui`.
        let d = (w / 1040.0).min(h / 700.0).clamp(1.0, 4.0);
        let mark = (72.0 * d).round();
        let gap = (22.0 * d).round();
        let name_h = (20.0 * d).round();
        let bar_w = (132.0 * d).round();
        let bar_h = (3.0 * d).round().max(2.0);
        let top = ((h - (mark + gap + name_h + gap + bar_h)) / 2.0).max(0.0).round();
        // The launcher's entrance: each part fades in over half a second, a little later than the one
        // before, and settles 4 dp up from where it starts.
        let rise = |delay: f32| -> (f32, f32) {
            let p = if calm {
                1.0
            } else {
                ((seconds - delay) / 0.5).clamp(0.0, 1.0)
            };
            let e = 1.0 - (1.0 - p).powi(4);
            (e, ((1.0 - e) * 4.0 * d).round())
        };

        // The mark: light on black, so its pixels are the grey they would be over the black.
        let (a, dy) = rise(0.0);
        let mut bits = [0u32; MARK_SIDE * MARK_SIDE];
        for (pixel, v) in bits.iter_mut().zip(MARK.iter()) {
            let g = (f32::from(*v) * a).round() as u32;
            *pixel = g | g << 8 | g << 16;
        }
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = MARK_SIDE as i32;
        info.bmiHeader.biHeight = -(MARK_SIDE as i32);
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        SetStretchBltMode(hdc, HALFTONE as _);
        StretchDIBits(
            hdc,
            ((w - mark) / 2.0) as i32,
            (top + dy) as i32,
            mark as i32,
            mark as i32,
            0,
            0,
            MARK_SIDE as i32,
            MARK_SIDE as i32,
            bits.as_ptr().cast(),
            &info,
            0,
            SRCCOPY as _,
        );

        // The name.
        let (a, dy) = rise(0.06);
        let name: Vec<u16> = "Native Client".encode_utf16().chain(std::iter::once(0)).collect();
        let face: Vec<u16> = "Geist SemiBold".encode_utf16().chain(std::iter::once(0)).collect();
        let font = CreateFontW(
            -((15.0 * d).round() as i32),
            0,
            0,
            0,
            600,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            CLEARTYPE_QUALITY as u32,
            FF_DONTCARE as u32,
            face.as_ptr(),
        );
        let old_font = SelectObject(hdc, font);
        SetBkMode(hdc, TRANSPARENT as i32);
        SetTextColor(hdc, gray(255.0 * a));
        let name_top = top + mark + gap + dy;
        let mut name_rect = RECT {
            left: 0,
            top: name_top as i32,
            right: iw,
            bottom: (name_top + name_h) as i32,
        };
        DrawTextW(
            hdc,
            name.as_ptr(),
            (name.len() - 1) as i32,
            &mut name_rect,
            DT_CENTER | DT_SINGLELINE | DT_NOCLIP | DT_VCENTER,
        );

        // The bar: a faint track and the light stretch sweeping through it, clipped at the track's ends.
        let (a, dy) = rise(0.12);
        let bar_x = ((w - bar_w) / 2.0).round();
        let bar_y = top + mark + gap + name_h + gap + dy;
        let bar = |from: f32, to: f32, v: f32| {
            let r = RECT {
                left: from.round() as i32,
                top: bar_y.round() as i32,
                right: to.round() as i32,
                bottom: (bar_y + bar_h).round() as i32,
            };
            if r.right > r.left {
                let brush = CreateSolidBrush(gray(v));
                FillRect(hdc, &r, brush);
                let _ = DeleteObject(brush);
            }
        };
        bar(bar_x, bar_x + bar_w, 26.0 * a);
        let len = bar_w * 0.4;
        let pos = -len + ease(phase) * (bar_w + len);
        bar(bar_x + pos.max(0.0), bar_x + (pos + len).min(bar_w), 198.0 * a);

        BitBlt(window, 0, 0, iw, ih, hdc, 0, 0, SRCCOPY as _);
        SelectObject(hdc, old_font);
        let _ = DeleteObject(font);
        SelectObject(hdc, old_canvas);
        let _ = DeleteObject(canvas);
        let _ = DeleteDC(hdc);
        let _ = ReleaseDC(hwnd, window);
    }
}
