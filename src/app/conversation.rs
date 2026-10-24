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
use crate::ui::conversation::document::{ConversationDocument, DocBlock, MessageRole};
use crate::ui::conversation::selection::{
    Anchor, DocPos, ScrollDir, Selection, anchor_at, edge_scroll, extract_text, hit_test,
    resolve_anchor, scroll_rows,
};
use ratatui::layout::Rect;
use std::io::Write;

/// Owned Kitty paint record: hash, png bytes, dest rect, source crop x/y/w/h.
type KittyPaintOwned = (String, Vec<u8>, Rect, u32, u32, u32, u32);

/// An in-progress mouse selection over the conversation.
struct DragState {
    selection: Selection,
    /// Content anchors for the endpoints, so a document rebuild mid-drag
    /// cannot silently move the selection onto different text.
    anchors: (Anchor, Anchor),
    /// Latest pointer position in terminal cells, for edge auto-scroll.
    pointer: Option<(u16, u16)>,
    last_tick: Option<Instant>,
    carry: f32,
    dragging: bool,
}

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
    /// Hash of the diagram shown in the lightbox overlay, if any.
    lightbox: Option<String>,
    /// Active mouse selection, if any.
    drag: Option<DragState>,
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
            lightbox: None,
            drag: None,
        }
    }
}

impl ConversationViewState {
    pub fn open(&mut self, session_id: String, session_title: String, return_focus: AppFocus) {
        self.session_id = Some(session_id);
        self.session_title = session_title;
        self.return_focus = return_focus;
        self.document = ConversationDocument::new();
        self.clear_diagram_state();
        self.scroll = 0;
        self.follow_tail = true;
        self.last_poll = None;
        self.load_error = None;
        self.search_query.clear();
        self.search_active = false;
        self.match_positions.clear();
        self.current_match = 0;
        self.drag = None;
    }

    pub fn close(&mut self) -> AppFocus {
        self.session_id = None;
        self.document = ConversationDocument::new();
        self.clear_diagram_state();
        self.scroll = 0;
        self.load_error = None;
        self.search_query.clear();
        self.search_active = false;
        self.match_positions.clear();
        self.current_match = 0;
        self.drag = None;
        // Kitty placements are cleared by the main loop via clear_kitty_graphics
        // when focus leaves conversation.
        self.return_focus
    }

