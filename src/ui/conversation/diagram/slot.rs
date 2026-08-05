use ratatui::text::Line;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone)]
pub enum DiagramPhase {
    EmptySource,
    Pending,
    InFlight,
    Ready {
        png: Arc<Vec<u8>>,
        png_w: u32,
        png_h: u32,
        width: u16,
        halfblocks: Vec<Line<'static>>,
        row_height: usize,
        /// Terminal columns the image should occupy (aspect-correct).
        col_width: usize,
    },
    Failed {
        message: String,
        fallback: Vec<Line<'static>>,
    },
    Unavailable {
        fallback: Vec<Line<'static>>,
    },
}

#[derive(Clone)]
pub struct DiagramSlot {
    pub hash: String,
    pub source: String,
    pub phase: DiagramPhase,
    pub row_height: usize,
}

impl DiagramSlot {
    pub fn placeholder(hash: &str, source: &str, rows: usize) -> Self {
        Self {
            hash: hash.to_string(),
            source: source.to_string(),
            phase: DiagramPhase::Pending,
            row_height: rows,
        }
    }
}

#[derive(Default)]
pub struct DiagramIndex {
    pub slots: HashMap<String, Arc<DiagramSlot>>,
}

impl DiagramIndex {
    pub fn get(&self, hash: &str) -> Option<Arc<DiagramSlot>> {
        self.slots.get(hash).cloned()
    }
}
