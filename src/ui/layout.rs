use std::collections::HashMap;

use ratatui::layout::{Constraint, Layout, Rect};

use crate::app::workspace::{PaneId, PaneLayout, SplitAxis, Workspace};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceChrome {
    Flat,
    Framed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRegions {
    pub target_pane: PaneId,
    pub target_area: Rect,
    pub siblings: Vec<(PaneId, Rect)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RootLayout {
    pub sidebar: Rect,
    pub main: Rect,
    pub footer: Rect,
}

pub fn split_root(area: Rect, sidebar_width: u16, footer_height: u16) -> RootLayout {
    let [body, footer] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(footer_height)]).areas(area);

    let [sidebar, main] = Layout::horizontal([
        Constraint::Length(sidebar_width.min(body.width)),
        Constraint::Min(0),
    ])
    .areas(body);

    RootLayout {
        sidebar,
        main,
        footer,
    }
}

pub fn terminal_inner_rect(area: Rect, sidebar_width: u16, footer_height: u16) -> Rect {
    let layout = split_root(area, sidebar_width, footer_height);
    Rect::new(
        layout.main.x,
        layout.main.y + 1,
        layout.main.width,
        layout.main.height.saturating_sub(1),
    )
}

pub fn centered_rect(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let width = area.width * percent_x / 100;
    let height = area.height * percent_y / 100;
    Rect::new(
        area.x + (area.width.saturating_sub(width)) / 2,
        area.y + (area.height.saturating_sub(height)) / 2,
        width.max(1),
        height.max(1),
    )
}

pub fn lightbox_inner_rect(screen: Rect) -> Rect {
    let body = Rect::new(
        screen.x,
        screen.y,
        screen.width,
        screen.height.saturating_sub(1),
    );
    let popup = centered_rect(body, 96, 96);
    Rect::new(
        popup.x.saturating_add(1),
        popup.y.saturating_add(1),
        popup.width.saturating_sub(2),
        popup.height.saturating_sub(2),
    )
}

#[cfg(test)]
mod lightbox_tests {
    use super::*;

    #[test]
    fn lightbox_inner_fits_screen_body() {
        let screen = Rect::new(0, 0, 100, 40);
        let inner = lightbox_inner_rect(screen);
        assert!(inner.x > screen.x);
        assert!(inner.y > screen.y);
        assert!(inner.x + inner.width <= screen.width);
        assert!(inner.y + inner.height <= screen.height - 1);
    }

    #[test]
    fn lightbox_inner_is_safe_for_tiny_screens() {
        let _ = lightbox_inner_rect(Rect::new(0, 0, 2, 2));
        let _ = lightbox_inner_rect(Rect::new(0, 0, 0, 0));
    }
}

pub fn layout_workspace(workspace: &Workspace, area: Rect) -> HashMap<PaneId, Rect> {
    let mut panes = HashMap::new();
    layout_node(&workspace.layout_tree(), area, &mut panes);
    if panes
        .values()
        .any(|pane| pane.width < 24 || pane.height < 6)
    {
        return HashMap::from([(workspace.target_pane(), area)]);
    }
    panes
}

pub fn workspace_chrome(workspace: &Workspace) -> WorkspaceChrome {
    if workspace.pane_count() == 1 {
        WorkspaceChrome::Flat
    } else {
        WorkspaceChrome::Framed
    }
}

pub fn pane_content_rect(chrome: WorkspaceChrome, area: Rect) -> Rect {
    match chrome {
        WorkspaceChrome::Flat => area,
        WorkspaceChrome::Framed => Rect::new(
            area.x.saturating_add(1),
            area.y.saturating_add(1),
            area.width.saturating_sub(2),
            area.height.saturating_sub(2),
        ),
    }
}

pub fn pane_content_rects(workspace: &Workspace, area: Rect) -> HashMap<PaneId, Rect> {
    let chrome = workspace_chrome(workspace);
    layout_workspace(workspace, area)
        .into_iter()
        .map(|(pane_id, pane_area)| (pane_id, pane_content_rect(chrome, pane_area)))
        .collect()
}

pub fn workspace_regions(workspace: &Workspace, area: Rect) -> Option<WorkspaceRegions> {
    let layout = layout_workspace(workspace, area);
    let target_pane = workspace.target_pane();
    let target_area = *layout.get(&target_pane)?;
    let siblings = workspace
        .pane_order()
        .into_iter()
        .filter(|pane_id| *pane_id != target_pane)
        .filter_map(|pane_id| layout.get(&pane_id).copied().map(|rect| (pane_id, rect)))
        .collect();
    Some(WorkspaceRegions {
        target_pane,
        target_area,
        siblings,
    })
}

pub fn pane_at(workspace: &Workspace, area: Rect, x: u16, y: u16) -> Option<PaneId> {
    layout_workspace(workspace, area)
        .into_iter()
        .find_map(|(pane_id, rect)| rect.contains((x, y).into()).then_some(pane_id))
}

fn layout_node(node: &PaneLayout, area: Rect, panes: &mut HashMap<PaneId, Rect>) {
    match node {
        PaneLayout::Leaf(id) => {
            panes.insert(*id, area);
        }
        PaneLayout::Split {
            axis,
            first,
            second,
        } => {
            let [first_area, second_area] = match axis {
                SplitAxis::Vertical => {
                    Layout::horizontal([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
                        .areas(area)
                }
                SplitAxis::Horizontal => {
                    Layout::vertical([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)]).areas(area)
                }
            };
            layout_node(first, first_area, panes);
            layout_node(second, second_area, panes);
        }
    }
}
