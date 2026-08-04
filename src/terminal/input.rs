use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};

pub fn key_event_to_bytes(event: KeyEvent) -> Option<Vec<u8>> {
    // Don't forward the focus toggle (Ctrl-\, reported as Ctrl-4)
    if event.code == KeyCode::Char('4') && event.modifiers.contains(KeyModifiers::CONTROL) {
        return None;
    }
    // Don't forward the panel toggle (Ctrl-H)
    if event.code == KeyCode::Char('h') && event.modifiers.contains(KeyModifiers::CONTROL) {
        return None;
    }

    let mods = event.modifiers;
    let has_alt = mods.contains(KeyModifiers::ALT);
    let has_ctrl = mods.contains(KeyModifiers::CONTROL);
    let has_shift = mods.contains(KeyModifiers::SHIFT);

    match event.code {
        KeyCode::Char(ch) => {
            if has_ctrl {
                let mut bytes = ctrl_char(ch)?;
                if has_alt {
                    bytes.insert(0, 0x1b);
                }
                Some(bytes)
            } else if has_alt {
                let mut bytes = vec![0x1b];
                bytes.extend(ch.to_string().as_bytes());
                Some(bytes)
            } else {
                Some(ch.to_string().into_bytes())
            }
        }
        KeyCode::Enter => Some(vec![b'\r']),
        KeyCode::Tab if has_shift => Some(b"\x1b[Z".to_vec()),
        KeyCode::Tab => Some(vec![b'\t']),
        KeyCode::Backspace => Some(vec![0x7f]),
        KeyCode::Esc => Some(vec![0x1b]),
        KeyCode::Delete => modified_key(b"3", mods),
        KeyCode::Insert => modified_key(b"2", mods),
        KeyCode::Home => modified_special(b"H", mods),
        KeyCode::End => modified_special(b"F", mods),
        KeyCode::PageUp => modified_key(b"5", mods),
        KeyCode::PageDown => modified_key(b"6", mods),
        KeyCode::Up => modified_arrow(b'A', mods),
        KeyCode::Down => modified_arrow(b'B', mods),
        KeyCode::Right => modified_arrow(b'C', mods),
        KeyCode::Left => modified_arrow(b'D', mods),
        KeyCode::F(n) => f_key(n, mods),
        KeyCode::BackTab => Some(b"\x1b[Z".to_vec()),
        _ => None,
    }
}

fn ctrl_char(ch: char) -> Option<Vec<u8>> {
    let upper = ch.to_ascii_uppercase() as u8;
    if upper.is_ascii_uppercase() {
        Some(vec![upper - b'@'])
    } else {
        None
    }
}

fn modifier_param(mods: KeyModifiers) -> Option<u8> {
    let shift = mods.contains(KeyModifiers::SHIFT);
    let alt = mods.contains(KeyModifiers::ALT);
    let ctrl = mods.contains(KeyModifiers::CONTROL);
    let param = 1 + if shift { 1 } else { 0 } + if alt { 2 } else { 0 } + if ctrl { 4 } else { 0 };
    if param > 1 { Some(param) } else { None }
}

fn modified_arrow(arrow: u8, mods: KeyModifiers) -> Option<Vec<u8>> {
    match modifier_param(mods) {
        Some(m) => Some(format!("\x1b[1;{}{}", m, arrow as char).into_bytes()),
        None => Some(vec![0x1b, b'[', arrow]),
    }
}

fn modified_special(code: &[u8], mods: KeyModifiers) -> Option<Vec<u8>> {
    match modifier_param(mods) {
        Some(m) => {
            let mut seq = format!("\x1b[1;{m}").into_bytes();
            seq.extend_from_slice(code);
            Some(seq)
        }
        None => {
            let mut seq = vec![0x1b, b'['];
            seq.extend_from_slice(code);
            Some(seq)
        }
    }
}

fn modified_key(num: &[u8], mods: KeyModifiers) -> Option<Vec<u8>> {
    match modifier_param(mods) {
        Some(m) => {
            let mut seq = vec![0x1b, b'['];
            seq.extend_from_slice(num);
            seq.extend(format!(";{m}~").as_bytes());
            Some(seq)
        }
        None => {
            let mut seq = vec![0x1b, b'['];
            seq.extend_from_slice(num);
            seq.push(b'~');
            Some(seq)
        }
    }
}

fn f_key(n: u8, mods: KeyModifiers) -> Option<Vec<u8>> {
    let code = match n {
        1 => return modified_ss3(b'P', mods),
        2 => return modified_ss3(b'Q', mods),
        3 => return modified_ss3(b'R', mods),
        4 => return modified_ss3(b'S', mods),
        5 => b"15",
        6 => b"17",
        7 => b"18",
        8 => b"19",
        9 => b"20",
        10 => b"21",
        11 => b"23",
        12 => b"24",
        _ => return None,
    };
    modified_key(code, mods)
}

fn modified_ss3(letter: u8, mods: KeyModifiers) -> Option<Vec<u8>> {
    match modifier_param(mods) {
        Some(m) => Some(format!("\x1b[1;{}{}", m, letter as char).into_bytes()),
        None => Some(vec![0x1b, b'O', letter]),
    }
}

pub fn mouse_scroll_to_sgr_bytes(
    kind: MouseEventKind,
    col: u16,
    row: u16,
    modifiers: KeyModifiers,
) -> Option<Vec<u8>> {
    mouse_event_to_sgr_bytes(kind, col, row, modifiers)
}

