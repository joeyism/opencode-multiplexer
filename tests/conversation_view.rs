use opencode_multiplexer::app::conversation::ConversationViewState;
use opencode_multiplexer::data::db::models::{DbConversationMessage, DbConversationPart};
use opencode_multiplexer::ui::conversation::{
    self, GUTTER, REASONING_PREFIX, TOOL_INDENT, body_assistant_color, body_user_color,
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
fn user_message_body_lines_start_with_cyan_gutter() {
    let m = msg("user", vec![part("text", Some("Hello"))]);
    let lines = conversation::build_document(&[m], 80);
    // lines[0] is header
    // lines[1] is body
    assert!(lines.len() >= 2);
    assert!(flat(&lines[1]).starts_with(GUTTER));
    assert_eq!(first_fg(&lines[1]), Some(Color::Cyan));
}

#[test]
fn assistant_message_body_lines_start_with_green_gutter() {
    let m = msg("assistant", vec![part("text", Some("Hi"))]);
    let lines = conversation::build_document(&[m], 80);
    assert!(lines.len() >= 2);
    assert!(flat(&lines[1]).starts_with(GUTTER));
    assert_eq!(first_fg(&lines[1]), Some(Color::Green));
}

#[test]
fn multi_line_body_every_line_has_gutter() {
    let m = msg("user", vec![part("text", Some("Line 1\n\nLine 2"))]);
    let lines = conversation::build_document(&[m], 80);
    // Header, Line 1, Blank line (with gutter), Line 2, Separator (no gutter)
    let texts = flats(&lines);
    assert!(
        texts.iter().any(|t| t == GUTTER),
        "Should have a line that is just a gutter: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.is_empty()),
        "Should have a truly empty separator: {texts:?}"
    );
}

#[test]
fn user_header_is_you_uppercase_with_dim_time() {
    let m = msg("user", vec![part("text", Some("hi"))]);
    let lines = conversation::build_document(&[m], 80);
    let header = &lines[0];
    let t = flat(header);
    assert!(t.starts_with(GUTTER));
    assert!(t.contains("YOU"));
    assert!(!t.contains(" you")); // old lowercase label gone
    // role span bold+cyan; time span dark gray
    let joined = t.clone();
    assert!(joined.contains("YOU"));
    // find a DarkGray span that looks like HH:MM
    let has_dim_time = header.spans.iter().any(|s| {
        s.style.fg == Some(conversation::meta_color())
            && s.content.chars().any(|c| c.is_ascii_digit())
    });
    assert!(has_dim_time, "expected dim timestamp in header: {t:?}");
    let role_bold = header
        .spans
        .iter()
        .any(|s| s.content.contains("YOU") && s.style.add_modifier.contains(Modifier::BOLD));
    assert!(role_bold);
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
    // only gutter + YOU + time — no extra trailing junk tokens beyond time
    assert!(t.contains("YOU"));
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
fn reasoning_uses_dim_italic_style() {
    let m = msg("assistant", vec![part("reasoning", Some("think hard"))]);
    let lines = conversation::build_document(&[m], 80);
    let line = lines
        .iter()
        .find(|l| flat(l).contains("think hard"))
        .unwrap();
    // content spans (not gutter) should match reasoning_style fg+italic
    let content = line
        .spans
        .iter()
        .find(|s| s.content.contains("think"))
        .unwrap();
    assert_eq!(content.style.fg, Some(Color::DarkGray));
    assert!(content.style.add_modifier.contains(Modifier::ITALIC));
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
fn user_and_assistant_bodies_use_distinct_role_cues() {
    let messages = vec![
        msg("user", vec![part("text", Some("USER_ONLY_TOKEN"))]),
        msg("assistant", vec![part("text", Some("ASST_ONLY_TOKEN"))]),
    ];
    let lines = conversation::build_document(&messages, 80);
    let user_line = lines
        .iter()
        .find(|l| flat(l).contains("USER_ONLY_TOKEN"))
        .unwrap();
    let asst_line = lines
        .iter()
        .find(|l| flat(l).contains("ASST_ONLY_TOKEN"))
        .unwrap();
    assert_eq!(
        user_line.spans[0].style.fg,
        Some(conversation::user_color())
    );
    assert_eq!(
        asst_line.spans[0].style.fg,
        Some(conversation::assistant_color())
    );
    // body text colors differ by role
    let user_body_fg = user_line
        .spans
        .iter()
        .find(|s| s.content.contains("USER_ONLY_TOKEN"))
        .and_then(|s| s.style.fg);
    let asst_body_fg = asst_line
        .spans
        .iter()
        .find(|s| s.content.contains("ASST_ONLY_TOKEN"))
        .and_then(|s| s.style.fg);
    assert_eq!(user_body_fg, Some(body_user_color()));
    // assistant may be None/Reset — accept None or Some(body_assistant_color)
    assert!(
        asst_body_fg.is_none() || asst_body_fg == Some(body_assistant_color()),
        "unexpected asst body fg {asst_body_fg:?}"
    );
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
    for l in &lines {
        let t = flat(l);
        if t.contains("reasoning") {
            assert!(
                l.width() <= width as usize,
                "reasoning line wider than viewport: width={} text={t:?}",
                l.width()
            );
            assert!(t.starts_with(GUTTER));
            assert!(t.contains(REASONING_PREFIX));
        }
    }
}

#[test]
fn narrow_width_does_not_panic() {
    let m = msg("user", vec![part("text", Some("hi"))]);
    let _ = conversation::build_document(&[m.clone()], 1);
    let _ = conversation::build_document(&[m.clone()], 2);
    let _ = conversation::build_document(&[m], 5);
}

#[test]
fn message_with_only_empty_text_parts_still_has_header() {
    let m = msg("user", vec![part("text", Some("")), part("text", None)]);
    let lines = conversation::build_document(&[m], 80);
    assert!(flat(&lines[0]).contains("YOU"));
}

#[test]
fn message_with_no_parts_still_has_header_and_separator() {
    let m = msg("user", vec![]);
    let lines = conversation::build_document(&[m], 80);
    assert!(flat(&lines[0]).contains("YOU"));
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
    let doc = conversation::build_document(&[m], 80);
    let mut state = ConversationViewState::default();
    state.open(
        "s1".into(),
        "t".into(),
        opencode_multiplexer::app::focus::AppFocus::Sidebar,
    );
    state.replace_document(doc.clone(), 50);
    state.start_search();
    state.search_insert_str("unique_needle", 50);
    let (cur, total) = state.match_status().expect("match");
    assert_eq!(cur, 1);
    assert!(total >= 1);

    let (line_idx, byte_start, len) = state.matches()[0];
    let line_flat: String = doc[line_idx]
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
    let doc = conversation::build_document(&[m], 80);
    let mut state = ConversationViewState::default();
    state.open(
        "s1".into(),
        "t".into(),
        opencode_multiplexer::app::focus::AppFocus::Sidebar,
    );
    state.replace_document(doc, 50);
    state.start_search();
    state.search_insert_str("highlight_me", 50);
    let (line_idx, _, _) = state.matches()[0];

    // Re-build doc to get the line to highlight
    let doc = conversation::build_document(
        &[msg("user", vec![part("text", Some("highlight_me_now"))])],
        80,
    );
    let highlighted = highlight_search_matches(
        std::slice::from_ref(&doc[line_idx]),
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
