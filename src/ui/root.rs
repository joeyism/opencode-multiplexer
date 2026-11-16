use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaneChrome {
    Focused,
    RememberedTarget,
    Inactive,
}

fn pane_chrome(target: bool, focus: AppFocus, app_focused: bool) -> PaneChrome {
    if !target {
        PaneChrome::Inactive
    } else if matches!(
        focus,
        AppFocus::Terminal | AppFocus::Conversation | AppFocus::Diff | AppFocus::Agents
    ) && app_focused
    {
        PaneChrome::Focused
    } else {
        PaneChrome::RememberedTarget
    }
}

use crate::{
    app::{
        agents::AgentsViewState,
        conversation::ConversationViewState,
        diff::DiffViewState,
        focus::AppFocus,
        help::HelpState,
        message_picker::MessagePickerState,
        playbook::{ParamFormState, PlaybookPickerState},
        session_manager::SessionManagerState,
        session_picker::SessionPickerState,
        split_picker::{SplitChoice, SplitPickerState},
        workspace::{PaneContent, Workspace},
    },
    config::Keybindings,
    terminal::{
        manager::PtyManager, renderer::TerminalWidget, selection::SelectionRange,
        shells::ShellManager,
    },
    ui::{
        conversation::selection::highlight_rows,
        diff::apply_cursor_and_selection,
        diff::highlight_search_matches,
        hints::footer_line,
        layout::{
            WorkspaceChrome, centered_rect, layout_workspace, pane_content_rect, split_root,
            terminal_inner_rect, workspace_chrome, workspace_regions,
        },
        message_picker::render_message_picker,
        session_manager::render_session_manager,
        session_picker::render_session_picker,
        sidebar::{SidebarVisibleRow, render_sidebar_column, repo_root_name},
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
    help: Option<&HelpState>,
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
    workspace: Option<&Workspace>,
    shells: &ShellManager,
    split_picker: Option<&SplitPickerState>,
    close_shell_pending: bool,
    vitals: Option<&crate::data::vitals::SessionVitals>,
    vitals_mode: crate::ui::vitals::VitalsMode,
) {
    let layout = split_root(frame.area(), sidebar_width, 1);
    if !panel_hidden {
        render_sidebar_column(
            frame,
            layout.sidebar,
            rows,
            selected,
            focus,
            sidebar_width,
            app_focused,
            vitals,
            vitals_mode,
        );
    }
    let footer = footer_line(
        focus,
        footer_message,
        keys,
        diff.is_visual(),
        if conversation.is_lightbox() {
            if conversation.protocol_enabled() {
                crate::ui::hints::LightboxHints::Zoomable
            } else {
                crate::ui::hints::LightboxHints::Fixed
            }
        } else {
            crate::ui::hints::LightboxHints::None
        },
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

    let main_inner = terminal_inner_rect(frame.area(), sidebar_width, 1);
    let pane_overlay = workspace.is_some()
        && matches!(
            focus,
            AppFocus::Conversation | AppFocus::Diff | AppFocus::Agents
        );
    let inner = if pane_overlay {
        let workspace = workspace.unwrap();
        let regions = workspace_regions(workspace, main_inner).unwrap();
        let target = regions.target_pane;
        render_workspace_terminals(
            frame,
            main_inner,
            manager,
            shells,
            workspace,
            terminal_selection,
            Some(target),
            focus,
            app_focused,
        );
        let pane_area = regions.target_area;
        if workspace_chrome(workspace) == WorkspaceChrome::Flat {
            main_inner
        } else {
            let title = workspace_pane_title(workspace, target, manager);
            let chrome = pane_chrome(true, focus, app_focused);
            let style = match chrome {
                PaneChrome::Focused => Style::default().fg(Color::Cyan),
                PaneChrome::RememberedTarget | PaneChrome::Inactive => {
                    Style::default().fg(Color::DarkGray)
                }
            };
            let title = if chrome == PaneChrome::RememberedTarget {
                format!("{} · target ", title.trim())
            } else {
                title
            };
            let block = Block::bordered().title(title).border_style(style);
            frame.render_widget(block, pane_area);
            pane_content_rect(WorkspaceChrome::Framed, pane_area)
        }
    } else {
        main_inner
    };
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
    } else if let Some(workspace) = workspace {
        render_workspace_terminals(
            frame,
            inner,
            manager,
            shells,
            workspace,
            terminal_selection,
            None,
            focus,
            app_focused,
        );
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
        let block_inner = crate::ui::layout::lightbox_inner_rect(frame.area());
        let popup = ratatui::layout::Rect::new(
            block_inner.x.saturating_sub(1),
            block_inner.y.saturating_sub(1),
            block_inner.width.saturating_add(2),
            block_inner.height.saturating_add(2),
        );
        frame.render_widget(Clear, popup);
        let block = Block::bordered().title(" mermaid ");
        frame.render_widget(block, popup);
        if !conversation.protocol_enabled()
            && let Some(lines) = conversation.lightbox_halfblock_lines(block_inner)
        {
            frame.render_widget(Paragraph::new(lines), block_inner);
        }
    }

    if let Some(picker) = split_picker {
        let mut lines = vec![Line::from("Select a session or shell (Esc cancels):")];
        for choice in picker.choices() {
            let label = match choice {
                SplitChoice::Session { title, .. } => title.clone(),
                SplitChoice::Shell => "Shell".into(),
            };
            let style = if picker.selected() == Some(choice) {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            };
            lines.push(Line::from(Span::styled(format!("  {label}"), style)));
        }
        let popup = centered_rect(frame.area(), 50, 40);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(lines).block(Block::bordered().title(" split pane ")),
            popup,
        );
    }

    if close_shell_pending {
        let popup = centered_rect(frame.area(), 44, 20);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new("Terminate this shell pane and its process? (y/N)")
                .block(Block::bordered().title(" close shell ")),
            popup,
        );
    }

    if let Some(help) = help {
        render_help_overlay(frame, keys, help.scroll_offset());
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

#[allow(clippy::too_many_arguments)]
fn render_workspace_terminals(
    frame: &mut Frame,
    area: Rect,
    manager: &PtyManager,
    shells: &ShellManager,
    workspace: &Workspace,
    terminal_selection: Option<SelectionRange>,
    skip_content: Option<crate::app::workspace::PaneId>,
    focus: AppFocus,
    app_focused: bool,
) {
    for (pane_id, pane_area) in layout_workspace(workspace, area) {
        if skip_content == Some(pane_id) {
            continue;
        }
        let content = workspace.content(pane_id).unwrap_or(PaneContent::Empty);
        let target = pane_id == workspace.target_pane();
        let chrome = pane_chrome(target, focus, app_focused);
        let style = match chrome {
            PaneChrome::Focused => Style::default().fg(Color::Cyan),
            PaneChrome::RememberedTarget | PaneChrome::Inactive => {
                Style::default().fg(Color::DarkGray)
            }
        };
        let title = match content {
            PaneContent::Empty => " empty ".to_string(),
            PaneContent::Session(id) => manager
                .summary_by_entry_id(id)
                .map(|summary| format!(" {} ", summary.title))
                .unwrap_or_else(|| " session unavailable ".to_string()),
            PaneContent::Shell(id) => format!(" shell:{id} "),
        };
        let title = if chrome == PaneChrome::RememberedTarget {
            format!("{} · target ", title.trim())
        } else {
            title
        };
        let chrome_mode = workspace_chrome(workspace);
        let inner = match chrome_mode {
            WorkspaceChrome::Flat => pane_area,
            WorkspaceChrome::Framed => {
                let block = Block::bordered().title(title).border_style(style);
                frame.render_widget(block, pane_area);
                pane_content_rect(chrome_mode, pane_area)
            }
        };
        match content {
            PaneContent::Session(id) => {
                if let Some(pty) = manager.session_by_entry_id(id) {
                    frame.render_widget(
                        TerminalWidget::new(&pty.surface).with_selection(
                            if chrome == PaneChrome::Focused {
                                terminal_selection
                            } else {
                                None
                            },
                        ),
                        inner,
                    );
                    if chrome == PaneChrome::Focused {
                        let (row, col) = pty.surface.cursor();
                        let x = inner.x + col as u16;
                        let y = inner.y + row as u16;
                        if x < inner.right() && y < inner.bottom() {
                            frame.set_cursor_position(ratatui::layout::Position::new(x, y));
                        }
                    }
                } else {
                    frame.render_widget(Paragraph::new("Press Enter to attach."), inner);
                }
            }
            PaneContent::Shell(id) => {
                if let Some(shell) = shells.terminal(id) {
                    frame.render_widget(TerminalWidget::new(&shell.surface), inner);
                    if chrome == PaneChrome::Focused {
                        let (row, col) = shell.surface.cursor();
                        let x = inner.x + col as u16;
                        let y = inner.y + row as u16;
                        if x < inner.right() && y < inner.bottom() {
                            frame.set_cursor_position(ratatui::layout::Position::new(x, y));
                        }
                    }
                } else {
                    frame.render_widget(Paragraph::new("Shell exited."), inner);
                }
            }
            PaneContent::Empty => {
                frame.render_widget(Paragraph::new("Choose a session or shell."), inner);
            }
        }
    }
}

fn workspace_pane_title(
    workspace: &Workspace,
    pane_id: crate::app::workspace::PaneId,
    manager: &PtyManager,
) -> String {
    match workspace.content(pane_id).unwrap_or(PaneContent::Empty) {
        PaneContent::Empty => " empty ".to_string(),
        PaneContent::Session(id) => manager
            .summary_by_entry_id(id)
            .map(|summary| format!(" {} ", summary.title))
            .unwrap_or_else(|| " session unavailable ".to_string()),
        PaneContent::Shell(id) => format!(" shell:{id} "),
    }
}

pub fn help_popup_rect(area: Rect) -> Rect {
    centered_rect(area, 62, 78)
}

pub fn help_line_count(keys: &Keybindings) -> usize {
    help_text(keys).lines().count()
}

pub fn help_viewport_height(area: Rect) -> usize {
    Block::bordered().inner(help_popup_rect(area)).height as usize
}

fn render_help_overlay(frame: &mut Frame, keys: &Keybindings, scroll: usize) {
    let popup = help_popup_rect(frame.area());
    let total = help_line_count(keys);
    let viewport = Block::bordered().inner(popup).height as usize;
    let scroll = scroll.min(total.saturating_sub(viewport));
    let first = if total == 0 { 0 } else { scroll + 1 };
    let last = (scroll + viewport).min(total);
    let suffix = if last < total {
        " ↓ more"
    } else if scroll > 0 {
        " ↑ more"
    } else {
        ""
    };
    let title = format!(" help {first}–{last}/{total}{suffix} ");
    let help = Paragraph::new(help_text(keys))
        .scroll((scroll as u16, 0))
        .block(Block::bordered().title(title));
    frame.render_widget(Clear, popup);
    frame.render_widget(help, popup);
}

fn help_text(keys: &Keybindings) -> String {
    let lines = [
        "Sidebar".to_string(),
        "  Ctrl-\\           sidebar / target pane".into(),
        "  | / -            split pane (sidebar)".into(),
        "  w                close comparison pane (or sole other pane)".into(),
        "  Ctrl-h           hide/show sidebar".into(),
        format!("  {}/{} · ↑/↓     move selection", keys.down, keys.up),
        format!(
            "  {:<16} next pane (terminal focused)",
            keys.pane_next.display()
        ),
        format!(
            "  {:<16} previous pane (terminal focused)",
            keys.pane_prev.display()
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
        "  z / click        enlarge mermaid diagram".into(),
        "  lightbox Kitty: wheel/+/- zoom · drag/hjkl pan · 0 reset".into(),
        "  lightbox: z/Esc  close".into(),
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
        "Help: j/k scroll · g/G top/bottom · Esc/q/? close".into(),
    ];
    lines.join("\n")
}

#[cfg(test)]
mod pane_chrome_tests {
    use super::{
        PaneChrome, help_line_count, help_popup_rect, help_viewport_height, pane_chrome,
        render_help_overlay,
    };
    use crate::config::Keybindings;
    use crate::{
        app::{
            focus::AppFocus,
            sessions::{SessionOrigin, SessionStatus},
            workspace::{PaneContent, WorkspaceStore},
        },
        terminal::{manager::PtyManager, shells::ShellManager},
    };
    use ratatui::layout::Rect;
    use ratatui::{Terminal, backend::TestBackend};
    use std::path::PathBuf;

    #[test]
    fn sidebar_focus_marks_target_as_remembered_not_focused() {
        assert_eq!(
            pane_chrome(true, AppFocus::Sidebar, true),
            PaneChrome::RememberedTarget
        );
    }

    #[test]
    fn live_terminal_focus_marks_target_as_focused() {
        assert_eq!(
            pane_chrome(true, AppFocus::Terminal, true),
            PaneChrome::Focused
        );
    }

    #[test]
    fn non_target_panes_remain_inactive() {
        assert_eq!(
            pane_chrome(false, AppFocus::Terminal, true),
            PaneChrome::Inactive
        );
    }

    #[test]
    fn unfocused_application_does_not_show_focused_pane_chrome() {
        assert_eq!(
            pane_chrome(true, AppFocus::Terminal, false),
            PaneChrome::RememberedTarget
        );
    }

    #[test]
    fn help_describes_workspace_and_pane_shortcuts_separately() {
        let help = super::help_text(&crate::config::Keybindings::default());
        assert!(help.contains("Ctrl-\\           sidebar / target pane"));
        assert!(help.contains("next pane (terminal focused)"));
        assert!(help.contains("previous pane (terminal focused)"));
        assert!(!help.contains("cycle session"));
        assert!(!help.contains("Ctrl-4"));
    }

    fn manager_with_one_session() -> (PtyManager, u64) {
        let mut manager = PtyManager::default();
        let id = manager.register_placeholder(
            PathBuf::from("/tmp/workspace-a"),
            "workspace-a".into(),
            SessionStatus::Idle,
            Some("session-a".into()),
            SessionOrigin::Managed,
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            vec![],
        );
        (manager, id)
    }

    #[test]
    fn single_pane_draws_no_inner_frame() {
        let (manager, session_id) = manager_with_one_session();
        let workspaces = {
            let mut store = WorkspaceStore::default();
            store.ensure_workspace(session_id);
            store
        };
        let workspace = workspaces.workspace(session_id).unwrap();
        let shells = ShellManager::default();
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

        terminal
            .draw(|frame| {
                super::render_workspace_terminals(
                    frame,
                    frame.area(),
                    &manager,
                    &shells,
                    workspace,
                    None,
                    None,
                    AppFocus::Terminal,
                    true,
                );
            })
            .unwrap();

        assert_ne!(terminal.backend().buffer()[(0, 0)].symbol(), "┌");
        assert!(
            terminal.backend().buffer()[(0, 0)]
                .symbol()
                .starts_with('P')
        );
    }

    #[test]
    fn split_workspace_draws_borders_and_a_title_for_each_pane() {
        let (manager, session_id) = manager_with_one_session();
        let mut store = WorkspaceStore::default();
        let workspace_id = store.ensure_workspace(session_id);
        let target = store.workspace(workspace_id).unwrap().primary_pane();
        store
            .split_with_content(workspace_id, target, PaneContent::Shell(9), false)
            .unwrap();
        let workspace = store.workspace(workspace_id).unwrap();
        let shells = ShellManager::default();
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

        terminal
            .draw(|frame| {
                super::render_workspace_terminals(
                    frame,
                    frame.area(),
                    &manager,
                    &shells,
                    workspace,
                    None,
                    None,
                    AppFocus::Terminal,
                    true,
                );
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(0, 0)].symbol(), "┌");
        assert_eq!(buffer[(40, 0)].symbol(), "┌");
        let top_row = (0..80).map(|x| buffer[(x, 0)].symbol()).collect::<String>();
        assert!(top_row.contains("workspace-a"));
        assert!(top_row.contains("shell:9"));
    }

    #[test]
    fn help_scrolled_to_bottom_shows_diff_and_pickers_sections() {
        let keys = Keybindings::default();
        let area = Rect::new(0, 0, 80, 24);
        let offset = help_line_count(&keys).saturating_sub(help_viewport_height(area));
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

        terminal
            .draw(|frame| render_help_overlay(frame, &keys, offset))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let text = (0..24)
            .map(|y| (0..80).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("Diff view"));
        assert!(text.contains("Pickers"));
    }

    #[test]
    fn help_title_shows_scroll_position_and_total() {
        let keys = Keybindings::default();
        let area = Rect::new(0, 0, 80, 24);
        let offset = help_line_count(&keys).saturating_sub(help_viewport_height(area));
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();

        terminal
            .draw(|frame| render_help_overlay(frame, &keys, offset))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let popup = help_popup_rect(area);
        let title = (popup.x..popup.right())
            .map(|x| buffer[(x, popup.y)].symbol())
            .collect::<String>();
        assert!(title.contains("/"));
        assert!(title.contains(&help_line_count(&keys).to_string()));
    }

    #[test]
    fn help_at_top_shows_sidebar_section() {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| render_help_overlay(frame, &Keybindings::default(), 0))
            .unwrap();

        let buffer = terminal.backend().buffer();
        let text = (0..24)
            .map(|y| (0..80).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("Sidebar"));
    }
}
