use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

use crate::{app::focus::AppFocus, config::Keybindings};

/// Combine a left-aligned hints line with a right-aligned status readout,
/// truncating hints (with an ellipsis) when the combined width exceeds `width`.
/// If status alone is wider than `width`, the status is truncated instead.
pub fn compose_footer(
    hints: Line<'static>,
    status: Option<Line<'static>>,
    width: u16,
) -> Line<'static> {
    let width = width as usize;
    let hints_w = hints.width();
    match status {
        None => hints,
        Some(status) => {
            let status_w = status.width();
            if hints_w + status_w <= width {
                let mut spans = hints.spans;
                spans.push(Span::raw(" ".repeat(width - hints_w - status_w)));
                spans.extend(status.spans);
                Line::from(spans)
            } else {
                // Hints don't fit with status; truncate hints with an ellipsis.
                let room = width.saturating_sub(status_w);
                let t: String = hints.spans.iter().map(|s| s.content.as_ref()).collect();
                if room == 0 {
                    // No room for hints at all — status truncated to fit.
                    let mut st: String = status.spans.iter().map(|s| s.content.as_ref()).collect();
                    st = st.chars().take(width.saturating_sub(1)).collect();
                    Line::from(vec![Span::raw(format!("{st}…"))])
                } else {
                    let ellipsis = 1usize;
                    let keep = room.saturating_sub(ellipsis);
                    let truncated: String = t.chars().take(keep).collect();
                    let mut spans = vec![Span::raw(format!("{truncated}…"))];
                    spans.push(Span::raw(" ".repeat(room - keep - ellipsis)));
                    spans.extend(status.spans);
                    Line::from(spans)
                }
            }
        }
    }
}

pub fn footer_line(
    focus: AppFocus,
    message: Option<&str>,
    keys: &Keybindings,
    diff_visual_mode: bool,
    conversation_lightbox: bool,
) -> Line<'static> {
    if let Some(message) = message {
        return Line::from(vec![Span::styled(
            message.to_string(),
            Style::default().fg(Color::Yellow),
        )]);
    }

    match focus {
        AppFocus::Conversation if conversation_lightbox => conversation_lightbox_hints(),
        AppFocus::Conversation => conversation_hints(keys),
        AppFocus::Diff => diff_hints(keys, diff_visual_mode),
        AppFocus::Agents => agents_hints(),
        _ => main_hints(focus, keys),
    }
}

fn agents_hints() -> Line<'static> {
    Line::from(vec![
        Span::styled("j/k", Style::default().fg(Color::Cyan)),
        Span::raw(" move  "),
        Span::styled("Enter", Style::default().fg(Color::Cyan)),
        Span::raw(" attach  "),
        Span::styled("v", Style::default().fg(Color::Cyan)),
        Span::raw(" convo  "),
        Span::styled("d", Style::default().fg(Color::Cyan)),
        Span::raw(" diff  "),
        Span::styled("a/q/Esc", Style::default().fg(Color::Cyan)),
        Span::raw(" back  "),
        Span::styled("[agents]", Style::default().fg(Color::Yellow)),
    ])
}

fn conversation_hints(keys: &Keybindings) -> Line<'static> {
    Line::from(vec![
        Span::styled("j/k", Style::default().fg(Color::Cyan)),
        Span::raw(" scroll  "),
        Span::styled("G", Style::default().fg(Color::Cyan)),
        Span::raw(" end  "),
        Span::styled("g", Style::default().fg(Color::Cyan)),
        Span::raw(" top  "),
        Span::styled("/", Style::default().fg(Color::Cyan)),
        Span::raw(" search  "),
        Span::styled("n/N", Style::default().fg(Color::Cyan)),
        Span::raw(" next/prev  "),
        Span::styled("drag", Style::default().fg(Color::Cyan)),
        Span::raw(" copy  "),
        Span::styled("J/K", Style::default().fg(Color::Cyan)),
        Span::raw(" jump  "),
        Span::styled("z", Style::default().fg(Color::Cyan)),
        Span::raw(" zoom  "),
        Span::styled(keys.view.to_string(), Style::default().fg(Color::Cyan)),
        Span::raw(" back  "),
        Span::styled(keys.help.to_string(), Style::default().fg(Color::Cyan)),
        Span::raw(" help  "),
        Span::styled(
            "[conversation]".to_string(),
            Style::default().fg(Color::Yellow),
        ),
    ])
}

fn conversation_lightbox_hints() -> Line<'static> {
    Line::from(vec![
        Span::styled("z/Esc", Style::default().fg(Color::Cyan)),
        Span::raw(" close  "),
        Span::styled("[mermaid]".to_string(), Style::default().fg(Color::Yellow)),
    ])
}

