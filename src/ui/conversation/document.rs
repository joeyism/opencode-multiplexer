use crate::ui::conversation::diagram::slot::{DiagramPhase, DiagramSlot};
use ratatui::text::Line;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextRow {
    /// Rendered line, including decorative gutter/prefix spans.
    pub line: Line<'static>,
    /// Readable text: the row's content after decorative prefixes.
    pub text: String,
    /// Display cell where `text` starts inside `line`.
    pub text_col: u16,
    /// True when this row continues the previous row through a soft wrap.
    pub continuation: bool,
    /// False for decoration that is never copied (headers, rules, padding).
    pub selectable: bool,
    /// Id of the message this row belongs to; empty for decoration.
    pub message: String,
    /// Character offset of `text` within the message's readable text.
    pub offset: usize,
}

impl TextRow {
    /// A plain content line with no decoration: the whole line is copyable.
    pub fn plain(line: Line<'static>) -> Self {
        let text = line
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>();
        Self {
            line,
            text,
            text_col: 0,
            continuation: false,
            selectable: true,
            message: String::new(),
            offset: 0,
        }
    }

    /// Decorative content: rendered, but never copied.
    pub fn decoration(line: Line<'static>) -> Self {
        Self {
            line,
            text: String::new(),
            text_col: 0,
            continuation: false,
            selectable: false,
            message: String::new(),
            offset: 0,
        }
    }

    /// A body row: `text` is the copyable content, `text_col` where it starts.
    pub fn body(line: Line<'static>, text: String, text_col: u16, continuation: bool) -> Self {
        Self {
            line,
            text,
            text_col,
            continuation,
            selectable: true,
            message: String::new(),
            offset: 0,
        }
    }

    /// Stable identity of this row's content, usable to re-find it after the
    /// document is rebuilt or reflowed.
    pub fn anchor(&self) -> Anchor {
        Anchor {
            message: self.message.clone(),
            offset: self.offset,
        }
    }
}

/// Content-based identity of a selection endpoint. Unlike a `(row, col)` pair
/// it survives document rebuilds, streaming appends and width changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchor {
    pub message: String,
    pub offset: usize,
}

pub enum DocBlock {
    Text(Vec<TextRow>),
    Diagram(DiagramSlot),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageHeader {
    pub block_idx: usize,
    pub role: MessageRole,
}

pub struct ConversationDocument {
    pub blocks: Vec<DocBlock>,
    pub headers: Vec<MessageHeader>,
}

pub struct SlotInfo<'a> {
    pub start_row: usize,
    pub slot: &'a DiagramSlot,
}

/// One display row: `Some(row)` for text, `None` for diagram rows.
pub type RowEntry<'a> = (usize, Option<&'a TextRow>);

pub struct RowIter<'a> {
    blocks: std::slice::Iter<'a, DocBlock>,
    current: usize,
    text: Option<std::slice::Iter<'a, TextRow>>,
    diagram_rows: usize,
}

impl<'a> Iterator for RowIter<'a> {
    type Item = RowEntry<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.diagram_rows > 0 {
                let row = self.current;
                self.current += 1;
                self.diagram_rows -= 1;
                return Some((row, None));
            }
            if let Some(iter) = self.text.as_mut() {
                if let Some(r) = iter.next() {
                    let row = self.current;
                    self.current += 1;
                    return Some((row, Some(r)));
                }
                self.text = None;
            }
            match self.blocks.next()? {
                DocBlock::Text(rows) => {
                    self.text = Some(rows.iter());
                }
                DocBlock::Diagram(slot) => {
                    self.diagram_rows = slot.row_height;
                }
            }
        }
    }
}

