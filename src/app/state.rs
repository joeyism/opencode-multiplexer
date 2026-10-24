use crate::app::agents::AgentsViewState;
use crate::app::focus::AppFocus;
use crate::app::message_picker::MessagePickerState;
use crate::app::playbook::{ParamFormState, PlaybookPickerState, PlaybookRun};
use crate::app::session_manager::SessionManagerState;
use crate::app::session_picker::SessionPickerState;
use std::collections::{HashMap, HashSet};

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
    pub agents: AgentsViewState,
    pub playbook_picker: Option<PlaybookPickerState>,
    pub playbook_form: Option<ParamFormState>,
    pub playbook_abort_for: Option<String>,
    pub playbook_runs: HashMap<String, PlaybookRun>,
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
            agents: AgentsViewState::default(),
            playbook_picker: None,
            playbook_form: None,
            playbook_abort_for: None,
            playbook_runs: HashMap::new(),
        }
    }
}
