use std::{
    collections::HashSet,
    error::Error,
    time::{Duration, Instant},
};

use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, KeyEvent, KeyModifiers, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use opencode_multiplexer::{
    app::{
        Action, conversation::ConversationViewState, diff::DiffViewState, focus::AppFocus,
        message_picker::MessagePickerState, reducer::reduce, session_picker::SessionPickerState,
        sessions::SessionStatus, state::AppState,
    },
    config::load_config,
    data::{db::reader::DbReader, poller::start_poller},
    notify::Notifier,
    ops::git::{diff_worktree, fetch_session_diff_from_serve},
    ops::worktree::create_worktree,
    ops::{fzf::pick_directory, opencode::display_title_for_cwd},
    registry::save_managed_sessions,
    terminal::manager::PtyManager,
    ui::{
        conversation, diff as ui_diff, root,
        sidebar::{SidebarRowKind, flatten_sidebar_entries},
    },
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::path::PathBuf;

const FOOTER_HEIGHT: u16 = 2;
const COLLAPSED_SIDEBAR_WIDTH: u16 = 12;

fn main() -> Result<(), Box<dyn Error>> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableBracketedPaste,
        crossterm::event::EnableFocusChange
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run(&mut terminal);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableBracketedPaste,
        crossterm::event::DisableFocusChange
    )?;
    terminal.show_cursor()?;
    result
}

