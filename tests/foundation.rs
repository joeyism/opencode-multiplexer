use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use opencode_multiplexer::{
    app::{Action, focus::AppFocus, reducer::reduce, state::AppState},
    terminal::{
        color::{ansi_named_color, convert_color},
        input::key_event_to_bytes,
        renderer::TerminalWidget,
        surface::TerminalSurface,
    },
    ui::{
        layout::split_root,
        sidebar::{
            SidebarEntry, flatten_sidebar_entries, render_sidebar, sidebar_row_modifier,
            sidebar_row_style,
        },
    },
};
use ratatui::{Terminal, backend::TestBackend, layout::Rect, style::Color};
use std::collections::HashSet;

#[test]
fn collapsed_sidebar_render_keeps_time_visible_inside_border() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let entries = vec![SidebarEntry {
        top_level_id: 1,
        session_id: Some("sess_1".into()),
        cwd: std::path::PathBuf::from("/tmp/delorean"),
        title: "ADO-2228 build flux".into(),
        status: opencode_multiplexer::app::sessions::SessionStatus::Working,
        time_updated: Some(now - 30),
        active: true,
        origin: opencode_multiplexer::app::sessions::SessionOrigin::Managed,
        has_children: false,
        children: vec![],
    }];

    let rows = flatten_sidebar_entries(&entries, &HashSet::new());
    let backend = TestBackend::new(16, 4);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget(
                render_sidebar(&rows, 0, AppFocus::Sidebar, 14, true),
                Rect::new(0, 0, 16, 4),
            );
        })
        .unwrap();

    let rendered_lines: Vec<String> = (0..4)
        .map(|y| {
            (0..16)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect()
        })
        .collect();
    let row = rendered_lines
        .iter()
        .find(|line| line.contains("1m"))
        .unwrap();
    assert!(row.contains("1m"));
    assert!(row.trim_end_matches('│').trim_end().ends_with("1m"));
}

#[test]
fn expanded_sidebar_render_right_aligns_time_within_content_width() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let entries = vec![SidebarEntry {
        top_level_id: 1,
        session_id: Some("sess_1".into()),
        cwd: std::path::PathBuf::from("/tmp/delorean"),
        title: "ADO-2228 build flux capacitor".into(),
        status: opencode_multiplexer::app::sessions::SessionStatus::Working,
        time_updated: Some(now - 120),
        active: false,
        origin: opencode_multiplexer::app::sessions::SessionOrigin::Managed,
        has_children: false,
        children: vec![],
    }];

    let rows = flatten_sidebar_entries(&entries, &HashSet::new());
    let backend = TestBackend::new(22, 4);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget(
                render_sidebar(&rows, 0, AppFocus::Sidebar, 20, true),
                Rect::new(0, 0, 22, 4),
            );
        })
        .unwrap();

    let rendered_lines: Vec<String> = (0..4)
        .map(|y| {
            (0..22)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect()
        })
        .collect();
    assert!(rendered_lines.iter().any(|line| line.contains("2m")));
    let row = rendered_lines
        .iter()
        .find(|line| line.contains("2m"))
        .unwrap();
    assert!(row.trim_end_matches('│').trim_end().ends_with("2m"));
}

#[test]
fn active_sidebar_rows_do_not_get_special_styling() {
    let modifier = sidebar_row_modifier();
    let style = sidebar_row_style(false, true);

    assert_eq!(modifier, ratatui::style::Modifier::empty());
    assert_eq!(style.fg, None);
    assert_eq!(style.bg, None);
    assert_eq!(style.add_modifier, ratatui::style::Modifier::empty());
}

#[test]
fn selected_row_does_not_restyle_status_dot() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let entries = vec![SidebarEntry {
        top_level_id: 1,
        session_id: Some("sess_1".into()),
        cwd: std::path::PathBuf::from("/tmp/delorean"),
        title: "ADO-2228 build flux capacitor".into(),
        status: opencode_multiplexer::app::sessions::SessionStatus::Working,
        time_updated: Some(now - 120),
        active: false,
        origin: opencode_multiplexer::app::sessions::SessionOrigin::Managed,
        has_children: false,
        children: vec![],
    }];

    let rows = flatten_sidebar_entries(&entries, &HashSet::new());
    let backend = TestBackend::new(22, 4);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget(
                render_sidebar(&rows, 0, AppFocus::Sidebar, 20, true),
                Rect::new(0, 0, 22, 4),
            );
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(0, 1)].symbol(), "●");
    assert_eq!(buffer[(0, 1)].fg, Color::Green);
    assert_eq!(buffer[(0, 1)].bg, Color::Reset);
}

