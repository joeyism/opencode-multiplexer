use opencode_multiplexer::app::{
    Action,
    focus::AppFocus,
    reducer::reduce,
    state::AppState,
    workspace::{CloseRequest, PaneContent, PaneCycleDirection, WorkspaceStore},
};

#[test]
fn each_session_keeps_an_independent_workspace() {
    let mut workspaces = WorkspaceStore::default();

    let a = workspaces.ensure_workspace(1);
    let primary_a = workspaces.workspace(a).unwrap().primary_pane();
    workspaces
        .split_with_content(a, primary_a, PaneContent::Shell(10), false)
        .unwrap();

    let b = workspaces.ensure_workspace(2);
    let primary_b = workspaces.workspace(b).unwrap().primary_pane();
    assert_ne!(primary_a, primary_b);
    assert_eq!(workspaces.workspace(a).unwrap().pane_count(), 2);
    assert_eq!(workspaces.workspace(b).unwrap().pane_count(), 1);
}

#[test]
fn split_rejects_a_session_already_in_the_same_workspace() {
    let mut workspaces = WorkspaceStore::default();
    let workspace = workspaces.ensure_workspace(1);
    let primary = workspaces.workspace(workspace).unwrap().primary_pane();

    let result = workspaces.split_with_content(workspace, primary, PaneContent::Session(1), false);

    assert!(result.is_err());
    assert_eq!(workspaces.workspace(workspace).unwrap().pane_count(), 1);
}

#[test]
fn closing_comparison_preserves_primary_and_collapses_sibling() {
    let mut workspaces = WorkspaceStore::default();
    let workspace = workspaces.ensure_workspace(1);
    let primary = workspaces.workspace(workspace).unwrap().primary_pane();
    let comparison = workspaces
        .split_with_content(workspace, primary, PaneContent::Session(2), true)
        .unwrap();

    assert!(workspaces.close_pane(workspace, comparison));
    let restored = workspaces.workspace(workspace).unwrap();
    assert_eq!(restored.pane_count(), 1);
    assert_eq!(restored.primary_pane(), primary);
}

#[test]
fn target_cycle_is_stable_and_wraps_in_visual_order() {
    let mut workspaces = WorkspaceStore::default();
    let workspace = workspaces.ensure_workspace(1);
    let first = workspaces.workspace(workspace).unwrap().primary_pane();
    let second = workspaces
        .split_with_content(workspace, first, PaneContent::Shell(20), false)
        .unwrap();
    let third = workspaces
        .split_with_content(workspace, second, PaneContent::Shell(30), true)
        .unwrap();

    assert_eq!(
        workspaces.cycle_pane(workspace, PaneCycleDirection::Next),
        Some(first)
    );
    assert_eq!(
        workspaces.cycle_pane(workspace, PaneCycleDirection::Next),
        Some(second)
    );
    assert_eq!(
        workspaces.cycle_pane(workspace, PaneCycleDirection::Next),
        Some(third)
    );
}

#[test]
fn app_state_restores_the_workspace_for_each_session() {
    let mut state = AppState::default();

    let a_workspace = state.activate_workspace(1);
    let a_primary = state
        .workspaces
        .workspace(a_workspace)
        .unwrap()
        .primary_pane();
    state
        .workspaces
        .split_with_content(a_workspace, a_primary, PaneContent::Shell(9), false)
        .unwrap();

    let b_workspace = state.activate_workspace(2);
    assert_eq!(
        state
            .workspaces
            .workspace(b_workspace)
            .unwrap()
            .pane_count(),
        1
    );

    assert_eq!(state.activate_workspace(1), a_workspace);
    assert_eq!(
        state
            .workspaces
            .workspace(a_workspace)
            .unwrap()
            .pane_count(),
        2
    );
}

#[test]
fn active_workspace_reports_content_of_its_target_pane() {
    let mut state = AppState::default();
    let workspace = state.activate_workspace(7);
    let primary = state
        .workspaces
        .workspace(workspace)
        .unwrap()
        .primary_pane();
    state
        .workspaces
        .split_with_content(workspace, primary, PaneContent::Shell(44), false)
        .unwrap();

    assert_eq!(state.target_pane_content(), Some(PaneContent::Shell(44)));
}

#[test]
fn removing_an_exited_shell_collapses_its_pane_in_its_workspace() {
    let mut workspaces = WorkspaceStore::default();
    let workspace = workspaces.ensure_workspace(1);
    let primary = workspaces.workspace(workspace).unwrap().primary_pane();
    workspaces
        .split_with_content(workspace, primary, PaneContent::Shell(44), false)
        .unwrap();

    assert_eq!(workspaces.remove_shell_pane(44), 1);
    assert_eq!(workspaces.workspace(workspace).unwrap().pane_count(), 1);
}

