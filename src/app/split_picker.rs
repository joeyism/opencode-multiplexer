use std::collections::HashSet;

use crate::app::workspace::{PaneId, WorkspaceId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SplitChoice {
    Session { entry_id: u64, title: String },
    Shell,
}

pub fn build_split_choices(
    candidates: impl IntoIterator<Item = (u64, String)>,
    excluded: &HashSet<u64>,
) -> Vec<SplitChoice> {
    let mut choices = vec![SplitChoice::Shell];
    choices.extend(candidates.into_iter().filter_map(|(entry_id, title)| {
        (!excluded.contains(&entry_id)).then_some(SplitChoice::Session { entry_id, title })
    }));
    choices
}

#[derive(Debug, Clone)]
pub struct SplitPickerState {
    workspace_id: WorkspaceId,
    target_pane: PaneId,
    horizontal: bool,
    choices: Vec<SplitChoice>,
    selected: usize,
}

impl SplitPickerState {
    pub fn new(
        workspace_id: WorkspaceId,
        target_pane: PaneId,
        horizontal: bool,
        choices: Vec<SplitChoice>,
    ) -> Self {
        Self {
            workspace_id,
            target_pane,
            horizontal,
            choices,
            selected: 0,
        }
    }

    pub fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id
    }

    pub fn target_pane(&self) -> PaneId {
        self.target_pane
    }

    pub fn horizontal(&self) -> bool {
        self.horizontal
    }

    pub fn choices(&self) -> &[SplitChoice] {
        &self.choices
    }

    pub fn selected(&self) -> Option<&SplitChoice> {
        self.choices.get(self.selected)
    }

    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn move_down(&mut self) {
        if !self.choices.is_empty() {
            self.selected = (self.selected + 1) % self.choices.len();
        }
    }
}
