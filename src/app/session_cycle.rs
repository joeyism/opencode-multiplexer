use crate::ui::sidebar::{SidebarRowKind, SidebarVisibleRow};

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
