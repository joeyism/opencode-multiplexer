use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use opencode_multiplexer::app::help::{HelpAction, HelpState, handle_help_key};

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

#[test]
fn scroll_down_and_up_move_one_line() {
    let mut help = HelpState::default();
    help.set_scroll_offset(10, 60, 20);

    assert_eq!(
        handle_help_key(
            key(KeyCode::Char('j'), KeyModifiers::NONE),
            &mut help,
            '?',
            60,
            20
        ),
        HelpAction::Consumed
    );
    assert_eq!(help.scroll_offset(), 11);
    handle_help_key(key(KeyCode::Up, KeyModifiers::NONE), &mut help, '?', 60, 20);
    assert_eq!(help.scroll_offset(), 10);
}

#[test]
fn scroll_clamps_at_top_and_bottom() {
    let mut help = HelpState::default();
    help.scroll_by(-5, 60, 20);
    assert_eq!(help.scroll_offset(), 0);
    help.scroll_by(100, 60, 20);
    assert_eq!(help.scroll_offset(), 40);
}

#[test]
fn page_down_moves_half_a_viewport() {
    let mut help = HelpState::default();
    handle_help_key(
        key(KeyCode::PageDown, KeyModifiers::NONE),
        &mut help,
        '?',
        100,
        20,
    );
    assert_eq!(help.scroll_offset(), 10);
}

#[test]
fn g_and_shift_g_jump_to_ends() {
    let mut help = HelpState::default();
    handle_help_key(
        key(KeyCode::Char('G'), KeyModifiers::SHIFT),
        &mut help,
        '?',
        60,
        20,
    );
    assert_eq!(help.scroll_offset(), 40);
    handle_help_key(
        key(KeyCode::Char('g'), KeyModifiers::NONE),
        &mut help,
        '?',
        60,
        20,
    );
    assert_eq!(help.scroll_offset(), 0);
}

#[test]
fn esc_q_and_help_key_request_close() {
    for key in [
        key(KeyCode::Esc, KeyModifiers::NONE),
        key(KeyCode::Char('q'), KeyModifiers::NONE),
        key(KeyCode::Char('?'), KeyModifiers::SHIFT),
    ] {
        let mut help = HelpState::default();
        assert_eq!(
            handle_help_key(key, &mut help, '?', 60, 20),
            HelpAction::Close
        );
    }
}

#[test]
fn other_characters_are_consumed_without_closing() {
    let mut help = HelpState::default();
    assert_eq!(
        handle_help_key(
            key(KeyCode::Char('x'), KeyModifiers::NONE),
            &mut help,
            '?',
            60,
            20
        ),
        HelpAction::Consumed
    );
    assert_eq!(help.scroll_offset(), 0);
}

#[test]
fn reclamp_after_viewport_grows() {
    let mut help = HelpState::default();
    help.set_scroll_offset(40, 60, 20);
    help.reclamp(60, 30);
    assert_eq!(help.scroll_offset(), 30);
}
