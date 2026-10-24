use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::{
    app::{
        agents::AgentsViewState,
        conversation::ConversationViewState,
        diff::DiffViewState,
        focus::AppFocus,
        message_picker::MessagePickerState,
        playbook::{ParamFormState, PlaybookPickerState},
        session_manager::SessionManagerState,
        session_picker::SessionPickerState,
    },
    config::Keybindings,
    terminal::{manager::PtyManager, renderer::TerminalWidget, selection::SelectionRange},
    ui::{
        conversation::selection::highlight_rows,
        diff::apply_cursor_and_selection,
        diff::highlight_search_matches,
        hints::footer_line,
        layout::{centered_rect, split_root, terminal_inner_rect},
        message_picker::render_message_picker,
        session_manager::render_session_manager,
        session_picker::render_session_picker,
        sidebar::{SidebarVisibleRow, render_sidebar, repo_root_name},
    },
};

#[allow(clippy::too_many_arguments)]
pub fn render(
    frame: &mut Frame,
    focus: AppFocus,
    selected: usize,
    rows: &[SidebarVisibleRow],
    manager: &PtyManager,
    footer_message: Option<&str>,
    keys: &Keybindings,
    show_help: bool,
    show_files: &[String],
    sidebar_width: u16,
    panel_hidden: bool,
    app_focused: bool,
    conversation: &ConversationViewState,
    diff: &DiffViewState,
    agents: &AgentsViewState,
    session_picker: Option<&mut SessionPickerState>,
    session_manager: Option<&mut SessionManagerState>,
    message_picker: Option<&mut MessagePickerState>,
    playbook_picker: Option<&PlaybookPickerState>,
    playbook_form: Option<&ParamFormState>,
    playbook_abort: bool,
    confirm_quit: bool,
    terminal_selection: Option<SelectionRange>,
) {
    let layout = split_root(frame.area(), sidebar_width, 1);
    if !panel_hidden {
        frame.render_widget(
            render_sidebar(rows, selected, focus, sidebar_width, app_focused),
            layout.sidebar,
        );
    }
    let footer = footer_line(
        focus,
        footer_message,
        keys,
        diff.is_visual(),
        conversation.is_lightbox(),
    );
    let footer = if matches!(focus, AppFocus::Conversation)
        && conversation.is_active()
        && footer_message.is_none()
    {
        let status = crate::ui::conversation::conversation_status_line(
            conversation.scroll_offset(),
            conversation.document_len(),
            conversation.is_following_tail(),
            conversation.match_status(),
        );
        crate::ui::hints::compose_footer(footer, Some(status), layout.footer.width)
    } else {
        footer
    };
    frame.render_widget(footer, layout.footer);

    let main_border_style = if !app_focused {
        Style::default().fg(Color::DarkGray)
    } else if matches!(
        focus,
        AppFocus::Terminal | AppFocus::Conversation | AppFocus::Diff | AppFocus::Agents
    ) {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let mode_tag = match focus {
        AppFocus::Sidebar => " opencode ",
        AppFocus::Terminal => " opencode [live] ",
        AppFocus::Conversation => " opencode [read-only] ",
        AppFocus::Diff => " opencode [diff] ",
        AppFocus::Agents => " opencode [agents] ",
    };
    let title_text = if let Some(summary) = manager.active_summary() {
        let repo = repo_root_name(&summary.cwd);
        let title = &summary.title;
        let dir = summary
            .cwd
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?");
        let meta = format!("{repo} \u{2502} {title} \u{2502} {dir} ");
        let max_meta = (layout.main.width as usize).saturating_sub(mode_tag.len() + 4); // 4 for "── " + trailing
        if max_meta >= meta.len() {
            format!("{mode_tag}\u{2500}\u{2500} {meta}")
        } else if max_meta > repo.len() + 6 {
            // Truncate: drop dir, truncate title
            let available = max_meta.saturating_sub(repo.len() + 5); // " │  │ " overhead without dir
            let truncated_title: String = title.chars().take(available.saturating_sub(1)).collect();
            format!("{mode_tag}\u{2500}\u{2500} {repo} \u{2502} {truncated_title}\u{2026} ")
        } else {
            mode_tag.to_string()
        }
    } else {
        mode_tag.to_string()
    };
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(main_border_style)
        .title(Span::styled(title_text, main_border_style));
    frame.render_widget(block, layout.main);

    let inner = terminal_inner_rect(frame.area(), sidebar_width, 1);
    let _viewport_height = inner.height as usize;

    if matches!(focus, AppFocus::Conversation) && conversation.is_active() {
        if let Some(error) = conversation.load_error() {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    format!("Error: {error}"),
                    Style::default().fg(Color::Red),
                )),
                inner,
            );
        } else {
            let has_search = conversation.is_searching() || !conversation.search_query().is_empty();

            let content_area = conversation.content_rect(inner);
            let search_bar_area = has_search
                .then(|| Rect::new(inner.x, inner.y + content_area.height, inner.width, 1));

            let content_vp = content_area.height as usize;
            let mut rows = conversation.visible_rows(content_vp);

            if !conversation.search_query().is_empty() {
                let lines: Vec<Line<'static>> = rows.iter().map(|r| r.line.clone()).collect();
                let highlighted = highlight_search_matches(
                    &lines,
                    conversation.scroll_offset(),
                    conversation.matches(),
                    conversation.current_match_index(),
                );
                for (row, line) in rows.iter_mut().zip(highlighted) {
                    row.line = line;
                }
            }
            if let Some(selection) = conversation.selection() {
                highlight_rows(&mut rows, conversation.scroll_offset(), selection);
            }
            let visible: Vec<Line<'static>> = rows.into_iter().map(|r| r.line).collect();

            frame.render_widget(Paragraph::new(visible), content_area);

            if let Some(bar) = search_bar_area {
                let query = conversation.search_query();
                let status = conversation
                    .match_status()
                    .map(|(cur, total)| format!(" {cur}/{total}"))
                    .unwrap_or_else(|| {
                        if query.is_empty() {
                            String::new()
                        } else {
                            " 0/0".to_string()
                        }
                    });

                let prompt = format!("/{query}");
                let available = (bar.width as usize).saturating_sub(status.len());
                let prompt_display = if prompt.len() > available {
                    prompt[prompt.len() - available..].to_string()
                } else {
                    prompt.clone()
                };

                let bar_line = Line::from(vec![
                    Span::styled(
                        prompt_display,
                        Style::default().fg(Color::White).bg(Color::DarkGray),
                    ),
                    Span::styled(
                        format!(
                            "{:<fill$}",
                            "",
                            fill = available.saturating_sub(prompt.len())
                        ),
                        Style::default().bg(Color::DarkGray),
                    ),
                    Span::styled(
                        status,
                        Style::default().fg(Color::Yellow).bg(Color::DarkGray),
                    ),
                ]);

                frame.render_widget(Paragraph::new(bar_line), bar);

                if conversation.is_searching() {
                    let cursor_x = bar.x + prompt.len().min(available) as u16;
                    let cursor_y = bar.y;
                    if cursor_x < bar.right() {
                        frame.set_cursor_position(ratatui::layout::Position::new(
                            cursor_x, cursor_y,
                        ));
                    }
                }
            }
        }
    } else if matches!(focus, AppFocus::Diff) && diff.is_active() {
        let has_search = diff.is_searching() || !diff.search_query().is_empty();

        // Split inner area: content + optional search bar at the bottom.
        let (content_area, search_bar_area) = if has_search {
            let bar_height = 1u16;
            let content_h = inner.height.saturating_sub(bar_height);
            (
                Rect::new(inner.x, inner.y, inner.width, content_h),
                Some(Rect::new(
                    inner.x,
                    inner.y + content_h,
                    inner.width,
                    bar_height,
                )),
            )
        } else {
            (inner, None)
        };

        let content_vp = content_area.height as usize;
        let visible = diff.visible_lines(content_vp);

        // Apply search highlights.
        let visible = if !diff.search_query().is_empty() {
            highlight_search_matches(
                &visible,
                diff.scroll_offset(),
                diff.matches(),
                diff.current_match_index(),
            )
        } else {
            visible
        };

        // Apply cursor and visual-selection highlights.
        let visible = apply_cursor_and_selection(
            visible,
            diff.scroll_offset(),
            diff.cursor(),
            diff.selection_range(),
        );

        frame.render_widget(Paragraph::new(visible), content_area);

        // Render search bar.
        if let Some(bar) = search_bar_area {
            let query = diff.search_query();
            let status = diff
                .match_status()
                .map(|(cur, total)| format!(" {cur}/{total}"))
                .unwrap_or_else(|| {
                    if query.is_empty() {
                        String::new()
                    } else {
                        " 0/0".to_string()
                    }
                });

            let prompt = format!("/{query}");
            let available = (bar.width as usize).saturating_sub(status.len());
            let prompt_display = if prompt.len() > available {
                prompt[prompt.len() - available..].to_string()
            } else {
                prompt.clone()
            };

            let bar_line = Line::from(vec![
                Span::styled(
                    prompt_display,
                    Style::default().fg(Color::White).bg(Color::DarkGray),
                ),
                Span::styled(
                    format!(
                        "{:<fill$}",
                        "",
                        fill = available.saturating_sub(prompt.len())
                    ),
                    Style::default().bg(Color::DarkGray),
                ),
                Span::styled(
                    status,
                    Style::default().fg(Color::Yellow).bg(Color::DarkGray),
                ),
            ]);

            frame.render_widget(Paragraph::new(bar_line), bar);

            // Show cursor in the search input.
            if diff.is_searching() {
                let cursor_x = bar.x + prompt.len().min(available) as u16;
                let cursor_y = bar.y;
                if cursor_x < bar.right() {
                    frame.set_cursor_position(ratatui::layout::Position::new(cursor_x, cursor_y));
                }
            }
        }
    } else if matches!(focus, AppFocus::Agents) {
        crate::ui::agents::render_agents(frame, inner, agents.graph.as_ref(), agents.selected);
    } else if let Some(pty) = manager.active_session() {
        frame.render_widget(Paragraph::new(""), inner);
        frame.render_widget(
            TerminalWidget::new(&pty.surface).with_selection(terminal_selection),
            inner,
        );
        if matches!(focus, AppFocus::Terminal) {
            let (cursor_row, cursor_col) = pty.surface.cursor();
            let x = inner.x + cursor_col as u16;
            let y = inner.y + cursor_row as u16;
            if x < inner.right() && y < inner.bottom() {
                frame.set_cursor_position(ratatui::layout::Position::new(x, y));
            }
        }
    } else if manager.selected_summary().is_some() {
        frame.render_widget(
            Paragraph::new("Press Enter to attach to this session."),
            inner,
        );
    } else {
        frame.render_widget(
            Paragraph::new("No opencode sessions found. Press n to spawn."),
            inner,
        );
    }

    // Lightbox overlay for mermaid diagrams.
    if matches!(focus, AppFocus::Conversation) && conversation.is_lightbox() {
        let body = ratatui::layout::Rect::new(
            frame.area().x,
            frame.area().y,
            frame.area().width,
            frame.area().height.saturating_sub(1),
        );
        let popup = centered_rect(body, 96, 96);
        frame.render_widget(Clear, popup);
        let block = Block::bordered().title(" mermaid ");
        let block_inner = block.inner(popup);
        frame.render_widget(block, popup);
        if !conversation.protocol_enabled()
            && let Some(lines) = conversation.lightbox_halfblock_lines(block_inner)
        {
            frame.render_widget(Paragraph::new(lines), block_inner);
        }
    }

    if show_help {
        let help = Paragraph::new(help_text(keys)).block(Block::bordered().title("help"));
        let popup = centered_rect(frame.area(), 62, 78);
        frame.render_widget(Clear, popup);
        frame.render_widget(help, popup);
    }

    if !show_files.is_empty() {
        let content = show_files.join("\n");
        let files_widget = Paragraph::new(content)
            .block(Block::bordered().title(format!("files ({})", show_files.len())));
        let popup = centered_rect(frame.area(), 70, 60);
        frame.render_widget(Clear, popup);
        frame.render_widget(files_widget, popup);
    }

    if let Some(manager) = session_manager {
        let popup = centered_rect(frame.area(), 85, 80);
        frame.render_widget(Clear, popup);
        render_session_manager(frame, manager, popup);
    } else if let Some(picker) = session_picker {
        let popup = centered_rect(frame.area(), 80, 70);
        frame.render_widget(Clear, popup);
        render_session_picker(frame, picker, popup);
    } else if let Some(picker) = message_picker {
        let popup = centered_rect(frame.area(), 80, 70);
        frame.render_widget(Clear, popup);
        render_message_picker(frame, picker, popup);
    } else if let Some(picker) = playbook_picker {
        let popup = centered_rect(frame.area(), 70, 60);
        frame.render_widget(Clear, popup);
        crate::ui::playbook::render_playbook_picker(frame, picker, popup);
    } else if let Some(form) = playbook_form {
        let popup = centered_rect(frame.area(), 60, 50);
        frame.render_widget(Clear, popup);
        crate::ui::playbook::render_param_form(frame, form, popup);
    }

    if confirm_quit {
        let quit_msg = Paragraph::new("Quit ocmux and stop managed serves?\ny confirm\nn cancel")
            .block(Block::bordered().title("quit"));
        let popup = centered_rect(frame.area(), 30, 20);
        frame.render_widget(Clear, popup);
        frame.render_widget(quit_msg, popup);
    }

    if playbook_abort {
        let popup = centered_rect(frame.area(), 40, 25);
        crate::ui::playbook::render_abort_panel(frame, popup);
    }
}

