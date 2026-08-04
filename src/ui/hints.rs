use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

use crate::{app::focus::AppFocus, config::Keybindings};

pub fn footer_line(
    focus: AppFocus,
    message: Option<&str>,
    keys: &Keybindings,
    diff_visual_mode: bool,
) -> Line<'static> {
    if let Some(message) = message {
        return Line::from(vec![Span::styled(
            message.to_string(),
            Style::default().fg(Color::Yellow),
        )]);
    }

    match focus {
        AppFocus::Conversation => conversation_hints(keys),
        AppFocus::Diff => diff_hints(keys, diff_visual_mode),
        _ => main_hints(focus, keys),
    }
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
        Span::styled(keys.view.to_string(), Style::default().fg(Color::Cyan)),
        Span::raw(" back  "),
        Span::styled("\u{21e7}+click", Style::default().fg(Color::Cyan)),
        Span::raw(" select  "),
        Span::styled(keys.help.to_string(), Style::default().fg(Color::Cyan)),
        Span::raw(" help  "),
        Span::styled(
            "[conversation]".to_string(),
            Style::default().fg(Color::Yellow),
        ),
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
