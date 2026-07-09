use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};

use crate::terminal::{selection::SelectionRange, surface::TerminalSurface};

pub struct TerminalWidget<'a> {
    surface: &'a TerminalSurface,
    selection: Option<SelectionRange>,
}

impl<'a> TerminalWidget<'a> {
    pub fn new(surface: &'a TerminalSurface) -> Self {
        Self {
            surface,
            selection: None,
        }
    }

    pub fn with_selection(mut self, selection: Option<SelectionRange>) -> Self {
        self.selection = selection;
        self
    }
}

impl Widget for TerminalWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let snapshot = self.surface.snapshot();
        let max_rows = area.height as usize;
        let max_cols = area.width as usize;

        let selection_style = Style::default().bg(Color::Rgb(50, 50, 80));

        for (row, row_data) in snapshot.iter().enumerate().take(max_rows) {
            for (col, cell) in row_data.iter().enumerate().take(max_cols) {
                let x = area.x + col as u16;
                let y = area.y + row as u16;

                let mut style = cell.style();
                if let Some(ref sel) = self.selection {
                    if sel.contains(row, col) {
                        style = style.patch(selection_style);
                    }
                }

                buf[(x, y)].set_symbol(&cell.symbol).set_style(style);
            }
        }
    }
}
