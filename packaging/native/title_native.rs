// Native Client's title screen mark and account line, drawn over the classic
// title logo. `title.rs` includes this file from `draw_logo`.

/// The wordmark above the logo and the account line under it.
pub fn native_title_branding(atlas: &HudAtlas, batch: &mut HudBatch, w: f32) {
    let mark = "NATIVE CLIENT";
    let tw = HudBatch::text_width(atlas, mark);
    batch.text(
        atlas,
        mark,
        (w / 2.0 - tw / 2.0).floor(),
        14.0,
        0xFFFF_D24A_u32,
        true,
    );

    let line = match crate::discord_presence::native_account() {
        Some(name) if !name.is_empty() => format!("Signed in as {name}"),
        _ => "Not signed in to Native".to_string(),
    };
    let lw = HudBatch::text_width(atlas, &line);
    batch.text(
        atlas,
        &line,
        (w / 2.0 - lw / 2.0).floor(),
        88.0,
        0xFF9F_B3C8_u32,
        true,
    );
}
