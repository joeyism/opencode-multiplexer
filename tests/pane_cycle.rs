use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use opencode_multiplexer::{
    app::{
        focus::AppFocus,
        pane_cycle::{is_pane_cycle_chord, pane_cycle_direction},
        workspace::PaneCycleDirection,
    },
    config::{KeyChord, Keybindings},
};

fn chord(ch: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL | KeyModifiers::ALT)
}

#[test]
fn ctrl_alt_j_cycles_next_from_live_pane() {
    assert_eq!(
        pane_cycle_direction(
            chord('j'),
            &Keybindings::default(),
            AppFocus::Terminal,
            false
        ),
        Some(PaneCycleDirection::Next)
    );
}

#[test]
fn ctrl_alt_k_cycles_previous_from_live_pane() {
    assert_eq!(
        pane_cycle_direction(
            chord('k'),
            &Keybindings::default(),
            AppFocus::Terminal,
            false
        ),
        Some(PaneCycleDirection::Previous)
    );
}

#[test]
fn pane_cycle_is_noop_from_sidebar() {
    assert_eq!(
        pane_cycle_direction(
            chord('j'),
            &Keybindings::default(),
            AppFocus::Sidebar,
            false
        ),
        None
    );
}

#[test]
fn configured_pane_chord_is_recognized_from_sidebar_for_consumption() {
    assert!(is_pane_cycle_chord(chord('j'), &Keybindings::default()));
}

#[test]
fn pane_cycle_is_noop_from_overlays() {
    let keys = Keybindings::default();
    for focus in [AppFocus::Conversation, AppFocus::Diff, AppFocus::Agents] {
        assert_eq!(pane_cycle_direction(chord('j'), &keys, focus, false), None);
    }
}

#[test]
fn pane_cycle_is_noop_when_modal_is_open() {
    assert_eq!(
        pane_cycle_direction(
            chord('j'),
            &Keybindings::default(),
            AppFocus::Terminal,
            true
        ),
        None
    );
}

#[test]
fn pane_cycle_respects_custom_chords() {
    let keys = Keybindings {
        pane_next: KeyChord::parse("ctrl-alt-n").unwrap(),
        pane_prev: KeyChord::parse("ctrl-alt-p").unwrap(),
        ..Keybindings::default()
    };

    assert_eq!(
        pane_cycle_direction(chord('n'), &keys, AppFocus::Terminal, false),
        Some(PaneCycleDirection::Next)
    );
    assert_eq!(
        pane_cycle_direction(chord('j'), &keys, AppFocus::Terminal, false),
        None
    );
}

#[test]
fn plain_j_and_k_are_not_pane_cycle_shortcuts() {
    let keys = Keybindings::default();
    for ch in ['j', 'k'] {
        assert_eq!(
            pane_cycle_direction(
                KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
                &keys,
                AppFocus::Terminal,
                false,
            ),
            None
        );
    }
}
