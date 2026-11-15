use std::collections::HashMap;

pub type WorkspaceId = u64;
pub type PaneId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitAxis {
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneCycleDirection {
    Next,
    Previous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseRequest {
    ClosePane(PaneId),
    ConfirmShell(u64),
    Ambiguous,
    NothingToClose,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneContent {
    Empty,
    Session(u64),
    Shell(u64),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PaneNode {
    Leaf {
        id: PaneId,
        content: PaneContent,
    },
    Split {
        axis: SplitAxis,
        first: Box<PaneNode>,
        second: Box<PaneNode>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneLayout {
    Leaf(PaneId),
    Split {
        axis: SplitAxis,
        first: Box<PaneLayout>,
        second: Box<PaneLayout>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    owner_session: u64,
    root: PaneNode,
    primary_pane: PaneId,
    target_pane: PaneId,
}

impl Workspace {
    pub fn owner_session(&self) -> u64 {
        self.owner_session
    }

    pub fn primary_pane(&self) -> PaneId {
        self.primary_pane
    }

    pub fn target_pane(&self) -> PaneId {
        self.target_pane
    }

    pub fn pane_count(&self) -> usize {
        self.panes().len()
    }

    pub fn content(&self, id: PaneId) -> Option<PaneContent> {
        self.root.content(id)
    }

    pub fn panes(&self) -> Vec<(PaneId, PaneContent)> {
        let mut result = Vec::new();
        self.root.collect(&mut result);
        result
    }

    pub fn pane_order(&self) -> Vec<PaneId> {
        self.panes().into_iter().map(|(id, _)| id).collect()
    }

    pub fn layout_tree(&self) -> PaneLayout {
        self.root.layout_tree()
    }

    fn contains_session(&self, session: u64) -> bool {
        self.panes()
            .iter()
            .any(|(_, content)| *content == PaneContent::Session(session))
    }

    fn replace_content(&mut self, id: PaneId, content: PaneContent) -> bool {
        self.root.replace_content(id, content)
    }

    fn split(
        &mut self,
        target: PaneId,
        new_id: PaneId,
        content: PaneContent,
        axis: SplitAxis,
    ) -> bool {
        self.root.split(target, new_id, content, axis)
    }

    fn close(&mut self, id: PaneId) -> bool {
        if id == self.primary_pane || !self.root.close(id) {
            return false;
        }
        if self.target_pane == id {
            self.target_pane = self.primary_pane;
        }
        true
    }
}

impl PaneNode {
    fn layout_tree(&self) -> PaneLayout {
        match self {
            Self::Leaf { id, .. } => PaneLayout::Leaf(*id),
            Self::Split {
                axis,
                first,
                second,
            } => PaneLayout::Split {
                axis: *axis,
                first: Box::new(first.layout_tree()),
                second: Box::new(second.layout_tree()),
            },
        }
    }

    fn content(&self, id: PaneId) -> Option<PaneContent> {
        match self {
            Self::Leaf { id: pane, content } if *pane == id => Some(*content),
            Self::Leaf { .. } => None,
            Self::Split { first, second, .. } => first.content(id).or_else(|| second.content(id)),
        }
    }

    fn collect(&self, output: &mut Vec<(PaneId, PaneContent)>) {
        match self {
            Self::Leaf { id, content } => output.push((*id, *content)),
            Self::Split { first, second, .. } => {
                first.collect(output);
                second.collect(output);
            }
        }
    }

    fn replace_content(&mut self, id: PaneId, content: PaneContent) -> bool {
        match self {
            Self::Leaf {
                id: pane,
                content: slot,
            } if *pane == id => {
                *slot = content;
                true
            }
            Self::Leaf { .. } => false,
            Self::Split { first, second, .. } => {
                first.replace_content(id, content) || second.replace_content(id, content)
            }
        }
    }

    fn split(
        &mut self,
        target: PaneId,
        new_id: PaneId,
        content: PaneContent,
        axis: SplitAxis,
    ) -> bool {
        match self {
            Self::Leaf {
                id,
                content: old_content,
            } if *id == target => {
                let old = Self::Leaf {
                    id: *id,
                    content: *old_content,
                };
                let new = Self::Leaf {
                    id: new_id,
                    content,
                };
                *self = Self::Split {
                    axis,
                    first: Box::new(old),
                    second: Box::new(new),
                };
                true
            }
            Self::Leaf { .. } => false,
            Self::Split { first, second, .. } => {
                first.split(target, new_id, content, axis)
                    || second.split(target, new_id, content, axis)
            }
        }
    }

    fn close(&mut self, id: PaneId) -> bool {
        match self {
            Self::Leaf { .. } => false,
            Self::Split { first, second, .. } => {
                if matches!(first.as_ref(), Self::Leaf { id: candidate, .. } if *candidate == id) {
                    *self = (**second).clone();
                    true
                } else if matches!(second.as_ref(), Self::Leaf { id: candidate, .. } if *candidate == id)
                {
                    *self = (**first).clone();
                    true
                } else {
                    first.close(id) || second.close(id)
                }
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct WorkspaceStore {
    workspaces: HashMap<WorkspaceId, Workspace>,
    next_pane_id: PaneId,
}

impl WorkspaceStore {
    pub fn ensure_workspace(&mut self, owner_session: u64) -> WorkspaceId {
        if !self.workspaces.contains_key(&owner_session) {
            let pane = self.allocate_pane_id();
            self.workspaces.insert(
                owner_session,
                Workspace {
                    owner_session,
                    root: PaneNode::Leaf {
                        id: pane,
                        content: PaneContent::Session(owner_session),
                    },
                    primary_pane: pane,
                    target_pane: pane,
                },
            );
        }
        owner_session
    }

    pub fn workspace(&self, id: WorkspaceId) -> Option<&Workspace> {
        self.workspaces.get(&id)
    }

    pub fn workspace_mut(&mut self, id: WorkspaceId) -> Option<&mut Workspace> {
        self.workspaces.get_mut(&id)
    }

    pub fn replace_content(
        &mut self,
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        content: PaneContent,
    ) -> Result<(), &'static str> {
        let workspace = self
            .workspaces
            .get_mut(&workspace_id)
            .ok_or("workspace not found")?;
        if let PaneContent::Session(session) = content
            && workspace.contains_session(session)
            && workspace.content(pane_id) != Some(content)
        {
            return Err("session already appears in workspace");
        }
        if workspace.replace_content(pane_id, content) {
            workspace.target_pane = pane_id;
            Ok(())
        } else {
            Err("pane not found")
        }
    }

    pub fn split_with_content(
        &mut self,
        workspace_id: WorkspaceId,
        target: PaneId,
        content: PaneContent,
        horizontal: bool,
    ) -> Result<PaneId, &'static str> {
        let workspace = self
            .workspaces
            .get_mut(&workspace_id)
            .ok_or("workspace not found")?;
        if workspace.content(target).is_none() {
            return Err("pane not found");
        }
        if let PaneContent::Session(session) = content
            && workspace.contains_session(session)
        {
            return Err("session already appears in workspace");
        }
        let pane_id = self.next_pane_id;
        self.next_pane_id += 1;
        let axis = if horizontal {
            SplitAxis::Horizontal
        } else {
            SplitAxis::Vertical
        };
        if workspace.split(target, pane_id, content, axis) {
            workspace.target_pane = pane_id;
            Ok(pane_id)
        } else {
            Err("pane not found")
        }
    }

    pub fn close_pane(&mut self, workspace_id: WorkspaceId, pane_id: PaneId) -> bool {
        self.workspaces
            .get_mut(&workspace_id)
            .is_some_and(|workspace| workspace.close(pane_id))
    }

    pub fn close_request(&self, workspace_id: WorkspaceId) -> CloseRequest {
        let Some(workspace) = self.workspaces.get(&workspace_id) else {
            return CloseRequest::NothingToClose;
        };
        let target = workspace.target_pane();
        if target != workspace.primary_pane() {
            return match workspace.content(target) {
                Some(PaneContent::Shell(id)) => CloseRequest::ConfirmShell(id),
                Some(_) => CloseRequest::ClosePane(target),
                None => CloseRequest::NothingToClose,
            };
        }

        let comparisons = workspace
            .panes()
            .into_iter()
            .filter(|(pane_id, _)| *pane_id != workspace.primary_pane())
            .collect::<Vec<_>>();
        match comparisons.as_slice() {
            [] => CloseRequest::NothingToClose,
            [(pane_id, PaneContent::Shell(id))] => {
                let _ = pane_id;
                CloseRequest::ConfirmShell(*id)
            }
            [(pane_id, _)] => CloseRequest::ClosePane(*pane_id),
            _ => CloseRequest::Ambiguous,
        }
    }

    pub fn cycle_pane(
        &mut self,
        workspace_id: WorkspaceId,
        direction: PaneCycleDirection,
    ) -> Option<PaneId> {
        let workspace = self.workspaces.get_mut(&workspace_id)?;
        let panes = workspace.pane_order();
        if panes.len() < 2 {
            return None;
        }
        let current = panes
            .iter()
            .position(|id| *id == workspace.target_pane)
            .unwrap_or(0);
        let target = match direction {
            PaneCycleDirection::Next => (current + 1) % panes.len(),
            PaneCycleDirection::Previous => (current + panes.len() - 1) % panes.len(),
        };
        workspace.target_pane = panes[target];
        Some(workspace.target_pane)
    }

    pub fn set_target(&mut self, workspace_id: WorkspaceId, pane_id: PaneId) -> bool {
        let Some(workspace) = self.workspaces.get_mut(&workspace_id) else {
            return false;
        };
        if workspace.content(pane_id).is_none() {
            return false;
        }
        workspace.target_pane = pane_id;
        true
    }

    pub fn remove_shell_pane(&mut self, shell_id: u64) -> usize {
        let mut removed = 0;
        for workspace in self.workspaces.values_mut() {
            let pane = workspace
                .panes()
                .into_iter()
                .find_map(|(pane_id, content)| {
                    (content == PaneContent::Shell(shell_id)).then_some(pane_id)
                });
            if let Some(pane_id) = pane
                && workspace.close(pane_id)
            {
                removed += 1;
            }
        }
        removed
    }

    pub fn remove_session(&mut self, session_id: u64) -> Vec<u64> {
        let mut shells = Vec::new();
        if let Some(workspace) = self.workspaces.remove(&session_id) {
            shells.extend(workspace.panes().into_iter().filter_map(|(_, content)| {
                if let PaneContent::Shell(id) = content {
                    Some(id)
                } else {
                    None
                }
            }));
        }

        for workspace in self.workspaces.values_mut() {
            let comparison = workspace
                .panes()
                .into_iter()
                .find_map(|(pane_id, content)| {
                    (content == PaneContent::Session(session_id)).then_some(pane_id)
                });
            if let Some(pane_id) = comparison {
                workspace.close(pane_id);
            }
        }

        shells.sort_unstable();
        shells.dedup();
        shells
    }

    fn allocate_pane_id(&mut self) -> PaneId {
        let id = self.next_pane_id;
        self.next_pane_id += 1;
        id
    }
}