fn run(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> Result<(), Box<dyn Error>> {
    let config = load_config().unwrap_or_default();
    let mut notifier = Notifier::new(config.notifications);
    let _ = opencode_multiplexer::registry::cleanup_stale_serve_entries();
    let mut state = AppState::default();
    let mut manager = PtyManager::default();
    let mut footer_message: Option<String> = None;
    let mut conversation = ConversationViewState::default();
    let mut diff_view = DiffViewState::default();
    let mut mouse_captured = false;
    let (poll_tx, poll_rx) = std::sync::mpsc::channel();
    let poller = start_poller(poll_tx);

    let mut prev_selected_kind: Option<SidebarRowKind> = None;
    let result = (|| -> Result<(), Box<dyn Error>> {
        loop {
            while let Ok(snapshot) = poll_rx.try_recv() {
                // Capture old statuses keyed by session_id so we can diff them.
                let prev_statuses: std::collections::HashMap<String, SessionStatus> = manager
                    .sessions()
                    .items()
                    .iter()
                    .filter_map(|s| Some((s.session_id.clone()?, s.status)))
                    .collect();

                let registry_dirty = manager.apply_poll_snapshot(snapshot.clone());
                if registry_dirty {
                    let _ = save_managed_sessions(manager.managed_session_ids());
                }

                // Notify on interesting transitions when the app is not focused.
                if config.notifications && !state.app_focused {
                    for discovered in &snapshot.sessions {
                        if let Some(&prev_status) = prev_statuses.get(&discovered.session_id)
                            && Notifier::is_interesting_transition(prev_status, discovered.status)
                            && !notifier.is_on_cooldown(&discovered.session_id)
                            && let Some(summary) =
                                manager.sessions().items().iter().find(|s| {
                                    s.session_id.as_deref() == Some(&discovered.session_id)
                                })
                        {
                            let title = format!("ocmux: {}", summary.title);
                            let body = Notifier::format_body(discovered.status);
                            notifier.notify(&title, body);
                            notifier.record_notification(&discovered.session_id);
                        }
                    }
                }
            }

            let active_before = manager.active_id();
            let exited = manager.reap_exited_ptys();
            if active_before.is_some_and(|id| exited.contains(&id)) {
                state.focus = AppFocus::Sidebar;
                footer_message = Some("session exited".into());
            }

            manager.drain_all_output();
            let entries = manager.sidebar_entries();
            let rows = flatten_sidebar_entries(&entries, &state.expanded_session_ids);
            if !rows.is_empty() {
                if let Some(prev_kind) = prev_selected_kind.as_ref()
                    && let Some(new_index) = rows.iter().position(|r| &r.kind == prev_kind)
                {
                    state.selected_sidebar_row = new_index;
                }
                if state.selected_sidebar_row >= rows.len() {
                    state.selected_sidebar_row = rows.len() - 1;
                }
            }
            let sidebar_width = if state.panel_hidden {
                0
            } else if state.sidebar_collapsed {
                COLLAPSED_SIDEBAR_WIDTH
            } else {
                config.sidebar_width
            };

            let content_width = terminal.size()?.width.saturating_sub(sidebar_width);
            let viewport_height = terminal
                .size()
                .map(|s| s.height.saturating_sub(FOOTER_HEIGHT + 1))
                .unwrap_or(24) as usize;

            if state.focus == AppFocus::Conversation
                && conversation.should_poll(Instant::now())
                && let Some(session_id) = conversation.session_id().map(String::from)
            {
                conversation.mark_polled(Instant::now());
                match DbReader::open_default().and_then(|r| r.get_conversation(&session_id)) {
                    Ok(messages) => {
                        let doc = conversation::build_document(&messages, content_width);
                        conversation.replace_document(doc, viewport_height);
                        conversation.clear_error();
                    }
                    Err(e) => {
                        conversation.set_error(e.to_string());
                    }
                }
            }

            if let Some(picker) = state.session_picker.as_mut() {
                picker.tick();
            }
            if let Some(picker) = state.message_picker.as_mut() {
                picker.tick();
            }

            terminal.draw(|frame| {
                root::render(
                    frame,
                    state.focus,
                    state.selected_sidebar_row,
                    &rows,
                    &manager,
                    footer_message.as_deref(),
                    &config.keybindings,
                    state.show_help,
                    &state.show_files,
                    sidebar_width,
                    state.sidebar_collapsed,
                    state.panel_hidden,
                    state.app_focused,
                    &conversation,
                    &diff_view,
                    state.session_picker.as_mut(),
                    state.message_picker.as_mut(),
                    state.confirm_quit,
                )
            })?;

            if !event::poll(Duration::from_millis(16))? {
                continue;
            }

            match event::read()? {
                Event::Key(key) if state.message_picker.is_some() => match key.code {
                    KeyCode::Esc => {
                        state.message_picker = None;
                        footer_message = Some("history canceled".into());
                    }
                    KeyCode::Enter => {
                        let selected_text = state
                            .message_picker
                            .as_ref()
                            .and_then(|p| p.selected_entry())
                            .map(|entry| entry.text.clone());

                        if let Some(text) = selected_text {
                            state.message_picker = None;
                            state.last_main_focus = AppFocus::Terminal;
                            reduce(&mut state, Action::SetFocus(AppFocus::Terminal));

                            if let Some(pty) = manager.active_session_mut() {
                                match pty.send_paste(&text) {
                                    Ok(_) => footer_message = None,
                                    Err(error) => {
                                        footer_message = Some(format!("paste failed: {error}"));
                                    }
                                }
                            } else {
                                footer_message = Some("no active session to paste into".into());
                            }
                        }
                    }
                    KeyCode::Up => {
                        if let Some(picker) = state.message_picker.as_mut() {
                            picker.move_up();
                        }
                    }
                    KeyCode::Down => {
                        if let Some(picker) = state.message_picker.as_mut() {
                            picker.move_down();
                        }
                    }
                    KeyCode::Backspace => {
                        if let Some(picker) = state.message_picker.as_mut() {
                            picker.backspace();
                        }
                    }
                    KeyCode::Char(c) => {
                        if let Some(picker) = state.message_picker.as_mut() {
                            picker.insert_char(c);
                        }
                    }
                    _ => {}
                },
                Event::Key(key) if state.session_picker.is_some() => match key.code {
                    KeyCode::Esc => {
                        state.session_picker = None;
                        footer_message = Some("search canceled".into());
                    }
                    KeyCode::Enter => {
                        let entry = state
                            .session_picker
                            .as_ref()
                            .and_then(|p| p.selected_entry());
                        state.session_picker = None;
                        if let Some(entry) = entry {
                            match DbReader::open_default()
                                .and_then(|r| r.get_session_status(&entry.session_id))
                            {
                                Ok(status) => {
                                    let (rows, cols) =
                                        pane_size(terminal.size()?.into(), config.sidebar_width);
                                    match manager.attach_arbitrary_session(
                                        entry.session_id,
                                        entry.dir_path,
                                        entry.title.clone(),
                                        status,
                                        Some(entry.time_updated),
                                        rows,
                                        cols,
                                    ) {
                                        Ok(_) => {
                                            save_managed_sessions(manager.managed_session_ids())?;
                                            state.focus = AppFocus::Terminal;
                                            state.selected_sidebar_row = 0;
                                            footer_message = None;
                                        }
                                        Err(error) => {
                                            footer_message =
                                                Some(format!("attach failed: {error}"));
                                        }
                                    }
                                }
                                Err(error) => {
                                    footer_message = Some(format!("status lookup failed: {error}"));
                                }
                            }
                        }
                    }
                    KeyCode::Up => {
                        if let Some(picker) = state.session_picker.as_mut() {
                            picker.move_up();
                        }
                    }
                    KeyCode::Down => {
                        if let Some(picker) = state.session_picker.as_mut() {
                            picker.move_down();
                        }
                    }
                    KeyCode::Backspace => {
                        if let Some(picker) = state.session_picker.as_mut() {
                            picker.backspace();
                        }
                    }
                    KeyCode::Char(c) => {
                        if let Some(picker) = state.session_picker.as_mut() {
                            picker.insert_char(c);
                        }
                    }
                    _ => {}
                },
                Event::Key(key)
                    if key.code == KeyCode::Char(config.keybindings.help)
                        && !matches!(state.focus, AppFocus::Terminal) =>
                {
                    reduce(&mut state, Action::ToggleHelp)
                }
                Event::Key(key)
                    if state.show_help && matches!(key.code, KeyCode::Esc | KeyCode::Char(_)) =>
                {
                    state.show_help = false;
                }
                Event::Key(key)
                    if !state.show_files.is_empty()
                        && matches!(key.code, KeyCode::Esc | KeyCode::Char(_)) =>
                {
                    state.show_files.clear();
                }
                Event::Key(key) if state.confirm_quit => match key.code {
                    KeyCode::Char('y') => break,
                    KeyCode::Char('n') | KeyCode::Esc | KeyCode::Char('q') => {
                        state.confirm_quit = false;
                    }
                    _ => {}
                },
                Event::Key(key)
                    if is_panel_toggle(key)
                        && matches!(
                            state.focus,
                            AppFocus::Terminal
                                | AppFocus::Sidebar
                                | AppFocus::Diff
                                | AppFocus::Conversation
                        ) =>
                {
                    reduce(&mut state, Action::TogglePanelHidden);
                    let new_sidebar_width = if state.panel_hidden {
                        0
                    } else if state.sidebar_collapsed {
                        COLLAPSED_SIDEBAR_WIDTH
                    } else {
                        config.sidebar_width
                    };
                    let (pty_rows, pty_cols) =
                        pane_size(terminal.size()?.into(), new_sidebar_width);
                    if let Err(error) = manager.resize_active(pty_rows, pty_cols) {
                        footer_message = Some(format!("resize failed: {error}"));
                    }

                    if diff_view.is_active() {
                        let new_content_width =
                            terminal.size()?.width.saturating_sub(new_sidebar_width);
                        let new_vp =
                            terminal.size()?.height.saturating_sub(FOOTER_HEIGHT + 1) as usize;
                        let (doc, meta) =
                            ui_diff::build_diff_document(diff_view.raw_diff(), new_content_width);
                        diff_view.replace_document(doc, meta, new_vp);
                    }

                    if matches!(state.focus, AppFocus::Conversation) {
                        conversation.force_poll();
                    }
                }
                Event::Key(key) if is_focus_toggle(key) => {
                    if state.panel_hidden {
                        reduce(&mut state, Action::TogglePanelHidden);
                        let new_sidebar_width = if state.sidebar_collapsed {
                            COLLAPSED_SIDEBAR_WIDTH
                        } else {
                            config.sidebar_width
                        };
                        let (pty_rows, pty_cols) =
                            pane_size(terminal.size()?.into(), new_sidebar_width);
                        if let Err(error) = manager.resize_active(pty_rows, pty_cols) {
                            footer_message = Some(format!("resize failed: {error}"));
                        }
                    } else {
                        reduce(&mut state, Action::ToggleFocus);
                    }
                }
                Event::Key(key)
                    if matches!(state.focus, AppFocus::Sidebar)
                        && manager.pending_kill().is_some() =>
                {
                    match key.code {
                        KeyCode::Char('y') => {
                            let _ = manager.kill_selected()?;
                            save_managed_sessions(manager.managed_session_ids())?;
                            footer_message = Some("killed session".into());
                        }
                        KeyCode::Char('n') | KeyCode::Esc => {
                            manager.cancel_kill();
                            footer_message = None;
                        }
                        _ => {}
                    }
                }
                Event::Key(key) if matches!(state.focus, AppFocus::Sidebar) => match key.code {
                    KeyCode::Char(c) if c == config.keybindings.quit => {
                        state.confirm_quit = true;
                    }
                    KeyCode::Char(c) if c == config.keybindings.down => {
                        reduce(&mut state, Action::SelectNextRow)
                    }
                    KeyCode::Down => reduce(&mut state, Action::SelectNextRow),
                    KeyCode::Char(c) if c == config.keybindings.up => {
                        reduce(&mut state, Action::SelectPrevRow)
                    }
                    KeyCode::Up => reduce(&mut state, Action::SelectPrevRow),
                    KeyCode::Enter => {
                        let (pty_rows, pty_cols) =
                            pane_size(terminal.size()?.into(), config.sidebar_width);
                        if let Some(row) = rows.get(state.selected_sidebar_row) {
                            match &row.kind {
                                SidebarRowKind::TopLevel { top_level_id, .. } => {
                                    manager.select_top_level(*top_level_id);
                                    match manager.activate_or_attach_selected(pty_rows, pty_cols) {
                                        Ok(_) => {
                                            save_managed_sessions(manager.managed_session_ids())?;
                                            state.focus = AppFocus::Terminal;
                                            footer_message = None
                                        }
                                        Err(error) => {
                                            footer_message = Some(format!("attach failed: {error}"))
                                        }
                                    }
                                }
                                SidebarRowKind::Child { .. } => {
                                    footer_message = Some(
                                        "child rows are selectable; attach not wired yet".into(),
                                    );
                                }
                            }
                        }
                    }
                    KeyCode::Char(c) if c == config.keybindings.view => {
                        if let Some(row) = rows.get(state.selected_sidebar_row) {
                            match &row.kind {
                                SidebarRowKind::TopLevel {
                                    session_id: Some(sid),
                                    ..
                                } => {
                                    let title = row.title.clone();
                                    conversation.open(sid.clone(), title, AppFocus::Sidebar);
                                    reduce(&mut state, Action::SetFocus(AppFocus::Conversation));
                                    footer_message = None;
                                }
                                SidebarRowKind::Child { session_id } => {
                                    let title = row.title.clone();
                                    conversation.open(session_id.clone(), title, AppFocus::Sidebar);
                                    reduce(&mut state, Action::SetFocus(AppFocus::Conversation));
                                    footer_message = None;
                                }
                                _ => {
                                    footer_message = Some(
                                        "conversation view requires a session with a DB ID".into(),
                                    );
                                }
                            }
                        }
                    }
                    KeyCode::Char(c) if c == config.keybindings.files => {
                        if let Some(row) = rows.get(state.selected_sidebar_row) {
                            if let Some(sid) = row.session_id.as_deref() {
                                match DbReader::open_default()
                                    .and_then(|r| r.get_session_modified_files(sid))
                                {
                                    Ok(files) if files.is_empty() => {
                                        footer_message =
                                            Some("no files modified by this session".into());
                                    }
                                    Ok(files) => {
                                        state.show_files = files;
                                    }
                                    Err(e) => {
                                        footer_message = Some(format!("failed to read files: {e}"));
                                    }
                                }
                            } else {
                                footer_message = Some("no session ID for this row".into());
                            }
                        }
                    }
                    KeyCode::Char(c) if c == config.keybindings.diff => {
                        if let Some(row) = rows.get(state.selected_sidebar_row) {
                            if let Some(sid) = row.session_id.as_deref() {
                                match resolve_session_diff(row, sid) {
                                    Ok(diff) => {
                                        let title = row.title.clone();
                                        diff_view.open(
                                            sid.to_string(),
                                            title,
                                            diff,
                                            AppFocus::Sidebar,
                                        );
                                        let (doc, meta) = ui_diff::build_diff_document(
                                            diff_view.raw_diff(),
                                            content_width,
                                        );
                                        diff_view.replace_document(doc, meta, viewport_height);
                                        reduce(&mut state, Action::SetFocus(AppFocus::Diff));
                                        footer_message = None;
                                    }
                                    Err(msg) => {
                                        footer_message = Some(msg);
                                    }
                                }
                            } else {
                                footer_message = Some("no session ID for this row".into());
                            }
                        }
                    }
                    KeyCode::Char('/') => {
                        let live_ids: HashSet<String> = manager
                            .sidebar_entries()
                            .iter()
                            .filter_map(|e| e.session_id.clone())
                            .collect();
                        match SessionPickerState::load(live_ids) {
                            Ok(picker) if picker.total_count() > 0 => {
                                state.session_picker = Some(picker);
                            }
                            Ok(_) => {
                                footer_message = Some("no sessions found".into());
                            }
                            Err(error) => {
                                footer_message = Some(format!("search failed: {error}"));
                            }
                        }
                    }
                    KeyCode::Char(c) if c == config.keybindings.history => {
                        match MessagePickerState::load() {
                            Ok(picker) if picker.total_count() > 0 => {
                                state.message_picker = Some(picker);
                                state.session_picker = None;
                                footer_message = None;
                            }
                            Ok(_) => {
                                footer_message = Some("no message history found".into());
                            }
                            Err(error) => {
                                footer_message = Some(format!("history failed: {error}"));
                            }
                        }
                    }
                    KeyCode::Char(c) if c == config.keybindings.kill => {
                        if let Some(row) = rows.get(state.selected_sidebar_row) {
                            match row.kind {
                                SidebarRowKind::TopLevel { top_level_id, .. } => {
                                    manager.select_top_level(top_level_id);
                                    manager.request_kill_selected();
                                    if let Some(summary) = manager.selected_summary() {
                                        footer_message = Some(format!(
                                            "kill {}? y confirm / n cancel",
                                            summary.title
                                        ));
                                    }
                                }
                                SidebarRowKind::Child { .. } => {
                                    footer_message =
                                        Some("kill only supported on top-level sessions".into());
                                }
                            }
                        }
                    }
                    KeyCode::Char(c) if c == config.keybindings.spawn => {
                        match pick_directory_with_terminal(terminal, config.spawn_maxdepth) {
                            Ok(Some(cwd)) => {
                                let title = display_title_for_cwd(&cwd);
                                let (rows, cols) =
                                    pane_size(terminal.size()?.into(), config.sidebar_width);
                                match manager.spawn_managed(cwd, title.clone(), rows, cols) {
                                    Ok(_) => {
                                        save_managed_sessions(manager.managed_session_ids())?;
                                        state.focus = AppFocus::Terminal;
                                        state.selected_sidebar_row = 0;
                                        footer_message = Some(format!("spawned {title}"))
                                    }
                                    Err(error) => {
                                        footer_message = Some(format!("spawn failed: {error}"))
                                    }
                                }
                            }
                            Ok(None) => footer_message = Some("spawn canceled".into()),
                            Err(error) => footer_message = Some(format!("picker failed: {error}")),
                        }
                    }
                    KeyCode::Tab => {
                        if let Some(row) = rows.get(state.selected_sidebar_row)
                            && row.has_children
                            && let Some(session_id) = row.session_id.clone()
                        {
                            reduce(&mut state, Action::ToggleExpandSelected(session_id));
                        }
                    }
                    KeyCode::Char('r') => {
                        let (pty_rows, pty_cols) =
                            pane_size(terminal.size()?.into(), sidebar_width);
                        match manager.refresh_active(pty_rows, pty_cols) {
                            Ok(true) => footer_message = Some("refreshed session".into()),
                            Ok(false) => {
                                footer_message = Some("no active session to refresh".into())
                            }
                            Err(error) => footer_message = Some(format!("refresh failed: {error}")),
                        }
                    }

                    KeyCode::Char('c') => {
                        if let Some(row) = rows.get(state.selected_sidebar_row) {
                            match &row.kind {
                                SidebarRowKind::TopLevel {
                                    session_id: Some(sid),
                                    ..
                                } => {
                                    if let Some(cwd) = resolve_session_cwd(row) {
                                        match commit_session_files(terminal, sid, &cwd) {
                                            Ok(Some(msg)) => footer_message = Some(msg),
                                            Ok(None) => {
                                                footer_message = Some("commit canceled".into())
                                            }
                                            Err(e) => {
                                                footer_message = Some(format!("commit failed: {e}"))
                                            }
                                        }
                                    } else {
                                        footer_message = Some("session directory not found".into());
                                    }
                                }
                                _ => {
                                    footer_message =
                                        Some("commit requires a top-level session with ID".into())
                                }
                            }
                        }
                    }
                    KeyCode::Char('!') => {
                        if let Some(row) = rows.get(state.selected_sidebar_row) {
                            match &row.kind {
                                SidebarRowKind::TopLevel { .. } => {
                                    if let Some(cwd) = resolve_session_cwd(row) {
                                        match drop_to_bash(terminal, &cwd) {
                                            Ok(_) => footer_message = None,
                                            Err(error) => {
                                                footer_message =
                                                    Some(format!("bash failed: {error}"))
                                            }
                                        }
                                    } else {
                                        footer_message = Some("session directory not found".into());
                                    }
                                }
                                SidebarRowKind::Child { .. } => {
                                    footer_message = Some("bash only on top-level sessions".into());
                                }
                            }
                        }
                    }
                    KeyCode::Char('s') => {
                        reduce(&mut state, Action::ToggleSidebarCollapse);

                        let new_sidebar_width = if state.panel_hidden {
                            0
                        } else if state.sidebar_collapsed {
                            COLLAPSED_SIDEBAR_WIDTH
                        } else {
                            config.sidebar_width
                        };

                        if diff_view.is_active() {
                            let new_content_width =
                                terminal.size()?.width.saturating_sub(new_sidebar_width);
                            let new_vp =
                                terminal.size()?.height.saturating_sub(FOOTER_HEIGHT + 1) as usize;
                            let (doc, meta) = ui_diff::build_diff_document(
                                diff_view.raw_diff(),
                                new_content_width,
                            );
                            diff_view.replace_document(doc, meta, new_vp);
                        }
                    }
                    KeyCode::Char(c) if c == config.keybindings.worktree => {
                        match pick_directory_with_terminal(terminal, config.spawn_maxdepth) {
                            Ok(Some(repo_dir)) => {
                                match prompt_text_with_terminal(
                                    terminal,
                                    "Branch name (empty = repo root):",
                                )? {
                                    Some(branch) if !branch.trim().is_empty() => {
                                        match create_worktree(&repo_dir, branch.trim()) {
                                            Ok(worktree_dir) => {
                                                let title = display_title_for_cwd(&worktree_dir);
                                                let (rows, cols) = pane_size(
                                                    terminal.size()?.into(),
                                                    sidebar_width,
                                                );
                                                match manager.spawn_managed(
                                                    worktree_dir,
                                                    title.clone(),
                                                    rows,
                                                    cols,
                                                ) {
                                                    Ok(_) => {
                                                        save_managed_sessions(
                                                            manager.managed_session_ids(),
                                                        )?;
                                                        footer_message = Some(format!(
                                                            "spawned worktree {title}"
                                                        ));
                                                    }
                                                    Err(error) => {
                                                        footer_message =
                                                            Some(format!("spawn failed: {error}"))
                                                    }
                                                }
                                            }
                                            Err(error) => {
                                                footer_message =
                                                    Some(format!("worktree failed: {error}"))
                                            }
                                        }
                                    }
                                    Some(_) => {
                                        let title = display_title_for_cwd(&repo_dir);
                                        let (rows, cols) = pane_size(
                                            terminal.size()?.into(),
                                            config.sidebar_width,
                                        );
                                        match manager.spawn_managed(
                                            repo_dir,
                                            title.clone(),
                                            rows,
                                            cols,
                                        ) {
                                            Ok(_) => {
                                                save_managed_sessions(
                                                    manager.managed_session_ids(),
                                                )?;
                                                footer_message = Some(format!("spawned {title}"));
                                            }
                                            Err(error) => {
                                                footer_message =
                                                    Some(format!("spawn failed: {error}"))
                                            }
                                        }
                                    }
                                    None => footer_message = Some("worktree canceled".into()),
                                }
                            }
                            Ok(None) => footer_message = Some("worktree canceled".into()),
                            Err(error) => footer_message = Some(format!("picker failed: {error}")),
                        }
                    }
                    _ => {}
                },
                Event::Paste(text)
                    if matches!(state.focus, AppFocus::Conversation)
                        && conversation.is_searching() =>
                {
                    conversation.search_insert_str(&text, viewport_height);
                }
                Event::Key(key)
                    if matches!(state.focus, AppFocus::Conversation)
                        && conversation.is_searching() =>
                {
                    let vp = viewport_height;
                    match key.code {
                        KeyCode::Char(c) => {
                            conversation.search_insert(c, vp);
                        }
                        KeyCode::Backspace => {
                            conversation.search_backspace(vp);
                        }
                        KeyCode::Enter => {
                            conversation.confirm_search();
                        }
                        KeyCode::Esc => {
                            conversation.cancel_search();
                        }
                        _ => {}
                    }
                }
                Event::Key(key) if matches!(state.focus, AppFocus::Conversation) => {
                    let vp = viewport_height;
                    match key.code {
                        KeyCode::Char('k') | KeyCode::Up => {
                            conversation.scroll_up(1);
                        }
                        KeyCode::Char('j') | KeyCode::Down => {
                            conversation.scroll_down(1, vp);
                        }
                        KeyCode::Char('y') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            conversation.scroll_up(1);
                        }
                        KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            conversation.scroll_down(1, vp);
                        }
                        KeyCode::Char('G') => {
                            conversation.scroll_to_end(vp);
                        }
                        KeyCode::Char('g') => {
                            conversation.scroll_to_top();
                        }
                        KeyCode::Char('/') => {
                            conversation.start_search();
                        }
                        KeyCode::Char('n') => {
                            conversation.next_match(vp);
                        }
                        KeyCode::Char('N') => {
                            conversation.prev_match(vp);
                        }
                        KeyCode::PageUp | KeyCode::Char('u')
                            if key.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            conversation.scroll_up(vp.saturating_sub(1));
                        }
                        KeyCode::PageDown | KeyCode::Char('d')
                            if key.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            conversation.scroll_down(vp.saturating_sub(1), vp);
                        }
                        KeyCode::Char(c) if c == config.keybindings.view => {
                            let return_focus = conversation.close();
                            state.last_main_focus = AppFocus::Terminal;
                            reduce(&mut state, Action::SetFocus(return_focus));
                            footer_message = None;
                        }
                        KeyCode::Char(c) if c == config.keybindings.quit => {
                            state.confirm_quit = true;
                        }
                        KeyCode::Esc => {
                            let return_focus = conversation.close();
                            state.last_main_focus = AppFocus::Terminal;
                            reduce(&mut state, Action::SetFocus(return_focus));
                            footer_message = None;
                        }
                        _ => {}
                    }
                }
                Event::Paste(text)
                    if matches!(state.focus, AppFocus::Diff) && diff_view.is_searching() =>
                {
                    diff_view.search_insert_str(&text, viewport_height);
                }
                Event::Key(key)
                    if matches!(state.focus, AppFocus::Diff) && diff_view.is_searching() =>
                {
                    let vp = viewport_height;
                    match key.code {
                        KeyCode::Char(c) => {
                            diff_view.search_insert(c, vp);
                        }
                        KeyCode::Backspace => {
                            diff_view.search_backspace(vp);
                        }
                        KeyCode::Enter => {
                            diff_view.confirm_search();
                        }
                        KeyCode::Esc => {
                            diff_view.cancel_search();
                        }
                        _ => {}
                    }
                }
                Event::Key(key) if matches!(state.focus, AppFocus::Diff) => {
                    let vp = viewport_height;
                    match key.code {
                        KeyCode::Char('k') | KeyCode::Up => {
                            diff_view.move_cursor_up(1, vp);
                        }
                        KeyCode::Char('j') | KeyCode::Down => {
                            diff_view.move_cursor_down(1, vp);
                        }
                        KeyCode::Char('G') => {
                            diff_view.move_cursor_to_end(vp);
                        }
                        KeyCode::Char('g') => {
                            diff_view.move_cursor_to_top(vp);
                        }
                        KeyCode::PageUp | KeyCode::Char('u')
                            if key.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            diff_view.move_cursor_up(vp.saturating_sub(1), vp);
                        }
                        KeyCode::PageDown | KeyCode::Char('d')
                            if key.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            diff_view.move_cursor_down(vp.saturating_sub(1), vp);
                        }
                        KeyCode::Char('y') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            diff_view.scroll_view_up(1, vp);
                        }
                        KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            diff_view.scroll_view_down(1, vp);
                        }
                        KeyCode::Char('v') => {
                            diff_view.toggle_visual();
                        }
                        KeyCode::Char('/') if !diff_view.is_visual() => {
                            diff_view.start_search();
                        }
                        KeyCode::Char('n') => {
                            diff_view.next_match(vp);
                        }
                        KeyCode::Char('N') => {
                            diff_view.prev_match(vp);
                        }
                        KeyCode::Enter if diff_view.is_visual() => {
                            if let Some(text) = diff_view.format_selection() {
                                diff_view.close();
                                state.last_main_focus = AppFocus::Terminal;
                                reduce(&mut state, Action::SetFocus(AppFocus::Terminal));
                                if let Some(pty) = manager.active_session_mut() {
                                    let _ = pty.send_paste(&text);
                                }
                                footer_message = None;
                            } else {
                                footer_message = Some("No valid lines in selection".to_string());
                            }
                        }
                        KeyCode::Enter => {}
                        KeyCode::Char(c) if c == config.keybindings.diff => {
                            let return_focus = diff_view.close();
                            state.last_main_focus = AppFocus::Terminal;
                            reduce(&mut state, Action::SetFocus(return_focus));
                            footer_message = None;
                        }
                        KeyCode::Char(c) if c == config.keybindings.quit => {
                            let return_focus = diff_view.close();
                            state.last_main_focus = AppFocus::Terminal;
                            reduce(&mut state, Action::SetFocus(return_focus));
                            footer_message = None;
                        }
                        KeyCode::Esc => {
                            if diff_view.is_visual() {
                                diff_view.cancel_visual();
                            } else {
                                let return_focus = diff_view.close();
                                state.last_main_focus = AppFocus::Terminal;
                                reduce(&mut state, Action::SetFocus(return_focus));
                                footer_message = None;
                            }
                        }
                        _ => {}
                    }
                }
                Event::Key(key) if matches!(state.focus, AppFocus::Terminal) => {
                    if let Some(pty) = manager.active_session_mut()
                        && let Err(error) = pty.send_key(key)
                    {
                        footer_message = Some(format!("terminal write failed: {error}"));
                    }
                }
                Event::Paste(text) if matches!(state.focus, AppFocus::Terminal) => {
                    if let Some(pty) = manager.active_session_mut()
                        && let Err(error) = pty.send_paste(&text)
                    {
                        footer_message = Some(format!("paste failed: {error}"));
                    }
                }
                Event::Mouse(mouse)
                    if matches!(mouse.kind, MouseEventKind::Down(_))
                        && mouse.column < config.sidebar_width =>
                {
                    state.focus = AppFocus::Sidebar;
                    let clicked_row = mouse.row.saturating_sub(1) as usize;
                    if clicked_row < rows.len() {
                        state.selected_sidebar_row = clicked_row;
                    }
                }
                Event::Mouse(mouse)
                    if matches!(
                        mouse.kind,
                        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                    ) =>
                {
                    let scroll_amount = 3;
                    match state.focus {
                        AppFocus::Conversation => match mouse.kind {
                            MouseEventKind::ScrollUp => {
                                conversation.scroll_up(scroll_amount);
                            }
                            MouseEventKind::ScrollDown => {
                                conversation.scroll_down(scroll_amount, viewport_height);
                            }
                            _ => {}
                        },
                        AppFocus::Diff => match mouse.kind {
                            MouseEventKind::ScrollUp => {
                                diff_view.scroll_up(scroll_amount);
                            }
                            MouseEventKind::ScrollDown => {
                                diff_view.scroll_down(scroll_amount, viewport_height);
                            }
                            _ => {}
                        },
                        _ => {}
                    }
                }
                Event::FocusGained => {
                    state.app_focused = true;
                }
                Event::FocusLost => {
                    state.app_focused = false;
                }
                Event::Resize(width, height) => {
                    let (pty_rows, pty_cols) = pane_size(
                        ratatui::layout::Rect::new(0, 0, width, height),
                        sidebar_width,
                    );
                    if let Err(error) = manager.resize_active(pty_rows, pty_cols) {
                        footer_message = Some(format!("resize failed: {error}"));
                    }
                    conversation.clamp_scroll(height.saturating_sub(FOOTER_HEIGHT + 1) as usize);
                    if diff_view.is_active() {
                        let new_content_width = width.saturating_sub(sidebar_width);
                        let new_vp = height.saturating_sub(FOOTER_HEIGHT + 1) as usize;
                        let (doc, meta) =
                            ui_diff::build_diff_document(diff_view.raw_diff(), new_content_width);
                        diff_view.replace_document(doc, meta, new_vp);
                    }
                }
                _ => {}
            }

            if matches!(state.focus, AppFocus::Terminal)
                && let Some(active_id) = manager.active_id()
                    && let Some(idx) = rows.iter().position(|r| {
                        matches!(&r.kind, SidebarRowKind::TopLevel { top_level_id, .. } if *top_level_id == active_id)
                    }) {
                        state.selected_sidebar_row = idx;
                    }
            prev_selected_kind = rows.get(state.selected_sidebar_row).map(|r| r.kind.clone());
            sync_mouse_capture(terminal, state.focus, &mut mouse_captured);
        }

        Ok(())
    })();

    manager.shutdown_local_ptys();
    poller.stop();

    result
}