fn diff_hints(keys: &Keybindings, visual_mode: bool) -> Line<'static> {
    if visual_mode {
        Line::from(vec![
            Span::styled("j/k", Style::default().fg(Color::Cyan)),
            Span::raw(" move  "),
            Span::styled("v", Style::default().fg(Color::Cyan)),
            Span::raw(" cancel  "),
            Span::styled("Enter", Style::default().fg(Color::Cyan)),
            Span::raw(" confirm  "),
            Span::styled("Esc", Style::default().fg(Color::Cyan)),
            Span::raw(" cancel  "),
            Span::styled(
                "[diff:visual]".to_string(),
                Style::default().fg(Color::Yellow),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled("j/k", Style::default().fg(Color::Cyan)),
            Span::raw(" move  "),
            Span::styled("G", Style::default().fg(Color::Cyan)),
            Span::raw(" end  "),
            Span::styled("g", Style::default().fg(Color::Cyan)),
            Span::raw(" top  "),
            Span::styled("/", Style::default().fg(Color::Cyan)),
            Span::raw(" search  "),
            Span::styled("n/N", Style::default().fg(Color::Cyan)),
            Span::raw(" next/prev  "),
            Span::styled("v", Style::default().fg(Color::Cyan)),
            Span::raw(" select  "),
            Span::styled(keys.diff.to_string(), Style::default().fg(Color::Cyan)),
            Span::raw(" back  "),
            Span::styled(keys.help.to_string(), Style::default().fg(Color::Cyan)),
            Span::raw(" help  "),
            Span::styled("[diff]".to_string(), Style::default().fg(Color::Yellow)),
        ])
    }
}

fn main_hints(focus: AppFocus, keys: &Keybindings) -> Line<'static> {
    let focus_label = match focus {
        AppFocus::Sidebar => "sidebar",
        AppFocus::Terminal => "terminal",
        AppFocus::Conversation => "conversation",
        AppFocus::Diff => "diff",
        AppFocus::Agents => "agents",
    };

    Line::from(vec![
        Span::styled("Ctrl-\\", Style::default().fg(Color::Cyan)),
        Span::raw(" focus  "),
        Span::styled(keys.quit.to_string(), Style::default().fg(Color::Cyan)),
        Span::raw(" quit  "),
        Span::styled(keys.spawn.to_string(), Style::default().fg(Color::Cyan)),
        Span::raw(" new  "),
        Span::styled(keys.worktree.to_string(), Style::default().fg(Color::Cyan)),
        Span::raw(" worktree  "),
        Span::styled(keys.view.to_string(), Style::default().fg(Color::Cyan)),
        Span::raw(" view  "),
        Span::styled(keys.files.to_string(), Style::default().fg(Color::Cyan)),
        Span::raw(" files  "),
        Span::styled(keys.diff.to_string(), Style::default().fg(Color::Cyan)),
        Span::raw(" diff  "),
        Span::styled("r", Style::default().fg(Color::Cyan)),
        Span::raw(" refresh  "),
        Span::styled("!", Style::default().fg(Color::Cyan)),
        Span::raw(" bash  "),
        Span::styled("/", Style::default().fg(Color::Cyan)),
        Span::raw(" attach  "),
        Span::styled(keys.sessions.to_string(), Style::default().fg(Color::Cyan)),
        Span::raw(" manage  "),
        Span::styled(keys.playbook.to_string(), Style::default().fg(Color::Cyan)),
        Span::raw(" playbook  "),
        Span::styled(keys.history.to_string(), Style::default().fg(Color::Cyan)),
        Span::raw(" history  "),
        Span::styled(keys.kill.to_string(), Style::default().fg(Color::Cyan)),
        Span::raw(" kill  "),
        Span::styled(
            format!("{}/{}", keys.down, keys.up),
            Style::default().fg(Color::Cyan),
        ),
        Span::raw(" move  "),
        Span::styled("c-h", Style::default().fg(Color::Cyan)),
        Span::raw(" hide  "),
        Span::styled(keys.help.to_string(), Style::default().fg(Color::Cyan)),
        Span::raw(" help  "),
        Span::styled(
            format!("[{focus_label}]"),
            Style::default().fg(Color::Yellow),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversation_lightbox_hints_show_close() {
        let line = footer_line(
            AppFocus::Conversation,
            None,
            &Keybindings::default(),
            false,
            true,
        );
        let text: String = line.spans.iter().map(|s| s.content.clone()).collect();
        assert!(text.contains("Esc"));
        assert!(text.contains("[mermaid]"));
    }

    #[test]
    fn conversation_hints_show_zoom_without_lightbox() {
        let line = footer_line(
            AppFocus::Conversation,
            None,
            &Keybindings::default(),
            false,
            false,
        );
        let text: String = line.spans.iter().map(|s| s.content.clone()).collect();
        assert!(text.contains("zoom"));
    }
}
