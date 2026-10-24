//! Mouse-driven text selection over a conversation document.
//!
//! Selection endpoints are stored both as display positions (for hit-testing
//! and highlighting) and as content anchors (so a document rebuild, a
//! streaming append or a reflow cannot silently move the selection onto
//! different text).

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;

pub use crate::ui::conversation::document::Anchor;
use crate::ui::conversation::document::{ConversationDocument, TextRow};

/// A position in the document: absolute display row plus a character column
/// within that row's readable text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DocPos {
    pub row: usize,
    pub col: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: DocPos,
    pub head: DocPos,
}

impl Selection {
    pub fn new(anchor: DocPos) -> Self {
        Self {
            anchor,
            head: anchor,
        }
    }

    pub fn with_head(mut self, head: DocPos) -> Self {
        self.head = head;
        self
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    /// `(start, end)` in document order.
    pub fn normalized(&self) -> (DocPos, DocPos) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }
}

fn char_widths(text: &str) -> Vec<usize> {
    let mut bounds: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
    bounds.push(text.len());
    bounds
        .windows(2)
        .map(|w| Span::raw(&text[w[0]..w[1]]).width())
        .collect()
}

/// Character column of `text` shown at display cell `cell_x`.
///
/// `text_col` is the display cell where `text` begins inside its rendered row.
/// Clicking a leading decorative cell clamps to the first character; clicking
/// inside a wide glyph snaps to that glyph's start.
pub fn cell_to_col(text: &str, text_col: u16, cell_x: u16) -> usize {
    let target = (cell_x as usize).saturating_sub(text_col as usize);
    let mut acc = 0;
    for (i, w) in char_widths(text).into_iter().enumerate() {
        if acc + w > target {
            return i;
        }
        acc += w;
    }
    text.chars().count()
}

/// Display cell where character column `col` of `text` begins.
pub fn col_to_cell(text: &str, text_col: u16, col: usize) -> u16 {
    let taken: usize = char_widths(text).into_iter().take(col).sum();
    (text_col as usize + taken) as u16
}

/// Map a mouse position to a document position.
///
/// Returns `None` only when `row` is outside the document. Rows without
/// readable text (diagram rows) map to column 0 and act as separators.
pub fn hit_test(doc: &ConversationDocument, row: usize, cell_x: u16) -> Option<DocPos> {
    if row >= doc.total_rows() {
        return None;
    }
    Some(match doc.row(row) {
        Some(meta) => DocPos {
            row,
            col: cell_to_col(&meta.text, meta.text_col, cell_x),
        },
        None => DocPos { row, col: 0 },
    })
}

/// Content anchor for a display position, used to survive document rebuilds.
///
/// Decorative rows carry no copyable text, so a position on one anchors to the
/// nearest selectable row instead — extraction output is unchanged, but the
/// anchor still round-trips.
pub fn anchor_at(doc: &ConversationDocument, pos: DocPos) -> Option<Anchor> {
    if let Some(meta) = doc.row(pos.row)
        && meta.selectable
    {
        let len = meta.text.chars().count();
        return Some(Anchor {
            message: meta.message.clone(),
            offset: meta.offset + pos.col.min(len),
        });
    }
    let next = doc.rows().find_map(|(abs, row)| {
        row.filter(|m| abs > pos.row && m.selectable)
            .map(|m| Anchor {
                message: m.message.clone(),
                offset: m.offset,
            })
    });
    if next.is_some() {
        return next;
    }
    doc.rows()
        .filter_map(|(abs, row)| row.filter(|m| abs < pos.row && m.selectable))
        .last()
        .map(|m| Anchor {
            message: m.message.clone(),
            offset: m.offset + m.text.chars().count(),
        })
}

/// Re-resolve a content anchor against a rebuilt document.
pub fn resolve_anchor(doc: &ConversationDocument, anchor: &Anchor) -> Option<DocPos> {
    if anchor.message.is_empty() {
        return None;
    }
    let mut tail: Option<DocPos> = None;
    for (abs, row) in doc.rows() {
        let Some(row) = row else { continue };
        if !row.selectable || row.message != anchor.message {
            continue;
        }
        let len = row.text.chars().count();
        if anchor.offset < row.offset || anchor.offset > row.offset + len {
            continue;
        }
        let col = anchor.offset - row.offset;
        if col < len {
            return Some(DocPos { row: abs, col });
        }
        tail = Some(DocPos { row: abs, col: len });
    }
    tail
}

