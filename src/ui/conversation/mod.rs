use std::sync::LazyLock;

pub mod diagram;
pub mod document;

use diagram::hash::hash_source;
use diagram::slot::{DiagramIndex, DiagramSlot};
use document::{ConversationDocument, DocBlock};

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use syntect::{
    easy::HighlightLines,
    highlighting::{Style as SynStyle, ThemeSet},
    parsing::SyntaxSet,
    util::LinesWithEndings,
};

use crate::data::db::models::{DbConversationMessage, DbConversationPart};

pub const GUTTER: &str = "│ ";
pub const TOOL_INDENT: &str = "  ";
pub const REASONING_PREFIX: &str = "· ";

pub fn user_color() -> Color {
    Color::Cyan
}
pub fn assistant_color() -> Color {
    Color::Green
}
pub fn meta_color() -> Color {
    Color::DarkGray
}
pub fn body_user_color() -> Color {
    Color::Cyan
}
pub fn body_assistant_color() -> Color {
    Color::Reset
}
pub fn reasoning_style() -> Style {
    Style::default()
        .fg(Color::DarkGray)
        .add_modifier(Modifier::ITALIC)
}

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static THEME_SET: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

const THEME_NAME: &str = "base16-ocean.dark";

pub fn build_conversation_document(
    messages: &[DbConversationMessage],
    width: u16,
    diagrams: &DiagramIndex,
) -> ConversationDocument {
    let mut doc = ConversationDocument::new();
    let content_width = width.saturating_sub(2);

    if messages.is_empty() {
        doc.push_lines(vec![Line::from(Span::styled(
            "No messages yet.",
            Style::default().fg(Color::DarkGray),
        ))]);
        return doc;
    }

    for msg in messages {
        let role_color = match msg.role.as_str() {
            "user" => user_color(),
            "assistant" => assistant_color(),
            _ => Color::Gray,
        };
        let body_color = match msg.role.as_str() {
            "user" => body_user_color(),
            "assistant" => body_assistant_color(),
            _ => Color::Reset,
        };

        let role_label = match msg.role.as_str() {
            "user" => "YOU".to_string(),
            "assistant" => {
                if let Some(agent) = &msg.agent {
                    if agent.is_empty() {
                        "assistant".to_string()
                    } else {
                        agent.clone()
                    }
                } else {
                    "assistant".to_string()
                }
            }
            _ => msg.role.clone(),
        };
        let time_str = format_timestamp(msg.time_created);

        let gutter_span = Span::styled(
            GUTTER,
            Style::default().fg(role_color).add_modifier(Modifier::BOLD),
        );

        let mut header_spans = vec![
            gutter_span.clone(),
            Span::styled(
                role_label,
                Style::default().fg(role_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("  {time_str}"), Style::default().fg(meta_color())),
        ];

        if let Some(model) = &msg.model_id {
            header_spans.push(Span::styled(
                format!("  {model}"),
                Style::default().fg(meta_color()),
            ));
        }

        doc.push_lines(vec![Line::from(header_spans)]);

        let mut text_buffer = String::new();

        let flush_text = |buf: &mut String, doc: &mut ConversationDocument| {
            if buf.is_empty() {
                return;
            }
            let blocks = render_markdown_blocks(buf, content_width, diagrams);
            for block in blocks {
                match block {
                    DocBlock::Text(mut lines) => {
                        for line in &mut lines {
                            apply_body_color(line, body_color);
                        }
                        let lines_with_gutter = lines
                            .into_iter()
                            .map(|l| prefix_gutter(l, gutter_span.clone()))
                            .collect();
                        doc.push_lines(lines_with_gutter);
                    }
                    DocBlock::Diagram(slot) => {
                        // For now, diagrams don't get the gutter in the block itself
                        // but maybe they should? The design says "gutters/headers still correct".
                        // I'll add gutter to diagram lines later in visible_lines or here.
                        // For simplicity, I'll just push the diagram block.
                        doc.push_diagram(slot);
                    }
                }
            }
            buf.clear();
        };

        for part in &msg.parts {
            match part.part_type.as_str() {
                "text" => {
                    if let Some(text) = &part.text
                        && !text.is_empty()
                    {
                        text_buffer.push_str(text);
                    }
                }
                "reasoning" => {
                    flush_text(&mut text_buffer, &mut doc);

                    if let Some(text) = &part.text
                        && !text.is_empty()
                    {
                        let reasoning_width =
                            content_width.saturating_sub(REASONING_PREFIX.chars().count() as u16);
                        let mut r_lines_all = Vec::new();
                        for line_text in text.lines() {
                            if line_text.trim().is_empty() {
                                continue;
                            }
                            let mut r_lines = vec![Line::from(line_text.to_string())];
                            wrap_lines(&mut r_lines, reasoning_width as usize);
                            for r_line in r_lines {
                                let mut spans = vec![
                                    gutter_span.clone(),
                                    Span::styled(REASONING_PREFIX, reasoning_style()),
                                ];
                                for s in r_line.spans {
                                    spans.push(Span::styled(s.content, reasoning_style()));
                                }
                                r_lines_all.push(Line::from(spans));
                            }
                        }
                        doc.push_lines(r_lines_all);
                    }
                }
                "tool" => {
                    flush_text(&mut text_buffer, &mut doc);
                    let tool_line = render_tool_line(part, content_width);
                    doc.push_lines(vec![prefix_gutter(tool_line, gutter_span.clone())]);
                }
                _ => {
                    flush_text(&mut text_buffer, &mut doc);
                    let other_line = Line::from(vec![
                        Span::raw(TOOL_INDENT),
                        Span::styled(
                            format!("[{}]", part.part_type),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]);
                    doc.push_lines(vec![prefix_gutter(other_line, gutter_span.clone())]);
                }
            }
        }

        flush_text(&mut text_buffer, &mut doc);

        doc.push_lines(vec![Line::from("")]);
    }

    doc
}

pub fn build_document(messages: &[DbConversationMessage], width: u16) -> Vec<Line<'static>> {
    let doc = build_conversation_document(messages, width, &DiagramIndex::default());
    doc.visible_lines(0, usize::MAX, false)
}

fn prefix_gutter(line: Line<'static>, gutter_span: Span<'static>) -> Line<'static> {
    let mut spans = vec![gutter_span];
    spans.extend(line.spans);
    Line::from(spans)
}

