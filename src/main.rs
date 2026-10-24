use std::{
    collections::HashSet,
    error::Error,
    sync::Arc,
    time::{Duration, Instant},
};

use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use opencode_multiplexer::{
    app::{
        Action,
        agents::AgentGraph,
        conversation::ConversationViewState,
        diff::DiffViewState,
        focus::AppFocus,
        key_handler::{KeyAction, handle_conversation_key, handle_diff_key},
        message_picker::MessagePickerState,
        playbook::{
            PlaybookUiAction, SessionTarget, abort_playbook,
            handle_form_key as handle_playbook_form_key,
            handle_picker_key as handle_playbook_picker_key, poll_playbook,
            should_refresh_pty_after_playbook, start_playbook,
        },
        reducer::reduce,
        session_cycle::{chrome_session_cycle, follow_attach_id},
        session_manager::{ManagerCommand, SessionManagerState, manager_key_to_command},
        session_picker::SessionPickerState,
        sessions::SessionStatus,
        state::AppState,
    },
    config::load_config,
    data::{
        db::{reader::DbReader, writer::DbWriter},
        parallel_builds::load_snapshot,
        poller::start_poller,
    },
    notify::Notifier,
    ops::git::{diff_worktree, fetch_session_diff_from_serve},
    ops::opencode_events::{ServeEvent, SessionEventSubscriber},
    ops::worktree::create_worktree,
    ops::{fzf::pick_directory, opencode::display_title_for_cwd},
    playbook::{http::ServePromptClient, shell::PosixShell},
    registry::{load_serve_registry, save_managed_sessions},
    terminal::{
        clipboard, input, manager::PtyManager, selection::MouseResult, selection::TerminalSelection,
    },
    ui::{
        conversation::build_conversation_document,
        conversation::diagram::mmdc::MermaidRenderConfig,
        diff as ui_diff,
        layout::{centered_rect, terminal_inner_rect},
        root,
        sidebar::{SidebarRowKind, flatten_sidebar_entries},
    },
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::path::PathBuf;

const FOOTER_HEIGHT: u16 = 2;

fn main() -> Result<(), Box<dyn Error>> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableBracketedPaste,
        EnableMouseCapture,
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
    let _ = opencode_multiplexer::registry::cleanup_orphaned_serve_processes();
    let mut state = AppState::default();
    let mut manager = PtyManager::default();
    let mut footer_message: Option<String> = None;
    let mut conversation = ConversationViewState::default();
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let mermaid_config = MermaidRenderConfig {
        mmdc_path: PathBuf::from("mmdc"),
        cache_dir: PathBuf::from(home).join(".cache/ocmux/mermaid"),
        timeout: Duration::from_secs(10),
        max_rows: 36,
        prefetch_viewports: 1,
        // true => force on; set_mermaid_config also ORs kitty auto-detect.
        // OCMUX_NO_KITTY_GRAPHICS disables. OCMUX_PROTOCOL forces on.
        protocol_enabled: std::env::var_os("OCMUX_PROTOCOL").is_some(),
        invocation_log: None,
    };
    conversation.set_mermaid_config(mermaid_config);
    let mut diff_view = DiffViewState::default();
    let mut open_requests = opencode_multiplexer::ops::open_requests::OpenRequestTracker::default();
    let mut terminal_selection = TerminalSelection::default();
    let (poll_tx, poll_rx) = std::sync::mpsc::channel();
    let poller = start_poller(poll_tx);

    // SSE event channel — receives session.created events from serve subscribers
    let (event_tx, event_rx) = std::sync::mpsc::channel::<(u16, ServeEvent)>();
    let mut subscribers: Vec<SessionEventSubscriber> = Vec::new();
    let mut subscribed_serve_ports = HashSet::new();

    // Start SSE subscribers for existing serves from the registry
    for entry in load_serve_registry().unwrap_or_default() {
        subscribed_serve_ports.insert(entry.port);
        subscribers.push(SessionEventSubscriber::start(entry.port, event_tx.clone()));
    }

    let mut prev_selected_kind: Option<SidebarRowKind> = None;
    let mut last_agents_refresh = Instant::now() - Duration::from_secs(2);
    let mut last_reconcile = Instant::now() - Duration::from_secs(60);
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

                let registry_dirty = manager.apply_poll_snapshot(snapshot.clone(), &open_requests);
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

            // Drain SSE events
            while let Ok((port, event)) = event_rx.try_recv() {
                match event {
                    ServeEvent::SessionCreated(e) => {
                        let dirty = manager.apply_session_event(port, &e);
                        if dirty {
                            let _ = save_managed_sessions(manager.managed_session_ids());
                        }
                    }
                    _ => {
                        open_requests.apply(port, &event);
                        manager.apply_open_requests_overlay(&open_requests);
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
                        let doc = build_conversation_document(
                            &messages,
                            content_width,
                            conversation.diagram_index(),
                        );
                        conversation.replace_document(doc, viewport_height);
                        conversation.clear_error();
                    }
                    Err(e) => {
                        conversation.set_error(e.to_string());
                    }
                }
            }

            if state.focus == AppFocus::Conversation {
                conversation.scheduler_tick(viewport_height, content_width);
                for finished in conversation.poll_diagram_completions() {
                    conversation.apply_diagram_update(finished, viewport_height);
                }
                // Edge auto-scroll runs on elapsed time, not mouse events, so
                // a drag held still at a viewport edge keeps extending.
                if conversation.is_dragging() {
                    let inner = terminal_inner_rect(terminal.size()?.into(), sidebar_width, 1);
                    let content = conversation.content_rect(inner);
                    conversation.tick_selection(Instant::now(), content, viewport_height);
                }
            }

            if state.focus == AppFocus::Agents
                && last_agents_refresh.elapsed() >= Duration::from_secs(1)
                && let Some(row) = rows.get(state.selected_sidebar_row)
                && let Some(root_summary) = selected_root_summary(&manager, row)
            {
                let snapshot = root_summary.session_id.as_deref().and_then(|session_id| {
                    load_snapshot(&root_summary.cwd, session_id).ok().flatten()
                });
                state
                    .agents
                    .replace_graph(AgentGraph::from_snapshot(&root_summary, snapshot.as_ref()));
                last_agents_refresh = Instant::now();
            }

            if let Some(picker) = state.session_picker.as_mut() {
                picker.tick();
            }
            if let Some(manager) = state.session_manager.as_mut() {
                manager.tick();
            }
            if let Some(picker) = state.message_picker.as_mut() {
                picker.tick();
            }

            let playbook_ids: Vec<String> = state.playbook_runs.keys().cloned().collect();
            for session_id in playbook_ids {
                let Some(run) = state.playbook_runs.get_mut(&session_id) else {
                    continue;
                };
                match poll_playbook(run) {
                    opencode_multiplexer::app::playbook::PlaybookPoll::Quiet => {}
                    opencode_multiplexer::app::playbook::PlaybookPoll::Banner(message)
                    | opencode_multiplexer::app::playbook::PlaybookPoll::Finished(message) => {
                        let finished = message.starts_with("playbook done:")
                            || message.starts_with("playbook failed:")
                            || message.starts_with("playbook aborted:");
                        footer_message = Some(message);
                        if finished {
                            state.playbook_runs.remove(&session_id);
                            if state.playbook_abort_for.as_deref() == Some(session_id.as_str()) {
                                state.playbook_abort_for = None;
                            }
                        }
                    }
                }
            }

            if last_reconcile.elapsed() >= Duration::from_secs(30) {
                let ports: Vec<u16> = manager
                    .sessions()
                    .items()
                    .iter()
                    .filter_map(|s| s.serve_port)
                    .collect();
                for port in ports {
                    if let Ok(permissions) =
                        opencode_multiplexer::ops::opencode::fetch_pending_permissions(port)
                    {
                        open_requests.reconcile_port(port, permissions);
                    }
                    if let Ok(questions) =
                        opencode_multiplexer::ops::opencode::fetch_pending_questions(port)
                    {
                        open_requests.reconcile_questions(port, questions);
                    }
                }
                manager.apply_open_requests_overlay(&open_requests);
                last_reconcile = Instant::now();
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
                    state.panel_hidden,
                    state.app_focused,
                    &conversation,
                    &diff_view,
                    &state.agents,
                    state.session_picker.as_mut(),
                    state.session_manager.as_mut(),
                    state.message_picker.as_mut(),
                    state.playbook_picker.as_ref(),
                    state.playbook_form.as_ref(),
                    state.playbook_abort_for.is_some(),
                    state.confirm_quit,
                    terminal_selection.range(),
                )
            })?;

            // Overlay Kitty pixel graphics for mermaid diagrams (scroll-coupled).
            // Must run AFTER ratatui draw so cells don't paint over the images.
            // Clear whenever conversation is not the exclusive full-screen view
            // (other focus, pickers, help, quit confirm) so images don't ghost.
            let overlays_block_graphics = state.session_picker.is_some()
                || state.session_manager.is_some()
                || state.message_picker.is_some()
                || state.playbook_picker.is_some()
                || state.playbook_form.is_some()
                || state.playbook_abort_for.is_some()
                || state.show_help
                || state.confirm_quit;
            let show_kitty = matches!(state.focus, AppFocus::Conversation)
                && conversation.is_active()
                && !conversation.is_lightbox()
                && !overlays_block_graphics;
            let show_lightbox_kitty = matches!(state.focus, AppFocus::Conversation)
                && conversation.is_active()
                && conversation.is_lightbox()
                && !overlays_block_graphics
                && conversation.protocol_enabled();
            if show_lightbox_kitty {
                let area: ratatui::layout::Rect = terminal.size()?.into();
                let body = ratatui::layout::Rect::new(
                    area.x,
                    area.y,
                    area.width,
                    area.height.saturating_sub(1),
                );
                let popup = centered_rect(body, 96, 96);
                let block_inner = ratatui::layout::Rect {
                    x: popup.x.saturating_add(1),
                    y: popup.y.saturating_add(1),
                    width: popup.width.saturating_sub(2),
                    height: popup.height.saturating_sub(2),
                };
                let _ = conversation.paint_lightbox_graphics(terminal.backend_mut(), block_inner);
            } else if show_kitty {
                let area = terminal_inner_rect(terminal.size()?.into(), sidebar_width, 1);
                // Shrink for search bar if active (mirror root.rs).
                let area = if conversation.is_searching() || !conversation.search_query().is_empty()
                {
                    ratatui::layout::Rect::new(
                        area.x,
                        area.y,
                        area.width,
                        area.height.saturating_sub(1),
                    )
                } else {
                    area
                };
                let _ = conversation.paint_kitty_graphics(terminal.backend_mut(), area);
            } else if conversation.has_kitty_graphics() {
                let _ = conversation.clear_kitty_graphics(terminal.backend_mut());
            }

            if !event::poll(Duration::from_millis(16))? {
                continue;
            }

            match event::read()? {
                Event::Key(key) => {
                    terminal_selection.clear();
                    if state.playbook_abort_for.is_some() {
                        match key.code {
                            KeyCode::Enter | KeyCode::Char('y') => {
                                if let Some(session_id) = state.playbook_abort_for.take()
                                    && let Some(run) = state.playbook_runs.get(&session_id)
                                {
                                    abort_playbook(run);
                                }
                                footer_message = Some("playbook abort requested".into());
                            }
                            KeyCode::Esc | KeyCode::Char('n') => {
                                state.playbook_abort_for = None;
                            }
                            _ => {}
                        }
                    } else if state.message_picker.is_some() {
                        match key.code {
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
                                                footer_message =
                                                    Some(format!("paste failed: {error}"));
                                            }
                                        }
                                    } else {
                                        footer_message =
                                            Some("no active session to paste into".into());
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
                        }
                    } else if state.session_picker.is_some() {
                        match key.code {
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
                                        .and_then(|r| r.get_session_status(&entry.session_id, None))
                                    {
                                        Ok(status) => {
                                            let (rows, cols) = pane_size(
                                                terminal.size()?.into(),
                                                config.sidebar_width,
                                            );
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
                                                    save_managed_sessions(
                                                        manager.managed_session_ids(),
                                                    )?;
                                                    if let Some(port) = manager
                                                        .selected_summary()
                                                        .and_then(|summary| summary.serve_port)
                                                        && subscribed_serve_ports.insert(port)
                                                    {
                                                        subscribers.push(
                                                            SessionEventSubscriber::start(
                                                                port,
                                                                event_tx.clone(),
                                                            ),
                                                        );
                                                    }
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
                                            footer_message =
                                                Some(format!("status lookup failed: {error}"));
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
                        }
                    } else if state.playbook_form.is_some() {
                        let form = state.playbook_form.as_mut().unwrap();
                        let action = handle_playbook_form_key(key, form);
                        match action {
                            PlaybookUiAction::Consumed => {}
                            PlaybookUiAction::Close => {
                                state.playbook_form = None;
                                footer_message = Some("playbook canceled".into());
                            }
                            PlaybookUiAction::OpenForm(_) => {}
                            PlaybookUiAction::Start {
                                playbook_name,
                                steps,
                                vars,
                            } => {
                                state.playbook_form = None;
                                match rows
                                    .get(state.selected_sidebar_row)
                                    .and_then(|row| selected_root_summary(&manager, row))
                                {
                                    Some(summary) => {
                                        match prepare_playbook_target(&mut manager, &summary)
                                            .and_then(|target| {
                                                if should_refresh_pty_after_playbook(
                                                    summary.serve_port,
                                                    summary.session_id.as_deref(),
                                                    summary.id,
                                                    manager.active_id(),
                                                ) {
                                                    let (pty_rows, pty_cols) = pane_size(
                                                        terminal
                                                            .size()
                                                            .map_err(|error| error.to_string())?
                                                            .into(),
                                                        sidebar_width,
                                                    );
                                                    manager
                                                        .refresh_active(pty_rows, pty_cols)
                                                        .map_err(|error| error.to_string())?;
                                                }
                                                start_playbook(
                                                    playbook_name,
                                                    steps,
                                                    vars,
                                                    &target,
                                                    Arc::new(ServePromptClient),
                                                    Arc::new(PosixShell::default()),
                                                )
                                            }) {
                                            Ok(run) => {
                                                footer_message = Some(format!(
                                                    "playbook: {}  running",
                                                    run.name
                                                ));
                                                state
                                                    .playbook_runs
                                                    .insert(run.session_id().to_string(), run);
                                            }
                                            Err(error) => footer_message = Some(error),
                                        }
                                    }
                                    None => footer_message = Some("no selected session".into()),
                                }
                            }
                        }
                    } else if state.playbook_picker.is_some() {
                        let picker = state.playbook_picker.as_mut().unwrap();
                        let action = handle_playbook_picker_key(key, picker);
                        match action {
                            PlaybookUiAction::Consumed => {}
                            PlaybookUiAction::Close => {
                                state.playbook_picker = None;
                                footer_message = Some("playbooks canceled".into());
                            }
                            PlaybookUiAction::OpenForm(form) => {
                                state.playbook_picker = None;
                                state.playbook_form = Some(form);
                                footer_message = None;
                            }
                            PlaybookUiAction::Start {
                                playbook_name,
                                steps,
                                vars,
                            } => {
                                state.playbook_picker = None;
                                match rows
                                    .get(state.selected_sidebar_row)
                                    .and_then(|row| selected_root_summary(&manager, row))
                                {
                                    Some(summary) => {
                                        match prepare_playbook_target(&mut manager, &summary)
                                            .and_then(|target| {
                                                if should_refresh_pty_after_playbook(
                                                    summary.serve_port,
                                                    summary.session_id.as_deref(),
                                                    summary.id,
                                                    manager.active_id(),
                                                ) {
                                                    let (pty_rows, pty_cols) = pane_size(
                                                        terminal
                                                            .size()
                                                            .map_err(|error| error.to_string())?
                                                            .into(),
                                                        sidebar_width,
                                                    );
                                                    manager
                                                        .refresh_active(pty_rows, pty_cols)
                                                        .map_err(|error| error.to_string())?;
                                                }
                                                start_playbook(
                                                    playbook_name,
                                                    steps,
                                                    vars,
                                                    &target,
                                                    Arc::new(ServePromptClient),
                                                    Arc::new(PosixShell::default()),
                                                )
                                            }) {
                                            Ok(run) => {
                                                footer_message = Some(format!(
                                                    "playbook: {}  running",
                                                    run.name
                                                ));
                                                state
                                                    .playbook_runs
                                                    .insert(run.session_id().to_string(), run);
                                            }
                                            Err(error) => footer_message = Some(error),
                                        }
                                    }
                                    None => footer_message = Some("no selected session".into()),
                                }
                            }
                        }
                    } else if state.session_manager.is_some() {
                        let manager_state = state.session_manager.as_mut().unwrap();
                        let command = manager_key_to_command(
                            key,
                            manager_state.pending_delete.is_some(),
                            !manager_state.selected_ids.is_empty(),
                        );

                        match command {
                            ManagerCommand::ConfirmDelete => {
                                if let Some(pending) = manager_state.pending_delete.clone() {
                                    let live_ids: HashSet<String> = manager
                                        .sidebar_entries()
                                        .iter()
                                        .filter_map(|e| e.session_id.clone())
                                        .collect();
                                    match DbWriter::open_default().and_then(|mut w| {
                                        w.delete_sessions(&pending.session_ids, &live_ids)
                                    }) {
                                        Ok(result) => {
                                            manager_state
                                                .apply_local_removal(&result.deleted_session_ids);
                                            manager_state.pending_delete = None;

                                            if let Some(active_sid) = manager.active_session_id()
                                                && result.deleted_session_ids.contains(&active_sid)
                                            {
                                                manager.detach_active();
                                                state.focus = AppFocus::Sidebar;
                                            }

                                            save_managed_sessions(manager.managed_session_ids())?;

                                            footer_message = Some(format!(
                                                "deleted {} session(s), skipped {} live",
                                                result.deleted_session_ids.len(),
                                                result.skipped_live_ids.len()
                                            ));
                                        }
                                        Err(error) => {
                                            footer_message =
                                                Some(format!("delete failed: {error}"));
                                            manager_state.pending_delete = None;
                                        }
                                    }
                                }
                            }
                            ManagerCommand::CancelPending => {
                                manager_state.pending_delete = None;
                            }
                            ManagerCommand::Close => {
                                state.session_manager = None;
                                footer_message = Some("manager closed".into());
                            }
                            ManagerCommand::Up => manager_state.move_up(),
                            ManagerCommand::Down => manager_state.move_down(),
                            ManagerCommand::Toggle => manager_state.toggle_select(),
                            ManagerCommand::SelectAll => manager_state.select_all_matched(),
                            ManagerCommand::Clear => manager_state.clear_selection(),
                            ManagerCommand::RequestDelete => manager_state.request_delete(),
                            ManagerCommand::Backspace => manager_state.backspace(),
                            ManagerCommand::Insert(c) => manager_state.insert_char(c),
                            ManagerCommand::Nop => {}
                        }
                    } else if key.code == KeyCode::Char(config.keybindings.help)
                        && !matches!(state.focus, AppFocus::Terminal)
                    {
                        reduce(&mut state, Action::ToggleHelp)
                    } else if state.show_help && matches!(key.code, KeyCode::Esc | KeyCode::Char(_))
                    {
                        state.show_help = false;
                    } else if !state.show_files.is_empty()
                        && matches!(key.code, KeyCode::Esc | KeyCode::Char(_))
                    {
                        state.show_files.clear();
                    } else if state.confirm_quit {
                        match key.code {
                            KeyCode::Char('y') => {
                                manager.shutdown_managed_serves()?;
                                break;
                            }
                            KeyCode::Char('n') | KeyCode::Esc | KeyCode::Char('q') => {
                                state.confirm_quit = false;
                            }
                            _ => {}
                        }
                    } else if is_panel_toggle(key)
                        && matches!(
                            state.focus,
                            AppFocus::Terminal
                                | AppFocus::Sidebar
                                | AppFocus::Diff
                                | AppFocus::Conversation
                                | AppFocus::Agents
                        )
                    {
                        reduce(&mut state, Action::TogglePanelHidden);
                        let new_sidebar_width = if state.panel_hidden {
                            0
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
                            let (doc, meta) = ui_diff::build_diff_document(
                                diff_view.raw_diff(),
                                new_content_width,
                            );
                            diff_view.replace_document(doc, meta, new_vp);
                        }

                        if matches!(state.focus, AppFocus::Conversation) {
                            conversation.force_poll();
                        }
                    } else if let Some(outcome) = chrome_session_cycle(
                        key,
                        &config.keybindings,
                        state.focus,
                        false,
                        state.selected_sidebar_row,
                        &rows,
                    ) {
                        let (pty_rows, pty_cols) =
                            pane_size(terminal.size()?.into(), config.sidebar_width);
                        state.selected_sidebar_row = outcome.selected_row;
                        manager.select_top_level(outcome.top_level_id);
                        match manager.activate_or_attach_selected(pty_rows, pty_cols) {
                            Ok(_) => {
                                save_managed_sessions(manager.managed_session_ids())?;
                                if let Some(port) = manager
                                    .selected_summary()
                                    .and_then(|summary| summary.serve_port)
                                    && subscribed_serve_ports.insert(port)
                                {
                                    subscribers.push(SessionEventSubscriber::start(
                                        port,
                                        event_tx.clone(),
                                    ));
                                }
                                footer_message = None;
                            }
                            Err(error) => {
                                footer_message = Some(format!("attach failed: {error}"));
                            }
                        }
                    } else if is_focus_toggle(key) {
                        if state.panel_hidden {
                            reduce(&mut state, Action::TogglePanelHidden);
                            let new_sidebar_width = if state.panel_hidden {
                                0
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
                    } else if matches!(state.focus, AppFocus::Sidebar)
                        && manager.pending_kill().is_some()
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
                    } else if matches!(state.focus, AppFocus::Sidebar) {
                        match key.code {
                            KeyCode::Char(c) if c == config.keybindings.quit => {
                                state.confirm_quit = true;
                            }
                            KeyCode::Char(c) if c == config.keybindings.down => {
                                reduce(&mut state, Action::SelectNextRow);
                                if let Some(row) = rows.get(state.selected_sidebar_row)
                                    && let Some(top_level_id) =
                                        follow_attach_id(config.follow_selection, row)
                                {
                                    let (pty_rows, pty_cols) =
                                        pane_size(terminal.size()?.into(), config.sidebar_width);
                                    manager.select_top_level(top_level_id);
                                    if let Err(error) =
                                        manager.activate_or_attach_selected(pty_rows, pty_cols)
                                    {
                                        footer_message = Some(format!("attach failed: {error}"));
                                    }
                                }
                            }
                            KeyCode::Down => {
                                reduce(&mut state, Action::SelectNextRow);
                                if let Some(row) = rows.get(state.selected_sidebar_row)
                                    && let Some(top_level_id) =
                                        follow_attach_id(config.follow_selection, row)
                                {
                                    let (pty_rows, pty_cols) =
                                        pane_size(terminal.size()?.into(), config.sidebar_width);
                                    manager.select_top_level(top_level_id);
                                    if let Err(error) =
                                        manager.activate_or_attach_selected(pty_rows, pty_cols)
                                    {
                                        footer_message = Some(format!("attach failed: {error}"));
                                    }
                                }
                            }
                            KeyCode::Char(c) if c == config.keybindings.up => {
                                reduce(&mut state, Action::SelectPrevRow);
                                if let Some(row) = rows.get(state.selected_sidebar_row)
                                    && let Some(top_level_id) =
                                        follow_attach_id(config.follow_selection, row)
                                {
                                    let (pty_rows, pty_cols) =
                                        pane_size(terminal.size()?.into(), config.sidebar_width);
                                    manager.select_top_level(top_level_id);
                                    if let Err(error) =
                                        manager.activate_or_attach_selected(pty_rows, pty_cols)
                                    {
                                        footer_message = Some(format!("attach failed: {error}"));
                                    }
                                }
                            }
                            KeyCode::Up => {
                                reduce(&mut state, Action::SelectPrevRow);
                                if let Some(row) = rows.get(state.selected_sidebar_row)
                                    && let Some(top_level_id) =
                                        follow_attach_id(config.follow_selection, row)
                                {
                                    let (pty_rows, pty_cols) =
                                        pane_size(terminal.size()?.into(), config.sidebar_width);
                                    manager.select_top_level(top_level_id);
                                    if let Err(error) =
                                        manager.activate_or_attach_selected(pty_rows, pty_cols)
                                    {
                                        footer_message = Some(format!("attach failed: {error}"));
                                    }
                                }
                            }
                            KeyCode::Enter => {
                                let (pty_rows, pty_cols) =
                                    pane_size(terminal.size()?.into(), config.sidebar_width);
                                if let Some(row) = rows.get(state.selected_sidebar_row) {
                                    match &row.kind {
                                        SidebarRowKind::TopLevel { top_level_id, .. } => {
                                            manager.select_top_level(*top_level_id);
                                            match manager
                                                .activate_or_attach_selected(pty_rows, pty_cols)
                                            {
                                                Ok(_) => {
                                                    save_managed_sessions(
                                                        manager.managed_session_ids(),
                                                    )?;
                                                    state.focus = AppFocus::Terminal;
                                                    footer_message = None
                                                }
                                                Err(error) => {
                                                    footer_message =
                                                        Some(format!("attach failed: {error}"))
                                                }
                                            }
                                        }
                                        SidebarRowKind::Child { .. } => {
                                            footer_message = Some(
                                                "child rows are selectable; attach not wired yet"
                                                    .into(),
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
                                            conversation.open(
                                                sid.clone(),
                                                title,
                                                AppFocus::Sidebar,
                                            );
                                            reduce(
                                                &mut state,
                                                Action::SetFocus(AppFocus::Conversation),
                                            );
                                            footer_message = None;
                                        }
                                        SidebarRowKind::Child { session_id } => {
                                            let title = row.title.clone();
                                            conversation.open(
                                                session_id.clone(),
                                                title,
                                                AppFocus::Sidebar,
                                            );
                                            reduce(
                                                &mut state,
                                                Action::SetFocus(AppFocus::Conversation),
                                            );
                                            footer_message = None;
                                        }
                                        _ => {
                                            footer_message = Some(
                                                "conversation view requires a session with a DB ID"
                                                    .into(),
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
                                                footer_message = Some(
                                                    "no files modified by this session".into(),
                                                );
                                            }
                                            Ok(files) => {
                                                state.show_files = files;
                                            }
                                            Err(e) => {
                                                footer_message =
                                                    Some(format!("failed to read files: {e}"));
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
                                                diff_view.replace_document(
                                                    doc,
                                                    meta,
                                                    viewport_height,
                                                );
                                                reduce(
                                                    &mut state,
                                                    Action::SetFocus(AppFocus::Diff),
                                                );
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
                            KeyCode::Char(c) if c == config.keybindings.agents => {
                                if let Some(row) = rows.get(state.selected_sidebar_row)
                                    && let Some(root_summary) = selected_root_summary(&manager, row)
                                {
                                    let selected_id = row.session_id.as_deref();
                                    let snapshot =
                                        root_summary.session_id.as_deref().and_then(|session_id| {
                                            load_snapshot(&root_summary.cwd, session_id)
                                                .ok()
                                                .flatten()
                                        });
                                    state.agents.open_at(
                                        AgentGraph::from_snapshot(&root_summary, snapshot.as_ref()),
                                        AppFocus::Sidebar,
                                        selected_id,
                                    );
                                    last_agents_refresh = Instant::now();
                                    reduce(&mut state, Action::SetFocus(AppFocus::Agents));
                                    footer_message = None;
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
                            KeyCode::Char(c) if c == config.keybindings.playbook => {
                                let selected_session_id = rows
                                    .get(state.selected_sidebar_row)
                                    .and_then(|row| selected_root_summary(&manager, row))
                                    .and_then(|summary| summary.session_id);
                                if let Some(session_id) = selected_session_id
                                    && state.playbook_runs.contains_key(&session_id)
                                {
                                    state.playbook_abort_for = Some(session_id);
                                } else {
                                    match opencode_multiplexer::playbook::store::load_playbooks(
                                        &opencode_multiplexer::playbook::store::default_playbooks_dir()
                                            .unwrap_or_default(),
                                    ) {
                                        Ok(loaded) if !loaded.playbooks.is_empty() => {
                                            state.playbook_picker = Some(
                                                opencode_multiplexer::app::playbook::PlaybookPickerState::from_playbooks(
                                                    loaded.playbooks,
                                                ),
                                            );
                                            footer_message = None;
                                        }
                                        Ok(_) => footer_message = Some("no playbooks found".into()),
                                        Err(error) => footer_message =
                                            Some(format!("playbooks failed: {error}")),
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
                            KeyCode::Char(c) if c == config.keybindings.sessions => {
                                let live_ids: HashSet<String> = manager
                                    .sidebar_entries()
                                    .iter()
                                    .filter_map(|e| e.session_id.clone())
                                    .collect();
                                match SessionManagerState::load(live_ids) {
                                    Ok(m) if m.total_count() > 0 => {
                                        state.session_manager = Some(m);
                                        state.session_picker = None;
                                        state.message_picker = None;
                                        footer_message = None;
                                    }
                                    Ok(_) => {
                                        footer_message = Some("no sessions found".into());
                                    }
                                    Err(error) => {
                                        footer_message = Some(format!("manager failed: {error}"));
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
                                            footer_message = Some(
                                                "kill only supported on top-level sessions".into(),
                                            );
                                        }
                                    }
                                }
                            }
                            KeyCode::Char(c) if c == config.keybindings.move_session => {
                                if let Some(row) = rows.get(state.selected_sidebar_row).cloned() {
                                    if let Some(session_id) = row.session_id.clone() {
                                        if manager.session_tree_has_busy_status(&session_id)
                                            || matches!(
                                                row.status,
                                                SessionStatus::Working
                                                    | SessionStatus::SubagentsWorking
                                                    | SessionStatus::NeedsInput
                                            )
                                        {
                                            footer_message = Some("session is busy".into());
                                        } else {
                                            match pick_directory_with_terminal(
                                            terminal,
                                            config.spawn_maxdepth,
                                        ) {
                                            Ok(Some(destination)) => {
                                                match opencode_multiplexer::ops::session_move::move_session_tree(
                                                    &session_id,
                                                    &destination,
                                                ) {
                                                    Ok(moved) => {
                                                        let (pty_rows, pty_cols) = pane_size(
                                                            terminal.size()?.into(),
                                                            config.sidebar_width,
                                                        );
                                                        match manager.reconnect_moved_session_tree(
                                                            &moved.session_ids,
                                                            &session_id,
                                                            moved.destination.clone(),
                                                            pty_rows,
                                                            pty_cols,
                                                        ) {
                                                            Ok(()) => {
                                                                if let Some(summary) = manager.active_summary()
                                                                    && let Some(port) = summary.serve_port
                                                                    && subscribed_serve_ports.insert(port)
                                                                {
                                                                    subscribers.push(
                                                                        SessionEventSubscriber::start(
                                                                            port,
                                                                            event_tx.clone(),
                                                                        ),
                                                                    );
                                                                }
                                                                state.focus = AppFocus::Terminal;
                                                                state.selected_sidebar_row = 0;
                                                                let mut message = moved
                                                                    .warning
                                                                    .unwrap_or_else(|| format!("moved to {}", moved.destination.display()));
                                                                if moved.dirty_source {
                                                                    message.push_str(&format!(
                                                                        " (uncommitted files left in {})",
                                                                        row.cwd.display()
                                                                    ));
                                                                }
                                                                footer_message = Some(message);
                                                            }
                                                            Err(error) => {
                                                                footer_message = Some(format!(
                                                                    "moved, but reattach failed: {error}"
                                                                ));
                                                            }
                                                        }
                                                    }
                                                    Err(error) => {
                                                        footer_message = Some(format!(
                                                            "move failed: {error}"
                                                        ));
                                                    }
                                                }
                                            }
                                            Ok(None) => footer_message = Some("move canceled".into()),
                                            Err(error) => {
                                                footer_message = Some(format!("picker failed: {error}"));
                                            }
                                        }
                                        }
                                    } else {
                                        footer_message =
                                            Some("selected row has no session ID".into());
                                    }
                                }
                            }
                            KeyCode::Char(c) if c == config.keybindings.spawn => {
                                match pick_directory_with_terminal(terminal, config.spawn_maxdepth)
                                {
                                    Ok(Some(cwd)) => {
                                        let title = display_title_for_cwd(&cwd);
                                        let (rows, cols) = pane_size(
                                            terminal.size()?.into(),
                                            config.sidebar_width,
                                        );
                                        match manager.spawn_managed(cwd, title.clone(), rows, cols)
                                        {
                                            Ok(id) => {
                                                save_managed_sessions(
                                                    manager.managed_session_ids(),
                                                )?;
                                                if let Some(summary) = manager
                                                    .sessions()
                                                    .items()
                                                    .iter()
                                                    .find(|s| s.id == id)
                                                    && let Some(port) = summary.serve_port
                                                {
                                                    subscribers.push(
                                                        SessionEventSubscriber::start(
                                                            port,
                                                            event_tx.clone(),
                                                        ),
                                                    );
                                                }
                                                state.focus = AppFocus::Terminal;
                                                state.selected_sidebar_row = 0;
                                                footer_message = Some(format!("spawned {title}"))
                                            }
                                            Err(error) => {
                                                footer_message =
                                                    Some(format!("spawn failed: {error}"))
                                            }
                                        }
                                    }
                                    Ok(None) => footer_message = Some("spawn canceled".into()),
                                    Err(error) => {
                                        footer_message = Some(format!("picker failed: {error}"))
                                    }
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
                                    Err(error) => {
                                        footer_message = Some(format!("refresh failed: {error}"))
                                    }
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
                                                        footer_message =
                                                            Some("commit canceled".into())
                                                    }
                                                    Err(e) => {
                                                        footer_message =
                                                            Some(format!("commit failed: {e}"))
                                                    }
                                                }
                                            } else {
                                                footer_message =
                                                    Some("session directory not found".into());
                                            }
                                        }
                                        _ => {
                                            footer_message = Some(
                                                "commit requires a top-level session with ID"
                                                    .into(),
                                            )
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
                                                footer_message =
                                                    Some("session directory not found".into());
                                            }
                                        }
                                        SidebarRowKind::Child { .. } => {
                                            footer_message =
                                                Some("bash only on top-level sessions".into());
                                        }
                                    }
                                }
                            }
                            KeyCode::Char(c) if c == config.keybindings.worktree => {
                                match pick_directory_with_terminal(terminal, config.spawn_maxdepth)
                                {
                                    Ok(Some(repo_dir)) => {
                                        match prompt_text_with_terminal(
                                            terminal,
                                            "Branch name (empty = repo root):",
                                        )? {
                                            Some(branch) if !branch.trim().is_empty() => {
                                                match create_worktree(&repo_dir, branch.trim()) {
                                                    Ok(worktree_dir) => {
                                                        let title =
                                                            display_title_for_cwd(&worktree_dir);
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
                                                            Ok(id) => {
                                                                save_managed_sessions(
                                                                    manager.managed_session_ids(),
                                                                )?;
                                                                if let Some(summary) = manager
                                                                    .sessions()
                                                                    .items()
                                                                    .iter()
                                                                    .find(|s| s.id == id)
                                                                    && let Some(port) =
                                                                        summary.serve_port
                                                                {
                                                                    subscribers.push(
                                                                        SessionEventSubscriber::start(port, event_tx.clone()),
                                                                    );
                                                                }
                                                                footer_message = Some(format!(
                                                                    "spawned worktree {title}"
                                                                ));
                                                            }
                                                            Err(error) => {
                                                                footer_message = Some(format!(
                                                                    "spawn failed: {error}"
                                                                ))
                                                            }
                                                        }
                                                    }
                                                    Err(error) => {
                                                        footer_message = Some(format!(
                                                            "worktree failed: {error}"
                                                        ))
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
                                                    Ok(id) => {
                                                        save_managed_sessions(
                                                            manager.managed_session_ids(),
                                                        )?;
                                                        if let Some(summary) = manager
                                                            .sessions()
                                                            .items()
                                                            .iter()
                                                            .find(|s| s.id == id)
                                                            && let Some(port) = summary.serve_port
                                                        {
                                                            subscribers.push(
                                                                SessionEventSubscriber::start(
                                                                    port,
                                                                    event_tx.clone(),
                                                                ),
                                                            );
                                                        }
                                                        footer_message =
                                                            Some(format!("spawned {title}"));
                                                    }
                                                    Err(error) => {
                                                        footer_message =
                                                            Some(format!("spawn failed: {error}"))
                                                    }
                                                }
                                            }
                                            None => {
                                                footer_message = Some("worktree canceled".into())
                                            }
                                        }
                                    }
                                    Ok(None) => footer_message = Some("worktree canceled".into()),
                                    Err(error) => {
                                        footer_message = Some(format!("picker failed: {error}"))
                                    }
                                }
                            }
                            _ => {}
                        }
                    } else if matches!(state.focus, AppFocus::Conversation) {
                        match handle_conversation_key(
                            key,
                            &mut conversation,
                            &config.keybindings,
                            viewport_height,
                        ) {
                            KeyAction::Consumed => {}
                            KeyAction::Close => {
                                let _ = conversation.clear_kitty_graphics(terminal.backend_mut());
                                let return_focus = conversation.close();
                                state.last_main_focus = AppFocus::Terminal;
                                reduce(&mut state, Action::SetFocus(return_focus));
                                footer_message = None;
                            }
                            KeyAction::PasteSelection(text) => {
                                let _ = conversation.clear_kitty_graphics(terminal.backend_mut());
                                let return_focus = conversation.close();
                                state.last_main_focus = AppFocus::Terminal;
                                reduce(&mut state, Action::SetFocus(return_focus));
                                footer_message = None;
                                if let Some(pty) = manager.active_session_mut() {
                                    let _ = pty.send_paste(&text);
                                }
                            }
                            KeyAction::SelectionEmpty => {
                                footer_message = Some("No valid lines in selection".to_string());
                            }
                            KeyAction::NoDiagramInView => {
                                footer_message = Some("no diagram in view".to_string());
                            }
                            KeyAction::ConfirmQuit => {
                                state.confirm_quit = true;
                            }
                        }
                    } else if matches!(state.focus, AppFocus::Diff) {
                        match handle_diff_key(
                            key,
                            &mut diff_view,
                            &config.keybindings,
                            viewport_height,
                        ) {
                            KeyAction::Consumed => {}
                            KeyAction::Close => {
                                let return_focus = diff_view.close();
                                state.last_main_focus = AppFocus::Terminal;
                                reduce(&mut state, Action::SetFocus(return_focus));
                                footer_message = None;
                            }
                            KeyAction::PasteSelection(text) => {
                                let return_focus = diff_view.close();
                                state.last_main_focus = AppFocus::Terminal;
                                reduce(&mut state, Action::SetFocus(return_focus));
                                footer_message = None;
                                if let Some(pty) = manager.active_session_mut() {
                                    let _ = pty.send_paste(&text);
                                }
                            }
                            KeyAction::SelectionEmpty => {
                                footer_message = Some("No valid lines in selection".to_string());
                            }
                            KeyAction::NoDiagramInView => {
                                footer_message = Some("no diagram in view".to_string());
                            }
                            KeyAction::ConfirmQuit => {
                                state.confirm_quit = true;
                            }
                        }
                    } else if matches!(state.focus, AppFocus::Agents) {
                        match key.code {
                            KeyCode::Char('j') | KeyCode::Down => state.agents.move_down(),
                            KeyCode::Char('k') | KeyCode::Up => state.agents.move_up(),
                            KeyCode::Char('g') => state.agents.move_top(),
                            KeyCode::Char('G') => state.agents.move_bottom(),
                            KeyCode::Char('a') | KeyCode::Char('q') | KeyCode::Esc => {
                                let return_focus = state.agents.close();
                                state.last_main_focus = AppFocus::Terminal;
                                reduce(&mut state, Action::SetFocus(return_focus));
                            }
                            KeyCode::Enter => {
                                if let Some(node) = state.agents.selected_node().cloned()
                                    && let Some(session_id) = node.session_id
                                {
                                    let (rows, cols) =
                                        pane_size(terminal.size()?.into(), sidebar_width);
                                    if let Err(error) = manager.attach_arbitrary_session(
                                        session_id,
                                        node.cwd,
                                        node.title,
                                        SessionStatus::Idle,
                                        None,
                                        rows,
                                        cols,
                                    ) {
                                        footer_message = Some(format!("attach failed: {error}"));
                                    } else {
                                        if let Some(port) = manager
                                            .selected_summary()
                                            .and_then(|summary| summary.serve_port)
                                            && subscribed_serve_ports.insert(port)
                                        {
                                            subscribers.push(SessionEventSubscriber::start(
                                                port,
                                                event_tx.clone(),
                                            ));
                                        }
                                        reduce(&mut state, Action::SetFocus(AppFocus::Terminal));
                                    }
                                }
                            }
                            KeyCode::Char('v') => {
                                if let Some(node) = state.agents.selected_node().cloned()
                                    && let Some(session_id) = node.session_id
                                {
                                    conversation.open(session_id, node.title, AppFocus::Agents);
                                    reduce(&mut state, Action::SetFocus(AppFocus::Conversation));
                                }
                            }
                            KeyCode::Char('d') => {
                                if let Some(node) = state.agents.selected_node().cloned()
                                    && let Some(ref session_id) = node.session_id
                                {
                                    let row = agent_sidebar_row(&node, session_id.clone());
                                    match resolve_session_diff(&row, session_id) {
                                        Ok(diff) => {
                                            diff_view.open(
                                                session_id.clone(),
                                                node.title,
                                                diff,
                                                AppFocus::Agents,
                                            );
                                            let (doc, meta) = ui_diff::build_diff_document(
                                                diff_view.raw_diff(),
                                                content_width,
                                            );
                                            diff_view.replace_document(doc, meta, viewport_height);
                                            reduce(&mut state, Action::SetFocus(AppFocus::Diff));
                                        }
                                        Err(error) => footer_message = Some(error),
                                    }
                                }
                            }
                            _ => {}
                        }
                    } else if matches!(state.focus, AppFocus::Terminal)
                        && let Some(pty) = manager.active_session_mut()
                        && let Err(error) = pty.send_key(key)
                    {
                        footer_message = Some(format!("terminal write failed: {error}"));
                    }
                }
                Event::Paste(text) => {
                    terminal_selection.clear();
                    if state.playbook_form.is_some() {
                        if let Some(form) = state.playbook_form.as_mut() {
                            form.insert_str(&text);
                        }
                    } else if matches!(state.focus, AppFocus::Conversation)
                        && conversation.is_searching()
                    {
                        conversation.search_insert_str(&text, viewport_height);
                    } else if matches!(state.focus, AppFocus::Diff) && diff_view.is_searching() {
                        diff_view.search_insert_str(&text, viewport_height);
                    } else if matches!(state.focus, AppFocus::Terminal)
                        && let Some(pty) = manager.active_session_mut()
                        && let Err(error) = pty.send_paste(&text)
                    {
                        footer_message = Some(format!("paste failed: {error}"));
                    }
                }
                Event::Mouse(mouse) => {
                    let inner = terminal_inner_rect(terminal.size()?.into(), sidebar_width, 1);
                    let content = conversation.content_rect(inner);
                    let surface_size = manager
                        .active_session()
                        .map(|s| (s.surface.rows(), s.surface.cols()));

                    let mut handled = false;
                    if conversation.is_active() && !conversation.is_lightbox() {
                        match mouse.kind {
                            MouseEventKind::Down(MouseButton::Left)
                                if state.focus == AppFocus::Conversation
                                    && content.contains((mouse.column, mouse.row).into()) =>
                            {
                                if let Some(pos) =
                                    conversation.pos_for_mouse(mouse.column, mouse.row, content)
                                {
                                    conversation.begin_selection(pos, (mouse.column, mouse.row));
                                }
                                handled = true;
                            }
                            MouseEventKind::Drag(MouseButton::Left)
                                if conversation.is_dragging() =>
                            {
                                if let Some(pos) =
                                    conversation.pos_for_mouse(mouse.column, mouse.row, content)
                                {
                                    conversation.extend_selection(pos, (mouse.column, mouse.row));
                                }
                                handled = true;
                            }
                            // Finish the drag even if focus moved, so it can
                            // never get stuck open.
                            MouseEventKind::Up(MouseButton::Left) if conversation.is_dragging() => {
                                if let Some(pos) =
                                    conversation.pos_for_mouse(mouse.column, mouse.row, content)
                                {
                                    conversation.extend_selection(pos, (mouse.column, mouse.row));
                                }
                                if let Some(text) = conversation.end_selection()
                                    && let Err(e) = clipboard::copy_to_clipboard(&text)
                                {
                                    footer_message = Some(format!("copy failed: {e}"));
                                }
                                handled = true;
                            }
                            _ => {}
                        }
                    }
                    if !state.focus.overlay_owns_main()
                        && let Some((rows, cols)) = surface_size
                    {
                        match terminal_selection.handle_mouse(mouse, inner, rows, cols) {
                            MouseResult::Claimed => {
                                state.focus = AppFocus::Terminal;
                                handled = true;
                            }
                            MouseResult::Finished => {
                                let session = manager.active_session().unwrap();
                                let snapshot = session.surface.snapshot();
                                let wrapped = session.surface.wrapped_rows();
                                if let Some(text) =
                                    terminal_selection.extract_text_from(&snapshot, &wrapped)
                                    && !text.is_empty()
                                    && let Err(e) = clipboard::copy_to_clipboard(&text)
                                {
                                    footer_message = Some(format!("copy failed: {e}"));
                                }
                                state.focus = AppFocus::Terminal;
                                handled = true;
                            }
                            MouseResult::Click { col, row } => {
                                state.focus = AppFocus::Terminal;
                                handled = true;
                                let col1 = (col as u16).saturating_add(1);
                                let row1 = (row as u16).saturating_add(1);
                                if let Some(pty) = manager.active_session_mut() {
                                    let bytes = input::mouse_click_press_release_sgr(
                                        col1,
                                        row1,
                                        mouse.modifiers,
                                    );
                                    if let Err(e) = pty.send_bytes(&bytes) {
                                        footer_message = Some(format!("mouse click failed: {e}"));
                                    }
                                }
                            }
                            MouseResult::Ignored => {}
                        }
                    }

                    if !handled {
                        if matches!(mouse.kind, MouseEventKind::Down(_)) {
                            terminal_selection.clear();
                        }

                        // Forward right/middle clicks if over terminal pane
                        let over_terminal = input::screen_to_pty_cell(
                            mouse.column,
                            mouse.row,
                            inner.x,
                            inner.y,
                            inner.width,
                            inner.height,
                        );

                        if matches!(mouse.kind, MouseEventKind::Down(_))
                            && mouse.column < sidebar_width
                        {
                            state.focus = AppFocus::Sidebar;
                            let clicked_row = mouse.row.saturating_sub(1) as usize;
                            if clicked_row < rows.len() {
                                state.selected_sidebar_row = clicked_row;
                                if let Some(row) = rows.get(state.selected_sidebar_row)
                                    && let Some(top_level_id) =
                                        follow_attach_id(config.follow_selection, row)
                                {
                                    let (pty_rows, pty_cols) =
                                        pane_size(terminal.size()?.into(), config.sidebar_width);
                                    manager.select_top_level(top_level_id);
                                    if let Err(error) =
                                        manager.activate_or_attach_selected(pty_rows, pty_cols)
                                    {
                                        footer_message = Some(format!("attach failed: {error}"));
                                    }
                                }
                            }
                        } else if !state.focus.overlay_owns_main()
                            && let Some((col, row)) = over_terminal
                            && matches!(
                                mouse.kind,
                                MouseEventKind::Down(MouseButton::Right | MouseButton::Middle)
                                    | MouseEventKind::Up(MouseButton::Right | MouseButton::Middle)
                            )
                        {
                            if let Some(bytes) = input::mouse_event_to_sgr_bytes(
                                mouse.kind,
                                col,
                                row,
                                mouse.modifiers,
                            ) && let Some(pty) = manager.active_session_mut()
                            {
                                let _ = pty.send_bytes(&bytes);
                            }
                        } else if matches!(
                            mouse.kind,
                            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                        ) {
                            let scroll_amount = 3;
                            match state.focus {
                                AppFocus::Conversation if !conversation.is_lightbox() => {
                                    match mouse.kind {
                                        MouseEventKind::ScrollUp => {
                                            conversation.scroll_up(scroll_amount);
                                        }
                                        MouseEventKind::ScrollDown => {
                                            conversation
                                                .scroll_down(scroll_amount, viewport_height);
                                        }
                                        _ => {}
                                    }
                                }
                                AppFocus::Diff => match mouse.kind {
                                    MouseEventKind::ScrollUp => {
                                        diff_view.scroll_view_up(scroll_amount, viewport_height);
                                    }
                                    MouseEventKind::ScrollDown => {
                                        diff_view.scroll_view_down(scroll_amount, viewport_height);
                                    }
                                    _ => {}
                                },
                                AppFocus::Terminal => {
                                    if let Some((col, row)) = input::screen_to_pty_cell(
                                        mouse.column,
                                        mouse.row,
                                        inner.x,
                                        inner.y,
                                        inner.width,
                                        inner.height,
                                    ) && let Some(bytes) = input::mouse_scroll_to_sgr_bytes(
                                        mouse.kind,
                                        col,
                                        row,
                                        mouse.modifiers,
                                    ) {
                                        terminal_selection.clear();
                                        if let Some(pty) = manager.active_session_mut()
                                            && let Err(error) = pty.send_bytes(&bytes)
                                        {
                                            footer_message =
                                                Some(format!("mouse scroll failed: {error}"));
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Event::FocusGained => {
                    terminal_selection.clear();
                    state.app_focused = true;
                }
                Event::FocusLost => {
                    terminal_selection.clear();
                    state.app_focused = false;
                }
                Event::Resize(width, height) => {
                    terminal_selection.clear();
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
            }

            if matches!(state.focus, AppFocus::Terminal)
                && let Some(active_id) = manager.active_id()
                    && let Some(idx) = rows.iter().position(|r| {
                        matches!(&r.kind, SidebarRowKind::TopLevel { top_level_id, .. } if *top_level_id == active_id)
                    }) {
                        state.selected_sidebar_row = idx;
                    }
            prev_selected_kind = rows.get(state.selected_sidebar_row).map(|r| r.kind.clone());
        }

        Ok(())
    })();

    manager.shutdown_local_ptys();
    let _ = manager.shutdown_managed_serves();
    poller.stop();
    // Drop subscribers: each sends a best-effort stop and detaches its thread
    // (joining would hang on the blocking SSE read). Threads die at process exit.
    subscribers.clear();

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

fn selected_root_summary(
    manager: &PtyManager,
    row: &opencode_multiplexer::ui::sidebar::SidebarVisibleRow,
) -> Option<opencode_multiplexer::app::sessions::SessionSummary> {
    let session = manager
        .sessions()
        .items()
        .iter()
        .find(|summary| match &row.kind {
            SidebarRowKind::TopLevel { top_level_id, .. } => summary.id == *top_level_id,
            SidebarRowKind::Child { session_id } => summary
                .children
                .iter()
                .any(|child| contains_child(child, session_id)),
        })?;
    Some(session.clone())
}

fn prepare_playbook_target(
    manager: &mut PtyManager,
    summary: &opencode_multiplexer::app::sessions::SessionSummary,
) -> Result<SessionTarget, String> {
    let (port, pid) = match summary.serve_port {
        Some(port) => (port, summary.serve_pid),
        None => {
            let (port, pid) =
                opencode_multiplexer::ops::opencode::ensure_serve_for_cwd(&summary.cwd)
                    .map_err(|error| error.to_string())?;
            (port, Some(pid))
        }
    };

    if summary.serve_port != Some(port) {
        if let Some(pid) = pid {
            manager.set_session_serve(summary.id, port, pid);
        }
    }

    let session_id = match summary.session_id.clone() {
        Some(session_id) => session_id,
        None => {
            let session_id = opencode_multiplexer::playbook::http::create_session(port)
                .map_err(|error| error.to_string())?;
            manager.set_session_id(summary.id, session_id.clone());
            session_id
        }
    };

    Ok(SessionTarget {
        session_id: Some(session_id),
        serve_port: Some(port),
        cwd: summary.cwd.clone(),
    })
}

fn agent_sidebar_row(
    node: &opencode_multiplexer::app::agents::AgentNode,
    session_id: String,
) -> opencode_multiplexer::ui::sidebar::SidebarVisibleRow {
    opencode_multiplexer::ui::sidebar::SidebarVisibleRow {
        kind: SidebarRowKind::Child {
            session_id: session_id.clone(),
        },
        cwd: node.cwd.clone(),
        title: node.title.clone(),
        status: SessionStatus::Idle,
        depth: node.depth,
        has_children: false,
        expanded: false,
        active: false,
        origin: opencode_multiplexer::app::sessions::SessionOrigin::Managed,
        session_id: Some(session_id),
        time_updated: None,
    }
}

fn contains_child(
    child: &opencode_multiplexer::data::poller::ChildSessionInfo,
    session_id: &str,
) -> bool {
    child.session_id == session_id
        || child
            .children
            .iter()
            .any(|nested| contains_child(nested, session_id))
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
        DisableMouseCapture,
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
        EnableMouseCapture,
        crossterm::event::EnableFocusChange
    )?;
    enable_raw_mode()?;
    terminal.clear()?;
    Ok(())
}