/// Copyable text for a selection, in readable form: soft wraps joined back
/// together, hard breaks preserved, decorative rows skipped and message
/// boundaries separated by a blank line.
pub fn extract_text(doc: &ConversationDocument, selection: Selection) -> String {
    let (start, end) = selection.normalized();
    let mut out = String::new();
    let mut started = false;
    let mut pending_nl = 0usize;
    let mut prev_message = String::new();

    for (abs, row) in doc.rows() {
        if abs < start.row {
            continue;
        }
        if abs > end.row {
            break;
        }

        let Some(row) = row else {
            if started {
                pending_nl = pending_nl.max(2);
            }
            continue;
        };
        if !row.selectable {
            if started {
                pending_nl = pending_nl.max(2);
            }
            continue;
        }

        let piece = row_piece(row, abs, start, end);
        if piece.is_empty() {
            // A blank content line is a paragraph break, not copied text.
            if started {
                pending_nl = pending_nl.max(2);
            }
            continue;
        }

        if !started {
            out.push_str(&piece);
            started = true;
        } else {
            let continues = row.continuation && row.message == prev_message && pending_nl == 0;
            let sep = if continues { 0 } else { pending_nl.max(1) };
            for _ in 0..sep {
                out.push('\n');
            }
            out.push_str(&piece);
        }
        pending_nl = 0;
        prev_message.clone_from(&row.message);
    }

    out
}

fn row_piece(row: &TextRow, abs: usize, start: DocPos, end: DocPos) -> String {
    let total = row.text.chars().count();
    let from = if abs == start.row {
        start.col.min(total)
    } else {
        0
    };
    let to = if abs == end.row {
        end.col.min(total)
    } else {
        total
    };
    if to <= from {
        return String::new();
    }
    row.text.chars().skip(from).take(to - from).collect()
}

/// The style painted under selected text. Patched onto the row's own style so
/// bold/italic and text colour survive; it overrides search highlighting.
pub fn selection_style() -> Style {
    Style::default().bg(ratatui::style::Color::Rgb(70, 90, 140))
}

/// Paint `style` over the display cells `[start, end)` of `line`.
///
/// Glyphs are never split: a wide character straddling a boundary is styled as
/// a whole.
pub fn apply_cell_style(
    line: &mut ratatui::text::Line<'static>,
    start: u16,
    end: u16,
    style: Style,
) {
    if end <= start {
        return;
    }
    let mut pieces: Vec<(String, Style)> = Vec::new();
    let mut cell = 0u16;
    for span in std::mem::take(&mut line.spans) {
        let base = span.style;
        let content = span.content.to_string();
        let mut chars = content.chars();
        for width in char_widths(&content) {
            let Some(ch) = chars.next() else { break };
            let next = cell + width as u16;
            let covered = cell < end && next > start;
            let style = if covered { base.patch(style) } else { base };
            match pieces.last_mut() {
                Some((text, prev)) if *prev == style => text.push(ch),
                _ => pieces.push((ch.to_string(), style)),
            }
            cell = next;
        }
    }
    line.spans = pieces
        .into_iter()
        .map(|(text, style)| Span::styled(text, style))
        .collect();
}

/// Highlight the selected character ranges of visible rows.
///
/// `scroll` is the absolute document row of `rows[0]`.
pub fn highlight_rows(rows: &mut [TextRow], scroll: usize, selection: Selection) {
    let (start, end) = selection.normalized();
    for (i, row) in rows.iter_mut().enumerate() {
        let abs = scroll + i;
        if abs < start.row || abs > end.row {
            continue;
        }
        if !row.selectable {
            continue;
        }
        let total = row.text.chars().count();
        let from = if abs == start.row {
            start.col.min(total)
        } else {
            0
        };
        let to = if abs == end.row {
            end.col.min(total)
        } else {
            total
        };
        if to <= from {
            continue;
        }
        let cell_start = col_to_cell(&row.text, row.text_col, from);
        let cell_end = col_to_cell(&row.text, row.text_col, to);
        apply_cell_style(&mut row.line, cell_start, cell_end, selection_style());
    }
}

/// Which way the viewport should scroll while a drag sits at an edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollDir {
    Up,
    Down,
}

/// Scroll direction and speed (rows per second) for a pointer at `pointer_y`.
///
/// A pointer above or below the pane scrolls at full speed; a pointer inside an
/// edge band scrolls faster the deeper it sits. `None` means leave the
/// viewport alone.
pub fn edge_scroll(pointer_y: u16, content: Rect) -> Option<(ScrollDir, f32)> {
    if content.height == 0 {
        return None;
    }
    let top = content.y;
    let bottom = content.y + content.height;
    if pointer_y < top {
        return Some((ScrollDir::Up, 60.0));
    }
    if pointer_y >= bottom {
        return Some((ScrollDir::Down, 60.0));
    }

    let edge = (content.height / 4).clamp(1, 3);
    if pointer_y < top + edge {
        let depth = (top + edge - pointer_y) as f32 / edge as f32;
        return Some((ScrollDir::Up, 20.0 + 40.0 * depth.min(1.0)));
    }
    if pointer_y >= bottom - edge {
        let depth = (pointer_y + 1 - (bottom - edge)) as f32 / edge as f32;
        return Some((ScrollDir::Down, 20.0 + 40.0 * depth.min(1.0)));
    }
    None
}

