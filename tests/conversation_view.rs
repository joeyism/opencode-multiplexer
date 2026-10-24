use opencode_multiplexer::app::conversation::ConversationViewState;
use opencode_multiplexer::data::db::models::{DbConversationMessage, DbConversationPart};
use opencode_multiplexer::ui::conversation::diagram::slot::DiagramIndex;
use opencode_multiplexer::ui::conversation::document::{DocBlock, MessageRole};
use opencode_multiplexer::ui::conversation::{
    self, GUTTER, REASONING_PREFIX, TOOL_INDENT, build_conversation_document,
};
use opencode_multiplexer::ui::diff::highlight_search_matches;
use ratatui::style::{Color, Modifier};
use ratatui::text::Line;

fn part(part_type: &str, text: Option<&str>) -> DbConversationPart {
    DbConversationPart {
        id: "p".into(),
        part_type: part_type.into(),
        text: text.map(str::to_string),
        tool: None,
        tool_status: None,
        tool_title: None,
        tool_input: None,
    }
}

#[allow(dead_code)]
fn tool_part(tool: &str, status: &str, title: Option<&str>) -> DbConversationPart {
    DbConversationPart {
        id: "t".into(),
        part_type: "tool".into(),
        text: None,
        tool: Some(tool.into()),
        tool_status: Some(status.into()),
        tool_title: title.map(str::to_string),
        tool_input: None,
    }
}

fn msg(role: &str, parts: Vec<DbConversationPart>) -> DbConversationMessage {
    DbConversationMessage {
        id: "m".into(),
        role: role.into(),
        time_created: 1_700_000_000_000, // ms → 14:13 UTC-ish; assert via format only if needed
        completed: Some(1_700_000_001_000),
        model_id: None,
        agent: None,
        parts,
    }
}

