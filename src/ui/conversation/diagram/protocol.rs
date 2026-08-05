//! Kitty graphics protocol helpers + scroll-coupled placement geometry.
//!
//! When a diagram is only partially on-screen we crop the *source* image
//! (Kitty x/y/w/h) and place into the visible cell rect at the *same scale*
//! as the full diagram. That keeps aspect ratio constant — the off-screen
//! part is clipped/covered, never squashed into the remaining rows.

use std::collections::HashMap;
use std::io::{self, Write};

use base64::{Engine, engine::general_purpose::STANDARD as B64};
use ratatui::layout::Rect;

/// How a diagram slot intersects the viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotVisibility {
    /// Destination cells on screen for the visible portion.
    pub screen_rect: Rect,
    /// Rows of the full slot that sit above the viewport (source crop top).
    pub clip_top_rows: usize,
    /// Rows currently visible.
    pub visible_rows: usize,
    /// Full slot height in rows (scale reference).
    pub full_rows: usize,
}

pub fn slot_visibility(
    slot_start: usize,
    slot_height: usize,
    scroll: usize,
    area: Rect,
    col_width: usize,
) -> Option<SlotVisibility> {
    if slot_height == 0 {
        return None;
    }
    let window_start = scroll;
    let window_end = scroll + area.height as usize;
    let slot_end = slot_start + slot_height;

    if slot_end <= window_start || slot_start >= window_end {
        return None;
    }

    let visible_start = slot_start.max(window_start);
    let visible_end = slot_end.min(window_end);
    let visible_rows = visible_end.saturating_sub(visible_start);
    if visible_rows == 0 {
        return None;
    }

    let clip_top_rows = visible_start.saturating_sub(slot_start);
    let y_offset = visible_start.saturating_sub(window_start);
    let width = (col_width as u16).min(area.width).max(1);

    Some(SlotVisibility {
        screen_rect: Rect::new(area.x, area.y + y_offset as u16, width, visible_rows as u16),
        clip_top_rows,
        visible_rows,
        full_rows: slot_height,
    })
}

/// Back-compat wrapper used by older tests.
pub fn screen_rect_for_slot(
    slot_start: usize,
    slot_height: usize,
    scroll: usize,
    area: Rect,
    col_width: usize,
) -> Option<Rect> {
    slot_visibility(slot_start, slot_height, scroll, area, col_width).map(|v| v.screen_rect)
}

/// Source pixel crop for a partially visible placement.
/// `png_w`/`png_h` are the bitmap dimensions; the full image maps to
/// `full_rows` terminal rows (and `col_width` cols, unused for y-crop).
pub fn source_crop(
    png_w: u32,
    png_h: u32,
    full_rows: usize,
    clip_top_rows: usize,
    visible_rows: usize,
) -> (u32, u32, u32, u32) {
    // x, y, w, h in source pixels
    if full_rows == 0 || png_h == 0 || png_w == 0 {
        return (0, 0, png_w.max(1), png_h.max(1));
    }
    let y = ((clip_top_rows as u64 * png_h as u64) / full_rows as u64) as u32;
    let mut h = ((visible_rows as u64 * png_h as u64) / full_rows as u64) as u32;
    if h == 0 {
        h = 1;
    }
    // Clamp to image bounds
    let y = y.min(png_h.saturating_sub(1));
    let h = h.min(png_h.saturating_sub(y)).max(1);
    (0, y, png_w, h)
}

pub fn kitty_graphics_supported() -> bool {
    if std::env::var_os("OCMUX_NO_KITTY_GRAPHICS").is_some() {
        return false;
    }
    if std::env::var_os("OCMUX_PROTOCOL").is_some() {
        return true;
    }
    if std::env::var_os("KITTY_WINDOW_ID").is_some() {
        return true;
    }
    if let Ok(term) = std::env::var("TERM") {
        if term.contains("kitty") {
            return true;
        }
    }
    if let Ok(prog) = std::env::var("TERM_PROGRAM") {
        if prog.eq_ignore_ascii_case("ghostty") || prog.eq_ignore_ascii_case("WezTerm") {
            return true;
        }
    }
    false
}

fn tmux_active() -> bool {
    std::env::var_os("TMUX").is_some()
}

