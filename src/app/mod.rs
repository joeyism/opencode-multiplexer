pub mod conversation;
pub mod diff;
pub mod focus;
pub mod key_handler;
pub mod message_picker;
pub mod reducer;
pub mod session_picker;
pub mod sessions;
pub mod state;

pub use focus::AppFocus;
pub use reducer::reduce;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    ToggleFocus,
    SetFocus(AppFocus),
    ToggleHelp,
    ToggleSidebarCollapse,
    SelectNextRow,
    SelectPrevRow,
    ToggleExpandSelected(String),
    SetSelectedRow(usize),
    TogglePanelHidden,
}
