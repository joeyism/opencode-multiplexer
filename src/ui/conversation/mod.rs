use std::sync::LazyLock;

pub mod diagram;
pub mod document;
pub mod selection;

use diagram::hash::hash_source;
use diagram::slot::{DiagramIndex, DiagramSlot};
use document::{ConversationDocument, DocBlock, MessageRole, TextRow};

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

pub const GUTTER: &str = "▌ ";
pub const TOOL_INDENT: &str = "  ";
pub const REASONING_PREFIX: &str = "· ";

/// Marker drawn in front of blockquoted text; decoration, never copied.
const BQ_MARK: &str = "▏ ";
/// Leading pad inside a code panel; decoration, never copied.
const CODE_INDENT: &str = "  ";
/// Display width of [`GUTTER`].
const GUTTER_WIDTH: u16 = 2;
/// Display width of [`REASONING_PREFIX`].
const REASONING_PREFIX_WIDTH: u16 = 2;

struct ListState {
    ordered: bool,
    counter: usize,
}

struct TableRow {
    cells: Vec<Vec<Span<'static>>>,
    is_header: bool,
}

struct TableBuffer {
    rows: Vec<TableRow>,
    current_row: Vec<Vec<Span<'static>>>,
    current_cell: Vec<Span<'static>>,
    in_head: bool,
    in_cell: bool,
}

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
    Color::Reset
}
pub fn body_assistant_color() -> Color {
    Color::Reset
}
pub fn code_accent() -> Color {
    Color::Green
}
pub fn link_color() -> Color {
    Color::Cyan
}
pub fn strong_color() -> Color {
    Color::White
}
pub fn panel_bg() -> Color {
    Color::Rgb(24, 24, 28)
}
pub fn reasoning_style() -> Style {
    Style::default().fg(Color::Gray)
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
        doc.push_decoration(vec![Line::from(Span::styled(
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
            "user" => "you".to_string(),
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
        let body_gutter_span = Span::styled(GUTTER, Style::default().fg(meta_color()));

        let mut header_spans = vec![
            gutter_span.clone(),
            Span::styled(
                "● ",
                Style::default().fg(role_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                role_label,
                Style::default().fg(role_color).add_modifier(Modifier::BOLD),
            ),
        ];

        if let Some(model) = &msg.model_id {
            header_spans.push(Span::styled(
                format!(" · {model}"),
                Style::default().fg(meta_color()),
            ));
        }

        let time_span = Span::styled(format!("  {time_str}"), Style::default().fg(meta_color()));
        // Right-align the timestamp to the viewport edge. Fall back to
        // inline placement when the viewport is too narrow for padding.
        let target = width as usize;
        let left_w: usize = header_spans.iter().map(|s| s.width()).sum();
        let time_w = time_span.width();
        let pad = if target >= left_w + time_w + 4 {
            target - left_w - time_w
        } else {
            0
        };
        if pad > 0 {
            header_spans.push(Span::raw(" ".repeat(pad)));
        }
        header_spans.push(time_span);

        if !doc.blocks.is_empty() {
            if !doc.ends_with_blank() {
                doc.push_decoration(vec![Line::from("")]);
            }
            let rule = Span::styled(
                "─".repeat(width as usize),
                Style::default().fg(meta_color()),
            );
            doc.push_decoration(vec![Line::from(rule)]);
        }

        doc.record_header(match msg.role.as_str() {
            "user" => MessageRole::User,
            "assistant" => MessageRole::Assistant,
            _ => MessageRole::Other,
        });
        doc.push_decoration(vec![Line::from(header_spans)]);

        let mut text_buffer = String::new();
        // Character offset of the next body row inside this message's
        // readable text; used to anchor selection across rebuilds.
        let mut text_offset = 0usize;

        let flush_text = |buf: &mut String,
                          doc: &mut ConversationDocument,
                          msg_id: &str,
                          text_offset: &mut usize| {
            if buf.is_empty() {
                return;
            }
            let blocks = render_markdown_blocks(buf, content_width, diagrams);
            for block in blocks {
                match block {
                    DocBlock::Text(mut rows) => {
                        for row in &mut rows {
                            apply_body_color(&mut row.line, body_color);
                            if row.text.is_empty() {
                                // Blank paragraph lines keep no gutter.
                                continue;
                            }
                            row.text_col += GUTTER_WIDTH;
                            row.line = prefix_gutter(
                                std::mem::take(&mut row.line),
                                body_gutter_span.clone(),
                            );
                        }
                        doc.push_text_rows(rows, msg_id, text_offset);
                    }
                    DocBlock::Diagram(slot) => {
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
                    flush_text(&mut text_buffer, &mut doc, &msg.id, &mut text_offset);

                    if let Some(text) = &part.text
                        && !text.is_empty()
                    {
                        let reasoning_width =
                            content_width.saturating_sub(REASONING_PREFIX.chars().count() as u16);
                        let blocks = render_markdown_blocks(text, reasoning_width, diagrams);
                        let mut first = true;
                        for block in blocks {
                            match block {
                                DocBlock::Text(mut r_rows) => {
                                    for r_row in &mut r_rows {
                                        // Drop blank paragraph lines.
                                        if r_row.text.is_empty() {
                                            continue;
                                        }
                                        // Mute reasoning content; the only accent that survives is inline code.
                                        for s in &mut r_row.line.spans {
                                            if s.style.fg != Some(code_accent()) {
                                                s.style.fg = Some(
                                                    reasoning_style().fg.unwrap_or(Color::Gray),
                                                );
                                            }
                                        }
                                        let mut spans = vec![body_gutter_span.clone()];
                                        if first {
                                            spans.push(Span::styled(
                                                REASONING_PREFIX,
                                                reasoning_style(),
                                            ));
                                        } else {
                                            spans.push(Span::styled("  ", reasoning_style()));
                                        }
                                        spans.append(&mut r_row.line.spans);
                                        r_row.line = Line::from(spans);
                                        r_row.text_col += GUTTER_WIDTH + REASONING_PREFIX_WIDTH;
                                        first = false;
                                    }
                                    doc.push_text_rows(r_rows, &msg.id, &mut text_offset);
                                }
                                DocBlock::Diagram(slot) => {
                                    doc.push_diagram(slot);
                                }
                            }
                        }
                    }
                }
                "tool" => {
                    flush_text(&mut text_buffer, &mut doc, &msg.id, &mut text_offset);
                    let tool_line = render_tool_line(part, content_width);
                    let text = plain_text(&tool_line);
                    let line = prefix_gutter(tool_line, body_gutter_span.clone());
                    doc.push_text_rows(
                        vec![TextRow::body(line, text, GUTTER_WIDTH, false)],
                        &msg.id,
                        &mut text_offset,
                    );
                }
                _ => {
                    flush_text(&mut text_buffer, &mut doc, &msg.id, &mut text_offset);
                    let other_line = Line::from(vec![
                        Span::raw(TOOL_INDENT),
                        Span::styled(
                            format!("[{}]", part.part_type),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]);
                    let text = plain_text(&other_line);
                    let line = prefix_gutter(other_line, body_gutter_span.clone());
                    doc.push_text_rows(
                        vec![TextRow::body(line, text, GUTTER_WIDTH, false)],
                        &msg.id,
                        &mut text_offset,
                    );
                }
            }
        }

        flush_text(&mut text_buffer, &mut doc, &msg.id, &mut text_offset);

        if !doc.ends_with_blank() {
            doc.push_decoration(vec![Line::from("")]);
        }
    }

    doc
}

pub fn build_document(messages: &[DbConversationMessage], width: u16) -> Vec<Line<'static>> {
    let doc = build_conversation_document(messages, width, &DiagramIndex::default());
    doc.visible_lines(0, usize::MAX, false)
}

/// Status readout for the conversation footer: follow state, scroll %, and
/// position. Optionally appends the active search match count.
pub fn conversation_status_line(
    scroll: usize,
    total: usize,
    follow_tail: bool,
    match_status: Option<(usize, usize)>,
) -> Line<'static> {
    let pct = if total == 0 {
        0
    } else {
        (scroll as f64 / total as f64 * 100.0).round() as usize
    };
    let mut spans = Vec::new();
    if follow_tail {
        spans.push(Span::styled(" LIVE", Style::default().fg(Color::Green)));
    }
    spans.push(Span::styled(
        format!(" · {pct}% · {scroll}/{total}"),
        Style::default().fg(meta_color()),
    ));
    if let Some((cur, total_matches)) = match_status {
        spans.push(Span::styled(
            format!(" · {cur}/{total_matches}"),
            Style::default().fg(Color::Yellow),
        ));
    }
    Line::from(spans)
}

fn prefix_gutter(line: Line<'static>, gutter_span: Span<'static>) -> Line<'static> {
    let mut spans = vec![gutter_span];
    spans.extend(line.spans);
    Line::from(spans)
}

/// Readable text of a rendered line: every span's content, without any trailing
/// line terminator that syntax highlighting may have carried along.
fn plain_text(line: &Line<'_>) -> String {
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    text.trim_end_matches(['\n', '\r']).to_string()
}

/// Split one logical line into display rows, flagging soft-wrap continuations.
fn wrap_line(line: Line<'static>, max_width: usize) -> Vec<(Line<'static>, bool)> {
    if max_width < 10 || line.width() <= max_width {
        return vec![(line, false)];
    }

    let mut out: Vec<(Line<'static>, bool)> = Vec::new();
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
                    out.push((
                        Line::from(std::mem::take(&mut current_spans)),
                        !out.is_empty(),
                    ));
                    current_width = 0;
                    continue;
                }
                let chunk_len = remaining.min(chars.len() - pos);
                let chunk: String = chars[pos..pos + chunk_len].iter().collect();
                current_spans.push(Span::styled(chunk, style));
                current_width += chunk_len;
                pos += chunk_len;

                if pos < chars.len() {
                    out.push((
                        Line::from(std::mem::take(&mut current_spans)),
                        !out.is_empty(),
                    ));
                    current_width = 0;
                }
            }
        }
    }

    if !current_spans.is_empty() {
        out.push((Line::from(current_spans), !out.is_empty()));
    }
    out
}

