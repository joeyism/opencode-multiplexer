use crate::ui::conversation::diagram::layout::DiagramLayout;
use image::{Rgba, RgbaImage};
use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

/// Dark canvas matching typical terminal backgrounds. Transparent mermaid
/// pixels are composited onto this before resize so light fills and thin
/// strokes survive downscaling (alpha-averaged edges used to become
/// Color::Reset and vanish).
const CANVAS_BG: Rgba<u8> = Rgba([26, 27, 38, 255]); // #1a1b26

pub fn png_to_halfblock_lines(
    png_bytes: &[u8],
    layout: &DiagramLayout,
) -> Result<Vec<Line<'static>>, image::ImageError> {
    let img = image::load_from_memory(png_bytes)?.to_rgba8();
    let composited = composite_on_canvas(&img, CANVAS_BG);
    let resized = image::imageops::resize(
        &composited,
        layout.out_width_px,
        layout.out_height_px,
        image::imageops::FilterType::CatmullRom,
    );

    let (width, height) = resized.dimensions();
    let mut lines = Vec::with_capacity((height as usize).div_ceil(2));

    for y in (0..height).step_by(2) {
        let mut spans = Vec::with_capacity(width as usize);
        for x in 0..width {
            let top = *resized.get_pixel(x, y);
            let bottom = if y + 1 < height {
                *resized.get_pixel(x, y + 1)
            } else {
                CANVAS_BG
            };

            spans.push(Span::styled(
                "▀",
                Style::default()
                    .fg(rgba_to_ratatui(top))
                    .bg(rgba_to_ratatui(bottom)),
            ));
        }
        lines.push(Line::from(spans));
    }

    Ok(lines)
}

fn composite_on_canvas(src: &RgbaImage, bg: Rgba<u8>) -> RgbaImage {
    let (w, h) = src.dimensions();
    let mut out = RgbaImage::from_pixel(w, h, bg);
    for (x, y, p) in src.enumerate_pixels() {
        let a = p.0[3] as u16;
        if a == 0 {
            continue;
        }
        if a == 255 {
            out.put_pixel(x, y, *p);
            continue;
        }
        // src over bg
        let inv = 255 - a;
        let blended = Rgba([
            ((p.0[0] as u16 * a + bg.0[0] as u16 * inv) / 255) as u8,
            ((p.0[1] as u16 * a + bg.0[1] as u16 * inv) / 255) as u8,
            ((p.0[2] as u16 * a + bg.0[2] as u16 * inv) / 255) as u8,
            255,
        ]);
        out.put_pixel(x, y, blended);
    }
    out
}

fn rgba_to_ratatui(rgba: Rgba<u8>) -> Color {
    Color::Rgb(rgba.0[0], rgba.0[1], rgba.0[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composite_makes_transparent_pixel_canvas_color() {
        let mut img = RgbaImage::new(1, 1);
        img.put_pixel(0, 0, Rgba([0, 0, 0, 0]));
        let out = composite_on_canvas(&img, CANVAS_BG);
        assert_eq!(*out.get_pixel(0, 0), CANVAS_BG);
    }
}
