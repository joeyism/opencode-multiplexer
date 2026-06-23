use alacritty_terminal::{
    event::VoidListener,
    term::{Config, Term, cell::Flags, test::TermSize},
    vte::ansi::Processor,
};
use ratatui::style::Color;

use crate::terminal::color::{convert_ansi_color, style_from_flags};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderCell {
    pub symbol: String,
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub copyable: bool,
}

impl Default for RenderCell {
    fn default() -> Self {
        Self {
            symbol: " ".into(),
            fg: Color::Reset,
            bg: Color::Reset,
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            copyable: true,
        }
    }
}

impl RenderCell {
    pub fn style(&self) -> ratatui::style::Style {
        style_from_flags(
            self.fg,
            self.bg,
            self.bold,
            self.italic,
            self.underline,
            self.strike,
        )
    }
}

pub struct TerminalSurface {
    term: Term<VoidListener>,
    parser: Processor,
    rows: usize,
    cols: usize,
}

impl TerminalSurface {
    pub fn new(rows: usize, cols: usize) -> Self {
        let rows = rows.max(1);
        let cols = cols.max(1);
        let size = TermSize::new(cols, rows);
        let term = Term::new(Config::default(), &size, VoidListener);
        Self {
            term,
            parser: Processor::new(),
            rows,
            cols,
        }
    }

    pub fn process(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }

    pub fn resize(&mut self, rows: usize, cols: usize) {
        let rows = rows.max(1);
        let cols = cols.max(1);
        self.rows = rows;
        self.cols = cols;
        self.term.resize(TermSize::new(cols, rows));
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn snapshot(&self) -> Vec<Vec<RenderCell>> {
        let mut output = vec![vec![RenderCell::default(); self.cols]; self.rows];

        for indexed in self.term.grid().display_iter() {
            let point = indexed.point;
            if point.line.0 < 0 {
                continue;
            }

            let row = point.line.0 as usize;
            let col = point.column.0;
            if row >= self.rows || col >= self.cols {
                continue;
            }

            let cell = indexed.cell;
            let (symbol, copyable) = cell_symbol(cell.c, cell.flags);
            output[row][col] = RenderCell {
                symbol,
                fg: convert_ansi_color(cell.fg),
                bg: convert_ansi_color(cell.bg),
                bold: cell.flags.contains(Flags::BOLD) || cell.flags.contains(Flags::DIM_BOLD),
                italic: cell.flags.contains(Flags::ITALIC),
                underline: cell.flags.intersects(Flags::ALL_UNDERLINES),
                strike: cell.flags.contains(Flags::STRIKEOUT),
                copyable,
            };
        }

        output
    }

    pub fn cursor(&self) -> (usize, usize) {
        let cursor = self.term.grid().cursor.point;
        let row = (cursor.line.0.max(0) as usize).min(self.rows.saturating_sub(1));
        let col = cursor.column.0.min(self.cols.saturating_sub(1));
        (row, col)
    }

    pub fn wrapped_rows(&self) -> Vec<bool> {
        use alacritty_terminal::index::Line;
        (0..self.rows)
            .map(|r| {
                let row = &self.term.grid()[Line(r as i32)];
                row.last().is_some_and(|cell| cell.flags.contains(Flags::WRAPLINE))
            })
            .collect()
    }
}

fn cell_symbol(ch: char, flags: Flags) -> (String, bool) {
    if flags.contains(Flags::WIDE_CHAR_SPACER) || flags.contains(Flags::LEADING_WIDE_CHAR_SPACER) {
        (" ".into(), false)
    } else {
        (ch.to_string(), true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regular_char_is_copyable() {
        let mut surface = TerminalSurface::new(1, 1);
        surface.process(b"A");
        let snapshot = surface.snapshot();
        assert_eq!(snapshot[0][0].symbol, "A");
        assert!(snapshot[0][0].copyable);
    }

    #[test]
    fn space_is_copyable() {
        let mut surface = TerminalSurface::new(1, 1);
        surface.process(b" ");
        let snapshot = surface.snapshot();
        assert_eq!(snapshot[0][0].symbol, " ");
        assert!(snapshot[0][0].copyable);
    }

    #[test]
    fn default_render_cell_is_copyable() {
        assert!(RenderCell::default().copyable);
    }

    #[test]
    fn surface_clamps_zero_dimensions() {
        let surface = TerminalSurface::new(0, 0);

        assert_eq!(surface.rows(), 1);
        assert_eq!(surface.cols(), 1);
    }

    #[test]
    fn unwrapped_row_is_not_marked() {
        let mut surface = TerminalSurface::new(2, 20);
        surface.process(b"hello");
        assert_eq!(surface.wrapped_rows(), vec![false, false]);
    }

    #[test]
    fn wrapped_row_is_marked() {
        let mut surface = TerminalSurface::new(2, 10);
        // "1234567890123" should wrap: row 0 gets 1-10, row 1 gets 11-13
        surface.process(b"1234567890123");
        assert_eq!(surface.wrapped_rows(), vec![true, false]);
    }

    #[test]
    fn multiple_wraps() {
        let mut surface = TerminalSurface::new(4, 5);
        // "abcdefghijklmnop"
        // row 0: abcde (wrap)
        // row 1: fghij (wrap)
        // row 2: klmno (wrap)
        // row 3: p     (no wrap)
        surface.process(b"abcdefghijklmnop");
        assert_eq!(surface.wrapped_rows(), vec![true, true, true, false]);
    }
}
