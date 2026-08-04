use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::app::{
    focus::AppFocus,
    sessions::{SessionOrigin, SessionStatus},
};

#[derive(Debug, Clone)]
pub struct SidebarEntry {
    pub top_level_id: u64,
    pub session_id: Option<String>,
    pub cwd: PathBuf,
    pub title: String,
    pub status: SessionStatus,
    pub time_updated: Option<i64>,
    pub active: bool,
    pub origin: SessionOrigin,
    pub has_children: bool,
    pub children: Vec<ChildSidebarEntry>,
}

#[derive(Debug, Clone)]
pub struct ChildSidebarEntry {
    pub session_id: String,
    pub cwd: PathBuf,
    pub title: String,
    pub status: SessionStatus,
    pub time_updated: Option<i64>,
    pub has_children: bool,
    pub children: Vec<ChildSidebarEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidebarRowKind {
    TopLevel {
        top_level_id: u64,
        session_id: Option<String>,
    },
    Child {
        session_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarVisibleRow {
    pub kind: SidebarRowKind,
    pub cwd: PathBuf,
    pub title: String,
    pub status: SessionStatus,
    pub depth: usize,
    pub has_children: bool,
    pub expanded: bool,
    pub active: bool,
    pub origin: SessionOrigin,
    pub session_id: Option<String>,
    pub time_updated: Option<i64>,
}

pub fn flatten_sidebar_entries(
    entries: &[SidebarEntry],
    expanded: &HashSet<String>,
) -> Vec<SidebarVisibleRow> {
    let mut rows = Vec::new();
    for entry in entries {
        let is_expanded = entry
            .session_id
            .as_ref()
            .is_some_and(|id| expanded.contains(id));
        rows.push(SidebarVisibleRow {
            kind: SidebarRowKind::TopLevel {
                top_level_id: entry.top_level_id,
                session_id: entry.session_id.clone(),
            },
            cwd: entry.cwd.clone(),
            title: entry.title.clone(),
            status: entry.status,
            depth: 0,
            has_children: entry.has_children,
            expanded: is_expanded,
            active: entry.active,
            origin: entry.origin,
            session_id: entry.session_id.clone(),
            time_updated: entry.time_updated,
        });
        if is_expanded {
            flatten_children(&mut rows, &entry.children, expanded, 1);
        }
    }
    rows
}

fn flatten_children(
    rows: &mut Vec<SidebarVisibleRow>,
    children: &[ChildSidebarEntry],
    expanded: &HashSet<String>,
    depth: usize,
) {
    for child in children {
        let is_expanded = expanded.contains(&child.session_id);
        rows.push(SidebarVisibleRow {
            kind: SidebarRowKind::Child {
                session_id: child.session_id.clone(),
            },
            cwd: child.cwd.clone(),
            title: child.title.clone(),
            status: child.status,
            depth,
            has_children: child.has_children,
            expanded: is_expanded,
            active: false,
            origin: SessionOrigin::Managed,
            session_id: Some(child.session_id.clone()),
            time_updated: child.time_updated,
        });
        if is_expanded {
            flatten_children(rows, &child.children, expanded, depth + 1);
        }
    }
}

pub fn render_sidebar(
    rows: &[SidebarVisibleRow],
    selected: usize,
    focus: AppFocus,
    sidebar_width: u16,
    app_focused: bool,
) -> Paragraph<'static> {
    let (title_style, border_style) = if !app_focused {
        (
            Style::default().fg(Color::DarkGray),
            Style::default().fg(Color::DarkGray),
        )
    } else if matches!(focus, AppFocus::Sidebar) {
        (
            Style::default().fg(Color::Cyan),
            Style::default().fg(Color::Cyan),
        )
    } else {
        (
            Style::default().fg(Color::DarkGray),
            Style::default().fg(Color::DarkGray),
        )
    };

    let lines = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            render_row(
                row,
                index == selected,
                sidebar_width.saturating_sub(1),
            )
        })
        .collect::<Vec<_>>();

    Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::RIGHT | Borders::TOP)
            .border_style(border_style)
            .title(Span::styled("sessions", title_style)),
    )
}

pub fn sidebar_row_style(is_selected: bool, is_active: bool) -> Style {
    let _ = is_active;
    if is_selected {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    }
}

pub fn sidebar_row_modifier() -> Modifier {
    Modifier::empty()
}

pub fn display_session_label(cwd: &Path, title: &str) -> String {
    let repo = repo_root_name(cwd);
    let repo_prefix: String = repo.chars().take(3).collect();
    format!("{repo_prefix}/{title} ")
}