fn flat(line: &Line<'_>) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

#[allow(dead_code)]
fn flats(lines: &[Line<'static>]) -> Vec<String> {
    lines.iter().map(flat).collect()
}

/// First span style that has a foreground set, else default.
#[allow(dead_code)]
fn first_fg(line: &Line<'_>) -> Option<Color> {
    line.spans.iter().find_map(|s| s.style.fg)
}

#[allow(dead_code)]
fn line_has_modifier(line: &Line<'_>, modifier: Modifier) -> bool {
    line.spans
        .iter()
        .any(|s| s.style.add_modifier.contains(modifier))
}

#[test]
fn empty_messages_shows_placeholder_without_gutter() {
    let lines = conversation::build_document(&[], 80);
    assert_eq!(lines.len(), 1);
    assert_eq!(flat(&lines[0]), "No messages yet.");
    assert!(!flat(&lines[0]).starts_with(GUTTER));
}

#[test]
fn user_message_body_lines_have_dim_gutter() {
    let m = msg("user", vec![part("text", Some("Hello"))]);
    let lines = conversation::build_document(&[m], 80);
    // lines[0] is header, lines[1] is body
    assert!(lines.len() >= 2);
    assert!(flat(&lines[1]).starts_with(GUTTER));
    assert_eq!(first_fg(&lines[1]), Some(conversation::meta_color()));
    // body content is neutral; only the header carries the role color
    let content = lines[1]
        .spans
        .iter()
        .find(|s| s.content.contains("Hello"))
        .unwrap();
    assert!(
        content.style.fg.is_none() || content.style.fg == Some(Color::Reset),
        "user body content should be neutral, got {:?}",
        content.style.fg
    );
}

#[test]
fn assistant_message_body_lines_have_dim_gutter() {
    let m = msg("assistant", vec![part("text", Some("Hi"))]);
    let lines = conversation::build_document(&[m], 80);
    assert!(lines.len() >= 2);
    assert!(flat(&lines[1]).starts_with(GUTTER));
    assert_eq!(first_fg(&lines[1]), Some(conversation::meta_color()));
}

#[test]
fn header_gutter_bright_body_gutter_dim() {
    let m = msg("user", vec![part("text", Some("Hello"))]);
    let lines = conversation::build_document(&[m], 80);
    let header = &lines[0];
    assert_eq!(
        header.spans[0].style.fg,
        Some(conversation::user_color()),
        "header gutter should be bright role color"
    );
    assert!(header.spans[0].style.add_modifier.contains(Modifier::BOLD));
    assert_eq!(
        lines[1].spans[0].style.fg,
        Some(conversation::meta_color()),
        "body gutter should be dim"
    );
    assert!(
        !lines[1].spans[0]
            .style
            .add_modifier
            .contains(Modifier::BOLD)
    );
}

#[test]
fn multi_line_body_every_content_line_has_gutter() {
    let m = msg("user", vec![part("text", Some("Line 1\n\nLine 2"))]);
    let lines = conversation::build_document(&[m], 80);
    let texts = flats(&lines);
    assert!(
        texts.iter().any(|t| t.is_empty()),
        "paragraphs should be separated by a blank line: {texts:?}"
    );
    for (i, t) in texts.iter().enumerate() {
        if t.is_empty() {
            continue;
        }
        assert!(
            t.starts_with(GUTTER),
            "content line {i} missing gutter: {t:?}"
        );
    }
}

#[test]
fn user_label_is_lowercase_you() {
    let m = msg("user", vec![part("text", Some("hi"))]);
    let lines = conversation::build_document(&[m], 80);
    let header = &lines[0];
    let t = flat(header);
    assert!(t.starts_with(GUTTER));
    assert!(t.contains("you"));
    assert!(!t.contains("YOU"), "role label should be lowercase: {t}");
    // role span bold+cyan; time span dark gray
    let has_dim_time = header.spans.iter().any(|s| {
        s.style.fg == Some(conversation::meta_color())
            && s.content.chars().any(|c| c.is_ascii_digit())
    });
    assert!(has_dim_time, "expected dim timestamp in header: {t:?}");
    let role_bold = header
        .spans
        .iter()
        .any(|s| s.content.contains("you") && s.style.add_modifier.contains(Modifier::BOLD));
    assert!(role_bold);
}

#[test]
fn header_has_filled_dot_marker_in_role_color() {
    let m = msg("user", vec![part("text", Some("hi"))]);
    let header = &conversation::build_document(&[m], 80)[0];
    let marker = header
        .spans
        .iter()
        .find(|s| s.content.contains('●'))
        .unwrap();
    assert_eq!(marker.style.fg, Some(conversation::user_color()));
}

#[test]
fn assistant_header_joins_agent_and_model_with_dot() {
    let mut m = msg("assistant", vec![part("text", Some("ok"))]);
    m.agent = Some("chat".into());
    m.model_id = Some("grok-4.5".into());
    let lines = conversation::build_document(&[m], 80);
    let t = flat(&lines[0]);
    assert!(
        t.contains("chat · grok-4.5"),
        "agent and model should be joined with a dot: {t}"
    );
    let model_span = lines[0]
        .spans
        .iter()
        .find(|s| s.content.contains("grok-4.5"))
        .unwrap();
    assert_eq!(model_span.style.fg, Some(conversation::meta_color()));
}

#[test]
fn header_without_model_omits_dot_separator() {
    let m = msg("user", vec![part("text", Some("x"))]);
    let t = flat(&conversation::build_document(&[m], 80)[0]);
    assert!(
        !t.contains('·'),
        "no model should mean no dot separator: {t}"
    );
}

#[test]
fn header_timestamp_is_right_aligned() {
    let m = msg("user", vec![part("text", Some("hi"))]);
    let width = 80u16;
    let header = &conversation::build_document(&[m], width)[0];
    assert_eq!(header.width(), width as usize);
    // last span is the timestamp; everything before it ends flush-right
    let prefix_w: usize = header.spans[..header.spans.len() - 1]
        .iter()
        .map(|s| s.width())
        .sum();
    assert_eq!(
        prefix_w + header.spans.last().unwrap().width(),
        width as usize
    );
}

#[test]
fn header_timestamp_inline_when_narrow() {
    let m = msg("user", vec![part("text", Some("hi"))]);
    let lines = conversation::build_document(&[m], 30);
    assert!(
        lines[0].width() <= 30,
        "narrow header must not exceed width: {}",
        lines[0].width()
    );
}

#[test]
fn assistant_header_prefers_nonempty_agent_name() {
    let mut m = msg("assistant", vec![part("text", Some("ok"))]);
    m.agent = Some("build".into());
    m.model_id = Some("claude-sonnet".into());
    let lines = conversation::build_document(&[m], 80);
    let t = flat(&lines[0]);
    assert!(t.contains("build"));
    assert!(t.contains("claude-sonnet"));
    // model is dim
    assert!(lines[0].spans.iter().any(|s| {
        s.content.contains("claude-sonnet") && s.style.fg == Some(conversation::meta_color())
    }));
}

#[test]
fn assistant_header_falls_back_when_agent_empty_or_missing() {
    let mut m = msg("assistant", vec![part("text", Some("ok"))]);
    m.agent = Some("".into());
    let t = flat(&conversation::build_document(&[m], 80)[0]);
    assert!(t.contains("assistant"));

    let mut m2 = msg("assistant", vec![part("text", Some("ok"))]);
    m2.agent = None;
    let t2 = flat(&conversation::build_document(&[m2], 80)[0]);
    assert!(t2.contains("assistant"));
}

#[test]
fn header_omits_model_when_none() {
    let m = msg("user", vec![part("text", Some("x"))]);
    let t = flat(&conversation::build_document(&[m], 80)[0]);
    // only gutter + marker + you + time — no extra trailing junk tokens beyond time
    assert!(t.contains("you"));
    assert!(!t.to_lowercase().contains("none"));
}

#[test]
fn unknown_role_still_gets_gutter_and_label() {
    let m = msg("system", vec![part("text", Some("sys"))]);
    let lines = conversation::build_document(&[m], 80);
    let t = flat(&lines[0]);
    assert!(t.starts_with(GUTTER));
    assert!(t.contains("system"));
}

#[test]
fn blank_separator_between_two_messages() {
    let messages = vec![
        msg("user", vec![part("text", Some("first"))]),
        msg("assistant", vec![part("text", Some("second"))]),
    ];
    let lines = conversation::build_document(&messages, 80);
    let texts = flats(&lines);
    let i_first = texts.iter().position(|t| t.contains("first")).unwrap();
    // Find assistant header: first line containing assistant role after first body
    let i_asst = texts
        .iter()
        .enumerate()
        .find(|(i, t)| {
            *i > i_first && t.contains(GUTTER) && (t.contains("assistant") || t.contains("build"))
        })
        .map(|(i, _)| i)
        .expect("assistant header");
    assert!(
        texts[i_first..i_asst].iter().any(|t| t.is_empty()),
        "expected blank separator between turns: {texts:?}"
    );
}

#[test]
fn separator_line_has_no_gutter() {
    let messages = vec![
        msg("user", vec![part("text", Some("a"))]),
        msg("user", vec![part("text", Some("b"))]),
    ];
    let texts = flats(&conversation::build_document(&messages, 80));
    assert!(texts.iter().any(|t| t.is_empty()));
    for t in &texts {
        if t.is_empty() {
            assert!(!t.starts_with(GUTTER));
        }
    }
}

#[test]
fn reasoning_not_merged_into_answer_markdown() {
    let m = msg(
        "assistant",
        vec![
            part("reasoning", Some("I should check the file")),
            part("text", Some("Here is the answer")),
        ],
    );
    let texts = flats(&conversation::build_document(&[m], 80));
    let r = texts.iter().find(|t| t.contains("I should check")).unwrap();
    let a = texts
        .iter()
        .find(|t| t.contains("Here is the answer"))
        .unwrap();
    assert!(r.starts_with(&format!("{GUTTER}{REASONING_PREFIX}")) || r.contains(REASONING_PREFIX));
    assert!(
        !a.contains("I should check"),
        "reasoning leaked into answer line: {a}"
    );
}

#[test]
fn reasoning_text_is_gray_without_italic() {
    let m = msg("assistant", vec![part("reasoning", Some("think hard"))]);
    let lines = conversation::build_document(&[m], 80);
    let line = lines
        .iter()
        .find(|l| flat(l).contains("think hard"))
        .unwrap();
    // content spans (not gutter) should match reasoning_style fg, no italic
    let content = line
        .spans
        .iter()
        .find(|s| s.content.contains("think"))
        .unwrap();
    assert_eq!(content.style.fg, Some(Color::Gray));
    assert!(!content.style.add_modifier.contains(Modifier::ITALIC));
}

#[test]
fn empty_reasoning_skipped() {
    let m = msg(
        "assistant",
        vec![
            part("reasoning", Some("")),
            part("text", Some("only answer")),
        ],
    );
    let texts = flats(&conversation::build_document(&[m], 80));
    assert!(
        !texts
            .iter()
            .any(|t| t.contains(REASONING_PREFIX) && !t.contains("only"))
    );
    assert!(texts.iter().any(|t| t.contains("only answer")));
}

#[test]
fn interleaved_text_tool_reasoning_preserves_order() {
    let m = msg(
        "assistant",
        vec![
            part("text", Some("before")),
            tool_part("bash", "completed", Some("echo hi")),
            part("reasoning", Some("mid think")),
            part("text", Some("after")),
        ],
    );
    let texts = flats(&conversation::build_document(&[m], 80));
    let positions = |needle: &str| texts.iter().position(|t| t.contains(needle)).expect(needle);
    assert!(positions("before") < positions("bash") || positions("before") < positions("echo hi"));
    assert!(
        positions("echo hi") < positions("mid think") || positions("bash") < positions("mid think")
    );
    assert!(positions("mid think") < positions("after"));
}

#[test]
fn role_distinctness_lives_in_header_marker() {
    let messages = vec![
        msg("user", vec![part("text", Some("USER_ONLY_TOKEN"))]),
        msg("assistant", vec![part("text", Some("ASST_ONLY_TOKEN"))]),
    ];
    let lines = conversation::build_document(&messages, 80);
    // header markers carry the role colors
    let user_header = lines.iter().find(|l| flat(l).contains("● you")).unwrap();
    let um = user_header
        .spans
        .iter()
        .find(|s| s.content.contains('●'))
        .unwrap();
    assert_eq!(um.style.fg, Some(conversation::user_color()));
    let asst_header = lines
        .iter()
        .find(|l| flat(l).contains("● assistant"))
        .unwrap();
    let am = asst_header
        .spans
        .iter()
        .find(|s| s.content.contains('●'))
        .unwrap();
    assert_eq!(am.style.fg, Some(conversation::assistant_color()));
    // body gutters are dim for both roles
    for l in lines.iter().filter(|l| flat(l).contains("_ONLY_TOKEN")) {
        assert_eq!(l.spans[0].style.fg, Some(conversation::meta_color()));
    }
}

#[test]
fn tool_line_is_indented_under_gutter() {
    let m = msg(
        "assistant",
        vec![tool_part("bash", "completed", Some("cargo test"))],
    );
    let lines = conversation::build_document(&[m], 80);
    let tool = lines.iter().find(|l| flat(l).contains("bash")).unwrap();
    let t = flat(tool);
    assert!(
        t.starts_with(&format!("{GUTTER}{TOOL_INDENT}")),
        "tool should be gutter+indent, got {t:?}"
    );
    assert!(t.contains('✓') || t.contains("bash"));
}

#[test]
fn tool_statuses_use_distinct_icons() {
    for (status, icon) in [
        ("completed", "✓"),
        ("running", "⟳"),
        ("error", "✗"),
        ("pending", "⏳"),
    ] {
        let m = msg("assistant", vec![tool_part("x", status, None)]);
        let texts = flats(&conversation::build_document(&[m], 80));
        assert!(
            texts.iter().any(|t| t.contains(icon)),
            "status {status} missing icon {icon} in {texts:?}"
        );
    }
}

#[test]
fn tool_falls_back_to_truncated_input_when_no_title() {
    let mut p = tool_part("grep", "completed", None);
    p.tool_input = Some("a".repeat(80));
    let m = msg("assistant", vec![p]);
    let t = flats(&conversation::build_document(&[m], 80))
        .into_iter()
        .find(|t| t.contains("grep"))
        .unwrap();
    assert!(t.contains('…') || t.chars().count() < 80 + 20);
}

#[test]
fn tool_detail_is_muted_not_role_body_color() {
    let m = msg(
        "assistant",
        vec![tool_part("bash", "completed", Some("cargo test"))],
    );
    let line = conversation::build_document(&[m], 80)
        .into_iter()
        .find(|l| flat(l).contains("cargo test"))
        .unwrap();
    let detail = line
        .spans
        .iter()
        .find(|s| s.content.contains("cargo test"))
        .unwrap();
    assert_eq!(detail.style.fg, Some(Color::Gray));
}

#[test]
fn unknown_part_type_muted_under_gutter() {
    let m = msg("assistant", vec![part("step-start", None)]);
    let t = flats(&conversation::build_document(&[m], 80))
        .into_iter()
        .find(|t| t.contains("step-start"))
        .unwrap();
    assert!(t.starts_with(GUTTER));
    assert!(t.contains("[step-start]"));
}

#[test]
fn long_line_wraps_within_total_width_including_gutter() {
    let width = 20u16;
    let word = "word ";
    let body = word.repeat(10); // longer than 20
    let m = msg("user", vec![part("text", Some(&body))]);
    let lines = conversation::build_document(&[m], width);
    for l in &lines {
        let t = flat(l);
        if t.is_empty() {
            continue;
        }
        // ratatui width in cells; GUTTER is 2 cells
        assert!(
            l.width() <= width as usize,
            "line wider than viewport: width={} text={t:?}",
            l.width()
        );
        if t.contains("word") {
            assert!(t.starts_with(GUTTER));
        }
    }
}

#[test]
fn gutter_never_split_across_wrapped_lines() {
    let m = msg("user", vec![part("text", Some(&"x".repeat(50)))]);
    let lines = conversation::build_document(&[m], 16);
    for l in lines.iter().filter(|l| flat(l).contains('x')) {
        assert!(
            flat(l).starts_with(GUTTER),
            "wrapped line lost gutter: {:?}",
            flat(l)
        );
    }
}

#[test]
fn long_reasoning_line_wraps() {
    let width = 20u16;
    let body = "reasoning ".repeat(10);
    let m = msg("assistant", vec![part("reasoning", Some(&body))]);
    let lines = conversation::build_document(&[m], width);
    let mut saw_prefix = false;
    for l in &lines {
        let t = flat(l);
        if t.contains("reasoning") {
            assert!(
                l.width() <= width as usize,
                "reasoning line wider than viewport: width={} text={t:?}",
                l.width()
            );
            assert!(t.starts_with(GUTTER));
            if t.contains(REASONING_PREFIX) {
                saw_prefix = true;
            }
        }
    }
    assert!(
        saw_prefix,
        "wrapped reasoning should still mark its first line"
    );
}

#[test]
fn narrow_width_does_not_panic() {
    let m = msg("user", vec![part("text", Some("hi"))]);
    let _ = conversation::build_document(std::slice::from_ref(&m), 1);
    let _ = conversation::build_document(std::slice::from_ref(&m), 2);
    let _ = conversation::build_document(&[m], 5);
}

#[test]
fn message_with_only_empty_text_parts_still_has_header() {
    let m = msg("user", vec![part("text", Some("")), part("text", None)]);
    let lines = conversation::build_document(&[m], 80);
    assert!(flat(&lines[0]).contains("you"));
}

#[test]
fn message_with_no_parts_still_has_header_and_separator() {
    let m = msg("user", vec![]);
    let lines = conversation::build_document(&[m], 80);
    assert!(flat(&lines[0]).contains("you"));
}

#[test]
fn unicode_body_does_not_panic_and_keeps_gutter() {
    let m = msg("user", vec![part("text", Some("你好 🎉 café"))]);
    let lines = conversation::build_document(&[m], 40);
    let body = lines
        .iter()
        .find(|l| flat(l).contains("café") || flat(l).contains("你好"))
        .unwrap();
    assert!(flat(body).starts_with(GUTTER));
}

#[test]
fn markdown_code_fence_lines_keep_gutter() {
    let md = "before\n```rust\nfn main() {}\n```\nafter";
    let m = msg("assistant", vec![part("text", Some(md))]);
    let lines = conversation::build_document(&[m], 80);
    let code = lines.iter().find(|l| flat(l).contains("fn main")).unwrap();
    assert!(flat(code).starts_with(GUTTER));
}

#[test]
fn markdown_list_items_keep_gutter() {
    let m = msg("user", vec![part("text", Some("1. one\n2. two"))]);
    let lines = conversation::build_document(&[m], 80);
    let hits: Vec<_> = lines
        .iter()
        .filter(|l| flat(l).contains("one") || flat(l).contains("two"))
        .collect();
    assert!(!hits.is_empty());
    for l in hits {
        assert!(flat(l).starts_with(GUTTER));
    }
}

#[test]
fn timestamp_millis_and_seconds_both_format() {
    let mut ms = msg("user", vec![part("text", Some("a"))]);
    ms.time_created = 1_700_000_000_000; // ms
    let mut sec = msg("user", vec![part("text", Some("b"))]);
    sec.time_created = 1_700_000_000; // s
    let t1 = flat(&conversation::build_document(&[ms], 80)[0]);
    let t2 = flat(&conversation::build_document(&[sec], 80)[0]);
    // both should contain HH:MM pattern
    let re_ok = |t: &str| t.chars().filter(|c| c.is_ascii_digit()).count() >= 3;
    assert!(re_ok(&t1), "{t1}");
    assert!(re_ok(&t2), "{t2}");
}

#[test]
fn multiple_tools_in_a_row() {
    let m = msg(
        "assistant",
        vec![
            tool_part("bash", "completed", Some("one")),
            tool_part("read", "running", Some("two")),
            tool_part("grep", "error", Some("three")),
        ],
    );
    let texts = flats(&conversation::build_document(&[m], 80));
    for needle in ["one", "two", "three"] {
        assert!(texts.iter().any(|t| t.contains(needle)), "missing {needle}");
    }
}

#[test]
fn tool_without_tool_name_uses_fallback() {
    let mut p = tool_part("bash", "completed", Some("x"));
    p.tool = None;
    let t = flats(&conversation::build_document(
        &[msg("assistant", vec![p])],
        80,
    ));
    assert!(t.iter().any(|line| line.contains("tool")));
}

#[test]
fn search_finds_body_text_despite_gutter_prefix() {
    let m = msg("user", vec![part("text", Some("unique_needle_abc"))]);
    let doc_blocks =
        build_conversation_document(std::slice::from_ref(&m), 80, &DiagramIndex::default());
    let mut state = ConversationViewState::default();
    state.open(
        "s1".into(),
        "t".into(),
        opencode_multiplexer::app::focus::AppFocus::Sidebar,
    );
    state.replace_document(doc_blocks, 50);
    state.start_search();
    state.search_insert_str("unique_needle", 50);
    let (cur, total) = state.match_status().expect("match");
    assert_eq!(cur, 1);
    assert!(total >= 1);

    let (line_idx, byte_start, len) = state.matches()[0];
    let doc_lines = conversation::build_document(&[m], 80);
    let line_flat: String = doc_lines[line_idx]
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    let sliced = &line_flat[byte_start..byte_start + len];
    assert_eq!(sliced.to_lowercase(), "unique_needle");
}

#[test]
fn search_highlight_preserves_gutter_and_marks_match() {
    let m = msg("user", vec![part("text", Some("highlight_me_now"))]);
    let doc_blocks =
        build_conversation_document(std::slice::from_ref(&m), 80, &DiagramIndex::default());
    let mut state = ConversationViewState::default();
    state.open(
        "s1".into(),
        "t".into(),
        opencode_multiplexer::app::focus::AppFocus::Sidebar,
    );
    state.replace_document(doc_blocks, 50);
    state.start_search();
    state.search_insert_str("highlight_me", 50);
    let (line_idx, _, _) = state.matches()[0];

    // Re-build doc to get the line to highlight
    let doc_lines = conversation::build_document(&[m], 80);
    let highlighted = highlight_search_matches(
        std::slice::from_ref(&doc_lines[line_idx]),
        line_idx, // scroll_offset so abs idx matches
        state.matches(),
        0,
    );

    assert!(flat(&highlighted[0]).starts_with(GUTTER));
    assert!(highlighted[0].spans.iter().any(|s| {
        s.content.contains("highlight_me")
            && (s.style.bg == Some(Color::Yellow) || s.style.bg == Some(Color::Rgb(255, 150, 0)))
    }));
}

#[test]
fn unordered_list_renders_bullets_not_numbers() {
    let m = msg("assistant", vec![part("text", Some("- apples\n- oranges"))]);
    let texts = flats(&conversation::build_document(&[m], 80));
    assert!(
        texts.iter().any(|t| t.contains("• apples")),
        "expected bullet for first item: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("• oranges")),
        "expected bullet for second item: {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t.contains("1.") || t.contains("2.")),
        "unordered list should not be numbered: {texts:?}"
    );
}

#[test]
fn ordered_list_still_numbered() {
    let m = msg("assistant", vec![part("text", Some("1. one\n2. two"))]);
    let texts = flats(&conversation::build_document(&[m], 80));
    assert!(texts.iter().any(|t| t.contains("1. one")), "{texts:?}");
    assert!(texts.iter().any(|t| t.contains("2. two")), "{texts:?}");
}

#[test]
fn link_text_is_cyan_underlined() {
    let m = msg(
        "assistant",
        vec![part("text", Some("[click here](https://example.com)"))],
    );
    let lines = conversation::build_document(&[m], 80);
    let line = lines
        .iter()
        .find(|l| flat(l).contains("click here"))
        .unwrap();
    let link = line
        .spans
        .iter()
        .find(|s| s.content.contains("click here"))
        .unwrap();
    assert_eq!(link.style.fg, Some(conversation::link_color()));
    assert!(link.style.add_modifier.contains(Modifier::UNDERLINED));
}

#[test]
fn link_with_text_differing_from_url_appends_url() {
    let m = msg(
        "assistant",
        vec![part("text", Some("[docs](https://docs.rs/ratatui)"))],
    );
    let texts = flats(&conversation::build_document(&[m], 80));
    let line = texts.iter().find(|t| t.contains("docs")).unwrap();
    assert!(
        line.contains("https://docs.rs/ratatui"),
        "URL should be appended: {line:?}"
    );
}

#[test]
fn link_where_text_equals_url_shows_url_once() {
    let m = msg(
        "assistant",
        vec![part("text", Some("[https://x.test](https://x.test)"))],
    );
    let texts = flats(&conversation::build_document(&[m], 80));
    let line = texts.iter().find(|t| t.contains("https://x.test")).unwrap();
    assert_eq!(
        line.matches("https://x.test").count(),
        1,
        "URL should appear exactly once: {line:?}"
    );
}

#[test]
fn blockquote_has_marker_and_muted_style() {
    let m = msg("assistant", vec![part("text", Some("> careful here"))]);
    let lines = conversation::build_document(&[m], 80);
    let line = lines
        .iter()
        .find(|l| flat(l).contains("careful here"))
        .unwrap();
    assert!(
        flat(line).starts_with(&format!("{GUTTER}▏ ")),
        "blockquote should have a thin marker: {:?}",
        flat(line)
    );
    let content = line
        .spans
        .iter()
        .find(|s| s.content.contains("careful here"))
        .unwrap();
    assert_eq!(content.style.fg, Some(conversation::meta_color()));
}

#[test]
fn gutter_glyph_is_solid_block() {
    assert_eq!(GUTTER, "▌ ", "gutter glyph must stay a solid block");
}

#[test]
fn messages_separated_by_faint_rule() {
    let messages = vec![
        msg("user", vec![part("text", Some("first"))]),
        msg("assistant", vec![part("text", Some("second"))]),
    ];
    let lines = conversation::build_document(&messages, 80);
    let texts = flats(&lines);
    let i_asst = texts.iter().position(|t| t.contains("assistant")).unwrap();
    let rule = &texts[i_asst - 1];
    assert!(
        rule.chars().all(|c| c == '─'),
        "separator should be a full-width rule: {rule:?}"
    );
    assert_eq!(rule.chars().count(), 80, "rule should span full width");
    assert!(!rule.starts_with(GUTTER), "rule should have no gutter");
    let rule_line = &lines[i_asst - 1];
    assert!(
        rule_line
            .spans
            .iter()
            .all(|s| s.style.fg == Some(conversation::meta_color())),
        "rule should be meta-colored"
    );
}

#[test]
fn first_message_has_no_rule_above() {
    let m = msg("user", vec![part("text", Some("hi"))]);
    let lines = conversation::build_document(&[m], 80);
    let texts = flats(&lines);
    assert!(!texts[0].starts_with('─'), "no rule above first message");
    assert!(texts[0].contains("you"));
}

#[test]
fn code_block_lines_have_panel_background() {
    let md = "```rust\nfn main() {}\n```";
    let m = msg("assistant", vec![part("text", Some(md))]);
    let lines = conversation::build_document(&[m], 80);
    let code = lines.iter().find(|l| flat(l).contains("fn main")).unwrap();
    let content = code
        .spans
        .iter()
        .find(|s| s.content.contains("main"))
        .unwrap();
    assert_eq!(
        content.style.bg,
        Some(conversation::panel_bg()),
        "code content should have panel background: {:?}",
        code.spans
    );
}

#[test]
fn code_block_padded_with_blank_panel_lines() {
    let md = "```rust\nfn main() {}\n```";
    let m = msg("assistant", vec![part("text", Some(md))]);
    let lines = conversation::build_document(&[m], 80);
    let panel_blank = lines
        .iter()
        .filter(|l| {
            l.spans
                .iter()
                .any(|s| s.style.bg == Some(conversation::panel_bg()))
        })
        .filter(|l| !flat(l).contains("fn main"))
        .count();
    assert!(
        panel_blank >= 2,
        "expected padding panel rows before and after code, got {panel_blank}"
    );
}

#[test]
fn code_block_unknown_lang_still_paneled() {
    let md = "```\nraw text here\n```";
    let m = msg("assistant", vec![part("text", Some(md))]);
    let lines = conversation::build_document(&[m], 80);
    let code = lines
        .iter()
        .find(|l| flat(l).contains("raw text here"))
        .unwrap();
    let content = code
        .spans
        .iter()
        .find(|s| s.content.contains("raw text here"))
        .unwrap();
    assert_eq!(content.style.bg, Some(conversation::panel_bg()));
}

#[test]
fn reasoning_renders_markdown_emphasis_not_raw_asterisks() {
    let m = msg(
        "assistant",
        vec![part("reasoning", Some("**Summarizing test files**"))],
    );
    let lines = conversation::build_document(&[m], 80);
    let line = lines
        .iter()
        .find(|l| flat(l).contains("Summarizing test files"))
        .unwrap();
    assert!(
        !flat(line).contains("**"),
        "reasoning should not show raw asterisks: {:?}",
        flat(line)
    );
    let strong = line
        .spans
        .iter()
        .find(|s| s.content.contains("Summarizing test files"))
        .unwrap();
    assert!(strong.style.add_modifier.contains(Modifier::BOLD));
    assert_eq!(strong.style.fg, Some(Color::Gray));
}

#[test]
fn reasoning_forces_gray_on_unstyled_spans() {
    let m = msg(
        "assistant",
        vec![part("reasoning", Some("plain `code` and bold"))],
    );
    let lines = conversation::build_document(&[m], 80);
    let line = lines.iter().find(|l| flat(l).contains("plain")).unwrap();
    let plain = line
        .spans
        .iter()
        .find(|s| s.content.contains("plain"))
        .unwrap();
    assert_eq!(
        plain.style.fg,
        Some(Color::Gray),
        "plain reasoning text must be gray"
    );
}

#[test]
fn reasoning_first_line_prefixed_continuation_indented() {
    let m = msg(
        "assistant",
        vec![part(
            "reasoning",
            Some("first line\nsecond line\nthird line"),
        )],
    );
    let lines = conversation::build_document(&[m], 80);
    let texts = flats(&lines);
    let i_first = texts.iter().position(|t| t.contains("first line")).unwrap();
    let i_second = texts
        .iter()
        .position(|t| t.contains("second line"))
        .unwrap();
    assert!(
        texts[i_first].contains(REASONING_PREFIX),
        "first reasoning line should be prefixed: {:?}",
        texts[i_first]
    );
    assert!(
        !texts[i_second].contains(REASONING_PREFIX),
        "continuation lines should not repeat the prefix: {:?}",
        texts[i_second]
    );
}

#[test]
fn inline_code_uses_code_accent_green() {
    let m = msg("assistant", vec![part("text", Some("use `No` here"))]);
    let lines = conversation::build_document(&[m], 80);
    let line = lines.iter().find(|l| flat(l).contains("No")).unwrap();
    let code = line.spans.iter().find(|s| s.content == "No").unwrap();
    assert_eq!(code.style.fg, Some(conversation::code_accent()));
}

#[test]
fn strong_text_is_white_and_bold() {
    let m = msg(
        "assistant",
        vec![part("text", Some("this is **critical** here"))],
    );
    let lines = conversation::build_document(&[m], 80);
    let line = lines.iter().find(|l| flat(l).contains("critical")).unwrap();
    let strong = line
        .spans
        .iter()
        .find(|s| s.content.contains("critical"))
        .unwrap();
    assert_eq!(strong.style.fg, Some(conversation::strong_color()));
    assert!(strong.style.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn list_markers_are_dim() {
    let m = msg(
        "assistant",
        vec![part(
            "text",
            Some("- apples\n- oranges\n\n1. first\n2. second"),
        )],
    );
    let lines = conversation::build_document(&[m], 80);
    let bullet = lines.iter().find(|l| flat(l).contains("•")).unwrap();
    let bullet_marker = bullet
        .spans
        .iter()
        .find(|s| s.content.contains('•'))
        .unwrap();
    assert_eq!(
        bullet_marker.style.fg,
        Some(conversation::meta_color()),
        "bullet marker should be dim"
    );
    let numbered = lines.iter().find(|l| flat(l).contains("1.")).unwrap();
    let num_marker = numbered
        .spans
        .iter()
        .find(|s| s.content.contains('1'))
        .unwrap();
    assert_eq!(num_marker.style.fg, Some(conversation::meta_color()));
}

#[test]
fn h3_headings_bold_neutral_not_accent() {
    let m = msg("assistant", vec![part("text", Some("### Deep"))]);
    let lines = conversation::build_document(&[m], 80);
    let line = lines.iter().find(|l| flat(l).contains("Deep")).unwrap();
    let span = line
        .spans
        .iter()
        .find(|s| s.content.contains("Deep"))
        .unwrap();
    assert_eq!(
        span.style.fg,
        Some(conversation::strong_color()),
        "h3+ should be neutral white, not accent"
    );
    assert!(span.style.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn h1_h2_still_accent() {
    let m = msg("assistant", vec![part("text", Some("# Title\n\n## Sub"))]);
    let lines = conversation::build_document(&[m], 80);
    for needle in ["Title", "Sub"] {
        let line = lines.iter().find(|l| flat(l).contains(needle)).unwrap();
        let span = line
            .spans
            .iter()
            .find(|s| s.content.contains(needle))
            .unwrap();
        assert_eq!(
            span.style.fg,
            Some(conversation::link_color()),
            "h1/h2 should stay accent-colored"
        );
        assert!(span.style.add_modifier.contains(Modifier::BOLD));
    }
}

#[test]
fn no_consecutive_blank_lines_in_document() {
    let md = "para one\n\n\n\npara two\n\n```rust\nlet x = 1;\n```\n\nafter";
    let m = msg("assistant", vec![part("text", Some(md))]);
    let texts = flats(&conversation::build_document(&[m], 80));
    let is_blank = |t: &str| t.is_empty() || t == GUTTER;
    let mut prev_blank = false;
    for t in &texts {
        let b = is_blank(t);
        assert!(!(prev_blank && b), "consecutive blank lines: {texts:?}");
        prev_blank = b;
    }
}

#[test]
fn exactly_one_blank_between_paragraph_and_code() {
    let md = "before\n```rust\nlet x = 1;\n```\nafter";
    let m = msg("assistant", vec![part("text", Some(md))]);
    let texts = flats(&conversation::build_document(&[m], 80));
    let i_before = texts.iter().position(|t| t.contains("before")).unwrap();
    let i_code = texts.iter().position(|t| t.contains("let x")).unwrap();
    let i_after = texts.iter().position(|t| t.contains("after")).unwrap();
    let between_before_code = texts[i_before..i_code]
        .iter()
        .filter(|t| t.is_empty())
        .count();
    let between_code_after = texts[i_code..i_after]
        .iter()
        .filter(|t| t.is_empty())
        .count();
    assert_eq!(
        between_before_code, 1,
        "one blank between paragraph and code"
    );
    assert_eq!(
        between_code_after, 1,
        "one blank between code and paragraph"
    );
}

#[test]
fn table_columns_align_across_header_rule_and_body() {
    let md = "| Alpha | Beta |\n|-------|------|\n| aaa   | 1    |\n| long name | 2 |";
    let m = msg("assistant", vec![part("text", Some(md))]);
    let lines = conversation::build_document(&[m], 80);
    let rows: Vec<Vec<char>> = lines.iter().map(|l| flat(l).chars().collect()).collect();
    let is_sep = |c: char| c == '│' || c == '┼';
    let seps = |row: &[char]| {
        row.iter()
            .enumerate()
            .filter(|(_, c)| is_sep(**c))
            .map(|(i, _)| i)
            .collect::<Vec<_>>()
    };
    let i_head = rows
        .iter()
        .position(|r| {
            r.windows(5)
                .any(|w| w.iter().collect::<String>() == "Alpha")
        })
        .expect("header row");
    let head = seps(&rows[i_head]);
    assert!(head.len() >= 3, "expected left/mid/right separators");
    for row in &rows[i_head + 1..] {
        let text: String = row.iter().collect();
        if text.trim().is_empty() {
            continue;
        }
        assert_eq!(seps(row), head, "separator columns must align: {text:?}");
    }
}

#[test]
fn table_rule_row_after_header_is_meta() {
    let md = "| Alpha | Beta |\n|-------|------|\n| 1 | 2 |";
    let m = msg("assistant", vec![part("text", Some(md))]);
    let lines = conversation::build_document(&[m], 80);
    let texts = flats(&lines);
    let i_head = texts
        .iter()
        .position(|t| t.contains("Alpha"))
        .expect("header row");
    let rule = &texts[i_head + 1];
    assert!(
        rule.contains('┼'),
        "rule row should mark column junctions: {rule:?}"
    );
    assert!(
        lines[i_head + 1]
            .spans
            .iter()
            .any(|s| s.style.fg == Some(conversation::meta_color())),
        "rule row should be meta-colored"
    );
}

#[test]
fn table_has_left_edge_border() {
    let md = "| Alpha | Beta |\n|-------|------|\n| 1 | 2 |";
    let m = msg("assistant", vec![part("text", Some(md))]);
    let lines = conversation::build_document(&[m], 80);
    let bordered = lines
        .iter()
        .filter(|l| flat(l).starts_with(&format!("{GUTTER}│")))
        .count();
    assert!(
        bordered >= 3,
        "header, rule, and body rows should all have a left border"
    );
    let header = lines.iter().find(|l| flat(l).contains("Alpha")).unwrap();
    assert!(flat(header).starts_with(&format!("{GUTTER}│")));
}

#[test]
fn table_truncates_wide_cells_with_ellipsis() {
    let long = "x".repeat(100);
    let md = format!("| {long} | y |\n|---|--|\n| z | w |");
    let m = msg("assistant", vec![part("text", Some(&md))]);
    let width = 60u16;
    let lines = conversation::build_document(&[m], width);
    let header = lines
        .iter()
        .find(|l| flat(l).contains("…"))
        .expect("wide header cell should be truncated");
    assert!(
        header.width() <= width as usize,
        "truncated table row exceeds width: {}",
        header.width()
    );
}

#[test]
fn table_inline_code_keeps_accent() {
    let md = "| Option |\n|--------|\n| `Yes`  |";
    let m = msg("assistant", vec![part("text", Some(md))]);
    let lines = conversation::build_document(&[m], 80);
    let body = lines.iter().find(|l| flat(l).contains("Yes")).unwrap();
    let code = body.spans.iter().find(|s| s.content == "Yes").unwrap();
    assert_eq!(code.style.fg, Some(conversation::code_accent()));
}

#[test]
fn narrow_width_table_does_not_panic() {
    let md = "| Alpha | Beta |\n|-------|------|\n| 1 | 2 |";
    let m = msg("assistant", vec![part("text", Some(md))]);
    for w in [10u16, 16] {
        let _ = conversation::build_document(std::slice::from_ref(&m), w);
    }
}

#[test]
fn document_records_header_block_index_and_role_per_message() {
    let messages = vec![
        msg("user", vec![part("text", Some("u"))]),
        msg("assistant", vec![part("text", Some("a"))]),
        msg("system", vec![part("text", Some("s"))]),
    ];
    let doc = build_conversation_document(&messages, 80, &DiagramIndex::default());
    let headers = doc.headers();
    assert_eq!(headers.len(), 3);
    assert_eq!(headers[0].role, MessageRole::User);
    assert_eq!(headers[1].role, MessageRole::Assistant);
    assert_eq!(headers[2].role, MessageRole::Other);
    assert!(headers[0].block_idx < headers[1].block_idx);
    assert!(headers[1].block_idx < headers[2].block_idx);
}

#[test]
fn header_rows_resolve_through_diagram_height_changes() {
    let md = "```mermaid\ngraph TD\nA-->B\n```";
    let m = msg("assistant", vec![part("text", Some(md))]);
    let u = msg("user", vec![part("text", Some("next"))]);
    let mut doc = build_conversation_document(&[m, u], 80, &DiagramIndex::default());
    assert_eq!(doc.headers().len(), 2);
    let before = doc.header_row(1).expect("user header row");
    // Grow the diagram placeholder the way a finished render would.
    let diagram = doc
        .blocks
        .iter_mut()
        .find_map(|b| match b {
            DocBlock::Diagram(slot) => Some(slot),
            _ => None,
        })
        .unwrap();
    diagram.row_height = 40;
    let after = doc.header_row(1).expect("user header row after growth");
    assert!(
        after > before,
        "user header row must shift when a diagram above it grows"
    );
}

#[test]
fn next_user_header_scrolls_to_next_user_message() {
    let tall = |role: &str| {
        msg(
            role,
            vec![part(
                "text",
                Some(
                    &(0..30)
                        .map(|i| format!("line {i}"))
                        .collect::<Vec<_>>()
                        .join("\n"),
                ),
            )],
        )
    };
    let messages = vec![
        tall("assistant"),
        tall("user"),
        tall("assistant"),
        tall("user"),
    ];
    let doc = build_conversation_document(&messages, 80, &DiagramIndex::default());
    let r1 = doc.header_row(1).unwrap();
    let r2 = doc.header_row(3).unwrap();
    assert!(r1 > 0 && r2 > r1, "expected headers deep in a tall doc");
    let mut state = ConversationViewState::default();
    state.open(
        "s1".into(),
        "t".into(),
        opencode_multiplexer::app::focus::AppFocus::Sidebar,
    );
    state.replace_document(doc, 10);
    state.scroll_to_top();
    state.next_user_header(10);
    assert_eq!(state.scroll_offset(), r1, "first jump lands on u1");
    state.next_user_header(10);
    assert_eq!(state.scroll_offset(), r2, "second jump lands on u2");
    assert!(!state.is_following_tail());
}

#[test]
fn prev_user_header_scrolls_to_previous_user_message() {
    let tall = |role: &str| {
        msg(
            role,
            vec![part(
                "text",
                Some(
                    &(0..30)
                        .map(|i| format!("line {i}"))
                        .collect::<Vec<_>>()
                        .join("\n"),
                ),
            )],
        )
    };
    let messages = vec![
        tall("assistant"),
        tall("user"),
        tall("assistant"),
        tall("user"),
    ];
    let doc = build_conversation_document(&messages, 80, &DiagramIndex::default());
    let r2 = doc.header_row(3).unwrap();
    let mut state = ConversationViewState::default();
    state.open(
        "s1".into(),
        "t".into(),
        opencode_multiplexer::app::focus::AppFocus::Sidebar,
    );
    state.replace_document(doc, 10);
    state.scroll_to_end(10);
    state.prev_user_header(10);
    assert_eq!(
        state.scroll_offset(),
        r2,
        "prev jump lands on last user message"
    );
}

#[test]
fn jump_when_already_on_header_goes_to_next_one() {
    let tall = |role: &str| {
        msg(
            role,
            vec![part(
                "text",
                Some(
                    &(0..30)
                        .map(|i| format!("line {i}"))
                        .collect::<Vec<_>>()
                        .join("\n"),
                ),
            )],
        )
    };
    let messages = vec![
        tall("assistant"),
        tall("user"),
        tall("assistant"),
        tall("user"),
    ];
    let doc = build_conversation_document(&messages, 80, &DiagramIndex::default());
    let r1 = doc.header_row(1).unwrap();
    let r2 = doc.header_row(3).unwrap();
    let mut state = ConversationViewState::default();
    state.open(
        "s1".into(),
        "t".into(),
        opencode_multiplexer::app::focus::AppFocus::Sidebar,
    );
    state.replace_document(doc, 10);
    state.scroll_to_top();
    state.next_user_header(10); // lands on u1 header
    assert_eq!(state.scroll_offset(), r1);
    state.next_user_header(10); // already on a header -> advance
    assert_eq!(state.scroll_offset(), r2);
}

#[test]
fn jump_with_no_user_messages_is_noop() {
    let m = msg("assistant", vec![part("text", Some("only"))]);
    let doc = build_conversation_document(&[m], 80, &DiagramIndex::default());
    let mut state = ConversationViewState::default();
    state.open(
        "s1".into(),
        "t".into(),
        opencode_multiplexer::app::focus::AppFocus::Sidebar,
    );
    state.replace_document(doc, 50);
    state.scroll_to_top();
    state.next_user_header(50);
    state.prev_user_header(50);
    assert_eq!(state.scroll_offset(), 0);
}

#[test]
fn status_line_shows_percentage_and_position() {
    let line = conversation::conversation_status_line(300, 1000, false, None);
    let t: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(t.contains("30%"), "expected 30% in {t:?}");
    assert!(t.contains("300/1000"), "expected position in {t:?}");
}

#[test]
fn status_line_shows_live_when_following_tail() {
    let live = conversation::conversation_status_line(995, 1000, true, None);
    let t: String = live.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(t.contains("LIVE"), "expected LIVE marker in {t:?}");

    let scrolled = conversation::conversation_status_line(300, 1000, false, None);
    let t2: String = scrolled.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(
        !t2.contains("LIVE"),
        "no LIVE marker when scrolled up: {t2:?}"
    );
}

#[test]
fn status_line_includes_match_count_during_search() {
    let line = conversation::conversation_status_line(10, 100, true, Some((2, 7)));
    let t: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(t.contains("2/7"), "expected match count in {t:?}");
}

#[test]
fn status_line_zero_rows_no_panic() {
    let line = conversation::conversation_status_line(0, 0, true, None);
    let _ = line;
}
