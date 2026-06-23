use ratatui::layout::{Constraint, Layout, Rect};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RootLayout {
    pub sidebar: Rect,
    pub main: Rect,
    pub footer: Rect,
}

pub fn split_root(area: Rect, sidebar_width: u16, footer_height: u16) -> RootLayout {
    let [body, footer] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(footer_height)]).areas(area);

    let [sidebar, main] = Layout::horizontal([
        Constraint::Length(sidebar_width.min(body.width)),
        Constraint::Min(0),
    ])
    .areas(body);

    RootLayout {
        sidebar,
        main,
        footer,
    }
}

pub fn terminal_inner_rect(area: Rect, sidebar_width: u16, footer_height: u16) -> Rect {
    let layout = split_root(area, sidebar_width, footer_height);
    Rect::new(
        layout.main.x,
        layout.main.y + 1,
        layout.main.width,
        layout.main.height.saturating_sub(1),
    )
}

pub fn centered_rect(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let width = area.width * percent_x / 100;
    let height = area.height * percent_y / 100;
    Rect::new(
        area.x + (area.width.saturating_sub(width)) / 2,
        area.y + (area.height.saturating_sub(height)) / 2,
        width.max(1),
        height.max(1),
    )
}
