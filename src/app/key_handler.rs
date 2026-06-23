use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::conversation::ConversationViewState;
use crate::app::diff::DiffViewState;
use crate::config::Keybindings;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyAction {
    Consumed,
    Close,
    PasteSelection(String),
    SelectionEmpty,
    ConfirmQuit,
}

pub fn handle_diff_key(
    key: KeyEvent,
    diff: &mut DiffViewState,
    keys: &Keybindings,
    vp: usize,
) -> KeyAction {
    if diff.is_searching() {
        match key.code {
            KeyCode::Char(c) if key.modifiers.is_empty() => {
                diff.search_insert(c, vp);
                KeyAction::Consumed
            }
            KeyCode::Backspace => {
                diff.search_backspace(vp);
                KeyAction::Consumed
            }
            KeyCode::Esc => {
                diff.cancel_search();
                KeyAction::Consumed
            }
            KeyCode::Enter => {
                diff.confirm_search();
                KeyAction::Consumed
            }
            _ => KeyAction::Consumed,
        }
    } else {
        match key.code {
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                diff.move_cursor_up(vp, vp);
                KeyAction::Consumed
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                diff.move_cursor_down(vp, vp);
                KeyAction::Consumed
            }
            KeyCode::Char('y') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                diff.scroll_view_up(vp, vp);
                KeyAction::Consumed
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                diff.scroll_view_down(vp, vp);
                KeyAction::Consumed
            }
            KeyCode::Char(c) if c == keys.diff || c == keys.quit => KeyAction::Close,
            KeyCode::Esc => {
                if diff.is_visual() {
                    diff.cancel_visual();
                    KeyAction::Consumed
                } else {
                    KeyAction::Close
                }
            }
            KeyCode::Char('v') => {
                diff.toggle_visual();
                KeyAction::Consumed
            }
            KeyCode::Enter => {
                if diff.is_visual() {
                    let action = match diff.format_selection() {
                        Some(text) => KeyAction::PasteSelection(text),
                        None => KeyAction::SelectionEmpty,
                    };
                    diff.cancel_visual();
                    action
                } else {
                    KeyAction::Consumed
                }
            }
            KeyCode::Char('/') => {
                if !diff.is_visual() {
                    diff.start_search();
                }
                KeyAction::Consumed
            }
            KeyCode::Char('g') => {
                diff.move_cursor_to_top(vp);
                KeyAction::Consumed
            }
            KeyCode::Char('G') => {
                diff.move_cursor_to_end(vp);
                KeyAction::Consumed
            }
            KeyCode::Char('j') | KeyCode::Down => {
                diff.move_cursor_down(1, vp);
                KeyAction::Consumed
            }
            KeyCode::Char('k') | KeyCode::Up => {
                diff.move_cursor_up(1, vp);
                KeyAction::Consumed
            }
            KeyCode::Char('n') => {
                diff.next_match(vp);
                KeyAction::Consumed
            }
            KeyCode::Char('N') => {
                diff.prev_match(vp);
                KeyAction::Consumed
            }
            KeyCode::PageUp => {
                diff.move_cursor_up(vp, vp);
                KeyAction::Consumed
            }
            KeyCode::PageDown => {
                diff.move_cursor_down(vp, vp);
                KeyAction::Consumed
            }
            _ => KeyAction::Consumed,
        }
    }
}

