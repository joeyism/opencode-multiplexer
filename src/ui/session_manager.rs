use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
};

use crate::{
    app::session_manager::{PendingDelete, SessionManagerState},
    ui::sidebar::relative_time_from_updated,
};

pub fn render_session_manager(frame: &mut Frame, picker: &mut SessionManagerState, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " manage sessions ",
            Style::default().fg(Color::Cyan),
        ));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Vertical layout: search input (1 line) + table + footer (1 line)
    let [search_area, table_area, footer_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    // Render search input
    let search_line = Line::from(vec![
        Span::styled(" Search: ", Style::default().fg(Color::DarkGray)),
        Span::raw(&picker.query),
        Span::styled("█", Style::default().fg(Color::Cyan)),
    ]);
    frame.render_widget(Paragraph::new(search_line), search_area);

    // Compute page size and ensure selection is visible
    let page_size = table_area.height.saturating_sub(1) as usize; // subtract 1 for header
    picker.ensure_visible(page_size);

    // Get visible entries with match indices
    let visible = picker.visible_entries(page_size);

    let matched_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let selected_style = Style::default().bg(Color::DarkGray);
    let selected_matched_style = Style::default()
        .fg(Color::Yellow)
        .bg(Color::DarkGray)
        .add_modifier(Modifier::BOLD);

    let header = Row::new(vec![
        Cell::from(Span::styled(
            " ",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Cyan),
        )),
        Cell::from(Span::styled(
            "Repo",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Cyan),
        )),
        Cell::from(Span::styled(
            "Title",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Cyan),
        )),
        Cell::from(Span::styled(
            "Directory",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Cyan),
        )),
        Cell::from(Span::styled(
            "Msgs",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Cyan),
        )),
        Cell::from(Span::styled(
            "Time",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Cyan),
        )),
    ]);

    let rows: Vec<Row> = visible
        .iter()
        .enumerate()
        .map(
            |(i, (entry, repo_idx, title_idx, dir_idx, is_live, is_selected))| {
                let row_idx = i + picker.scroll_offset;
                let current = row_idx == picker.selected;

                let (normal_style, highlight_style) = if current {
                    (
                        Style::default().fg(Color::White).bg(Color::DarkGray),
                        selected_matched_style,
                    )
                } else {
                    (Style::default().fg(Color::White), matched_style)
                };

                let checkbox = if *is_selected { "•" } else { " " };
                let live_dot = if *is_live {
                    Span::styled(" ●", Style::default().fg(Color::Green))
                } else {
                    Span::raw("  ")
                };

                let checkbox_cell = Cell::from(Line::from(vec![
                    Span::styled(
                        checkbox,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    live_dot,
                ]));

                let repo_cell = Cell::from(highlight_text(
                    &entry.repo,
                    repo_idx,
                    normal_style,
                    highlight_style,
                ));
                let title_cell = Cell::from(highlight_text(
                    &entry.title,
                    title_idx,
                    normal_style,
                    highlight_style,
                ));
                let dir_cell = Cell::from(highlight_text(
                    &entry.directory,
                    dir_idx,
                    normal_style,
                    highlight_style,
                ));

                let count_style = if current {
                    Style::default().fg(Color::White).bg(Color::DarkGray)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                let count_cell = Cell::from(Span::styled(
                    entry.user_message_count.to_string(),
                    count_style,
                ));

                let time = relative_time_from_updated(Some(entry.time_updated));
                let time_style = if current {
                    Style::default().fg(Color::White).bg(Color::DarkGray)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                let time_cell = Cell::from(Span::styled(time, time_style));

                let row = Row::new(vec![
                    checkbox_cell,
                    repo_cell,
                    title_cell,
                    dir_cell,
                    count_cell,
                    time_cell,
                ]);
                if current {
                    row.style(selected_style)
                } else {
                    row
                }
            },
        )
        .collect();

    let widths = [
        Constraint::Length(4),  // checkbox + live dot
        Constraint::Length(12), // repo
        Constraint::Length(24), // title
        Constraint::Min(20),    // directory
        Constraint::Length(6),  // messages
        Constraint::Length(6),  // time
    ];

    let table = Table::new(rows, widths).header(header);
    frame.render_widget(table, table_area);

    let footer = if let Some(pending) = &picker.pending_delete {
        Line::from(vec![Span::styled(
            pending_delete_message(pending),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )])
    } else {
        Line::from(vec![
            Span::styled(" Tab", Style::default().fg(Color::Cyan)),
            Span::raw(" select  "),
            Span::styled("C-a/C-u", Style::default().fg(Color::Cyan)),
            Span::raw(" all/none  "),
            Span::styled("C-d/Del", Style::default().fg(Color::Cyan)),
            Span::raw(" delete  "),
            Span::styled("Esc", Style::default().fg(Color::Cyan)),
            Span::raw(" close"),
        ])
    };
    frame.render_widget(Paragraph::new(footer), footer_area);
}

pub fn pending_delete_message(pending: &PendingDelete) -> String {
    format!(
        " Delete {} session(s) ({} user msgs)? y confirm / n cancel ",
        pending.session_ids.len(),
        pending.user_message_count
    )
}

fn highlight_text(
    text: &str,
    indices: &[u32],
    normal_style: Style,
    highlight_style: Style,
) -> Line<'static> {
    if indices.is_empty() {
        return Line::from(Span::styled(text.to_string(), normal_style));
    }

    let mut spans = Vec::new();
    let mut current = String::new();
    let mut in_highlight = false;

    for (i, ch) in text.chars().enumerate() {
        let is_match = indices.contains(&(i as u32));
        if is_match != in_highlight {
            if !current.is_empty() {
                let style = if in_highlight {
                    highlight_style
                } else {
                    normal_style
                };
                spans.push(Span::styled(std::mem::take(&mut current), style));
            }
            in_highlight = is_match;
        }
        current.push(ch);
    }
    if !current.is_empty() {
        let style = if in_highlight {
            highlight_style
        } else {
            normal_style
        };
        spans.push(Span::styled(current, style));
    }

    Line::from(spans)
}