#[test]
fn mouse_target_can_select_any_visible_pane_without_changing_workspace_owner() {
    let mut workspaces = WorkspaceStore::default();
    let workspace = workspaces.ensure_workspace(1);
    let first = workspaces.workspace(workspace).unwrap().primary_pane();
    let second = workspaces
        .split_with_content(workspace, first, PaneContent::Shell(4), false)
        .unwrap();

    assert!(workspaces.set_target(workspace, first));
    assert_eq!(
        workspaces.workspace(workspace).unwrap().target_pane(),
        first
    );
    assert_eq!(workspaces.workspace(workspace).unwrap().owner_session(), 1);
    assert!(!workspaces.set_target(workspace, 999));
    assert_eq!(
        workspaces.workspace(workspace).unwrap().target_pane(),
        first
    );
    assert_ne!(first, second);
}

#[test]
fn removing_a_session_cleans_comparisons_and_owned_shells() {
    let mut workspaces = WorkspaceStore::default();
    let a = workspaces.ensure_workspace(1);
    let a_primary = workspaces.workspace(a).unwrap().primary_pane();
    workspaces
        .split_with_content(a, a_primary, PaneContent::Session(2), false)
        .unwrap();

    let b = workspaces.ensure_workspace(2);
    let b_primary = workspaces.workspace(b).unwrap().primary_pane();
    workspaces
        .split_with_content(b, b_primary, PaneContent::Shell(99), false)
        .unwrap();

    let shells = workspaces.remove_session(2);

    assert_eq!(shells, vec![99]);
    assert!(workspaces.workspace(2).is_none());
    assert_eq!(workspaces.workspace(1).unwrap().pane_count(), 1);
}

#[test]
fn pane_cycle_next_wraps_in_visual_order() {
    let mut workspaces = WorkspaceStore::default();
    let workspace = workspaces.ensure_workspace(1);
    let first = workspaces.workspace(workspace).unwrap().primary_pane();
    let second = workspaces
        .split_with_content(workspace, first, PaneContent::Session(2), false)
        .unwrap();
    let third = workspaces
        .split_with_content(workspace, second, PaneContent::Session(3), false)
        .unwrap();
    workspaces.set_target(workspace, first);

    assert_eq!(
        workspaces.cycle_pane(workspace, PaneCycleDirection::Next),
        Some(second)
    );
    assert_eq!(
        workspaces.cycle_pane(workspace, PaneCycleDirection::Next),
        Some(third)
    );
    assert_eq!(
        workspaces.cycle_pane(workspace, PaneCycleDirection::Next),
        Some(first)
    );
}

#[test]
fn pane_cycle_previous_wraps_in_visual_order() {
    let mut workspaces = WorkspaceStore::default();
    let workspace = workspaces.ensure_workspace(1);
    let first = workspaces.workspace(workspace).unwrap().primary_pane();
    let second = workspaces
        .split_with_content(workspace, first, PaneContent::Session(2), false)
        .unwrap();
    let third = workspaces
        .split_with_content(workspace, second, PaneContent::Session(3), false)
        .unwrap();
    workspaces.set_target(workspace, first);

    assert_eq!(
        workspaces.cycle_pane(workspace, PaneCycleDirection::Previous),
        Some(third)
    );
    assert_eq!(
        workspaces.cycle_pane(workspace, PaneCycleDirection::Previous),
        Some(second)
    );
    assert_eq!(
        workspaces.cycle_pane(workspace, PaneCycleDirection::Previous),
        Some(first)
    );
}

#[test]
fn pane_cycle_single_pane_is_noop() {
    let mut workspaces = WorkspaceStore::default();
    let workspace = workspaces.ensure_workspace(1);
    let primary = workspaces.workspace(workspace).unwrap().primary_pane();

    assert_eq!(
        workspaces.cycle_pane(workspace, PaneCycleDirection::Next),
        None
    );
    assert_eq!(
        workspaces.workspace(workspace).unwrap().target_pane(),
        primary
    );
}

#[test]
fn pane_cycle_does_not_change_workspace_owner() {
    let mut workspaces = WorkspaceStore::default();
    let workspace = workspaces.ensure_workspace(1);
    let primary = workspaces.workspace(workspace).unwrap().primary_pane();
    workspaces
        .split_with_content(workspace, primary, PaneContent::Session(2), false)
        .unwrap();

    workspaces.cycle_pane(workspace, PaneCycleDirection::Next);

    assert_eq!(workspaces.workspace(workspace).unwrap().owner_session(), 1);
}

