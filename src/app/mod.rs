pub mod agents;
pub mod conversation;
pub mod diff;
pub mod focus;
pub mod help;
pub mod key_handler;
pub mod message_picker;
pub mod pane_cycle;
pub mod playbook;
pub mod reducer;
pub mod session_cycle;
pub mod session_manager;
pub mod session_picker;
pub mod sessions;
pub mod split_picker;
pub mod state;
pub mod workspace;

pub use focus::AppFocus;
pub use reducer::reduce;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    ToggleFocus,
    SetFocus(AppFocus),
    ToggleHelp,
    SelectNextRow,
    SelectPrevRow,
    ToggleExpandSelected(String),
    SetSelectedRow(usize),
    TogglePanelHidden,
}
