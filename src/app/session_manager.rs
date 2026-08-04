use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashSet;
use std::sync::Arc;

use nucleo::{
    Config, Nucleo,
    pattern::{CaseMatching, Normalization},
};

use crate::data::db::{models::DbManagedSession, reader::DbReader};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManagerCommand {
    Insert(char),
    Backspace,
    Up,
    Down,
    Toggle,
    SelectAll,
    Clear,
    RequestDelete,
    Close,
    CancelPending,
    ConfirmDelete,
    Nop,
}

pub fn manager_key_to_command(
    key: KeyEvent,
    has_pending: bool,
    has_selection: bool,
) -> ManagerCommand {
    if has_pending {
        return match key.code {
            KeyCode::Char('y') => ManagerCommand::ConfirmDelete,
            KeyCode::Char('n') | KeyCode::Esc => ManagerCommand::CancelPending,
            _ => ManagerCommand::Nop,
        };
    }

    match key.code {
        KeyCode::Esc => {
            if has_selection {
                ManagerCommand::Clear
            } else {
                ManagerCommand::Close
            }
        }
        KeyCode::Up => ManagerCommand::Up,
        KeyCode::Down => ManagerCommand::Down,
        KeyCode::Tab => ManagerCommand::Toggle,
        KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            ManagerCommand::SelectAll
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            ManagerCommand::Clear
        }
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            ManagerCommand::RequestDelete
        }
        KeyCode::Delete => ManagerCommand::RequestDelete,
        KeyCode::Backspace => ManagerCommand::Backspace,
        KeyCode::Char(c) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            ManagerCommand::Insert(c)
        }
        _ => ManagerCommand::Nop,
    }
}

pub type VisibleEntry = (
    SessionManagerEntry,
    Vec<u32>,
    Vec<u32>,
    Vec<u32>,
    bool,
    bool,
);

#[derive(Debug, Clone)]
pub struct SessionManagerEntry {
    pub session_id: String,
    pub repo: String,
    pub title: String,
    pub directory: String,
    pub user_message_count: i64,
    pub time_updated: i64,
}

#[derive(Debug, Clone)]
pub struct PendingDelete {
    pub session_ids: Vec<String>,
    pub user_message_count: i64,
}

pub struct SessionManagerState {
    pub query: String,
    pub selected: usize,
    pub scroll_offset: usize,
    pub live_session_ids: HashSet<String>,
    pub selected_ids: HashSet<String>,
    pub pending_delete: Option<PendingDelete>,
    entries: Vec<SessionManagerEntry>,
    matcher: Nucleo<usize>,
}

impl SessionManagerState {
    pub fn load(live_ids: HashSet<String>) -> anyhow::Result<Self> {
        let reader = DbReader::open_default()?;
        let managed = reader.list_sessions_for_manager()?;
        Ok(Self::from_entries(managed, live_ids))
    }

    pub fn from_entries(managed: Vec<DbManagedSession>, live_ids: HashSet<String>) -> Self {
        let entries: Vec<SessionManagerEntry> = managed
            .into_iter()
            .map(|s| SessionManagerEntry {
                session_id: s.id,
                repo: s
                    .worktree
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("?")
                    .to_string(),
                title: s.title,
                directory: s.directory.to_string_lossy().to_string(),
                user_message_count: s.user_message_count,
                time_updated: s.time_updated,
            })
            .collect();

        let mut matcher = Nucleo::new(Config::DEFAULT, Arc::new(|| {}), Some(1), 1);
        let injector = matcher.injector();
        for (idx, entry) in entries.iter().enumerate() {
            let search_text = format!("{} {} {}", entry.repo, entry.title, entry.directory);
            let _ = injector.push(idx, |_, dst| {
                dst[0] = search_text.into();
            });
        }
        matcher.tick(10);

        Self {
            query: String::new(),
            selected: 0,
            scroll_offset: 0,
            live_session_ids: live_ids,
            selected_ids: HashSet::new(),
            pending_delete: None,
            entries,
            matcher,
        }
    }

    pub fn tick(&mut self) {
        self.matcher.tick(10);
    }

    pub fn insert_char(&mut self, ch: char) {
        self.query.push(ch);
        self.refresh_pattern();
        self.selected = 0;
        self.scroll_offset = 0;
    }

    pub fn backspace(&mut self) {
        self.query.pop();
        self.refresh_pattern();
        self.selected = 0;
        self.scroll_offset = 0;
    }

    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn move_down(&mut self) {
        let count = self.matched_count();
        if count > 0 && self.selected < count - 1 {
            self.selected += 1;
        }
    }

    pub fn matched_count(&self) -> usize {
        self.matcher.snapshot().matched_item_count() as usize
    }

    pub fn total_count(&self) -> usize {
        self.entries.len()
    }