fn pane_size(area: ratatui::layout::Rect, sidebar_width: u16) -> (u16, u16) {
    let rows = area.height.saturating_sub(FOOTER_HEIGHT).max(1);
    let cols = area.width.saturating_sub(sidebar_width).max(1);
    (rows, cols)
}

fn is_focus_toggle(key: KeyEvent) -> bool {
    key.code == KeyCode::Char('4') && key.modifiers.contains(KeyModifiers::CONTROL)
}

fn is_panel_toggle(key: KeyEvent) -> bool {
    key.code == KeyCode::Char('h') && key.modifiers.contains(KeyModifiers::CONTROL)
}

fn pick_directory_with_terminal(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    maxdepth: u32,
) -> Result<Option<std::path::PathBuf>, Box<dyn Error>> {
    leave_tui(terminal)?;
    let picked = pick_directory(maxdepth);
    enter_tui(terminal)?;

    Ok(picked?)
}

fn prompt_text_with_terminal(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    prompt: &str,
) -> Result<Option<String>, Box<dyn Error>> {
    leave_tui(terminal)?;

    use std::io::Write;
    print!("\x1b[2J\x1b[H");
    print!("{prompt} ");
    std::io::stdout().flush()?;

    let mut input = String::new();
    let result = std::io::stdin().read_line(&mut input);

    enter_tui(terminal)?;

    match result {
        Ok(0) => Ok(None),
        Ok(_) => {
            let trimmed = input.trim().to_string();
            Ok(Some(trimmed))
        }
        Err(_) => Ok(None),
    }
}

