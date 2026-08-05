use image::GenericImageView;

/// Approximate terminal cell aspect: height/width in pixels.
/// Most monospace cells are ~1:2 (e.g. 8×16), so one row is "twice as tall"
/// as one column is wide. Used so diagrams keep their PNG aspect ratio when
/// mapped into (cols × rows) of cells.
pub const CELL_ASPECT_H_OVER_W: f32 = 2.0;

/// Halfblock geometry: each terminal row shows 2 image pixels of height.
pub const PX_PER_ROW: u32 = 2;

#[derive(Debug, Clone, Copy)]
pub struct DiagramLayout {
    /// Pixel size used for halfblock rasterization (square-ish pixels).
    pub out_width_px: u32,
    pub out_height_px: u32,
    /// Terminal rows the diagram occupies.
    pub row_height: usize,
    /// Terminal columns the diagram occupies (≤ available cols).
    /// Kitty placement and halfblock line width must use this — NOT the full
    /// pane width — or the image is stretched horizontally.
    pub col_width: usize,
}

pub fn compute_layout_from_png_bytes(
    png_bytes: &[u8],
    cols: u16,
    max_rows: usize,
) -> Result<DiagramLayout, image::ImageError> {
    let img = image::load_from_memory(png_bytes)?;
    let (w, h) = img.dimensions();
    Ok(compute_layout(w, h, cols, max_rows))
}

/// Fit `src_w×src_h` into at most `cols×max_rows` cells, preserving aspect.
///
/// Cell grid aspect: a `c×r` block of cells has pixel aspect roughly
/// `(c) : (r * CELL_ASPECT_H_OVER_W)`. We choose c,r so that matches src_w:src_h.
pub fn compute_layout(src_w: u32, src_h: u32, cols: u16, max_rows: usize) -> DiagramLayout {
    if cols == 0 || src_w == 0 || src_h == 0 || max_rows == 0 {
        return DiagramLayout {
            out_width_px: 1,
            out_height_px: 1,
            row_height: 1,
            col_width: 1,
        };
    }

    let max_cols = cols as f32;
    let max_rows_f = max_rows as f32;
    let img_aspect = src_w as f32 / src_h as f32; // width/height

    // For a c×r cell block: pixel_aspect ≈ c / (r * CELL_ASPECT) = img_aspect
    // => c / r = img_aspect * CELL_ASPECT
    // => r = c / (img_aspect * CELL_ASPECT)
    // => c = r * img_aspect * CELL_ASPECT

    // Fit width-first: use all columns, compute rows from aspect.
    let mut col_width = max_cols;
    let mut row_height = col_width / (img_aspect * CELL_ASPECT_H_OVER_W);

    // If too tall, fit height-first instead.
    if row_height > max_rows_f {
        row_height = max_rows_f;
        col_width = row_height * img_aspect * CELL_ASPECT_H_OVER_W;
        if col_width > max_cols {
            col_width = max_cols;
            row_height = col_width / (img_aspect * CELL_ASPECT_H_OVER_W);
        }
    }

    let col_width = (col_width.round() as usize).clamp(1, cols as usize);
    let row_height = (row_height.round() as usize).clamp(1, max_rows);

    // Halfblock raster uses square logical pixels: 1 per col, 2 per row.
    let out_width_px = col_width as u32;
    let out_height_px = (row_height as u32).saturating_mul(PX_PER_ROW).max(1);

    DiagramLayout {
        out_width_px,
        out_height_px,
        row_height,
        col_width,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_image_gets_cols_approx_2x_rows() {
        // Square PNG in cells with 2:1 cell aspect → cols ≈ 2 * rows
        let layout = compute_layout(100, 100, 80, 40);
        assert!(layout.col_width <= 80);
        assert!(layout.row_height <= 40);
        // col/row ≈ 2
        let ratio = layout.col_width as f32 / layout.row_height as f32;
        assert!(
            (ratio - 2.0).abs() < 0.35,
            "expected col/row ≈ 2 for square image, got {ratio} ({}x{})",
            layout.col_width,
            layout.row_height
        );
    }

    #[test]
    fn tall_image_is_height_capped_not_width_stretched() {
        // Very tall diagram: should hit max_rows and shrink col_width
        let layout = compute_layout(100, 800, 80, 20);
        assert_eq!(layout.row_height, 20);
        assert!(
            layout.col_width < 80,
            "tall image must not use full width (got col_width={})",
            layout.col_width
        );
    }

    #[test]
    fn wide_image_uses_full_width_when_height_allows() {
        let layout = compute_layout(800, 100, 80, 40);
        assert_eq!(layout.col_width, 80);
        // rows = 80 / ((800/100)*2) = 80 / 16 = 5
        assert!(layout.row_height <= 10);
    }

    #[test]
    fn col_width_never_exceeds_available_cols() {
        let layout = compute_layout(50, 50, 10, 100);
        assert!(layout.col_width <= 10);
    }
}
