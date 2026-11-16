use ratatui::layout::Rect;

pub const MIN_ZOOM: f32 = 1.0;
pub const MAX_ZOOM: f32 = 8.0;

#[derive(Clone, Debug)]
pub struct LightboxView {
    pub hash: String,
    pub zoom: f32,
    pub cx: f32,
    pub cy: f32,
}

impl LightboxView {
    pub fn new(hash: String) -> Self {
        Self {
            hash,
            zoom: 1.0,
            cx: 0.5,
            cy: 0.5,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    pub dest: Rect,
    pub src_x: u32,
    pub src_y: u32,
    pub src_w: u32,
    pub src_h: u32,
}

pub fn place(png_w: u32, png_h: u32, inner: Rect, zoom: f32, cx: f32, cy: f32) -> Placement {
    if png_w == 0 || png_h == 0 || inner.width == 0 || inner.height == 0 {
        return Placement {
            dest: Rect::new(inner.x, inner.y, 0, 0),
            src_x: 0,
            src_y: 0,
            src_w: png_w,
            src_h: png_h,
        };
    }
    let zoom = if zoom.is_finite() {
        zoom.clamp(MIN_ZOOM, MAX_ZOOM)
    } else {
        1.0
    };
    let fit = crate::ui::conversation::diagram::layout::compute_layout(
        png_w,
        png_h,
        inner.width,
        inner.height as usize,
    );
    let vw = fit.col_width as f32 * zoom;
    let vh = fit.row_height as f32 * zoom;
    let dw = (vw.round() as u16).clamp(1, inner.width);
    let dh = (vh.round() as u16).clamp(1, inner.height);
    let dest = Rect::new(
        inner.x + inner.width.saturating_sub(dw) / 2,
        inner.y + inner.height.saturating_sub(dh) / 2,
        dw,
        dh,
    );
    let sw = ((dw as f32 * png_w as f32 / vw).round() as u32).clamp(1, png_w);
    let sh = ((dh as f32 * png_h as f32 / vh).round() as u32).clamp(1, png_h);
    let cx = if zoom <= 1.0 {
        0.5
    } else if cx.is_finite() {
        cx.clamp(0.0, 1.0)
    } else {
        0.5
    };
    let cy = if zoom <= 1.0 {
        0.5
    } else if cy.is_finite() {
        cy.clamp(0.0, 1.0)
    } else {
        0.5
    };
    let sx =
        ((cx * png_w as f32 - sw as f32 / 2.0).round() as i64).clamp(0, (png_w - sw) as i64) as u32;
    let sy =
        ((cy * png_h as f32 - sh as f32 / 2.0).round() as i64).clamp(0, (png_h - sh) as i64) as u32;
    Placement {
        dest,
        src_x: sx,
        src_y: sy,
        src_w: sw,
        src_h: sh,
    }
}

pub fn zoom_at(
    view: &mut LightboxView,
    png_w: u32,
    png_h: u32,
    inner: Rect,
    factor: f32,
    pointer: (u16, u16),
) {
    if !factor.is_finite() || factor <= 0.0 {
        return;
    }
    let before = place(png_w, png_h, inner, view.zoom, view.cx, view.cy);
    let px = pointer
        .0
        .saturating_sub(before.dest.x)
        .min(before.dest.width.saturating_sub(1)) as f32
        / before.dest.width.max(1) as f32;
    let py = pointer
        .1
        .saturating_sub(before.dest.y)
        .min(before.dest.height.saturating_sub(1)) as f32
        / before.dest.height.max(1) as f32;
    let image_x = (before.src_x as f32 + px * before.src_w as f32) / png_w.max(1) as f32;
    let image_y = (before.src_y as f32 + py * before.src_h as f32) / png_h.max(1) as f32;
    view.zoom = (view.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
    let after = place(png_w, png_h, inner, view.zoom, view.cx, view.cy);
    view.cx = image_x - (px - 0.5) * after.src_w as f32 / png_w.max(1) as f32;
    view.cy = image_y - (py - 0.5) * after.src_h as f32 / png_h.max(1) as f32;
    let after = place(png_w, png_h, inner, view.zoom, view.cx, view.cy);
    view.cx = (after.src_x as f32 + after.src_w as f32 / 2.0) / png_w as f32;
    view.cy = (after.src_y as f32 + after.src_h as f32 / 2.0) / png_h as f32;
}

pub fn pan_cells(view: &mut LightboxView, png_w: u32, png_h: u32, inner: Rect, dx: i16, dy: i16) {
    let p = place(png_w, png_h, inner, view.zoom, view.cx, view.cy);
    view.cx -= dx as f32 * p.src_w as f32 / p.dest.width.max(1) as f32 / png_w.max(1) as f32;
    view.cy -= dy as f32 * p.src_h as f32 / p.dest.height.max(1) as f32 / png_h.max(1) as f32;
    let p = place(png_w, png_h, inner, view.zoom, view.cx, view.cy);
    view.cx = (p.src_x as f32 + p.src_w as f32 / 2.0) / png_w.max(1) as f32;
    view.cy = (p.src_y as f32 + p.src_h as f32 / 2.0) / png_h.max(1) as f32;
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    #[test]
    fn zoom_one_uses_full_image() {
        let inner = Rect::new(0, 0, 80, 40);
        let p = place(100, 200, inner, 1.0, 0.1, 0.9);
        assert_eq!((p.src_x, p.src_y, p.src_w, p.src_h), (0, 0, 100, 200));
        assert!(p.dest.width < inner.width);
        assert!(p.dest.x > inner.x);
    }

    #[test]
    fn zoom_two_centered_shows_middle_half() {
        let inner = Rect::new(0, 0, 80, 40);
        let fit = crate::ui::conversation::diagram::layout::compute_layout(200, 100, 80, 40);
        let p = place(200, 100, inner, 2.0, 0.5, 0.5);
        assert_eq!(p.src_w, 100);
        assert!((p.src_x as i32 - 50).abs() <= 1);
        assert_eq!(p.dest.width as usize, fit.col_width);
    }

    #[test]
    fn placement_clamps_crop_to_edges() {
        let inner = Rect::new(0, 0, 80, 40);
        let left = place(200, 100, inner, 4.0, 0.0, 0.0);
        assert_eq!((left.src_x, left.src_y), (0, 0));
        let right = place(200, 100, inner, 4.0, 1.0, 1.0);
        assert_eq!(right.src_x + right.src_w, 200);
        assert_eq!(right.src_y + right.src_h, 100);
    }

    #[test]
    fn zoom_at_pointer_and_pan_are_clamped() {
        let inner = Rect::new(0, 0, 80, 40);
        let mut view = LightboxView::new("d".into());
        zoom_at(&mut view, 200, 100, inner, 1.25, (20, 10));
        assert!(view.zoom > 1.0);
        pan_cells(&mut view, 200, 100, inner, 1000, 1000);
        let p = place(200, 100, inner, view.zoom, view.cx, view.cy);
        assert!(p.src_x + p.src_w <= 200);
        assert!(p.src_y + p.src_h <= 100);
    }
}