fn drop_to_bash(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    cwd: &std::path::Path,
) -> Result<(), Box<dyn Error>> {
    leave_tui(terminal)?;

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
    let _ = std::process::Command::new(&shell)
        .current_dir(cwd)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status();

    enter_tui(terminal)?;
    Ok(())
}

fn commit_session_files(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    session_id: &str,
    cwd: &std::path::Path,
) -> Result<Option<String>, Box<dyn Error>> {
    let reader = opencode_multiplexer::data::db::reader::DbReader::open_default()?;
    let files = reader.get_session_modified_files(session_id)?;
    if files.is_empty() {
        return Ok(Some("no files modified by this session".into()));
    }

    leave_tui(terminal)?;
    print!("\x1b[2J\x1b[H");

    // Get git status for session files
    let (created, modified, deleted) =
        opencode_multiplexer::ops::git::get_file_statuses(cwd, &files)?;

    println!("Files modified by this session:\n");

    if !created.is_empty() {
        println!("\x1b[32mCreated:\x1b[0m"); // Green
        for f in &created {
            println!("\x1b[32m  {f}\x1b[0m");
        }
        println!();
    }

    if !modified.is_empty() {
        println!("\x1b[33mModified:\x1b[0m"); // Yellow
        for f in &modified {
            println!("\x1b[33m  {f}\x1b[0m");
        }
        println!();
    }

    if !deleted.is_empty() {
        println!("\x1b[31mDeleted:\x1b[0m"); // Red
        for f in &deleted {
            println!("\x1b[31m  {f}\x1b[0m");
        }
        println!();
    }

    if created.is_empty() && modified.is_empty() && deleted.is_empty() {
        println!("No uncommitted changes for session files.\n");
    }

    // Prompt for commit message
    use std::io::Write;
    print!("Commit message (empty to cancel): ");
    std::io::stdout().flush()?;
    let mut message = String::new();
    std::io::stdin().read_line(&mut message)?;
    let message = message.trim().to_string();

    let result = if message.is_empty() {
        None
    } else {
        // Run commit + push, show output
        let output = opencode_multiplexer::ops::git::commit_and_push_files(
            cwd, &created, &modified, &deleted, &message,
        )?;
        println!("\n{output}");
        println!("Press Enter to continue...");
        let _ = std::io::stdin().read_line(&mut String::new());
        Some(format!("committed {} files", files.len()))
    };

    enter_tui(terminal)?;
    Ok(result)
}

