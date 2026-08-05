use std::sync::Arc;
use std::time::Instant;

use ratatui::text::Line;

use crate::app::focus::AppFocus;
use crate::ui::conversation::diagram::mmdc::MermaidRenderConfig;
use crate::ui::conversation::diagram::protocol::{
    DiagramPaint, KittyImagePlacer, kitty_graphics_supported, slot_visibility, source_crop,
};
use crate::ui::conversation::diagram::scheduler::{DiagramScheduler, RenderFinished};
use crate::ui::conversation::diagram::slot::{DiagramIndex, DiagramPhase, DiagramSlot};
use crate::ui::conversation::document::{ConversationDocument, DocBlock};
use ratatui::layout::Rect;
use std::io::Write;

/// Owned Kitty paint record: hash, png bytes, dest rect, source crop x/y/w/h.
type KittyPaintOwned = (String, Vec<u8>, Rect, u32, u32, u32, u32);

pub struct ConversationViewState {
    session_id: Option<String>,
    session_title: String,
    return_focus: AppFocus,
    document: ConversationDocument,
    diagrams: DiagramIndex,
    scheduler: Option<DiagramScheduler>,
    scroll: usize,
    follow_tail: bool,
    last_poll: Option<Instant>,
    load_error: Option<String>,
    protocol_enabled: bool,
    kitty: KittyImagePlacer,
    // Search state
    search_query: String,
    search_active: bool,
    match_positions: Vec<(usize, usize, usize)>, // (line_idx, byte_start, byte_len)
    current_match: usize,
}

impl Default for ConversationViewState {
    fn default() -> Self {
        Self {
            session_id: None,
            session_title: String::new(),
            return_focus: AppFocus::Sidebar,
            document: ConversationDocument::new(),
            diagrams: DiagramIndex::default(),
            scheduler: None,
            scroll: 0,
            follow_tail: true,
            last_poll: None,
            load_error: None,
            protocol_enabled: false,
            kitty: KittyImagePlacer::new(),
            search_query: String::new(),
            search_active: false,
            match_positions: Vec::new(),
            current_match: 0,
        }
    }
}

impl ConversationViewState {
    pub fn open(&mut self, session_id: String, session_title: String, return_focus: AppFocus) {
        self.session_id = Some(session_id);
        self.session_title = session_title;
        self.return_focus = return_focus;
        self.document = ConversationDocument::new();
        self.scroll = 0;
        self.follow_tail = true;
        self.last_poll = None;
        self.load_error = None;
        self.search_query.clear();
        self.search_active = false;
        self.match_positions.clear();
        self.current_match = 0;
    }

    pub fn close(&mut self) -> AppFocus {
        self.session_id = None;
        self.document = ConversationDocument::new();
        self.scroll = 0;
        self.load_error = None;
        self.search_query.clear();
        self.search_active = false;
        self.match_positions.clear();
        self.current_match = 0;
        // Kitty placements are cleared by the main loop via clear_kitty_graphics
        // when focus leaves conversation; keep placer state for next open.
        self.return_focus
    }

    pub fn is_active(&self) -> bool {
        self.session_id.is_some()
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn session_title(&self) -> &str {
        &self.session_title
    }

    pub fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    pub fn should_poll(&self, now: Instant) -> bool {
        self.session_id.is_some()
            && self
                .last_poll
                .is_none_or(|last| now.duration_since(last).as_millis() >= 1000)
    }

    pub fn mark_polled(&mut self, now: Instant) {
        self.last_poll = Some(now);
    }

    pub fn replace_document(&mut self, doc: ConversationDocument, viewport_height: usize) {
        let was_at_tail = self.follow_tail;
        self.document = doc;
        if was_at_tail {
            self.scroll_to_end(viewport_height);
        } else if self.scroll >= self.document.total_rows() {
            self.scroll = self.document.total_rows().saturating_sub(1);
        }
        if !self.search_query.is_empty() {
            self.refresh_matches(viewport_height);
        }
    }

    pub fn set_error(&mut self, error: String) {
        self.load_error = Some(error);
    }

    pub fn clear_error(&mut self) {
        self.load_error = None;
    }

    pub fn visible_lines(&self, viewport_height: usize) -> Vec<Line<'static>> {
        self.document
            .visible_lines(self.scroll, viewport_height, self.protocol_enabled)
    }

    pub fn scroll_offset(&self) -> usize {
        self.scroll
    }

    pub fn document_len(&self) -> usize {
        self.document.total_rows()
    }

    pub fn scroll_up(&mut self, amount: usize) {
        self.scroll = self.scroll.saturating_sub(amount);
        self.follow_tail = false;
    }

    pub fn scroll_down(&mut self, amount: usize, viewport_height: usize) {
        let total = self.document.total_rows();
        let max_scroll = total.saturating_sub(viewport_height);
        self.scroll = (self.scroll + amount).min(max_scroll);
        if self.scroll >= max_scroll {
            self.follow_tail = true;
        }
    }

    pub fn scroll_to_top(&mut self) {
        self.scroll = 0;
        self.follow_tail = false;
    }