/// Wrap a logical line into selectable body rows. `bq` strips the blockquote
/// marker, which is decoration rather than copyable content.
fn body_rows(line: Line<'static>, max_width: usize, bq: bool) -> Vec<TextRow> {
    wrap_line(line, max_width)
        .into_iter()
        .map(|(line, continuation)| {
            let mut text = plain_text(&line);
            let mut text_col = 0u16;
            if bq && text.starts_with(BQ_MARK) {
                text = text[BQ_MARK.len()..].to_string();
                text_col = BQ_MARK.chars().count() as u16;
            }
            TextRow::body(line, text, text_col, continuation)
        })
        .collect()
}

/// A code panel row: the panel's left pad is decoration, the code is content.
fn code_row(line: Line<'static>, width: usize) -> TextRow {
    let mut text = plain_text(&line);
    let mut text_col = 0u16;
    if let Some(rest) = text.strip_prefix(CODE_INDENT) {
        text = rest.to_string();
        text_col = CODE_INDENT.chars().count() as u16;
    }
    let mut line = line;
    let w = line.width();
    if w < width {
        line.spans.push(Span::styled(
            " ".repeat(width - w),
            Style::default().bg(panel_bg()),
        ));
    }
    TextRow::body(line, text, text_col, false)
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

fn render_table_cell(cell: &[Span<'static>], width: usize) -> Vec<Span<'static>> {
    let cell_w: usize = cell.iter().map(|s| s.width()).sum();
    if cell_w <= width {
        let mut out: Vec<Span<'static>> = cell.to_vec();
        if cell_w < width {
            out.push(Span::raw(" ".repeat(width - cell_w)));
        }
        out
    } else {
        let flat: String = cell.iter().map(|s| s.content.as_ref()).collect();
        let truncated: String = flat.chars().take(width.saturating_sub(1)).collect();
        vec![Span::raw(format!("{truncated}…"))]
    }
}

/// Render a buffered table with aligned columns. `width` is the content width
/// (before the gutter is prepended by the caller).
fn render_table(rows: Vec<TableRow>, width: usize) -> Vec<Line<'static>> {
    if rows.is_empty() {
        return Vec::new();
    }
    let col_count = rows.iter().map(|r| r.cells.len()).max().unwrap_or(0);
    if col_count == 0 {
        return Vec::new();
    }

    let mut col_widths = vec![0usize; col_count];
    for row in &rows {
        for (c, cell) in row.cells.iter().enumerate() {
            let w: usize = cell.iter().map(|s| s.width()).sum();
            col_widths[c] = col_widths[c].max(w.min(40));
        }
    }

    // Fixed overhead per row: "│ " left edge + " │ " between columns + " │" right edge
    // = 2 + 3*(col_count-1) + 2 = 3*col_count + 1.
    let fixed = 3 * col_count + 1;
    if width > fixed {
        let budget = width - fixed;
        let mut total: usize = col_widths.iter().sum();
        while total > budget {
            let widest = col_widths
                .iter()
                .enumerate()
                .filter(|(_, w)| **w > 6)
                .max_by_key(|(_, w)| **w)
                .map(|(i, _)| i);
            match widest {
                Some(idx) => {
                    col_widths[idx] -= 1;
                    total -= 1;
                }
                None => break,
            }
        }
    }

    let mut out = Vec::new();
    for row in &rows {
        let mut spans: Vec<Span<'static>> =
            vec![Span::styled("│ ", Style::default().fg(meta_color()))];
        for (c, col_w) in col_widths.iter().enumerate() {
            let cell = row.cells.get(c).cloned().unwrap_or_default();
            let mut cell_spans = render_table_cell(&cell, *col_w);
            if row.is_header {
                for s in &mut cell_spans {
                    s.style = s.style.add_modifier(Modifier::BOLD);
                }
            }
            spans.append(&mut cell_spans);
            if c + 1 < col_count {
                spans.push(Span::styled(" │ ", Style::default().fg(meta_color())));
            }
        }
        spans.push(Span::styled(" │", Style::default().fg(meta_color())));
        out.push(Line::from(spans));

        if row.is_header {
            let mut rule: Vec<Span<'static>> =
                vec![Span::styled("│ ", Style::default().fg(meta_color()))];
            for (c, col_w) in col_widths.iter().enumerate() {
                rule.push(Span::styled(
                    "─".repeat(*col_w),
                    Style::default().fg(meta_color()),
                ));
                if c + 1 < col_count {
                    rule.push(Span::styled(" ┼ ", Style::default().fg(meta_color())));
                }
            }
            rule.push(Span::styled(" │", Style::default().fg(meta_color())));
            out.push(Line::from(rule));
        }
    }
    out
}

fn render_markdown_blocks(text: &str, width: u16, diagrams: &DiagramIndex) -> Vec<DocBlock> {
    let mut blocks: Vec<DocBlock> = Vec::new();
    let mut lines: Vec<TextRow> = Vec::new();
    let parser = Parser::new_ext(text, Options::ENABLE_TABLES);
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut style_stack: Vec<Style> = vec![Style::default()];
    let mut in_code_block = false;
    let mut code_block_lang: Option<String> = None;
    let mut code_block_text = String::new();

    let flush_lines = |lines: &mut Vec<TextRow>, blocks: &mut Vec<DocBlock>| {
        if !lines.is_empty() {
            blocks.push(DocBlock::Text(std::mem::take(lines)));
        }
    };

    // Flush current spans into a line. When inside a blockquote, prepend the
    // marker so the quote is visually distinct. blockquote depth is threaded
    // explicitly to keep the closure borrow-free.
    let flush_spans =
        |spans: &mut Vec<Span<'static>>, lines: &mut Vec<TextRow>, bq_depth: usize| {
            if spans.is_empty() {
                return;
            }
            let bq = bq_depth > 0;
            let mut styled = Vec::with_capacity(spans.len() + 1);
            if bq {
                styled.push(Span::styled(BQ_MARK, Style::default().fg(meta_color())));
            }
            styled.extend(std::mem::take(spans));
            lines.extend(body_rows(Line::from(styled), width as usize, bq));
        };

    let mut list_stack: Vec<ListState> = Vec::new();
    let mut link_stack: Vec<(String, String)> = Vec::new(); // (dest_url, accumulated text)
    let mut blockquote_depth: usize = 0;
    let mut table: Option<TableBuffer> = None;

    // Emit a blank line, collapsing runs so consecutive blanks never occur.
    let push_blank = |lines: &mut Vec<TextRow>| {
        let last_is_blank = lines
            .last()
            .is_some_and(|l| l.line.spans.iter().all(|s| s.content.is_empty()));
        if !last_is_blank {
            lines.push(TextRow::body(Line::from(""), String::new(), 0, false));
        }
    };

    for event in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                flush_spans(&mut current_spans, &mut lines, blockquote_depth);
                let color = if level as usize >= 3 {
                    strong_color()
                } else {
                    link_color()
                };
                style_stack.push(Style::default().fg(color).add_modifier(Modifier::BOLD));
            }
            Event::End(TagEnd::Heading(_)) => {
                flush_spans(&mut current_spans, &mut lines, blockquote_depth);
                style_stack.pop();
            }
            Event::Start(Tag::Paragraph) => {
                flush_spans(&mut current_spans, &mut lines, blockquote_depth);
            }
            Event::End(TagEnd::Paragraph) => {
                flush_spans(&mut current_spans, &mut lines, blockquote_depth);
                push_blank(&mut lines);
            }
            Event::Start(Tag::BlockQuote(_)) => {
                flush_spans(&mut current_spans, &mut lines, blockquote_depth);
                blockquote_depth += 1;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                flush_spans(&mut current_spans, &mut lines, blockquote_depth);
                blockquote_depth = blockquote_depth.saturating_sub(1);
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                link_stack.push((dest_url.to_string(), String::new()));
                style_stack.push(
                    style_stack
                        .last()
                        .copied()
                        .unwrap_or_default()
                        .fg(link_color())
                        .add_modifier(Modifier::UNDERLINED),
                );
            }
            Event::End(TagEnd::Link) => {
                if let Some((dest_url, link_text)) = link_stack.pop()
                    && link_text != dest_url
                {
                    current_spans.push(Span::styled(
                        format!(" ({dest_url})"),
                        Style::default().fg(meta_color()),
                    ));
                }
                style_stack.pop();
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                flush_spans(&mut current_spans, &mut lines, blockquote_depth);
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
                    let pad = Line::from(Span::styled(
                        " ".repeat(width as usize),
                        Style::default().bg(panel_bg()),
                    ));
                    lines.push(TextRow::decoration(pad.clone()));
                    for line in highlight_code_block(&code_block_text, code_block_lang.as_deref()) {
                        lines.push(code_row(line, width as usize));
                    }
                    lines.push(TextRow::decoration(pad));
                    push_blank(&mut lines);
                }
                code_block_lang = None;
            }
            Event::Start(Tag::List(start_num)) => {
                list_stack.push(ListState {
                    ordered: start_num.is_some(),
                    counter: start_num.unwrap_or(1).saturating_sub(1) as usize,
                });
            }
            Event::End(TagEnd::List(_)) => {
                list_stack.pop();
            }
            Event::Start(Tag::Item) => {
                flush_spans(&mut current_spans, &mut lines, blockquote_depth);
                let prefix = if let Some(state) = list_stack.last_mut() {
                    if state.ordered {
                        state.counter += 1;
                        format!("  {}. ", state.counter)
                    } else {
                        "  • ".to_string()
                    }
                } else {
                    "  • ".to_string()
                };
                current_spans.push(Span::styled(prefix, Style::default().fg(meta_color())));
            }
            Event::End(TagEnd::Item) => {
                flush_spans(&mut current_spans, &mut lines, blockquote_depth);
            }
            Event::Start(Tag::Strong) => {
                style_stack.push(
                    style_stack
                        .last()
                        .copied()
                        .unwrap_or_default()
                        .fg(strong_color())
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
                    .fg(code_accent());
                let span = Span::styled(code.to_string(), style);
                if let Some(t) = table.as_mut()
                    && t.in_cell
                {
                    t.current_cell.push(span);
                } else {
                    current_spans.push(span);
                }
            }
            Event::Text(text) => {
                if in_code_block {
                    code_block_text.push_str(&text);
                } else if let Some(t) = table.as_mut()
                    && t.in_cell
                {
                    if let Some((_, link_text)) = link_stack.last_mut() {
                        link_text.push_str(&text);
                    }
                    let mut style = style_stack.last().copied().unwrap_or_default();
                    if blockquote_depth > 0 && style.fg.is_none() {
                        style = style.fg(meta_color());
                    }
                    t.current_cell.push(Span::styled(text.to_string(), style));
                } else {
                    if let Some((_, link_text)) = link_stack.last_mut() {
                        link_text.push_str(&text);
                    }
                    let mut style = style_stack.last().copied().unwrap_or_default();
                    if blockquote_depth > 0 && style.fg.is_none() {
                        style = style.fg(meta_color());
                    }
                    current_spans.push(Span::styled(text.to_string(), style));
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(t) = table.as_mut()
                    && t.in_cell
                {
                    t.current_cell.push(Span::raw(" "));
                } else {
                    flush_spans(&mut current_spans, &mut lines, blockquote_depth);
                }
            }
            Event::Start(Tag::Table(_)) => {
                flush_spans(&mut current_spans, &mut lines, blockquote_depth);
                table = Some(TableBuffer {
                    rows: Vec::new(),
                    current_row: Vec::new(),
                    current_cell: Vec::new(),
                    in_head: false,
                    in_cell: false,
                });
            }
            Event::End(TagEnd::Table) => {
                if let Some(t) = table.take() {
                    lines.extend(
                        render_table(t.rows, width as usize)
                            .into_iter()
                            .flat_map(|l| body_rows(l, width as usize, false)),
                    );
                }
            }
            Event::Start(Tag::TableHead) => {
                if let Some(t) = table.as_mut() {
                    t.in_head = true;
                }
                style_stack.push(Style::default().add_modifier(Modifier::BOLD));
            }
            Event::End(TagEnd::TableHead) => {
                if let Some(t) = table.as_mut() {
                    // pulldown-cmark emits head cells directly under TableHead
                    // (no TableRow wrapper) — flush them as a header row here.
                    if !t.current_row.is_empty() {
                        let cells = std::mem::take(&mut t.current_row);
                        t.rows.push(TableRow {
                            cells,
                            is_header: true,
                        });
                    }
                    t.in_head = false;
                }
                style_stack.pop();
            }
            Event::Start(Tag::TableRow) => {
                if let Some(t) = table.as_mut()
                    && !t.current_row.is_empty()
                {
                    let cells = std::mem::take(&mut t.current_row);
                    let is_header = t.in_head;
                    t.rows.push(TableRow { cells, is_header });
                }
            }
            Event::End(TagEnd::TableRow) => {
                if let Some(t) = table.as_mut() {
                    let cells = std::mem::take(&mut t.current_row);
                    let is_header = t.in_head;
                    t.rows.push(TableRow { cells, is_header });
                }
            }
            Event::Start(Tag::TableCell) => {
                if let Some(t) = table.as_mut() {
                    t.current_cell = Vec::new();
                    t.in_cell = true;
                }
            }
            Event::End(TagEnd::TableCell) => {
                if let Some(t) = table.as_mut() {
                    let cell = std::mem::take(&mut t.current_cell);
                    t.current_row.push(cell);
                    t.in_cell = false;
                }
            }
            Event::Html(html) => {
                let style = Style::default().fg(Color::DarkGray);
                for line in html.lines() {
                    lines.extend(body_rows(
                        Line::from(Span::styled(line.to_string(), style)),
                        width as usize,
                        false,
                    ));
                }
            }
            _ => {}
        }
    }

    flush_spans(&mut current_spans, &mut lines, blockquote_depth);
    flush_lines(&mut lines, &mut blocks);

    blocks
}

fn highlight_code_block(code: &str, lang: Option<&str>) -> Vec<Line<'static>> {
    let code = code.trim_end_matches('\n');
    if code.is_empty() {
        return vec![Line::from(Span::styled(
            "  (empty)",
            Style::default().fg(Color::DarkGray).bg(panel_bg()),
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
                    Style::default().fg(Color::Gray).bg(panel_bg()),
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
                Style::default().fg(Color::Gray).bg(panel_bg()),
            )));
            continue;
        };

        let mut spans: Vec<Span<'static>> =
            vec![Span::styled("  ", Style::default().bg(panel_bg()))];
        for (syn_style, text) in ranges {
            let style = syntect_style_to_ratatui(syn_style).bg(panel_bg());
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
