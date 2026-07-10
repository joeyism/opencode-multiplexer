use std::collections::HashMap;

use ratatui::text::Line;

use crate::app::focus::AppFocus;

/// Metadata for a single rendered diff line, mapping it back to source location.
#[derive(Clone, Debug)]
pub struct LineMeta {
    pub filepath: String,
    pub new_line_no: Option<usize>,
    pub old_line_no: Option<usize>,
}

pub struct DiffViewState {
    session_id: Option<String>,
    session_title: String,
    raw_diff: String,
    return_focus: AppFocus,
    document: Vec<Line<'static>>,
    metadata: Vec<Option<LineMeta>>,
    scroll: usize,
    cursor: usize,
    selection_anchor: Option<usize>,
    // Search state
    search_query: String,
    search_active: bool,
    match_positions: Vec<(usize, usize, usize)>, // (line_idx, byte_start, byte_len)
    current_match: usize,
    history: HashMap<String, (usize, usize)>,
}

impl Default for DiffViewState {
    fn default() -> Self {
        Self {
            session_id: None,
            session_title: String::new(),
            raw_diff: String::new(),
            return_focus: AppFocus::Sidebar,
            document: Vec::new(),
            metadata: Vec::new(),
            scroll: 0,
            cursor: 0,
            selection_anchor: None,
            search_query: String::new(),
            search_active: false,
            match_positions: Vec::new(),
            current_match: 0,
            history: HashMap::new(),
        }
    }
}

impl DiffViewState {
    pub fn open(
        &mut self,
        session_id: String,
        session_title: String,
        raw_diff: String,
        return_focus: AppFocus,
    ) {
        let (saved_scroll, saved_cursor) = self.history.get(&session_id).copied().unwrap_or((0, 0));

        self.session_id = Some(session_id);
        self.session_title = session_title;
        self.raw_diff = raw_diff;
        self.return_focus = return_focus;
        self.document.clear();
        self.metadata.clear();
        self.scroll = saved_scroll;
        self.cursor = saved_cursor;
        self.selection_anchor = None;
        self.search_query.clear();
        self.search_active = false;
        self.match_positions.clear();
        self.current_match = 0;
    }

    pub fn close(&mut self) -> AppFocus {
        if let Some(ref sid) = self.session_id {
            if self.history.len() > 50
                && let Some(first_key) = self.history.keys().next().cloned()
            {
                self.history.remove(&first_key);
            }
            self.history.insert(sid.clone(), (self.scroll, self.cursor));
        }

        self.session_id = None;
        self.raw_diff.clear();
        self.document.clear();
        self.metadata.clear();
        self.scroll = 0;
        self.cursor = 0;
        self.selection_anchor = None;
        self.search_query.clear();
        self.search_active = false;
        self.match_positions.clear();
        self.current_match = 0;
        self.return_focus
    }

    pub fn is_active(&self) -> bool {
        self.session_id.is_some()
    }

    pub fn raw_diff(&self) -> &str {
        &self.raw_diff
    }

    pub fn replace_document(
        &mut self,
        lines: Vec<Line<'static>>,
        meta: Vec<Option<LineMeta>>,
        viewport_height: usize,
    ) {
        self.document = lines;
        self.metadata = meta;
        let max_idx = self.document.len().saturating_sub(1);
        if self.cursor > max_idx {
            self.cursor = max_idx;
        }
        if let Some(ref mut anchor) = self.selection_anchor
            && *anchor > max_idx
        {
            *anchor = max_idx;
        }
        let max_scroll = self.document.len().saturating_sub(viewport_height);
        if self.scroll > max_scroll {
            self.scroll = max_scroll;
        }
    }