#[test]
fn split_root_reserves_sidebar_and_footer() {
    let layout = split_root(Rect::new(0, 0, 120, 40), 28, 1);

    assert_eq!(layout.sidebar.width, 28);
    assert_eq!(layout.footer.height, 1);
    assert_eq!(layout.main.x, 28);
    assert_eq!(layout.main.width, 92);
}

#[test]
fn ctrl_backslash_toggles_focus_between_sidebar_and_terminal() {
    let mut state = AppState::default();

    reduce(&mut state, Action::ToggleFocus);
    assert_eq!(state.focus, AppFocus::Terminal);

    reduce(&mut state, Action::ToggleFocus);
    assert_eq!(state.focus, AppFocus::Sidebar);
}

#[test]
fn toggle_panel_hidden_action_flips_panel_state_and_focus() {
    let mut state = AppState::default();

    reduce(&mut state, Action::TogglePanelHidden);
    assert!(state.panel_hidden);
    assert_eq!(state.focus, AppFocus::Terminal);

    reduce(&mut state, Action::TogglePanelHidden);
    assert!(!state.panel_hidden);
    assert_eq!(state.focus, AppFocus::Sidebar);
}

#[test]
fn toggle_help_action_flips_help_overlay_state() {
    let mut state = AppState::default();

    reduce(&mut state, Action::ToggleHelp);
    assert!(state.show_help);

    reduce(&mut state, Action::ToggleHelp);
    assert!(!state.show_help);
}

#[test]
fn ansi_named_and_rgb_colors_map_to_ratatui_colors() {
    assert_eq!(ansi_named_color(2), Color::Green);
    assert_eq!(convert_color(1, 2, 3), Color::Rgb(1, 2, 3));
}

#[test]
fn surface_processes_ansi_text_into_render_snapshot() {
    let mut surface = TerminalSurface::new(4, 12);
    surface.process(b"\x1b[31mhi\x1b[0m there");

    let snapshot = surface.snapshot();
    assert_eq!(snapshot[0][0].symbol, "h");
    assert_eq!(snapshot[0][0].fg, Color::Red);
    assert_eq!(snapshot[0][1].symbol, "i");
    assert_eq!(snapshot[0][3].symbol, "t");
}

#[test]
fn widget_renders_surface_content_into_buffer() {
    let mut surface = TerminalSurface::new(3, 10);
    surface.process(b"ocmux");

    let backend = TestBackend::new(20, 8);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            let widget = TerminalWidget::new(&surface);
            frame.render_widget(widget, Rect::new(0, 0, 10, 3));
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(0, 0)].symbol(), "o");
    assert_eq!(buffer[(1, 0)].symbol(), "c");
    assert_eq!(buffer[(2, 0)].symbol(), "m");
    assert_eq!(buffer[(3, 0)].symbol(), "u");
    assert_eq!(buffer[(4, 0)].symbol(), "x");
}

#[test]
fn key_events_translate_into_pty_bytes() {
    assert_eq!(
        key_event_to_bytes(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)).as_deref(),
        Some(&b"a"[..])
    );
    assert_eq!(
        key_event_to_bytes(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)).as_deref(),
        Some(&b"\r"[..])
    );
    assert_eq!(
        key_event_to_bytes(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)).as_deref(),
        Some(&[0x7f][..])
    );
    assert_eq!(
        key_event_to_bytes(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)).as_deref(),
        Some(&b"\x1b[D"[..])
    );
}

#[test]
fn surface_clamps_zero_dimensions() {
    let surface = TerminalSurface::new(0, 0);

    assert_eq!(surface.rows(), 1);
    assert_eq!(surface.cols(), 1);
}