pub fn relative_time_label(age_secs: u64) -> String {
    if age_secs < 60 {
        String::from("1m")
    } else if age_secs < 3600 {
        format!("{}m", age_secs / 60)
    } else if age_secs < 86_400 {
        format!("{}h", age_secs / 3600)
    } else {
        format!("{}d", age_secs / 86_400)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn format_sidebar_text(
    cwd: &Path,
    title: &str,
    time: &str,
    sidebar_width: u16,
    depth: usize,
    has_children: bool,
    expanded: bool,
    active: bool,
    is_child: bool,
) -> String {
    let (left, right) = format_sidebar_parts(
        cwd,
        title,
        time,
        sidebar_width,
        depth,
        has_children,
        expanded,
        active,
        is_child,
    );
    format!("{left}{right}")
}

#[allow(clippy::too_many_arguments)]
fn format_sidebar_parts(
    cwd: &Path,
    title: &str,
    time: &str,
    sidebar_width: u16,
    depth: usize,
    has_children: bool,
    expanded: bool,
    active: bool,
    is_child: bool,
) -> (String, String) {
    let label = if is_child {
        title.to_string()
    } else {
        display_session_label(cwd, title)
    };
    let time_text = time.to_string();
    
    let marker = if is_child {
        ""
    } else if has_children {
        if expanded { "▽ " } else { "▸ " }
    } else {
        "  "
    };
    let indent = if is_child {
        " ".repeat(depth + 1)
    } else {
        String::new()
    };
    let active_prefix = if is_child {
        "└─ "
    } else if active {
        "▶ "
    } else {
        "  "
    };
    let prefix = format!("{indent}{active_prefix}{marker}");
    let content_width = sidebar_width.saturating_sub(3); // 2 dot span + 1 border
    let left_width = content_width.saturating_sub(time_text.chars().count() as u16) as usize;
    let left = format!(
        "{}{}",
        prefix,
        truncate_label(&label, left_width.saturating_sub(prefix.chars().count()))
    );
    let padded_left = format!("{left:<left_width$}");
    (padded_left, time_text)
}

pub fn repo_root_name(cwd: &Path) -> String {
    for ancestor in cwd.ancestors() {
        let dot_git = ancestor.join(".git");
        if dot_git.is_dir() {
            return ancestor
                .file_name()
                .and_then(|n| n.to_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("?")
                .to_string();
        }
        if dot_git.is_file() {
            if let Ok(contents) = std::fs::read_to_string(&dot_git)
                && let Some(target) = contents.strip_prefix("gitdir: ")
            {
                let gitdir = Path::new(target.trim());
                if let Some(repo_root) = gitdir
                    .parent()
                    .and_then(|p| p.parent())
                    .and_then(|p| p.parent())
                {
                    return repo_root
                        .file_name()
                        .and_then(|n| n.to_str())
                        .filter(|s| !s.is_empty())
                        .unwrap_or("?")
                        .to_string();
                }
            }
            return ancestor
                .file_name()
                .and_then(|n| n.to_str())
                .filter(|s| !s.is_empty())
                .unwrap_or("?")
                .to_string();
        }
    }
    cwd.file_name()
        .and_then(|n| n.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("?")
        .to_string()
}

fn truncate_label(label: &str, max_len: usize) -> String {
    if max_len == 0 {
        return String::new();
    }
    let chars = label.chars().collect::<Vec<_>>();
    if chars.len() <= max_len {
        return format!("{label:<max_len$}");
    }
    if max_len <= 1 {
        return String::from("…");
    }
    let mut out = chars[..max_len - 1].iter().collect::<String>();
    out.push('…');
    out
}

pub fn relative_time_from_updated(time_updated: Option<i64>) -> String {
    let Some(updated) = time_updated else {
        return String::from("--");
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let updated = if updated > 10_000_000_000 {
        updated / 1000
    } else {
        updated
    };
    let age = now.saturating_sub(updated).max(0) as u64;
    relative_time_label(age)
}

fn render_row(
    row: &SidebarVisibleRow,
    is_selected: bool,
    sidebar_width: u16,
) -> Line<'static> {
    let (symbol, color) = match row.status {
        SessionStatus::Working => ("●", Color::Green),
        SessionStatus::SubagentsWorking => ("●", Color::Cyan),
        SessionStatus::NeedsInput => ("◐", Color::Yellow),
        SessionStatus::Idle => ("○", Color::DarkGray),
        SessionStatus::Error => ("✗", Color::Red),
    };
    let row_style = sidebar_row_style(is_selected, row.active);
    let time = relative_time_from_updated(row.time_updated);
    let (left, right) = format_sidebar_parts(
        &row.cwd,
        &row.title,
        &time,
        sidebar_width,
        row.depth,
        row.has_children,
        row.expanded,
        row.active,
        matches!(row.kind, SidebarRowKind::Child { .. }),
    );
    Line::from(vec![
        Span::styled(format!("{symbol} "), Style::default().fg(color)),
        Span::styled(left, row_style),
        Span::styled(format!(" {right}"), row_style),
    ])
}
