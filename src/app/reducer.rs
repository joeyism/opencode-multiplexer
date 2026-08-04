use crate::app::{Action, focus::AppFocus, state::AppState};

pub fn reduce(state: &mut AppState, action: Action) {
    match action {
        Action::ToggleFocus => {
            state.focus = match state.focus {
                AppFocus::Sidebar => state.last_main_focus,
                AppFocus::Terminal | AppFocus::Conversation | AppFocus::Diff => {
                    if matches!(state.focus, AppFocus::Conversation | AppFocus::Diff) {
                        state.last_main_focus = AppFocus::Terminal;
                    }
                    AppFocus::Sidebar
                }
            };
        }
        Action::SetFocus(target) => {
            match target {
                AppFocus::Terminal | AppFocus::Conversation | AppFocus::Diff => {
                    state.last_main_focus = target;
                }
                AppFocus::Sidebar => {}
            }
            state.focus = target;
        }
        Action::ToggleHelp => {
            state.show_help = !state.show_help;
        }
        Action::SelectNextRow => {
            state.selected_sidebar_row = state.selected_sidebar_row.saturating_add(1);
        }
        Action::SelectPrevRow => {
            state.selected_sidebar_row = state.selected_sidebar_row.saturating_sub(1);
        }
        Action::ToggleExpandSelected(session_id) => {
            if !state.expanded_session_ids.remove(&session_id) {
                state.expanded_session_ids.insert(session_id);
            }
        }
        Action::SetSelectedRow(row) => {
            state.selected_sidebar_row = row;
        }
        Action::TogglePanelHidden => {
            if state.panel_hidden {
                state.panel_hidden = false;
                state.focus = AppFocus::Sidebar;
            } else {
                state.panel_hidden = true;
                state.focus = AppFocus::Terminal;
                state.last_main_focus = AppFocus::Terminal;
            }
        }
    }
}
