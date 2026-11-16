use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    app::{focus::AppFocus, workspace::PaneCycleDirection},
    config::Keybindings,
};

pub fn is_pane_cycle_chord(key: KeyEvent, bindings: &Keybindings) -> bool {
    let KeyCode::Char(ch) = key.code else {
        return false;
    };
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    bindings.pane_next.matches(ctrl, alt, shift, ch)
        || bindings.pane_prev.matches(ctrl, alt, shift, ch)
}

pub fn pane_cycle_direction(
    key: KeyEvent,
    bindings: &Keybindings,
    focus: AppFocus,
    modal_open: bool,
) -> Option<PaneCycleDirection> {
    if modal_open || focus != AppFocus::Terminal {
        return None;
    }
    let KeyCode::Char(ch) = key.code else {
        return None;
    };
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    if bindings.pane_next.matches(ctrl, alt, shift, ch) {
        Some(PaneCycleDirection::Next)
    } else if bindings.pane_prev.matches(ctrl, alt, shift, ch) {
        Some(PaneCycleDirection::Previous)
    } else {
        None
    }
}