/// Display rows a diagram slot expands to. Diagram pixels and their
/// placeholders are never copied as text.
fn diagram_rows(slot: &DiagramSlot, protocol_enabled: bool) -> Vec<TextRow> {
    match &slot.phase {
        DiagramPhase::Ready { halfblocks, .. } => {
            if protocol_enabled {
                // Blank spacers: Kitty paints pixels on top. Using halfblocks
                // here causes visible flash whenever placements are refreshed.
                (0..slot.row_height)
                    .map(|_| TextRow::decoration(Line::from("")))
                    .collect()
            } else {
                halfblocks
                    .iter()
                    .cloned()
                    .map(TextRow::decoration)
                    .collect()
            }
        }
        DiagramPhase::Failed { message, fallback } => {
            if fallback.is_empty() {
                vec![TextRow::decoration(Line::from(format!(
                    "▌ [mermaid error: {message}]"
                )))]
            } else {
                fallback.iter().cloned().map(TextRow::decoration).collect()
            }
        }
        DiagramPhase::Unavailable { fallback } => {
            if fallback.is_empty() {
                vec![TextRow::decoration(Line::from(
                    "▌ [mermaid unavailable (mmdc not on PATH)]",
                ))]
            } else {
                fallback.iter().cloned().map(TextRow::decoration).collect()
            }
        }
        _ => (0..slot.row_height)
            .map(|i| {
                TextRow::decoration(Line::from(format!(
                    "▌ [mermaid rendering row {}...]",
                    i + 1
                )))
            })
            .collect(),
    }
}

impl Default for ConversationDocument {
    fn default() -> Self {
        Self::new()
    }
}

impl ConversationDocument {
    pub fn new() -> Self {
        Self {
            blocks: Vec::new(),
            headers: Vec::new(),
        }
    }

    /// Record that the next pushed block is a message header of `role`.
    /// Block indices (not rows) are stored because diagram row heights can
    /// change in place when renders finish.
    pub fn record_header(&mut self, role: MessageRole) {
        self.headers.push(MessageHeader {
            block_idx: self.blocks.len(),
            role,
        });
    }

    pub fn headers(&self) -> &[MessageHeader] {
        &self.headers
    }

    /// Absolute row where `header_idx`'s header block begins.
    pub fn header_row(&self, header_idx: usize) -> Option<usize> {
        let header = self.headers.get(header_idx)?;
        let mut row = 0;
        for (i, block) in self.blocks.iter().enumerate() {
            if i == header.block_idx {
                return Some(row);
            }
            row += match block {
                DocBlock::Text(rows) => rows.len(),
                DocBlock::Diagram(slot) => slot.row_height,
            };
        }
        None
    }

    /// Absolute rows of all headers matching `role`, in document order.
    pub fn header_rows(&self, role: MessageRole) -> Vec<usize> {
        let mut rows = Vec::new();
        for (i, h) in self.headers.iter().enumerate() {
            if h.role == role
                && let Some(r) = self.header_row(i)
            {
                rows.push(r);
            }
        }
        rows
    }

    /// Push plain content lines (no decoration, fully copyable).
    pub fn push_lines(&mut self, lines: Vec<Line<'static>>) {
        if lines.is_empty() {
            return;
        }
        self.blocks.push(DocBlock::Text(
            lines.into_iter().map(TextRow::plain).collect(),
        ));
    }

    /// Push rendered-but-never-copied lines: headers, rules, padding.
    pub fn push_decoration(&mut self, lines: Vec<Line<'static>>) {
        if lines.is_empty() {
            return;
        }
        self.blocks.push(DocBlock::Text(
            lines.into_iter().map(TextRow::decoration).collect(),
        ));
    }

    /// Push body rows belonging to `message`, assigning content offsets.
    ///
    /// `next_offset` is the caller's running character offset into the
    /// message's readable text; it is advanced past every pushed row. Rows
    /// that continue a soft wrap sit immediately after the previous row's
    /// text; every other row is followed by a newline in the message text.
    pub fn push_text_rows(&mut self, rows: Vec<TextRow>, message: &str, next_offset: &mut usize) {
        if rows.is_empty() {
            return;
        }
        let mut tagged = Vec::with_capacity(rows.len());
        for (i, mut row) in rows.into_iter().enumerate() {
            if i > 0 && !row.continuation {
                // Rows that do not continue a soft wrap are separated by a
                // newline in the message's readable text.
                *next_offset += 1;
            }
            row.message = message.to_string();
            row.offset = *next_offset;
            *next_offset += row.text.chars().count();
            tagged.push(row);
        }
        self.blocks.push(DocBlock::Text(tagged));
    }

    pub fn push_diagram(&mut self, slot: DiagramSlot) {
        self.blocks.push(DocBlock::Diagram(slot));
    }