fn apply_body_color(line: &mut Line<'static>, color: Color) {
    if color == Color::Reset {
        return;
    }
    for span in &mut line.spans {
        if span.style.fg.is_none() || span.style.fg == Some(Color::Reset) {
            span.style.fg = Some(color);
        }
    }
}

fn format_timestamp(ts: i64) -> String {
    let secs = if ts > 1_000_000_000_000 {
        ts / 1000
    } else {
        ts
    };
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|dt| dt.format("%H:%M").to_string())
        .unwrap_or_else(|| secs.to_string())
}

fn render_tool_line(part: &DbConversationPart, width: u16) -> Line<'static> {
    let (icon, color) = match part.tool_status.as_deref() {
        Some("completed") => ("✓", Color::Green),
        Some("running") => ("⟳", Color::Yellow),
        Some("error") => ("✗", Color::Red),
        _ => ("⏳", Color::DarkGray),
    };

    let tool_name = part.tool.as_deref().unwrap_or("tool");

    let prefix_len = TOOL_INDENT.len() + icon.len() + 1 + tool_name.len() + 2;
    let max_detail_len = width.saturating_sub(prefix_len as u16) as usize;

    let detail = part
        .tool_title
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| truncate(s, max_detail_len))
        .or_else(|| {
            part.tool_input
                .as_deref()
                .map(|s| truncate(s, max_detail_len))
        });

    let mut spans = vec![
        Span::raw(TOOL_INDENT),
        Span::styled(format!("{icon} "), Style::default().fg(color)),
        Span::styled(tool_name.to_string(), Style::default().fg(Color::Gray)),
    ];

    if let Some(detail) = detail {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(detail, Style::default().fg(Color::Gray)));
    }

    Line::from(spans)
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}…", &s[..max_len.saturating_sub(1)])
    }
}