pub fn handle_conversation_key(
    key: KeyEvent,
    conv: &mut ConversationViewState,
    keys: &Keybindings,
    vp: usize,
) -> KeyAction {
    if conv.is_searching() {
        match key.code {
            KeyCode::Char(c) if key.modifiers.is_empty() => {
                conv.search_insert(c, vp);
                KeyAction::Consumed
            }
            KeyCode::Backspace => {
                conv.search_backspace(vp);
                KeyAction::Consumed
            }
            KeyCode::Esc => {
                conv.cancel_search();
                KeyAction::Consumed
            }
            KeyCode::Enter => {
                conv.confirm_search();
                KeyAction::Consumed
            }
            _ => KeyAction::Consumed,
        }
    } else {
        match key.code {
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                conv.scroll_up(vp);
                KeyAction::Consumed
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                conv.scroll_down(vp, vp);
                KeyAction::Consumed
            }
            KeyCode::Char('y') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                conv.scroll_up(vp);
                KeyAction::Consumed
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                conv.scroll_down(vp, vp);
                KeyAction::Consumed
            }
            KeyCode::Char(c) if c == keys.quit => KeyAction::Close,
            KeyCode::Esc => KeyAction::Close,
            KeyCode::Char('j') | KeyCode::Down => {
                conv.scroll_down(1, vp);
                KeyAction::Consumed
            }
            KeyCode::Char('k') | KeyCode::Up => {
                conv.scroll_up(1);
                KeyAction::Consumed
            }
            KeyCode::Char('g') => {
                conv.scroll_to_top();
                KeyAction::Consumed
            }
            KeyCode::Char('G') => {
                conv.scroll_to_end(vp);
                KeyAction::Consumed
            }
            KeyCode::Char('n') => {
                conv.next_match(vp);
                KeyAction::Consumed
            }
            KeyCode::Char('N') => {
                conv.prev_match(vp);
                KeyAction::Consumed
            }
            KeyCode::Char('/') => {
                conv.start_search();
                KeyAction::Consumed
            }
            _ => KeyAction::Consumed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::diff::LineMeta;
    use ratatui::text::{Line, Span};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(ch: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL)
    }

    fn default_keys() -> Keybindings {
        Keybindings::default()
    }

    fn make_document(texts: &[&str]) -> Vec<Line<'static>> {
        texts.iter().map(|t| Line::from(Span::raw(t.to_string()))).collect()
    }

    fn diff_with_doc(texts: &[&str]) -> DiffViewState {
        let mut d = DiffViewState::default();
        d.open("test".into(), "Test".into(), "raw".into(), crate::app::focus::AppFocus::Sidebar);
        let doc = make_document(texts);
        let meta = vec![None; doc.len()];
        d.replace_document(doc, meta, 100);
        d
    }

    fn conv_with_doc(texts: &[&str]) -> ConversationViewState {
        let mut c = ConversationViewState::default();
        c.open("test".into(), "Test".into(), crate::app::focus::AppFocus::Sidebar);
        c.replace_document(make_document(texts), 100);
        c
    }

    #[test]
    fn diff_search_mode_captures_char_g() { let mut diff = diff_with_doc(&["hello world", "go go go"]); diff.start_search(); assert!(diff.is_searching()); handle_diff_key(key(KeyCode::Char('g')), &mut diff, &default_keys(), 100); assert_eq!(diff.search_query(), "g"); assert!(diff.is_searching()); }
    #[test]
    fn diff_search_mode_captures_char_j() { let mut diff = diff_with_doc(&["hello world", "jjj"]); diff.start_search(); handle_diff_key(key(KeyCode::Char('j')), &mut diff, &default_keys(), 100); assert_eq!(diff.search_query(), "j"); }
    #[test]
    fn diff_search_mode_captures_char_k() { let mut diff = diff_with_doc(&["hello world", "kkk"]); diff.start_search(); handle_diff_key(key(KeyCode::Char('k')), &mut diff, &default_keys(), 100); assert_eq!(diff.search_query(), "k"); }
    #[test]
    fn diff_search_mode_backspace_deletes() { let mut diff = diff_with_doc(&["hello world"]); diff.start_search(); diff.search_insert('a', 100); diff.search_insert('b', 100); assert_eq!(diff.search_query(), "ab"); handle_diff_key(key(KeyCode::Backspace), &mut diff, &default_keys(), 100); assert_eq!(diff.search_query(), "a"); }
    #[test]
    fn diff_search_mode_esc_cancels_search() { let mut diff = diff_with_doc(&["hello world"]); diff.start_search(); diff.search_insert('x', 100); handle_diff_key(key(KeyCode::Esc), &mut diff, &default_keys(), 100); assert!(!diff.is_searching()); assert!(diff.search_query().is_empty()); }
    #[test]
    fn diff_search_mode_enter_confirms_search() { let mut diff = diff_with_doc(&["hello world"]); diff.start_search(); diff.search_insert('h', 100); handle_diff_key(key(KeyCode::Enter), &mut diff, &default_keys(), 100); assert!(!diff.is_searching()); assert_eq!(diff.search_query(), "h"); }
    #[test]
    fn diff_normal_mode_g_goes_to_top() { let mut diff = diff_with_doc(&["a", "b", "c", "d", "e"]); diff.move_cursor_down(3, 100); assert_eq!(diff.cursor(), 3); handle_diff_key(key(KeyCode::Char('g')), &mut diff, &default_keys(), 100); assert_eq!(diff.cursor(), 0); }
    #[test]
    fn diff_normal_mode_g_goes_to_end() { let mut diff = diff_with_doc(&["a", "b", "c", "d", "e"]); assert_eq!(diff.cursor(), 0); handle_diff_key(key(KeyCode::Char('G')), &mut diff, &default_keys(), 100); assert_eq!(diff.cursor(), 4); }
    #[test]
    fn diff_normal_mode_j_moves_down() { let mut diff = diff_with_doc(&["a", "b", "c"]); assert_eq!(diff.cursor(), 0); handle_diff_key(key(KeyCode::Char('j')), &mut diff, &default_keys(), 100); assert_eq!(diff.cursor(), 1); }
    #[test]
    fn diff_normal_mode_k_moves_up() { let mut diff = diff_with_doc(&["a", "b", "c"]); diff.move_cursor_down(2, 100); handle_diff_key(key(KeyCode::Char('k')), &mut diff, &default_keys(), 100); assert_eq!(diff.cursor(), 1); }
    #[test]
    fn diff_normal_mode_slash_starts_search() { let mut diff = diff_with_doc(&["a", "b"]); assert!(!diff.is_searching()); handle_diff_key(key(KeyCode::Char('/')), &mut diff, &default_keys(), 100); assert!(diff.is_searching()); }
    #[test]
    fn diff_normal_mode_n_next_match() { let mut diff = diff_with_doc(&["aaa", "bbb", "aaa"]); diff.start_search(); for ch in "aaa".chars() { diff.search_insert(ch, 100); } diff.confirm_search(); assert!(!diff.is_searching()); assert_eq!(diff.match_status(), Some((1, 2))); handle_diff_key(key(KeyCode::Char('n')), &mut diff, &default_keys(), 100); assert_eq!(diff.current_match_index(), 1); }
    #[test]
    fn diff_normal_mode_n_prev_match() { let mut diff = diff_with_doc(&["aaa", "bbb", "aaa"]); diff.start_search(); for ch in "aaa".chars() { diff.search_insert(ch, 100); } diff.confirm_search(); handle_diff_key(key(KeyCode::Char('N')), &mut diff, &default_keys(), 100); assert_eq!(diff.current_match_index(), 1); }
    #[test]
    fn diff_v_toggles_visual() { let mut diff = diff_with_doc(&["a", "b"]); assert!(!diff.is_visual()); handle_diff_key(key(KeyCode::Char('v')), &mut diff, &default_keys(), 100); assert!(diff.is_visual()); handle_diff_key(key(KeyCode::Char('v')), &mut diff, &default_keys(), 100); assert!(!diff.is_visual()); }
    #[test]
    fn diff_enter_in_visual_mode_pastes_selection() { let mut diff = DiffViewState::default(); diff.open("test".into(), "Test".into(), "raw".into(), crate::app::focus::AppFocus::Sidebar); diff.replace_document(make_document(&["a", "b"]), vec![Some(LineMeta { filepath: "foo.rs".into(), new_line_no: Some(10), old_line_no: None }), Some(LineMeta { filepath: "foo.rs".into(), new_line_no: Some(11), old_line_no: None })], 100); diff.toggle_visual(); diff.move_cursor_down(1, 100); let action = handle_diff_key(key(KeyCode::Enter), &mut diff, &default_keys(), 100); assert_eq!(action, KeyAction::PasteSelection("foo.rs:10-11".to_string())); assert!(!diff.is_visual()); }
    #[test]
    fn diff_enter_outside_visual_mode_is_consumed() { let mut diff = diff_with_doc(&["a", "b"]); let action = handle_diff_key(key(KeyCode::Enter), &mut diff, &default_keys(), 100); assert_eq!(action, KeyAction::Consumed); }
    #[test]
    fn diff_slash_does_not_start_search_in_visual_mode() { let mut diff = diff_with_doc(&["a", "b"]); diff.toggle_visual(); assert!(diff.is_visual()); handle_diff_key(key(KeyCode::Char('/')), &mut diff, &default_keys(), 100); assert!(!diff.is_searching()); }
    #[test]
    fn diff_esc_cancels_visual_before_closing() { let mut diff = diff_with_doc(&["a", "b"]); diff.toggle_visual(); let action = handle_diff_key(key(KeyCode::Esc), &mut diff, &default_keys(), 100); assert_eq!(action, KeyAction::Consumed); assert!(!diff.is_visual()); }
    #[test]
    fn diff_esc_closes_when_not_visual_not_searching() { let mut diff = diff_with_doc(&["a", "b"]); let action = handle_diff_key(key(KeyCode::Esc), &mut diff, &default_keys(), 100); assert_eq!(action, KeyAction::Close); }
    #[test]
    fn diff_ctrl_u_scrolls_up() { let mut diff = diff_with_doc(&(0..50).map(|i| Box::leak(format!("line{i}").into_boxed_str()) as &str).collect::<Vec<_>>()); diff.move_cursor_down(30, 20); let before = diff.cursor(); handle_diff_key(ctrl('u'), &mut diff, &default_keys(), 20); assert!(diff.cursor() < before); }
    #[test]
    fn diff_ctrl_d_scrolls_down() { let mut diff = diff_with_doc(&(0..50).map(|i| Box::leak(format!("line{i}").into_boxed_str()) as &str).collect::<Vec<_>>()); let before = diff.cursor(); handle_diff_key(ctrl('d'), &mut diff, &default_keys(), 20); assert!(diff.cursor() > before); }
    #[test]
    fn diff_ctrl_y_scroll_view_up() { let mut diff = diff_with_doc(&(0..50).map(|i| Box::leak(format!("line{i}").into_boxed_str()) as &str).collect::<Vec<_>>()); diff.move_cursor_down(25, 20); let scroll_before = diff.scroll_offset(); handle_diff_key(ctrl('y'), &mut diff, &default_keys(), 20); assert!(diff.scroll_offset() < scroll_before || scroll_before == 0); }
    #[test]
    fn diff_ctrl_e_scroll_view_down() { let mut diff = diff_with_doc(&(0..50).map(|i| Box::leak(format!("line{i}").into_boxed_str()) as &str).collect::<Vec<_>>()); let scroll_before = diff.scroll_offset(); handle_diff_key(ctrl('e'), &mut diff, &default_keys(), 20); assert!(diff.scroll_offset() > scroll_before); }
    #[test]
    fn diff_keybinding_diff_closes() { let mut diff = diff_with_doc(&["a"]); let keys = default_keys(); let action = handle_diff_key(key(KeyCode::Char(keys.diff)), &mut diff, &keys, 100); assert_eq!(action, KeyAction::Close); }
    #[test]
    fn diff_keybinding_quit_closes() { let mut diff = diff_with_doc(&["a"]); let keys = default_keys(); let action = handle_diff_key(key(KeyCode::Char(keys.quit)), &mut diff, &keys, 100); assert_eq!(action, KeyAction::Close); }
    #[test]
    fn diff_up_arrow_moves_up() { let mut diff = diff_with_doc(&["a", "b", "c"]); diff.move_cursor_down(2, 100); handle_diff_key(key(KeyCode::Up), &mut diff, &default_keys(), 100); assert_eq!(diff.cursor(), 1); }
    #[test]
    fn diff_down_arrow_moves_down() { let mut diff = diff_with_doc(&["a", "b", "c"]); handle_diff_key(key(KeyCode::Down), &mut diff, &default_keys(), 100); assert_eq!(diff.cursor(), 1); }
    #[test]
    fn diff_page_up_scrolls() { let mut diff = diff_with_doc(&(0..50).map(|i| Box::leak(format!("line{i}").into_boxed_str()) as &str).collect::<Vec<_>>()); diff.move_cursor_down(30, 20); let before = diff.cursor(); handle_diff_key(key(KeyCode::PageUp), &mut diff, &default_keys(), 20); assert!(diff.cursor() < before); }
    #[test]
    fn diff_page_down_scrolls() { let mut diff = diff_with_doc(&(0..50).map(|i| Box::leak(format!("line{i}").into_boxed_str()) as &str).collect::<Vec<_>>()); let before = diff.cursor(); handle_diff_key(key(KeyCode::PageDown), &mut diff, &default_keys(), 20); assert!(diff.cursor() > before); }

    #[test]
    fn conversation_has_scroll_offset() { let conv = conv_with_doc(&["a", "b"]); assert_eq!(conv.scroll_offset(), 0); }

    #[test]
    fn conversation_search_mode_captures_char() { let mut conv = conv_with_doc(&["hello world"]); conv.start_search(); handle_conversation_key(key(KeyCode::Char('h')), &mut conv, &default_keys(), 100); assert_eq!(conv.search_query(), "h"); }

    #[test]
    fn conversation_search_mode_backspace_deletes() { let mut conv = conv_with_doc(&["hello world"]); conv.start_search(); conv.search_insert('a', 100); conv.search_insert('b', 100); handle_conversation_key(key(KeyCode::Backspace), &mut conv, &default_keys(), 100); assert_eq!(conv.search_query(), "a"); }

    #[test]
    fn conversation_search_mode_esc_cancels() { let mut conv = conv_with_doc(&["hello world"]); conv.start_search(); conv.search_insert('x', 100); handle_conversation_key(key(KeyCode::Esc), &mut conv, &default_keys(), 100); assert!(!conv.is_searching()); assert!(conv.search_query().is_empty()); }

    #[test]
    fn conversation_search_mode_enter_confirms() { let mut conv = conv_with_doc(&["hello world"]); conv.start_search(); conv.search_insert('x', 100); handle_conversation_key(key(KeyCode::Enter), &mut conv, &default_keys(), 100); assert!(!conv.is_searching()); assert_eq!(conv.search_query(), "x"); }

    #[test]
    fn conversation_j_moves_down() { let mut conv = conv_with_doc(&(0..50).map(|i| Box::leak(format!("line{i}").into_boxed_str()) as &str).collect::<Vec<_>>()); handle_conversation_key(key(KeyCode::Char('j')), &mut conv, &default_keys(), 20); assert_eq!(conv.scroll_offset(), 1); }

    #[test]
    fn conversation_k_moves_up() { let mut conv = conv_with_doc(&(0..50).map(|i| Box::leak(format!("line{i}").into_boxed_str()) as &str).collect::<Vec<_>>()); conv.scroll_down(2, 20); handle_conversation_key(key(KeyCode::Char('k')), &mut conv, &default_keys(), 20); assert_eq!(conv.scroll_offset(), 1); }

    #[test]
    fn conversation_slash_starts_search() { let mut conv = conv_with_doc(&["a"]); handle_conversation_key(key(KeyCode::Char('/')), &mut conv, &default_keys(), 100); assert!(conv.is_searching()); }

    #[test]
    fn conversation_quit_closes() { let mut conv = conv_with_doc(&["a"]); let action = handle_conversation_key(key(KeyCode::Char(default_keys().quit)), &mut conv, &default_keys(), 100); assert_eq!(action, KeyAction::Close); }
}