#[test]
fn subagents_working_status_uses_cyan_dot() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let entries = vec![SidebarEntry {
        top_level_id: 1,
        session_id: Some("sess_1".into()),
        cwd: std::path::PathBuf::from("/tmp/delorean"),
        title: "Subagent Test".into(),
        status: opencode_multiplexer::app::sessions::SessionStatus::SubagentsWorking,
        time_updated: Some(now - 30),
        active: true,
        origin: opencode_multiplexer::app::sessions::SessionOrigin::Managed,
        has_children: true,
        children: vec![],
    }];

    let rows = flatten_sidebar_entries(&entries, &HashSet::new());
    let backend = TestBackend::new(30, 4);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget(
                render_sidebar(&rows, 0, AppFocus::Sidebar, 25, true),
                Rect::new(0, 0, 30, 4),
            );
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    // Dot is usually at (0, 1) in expanded sidebar (indent 0)
    assert_eq!(buffer[(0, 1)].symbol(), "●");
    assert_eq!(buffer[(0, 1)].fg, Color::Cyan);
}

#[test]
fn subagents_working_status_uses_cyan_dot_when_collapsed() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let entries = vec![SidebarEntry {
        top_level_id: 1,
        session_id: Some("sess_1".into()),
        cwd: std::path::PathBuf::from("/tmp/delorean"),
        title: "Subagent Test".into(),
        status: opencode_multiplexer::app::sessions::SessionStatus::SubagentsWorking,
        time_updated: Some(now - 30),
        active: true,
        origin: opencode_multiplexer::app::sessions::SessionOrigin::Managed,
        has_children: false,
        children: vec![],
    }];

    let rows = flatten_sidebar_entries(&entries, &HashSet::new());
    let backend = TestBackend::new(20, 4);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget(
                render_sidebar(&rows, 0, AppFocus::Sidebar, 15, true),
                Rect::new(0, 0, 20, 4),
            );
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(0, 1)].symbol(), "●");
    assert_eq!(buffer[(0, 1)].fg, Color::Cyan);
}

#[test]
fn subagents_working_status_stays_cyan_when_selected() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let entries = vec![SidebarEntry {
        top_level_id: 1,
        session_id: Some("sess_1".into()),
        cwd: std::path::PathBuf::from("/tmp/delorean"),
        title: "Subagent Test".into(),
        status: opencode_multiplexer::app::sessions::SessionStatus::SubagentsWorking,
        time_updated: Some(now - 30),
        active: false,
        origin: opencode_multiplexer::app::sessions::SessionOrigin::Managed,
        has_children: false,
        children: vec![],
    }];

    let rows = flatten_sidebar_entries(&entries, &HashSet::new());
    let backend = TestBackend::new(30, 4);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget(
                render_sidebar(&rows, 0, AppFocus::Sidebar, 25, true),
                Rect::new(0, 0, 30, 4),
            );
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    // The dot at (0, 1) should be Cyan, even though the row is selected (Cyan background)
    // Actually, sidebar_row_style sets fg(Color::Black).bg(Color::Cyan) for selected row.
    // BUT render_row renders the dot Span SEPARATELY from the label Span.
    // Dot span: Span::styled(format!("{symbol} "), Style::default().fg(color))
    // Label span: Span::styled(left, row_style)
    // So the dot should NOT have the selection background if it's not part of the label style.
    // Let's verify this.
    assert_eq!(buffer[(0, 1)].symbol(), "●");
    assert_eq!(buffer[(0, 1)].fg, Color::Cyan);
    assert_eq!(buffer[(0, 1)].bg, Color::Reset);
    
    // The label starts at (2, 1) because of symbol + " "
    // Prefix for top-level is "  " (active=false, has_children=false)
    // So label content starts at (4, 1)?
    // Prefix is format!("{indent}{active_prefix}{marker}");
    // indent="", active_prefix="  ", marker="  " -> "    "
    // So symbol "●" at (0, 1), " " at (1, 1), "    " at (2, 1) to (5, 1).
    // Let's check (6, 1) for label content.
    assert_eq!(buffer[(6, 1)].bg, Color::Cyan);
    assert_eq!(buffer[(6, 1)].fg, Color::Black);
}