/// Turn elapsed time into whole rows to scroll, bounding catch-up after a
/// stall so the viewport cannot teleport.
pub fn scroll_rows(carry: &mut f32, dt: f32, rows_per_second: f32, max_step: usize) -> usize {
    *carry += dt * rows_per_second;
    let mut moved = 0usize;
    while *carry >= 1.0 {
        *carry -= 1.0;
        moved += 1;
        if moved >= max_step {
            *carry = 0.0;
            break;
        }
    }
    moved
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;
    use ratatui::style::{Modifier, Style};
    use ratatui::text::Line;

    fn body(text: &str, message: &str, offset: usize, continuation: bool) -> TextRow {
        TextRow {
            line: Line::from(text.to_string()),
            text: text.to_string(),
            text_col: 0,
            continuation,
            selectable: true,
            message: message.to_string(),
            offset,
        }
    }

    fn decoration() -> TextRow {
        TextRow::decoration(Line::from("─"))
    }

    #[test]
    fn empty_selection_produces_no_text() {
        let mut doc = ConversationDocument::new();
        doc.push_text_rows(vec![body("hello", "m", 0, false)], "m", &mut 0);
        let pos = DocPos { row: 0, col: 2 };
        assert_eq!(extract_text(&doc, Selection::new(pos)), "");
    }

    #[test]
    fn decoration_rows_between_text_become_a_blank_line() {
        let mut doc = ConversationDocument::new();
        doc.push_text_rows(vec![body("one", "m1", 0, false)], "m1", &mut 0);
        doc.push_decoration(vec![decoration().line]);
        doc.push_text_rows(vec![body("two", "m2", 0, false)], "m2", &mut 0);
        let sel = Selection::new(DocPos { row: 0, col: 0 }).with_head(DocPos {
            row: 2,
            col: usize::MAX,
        });
        assert_eq!(extract_text(&doc, sel), "one\n\ntwo");
    }

    #[test]
    fn blank_content_rows_are_collapsed_into_the_separating_break() {
        let mut doc = ConversationDocument::new();
        doc.push_text_rows(
            vec![
                body("one", "m", 0, false),
                body("", "m", 4, false),
                body("", "m", 5, false),
                body("two", "m", 6, false),
            ],
            "m",
            &mut 0,
        );
        let sel = Selection::new(DocPos { row: 0, col: 0 }).with_head(DocPos {
            row: 3,
            col: usize::MAX,
        });
        assert_eq!(extract_text(&doc, sel), "one\n\ntwo");
    }

    #[test]
    fn consecutive_content_rows_keep_single_line_breaks() {
        let mut doc = ConversationDocument::new();
        doc.push_text_rows(
            vec![
                body("fn main() {", "m", 0, false),
                body("    run();", "m", 12, false),
                body("}", "m", 23, false),
            ],
            "m",
            &mut 0,
        );
        let sel = Selection::new(DocPos { row: 0, col: 0 }).with_head(DocPos {
            row: 2,
            col: usize::MAX,
        });
        assert_eq!(extract_text(&doc, sel), "fn main() {\n    run();\n}");
    }

    #[test]
    fn soft_wrap_continuations_are_joined() {
        let mut doc = ConversationDocument::new();
        doc.push_text_rows(
            vec![body("hello ", "m", 0, false), body("world", "m", 6, true)],
            "m",
            &mut 0,
        );
        let sel = Selection::new(DocPos { row: 0, col: 0 }).with_head(DocPos {
            row: 1,
            col: usize::MAX,
        });
        assert_eq!(extract_text(&doc, sel), "hello world");
    }

    #[test]
    fn partial_rows_are_sliced_by_character_column() {
        let mut doc = ConversationDocument::new();
        doc.push_text_rows(vec![body("hello world", "m", 0, false)], "m", &mut 0);
        let sel = Selection::new(DocPos { row: 0, col: 6 }).with_head(DocPos { row: 0, col: 11 });
        assert_eq!(extract_text(&doc, sel), "world");
    }

    #[test]
    fn anchor_round_trips_through_a_rebuild() {
        let mut doc = ConversationDocument::new();
        doc.push_text_rows(vec![body("alpha beta", "m", 0, false)], "m", &mut 0);
        let anchor = anchor_at(&doc, DocPos { row: 0, col: 6 }).unwrap();

        let mut rebuilt = ConversationDocument::new();
        rebuilt.push_text_rows(vec![body("alpha beta", "m", 0, false)], "m", &mut 0);
        rebuilt.push_text_rows(vec![body("later", "m2", 0, false)], "m2", &mut 0);
        assert_eq!(
            resolve_anchor(&rebuilt, &anchor),
            Some(DocPos { row: 0, col: 6 })
        );
    }

    #[test]
    fn anchor_for_missing_message_is_dropped() {
        let mut doc = ConversationDocument::new();
        doc.push_text_rows(vec![body("alpha", "m", 0, false)], "m", &mut 0);
        let gone = Anchor {
            message: "other".into(),
            offset: 0,
        };
        assert_eq!(resolve_anchor(&doc, &gone), None);
    }

    #[test]
    fn anchor_on_decoration_falls_forward_to_the_next_body_row() {
        let mut doc = ConversationDocument::new();
        doc.push_decoration(vec![Line::from("header")]);
        doc.push_text_rows(vec![body("payload", "m", 0, false)], "m", &mut 0);
        assert_eq!(
            anchor_at(&doc, DocPos { row: 0, col: 0 }),
            Some(Anchor {
                message: "m".into(),
                offset: 0
            })
        );
    }

    #[test]
    fn pointer_in_the_middle_of_the_viewport_does_not_scroll() {
        let content = Rect::new(0, 0, 20, 10);
        assert_eq!(edge_scroll(5, content), None);
    }

    #[test]
    fn pointer_outside_the_viewport_scrolls_at_full_speed() {
        let content = Rect::new(0, 5, 20, 10);
        assert_eq!(edge_scroll(0, content), Some((ScrollDir::Up, 60.0)));
        assert_eq!(edge_scroll(15, content), Some((ScrollDir::Down, 60.0)));
    }

    #[test]
    fn edge_bands_scroll_faster_the_deeper_the_pointer() {
        let content = Rect::new(0, 0, 20, 10);
        let (dir_near, near) = edge_scroll(1, content).expect("top band scrolls");
        let (dir_deep, deep) = edge_scroll(0, content).expect("top edge scrolls");
        assert_eq!(dir_near, ScrollDir::Up);
        assert_eq!(dir_deep, ScrollDir::Up);
        assert!(
            deep > near,
            "deeper pointer should scroll faster: {deep} vs {near}"
        );

        let (dir_bottom, bottom) = edge_scroll(9, content).expect("bottom band scrolls");
        assert_eq!(dir_bottom, ScrollDir::Down);
        assert!(bottom > 0.0);
    }

    #[test]
    fn zero_height_viewport_never_scrolls() {
        assert_eq!(edge_scroll(0, Rect::new(0, 0, 20, 0)), None);
    }

    #[test]
    fn scroll_rows_accumulate_time_and_bound_catch_up() {
        let mut carry = 0.0;
        assert_eq!(scroll_rows(&mut carry, 0.5, 2.0, 20), 1);
        assert_eq!(scroll_rows(&mut carry, 0.4, 2.0, 20), 0);
        assert_eq!(scroll_rows(&mut carry, 0.1, 2.0, 20), 1);
    }

    #[test]
    fn scroll_rows_drop_carry_when_a_stall_would_overshoot() {
        let mut carry = 0.0;
        // A long stall must not teleport the viewport to the end.
        assert_eq!(scroll_rows(&mut carry, 30.0, 10.0, 5), 5);
        assert_eq!(carry, 0.0, "carry is dropped after the cap");
    }

    #[test]
    fn selection_highlight_splits_spans_and_keeps_other_styles() {
        let mut row = TextRow::body(
            Line::from(vec![
                Span::raw("ab"),
                Span::styled("cd", Style::default().add_modifier(Modifier::BOLD)),
            ]),
            "abcd".to_string(),
            0,
            false,
        );
        apply_cell_style(&mut row.line, 1, 3, selection_style());

        let flat: String = row.line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(flat, "abcd");
        let bgs: Vec<_> = row.line.spans.iter().map(|s| s.style.bg).collect();
        assert_eq!(
            bgs,
            vec![None, selection_style().bg, selection_style().bg, None],
            "selection covers exactly the styled cells: {:?}",
            row.line.spans
        );
        assert!(
            row.line.spans[2]
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
        assert!(
            row.line.spans[3]
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
    }

    #[test]
    fn selection_highlight_covers_wide_glyphs_whole() {
        let mut row = TextRow::body(Line::from("日本"), "日本".to_string(), 0, false);
        apply_cell_style(&mut row.line, 1, 2, selection_style());
        // The glyph straddling the boundary is highlighted as a unit.
        assert_eq!(row.line.spans[0].style.bg, selection_style().bg);
        assert_eq!(row.line.spans[1].style.bg, None);
    }
}