#[test]
fn focus_toggle_returns_to_remembered_terminal_pane() {
    let mut state = AppState::default();
    let workspace = state.activate_workspace(1);
    let primary = state
        .workspaces
        .workspace(workspace)
        .unwrap()
        .primary_pane();
    let comparison = state
        .workspaces
        .split_with_content(workspace, primary, PaneContent::Session(2), false)
        .unwrap();
    state.workspaces.set_target(workspace, comparison);
    state.focus = AppFocus::Terminal;

    reduce(&mut state, Action::ToggleFocus);
    assert_eq!(state.focus, AppFocus::Sidebar);
    assert_eq!(
        state.workspaces.workspace(workspace).unwrap().target_pane(),
        comparison
    );

    reduce(&mut state, Action::ToggleFocus);
    assert_eq!(state.focus, AppFocus::Terminal);
    assert_eq!(
        state.workspaces.workspace(workspace).unwrap().target_pane(),
        comparison
    );
}

#[test]
fn pane_cycle_then_focus_toggle_preserves_new_target() {
    let mut state = AppState::default();
    let workspace = state.activate_workspace(1);
    let primary = state
        .workspaces
        .workspace(workspace)
        .unwrap()
        .primary_pane();
    let comparison = state
        .workspaces
        .split_with_content(workspace, primary, PaneContent::Session(2), false)
        .unwrap();
    state.workspaces.set_target(workspace, primary);
    state.focus = AppFocus::Terminal;
    state
        .workspaces
        .cycle_pane(workspace, PaneCycleDirection::Next);

    reduce(&mut state, Action::ToggleFocus);
    reduce(&mut state, Action::ToggleFocus);

    assert_eq!(state.focus, AppFocus::Terminal);
    assert_eq!(
        state.workspaces.workspace(workspace).unwrap().target_pane(),
        comparison
    );
}

#[test]
fn close_request_on_comparison_targets_that_pane() {
    let mut workspaces = WorkspaceStore::default();
    let workspace = workspaces.ensure_workspace(1);
    let primary = workspaces.workspace(workspace).unwrap().primary_pane();
    let comparison = workspaces
        .split_with_content(workspace, primary, PaneContent::Session(2), false)
        .unwrap();

    assert_eq!(
        workspaces.close_request(workspace),
        CloseRequest::ClosePane(comparison)
    );
}

#[test]
fn close_request_on_primary_with_one_other_targets_the_other_pane() {
    let mut workspaces = WorkspaceStore::default();
    let workspace = workspaces.ensure_workspace(1);
    let primary = workspaces.workspace(workspace).unwrap().primary_pane();
    let comparison = workspaces
        .split_with_content(workspace, primary, PaneContent::Session(2), false)
        .unwrap();
    workspaces.set_target(workspace, primary);

    assert_eq!(
        workspaces.close_request(workspace),
        CloseRequest::ClosePane(comparison)
    );
}

#[test]
fn close_request_on_primary_with_one_shell_other_requests_confirmation() {
    let mut workspaces = WorkspaceStore::default();
    let workspace = workspaces.ensure_workspace(1);
    let primary = workspaces.workspace(workspace).unwrap().primary_pane();
    workspaces
        .split_with_content(workspace, primary, PaneContent::Shell(77), false)
        .unwrap();
    workspaces.set_target(workspace, primary);

    assert_eq!(
        workspaces.close_request(workspace),
        CloseRequest::ConfirmShell(77)
    );
}

#[test]
fn close_request_on_primary_with_multiple_others_is_ambiguous() {
    let mut workspaces = WorkspaceStore::default();
    let workspace = workspaces.ensure_workspace(1);
    let primary = workspaces.workspace(workspace).unwrap().primary_pane();
    let second = workspaces
        .split_with_content(workspace, primary, PaneContent::Session(2), false)
        .unwrap();
    workspaces
        .split_with_content(workspace, second, PaneContent::Session(3), false)
        .unwrap();
    workspaces.set_target(workspace, primary);

    assert_eq!(workspaces.close_request(workspace), CloseRequest::Ambiguous);
}

#[test]
fn close_request_on_single_pane_reports_nothing_to_close() {
    let mut workspaces = WorkspaceStore::default();
    let workspace = workspaces.ensure_workspace(1);

    assert_eq!(
        workspaces.close_request(workspace),
        CloseRequest::NothingToClose
    );
}

#[test]
fn closing_resolved_comparison_returns_target_to_primary() {
    let mut workspaces = WorkspaceStore::default();
    let workspace = workspaces.ensure_workspace(1);
    let primary = workspaces.workspace(workspace).unwrap().primary_pane();
    let comparison = workspaces
        .split_with_content(workspace, primary, PaneContent::Session(2), false)
        .unwrap();
    workspaces.set_target(workspace, primary);
    let CloseRequest::ClosePane(pane) = workspaces.close_request(workspace) else {
        panic!("expected comparison close request");
    };

    assert_eq!(pane, comparison);
    assert!(workspaces.close_pane(workspace, pane));
    assert_eq!(
        workspaces.workspace(workspace).unwrap().target_pane(),
        primary
    );
}
