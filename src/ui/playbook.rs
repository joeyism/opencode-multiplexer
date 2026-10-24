use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::app::playbook::{ParamFormState, PlaybookPickerState};

pub fn render_playbook_picker(frame: &mut Frame, picker: &PlaybookPickerState, area: Rect) {
    let visible = picker.visible();
    let mut lines: Vec<Line> = vec![Line::from(Span::styled(
        format!(" {} ", picker.query),
        Style::default().fg(Color::Yellow),
    ))];
    lines.push(Line::from(""));
    for (i, (_, pb)) in visible.iter().enumerate() {
        let marker = if i == picker.selected { ">" } else { " " };
        let desc = pb.description.as_deref().unwrap_or("");
        let line = format!("{} {}  {}", marker, pb.name, desc);
        lines.push(Line::from(line));
    }
    let block = Block::default().borders(Borders::ALL).title("playbooks");
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

pub fn render_param_form(frame: &mut Frame, form: &ParamFormState, area: Rect) {
    let mut lines: Vec<Line> = vec![Line::from(Span::styled(
        form.playbook_name().to_string(),
        Style::default().fg(Color::Cyan),
    ))];
    if let Some(desc) = form.description() {
        lines.push(Line::from(Span::raw(desc.to_string())));
    }
    lines.push(Line::from(""));
    for (i, field) in form.fields.iter().enumerate() {
        let marker = if i == form.focused { ">" } else { " " };
        let req = if field.required { " *" } else { "" };
        lines.push(Line::from(Span::styled(
            format!("{} {}{}", marker, field.prompt, req),
            Style::default().fg(Color::White),
        )));
        lines.push(Line::from(Span::raw(format!("  {}", field.value))));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Tab next  Enter run  Esc back",
        Style::default().fg(Color::DarkGray),
    )));
    let block = Block::default().borders(Borders::ALL).title("params");
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

pub fn render_abort_panel(frame: &mut Frame, area: Rect) {
    let text = Paragraph::new("Playbook running.\n\nEnter abort\nEsc back")
        .block(Block::default().borders(Borders::ALL).title("playbook"));
    frame.render_widget(Clear, area);
    frame.render_widget(text, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playbook::schema::parse_playbook;
    use ratatui::{Terminal, backend::TestBackend};

    fn render_to_string(width: u16, height: u16, draw: impl FnOnce(&mut Frame, Rect)) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                draw(frame, area);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let mut out = String::new();
        for y in 0..height {
            for x in 0..width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn picker_renders_playbook_name() {
        let pb = parse_playbook(
            "name: pr-comments\ndescription: Review PR comments\nsteps:\n  - send:\n      agent: chat\n      message: hi\n",
        )
        .unwrap();
        let picker = PlaybookPickerState::from_playbooks(vec![pb]);
        let out = render_to_string(40, 10, |f, a| render_playbook_picker(f, &picker, a));
        assert!(out.contains("pr-comments"));
    }

    #[test]
    fn param_form_renders_title_prompt_and_value() {
        let pb = parse_playbook(
            r#"
name: pr-comments
description: Review PR comments
params:
  GITHUB_PR_URL:
    prompt: GitHub PR URL
    required: true
steps:
  - send:
      agent: chat
      message: ${GITHUB_PR_URL}
"#,
        )
        .unwrap();
        let mut form = ParamFormState::from_playbook(&pb);
        form.insert_char('h');
        form.insert_char('t');
        form.insert_char('t');
        form.insert_char('p');
        let out = render_to_string(50, 12, |f, a| render_param_form(f, &form, a));
        assert!(out.contains("pr-comments"));
        assert!(out.contains("GitHub PR URL"));
        assert!(out.contains("http"));
    }
}