fn help_text(keys: &Keybindings) -> String {
    let lines = [
        "Sidebar".to_string(),
        "  Ctrl-\\           focus sidebar/terminal".into(),
        "  Ctrl-h           hide/show sidebar".into(),
        format!("  {}/{} · ↑/↓     move selection", keys.down, keys.up),
        format!(
            "  {:<16} cycle session (keep focus)",
            keys.session_next.display()
        ),
        format!(
            "  {:<16} cycle session (keep focus)",
            keys.session_prev.display()
        ),
        "  Enter            attach / open session".into(),
        "  Tab              expand/collapse children".into(),
        format!("  {:<16} spawn new session", keys.spawn),
        format!("  {:<16} worktree + spawn", keys.worktree),
        format!("  {:<16} move session directory", keys.move_session),
        format!("  {:<16} agents relationship view", keys.agents),
        format!("  {:<16} conversation view", keys.view),
        format!("  {:<16} diff view", keys.diff),
        format!("  {:<16} list modified files", keys.files),
        "  /                session picker (attach)".into(),
        format!("  {:<16} session manager (delete)", keys.sessions),
        format!("  {:<16} message history picker", keys.history),
        "  r                refresh active session".into(),
        "  !                shell in session directory".into(),
        "  c                commit/push modified files".into(),
        format!("  {:<16} kill session (y confirm)", keys.kill),
        format!("  {:<16} toggle this help", keys.help),
        format!("  {:<16} quit + stop serves (y confirm)", keys.quit),
        String::new(),
        "Agents view".into(),
        "  j/k · ↑/↓        move  ·  g/G top/end".into(),
        "  Enter            attach to linked session".into(),
        "  v                conversation  ·  d diff".into(),
        "  a/q/Esc          back".into(),
        String::new(),
        "Conversation view".into(),
        "  j/k · ↑/↓        scroll  ·  g/G top/end".into(),
        "  Ctrl-u/d         page up/down".into(),
        "  /                search  ·  n/N next/prev".into(),
        "  z                enlarge mermaid diagram".into(),
        format!("  {}/q/Esc          close", keys.view),
        String::new(),
        "Diff view".into(),
        "  j/k · ↑/↓        move cursor  ·  g/G top/end".into(),
        "  Ctrl-u/d         page  ·  Ctrl-y/e scroll view".into(),
        "  /                search  ·  n/N next/prev".into(),
        "  v                visual select  ·  Enter paste".into(),
        format!("  {}/q/Esc          close", keys.diff),
        String::new(),
        "Pickers (session / history / manager)".into(),
        "  type             fuzzy filter".into(),
        "  ↑/↓              move  ·  Enter confirm".into(),
        "  Esc              cancel".into(),
        "  manager: Tab select · Ctrl-a all · Ctrl-u clear · Ctrl-d delete".into(),
        String::new(),
        "Esc or any key closes this help.".into(),
    ];
    lines.join("\n")
}