fn resolve_session_cwd(
    row: &opencode_multiplexer::ui::sidebar::SidebarVisibleRow,
) -> Option<PathBuf> {
    if !row.cwd.as_os_str().is_empty() && row.cwd.is_dir() {
        return Some(row.cwd.clone());
    }
    if let Some(sid) = row.session_id.as_deref()
        && let Ok(reader) = opencode_multiplexer::data::db::reader::DbReader::open_default()
        && let Ok(Some(session)) = reader.get_session_by_id(sid)
    {
        if !session.directory.as_os_str().is_empty() && session.directory.is_dir() {
            return Some(session.directory);
        }
        if let Ok(projects) = reader.get_projects()
            && let Some(proj) = projects.iter().find(|p| p.id == session.project_id)
            && proj.worktree.is_dir()
        {
            return Some(proj.worktree.clone());
        }
    }
    None
}

/// Resolve the diff for a session. Tries the opencode serve API first (targeted
/// to the matching port), then falls back to a full worktree git diff.
fn resolve_session_diff(
    row: &opencode_multiplexer::ui::sidebar::SidebarVisibleRow,
    sid: &str,
) -> Result<String, String> {
    let cwd = resolve_session_cwd(row).ok_or_else(|| "session directory not found".to_string())?;

    // Try serve API first (targeted to the matching port only).
    if let Some(diff) = fetch_session_diff_from_serve(sid, &cwd) {
        return Ok(diff);
    }

    // Fall back to full worktree git diff (tracked + untracked).
    diff_worktree(&cwd).map_err(|e| e.to_string())
}

fn leave_tui(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
) -> Result<(), Box<dyn Error>> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableBracketedPaste,
        crossterm::event::DisableFocusChange
    )?;
    terminal.show_cursor()?;
    Ok(())
}

fn enter_tui(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
) -> Result<(), Box<dyn Error>> {
    execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        EnableBracketedPaste,
        crossterm::event::EnableFocusChange
    )?;
    enable_raw_mode()?;
    terminal.clear()?;
    Ok(())
}

fn sync_mouse_capture(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    focus: AppFocus,
    captured: &mut bool,
) {
    let want = matches!(focus, AppFocus::Conversation | AppFocus::Diff);
    if want == *captured {
        return;
    }
    if want {
        let _ = execute!(terminal.backend_mut(), EnableMouseCapture);
    } else {
        let _ = execute!(terminal.backend_mut(), DisableMouseCapture);
    }
    *captured = want;
}
