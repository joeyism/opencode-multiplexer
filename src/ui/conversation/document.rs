use crate::ui::conversation::diagram::slot::{DiagramPhase, DiagramSlot};
use ratatui::text::Line;

pub enum DocBlock {
    Text(Vec<Line<'static>>),
    Diagram(DiagramSlot),
}

pub struct ConversationDocument {
    pub blocks: Vec<DocBlock>,
}

pub struct SlotInfo<'a> {
    pub start_row: usize,
    pub slot: &'a DiagramSlot,
}

impl Default for ConversationDocument {
    fn default() -> Self {
        Self::new()
    }
}

impl ConversationDocument {
    pub fn new() -> Self {
        Self { blocks: Vec::new() }
    }

    pub fn push_lines(&mut self, lines: Vec<Line<'static>>) {
        if lines.is_empty() {
            return;
        }
        self.blocks.push(DocBlock::Text(lines));
    }

    pub fn push_diagram(&mut self, slot: DiagramSlot) {
        self.blocks.push(DocBlock::Diagram(slot));
    }

    pub fn total_rows(&self) -> usize {
        self.blocks
            .iter()
            .map(|b| match b {
                DocBlock::Text(lines) => lines.len(),
                DocBlock::Diagram(slot) => slot.row_height,
            })
            .sum()
    }

    pub fn visible_lines(
        &self,
        scroll: usize,
        vp: usize,
        protocol_enabled: bool,
    ) -> Vec<Line<'static>> {
        let mut all_lines = Vec::new();
        for block in &self.blocks {
            match block {
                DocBlock::Text(lines) => {
                    all_lines.extend(lines.iter().cloned());
                }
                DocBlock::Diagram(slot) => {
                    match &slot.phase {
                        DiagramPhase::Ready { halfblocks, .. } => {
                            if protocol_enabled {
                                // Blank spacers: Kitty paints pixels on top.
                                // Using halfblocks here causes visible flash
                                // whenever placements are refreshed.
                                for _ in 0..slot.row_height {
                                    all_lines.push(Line::from(""));
                                }
                            } else {
                                all_lines.extend(halfblocks.iter().cloned());
                            }
                        }
                        DiagramPhase::Failed { message, fallback } => {
                            if fallback.is_empty() {
                                all_lines.push(Line::from(format!("│ [mermaid error: {message}]")));
                            } else {
                                all_lines.extend(fallback.iter().cloned());
                            }
                        }
                        DiagramPhase::Unavailable { fallback } => {
                            if fallback.is_empty() {
                                all_lines
                                    .push(Line::from("│ [mermaid unavailable (mmdc not on PATH)]"));
                            } else {
                                all_lines.extend(fallback.iter().cloned());
                            }
                        }
                        _ => {
                            // Expand placeholder
                            for i in 0..slot.row_height {
                                all_lines.push(Line::from(format!(
                                    "│ [mermaid rendering row {}...]",
                                    i + 1
                                )));
                            }
                        }
                    }
                }
            }
        }

        let end = (scroll + vp).min(all_lines.len());
        if scroll >= all_lines.len() {
            return Vec::new();
        }
        all_lines[scroll..end].to_vec()
    }

    pub fn find_slot_start(&self, hash: &str) -> Option<usize> {
        let mut current_row = 0;
        for block in &self.blocks {
            match block {
                DocBlock::Text(lines) => current_row += lines.len(),
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
                DocBlock::Text(lines) => lines.len(),
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
}
