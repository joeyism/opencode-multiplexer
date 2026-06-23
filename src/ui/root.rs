use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::{
    app::{
        conversation::ConversationViewState, diff::DiffViewState, focus::AppFocus,
        message_picker::MessagePickerState, session_picker::SessionPickerState,
    },
    config::Keybindings,
    terminal::{manager::PtyManager, renderer::TerminalWidget, selection::SelectionRange},
    ui::{
        diff::apply_cursor_and_selection,
        diff::highlight_search_matches,
        hints::footer_line,
        layout::{centered_rect, split_root, terminal_inner_rect},
        message_picker::render_message_picker,
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
    sidebar_collapsed: bool,
    panel_hidden: bool,
    app_focused: bool,
    conversation: &ConversationViewState,
    diff: &DiffViewState,
    session_picker: Option<&mut SessionPickerState>,
    message_picker: Option<&mut MessagePickerState>,
    confirm_quit: bool,
    terminal_selection: Option<SelectionRange>,
) {
    let layout = split_root(frame.area(), sidebar_width, 1);
    if !panel_hidden {
        frame.render_widget(
            render_sidebar(
                rows,
                selected,
                focus,
                sidebar_collapsed,
                sidebar_width,
                app_focused,
            ),
            layout.sidebar,
        );
    }
    frame.render_widget(
        footer_line(focus, footer_message, keys, diff.is_visual()),
        layout.footer,
    );

    let main_border_style = if !app_focused {
        Style::default().fg(Color::DarkGray)
    } else if matches!(
        focus,
        AppFocus::Terminal | AppFocus::Conversation | AppFocus::Diff
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
            let visible = conversation.visible_lines(content_vp);

            let visible = if !conversation.search_query().is_empty() {
                highlight_search_matches(
                    &visible,
                    conversation.scroll_offset(),
                    conversation.matches(),
                    conversation.current_match_index(),
                )
            } else {
                visible
            };

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

    if show_help {
        let help = Paragraph::new(format!(
            "{} focus\n{} / {} move\nEnter attach/open\n{} view\n{} files\n{} diff\n{} spawn\n{} worktree\n{} kill\n{} help\n{} quit",
            "Ctrl-\\",
            keys.down,
            keys.up,
            keys.view,
            keys.files,
            keys.diff,
            keys.spawn,
            keys.worktree,
            keys.kill,
            keys.help,
            keys.quit,
        ))
        .block(Block::bordered().title("help"));
        let popup = centered_rect(frame.area(), 50, 40);
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

    if let Some(picker) = session_picker {
        let popup = centered_rect(frame.area(), 80, 70);
        frame.render_widget(Clear, popup);
        render_session_picker(frame, picker, popup);
    } else if let Some(picker) = message_picker {
        let popup = centered_rect(frame.area(), 80, 70);
        frame.render_widget(Clear, popup);
        render_message_picker(frame, picker, popup);
    }

    if confirm_quit {
        let quit_msg = Paragraph::new("Quit ocmux?\ny confirm\nn cancel")
            .block(Block::bordered().title("quit"));
        let popup = centered_rect(frame.area(), 30, 20);
        frame.render_widget(Clear, popup);
        frame.render_widget(quit_msg, popup);
    }
}
