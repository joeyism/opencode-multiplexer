use crate::terminal::surface::RenderCell;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionPoint {
    pub row: usize,
    pub col: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionRange {
    pub start: SelectionPoint,
    pub end: SelectionPoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseResult {
    Ignored,
    Claimed,
    Finished,
}

impl SelectionRange {
    pub fn new(a: SelectionPoint, b: SelectionPoint) -> Self {
        if a.row < b.row || (a.row == b.row && a.col <= b.col) {
            Self { start: a, end: b }
        } else {
            Self { start: b, end: a }
        }
    }

    pub fn contains(&self, row: usize, col: usize) -> bool {
        if row < self.start.row || row > self.end.row {
            return false;
        }
        if self.start.row == self.end.row {
            return row == self.start.row && col >= self.start.col && col <= self.end.col;
        }
        if row == self.start.row {
            return col >= self.start.col;
        }
        if row == self.end.row {
            return col <= self.end.col;
        }
        true
    }
}

#[derive(Debug, Default)]
pub struct TerminalSelection {
    anchor: Option<SelectionPoint>,
    head: Option<SelectionPoint>,
    dragging: bool,
}

impl TerminalSelection {
    pub fn clear(&mut self) {
        self.anchor = None;
        self.head = None;
        self.dragging = false;
    }

    pub fn is_active(&self) -> bool {
        self.anchor.is_some()
    }

    pub fn is_dragging(&self) -> bool {
        self.dragging
    }

    pub fn begin(&mut self, point: SelectionPoint) {
        self.anchor = Some(point);
        self.head = Some(point);
        self.dragging = true;
    }

    pub fn update(&mut self, point: SelectionPoint) {
        if self.dragging {
            self.head = Some(point);
        }
    }

    pub fn finish(&mut self) -> Option<SelectionRange> {
        if self.dragging {
            self.dragging = false;
            self.range()
        } else {
            None
        }
    }

    pub fn range(&self) -> Option<SelectionRange> {
        match (self.anchor, self.head) {
            (Some(a), Some(h)) => Some(SelectionRange::new(a, h)),
            _ => None,
        }
    }

    pub fn handle_mouse(
        &mut self,
        mouse: MouseEvent,
        pane: Rect,
        surface_rows: usize,
        surface_cols: usize,
    ) -> MouseResult {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if pane.contains((mouse.column, mouse.row).into()) {
                    let col = (mouse.column as usize)
                        .saturating_sub(pane.x as usize)
                        .min(surface_cols.saturating_sub(1));
                    let row = (mouse.row as usize)
                        .saturating_sub(pane.y as usize)
                        .min(surface_rows.saturating_sub(1));
                    self.begin(SelectionPoint { row, col });
                    MouseResult::Claimed
                } else {
                    self.clear();
                    MouseResult::Ignored
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if self.dragging {
                    let col = (mouse.column as usize)
                        .saturating_sub(pane.x as usize)
                        .min(surface_cols.saturating_sub(1));
                    let row = (mouse.row as usize)
                        .saturating_sub(pane.y as usize)
                        .min(surface_rows.saturating_sub(1));
                    self.update(SelectionPoint { row, col });
                    MouseResult::Claimed
                } else {
                    MouseResult::Ignored
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if self.dragging {
                    let col = (mouse.column as usize)
                        .saturating_sub(pane.x as usize)
                        .min(surface_cols.saturating_sub(1));
                    let row = (mouse.row as usize)
                        .saturating_sub(pane.y as usize)
                        .min(surface_rows.saturating_sub(1));
                    self.update(SelectionPoint { row, col });
                    self.finish();
                    MouseResult::Finished
                } else {
                    MouseResult::Ignored
                }
            }
            _ => MouseResult::Ignored,
        }
    }

    pub fn extract_text_from(
        &self,
        snapshot: &[Vec<RenderCell>],
        wrapped: &[bool],
    ) -> Option<String> {
        self.range()
            .map(|r| extract_selection_text(snapshot, r, wrapped))
    }
}

pub fn extract_selection_text(
    snapshot: &[Vec<RenderCell>],
    range: SelectionRange,
    wrapped: &[bool],
) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut current_line = String::new();

    for r in range.start.row..=range.end.row {
        if r >= snapshot.len() {
            break;
        }
        let row_data = &snapshot[r];
        let start_col = if r == range.start.row {
            range.start.col
        } else {
            0
        };
        let end_col = if r == range.end.row {
            range.end.col
        } else {
            row_data.len().saturating_sub(1)
        };

        for c in start_col..=end_col {
            if c >= row_data.len() {
                break;
            }
            if row_data[c].copyable {
                current_line.push_str(&row_data[c].symbol);
            }
        }

        let is_wrapped = wrapped.get(r).copied().unwrap_or(false);
        if is_wrapped && r != range.end.row {
            // Row wraps — continue accumulating into current_line
        } else {
            // End of logical line or end of selection — trim and push
            lines.push(current_line.trim_end().to_string());
            current_line = String::new();
        }
    }

    // Flush any remaining content (should only happen if the loop logic missed something)
    if !current_line.is_empty() {
        lines.push(current_line.trim_end().to_string());
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    #[test]
    fn normalize_forward_same_row() {
        let r = SelectionRange::new(
            SelectionPoint { row: 0, col: 2 },
            SelectionPoint { row: 0, col: 5 },
        );
        assert_eq!(r.start.col, 2);
        assert_eq!(r.end.col, 5);
    }

    #[test]
    fn normalize_reverse_same_row() {
        let r = SelectionRange::new(
            SelectionPoint { row: 0, col: 5 },
            SelectionPoint { row: 0, col: 2 },
        );
        assert_eq!(r.start.col, 2);
        assert_eq!(r.end.col, 5);
    }

    #[test]
    fn normalize_forward_multi_row() {
        let r = SelectionRange::new(
            SelectionPoint { row: 1, col: 3 },
            SelectionPoint { row: 3, col: 7 },
        );
        assert_eq!(r.start.row, 1);
        assert_eq!(r.end.row, 3);
    }

    #[test]
    fn normalize_reverse_multi_row() {
        let r = SelectionRange::new(
            SelectionPoint { row: 3, col: 7 },
            SelectionPoint { row: 1, col: 3 },
        );
        assert_eq!(r.start.row, 1);
        assert_eq!(r.end.row, 3);
    }

    #[test]
    fn contains_single_row() {
        let r = SelectionRange::new(
            SelectionPoint { row: 0, col: 2 },
            SelectionPoint { row: 0, col: 5 },
        );
        assert!(r.contains(0, 2));
        assert!(r.contains(0, 4));
        assert!(r.contains(0, 5));
        assert!(!r.contains(0, 1));
        assert!(!r.contains(0, 6));
        assert!(!r.contains(1, 3));
    }

    #[test]
    fn contains_multi_row() {
        let r = SelectionRange::new(
            SelectionPoint { row: 1, col: 5 },
            SelectionPoint { row: 3, col: 2 },
        );
        // Row 1
        assert!(!r.contains(1, 4));
        assert!(r.contains(1, 5));
        assert!(r.contains(1, 10));
        // Row 2
        assert!(r.contains(2, 0));
        assert!(r.contains(2, 10));
        // Row 3
        assert!(r.contains(3, 0));
        assert!(r.contains(3, 2));
        assert!(!r.contains(3, 3));
        // Outside
        assert!(!r.contains(0, 5));
        assert!(!r.contains(4, 0));
    }

    fn mock_snapshot(data: Vec<Vec<(&str, bool)>>) -> Vec<Vec<RenderCell>> {
        data.into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|(sym, cp)| RenderCell {
                        symbol: sym.to_string(),
                        copyable: cp,
                        ..Default::default()
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn extraction_basics() {
        let snap = mock_snapshot(vec![
            vec![
                ("h", true),
                ("e", true),
                ("l", true),
                ("l", true),
                ("o", true),
                (" ", true),
                (" ", true),
            ],
            vec![
                ("w", true),
                ("o", true),
                ("r", true),
                ("l", true),
                ("d", true),
                (" ", true),
                (" ", true),
            ],
        ]);

        // Single row partial
        let r1 = SelectionRange::new(
            SelectionPoint { row: 0, col: 0 },
            SelectionPoint { row: 0, col: 4 },
        );
        assert_eq!(
            extract_selection_text(&snap, r1, &[false, false]),
            "hello"
        );

        // Multi row
        let r2 = SelectionRange::new(
            SelectionPoint { row: 0, col: 3 },
            SelectionPoint { row: 1, col: 2 },
        );
        assert_eq!(
            extract_selection_text(&snap, r2, &[false, false]),
            "lo\nwor"
        );
    }

    #[test]
    fn extraction_skips_non_copyable_and_trims() {
        let snap = mock_snapshot(vec![vec![
            ("A", true),
            (" ", false),
            ("B", true),
            (" ", true),
            (" ", true),
        ]]);
        let r = SelectionRange::new(
            SelectionPoint { row: 0, col: 0 },
            SelectionPoint { row: 0, col: 4 },
        );
        assert_eq!(extract_selection_text(&snap, r, &[false]), "AB");
    }

    #[test]
    fn extraction_joins_wrapped_rows() {
        let snap = mock_snapshot(vec![
            vec![
                ("p", true),
                ("a", true),
                ("r", true),
                ("t", true),
                (" ", true),
            ],
            vec![
                ("o", true),
                ("n", true),
                ("e", true),
                (" ", true),
                (" ", true),
            ],
        ]);
        let r = SelectionRange::new(
            SelectionPoint { row: 0, col: 0 },
            SelectionPoint { row: 1, col: 2 },
        );
        // Wrapped: part + one -> "partone"
        assert_eq!(
            extract_selection_text(&snap, r, &[true, false]),
            "part one"
        );

        // Wait, "part one" or "partone"?
        // In my mock snapshot, row 0 col 4 is " ". If it's a real space it should stay if row wraps.
    }

    #[test]
    fn extraction_wrapped_does_not_trim_intermediate() {
        let snap = mock_snapshot(vec![
            vec![
                ("h", true),
                ("e", true),
                ("l", true),
                ("l", true),
                (" ", true),
            ], // wraps here
            vec![
                ("o", true),
                (" ", true),
                (" ", true),
                (" ", true),
                (" ", true),
            ],
        ]);
        let r = SelectionRange::new(
            SelectionPoint { row: 0, col: 0 },
            SelectionPoint { row: 1, col: 0 },
        );
        // Row 0 is wrapped. We should NOT trim it.
        // Result should be "hell " + "o" -> "hell o"
        assert_eq!(
            extract_selection_text(&snap, r, &[true, false]),
            "hell o"
        );
    }

    #[test]
    fn extraction_mixed_wrapped_and_unwrapped() {
        let snap = mock_snapshot(vec![
            vec![("A", true), (" ", true)], // wraps
            vec![("B", true), (" ", true)], // doesn't wrap
            vec![("C", true), (" ", true)], // doesn't wrap
        ]);
        let r = SelectionRange::new(
            SelectionPoint { row: 0, col: 0 },
            SelectionPoint { row: 2, col: 0 },
        );
        // Row 0-1 wrapped -> "A B"
        // Row 2 is separate line -> "C"
        assert_eq!(
            extract_selection_text(&snap, r, &[true, false, false]),
            "A B\nC"
        );
    }

    #[test]
    fn extraction_wrapped_last_row_always_trimmed() {
        let snap = mock_snapshot(vec![
            vec![("h", true), ("e", true), (" ", true)], // wraps
            vec![("l", true), ("l", true), ("o", true)], // wraps, but end of selection
        ]);
        let r = SelectionRange::new(
            SelectionPoint { row: 0, col: 0 },
            SelectionPoint { row: 1, col: 2 },
        );
        // Result: "he " + "llo" -> "he llo"
        assert_eq!(
            extract_selection_text(&snap, r, &[true, true]),
            "he llo"
        );
    }

    #[test]
    fn mouse_handling_claimed() {
        let mut sel = TerminalSelection::default();
        let pane = Rect::new(10, 10, 20, 20);

        // Down inside
        let res = sel.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 15,
                row: 15,
                modifiers: KeyModifiers::empty(),
            },
            pane,
            20,
            20,
        );
        assert_eq!(res, MouseResult::Claimed);
        assert!(sel.dragging);
        assert_eq!(sel.anchor, Some(SelectionPoint { row: 5, col: 5 }));

        // Drag inside
        let res = sel.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: 17,
                row: 16,
                modifiers: KeyModifiers::empty(),
            },
            pane,
            20,
            20,
        );
        assert_eq!(res, MouseResult::Claimed);
        assert_eq!(sel.head, Some(SelectionPoint { row: 6, col: 7 }));

        // Up inside
        let res = sel.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: 17,
                row: 16,
                modifiers: KeyModifiers::empty(),
            },
            pane,
            20,
            20,
        );
        assert_eq!(res, MouseResult::Finished);
        assert!(!sel.dragging);
        assert!(sel.is_active());
    }
}