    /// Drop in-memory mermaid slots and abandon in-flight renders.
    /// Disk cache under `~/.cache/ocmux/mermaid` is kept for reuse.
    fn clear_diagram_state(&mut self) {
        self.diagrams = DiagramIndex::default();
        if let Some(sched) = self.scheduler.as_mut() {
            sched.reset();
        }
        self.kitty = KittyImagePlacer::new();
        self.lightbox = None;
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
        if let Some(hash) = self.lightbox.as_deref() {
            let still_ready = self.document.blocks.iter().any(|b| match b {
                DocBlock::Diagram(s) if s.hash == hash => {
                    matches!(s.phase, DiagramPhase::Ready { .. })
                }
                _ => false,
            });
            if !still_ready {
                self.lightbox = None;
            }
        }
        if was_at_tail {
            self.scroll_to_end(viewport_height);
        } else if self.scroll >= self.document.total_rows() {
            self.scroll = self.document.total_rows().saturating_sub(1);
        }
        if !self.search_query.is_empty() {
            self.refresh_matches(viewport_height);
        }
        self.rebind_selection();
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

    /// Visible rows including their selection metadata, in display order.
    pub fn visible_rows(
        &self,
        viewport_height: usize,
    ) -> Vec<crate::ui::conversation::document::TextRow> {
        self.document
            .visible_rows(self.scroll, viewport_height, self.protocol_enabled)
    }

    pub fn scroll_offset(&self) -> usize {
        self.scroll
    }

    pub fn is_following_tail(&self) -> bool {
        self.follow_tail
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
    // Text selection
    // -----------------------------------------------------------------------

    /// Rect holding conversation text inside the main pane: `inner` minus the
    /// search bar row. Shared by rendering, hit-testing and edge scrolling.
    pub fn content_rect(&self, inner: Rect) -> Rect {
        if self.is_searching() || !self.search_query().is_empty() {
            let height = inner.height.saturating_sub(1);
            Rect::new(inner.x, inner.y, inner.width, height)
        } else {
            inner
        }
    }

    /// Document position under a mouse cell inside `content`.
    ///
    /// Positions outside the pane clamp to its nearest edge so a drag can be
    /// tracked while the pointer leaves the conversation.
    pub fn pos_for_mouse(&self, mouse_x: u16, mouse_y: u16, content: Rect) -> Option<DocPos> {
        if content.height == 0 {
            return None;
        }
        let rel_y = mouse_y
            .saturating_sub(content.y)
            .min(content.height.saturating_sub(1));
        let rel_x = mouse_x.saturating_sub(content.x);
        hit_test(&self.document, self.scroll + rel_y as usize, rel_x)
    }

    pub fn selection(&self) -> Option<Selection> {
        self.drag.as_ref().map(|d| d.selection)
    }

    pub fn is_dragging(&self) -> bool {
        self.drag.as_ref().is_some_and(|d| d.dragging)
    }

    pub fn has_selection(&self) -> bool {
        self.drag.is_some()
    }

    /// Start a selection at `pos`, which also disables tail-following.
    pub fn begin_selection(&mut self, pos: DocPos, pointer: (u16, u16)) {
        let Some(anchor) = anchor_at(&self.document, pos) else {
            self.drag = None;
            return;
        };
        self.follow_tail = false;
        self.drag = Some(DragState {
            selection: Selection::new(pos),
            anchors: (anchor.clone(), anchor),
            pointer: Some(pointer),
            last_tick: None,
            carry: 0.0,
            dragging: true,
        });
    }

    /// Move the selection head to `pos` and remember the pointer for scrolling.
    pub fn extend_selection(&mut self, pos: DocPos, pointer: (u16, u16)) {
        if let Some(drag) = self.drag.as_mut() {
            drag.selection.head = pos;
            drag.pointer = Some(pointer);
            if let Some(anchor) = anchor_at(&self.document, pos) {
                drag.anchors.1 = anchor;
            }
        }
    }

    /// Finish the drag and return the readable text to copy.
    ///
    /// A click with no movement produces nothing and drops the selection;
    /// otherwise the selection stays visible so it is clear what was copied.
    pub fn end_selection(&mut self) -> Option<String> {
        let drag = self.drag.as_mut()?;
        drag.dragging = false;
        drag.pointer = None;
        drag.carry = 0.0;
        let selection = drag.selection;
        if selection.is_empty() {
            self.drag = None;
            return None;
        }
        let text = extract_text(&self.document, selection);
        if text.is_empty() {
            self.drag = None;
            return None;
        }
        Some(text)
    }

    /// Drop any selection without copying.
    pub fn cancel_selection(&mut self) {
        self.drag = None;
    }

    /// Scroll the viewport when a drag sits at an edge, and extend the
    /// selection to match. Returns true when the selection changed.
    ///
    /// Driven by elapsed time, not mouse events, so it keeps running while the
    /// pointer is held still.
    pub fn tick_selection(&mut self, now: Instant, content: Rect, viewport_height: usize) -> bool {
        let Some(drag) = self.drag.as_mut() else {
            return false;
        };
        if !drag.dragging {
            return false;
        }
        let Some((px, py)) = drag.pointer else {
            return false;
        };

        let dt = now
            .duration_since(drag.last_tick.unwrap_or(now))
            .as_secs_f32()
            .min(0.25);
        drag.last_tick = Some(now);

        let Some((dir, speed)) = edge_scroll(py, content) else {
            drag.carry = 0.0;
            return false;
        };
        let moved = scroll_rows(&mut drag.carry, dt, speed, 20);
        if moved == 0 {
            return false;
        }

        let total = self.document.total_rows();
        let max_scroll = total.saturating_sub(viewport_height);
        match dir {
            ScrollDir::Up => self.scroll = self.scroll.saturating_sub(moved),
            ScrollDir::Down => self.scroll = (self.scroll + moved).min(max_scroll),
        }
        self.follow_tail = false;

        let Some(head) = self.pos_for_mouse(px, py, content) else {
            return true;
        };
        self.extend_selection(head, (px, py));
        true
    }

    /// Re-resolve selection endpoints against a rebuilt document. Cancels the
    /// selection when its content is gone rather than silently moving it.
    fn rebind_selection(&mut self) {
        let resolved = self.drag.as_ref().and_then(|d| {
            Some((
                resolve_anchor(&self.document, &d.anchors.0)?,
                resolve_anchor(&self.document, &d.anchors.1)?,
            ))
        });
        match resolved {
            Some((start, end)) => {
                if let Some(drag) = self.drag.as_mut() {
                    drag.selection = Selection::new(start).with_head(end);
                }
            }
            None => self.drag = None,
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
                DocBlock::Text(rows) => {
                    for (i, row) in rows.iter().enumerate() {
                        let line_idx = current_row + i;
                        let flat: String =
                            row.line.spans.iter().map(|s| s.content.as_ref()).collect();
                        let flat_lower = flat.to_lowercase();

                        let mut start = 0;
                        while let Some(pos) = flat_lower[start..].find(&query_lower) {
                            let byte_start = start + pos;
                            self.match_positions
                                .push((line_idx, byte_start, query_lower.len()));
                            start = byte_start + query_lower.len();
                        }
                    }
                    current_row += rows.len();
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
            self.ensure_row_visible(line_idx, viewport_height);
        }
    }

    /// Position the viewport so `row` is visible, only scrolling when it is
    /// currently outside the viewport.
    fn ensure_row_visible(&mut self, row: usize, viewport_height: usize) {
        let total = self.document.total_rows();
        let max_scroll = total.saturating_sub(viewport_height);
        if row < self.scroll {
            // Match is above viewport — scroll up to show it.
            self.scroll = row;
            self.follow_tail = false;
        } else if row >= self.scroll + viewport_height {
            // Match is below viewport — scroll down.
            self.scroll = row.saturating_sub(viewport_height / 2).min(max_scroll);
            self.follow_tail = self.scroll >= max_scroll;
        }
    }

    /// Position the viewport so `row` lands at the top (clamped to the tail).
    fn jump_to_row(&mut self, row: usize, viewport_height: usize) {
        let total = self.document.total_rows();
        let max_scroll = total.saturating_sub(viewport_height);
        self.scroll = row.min(max_scroll);
        self.follow_tail = self.scroll >= max_scroll;
    }

    /// Jump to the next user message header at or after the current scroll.
    pub fn next_user_header(&mut self, viewport_height: usize) {
        if let Some(row) = self
            .document
            .header_rows(MessageRole::User)
            .into_iter()
            .find(|&r| r > self.scroll)
        {
            self.jump_to_row(row, viewport_height);
        }
    }

    /// Jump to the previous user message header before the current scroll.
    pub fn prev_user_header(&mut self, viewport_height: usize) {
        if let Some(row) = self
            .document
            .header_rows(MessageRole::User)
            .into_iter()
            .rev()
            .find(|&r| r < self.scroll)
        {
            self.jump_to_row(row, viewport_height);
        }
    }

    pub fn apply_diagram_update(&mut self, finished: RenderFinished, viewport_height: usize) {
        // Ignore late completions after leave/switch so RAM cannot rebuild.
        if !self.is_active() {
            return;
        }
        let hash = finished.hash;
        let phase = finished.result;

        if self.lightbox.as_deref() == Some(hash.as_str())
            && !matches!(&phase, DiagramPhase::Ready { .. })
        {
            self.lightbox = None;
        }

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
            if let DocBlock::Diagram(slot) = block
                && slot.hash == hash
            {
                *slot = (*updated_slot).clone();
            }
        }

        self.rebind_selection();

        if self.follow_tail {
            self.scroll_to_end(viewport_height);
        } else {
            self.clamp_scroll(viewport_height);
        }
    }

    pub fn diagram_index(&self) -> &DiagramIndex {
        &self.diagrams
    }

    pub fn is_lightbox(&self) -> bool {
        self.lightbox.is_some()
    }

    pub fn lightbox_hash(&self) -> Option<&str> {
        self.lightbox.as_deref()
    }

    pub fn open_lightbox(&mut self, viewport_height: usize) -> bool {
        let Some(info) = self
            .document
            .first_visible_ready_diagram(self.scroll, viewport_height)
        else {
            return false;
        };
        self.lightbox = Some(info.slot.hash.clone());
        true
    }

    pub fn close_lightbox(&mut self) {
        self.lightbox = None;
    }

    pub fn lightbox_halfblock_lines(&self, inner: Rect) -> Option<Vec<Line<'static>>> {
        let hash = self.lightbox.as_deref()?;
        let slot = self.document.blocks.iter().find_map(|b| match b {
            DocBlock::Diagram(s) if s.hash == hash => Some(s),
            _ => None,
        })?;
        let DiagramPhase::Ready {
            png, png_w, png_h, ..
        } = &slot.phase
        else {
            return None;
        };
        if png.is_empty() {
            return None;
        }
        let layout = crate::ui::conversation::diagram::layout::compute_layout(
            *png_w,
            *png_h,
            inner.width,
            inner.height as usize,
        );
        crate::ui::conversation::diagram::halfblock::png_to_halfblock_lines(png, &layout).ok()
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
                        && let Some(vis) =
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

    pub fn paint_lightbox_graphics(
        &mut self,
        out: &mut dyn Write,
        inner: Rect,
    ) -> std::io::Result<()> {
        if !self.protocol_enabled {
            return Ok(());
        }
        let Some(hash) = self.lightbox.as_deref() else {
            return Ok(());
        };
        let Some(slot) = self.document.blocks.iter().find_map(|b| match b {
            DocBlock::Diagram(s) if s.hash == hash => Some(s),
            _ => None,
        }) else {
            return Ok(());
        };
        let DiagramPhase::Ready {
            png, png_w, png_h, ..
        } = &slot.phase
        else {
            return Ok(());
        };

        let layout = crate::ui::conversation::diagram::layout::compute_layout(
            *png_w,
            *png_h,
            inner.width,
            inner.height as usize,
        );
        let dest = Rect::new(
            inner.x,
            inner.y,
            layout.col_width.min(inner.width as usize) as u16,
            layout.row_height.min(inner.height as usize) as u16,
        );

        let (sx, sy, sw, sh) = (0, 0, *png_w, *png_h);
        let paints = vec![DiagramPaint {
            hash,
            png: png.as_slice(),
            rect: dest,
            src_x: sx,
            src_y: sy,
            src_w: sw,
            src_h: sh,
        }];
        self.kitty.paint_frame(out, &paints)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::text::Line;

    fn ready_slot(hash: &str, rows: usize) -> DiagramSlot {
        DiagramSlot {
            hash: hash.to_string(),
            source: "graph TD; A-->B".into(),
            phase: DiagramPhase::Ready {
                png: Arc::new(Vec::new()),
                png_w: 100,
                png_h: 100,
                width: 80,
                halfblocks: vec![],
                row_height: rows,
                col_width: 40,
            },
            row_height: rows,
        }
    }

    fn conv_with_ready_diagram() -> ConversationViewState {
        let mut conv = ConversationViewState::default();
        conv.open("s".into(), "t".into(), AppFocus::Sidebar);
        let mut doc = ConversationDocument::new();
        doc.push_lines(vec![Line::from("hi")]);
        doc.push_diagram(ready_slot("d1", 8));
        conv.replace_document(doc, 20);
        conv
    }

    #[test]
    fn open_lightbox_succeeds_when_ready_diagram_in_view() {
        let mut conv = conv_with_ready_diagram();
        assert!(conv.open_lightbox(20));
        assert!(conv.is_lightbox());
        assert_eq!(conv.lightbox_hash(), Some("d1"));
    }

    #[test]
    fn open_lightbox_fails_when_no_ready_diagram_in_view() {
        let mut conv = ConversationViewState::default();
        conv.open("s".into(), "t".into(), AppFocus::Sidebar);
        let mut doc = ConversationDocument::new();
        doc.push_lines(vec![Line::from("only text")]);
        conv.replace_document(doc, 20);
        assert!(!conv.open_lightbox(20));
        assert!(!conv.is_lightbox());
    }

    #[test]
    fn close_lightbox_clears_mode() {
        let mut conv = conv_with_ready_diagram();
        conv.open_lightbox(20);
        conv.close_lightbox();
        assert!(!conv.is_lightbox());
    }

    #[test]
    fn replace_document_closes_lightbox_if_hash_gone() {
        let mut conv = conv_with_ready_diagram();
        conv.open_lightbox(20);
        let mut doc = ConversationDocument::new();
        doc.push_lines(vec![Line::from("rebuilt")]);
        conv.replace_document(doc, 20);
        assert!(!conv.is_lightbox());
    }

    #[test]
    fn replace_document_keeps_lightbox_if_hash_still_ready() {
        let mut conv = conv_with_ready_diagram();
        conv.open_lightbox(20);
        let mut doc = ConversationDocument::new();
        doc.push_diagram(ready_slot("d1", 8));
        conv.replace_document(doc, 20);
        assert!(conv.is_lightbox());
    }

    #[test]
    fn close_and_open_session_clears_lightbox() {
        let mut conv = conv_with_ready_diagram();
        conv.open_lightbox(20);
        conv.close();
        assert!(!conv.is_lightbox());
    }

    use crate::ui::conversation::document::TextRow;
    use std::time::Duration;

    fn body(text: &str, continuation: bool) -> TextRow {
        TextRow::body(
            Line::from(text.to_string()),
            text.to_string(),
            0,
            continuation,
        )
    }

    /// A conversation whose single message renders as `rows`.
    fn conv_with_rows(rows: Vec<TextRow>) -> ConversationViewState {
        let mut conv = ConversationViewState::default();
        conv.open("s".into(), "t".into(), AppFocus::Sidebar);
        let mut doc = ConversationDocument::new();
        let mut off = 0;
        doc.push_text_rows(rows, "m1", &mut off);
        conv.replace_document(doc, 20);
        conv
    }

    #[test]
    fn drag_copies_readable_text_and_keeps_the_selection_visible() {
        let mut conv = conv_with_rows(vec![body("hello ", false), body("world", true)]);
        conv.begin_selection(DocPos { row: 0, col: 0 }, (0, 0));
        assert!(conv.is_dragging());
        conv.extend_selection(DocPos { row: 1, col: 5 }, (0, 1));

        assert_eq!(conv.end_selection().as_deref(), Some("hello world"));
        assert!(!conv.is_dragging(), "drag ends on release");
        assert!(conv.has_selection(), "copied range stays highlighted");
    }

    #[test]
    fn click_without_dragging_copies_nothing() {
        let mut conv = conv_with_rows(vec![body("hello", false)]);
        conv.begin_selection(DocPos { row: 0, col: 0 }, (0, 0));
        assert_eq!(conv.end_selection(), None);
        assert!(!conv.has_selection());
    }

    #[test]
    fn cancel_selection_drops_it_without_copying() {
        let mut conv = conv_with_rows(vec![body("hello", false)]);
        conv.begin_selection(DocPos { row: 0, col: 0 }, (0, 0));
        conv.extend_selection(DocPos { row: 0, col: 5 }, (0, 0));
        conv.cancel_selection();
        assert!(!conv.has_selection());
    }

    #[test]
    fn selection_still_copies_the_same_text_after_a_rebuild() {
        let mut conv = conv_with_rows(vec![body("alpha ", false), body("beta", true)]);
        conv.begin_selection(DocPos { row: 0, col: 0 }, (0, 0));
        conv.extend_selection(DocPos { row: 1, col: 4 }, (0, 1));

        // A poll rebuilds the document; the endpoints must not drift.
        let mut doc = ConversationDocument::new();
        let mut off = 0;
        doc.push_text_rows(
            vec![body("alpha ", false), body("beta", true)],
            "m1",
            &mut off,
        );
        doc.push_text_rows(vec![body("later", false)], "m2", &mut off);
        conv.replace_document(doc, 20);

        assert_eq!(conv.end_selection().as_deref(), Some("alpha beta"));
    }

    #[test]
    fn selection_is_dropped_when_its_content_disappears() {
        let mut conv = conv_with_rows(vec![body("alpha", false)]);
        conv.begin_selection(DocPos { row: 0, col: 0 }, (0, 0));
        conv.extend_selection(DocPos { row: 0, col: 5 }, (0, 0));

        let mut doc = ConversationDocument::new();
        let mut off = 0;
        doc.push_text_rows(vec![body("other", false)], "m9", &mut off);
        conv.replace_document(doc, 20);
        assert!(!conv.has_selection());
    }

    #[test]
    fn drag_below_the_viewport_auto_scrolls_and_keeps_extending() {
        let mut doc = ConversationDocument::new();
        let mut off = 0;
        doc.push_text_rows(
            (0..30).map(|_| body("line", false)).collect::<Vec<_>>(),
            "m1",
            &mut off,
        );
        let mut conv = ConversationViewState::default();
        conv.open("s".into(), "t".into(), AppFocus::Sidebar);
        conv.replace_document(doc, 5);
        conv.scroll_to_top();

        let content = Rect::new(0, 0, 40, 5);
        // Pointer held below the pane: nothing moves until the tick fires.
        conv.begin_selection(DocPos { row: 0, col: 0 }, (0, 9));
        let start = Instant::now();
        conv.tick_selection(start, content, 5);
        assert_eq!(conv.scroll_offset(), 0, "first tick only arms the clock");

        conv.tick_selection(start + Duration::from_secs(1), content, 5);
        assert!(conv.scroll_offset() > 0, "edge drag scrolls the viewport");
        assert!(
            conv.selection().is_some_and(|s| s.head.row > 0),
            "selection extends as the document moves under the pointer"
        );
    }

    #[test]
    fn drag_in_the_middle_of_the_viewport_does_not_scroll() {
        let mut doc = ConversationDocument::new();
        let mut off = 0;
        doc.push_text_rows(
            (0..30).map(|_| body("line", false)).collect::<Vec<_>>(),
            "m1",
            &mut off,
        );
        let mut conv = ConversationViewState::default();
        conv.open("s".into(), "t".into(), AppFocus::Sidebar);
        conv.replace_document(doc, 5);
        conv.scroll_to_top();

        let content = Rect::new(0, 0, 40, 10);
        conv.begin_selection(DocPos { row: 0, col: 0 }, (0, 5));
        let start = Instant::now();
        conv.tick_selection(start, content, 5);
        conv.tick_selection(start + Duration::from_secs(1), content, 5);
        assert_eq!(conv.scroll_offset(), 0);
    }
}