pub fn mouse_event_to_sgr_bytes(
    kind: MouseEventKind,
    col: u16,
    row: u16,
    modifiers: KeyModifiers,
) -> Option<Vec<u8>> {
    let (base, trailer) = match kind {
        MouseEventKind::Down(MouseButton::Left) => (0u16, b'M'),
        MouseEventKind::Up(MouseButton::Left) => (0u16, b'm'),
        MouseEventKind::Down(MouseButton::Middle) => (1u16, b'M'),
        MouseEventKind::Up(MouseButton::Middle) => (1u16, b'm'),
        MouseEventKind::Down(MouseButton::Right) => (2u16, b'M'),
        MouseEventKind::Up(MouseButton::Right) => (2u16, b'm'),
        MouseEventKind::ScrollUp => (64u16, b'M'),
        MouseEventKind::ScrollDown => (65u16, b'M'),
        _ => return None,
    };

    let mut btn = base;
    if modifiers.contains(KeyModifiers::SHIFT) {
        btn += 4;
    }
    if modifiers.contains(KeyModifiers::ALT) {
        btn += 8;
    }
    if modifiers.contains(KeyModifiers::CONTROL) {
        btn += 16;
    }

    let col = col.max(1);
    let row = row.max(1);
    Some(format!("\x1b[<{btn};{col};{row}{}", trailer as char).into_bytes())
}

pub fn screen_to_pty_cell(
    screen_col: u16,
    screen_row: u16,
    pane_x: u16,
    pane_y: u16,
    pane_width: u16,
    pane_height: u16,
) -> Option<(u16, u16)> {
    if pane_width == 0 || pane_height == 0 {
        return None;
    }
    if screen_col < pane_x
        || screen_row < pane_y
        || screen_col >= pane_x.saturating_add(pane_width)
        || screen_row >= pane_y.saturating_add(pane_height)
    {
        return None;
    }
    let col = (screen_col - pane_x + 1).min(pane_width);
    let row = (screen_row - pane_y + 1).min(pane_height);
    Some((col, row))
}

pub fn mouse_click_press_release_sgr(
    col_1based: u16,
    row_1based: u16,
    modifiers: KeyModifiers,
) -> Vec<u8> {
    let mut out = mouse_event_to_sgr_bytes(
        MouseEventKind::Down(MouseButton::Left),
        col_1based,
        row_1based,
        modifiers,
    )
    .unwrap_or_default();
    out.extend(
        mouse_event_to_sgr_bytes(
            MouseEventKind::Up(MouseButton::Left),
            col_1based,
            row_1based,
            modifiers,
        )
        .unwrap_or_default(),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyModifiers, MouseEventKind};

    #[test]
    fn scroll_up_encodes_sgr_button_64() {
        let bytes = mouse_scroll_to_sgr_bytes(
            MouseEventKind::ScrollUp,
            3, // col 1-based
            5, // row 1-based
            KeyModifiers::NONE,
        )
        .unwrap();
        assert_eq!(bytes, b"\x1b[<64;3;5M");
    }

    #[test]
    fn scroll_down_encodes_sgr_button_65() {
        let bytes = mouse_scroll_to_sgr_bytes(MouseEventKind::ScrollDown, 1, 1, KeyModifiers::NONE)
            .unwrap();
        assert_eq!(bytes, b"\x1b[<65;1;1M");
    }

    #[test]
    fn scroll_with_ctrl_adds_16_to_button() {
        let bytes =
            mouse_scroll_to_sgr_bytes(MouseEventKind::ScrollUp, 2, 4, KeyModifiers::CONTROL)
                .unwrap();
        assert_eq!(bytes, b"\x1b[<80;2;4M"); // 64 + 16
    }

    #[test]
    fn non_scroll_kinds_return_none() {
        assert!(
            mouse_scroll_to_sgr_bytes(MouseEventKind::Moved, 1, 1, KeyModifiers::NONE,).is_none()
        );
    }

    #[test]
    fn screen_to_pty_cell_translates_and_clamps() {
        // pane at (10, 2), size 40x20
        assert_eq!(screen_to_pty_cell(10, 2, 10, 2, 40, 20), Some((1, 1)));
        assert_eq!(screen_to_pty_cell(19, 6, 10, 2, 40, 20), Some((10, 5)));
        // outside pane
        assert_eq!(screen_to_pty_cell(5, 6, 10, 2, 40, 20), None);
        assert_eq!(screen_to_pty_cell(10, 1, 10, 2, 40, 20), None);
        // clamp right/bottom edge inside pane to pane size
        assert_eq!(screen_to_pty_cell(49, 21, 10, 2, 40, 20), Some((40, 20)));
    }

    #[test]
    fn left_press_encodes_button_0_m_uppercase() {
        assert_eq!(
            mouse_event_to_sgr_bytes(
                MouseEventKind::Down(MouseButton::Left),
                4,
                7,
                KeyModifiers::NONE
            )
            .unwrap(),
            b"\x1b[<0;4;7M"
        );
    }

    #[test]
    fn left_release_encodes_button_0_m_lowercase() {
        assert_eq!(
            mouse_event_to_sgr_bytes(
                MouseEventKind::Up(MouseButton::Left),
                4,
                7,
                KeyModifiers::NONE
            )
            .unwrap(),
            b"\x1b[<0;4;7m"
        );
    }

    #[test]
    fn right_press_encodes_button_2() {
        assert_eq!(
            mouse_event_to_sgr_bytes(
                MouseEventKind::Down(MouseButton::Right),
                1,
                1,
                KeyModifiers::NONE
            )
            .unwrap(),
            b"\x1b[<2;1;1M"
        );
    }

    #[test]
    fn drag_returns_none() {
        assert!(
            mouse_event_to_sgr_bytes(
                MouseEventKind::Drag(MouseButton::Left),
                1,
                1,
                KeyModifiers::NONE
            )
            .is_none()
        );
    }
}
