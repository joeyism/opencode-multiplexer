use opencode_multiplexer::{
    app::workspace::{PaneContent, WorkspaceStore},
    ui::layout::{
        WorkspaceChrome, layout_workspace, pane_at, pane_content_rects, workspace_chrome,
        workspace_regions,
    },
};
use ratatui::layout::Rect;

#[test]
fn vertical_split_divides_workspace_into_left_and_right_panes() {
    let mut workspaces = WorkspaceStore::default();
    let workspace = workspaces.ensure_workspace(1);
    let first = workspaces.workspace(workspace).unwrap().primary_pane();
    let second = workspaces
        .split_with_content(workspace, first, PaneContent::Shell(1), false)
        .unwrap();

    let layout = layout_workspace(
        workspaces.workspace(workspace).unwrap(),
        Rect::new(0, 0, 80, 24),
    );

    assert_eq!(layout[&first], Rect::new(0, 0, 40, 24));
    assert_eq!(layout[&second], Rect::new(40, 0, 40, 24));
}

#[test]
fn horizontal_split_divides_workspace_into_top_and_bottom_panes() {
    let mut workspaces = WorkspaceStore::default();
    let workspace = workspaces.ensure_workspace(1);
    let first = workspaces.workspace(workspace).unwrap().primary_pane();
    let second = workspaces
        .split_with_content(workspace, first, PaneContent::Shell(1), true)
        .unwrap();

    let layout = layout_workspace(
        workspaces.workspace(workspace).unwrap(),
        Rect::new(0, 0, 80, 24),
    );

    assert_eq!(layout[&first], Rect::new(0, 0, 80, 12));
    assert_eq!(layout[&second], Rect::new(0, 12, 80, 12));
}

#[test]
fn too_small_window_temporarily_shows_only_the_target_pane() {
    let mut workspaces = WorkspaceStore::default();
    let workspace = workspaces.ensure_workspace(1);
    let first = workspaces.workspace(workspace).unwrap().primary_pane();
    let second = workspaces
        .split_with_content(workspace, first, PaneContent::Shell(1), false)
        .unwrap();

    let layout = layout_workspace(
        workspaces.workspace(workspace).unwrap(),
        Rect::new(0, 0, 30, 10),
    );

    assert_eq!(layout.len(), 1);
    assert_eq!(layout[&second], Rect::new(0, 0, 30, 10));
}

#[test]
fn overlay_regions_keep_sibling_panes_visible_around_the_target() {
    let mut workspaces = WorkspaceStore::default();
    let workspace = workspaces.ensure_workspace(1);
    let first = workspaces.workspace(workspace).unwrap().primary_pane();
    let second = workspaces
        .split_with_content(workspace, first, PaneContent::Shell(1), false)
        .unwrap();
    let area = Rect::new(0, 0, 80, 24);

    let regions = workspace_regions(workspaces.workspace(workspace).unwrap(), area).unwrap();

    assert_eq!(regions.target_pane, second);
    assert_eq!(regions.target_area, Rect::new(40, 0, 40, 24));
    assert_eq!(regions.siblings, vec![(first, Rect::new(0, 0, 40, 24))]);
}

#[test]
fn pane_at_maps_terminal_coordinates_to_the_correct_pane() {
    let mut workspaces = WorkspaceStore::default();
    let workspace = workspaces.ensure_workspace(1);
    let first = workspaces.workspace(workspace).unwrap().primary_pane();
    let second = workspaces
        .split_with_content(workspace, first, PaneContent::Shell(1), false)
        .unwrap();
    let workspace = workspaces.workspace(workspace).unwrap();

    assert_eq!(
        pane_at(workspace, Rect::new(0, 0, 80, 24), 10, 10),
        Some(first)
    );
    assert_eq!(
        pane_at(workspace, Rect::new(0, 0, 80, 24), 60, 10),
        Some(second)
    );
    assert_eq!(pane_at(workspace, Rect::new(0, 0, 80, 24), 81, 10), None);
}

#[test]
fn single_pane_workspace_is_flat() {
    let mut workspaces = WorkspaceStore::default();
    let workspace = workspaces.ensure_workspace(1);

    assert_eq!(
        workspace_chrome(workspaces.workspace(workspace).unwrap()),
        WorkspaceChrome::Flat
    );
}

#[test]
fn split_workspace_is_framed() {
    let mut workspaces = WorkspaceStore::default();
    let workspace = workspaces.ensure_workspace(1);
    let primary = workspaces.workspace(workspace).unwrap().primary_pane();
    workspaces
        .split_with_content(workspace, primary, PaneContent::Shell(1), false)
        .unwrap();

    assert_eq!(
        workspace_chrome(workspaces.workspace(workspace).unwrap()),
        WorkspaceChrome::Framed
    );
}

#[test]
fn closing_comparison_returns_to_flat() {
    let mut workspaces = WorkspaceStore::default();
    let workspace = workspaces.ensure_workspace(1);
    let primary = workspaces.workspace(workspace).unwrap().primary_pane();
    let comparison = workspaces
        .split_with_content(workspace, primary, PaneContent::Shell(1), false)
        .unwrap();
    workspaces.close_pane(workspace, comparison);

    assert_eq!(
        workspace_chrome(workspaces.workspace(workspace).unwrap()),
        WorkspaceChrome::Flat
    );
}

#[test]
fn flat_content_rect_is_the_whole_area() {
    let mut workspaces = WorkspaceStore::default();
    let workspace = workspaces.ensure_workspace(1);
    let pane = workspaces.workspace(workspace).unwrap().primary_pane();
    let area = Rect::new(0, 0, 80, 24);

    let rects = pane_content_rects(workspaces.workspace(workspace).unwrap(), area);

    assert_eq!(rects[&pane], area);
}

#[test]
fn framed_content_rects_exclude_borders() {
    let mut workspaces = WorkspaceStore::default();
    let workspace = workspaces.ensure_workspace(1);
    let first = workspaces.workspace(workspace).unwrap().primary_pane();
    let second = workspaces
        .split_with_content(workspace, first, PaneContent::Shell(1), false)
        .unwrap();

    let rects = pane_content_rects(
        workspaces.workspace(workspace).unwrap(),
        Rect::new(0, 0, 80, 24),
    );

    assert_eq!(rects[&first], Rect::new(1, 1, 38, 22));
    assert_eq!(rects[&second], Rect::new(41, 1, 38, 22));
}

#[test]
fn too_small_split_fallback_stays_framed_and_insets_target() {
    let mut workspaces = WorkspaceStore::default();
    let workspace = workspaces.ensure_workspace(1);
    let first = workspaces.workspace(workspace).unwrap().primary_pane();
    let target = workspaces
        .split_with_content(workspace, first, PaneContent::Shell(1), false)
        .unwrap();

    let rects = pane_content_rects(
        workspaces.workspace(workspace).unwrap(),
        Rect::new(0, 0, 30, 10),
    );

    assert_eq!(
        workspace_chrome(workspaces.workspace(workspace).unwrap()),
        WorkspaceChrome::Framed
    );
    assert_eq!(rects.len(), 1);
    assert_eq!(rects[&target], Rect::new(1, 1, 28, 8));
}
