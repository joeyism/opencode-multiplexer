use crate::app::focus::AppFocus;
use crate::app::message_picker::MessagePickerState;
use crate::app::session_manager::SessionManagerState;
use crate::app::session_picker::SessionPickerState;
use std::collections::HashSet;

pub struct AppState {
    pub focus: AppFocus,
    pub show_help: bool,
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
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            focus: AppFocus::default(),
            show_help: false,
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
        }
    }
}