    pub fn toggle_select(&mut self) {
        if let Some(id) = self.current_entry().map(|e| e.session_id.clone())
            && !self.selected_ids.remove(&id)
        {
            self.selected_ids.insert(id);
        }
    }

    pub fn select_all_matched(&mut self) {
        let snapshot = self.matcher.snapshot();
        let count = snapshot.matched_item_count() as usize;
        for item in snapshot.matched_items(0..count as u32) {
            let entry_idx = *item.data;
            if let Some(entry) = self.entries.get(entry_idx) {
                self.selected_ids.insert(entry.session_id.clone());
            }
        }
    }

    pub fn clear_selection(&mut self) {
        self.selected_ids.clear();
    }

    pub fn request_delete(&mut self) {
        let targets = if self.selected_ids.is_empty() {
            self.current_entry()
                .map(|e| vec![e.session_id.clone()])
                .unwrap_or_default()
        } else {
            let mut ids: Vec<String> = self.selected_ids.iter().cloned().collect();
            ids.sort();
            ids
        };

        if targets.is_empty() {
            return;
        }

        let total_msgs = self
            .entries
            .iter()
            .filter(|e| targets.contains(&e.session_id))
            .map(|e| e.user_message_count)
            .sum();

        self.pending_delete = Some(PendingDelete {
            session_ids: targets,
            user_message_count: total_msgs,
        });
    }

    pub fn cancel_pending(&mut self) {
        self.pending_delete = None;
    }

    pub fn apply_local_removal(&mut self, deleted_ids: &[String]) {
        let deleted_set: HashSet<_> = deleted_ids.iter().collect();
        self.entries
            .retain(|e| !deleted_set.contains(&e.session_id));
        self.selected_ids.retain(|id| !deleted_set.contains(id));

        // Rebuild matcher
        let mut matcher = Nucleo::new(Config::DEFAULT, Arc::new(|| {}), Some(1), 1);
        let injector = matcher.injector();
        for (idx, entry) in self.entries.iter().enumerate() {
            let search_text = format!("{} {} {}", entry.repo, entry.title, entry.directory);
            let _ = injector.push(idx, |_, dst| {
                dst[0] = search_text.into();
            });
        }
        matcher.tick(10);
        self.matcher = matcher;
        self.refresh_pattern();

        self.selected = self.selected.min(self.matched_count().saturating_sub(1));
    }

    fn current_entry(&self) -> Option<&SessionManagerEntry> {
        let snapshot = self.matcher.snapshot();
        let count = snapshot.matched_item_count();
        if count == 0 {
            return None;
        }
        let sel = self.selected.min(count as usize - 1);
        let sorted = self.sorted_match_indices();
        let idx = *sorted.get(sel)?;
        self.entries.get(idx)
    }