fn render_markdown_blocks(text: &str, width: u16, diagrams: &DiagramIndex) -> Vec<DocBlock> {
    let mut blocks: Vec<DocBlock> = Vec::new();
    let mut lines: Vec<Line<'static>> = Vec::new();
    let parser = Parser::new_ext(text, Options::ENABLE_TABLES);
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut style_stack: Vec<Style> = vec![Style::default()];
    let mut in_code_block = false;
    let mut code_block_lang: Option<String> = None;
    let mut code_block_text = String::new();
    let mut list_counter: usize = 0;

    let flush_lines = |lines: &mut Vec<Line<'static>>, blocks: &mut Vec<DocBlock>| {
        if !lines.is_empty() {
            blocks.push(DocBlock::Text(std::mem::take(lines)));
        }
    };

    for event in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                flush_spans(&mut current_spans, &mut lines);
                let modifier = if level as usize <= 2 {
                    Modifier::BOLD | Modifier::UNDERLINED
                } else {
                    Modifier::BOLD
                };
                style_stack.push(Style::default().add_modifier(modifier));
            }
            Event::End(TagEnd::Heading(_)) => {
                flush_spans(&mut current_spans, &mut lines);
                style_stack.pop();
            }
            Event::Start(Tag::Paragraph) => {
                flush_spans(&mut current_spans, &mut lines);
            }
            Event::End(TagEnd::Paragraph) => {
                flush_spans(&mut current_spans, &mut lines);
                lines.push(Line::from(""));
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                flush_spans(&mut current_spans, &mut lines);
                in_code_block = true;
                code_block_lang = match kind {
                    pulldown_cmark::CodeBlockKind::Fenced(lang) => {
                        if lang.is_empty() {
                            None
                        } else {
                            Some(lang.to_string())
                        }
                    }
                    _ => None,
                };
                code_block_text.clear();
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                if let Some(lang) = &code_block_lang
                    && lang.eq_ignore_ascii_case("mermaid")
                {
                    flush_lines(&mut lines, &mut blocks);
                    let h = hash_source(&code_block_text);
                    let slot = diagrams
                        .get(&h)
                        .map(|s| (*s).clone())
                        .unwrap_or_else(|| DiagramSlot::placeholder(&h, &code_block_text, 8));
                    blocks.push(DocBlock::Diagram(slot));
                } else {
                    let highlighted =
                        highlight_code_block(&code_block_text, code_block_lang.as_deref());
                    lines.extend(highlighted);
                    lines.push(Line::from(""));
                }
                code_block_lang = None;
            }
            Event::Start(Tag::List(start_num)) => {
                list_counter = start_num.unwrap_or(1).saturating_sub(1) as usize;
            }
            Event::End(TagEnd::List(_)) => {}
            Event::Start(Tag::Item) => {
                flush_spans(&mut current_spans, &mut lines);
                list_counter += 1;
                let prefix = format!("  {list_counter}. ");
                current_spans.push(Span::raw(prefix));
            }
            Event::End(TagEnd::Item) => {
                flush_spans(&mut current_spans, &mut lines);
            }
            Event::Start(Tag::Strong) => {
                style_stack.push(
                    style_stack
                        .last()
                        .copied()
                        .unwrap_or_default()
                        .add_modifier(Modifier::BOLD),
                );
            }
            Event::End(TagEnd::Strong) => {
                style_stack.pop();
            }
            Event::Start(Tag::Emphasis) => {
                style_stack.push(
                    style_stack
                        .last()
                        .copied()
                        .unwrap_or_default()
                        .add_modifier(Modifier::ITALIC),
                );
            }
            Event::End(TagEnd::Emphasis) => {
                style_stack.pop();
            }
            Event::Code(code) => {
                let style = style_stack
                    .last()
                    .copied()
                    .unwrap_or_default()
                    .fg(Color::Gray);
                current_spans.push(Span::styled(code.to_string(), style));
            }
            Event::Text(text) => {
                if in_code_block {
                    code_block_text.push_str(&text);
                } else {
                    let style = style_stack.last().copied().unwrap_or_default();
                    current_spans.push(Span::styled(text.to_string(), style));
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                flush_spans(&mut current_spans, &mut lines);
            }
            Event::Start(Tag::Table(_)) => {}
            Event::End(TagEnd::Table) => {}
            Event::Start(Tag::TableHead) => {
                flush_spans(&mut current_spans, &mut lines);
                style_stack.push(Style::default().add_modifier(Modifier::BOLD));
            }
            Event::End(TagEnd::TableHead) => {
                flush_spans(&mut current_spans, &mut lines);
                style_stack.pop();
            }
            Event::Start(Tag::TableCell) => {
                current_spans.push(Span::raw(" "));
            }
            Event::End(TagEnd::TableCell) => {
                current_spans.push(Span::raw(" │"));
            }
            Event::Start(Tag::TableRow) => {
                flush_spans(&mut current_spans, &mut lines);
            }
            Event::End(TagEnd::TableRow) => {
                flush_spans(&mut current_spans, &mut lines);
            }
            Event::Html(html) => {
                let style = Style::default().fg(Color::DarkGray);
                for line in html.lines() {
                    lines.push(Line::from(Span::styled(line.to_string(), style)));
                }
            }
            _ => {}
        }
    }

    flush_spans(&mut current_spans, &mut lines);
    flush_lines(&mut lines, &mut blocks);

    // Apply wrapping to text blocks
    for block in &mut blocks {
        if let DocBlock::Text(lines) = block {
            wrap_lines(lines, width as usize);
        }
    }

    blocks
}