    pub fn scroll_to_end(&mut self, viewport_height: usize) {
        let total = self.document.total_rows();
        let max_scroll = total.saturating_sub(viewport_height);
        self.scroll = max_scroll;
        self.follow_tail = true;
    }

    /// Force the next poll to refresh even if within the poll interval.
    /// Useful after viewport changes (e.g., sidebar resize) to ensure the
    /// document is rebuilt with the new content width.
    pub fn force_poll(&mut self) {
        self.last_poll = None;
    }

    pub fn clamp_scroll(&mut self, viewport_height: usize) {
        let total = self.document.total_rows();
        let max_scroll = total.saturating_sub(viewport_height);
        if self.scroll > max_scroll {
            self.scroll = max_scroll;
        }
    }

    // -----------------------------------------------------------------------
    // Search
    // -----------------------------------------------------------------------

    /// Enter search input mode.
    pub fn start_search(&mut self) {
        self.search_active = true;
    }

    /// Exit search input mode without clearing the query.
    pub fn confirm_search(&mut self) {
        self.search_active = false;
    }

    /// Clear the search query and exit input mode.
    pub fn cancel_search(&mut self) {
        self.search_active = false;
        self.search_query.clear();
        self.match_positions.clear();
        self.current_match = 0;
    }

    /// Insert a character into the search query and refresh matches.
    pub fn search_insert(&mut self, ch: char, viewport_height: usize) {
        self.search_query.push(ch);
        self.refresh_matches(viewport_height);
    }

    /// Insert a string (e.g. from paste) into the search query and refresh once.
    pub fn search_insert_str(&mut self, text: &str, viewport_height: usize) {
        self.search_query.push_str(text);
        self.refresh_matches(viewport_height);
    }

    /// Delete last character from the search query and refresh matches.
    pub fn search_backspace(&mut self, viewport_height: usize) {
        self.search_query.pop();
        self.refresh_matches(viewport_height);
    }

    /// Whether the search input bar is active (for key routing).
    pub fn is_searching(&self) -> bool {
        self.search_active
    }

    /// The current search query string.
    pub fn search_query(&self) -> &str {
        &self.search_query
    }

    /// Current match index and total count, if any matches exist.
    pub fn match_status(&self) -> Option<(usize, usize)> {
        if self.match_positions.is_empty() {
            None
        } else {
            Some((self.current_match + 1, self.match_positions.len()))
        }
    }

    /// All match positions: `(line_idx, byte_start, byte_len)`.
    pub fn matches(&self) -> &[(usize, usize, usize)] {
        &self.match_positions
    }

    /// Index of the currently focused match.
    pub fn current_match_index(&self) -> usize {
        self.current_match
    }

    /// Jump to the next match, wrapping around.
    pub fn next_match(&mut self, viewport_height: usize) {
        if self.match_positions.is_empty() {
            return;
        }
        self.current_match = (self.current_match + 1) % self.match_positions.len();
        self.scroll_to_current_match(viewport_height);
    }

    /// Jump to the previous match, wrapping around.
    pub fn prev_match(&mut self, viewport_height: usize) {
        if self.match_positions.is_empty() {
            return;
        }
        if self.current_match == 0 {
            self.current_match = self.match_positions.len() - 1;
        } else {
            self.current_match -= 1;
        }
        self.scroll_to_current_match(viewport_height);
    }

    /// Recalculate match positions from the document and current query.
    fn refresh_matches(&mut self, viewport_height: usize) {
        self.match_positions.clear();
        self.current_match = 0;

        if self.search_query.is_empty() {
            return;
        }

        let query_lower = self.search_query.to_lowercase();
        let mut current_row = 0;

        for block in &self.document.blocks {
            match block {
                DocBlock::Text(lines) => {
                    for (i, line) in lines.iter().enumerate() {
                        let line_idx = current_row + i;
                        let flat: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
                        let flat_lower = flat.to_lowercase();

                        let mut start = 0;
                        while let Some(pos) = flat_lower[start..].find(&query_lower) {
                            let byte_start = start + pos;
                            self.match_positions
                                .push((line_idx, byte_start, query_lower.len()));
                            start = byte_start + query_lower.len();
                        }
                    }
                    current_row += lines.len();
                }
                DocBlock::Diagram(slot) => {
                    let source_lower = slot.source.to_lowercase();
                    if source_lower.contains(&query_lower) {
                        self.match_positions.push((current_row, 0, 0));
                    }
                    current_row += slot.row_height;
                }
            }
        }

        // Jump to the first match at or after the current scroll position.
        if !self.match_positions.is_empty() {
            self.current_match = self
                .match_positions
                .iter()
                .position(|(line_idx, _, _)| *line_idx >= self.scroll)
                .unwrap_or(0);
            self.scroll_to_current_match(viewport_height);
        }
    }