    fn sorted_match_indices(&self) -> Vec<usize> {
        let snapshot = self.matcher.snapshot();
        let count = snapshot.matched_item_count() as usize;
        if count == 0 {
            return Vec::new();
        }

        let pattern = snapshot.pattern().column_pattern(0);
        let mut scorer = nucleo::Matcher::default();

        let mut scored: Vec<(u32, i64, usize)> = Vec::with_capacity(count);
        for item in snapshot.matched_items(0..count as u32) {
            let entry_idx = *item.data;
            let haystack = item.matcher_columns[0].slice(..);
            if let Some(score) = pattern.score(haystack, &mut scorer) {
                let entry = &self.entries[entry_idx];
                scored.push((score, entry.time_updated, entry_idx));
            }
        }

        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));

        scored.into_iter().map(|(_, _, idx)| idx).collect()
    }

    pub fn visible_entries(&self, page_size: usize) -> Vec<VisibleEntry> {
        let snapshot = self.matcher.snapshot();
        let count = snapshot.matched_item_count() as usize;
        if count == 0 {
            return Vec::new();
        }

        let start = self.scroll_offset;
        let end = (start + page_size).min(count);

        let pattern = snapshot.pattern().column_pattern(0);
        let mut indices_matcher = nucleo::Matcher::default();
        let mut indices_buf = Vec::new();

        let mut result = Vec::new();
        let sorted = self.sorted_match_indices();

        for idx in sorted.into_iter().skip(start).take(end - start) {
            let Some(item) = snapshot.get_item(idx as u32) else {
                continue;
            };
            let Some(entry) = self.entries.get(idx) else {
                continue;
            };

            indices_buf.clear();
            let haystack = item.matcher_columns[0].slice(..);
            let _ = pattern.indices(haystack, &mut indices_matcher, &mut indices_buf);
            indices_buf.sort_unstable();
            indices_buf.dedup();

            let repo_len = entry.repo.chars().count() as u32;
            let title_len = entry.title.chars().count() as u32;
            let title_start = repo_len + 1;
            let dir_start = title_start + title_len + 1;

            let mut repo_indices = Vec::new();
            let mut title_indices = Vec::new();
            let mut dir_indices = Vec::new();
            let is_live = self.live_session_ids.contains(&entry.session_id);
            let is_selected = self.selected_ids.contains(&entry.session_id);

            for &i in &indices_buf {
                if i < repo_len {
                    repo_indices.push(i);
                } else if i >= title_start && i < title_start + title_len {
                    title_indices.push(i - title_start);
                } else if i >= dir_start {
                    dir_indices.push(i - dir_start);
                }
            }

            result.push((
                entry.clone(),
                repo_indices,
                title_indices,
                dir_indices,
                is_live,
                is_selected,
            ));
        }

        result
    }

    pub fn ensure_visible(&mut self, page_size: usize) {
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + page_size {
            self.scroll_offset = self.selected.saturating_sub(page_size).saturating_add(1);
        }
    }

    fn refresh_pattern(&mut self) {
        self.matcher.pattern.reparse(
            0,
            &self.query,
            CaseMatching::Smart,
            Normalization::Smart,
            false,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_entry(id: &str, title: &str) -> DbManagedSession {
        DbManagedSession {
            id: id.into(),
            title: title.into(),
            directory: PathBuf::from("/tmp"),
            worktree: PathBuf::from("/tmp/repo"),
            user_message_count: 10,
            time_updated: 1000,
        }
    }

    #[test]
    fn space_toggles_selection() {
        let mut state = SessionManagerState::from_entries(
            vec![test_entry("s1", "T1"), test_entry("s2", "T2")],
            HashSet::new(),
        );
        state.toggle_select();
        assert!(state.selected_ids.contains("s1"));
        state.toggle_select();
        assert!(!state.selected_ids.contains("s1"));
    }

    #[test]
    fn select_all_matched_only_selects_filtered() {
        let mut state = SessionManagerState::from_entries(
            vec![test_entry("s1", "foo"), test_entry("s2", "bar")],
            HashSet::new(),
        );
        state.insert_char('f');
        state.tick();
        state.select_all_matched();
        assert!(state.selected_ids.contains("s1"));
        assert!(!state.selected_ids.contains("s2"));
    }

    #[test]
    fn typing_does_not_clear_selection() {
        let mut state = SessionManagerState::from_entries(
            vec![test_entry("s1", "foo"), test_entry("s2", "bar")],
            HashSet::new(),
        );
        state.toggle_select();
        assert!(state.selected_ids.contains("s1"));
        state.insert_char('b');
        assert!(state.selected_ids.contains("s1"));
    }

    #[test]
    fn key_to_command_mapping() {
        let k = |code| KeyEvent::new(code, KeyModifiers::NONE);
        let ctrl = |ch| KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL);

        // Plain typing
        assert_eq!(
            manager_key_to_command(k(KeyCode::Char('a')), false, false),
            ManagerCommand::Insert('a')
        );
        assert_eq!(
            manager_key_to_command(k(KeyCode::Char('d')), false, false),
            ManagerCommand::Insert('d')
        );

        // Multi-select actions
        assert_eq!(
            manager_key_to_command(k(KeyCode::Tab), false, false),
            ManagerCommand::Toggle
        );
        assert_eq!(
            manager_key_to_command(ctrl('a'), false, false),
            ManagerCommand::SelectAll
        );
        assert_eq!(
            manager_key_to_command(ctrl('u'), false, true),
            ManagerCommand::Clear
        );
        assert_eq!(
            manager_key_to_command(ctrl('d'), false, false),
            ManagerCommand::RequestDelete
        );

        // Esc behavior
        assert_eq!(
            manager_key_to_command(k(KeyCode::Esc), false, true),
            ManagerCommand::Clear
        );
        assert_eq!(
            manager_key_to_command(k(KeyCode::Esc), false, false),
            ManagerCommand::Close
        );

        // Pending delete
        assert_eq!(
            manager_key_to_command(k(KeyCode::Char('y')), true, false),
            ManagerCommand::ConfirmDelete
        );
        assert_eq!(
            manager_key_to_command(k(KeyCode::Char('n')), true, false),
            ManagerCommand::CancelPending
        );
        assert_eq!(
            manager_key_to_command(k(KeyCode::Esc), true, false),
            ManagerCommand::CancelPending
        );
    }

    #[test]
    fn request_delete_targets_current_if_no_selection() {
        let mut state =
            SessionManagerState::from_entries(vec![test_entry("s1", "T1")], HashSet::new());
        state.request_delete();
        assert_eq!(state.pending_delete.unwrap().session_ids, vec!["s1"]);
    }
}
