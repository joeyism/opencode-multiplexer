use crate::app::focus::AppFocus;
use crate::config::Keybindings;
use crate::ui::sidebar::{SidebarRowKind, SidebarVisibleRow};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CycleDir {
    Next,
    Prev,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CycleOutcome {
    pub selected_row: usize,
    pub top_level_id: u64,
}

pub fn apply_cycle(
    selected: usize,
    rows: &[SidebarVisibleRow],
    dir: CycleDir,
) -> Option<CycleOutcome> {
    let tops: Vec<(usize, u64)> = rows
        .iter()
        .enumerate()
        .filter_map(|(idx, row)| match row.kind {
            SidebarRowKind::TopLevel { top_level_id, .. } => Some((idx, top_level_id)),
            SidebarRowKind::Child { .. } => None,
        })
        .collect();
    if tops.len() < 2 {
        return None;
    }
    let current = tops
        .iter()
        .rposition(|(idx, _)| *idx <= selected)
        .unwrap_or(0);
    let next = match dir {
        CycleDir::Next => (current + 1) % tops.len(),
        CycleDir::Prev => (current + tops.len() - 1) % tops.len(),
    };
    let (selected_row, top_level_id) = tops[next];
    Some(CycleOutcome {
        selected_row,
        top_level_id,
    })
}

pub fn chrome_session_cycle(
    key: KeyEvent,
    keys: &Keybindings,
    focus: AppFocus,
    overlay_open: bool,
    selected: usize,
    rows: &[SidebarVisibleRow],
) -> Option<CycleOutcome> {
    if overlay_open || !matches!(focus, AppFocus::Sidebar | AppFocus::Terminal) {
        return None;
    }
    let (ctrl, alt, shift, ch) = match key.code {
        KeyCode::Char(ch) => (
            key.modifiers.contains(KeyModifiers::CONTROL),
            key.modifiers.contains(KeyModifiers::ALT),
            key.modifiers.contains(KeyModifiers::SHIFT),
            ch,
        ),
        _ => return None,
    };
    let dir = if keys.session_next.matches(ctrl, alt, shift, ch) {
        CycleDir::Next
    } else if keys.session_prev.matches(ctrl, alt, shift, ch) {
        CycleDir::Prev
    } else {
        return None;
    };
    apply_cycle(selected, rows, dir)
}

pub fn follow_attach_id(follow_selection: bool, row: &SidebarVisibleRow) -> Option<u64> {
    if !follow_selection {
        return None;
    }
    match row.kind {
        SidebarRowKind::TopLevel { top_level_id, .. } => Some(top_level_id),
        SidebarRowKind::Child { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::sessions::{SessionOrigin, SessionStatus};
    use std::path::PathBuf;

    fn top(id: u64) -> SidebarVisibleRow {
        SidebarVisibleRow {
            kind: SidebarRowKind::TopLevel {
                top_level_id: id,
                session_id: Some(format!("s{id}")),
            },
            cwd: PathBuf::from("/tmp"),
            title: format!("s{id}"),
            status: SessionStatus::Idle,
            depth: 0,
            has_children: false,
            expanded: false,
            active: false,
            origin: SessionOrigin::Managed,
            session_id: Some(format!("s{id}")),
            time_updated: None,
        }
    }

    fn child(session_id: &str) -> SidebarVisibleRow {
        let mut row = top(99);
        row.kind = SidebarRowKind::Child {
            session_id: session_id.into(),
        };
        row.depth = 1;
        row.session_id = Some(session_id.into());
        row
    }

    fn cycle_key(ch: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL | KeyModifiers::ALT)
    }

    #[test]
    fn chrome_cycle_from_terminal_moves_selection() {
        let rows = vec![top(1), top(2)];
        let keys = Keybindings::default();
        let out = chrome_session_cycle(cycle_key('j'), &keys, AppFocus::Terminal, false, 0, &rows)
            .unwrap();
        assert_eq!(out.top_level_id, 2);
    }

    #[test]
    fn chrome_cycle_ignored_in_diff_or_overlay() {
        let rows = vec![top(1), top(2)];
        let keys = Keybindings::default();
        assert!(
            chrome_session_cycle(cycle_key('j'), &keys, AppFocus::Diff, false, 0, &rows).is_none()
        );
        assert!(
            chrome_session_cycle(cycle_key('j'), &keys, AppFocus::Terminal, true, 0, &rows)
                .is_none()
        );
    }

    #[test]
    fn chrome_cycle_ignores_unrelated_keys() {
        let rows = vec![top(1), top(2)];
        let keys = Keybindings::default();
        let key = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        assert!(chrome_session_cycle(key, &keys, AppFocus::Sidebar, false, 0, &rows).is_none());
    }

    #[test]
    fn chrome_cycle_prev_uses_session_prev_chord() {
        let rows = vec![top(1), top(2)];
        let keys = Keybindings::default();
        let out = chrome_session_cycle(cycle_key('k'), &keys, AppFocus::Sidebar, false, 0, &rows)
            .unwrap();
        assert_eq!(out.top_level_id, 2);
    }

    #[test]
    fn next_wraps_among_top_level_rows() {
        let rows = vec![top(1), top(2), top(3)];
        let a = apply_cycle(0, &rows, CycleDir::Next).unwrap();
        assert_eq!(a.selected_row, 1);
        assert_eq!(a.top_level_id, 2);
        let b = apply_cycle(2, &rows, CycleDir::Next).unwrap();
        assert_eq!(b.selected_row, 0);
        assert_eq!(b.top_level_id, 1);
    }

    #[test]
    fn prev_wraps_among_top_level_rows() {
        let rows = vec![top(1), top(2), top(3)];
        let a = apply_cycle(0, &rows, CycleDir::Prev).unwrap();
        assert_eq!(a.selected_row, 2);
        assert_eq!(a.top_level_id, 3);
    }

    #[test]
    fn cycle_skips_child_rows() {
        let rows = vec![top(1), child("c1"), top(2)];
        let a = apply_cycle(0, &rows, CycleDir::Next).unwrap();
        assert_eq!(a.selected_row, 2);
        assert_eq!(a.top_level_id, 2);
        let b = apply_cycle(1, &rows, CycleDir::Next).unwrap();
        assert_eq!(b.selected_row, 2);
        assert_eq!(b.top_level_id, 2);
    }

    #[test]
    fn single_top_level_is_noop() {
        let rows = vec![top(1), child("c1")];
        assert!(apply_cycle(0, &rows, CycleDir::Next).is_none());
        assert!(apply_cycle(0, &rows, CycleDir::Prev).is_none());
    }

    #[test]
    fn empty_rows_are_noop() {
        assert!(apply_cycle(0, &[], CycleDir::Next).is_none());
    }

    #[test]
    fn follow_attach_only_for_top_level_when_enabled() {
        let top_row = top(1);
        let child_row = child("c1");
        assert_eq!(follow_attach_id(true, &top_row), Some(1));
        assert_eq!(follow_attach_id(true, &child_row), None);
        assert_eq!(follow_attach_id(false, &top_row), None);
    }

    #[test]
    fn follow_does_not_attach_after_landing_on_child() {
        let rows = [top(1), child("c1"), top(2)];
        assert_eq!(follow_attach_id(true, &rows[1]), None);
    }
}