    /// Scroll so the current match is visible in the viewport.
    fn scroll_to_current_match(&mut self, viewport_height: usize) {
        if let Some(&(line_idx, _, _)) = self.match_positions.get(self.current_match) {
            let total = self.document.total_rows();
            let max_scroll = total.saturating_sub(viewport_height);
            if line_idx < self.scroll {
                // Match is above viewport — scroll up to show it.
                self.scroll = line_idx;
                self.follow_tail = false;
            } else if line_idx >= self.scroll + viewport_height {
                // Match is below viewport — scroll down.
                self.scroll = line_idx.saturating_sub(viewport_height / 2).min(max_scroll);
                self.follow_tail = self.scroll >= max_scroll;
            }
        }
    }

    pub fn apply_diagram_update(&mut self, finished: RenderFinished, viewport_height: usize) {
        let hash = finished.hash;
        let phase = finished.result;

        // Update index
        let mut row_height = 8;
        if let DiagramPhase::Ready { row_height: h, .. } = &phase {
            row_height = *h;
        }

        let source = self
            .diagrams
            .get(&hash)
            .map(|s| s.source.clone())
            .or_else(|| {
                self.document.blocks.iter().find_map(|b| match b {
                    DocBlock::Diagram(s) if s.hash == hash => Some(s.source.clone()),
                    _ => None,
                })
            })
            .unwrap_or_default();

        let updated_slot = Arc::new(DiagramSlot {
            hash: hash.clone(),
            source,
            phase,
            row_height,
        });

        self.diagrams
            .slots
            .insert(hash.clone(), updated_slot.clone());

        // Update current document slots
        for block in &mut self.document.blocks {
            if let DocBlock::Diagram(slot) = block {
                if slot.hash == hash {
                    *slot = (*updated_slot).clone();
                }
            }
        }

        if self.follow_tail {
            self.scroll_to_end(viewport_height);
        } else {
            self.clamp_scroll(viewport_height);
        }
    }

    pub fn diagram_index(&self) -> &DiagramIndex {
        &self.diagrams
    }

    pub fn set_mermaid_config(&mut self, cfg: MermaidRenderConfig) {
        // Auto-enable Kitty pixels when supported unless caller forced a value
        // via OCMUX_NO_KITTY_GRAPHICS (handled inside kitty_graphics_supported).
        self.protocol_enabled = cfg.protocol_enabled || kitty_graphics_supported();
        self.scheduler = Some(DiagramScheduler::new(cfg));
    }

    pub fn protocol_enabled(&self) -> bool {
        self.protocol_enabled
    }

    pub fn scheduler_tick(&mut self, viewport_height: usize, width: u16) {
        if let Some(ref mut sched) = self.scheduler {
            sched.tick(&self.document, self.scroll, viewport_height, width);
        }
    }

    pub fn poll_diagram_completions(&mut self) -> Vec<RenderFinished> {
        if let Some(ref mut sched) = self.scheduler {
            sched.poll_completions()
        } else {
            vec![]
        }
    }

    /// Owned paint records for Kitty (hash, png, dest rect, source crop).
    pub fn collect_kitty_paints(&self, area: Rect) -> Vec<KittyPaintOwned> {
        if !self.protocol_enabled {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut row = 0usize;
        for block in &self.document.blocks {
            match block {
                DocBlock::Text(lines) => row += lines.len(),
                DocBlock::Diagram(slot) => {
                    let start = row;
                    let height = slot.row_height;
                    if let DiagramPhase::Ready {
                        png,
                        png_w,
                        png_h,
                        col_width,
                        ..
                    } = &slot.phase
                    {
                        if let Some(vis) =
                            slot_visibility(start, height, self.scroll, area, *col_width)
                        {
                            let (sx, sy, sw, sh) = source_crop(
                                *png_w,
                                *png_h,
                                vis.full_rows,
                                vis.clip_top_rows,
                                vis.visible_rows,
                            );
                            out.push((
                                slot.hash.clone(),
                                png.as_ref().clone(),
                                vis.screen_rect,
                                sx,
                                sy,
                                sw,
                                sh,
                            ));
                        }
                    }
                    row += height;
                }
            }
        }
        out
    }

    pub fn paint_kitty_graphics(&mut self, out: &mut dyn Write, area: Rect) -> std::io::Result<()> {
        if !self.protocol_enabled {
            return Ok(());
        }
        let paints_owned = self.collect_kitty_paints(area);
        let paints: Vec<DiagramPaint<'_>> = paints_owned
            .iter()
            .map(|(hash, png, rect, sx, sy, sw, sh)| DiagramPaint {
                hash: hash.as_str(),
                png: png.as_slice(),
                rect: *rect,
                src_x: *sx,
                src_y: *sy,
                src_w: *sw,
                src_h: *sh,
            })
            .collect();
        self.kitty.paint_frame(out, &paints)
    }

    pub fn has_kitty_graphics(&self) -> bool {
        self.protocol_enabled && self.kitty.has_graphics()
    }

    pub fn clear_kitty_graphics(&mut self, out: &mut dyn Write) -> std::io::Result<()> {
        if !self.protocol_enabled {
            return Ok(());
        }
        self.kitty.clear_all(out)
    }
}
