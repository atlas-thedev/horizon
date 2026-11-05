// Native Client's launch screen in the game (wgpu). tools/native/boot_screen.py copies this file next to
// ui_theme.rs and includes it there; `wordmark` draws it instead of Horizon's wordmark.
//
// The look is the launcher's boot splash (index.html): pure black, the Native mark (72), a 22 gap, the name
// (15, semibold, white), a 22 gap and a 132x3 bar with a 40% light stretch sweeping along it. All in dp, so it
// scales with the window like the rest of the design system. The early GDI screen (boot_paint.rs) draws the
// same thing and handles the fade-in; the bar's phase comes from the wall clock so both screens agree.

/// The Native mark: 64x64, light on black, one byte per pixel (tools/native/make_mark.py).
const NATIVE_MARK: &[u8] = include_bytes!("native_mark.bin");
const NATIVE_MARK_SIDE: usize = 64;

/// One sweep of the bar in milliseconds; the launcher's reduced-motion setting (`NATIVE_REDUCED_MOTION`) slows it.
fn native_sweep_ms() -> u128 {
    static CALM: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let calm = *CALM
        .get_or_init(|| std::env::var_os("NATIVE_REDUCED_MOTION").is_some_and(|v| v != "0" && !v.is_empty()));
    if calm { 2400 } else { 1100 }
}

/// Where the bar is in its sweep (0..1).
fn native_sweep_phase() -> f32 {
    let ms = native_sweep_ms();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    (now % ms) as f32 / ms as f32
}

/// The launcher's `cubic-bezier(0.65, 0, 0.35, 1)`, as an in-out cubic.
fn native_ease(t: f32) -> f32 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

impl Cache {
    /// The mark as a `side`x`side` pixel mask in the atlas (resampled once per size: the atlas samples
    /// without filtering, so the mask must already be the size it is drawn at).
    fn native_mark(&mut self, atlas: &HudAtlas, side: u32) -> Option<Rect> {
        let side = side.clamp(8, 1024);
        let key = Key::Glyph(Face::Mono, u16::MAX, side as u16);
        if let Some(c) = self.entries.get(&key) {
            return c.rect;
        }
        let n = NATIVE_MARK_SIDE;
        if NATIVE_MARK.len() != n * n {
            return None;
        }
        let s = side as usize;
        let k = 3;
        let scale = n as f32 / side as f32;
        let at = |x: f32, y: f32| -> f32 {
            let x = x.clamp(0.0, (n - 1) as f32);
            let y = y.clamp(0.0, (n - 1) as f32);
            let (x0, y0) = (x.floor() as usize, y.floor() as usize);
            let (x1, y1) = ((x0 + 1).min(n - 1), (y0 + 1).min(n - 1));
            let (fx, fy) = (x - x0 as f32, y - y0 as f32);
            let p = |px: usize, py: usize| f32::from(NATIVE_MARK[py * n + px]);
            let top = p(x0, y0) * (1.0 - fx) + p(x1, y0) * fx;
            let bottom = p(x0, y1) * (1.0 - fx) + p(x1, y1) * fx;
            (top * (1.0 - fy) + bottom * fy) / 255.0
        };
        let mut alpha = vec![0.0f32; s * s];
        for dy in 0..s {
            for dx in 0..s {
                let mut sum = 0.0;
                for sy in 0..k {
                    for sx in 0..k {
                        let u = (dx as f32 + (sx as f32 + 0.5) / k as f32) * scale - 0.5;
                        let v = (dy as f32 + (sy as f32 + 0.5) / k as f32) * scale - 0.5;
                        sum += at(u, v);
                    }
                }
                alpha[dy * s + dx] = sum / (k * k) as f32;
            }
        }
        let rect = self.upload(atlas, side, side, &alpha);
        if rect.is_some() {
            self.entries.insert(
                key,
                Cached {
                    rect,
                    left: 0.0,
                    top: 0.0,
                },
            );
        }
        rect
    }
}

/// The launch screen: the mark, "Native Client" and the sweeping bar, centred on the (already black) frame.
fn native_launch_screen(atlas: &HudAtlas, batch: &mut HudBatch) {
    let mut ui = Ui::new(atlas, batch, (0.0, 0.0));
    let (mark, gap, name_line, bar_w, bar_h) = (72.0_f32, 22.0_f32, 20.0_f32, 132.0_f32, 3.0_f32);
    let top = ((ui.h - (mark + gap + name_line + gap + bar_h)) / 2.0).max(0.0);
    let white = color::hex(0xFFFFFF, 255);

    // The mark.
    let side = (mark * ui.density).round().max(1.0);
    let s = ui.scale;
    if let Some(rect) = with_cache(atlas, |c| c.native_mark(atlas, side as u32)) {
        let (px, py) = (ui.px_x((ui.w - mark) / 2.0), ui.px_y(top));
        ui.batch.rect_uv(px / s, py / s, side / s, side / s, rect, white);
    }

    // The name.
    let label = "Native Client";
    let width = ui.measure(Face::SemiBold, 15.0, label);
    let x = (ui.w - width) / 2.0;
    ui.text_line(Face::SemiBold, 15.0, name_line, x, top + mark + gap, white, label);

    // The bar: a faint track and the light stretch sweeping through it, clipped at the track's ends.
    let (bx, by) = ((ui.w - bar_w) / 2.0, top + mark + gap + name_line + gap);
    ui.rounded(bx, by, bar_w, bar_h, bar_h / 2.0, color::hex(0xFFFFFF, 26));
    let len = bar_w * 0.4;
    let pos = -len + native_ease(native_sweep_phase()) * (bar_w + len);
    let (x0, x1) = (pos.max(0.0), (pos + len).min(bar_w));
    if x1 > x0 {
        let radius = (bar_h / 2.0).min((x1 - x0) / 2.0);
        ui.rounded(bx + x0, by, x1 - x0, bar_h, radius, color::hex(0xFFFFFF, 191));
    }
    ui.finish();
}
