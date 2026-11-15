use crate::app::agents::AgentsViewState;
use crate::app::focus::AppFocus;
use crate::app::help::HelpState;
use crate::app::message_picker::MessagePickerState;
use crate::app::playbook::{ParamFormState, PlaybookPickerState, PlaybookRun};
use crate::app::session_manager::SessionManagerState;
use crate::app::session_picker::SessionPickerState;
use crate::app::split_picker::SplitPickerState;
use crate::app::workspace::{PaneContent, WorkspaceId, WorkspaceStore};
use crate::data::poller::ChildSessionInfo;
use crate::data::vitals::{SessionVitals, VitalsRequest};
use crate::ui::vitals::VitalsMode;
use std::collections::{HashMap, HashSet};

pub struct AppState {
    pub focus: AppFocus,
    pub help: Option<HelpState>,
    pub panel_hidden: bool,
    pub selected_sidebar_row: usize,
    pub expanded_session_ids: HashSet<String>,
    pub app_focused: bool,
    pub last_main_focus: AppFocus,
    pub show_files: Vec<String>,
    pub session_picker: Option<SessionPickerState>,
    pub session_manager: Option<SessionManagerState>,
    pub message_picker: Option<MessagePickerState>,
    pub confirm_quit: bool,
    pub agents: AgentsViewState,
    pub playbook_picker: Option<PlaybookPickerState>,
    pub playbook_form: Option<ParamFormState>,
    pub playbook_abort_for: Option<String>,
    pub playbook_runs: HashMap<String, PlaybookRun>,
    pub workspaces: WorkspaceStore,
    pub active_workspace: Option<WorkspaceId>,
    pub split_picker: Option<SplitPickerState>,
    pub close_shell_for: Option<u64>,
    /// Last computed session vitals for the active session tree.
    pub vitals: Option<SessionVitals>,
    pub vitals_mode: VitalsMode,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            focus: AppFocus::default(),
            help: None,
            panel_hidden: false,
            selected_sidebar_row: 0,
            expanded_session_ids: HashSet::new(),
            app_focused: true,
            last_main_focus: AppFocus::Terminal,
            show_files: Vec::new(),
            session_picker: None,
            session_manager: None,
            message_picker: None,
            confirm_quit: false,
            agents: AgentsViewState::default(),
            playbook_picker: None,
            playbook_form: None,
            playbook_abort_for: None,
            playbook_runs: HashMap::new(),
            workspaces: WorkspaceStore::default(),
            active_workspace: None,
            split_picker: None,
            close_shell_for: None,
            vitals: None,
            vitals_mode: VitalsMode::StackedAll,
        }
    }
}

impl AppState {
    pub fn activate_workspace(&mut self, owner_session: u64) -> WorkspaceId {
        let workspace = self.workspaces.ensure_workspace(owner_session);
        self.active_workspace = Some(workspace);
        workspace
    }

    pub fn target_pane_content(&self) -> Option<PaneContent> {
        let workspace = self.workspaces.workspace(self.active_workspace?)?;
        workspace.content(workspace.target_pane())
    }
}

/// All descendant session ids under `children`, depth-first.
pub fn flatten_child_ids(children: &[ChildSessionInfo]) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack: Vec<&ChildSessionInfo> = children.iter().rev().collect();
    while let Some(node) = stack.pop() {
        out.push(node.session_id.clone());
        for child in node.children.iter().rev() {
            stack.push(child);
        }
    }
    out
}

/// Find the subtree child ids for `session_id` by walking top-level summaries
/// and their child trees. Returns `None` when the session is absent.
pub fn find_subtree_ids(
    summaries: &[crate::app::sessions::SessionSummary],
    session_id: &str,
) -> Option<Vec<String>> {
    for s in summaries {
        if s.session_id.as_deref() == Some(session_id) {
            return Some(flatten_child_ids(&s.children));
        }
        let mut stack: Vec<&ChildSessionInfo> = s.children.iter().collect();
        while let Some(node) = stack.pop() {
            if node.session_id == session_id {
                return Some(flatten_child_ids(&node.children));
            }
            for child in &node.children {
                stack.push(child);
            }
        }
    }
    None
}