fn write_apc(out: &mut dyn Write, body: &str) -> io::Result<()> {
    if tmux_active() {
        write!(out, "\x1bPtmux;\x1b\x1b_G{body}\x1b\x1b\\\x1b\\")
    } else {
        write!(out, "\x1b_G{body}\x1b\\")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PlacementKey {
    hash: String,
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    src_y: u32,
    src_h: u32,
}

impl PlacementKey {
    fn from_paint(p: &DiagramPaint<'_>) -> Self {
        Self {
            hash: p.hash.to_string(),
            x: p.rect.x,
            y: p.rect.y,
            w: p.rect.width,
            h: p.rect.height,
            src_y: p.src_y,
            src_h: p.src_h,
        }
    }
}

pub struct KittyImagePlacer {
    next_id: u32,
    uploaded: HashMap<String, u32>,
    last_visible: Vec<String>,
    last_keys: Vec<PlacementKey>,
}

impl Default for KittyImagePlacer {
    fn default() -> Self {
        Self {
            next_id: 1,
            uploaded: HashMap::new(),
            last_visible: Vec::new(),
            last_keys: Vec::new(),
        }
    }
}

pub struct DiagramPaint<'a> {
    pub hash: &'a str,
    pub png: &'a [u8],
    pub rect: Rect,
    /// Source crop in pixels (Kitty x,y,w,h).
    pub src_x: u32,
    pub src_y: u32,
    pub src_w: u32,
    pub src_h: u32,
}

impl KittyImagePlacer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn has_graphics(&self) -> bool {
        !self.uploaded.is_empty() || !self.last_visible.is_empty() || !self.last_keys.is_empty()
    }

    pub fn clear_all(&mut self, out: &mut dyn Write) -> io::Result<()> {
        if !self.has_graphics() {
            return Ok(());
        }
        // d=A: delete all visible placements and free image data on the alt screen.
        write_apc(out, "a=d,d=A,q=2")?;
        // Also try d=a in case some placements linger without data free.
        write_apc(out, "a=d,d=a,q=2")?;
        self.uploaded.clear();
        self.last_visible.clear();
        self.last_keys.clear();
        out.flush()
    }

    pub fn paint_frame(
        &mut self,
        out: &mut dyn Write,
        paints: &[DiagramPaint<'_>],
    ) -> io::Result<()> {
        let keys: Vec<PlacementKey> = paints.iter().map(PlacementKey::from_paint).collect();
        let visible_hashes: Vec<String> = paints.iter().map(|p| p.hash.to_string()).collect();

        for old in &self.last_visible {
            if !visible_hashes.iter().any(|h| h == old) {
                if let Some(&id) = self.uploaded.get(old) {
                    write_apc(out, &format!("a=d,d=i,i={id},q=2"))?;
                    self.uploaded.remove(old);
                }
            }
        }

        for paint in paints {
            if paint.rect.width == 0 || paint.rect.height == 0 {
                continue;
            }
            let id = self.ensure_uploaded(out, paint.hash, paint.png)?;

            write!(out, "\x1b[{};{}H", paint.rect.y + 1, paint.rect.x + 1)?;
            // Crop source (x,y,w,h) → dest cells (c,r). Same scale as full
            // image in full_rows; off-screen rows are clipped, not rescaled.
            write_apc(
                out,
                &format!(
                    "a=p,i={id},p=1,x={},y={},w={},h={},c={},r={},q=2",
                    paint.src_x,
                    paint.src_y,
                    paint.src_w,
                    paint.src_h,
                    paint.rect.width,
                    paint.rect.height
                ),
            )?;
        }

        self.last_keys = keys;
        self.last_visible = visible_hashes;
        out.flush()
    }

    fn ensure_uploaded(&mut self, out: &mut dyn Write, hash: &str, png: &[u8]) -> io::Result<u32> {
        if let Some(&id) = self.uploaded.get(hash) {
            return Ok(id);
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);

        let b64 = B64.encode(png);
        let bytes = b64.as_bytes();
        const CHUNK: usize = 4096;
        let mut offset = 0;
        while offset < bytes.len() {
            let end = (offset + CHUNK).min(bytes.len());
            let more = if end < bytes.len() { 1 } else { 0 };
            let chunk = std::str::from_utf8(&bytes[offset..end]).unwrap();
            if offset == 0 {
                write_apc(out, &format!("a=t,f=100,t=d,i={id},m={more},q=2;{chunk}"))?;
            } else {
                write_apc(out, &format!("m={more},q=2;{chunk}"))?;
            }
            offset = end;
        }

        self.uploaded.insert(hash.to_string(), id);
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_png() -> Vec<u8> {
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tiny.png"),
        )
        .unwrap()
    }

    #[test]
    fn source_crop_top_half() {
        // 100px tall image, 10 rows full → 10px per row.
        // Clip top 3 rows, show 4 → y=30, h=40
        let (x, y, w, h) = source_crop(200, 100, 10, 3, 4);
        assert_eq!(x, 0);
        assert_eq!(w, 200);
        assert_eq!(y, 30);
        assert_eq!(h, 40);
    }

    #[test]
    fn source_crop_full_visible() {
        let (x, y, w, h) = source_crop(200, 100, 10, 0, 10);
        assert_eq!((x, y, w, h), (0, 0, 200, 100));
    }

    #[test]
    fn slot_visibility_reports_clip_top() {
        let area = Rect::new(0, 0, 80, 10);
        // slot at rows 5..15, scroll=8 → visible rows 8..15 = 7 rows, clip_top=3
        let v = slot_visibility(5, 10, 8, area, 40).unwrap();
        assert_eq!(v.clip_top_rows, 3);
        assert_eq!(v.visible_rows, 7);
        assert_eq!(v.full_rows, 10);
        assert_eq!(v.screen_rect.height, 7);
        assert_eq!(v.screen_rect.width, 40);
    }

    #[test]
    fn screen_rect_respects_col_width() {
        let area = Rect::new(5, 2, 80, 20);
        let r = screen_rect_for_slot(0, 10, 0, area, 40).unwrap();
        assert_eq!(r.width, 40);
        assert_eq!(r.x, 5);
    }

    #[test]
    fn paint_emits_source_crop_params() {
        let mut placer = KittyImagePlacer::new();
        let png = tiny_png();
        let mut buf = Vec::new();
        placer
            .paint_frame(
                &mut buf,
                &[DiagramPaint {
                    hash: "h",
                    png: &png,
                    rect: Rect::new(2, 3, 40, 5),
                    src_x: 0,
                    src_y: 20,
                    src_w: 100,
                    src_h: 50,
                }],
            )
            .unwrap();
        let s = String::from_utf8_lossy(&buf);
        assert!(s.contains("x=0,y=20,w=100,h=50"), "crop missing: {s}");
        assert!(s.contains("c=40,r=5"), "dest missing: {s}");
    }
}