    pub fn visible_lines(&self, viewport_height: usize) -> Vec<Line<'static>> {
        let end = (self.scroll + viewport_height).min(self.document.len());
        self.document[self.scroll..end].to_vec()
    }

    pub fn scroll_up(&mut self, amount: usize) {
        self.scroll = self.scroll.saturating_sub(amount);
    }

    pub fn scroll_down(&mut self, amount: usize, viewport_height: usize) {
        let max_scroll = self.document.len().saturating_sub(viewport_height);
        self.scroll = (self.scroll + amount).min(max_scroll);
    }

    pub fn scroll_to_top(&mut self) {
        self.scroll = 0;
    }

    pub fn scroll_to_end(&mut self, viewport_height: usize) {
        let max_scroll = self.document.len().saturating_sub(viewport_height);
        self.scroll = max_scroll;
    }

    pub fn clamp_scroll(&mut self, viewport_height: usize) {
        let max_scroll = self.document.len().saturating_sub(viewport_height);
        if self.scroll > max_scroll {
            self.scroll = max_scroll;
        }
    }

    pub fn scroll_offset(&self) -> usize {
        self.scroll
    }

    /// Scroll the viewport up by `amount` lines (content moves down).
    /// If the cursor falls below the viewport, it is moved up to stay visible.
    pub fn scroll_view_up(&mut self, amount: usize, vp: usize) {
        self.scroll = self.scroll.saturating_sub(amount);
        let max_visible = self.scroll + vp.saturating_sub(1);
        if self.cursor > max_visible {
            self.cursor = max_visible;
        }
    }

    /// Scroll the viewport down by `amount` lines (content moves up).
    /// If the cursor falls above the viewport, it is moved down to stay visible.
    pub fn scroll_view_down(&mut self, amount: usize, vp: usize) {
        let max_scroll = self.document.len().saturating_sub(vp);
        self.scroll = (self.scroll + amount).min(max_scroll);
        if self.cursor < self.scroll {
            self.cursor = self.scroll;
        }
    }

    // -----------------------------------------------------------------------
    // Cursor & Visual Selection
    // -----------------------------------------------------------------------

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Move cursor up by `amount` lines, keeping it in bounds and visible.
    pub fn move_cursor_up(&mut self, amount: usize, _vp: usize) {
        self.cursor = self.cursor.saturating_sub(amount);
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        }
    }

    /// Move cursor down by `amount` lines, keeping it in bounds and visible.
    pub fn move_cursor_down(&mut self, amount: usize, vp: usize) {
        let max_idx = self.document.len().saturating_sub(1);
        self.cursor = (self.cursor + amount).min(max_idx);
        if self.cursor >= self.scroll + vp {
            let max_scroll = self.document.len().saturating_sub(vp);
            self.scroll = (self.cursor.saturating_sub(vp) + 1).min(max_scroll);
        }
    }

    /// Move cursor to the top of the document.
    pub fn move_cursor_to_top(&mut self, _vp: usize) {
        self.cursor = 0;
        self.scroll = 0;
    }

    /// Move cursor to the end of the document.
    pub fn move_cursor_to_end(&mut self, vp: usize) {
        let max_idx = self.document.len().saturating_sub(1);
        self.cursor = max_idx;
        let max_scroll = self.document.len().saturating_sub(vp);
        self.scroll = max_scroll;
    }

    /// Toggle visual selection mode.
    pub fn toggle_visual(&mut self) {
        if self.selection_anchor.is_some() {
            self.selection_anchor = None;
        } else {
            self.selection_anchor = Some(self.cursor);
        }
    }

    /// Cancel visual selection mode without clearing the cursor.
    pub fn cancel_visual(&mut self) {
        self.selection_anchor = None;
    }

    pub fn is_visual(&self) -> bool {
        self.selection_anchor.is_some()
    }

    /// Returns `(start, end)` inclusive range of the selection, if active.
    pub fn selection_range(&self) -> Option<(usize, usize)> {
        self.selection_anchor.map(|anchor| {
            let lo = anchor.min(self.cursor);
            let hi = anchor.max(self.cursor);
            (lo, hi)
        })
    }

    pub fn metadata(&self) -> &[Option<LineMeta>] {
        &self.metadata
    }

    /// Format the current selection as a paste-able string.
    /// Returns `None` if no selection is active or no valid lines are found.
    pub fn format_selection(&self) -> Option<String> {
        let (start, end) = self.selection_range()?;
        let mut files: Vec<(String, usize, usize)> = Vec::new();

        for entry in self.metadata.iter().take(end + 1).skip(start) {
            let Some(LineMeta {
                filepath,
                new_line_no,
                old_line_no: _,
            }) = entry
            else {
                continue;
            };
            if filepath == "/dev/null" {
                continue;
            }
            let Some(ln) = new_line_no else {
                continue;
            };
            if let Some((_, existing_min, existing_max)) =
                files.iter_mut().find(|(f, _, _)| f == filepath)
            {
                *existing_min = (*existing_min).min(*ln);
                *existing_max = (*existing_max).max(*ln);
            } else {
                files.push((filepath.clone(), *ln, *ln));
            }
        }

        if files.is_empty() {
            return None;
        }

        let parts: Vec<String> = files
            .into_iter()
            .map(|(path, lo, hi)| {
                if lo == hi {
                    format!("{path}:{lo}")
                } else {
                    format!("{path}:{lo}-{hi}")
                }
            })
            .collect();

        Some(parts.join(" "))
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
    /// Scrolls to the first match (or nearest match to current scroll).
    fn refresh_matches(&mut self, viewport_height: usize) {
        self.match_positions.clear();
        self.current_match = 0;

        if self.search_query.is_empty() {
            return;
        }

        let query_lower = self.search_query.to_lowercase();

        for (line_idx, line) in self.document.iter().enumerate() {
            // Flatten all span text into a single string for searching.
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
            let max_scroll = self.document.len().saturating_sub(viewport_height);
            if line_idx < self.scroll {
                // Match is above viewport — scroll up to show it.
                self.scroll = line_idx;
            } else if line_idx >= self.scroll + viewport_height {
                // Match is below viewport — scroll down.
                self.scroll = line_idx.saturating_sub(viewport_height / 2).min(max_scroll);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::field_reassign_with_default)]
    use super::*;
    use ratatui::text::Span;

    fn make_document(texts: &[&str]) -> Vec<Line<'static>> {
        texts
            .iter()
            .map(|t| Line::from(Span::raw(t.to_string())))
            .collect()
    }

    #[test]
    fn search_finds_matches_in_document() {
        let mut state = DiffViewState::default();
        state.session_id = Some("test".into());
        state.document = make_document(&["hello world", "foo bar", "hello again", "nothing here"]);
        state.start_search();
        state.search_insert('h', 100);
        state.search_insert('e', 100);
        state.search_insert('l', 100);
        state.search_insert('l', 100);
        state.search_insert('o', 100);

        assert_eq!(state.match_positions.len(), 2);
        assert_eq!(state.match_positions[0], (0, 0, 5)); // "hello" in line 0
        assert_eq!(state.match_positions[1], (2, 0, 5)); // "hello" in line 2
    }

    #[test]
    fn search_is_case_insensitive() {
        let mut state = DiffViewState::default();
        state.session_id = Some("test".into());
        state.document = make_document(&["Hello World", "HELLO again"]);
        state.start_search();
        state.search_insert('h', 100);
        state.search_insert('e', 100);
        state.search_insert('l', 100);
        state.search_insert('l', 100);
        state.search_insert('o', 100);

        assert_eq!(state.match_positions.len(), 2);
    }

    #[test]
    fn next_match_wraps_around() {
        let mut state = DiffViewState::default();
        state.session_id = Some("test".into());
        state.document = make_document(&["aaa", "bbb", "aaa"]);
        state.start_search();
        state.search_insert('a', 100);
        state.search_insert('a', 100);
        state.search_insert('a', 100);

        assert_eq!(state.match_positions.len(), 2);
        assert_eq!(state.current_match, 0);

        state.next_match(100);
        assert_eq!(state.current_match, 1);

        state.next_match(100);
        assert_eq!(state.current_match, 0); // wrapped
    }

    #[test]
    fn prev_match_wraps_around() {
        let mut state = DiffViewState::default();
        state.session_id = Some("test".into());
        state.document = make_document(&["aaa", "bbb", "aaa"]);
        state.start_search();
        state.search_insert('a', 100);
        state.search_insert('a', 100);
        state.search_insert('a', 100);

        assert_eq!(state.current_match, 0);

        state.prev_match(100);
        assert_eq!(state.current_match, 1); // wrapped to last

        state.prev_match(100);
        assert_eq!(state.current_match, 0);
    }

    #[test]
    fn cancel_search_clears_query() {
        let mut state = DiffViewState::default();
        state.session_id = Some("test".into());
        state.document = make_document(&["hello"]);
        state.start_search();
        state.search_insert('h', 100);
        assert!(!state.match_positions.is_empty());

        state.cancel_search();
        assert!(!state.search_active);
        assert!(state.search_query.is_empty());
        assert!(state.match_positions.is_empty());
    }

    #[test]
    fn search_scrolls_to_first_match() {
        let mut state = DiffViewState::default();
        state.session_id = Some("test".into());
        // 100 lines, match only on line 50.
        let mut texts: Vec<&str> = vec!["no match"; 50];
        texts.push("found it");
        texts.extend(vec!["no match"; 49]);
        state.document = make_document(&texts);
        state.scroll = 0;

        state.start_search();
        // Type "found"
        for ch in "found".chars() {
            state.search_insert(ch, 20);
        }

        assert_eq!(state.match_positions.len(), 1);
        assert_eq!(state.match_positions[0].0, 50); // line 50
        // Scroll should have moved to make line 50 visible.
        assert!(state.scroll <= 50);
        assert!(state.scroll + 20 > 50);
    }

    // -----------------------------------------------------------------------
    // Cursor & visual selection tests
    // -----------------------------------------------------------------------

    #[test]
    fn cursor_clamps_on_shrink() {
        let mut state = DiffViewState::default();
        state.session_id = Some("test".into());
        state.document = make_document(&["a", "b", "c", "d", "e"]);
        state.cursor = 4;
        state.replace_document(make_document(&["a"]), vec![], 10);
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn selection_anchor_clamps_on_shrink() {
        let mut state = DiffViewState::default();
        state.session_id = Some("test".into());
        state.document = make_document(&["a", "b", "c"]);
        state.selection_anchor = Some(2);
        state.replace_document(make_document(&["a"]), vec![], 10);
        assert_eq!(state.selection_anchor, Some(0));
    }

    #[test]
    fn toggle_visual_sets_and_clears_anchor() {
        let mut state = DiffViewState::default();
        state.session_id = Some("test".into());
        state.document = make_document(&["a", "b"]);
        state.cursor = 1;

        assert!(!state.is_visual());
        state.toggle_visual();
        assert!(state.is_visual());
        assert_eq!(state.selection_anchor, Some(1));

        state.toggle_visual();
        assert!(!state.is_visual());
        assert!(state.selection_anchor.is_none());
    }

    #[test]
    fn selection_range_returns_correct_bounds() {
        let mut state = DiffViewState::default();
        state.session_id = Some("test".into());
        state.document = make_document(&["a", "b", "c", "d"]);
        state.cursor = 3;
        state.selection_anchor = Some(1);
        assert_eq!(state.selection_range(), Some((1, 3)));

        // reverse anchor/cursor
        state.cursor = 0;
        state.selection_anchor = Some(3);
        assert_eq!(state.selection_range(), Some((0, 3)));
    }

    #[test]
    fn format_selection_skips_none_and_deleted() {
        let mut state = DiffViewState::default();
        state.session_id = Some("test".into());
        state.document = make_document(&["a", "b", "c"]);
        state.metadata = vec![
            Some(LineMeta {
                filepath: "foo.rs".into(),
                new_line_no: Some(10),
                old_line_no: Some(5),
            }),
            None,
            Some(LineMeta {
                filepath: "/dev/null".into(),
                new_line_no: Some(99),
                old_line_no: Some(50),
            }),
        ];
        state.selection_anchor = Some(0);
        state.cursor = 2;

        let result = state.format_selection();
        assert_eq!(result, Some("foo.rs:10".to_string()));
    }

    #[test]
    fn format_selection_groups_by_file() {
        let mut state = DiffViewState::default();
        state.session_id = Some("test".into());
        state.document = make_document(&["a", "b", "c", "d"]);
        state.metadata = vec![
            Some(LineMeta {
                filepath: "bar.rs".into(),
                new_line_no: Some(5),
                old_line_no: None,
            }),
            Some(LineMeta {
                filepath: "bar.rs".into(),
                new_line_no: Some(20),
                old_line_no: None,
            }),
            Some(LineMeta {
                filepath: "foo.rs".into(),
                new_line_no: Some(42),
                old_line_no: None,
            }),
            Some(LineMeta {
                filepath: "foo.rs".into(),
                new_line_no: Some(58),
                old_line_no: None,
            }),
        ];
        state.selection_anchor = Some(0);
        state.cursor = 3;

        let result = state.format_selection();
        assert_eq!(result, Some("bar.rs:5-20 foo.rs:42-58".to_string()));
    }

    #[test]
    fn format_selection_returns_none_when_no_valid_lines() {
        let mut state = DiffViewState::default();
        state.session_id = Some("test".into());
        state.document = make_document(&["a", "b"]);
        state.metadata = vec![None, None];
        state.selection_anchor = Some(0);
        state.cursor = 1;

        assert!(state.format_selection().is_none());
    }
}

#[cfg(test)]
mod tests_scroll_view {
    use super::*;
    use ratatui::text::Span;

    fn make_document(texts: &[&str]) -> Vec<Line<'static>> {
        texts
            .iter()
            .map(|t| Line::from(Span::raw(t.to_string())))
            .collect()
    }

    #[test]
    fn scroll_view_down_clamps_cursor() {
        let mut state = DiffViewState {
            document: make_document(&["1", "2", "3", "4", "5", "6", "7", "8", "9", "10"]),
            ..Default::default()
        };

        // Scroll down by 1 (content moves up). Viewport is 5 lines.
        // Visible lines: 1..5. Cursor is at 0.
        // After scroll: Visible lines: 2..6 (scroll=1). Cursor should be clamped to 1.
        state.scroll_view_down(1, 5);
        assert_eq!(state.scroll, 1);
        assert_eq!(state.cursor, 1);
    }

    #[test]
    fn scroll_view_up_clamps_cursor() {
        let mut state = DiffViewState {
            document: make_document(&["1", "2", "3", "4", "5", "6", "7", "8", "9", "10"]),
            scroll: 5,
            cursor: 9,
            ..Default::default()
        };

        // Scroll up by 1 (content moves down). Viewport is 5 lines.
        // Visible lines: 5..9. Cursor is at 9.
        // After scroll: Visible lines: 4..8 (scroll=4). Cursor should be clamped to 8.
        state.scroll_view_up(1, 5);
        assert_eq!(state.scroll, 4);
        assert_eq!(state.cursor, 8);
    }

    #[test]
    fn open_restores_from_history() {
        let mut state = DiffViewState::default();
        let sid = "session1".to_string();

        // Open, move cursor/scroll, and close
        state.open(
            sid.clone(),
            "Title".to_string(),
            "Diff".to_string(),
            AppFocus::Sidebar,
        );
        state.scroll = 10;
        state.cursor = 15;
        state.close();

        // Re-open same session
        state.open(
            sid.clone(),
            "Title".to_string(),
            "Diff".to_string(),
            AppFocus::Sidebar,
        );
        assert_eq!(state.scroll, 10);
        assert_eq!(state.cursor, 15);

        // Open different session
        state.open(
            "session2".to_string(),
            "Title 2".to_string(),
            "Diff 2".to_string(),
            AppFocus::Sidebar,
        );
        assert_eq!(state.scroll, 0);
        assert_eq!(state.cursor, 0);
    }
}