/// Resolve which session tree the vitals panel should track.
///
/// Follows the inspected conversation session when `conversation_session` is
/// set (conversation view open, possibly on a child subagent); otherwise the
/// target pane's session. Shell/empty panes return `None`.
pub fn resolve_vitals_target_from(
    summaries: &[crate::app::sessions::SessionSummary],
    target_entry_id: Option<u64>,
    conversation_session: Option<&str>,
) -> Option<VitalsRequest> {
    if let Some(sid) = conversation_session {
        let children = find_subtree_ids(summaries, sid).unwrap_or_default();
        let is_working = summaries.iter().any(|s| {
            s.session_id.as_deref() == Some(sid)
                && matches!(
                    s.status,
                    crate::app::sessions::SessionStatus::Working
                        | crate::app::sessions::SessionStatus::SubagentsWorking
                )
        });
        return Some(VitalsRequest {
            primary_session_id: sid.to_string(),
            child_session_ids: children,
            is_working,
        });
    }
    let entry = target_entry_id?;
    let summary = summaries.iter().find(|s| s.id == entry)?;
    let sid = summary.session_id.clone()?;
    let is_working = matches!(
        summary.status,
        crate::app::sessions::SessionStatus::Working
            | crate::app::sessions::SessionStatus::SubagentsWorking
    );
    Some(VitalsRequest {
        primary_session_id: sid,
        child_session_ids: flatten_child_ids(&summary.children),
        is_working,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::sessions::{SessionOrigin, SessionStatus, SessionSummary};
    use std::path::PathBuf;

    fn child(id: &str, children: Vec<ChildSessionInfo>) -> ChildSessionInfo {
        ChildSessionInfo {
            session_id: id.to_string(),
            cwd: PathBuf::from("/tmp"),
            title: id.to_string(),
            status: SessionStatus::Idle,
            time_updated: None,
            has_children: !children.is_empty(),
            children,
        }
    }

    fn summary(id: u64, sid: Option<&str>, children: Vec<ChildSessionInfo>) -> SessionSummary {
        SessionSummary {
            id,
            cwd: PathBuf::from("/tmp"),
            title: "t".to_string(),
            status: SessionStatus::Working,
            session_id: sid.map(str::to_string),
            origin: SessionOrigin::Managed,
            process_pid: None,
            serve_pid: None,
            serve_port: None,
            model: None,
            preview: None,
            time_updated: None,
            has_children: !children.is_empty(),
            children,
        }
    }

    #[test]
    fn target_pane_session_resolves_with_flattened_descendants() {
        let summaries = vec![summary(
            1,
            Some("parent"),
            vec![
                child("kid1", vec![child("grand", vec![])]),
                child("kid2", vec![]),
            ],
        )];
        let req = resolve_vitals_target_from(&summaries, Some(1), None).unwrap();
        assert_eq!(req.primary_session_id, "parent");
        assert_eq!(req.child_session_ids, vec!["kid1", "grand", "kid2"]);
        assert!(req.is_working);
    }

    #[test]
    fn conversation_session_overrides_target_pane_and_finds_child_subtree() {
        let summaries = vec![summary(
            1,
            Some("parent"),
            vec![child("kid1", vec![child("grand", vec![])])],
        )];
        let req = resolve_vitals_target_from(&summaries, Some(1), Some("kid1")).unwrap();
        assert_eq!(req.primary_session_id, "kid1");
        assert_eq!(req.child_session_ids, vec!["grand"]);
    }

    #[test]
    fn shell_or_unknown_target_resolves_to_none() {
        let summaries = vec![summary(1, Some("parent"), vec![])];
        assert!(resolve_vitals_target_from(&summaries, None, None).is_none());
        assert!(resolve_vitals_target_from(&summaries, Some(99), None).is_none());
        let no_sid = vec![summary(1, None, vec![])];
        assert!(resolve_vitals_target_from(&no_sid, Some(1), None).is_none());
    }
}