    /// Whether the document currently ends on a blank (empty-content) line.
    /// Used to avoid emitting consecutive blank separators.
    pub fn ends_with_blank(&self) -> bool {
        self.blocks.last().is_some_and(|b| match b {
            DocBlock::Text(rows) => rows
                .last()
                .is_some_and(|r| r.line.spans.iter().all(|s| s.content.is_empty())),
            DocBlock::Diagram(_) => false,
        })
    }

    pub fn total_rows(&self) -> usize {
        self.blocks
            .iter()
            .map(|b| match b {
                DocBlock::Text(rows) => rows.len(),
                DocBlock::Diagram(slot) => slot.row_height,
            })
            .sum()
    }

    /// Every display row with its absolute index and, for text rows, metadata.
    pub fn rows(&self) -> RowIter<'_> {
        RowIter {
            blocks: self.blocks.iter(),
            current: 0,
            text: None,
            diagram_rows: 0,
        }
    }

    /// Metadata of the text row at `abs`, if that row is text.
    pub fn row(&self, abs: usize) -> Option<&TextRow> {
        self.rows().find(|(i, _)| *i == abs).and_then(|(_, r)| r)
    }

    /// Readable text of the row at `abs`, if that row is text.
    pub fn row_text(&self, abs: usize) -> Option<&str> {
        self.row(abs).map(|r| r.text.as_str())
    }

    pub fn visible_lines(
        &self,
        scroll: usize,
        vp: usize,
        protocol_enabled: bool,
    ) -> Vec<Line<'static>> {
        self.visible_rows(scroll, vp, protocol_enabled)
            .into_iter()
            .map(|r| r.line)
            .collect()
    }

    /// Visible rows including their selection metadata, in display order.
    ///
    /// Only the visible window is materialised: cloning the whole document on
    /// every frame does not scale to long conversations.
    pub fn visible_rows(&self, scroll: usize, vp: usize, protocol_enabled: bool) -> Vec<TextRow> {
        if vp == 0 {
            return Vec::new();
        }
        let end = scroll + vp;
        let mut out = Vec::new();
        let mut row = 0usize;

        'outer: for block in &self.blocks {
            match block {
                DocBlock::Text(rows) => {
                    for r in rows {
                        if row >= scroll && row < end {
                            out.push(r.clone());
                        }
                        row += 1;
                        if row >= end {
                            break 'outer;
                        }
                    }
                }
                DocBlock::Diagram(slot) => {
                    for item in diagram_rows(slot, protocol_enabled) {
                        if row >= scroll && row < end {
                            out.push(item);
                        }
                        row += 1;
                        if row >= end {
                            break 'outer;
                        }
                    }
                }
            }
        }
        out
    }

    pub fn find_slot_start(&self, hash: &str) -> Option<usize> {
        let mut current_row = 0;
        for block in &self.blocks {
            match block {
                DocBlock::Text(rows) => current_row += rows.len(),
                DocBlock::Diagram(slot) => {
                    if slot.hash == hash {
                        return Some(current_row);
                    }
                    current_row += slot.row_height;
                }
            }
        }
        None
    }

    pub fn diagram_covering_row(&self, row: usize) -> Option<SlotInfo<'_>> {
        let mut current_row = 0;
        for block in &self.blocks {
            let height = match block {
                DocBlock::Text(rows) => rows.len(),
                DocBlock::Diagram(slot) => slot.row_height,
            };

            if row >= current_row && row < current_row + height {
                if let DocBlock::Diagram(slot) = block {
                    return Some(SlotInfo {
                        start_row: current_row,
                        slot,
                    });
                } else {
                    return None;
                }
            }
            current_row += height;
        }
        None
    }

    pub fn first_visible_ready_diagram(
        &self,
        scroll: usize,
        viewport_height: usize,
    ) -> Option<SlotInfo<'_>> {
        if viewport_height == 0 {
            return None;
        }

        let viewport_end = scroll.saturating_add(viewport_height);
        let mut start_row: usize = 0;
        for block in &self.blocks {
            let height = match block {
                DocBlock::Text(rows) => rows.len(),
                DocBlock::Diagram(slot) => slot.row_height,
            };
            let end_row = start_row.saturating_add(height);

            if let DocBlock::Diagram(slot) = block
                && start_row < viewport_end
                && end_row > scroll
                && matches!(&slot.phase, DiagramPhase::Ready { .. })
            {
                return Some(SlotInfo { start_row, slot });
            }

            start_row = end_row;
        }
        None
    }
}