fn flush_spans(spans: &mut Vec<Span<'static>>, lines: &mut Vec<Line<'static>>) {
    if spans.is_empty() {
        return;
    }
    let line = Line::from(std::mem::take(spans));
    lines.push(line);
}

fn highlight_code_block(code: &str, lang: Option<&str>) -> Vec<Line<'static>> {
    let code = code.trim_end_matches('\n');
    if code.is_empty() {
        return vec![Line::from(Span::styled(
            "  (empty)",
            Style::default().fg(Color::DarkGray),
        ))];
    }

    let syntax = lang.and_then(|l| {
        SYNTAX_SET
            .find_syntax_by_token(l)
            .or_else(|| SYNTAX_SET.find_syntax_by_extension(l))
    });

    let Some(syntax) = syntax else {
        return code
            .lines()
            .map(|line| {
                Line::from(Span::styled(
                    format!("  {line}"),
                    Style::default().fg(Color::Gray),
                ))
            })
            .collect();
    };

    let theme = &THEME_SET.themes[THEME_NAME];
    let mut highlighter = HighlightLines::new(syntax, theme);

    let mut result = Vec::new();
    for line in LinesWithEndings::from(code) {
        let Ok(ranges) = highlighter.highlight_line(line, &SYNTAX_SET) else {
            result.push(Line::from(Span::styled(
                format!("  {line}"),
                Style::default().fg(Color::Gray),
            )));
            continue;
        };

        let mut spans: Vec<Span<'static>> = vec![Span::raw("  ")];
        for (syn_style, text) in ranges {
            let style = syntect_style_to_ratatui(syn_style);
            spans.push(Span::styled(text.to_string(), style));
        }
        result.push(Line::from(spans));
    }

    result
}

fn syntect_style_to_ratatui(style: SynStyle) -> Style {
    let fg = style.foreground;
    let color = Color::Rgb(fg.r, fg.g, fg.b);

    let mut s = Style::default().fg(color);

    if style
        .font_style
        .contains(syntect::highlighting::FontStyle::BOLD)
    {
        s = s.add_modifier(Modifier::BOLD);
    }
    if style
        .font_style
        .contains(syntect::highlighting::FontStyle::ITALIC)
    {
        s = s.add_modifier(Modifier::ITALIC);
    }
    if style
        .font_style
        .contains(syntect::highlighting::FontStyle::UNDERLINE)
    {
        s = s.add_modifier(Modifier::UNDERLINED);
    }

    s
}

fn wrap_lines(lines: &mut Vec<Line<'static>>, max_width: usize) {
    if max_width < 10 {
        return;
    }

    let mut wrapped = Vec::with_capacity(lines.len());
    for line in lines.drain(..) {
        let line_width: usize = line.width();
        if line_width <= max_width {
            wrapped.push(line);
            continue;
        }

        let mut current_spans: Vec<Span<'static>> = Vec::new();
        let mut current_width: usize = 0;

        for span in line.spans {
            let span_width = span.width();
            if current_width + span_width <= max_width {
                current_spans.push(span);
                current_width += span_width;
            } else {
                let text = span.content.to_string();
                let style = span.style;
                let chars: Vec<char> = text.chars().collect();
                let mut pos = 0;

                while pos < chars.len() {
                    let remaining = max_width.saturating_sub(current_width);
                    if remaining == 0 && !current_spans.is_empty() {
                        wrapped.push(Line::from(std::mem::take(&mut current_spans)));
                        current_width = 0;
                        continue;
                    }
                    let chunk_len = remaining.min(chars.len() - pos);
                    let chunk: String = chars[pos..pos + chunk_len].iter().collect();
                    current_spans.push(Span::styled(chunk, style));
                    current_width += chunk_len;
                    pos += chunk_len;

                    if pos < chars.len() {
                        wrapped.push(Line::from(std::mem::take(&mut current_spans)));
                        current_width = 0;
                    }
                }
            }
        }

        if !current_spans.is_empty() {
            wrapped.push(Line::from(current_spans));
        }
    }

    *lines = wrapped;
}
