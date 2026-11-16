use opencode_multiplexer::data::db::models::{DbConversationMessage, DbConversationPart};
use opencode_multiplexer::ui::conversation::diagram::slot::DiagramIndex;
use opencode_multiplexer::ui::conversation::document::ConversationDocument;
use opencode_multiplexer::ui::conversation::selection::{
    DocPos, Selection, cell_to_col, col_to_cell, extract_text, hit_test, resolve_anchor,
};
use opencode_multiplexer::ui::conversation::{GUTTER, build_conversation_document};

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

fn msg(id: &str, role: &str, parts: Vec<DbConversationPart>) -> DbConversationMessage {
    DbConversationMessage {
        id: id.into(),
        role: role.into(),
        time_created: 1_700_000_000_000,
        completed: Some(1_700_000_001_000),
        model_id: None,
        agent: None,
        parts,
    }
}

fn doc_of(messages: &[DbConversationMessage], width: u16) -> ConversationDocument {
    build_conversation_document(messages, width, &DiagramIndex::default())
}

/// Locate the row holding `text` and return it with its absolute index.
fn row_with<'a>(doc: &'a ConversationDocument, text: &str) -> (usize, &'a TextRow) {
    doc.rows()
        .find_map(|(abs, row)| row.filter(|r| r.text == text).map(|r| (abs, r)))
        .unwrap_or_else(|| panic!("no row with text {text:?}"))
}

/// Select the whole document from the first row to the very end.
fn select_all(doc: &ConversationDocument) -> Selection {
    let last = doc.total_rows().saturating_sub(1);
    Selection::new(DocPos { row: 0, col: 0 }).with_head(DocPos {
        row: last,
        col: usize::MAX,
    })
}

use opencode_multiplexer::ui::conversation::document::TextRow;

#[test]
fn soft_wrapped_paragraph_copies_as_one_unbroken_line() {
    let sentence = "the quick brown fox jumps over the lazy dog while counting every single star";
    let doc = doc_of(&[msg("m1", "user", vec![part("text", Some(sentence))])], 30);

    // The paragraph must actually wrap, otherwise this test proves nothing.
    assert!(
        doc.total_rows() > 3,
        "expected wrapping, got {} rows",
        doc.total_rows()
    );

    let copied = extract_text(&doc, select_all(&doc));
    assert_eq!(copied.trim(), sentence);
    assert!(
        !copied.trim().contains('\n'),
        "soft wraps must not become newlines: {copied:?}"
    );
}

#[test]
fn hard_line_breaks_are_preserved() {
    let text = "first line\nsecond line\nthird line";
    let doc = doc_of(&[msg("m1", "user", vec![part("text", Some(text))])], 80);
    let copied = extract_text(&doc, select_all(&doc));
    assert!(
        copied.contains("first line\nsecond line\nthird line"),
        "{copied:?}"
    );
}

#[test]
fn separate_paragraphs_keep_a_blank_line_between_them() {
    let text = "alpha paragraph\n\nbeta paragraph";
    let doc = doc_of(&[msg("m1", "user", vec![part("text", Some(text))])], 80);
    let copied = extract_text(&doc, select_all(&doc));
    assert!(
        copied.contains("alpha paragraph\n\nbeta paragraph"),
        "{copied:?}"
    );
}

#[test]
fn headers_rules_and_gutters_are_not_copied() {
    let doc = doc_of(
        &[
            msg("m1", "user", vec![part("text", Some("hello there"))]),
            msg(
                "m2",
                "assistant",
                vec![part("text", Some("general kenobi"))],
            ),
        ],
        80,
    );

    let copied = extract_text(&doc, select_all(&doc));
    assert!(!copied.contains(GUTTER.trim()), "gutter leaked: {copied:?}");
    assert!(!copied.contains('─'), "separator rule leaked: {copied:?}");
    assert!(!copied.contains('●'), "role header leaked: {copied:?}");
    assert!(copied.contains("hello there"), "{copied:?}");
    assert!(copied.contains("general kenobi"), "{copied:?}");
}

#[test]
fn selecting_across_messages_separates_them_with_a_blank_line() {
    let doc = doc_of(
        &[
            msg("m1", "user", vec![part("text", Some("question"))]),
            msg("m2", "assistant", vec![part("text", Some("answer"))]),
        ],
        80,
    );

    let copied = extract_text(&doc, select_all(&doc));
    assert_eq!(copied.trim(), "question\n\nanswer");
}

#[test]
fn code_block_keeps_its_indentation_and_line_breaks() {
    let text = "intro\n\n```\nfn main() {\n    run();\n}\n```\n\noutro";
    let doc = doc_of(&[msg("m1", "user", vec![part("text", Some(text))])], 60);

    let copied = extract_text(&doc, select_all(&doc));
    assert!(copied.contains("fn main() {"), "{copied:?}");
    assert!(copied.contains("    run();"), "indent lost: {copied:?}");
    assert!(
        copied.contains("fn main() {\n    run();\n}"),
        "line breaks lost: {copied:?}"
    );
}

#[test]
fn partial_selection_uses_character_columns_not_display_cells() {
    let doc = doc_of(
        &[msg("m1", "user", vec![part("text", Some("hello world"))])],
        80,
    );
    let (abs, _) = row_with(&doc, "hello world");
    let sel = Selection::new(DocPos { row: abs, col: 6 }).with_head(DocPos { row: abs, col: 11 });
    assert_eq!(extract_text(&doc, sel), "world");
}

#[test]
fn wide_characters_map_between_display_cells_and_columns() {
    // "日本語" is three chars, each two display cells wide.
    assert_eq!(cell_to_col("日本語", 0, 0), 0);
    assert_eq!(
        cell_to_col("日本語", 0, 1),
        0,
        "mid-glyph snaps to char start"
    );
    assert_eq!(cell_to_col("日本語", 0, 2), 1);
    assert_eq!(cell_to_col("日本語", 0, 5), 2);
    assert_eq!(cell_to_col("日本語", 0, 99), 3);

    assert_eq!(col_to_cell("日本語", 0, 0), 0);
    assert_eq!(col_to_cell("日本語", 0, 1), 2);
    assert_eq!(col_to_cell("日本語", 0, 3), 6);
    // Offset past the row's decoration start.
    assert_eq!(col_to_cell("ab", 2, 1), 3);
}

#[test]
fn hit_test_skips_leading_decoration_cells() {
    // Body rows are prefixed with a two-cell gutter; clicking inside it snaps
    // to the start of the readable text.
    let doc = doc_of(
        &[msg("m1", "user", vec![part("text", Some("hello world"))])],
        80,
    );
    let (abs, row) = row_with(&doc, "hello world");
    assert!(row.text_col > 0, "expected a gutter on the body row");

    assert_eq!(
        hit_test(&doc, abs, row.text_col),
        Some(DocPos { row: abs, col: 0 })
    );
    // Click one cell into "hello" (after the gutter) → col 1.
    assert_eq!(
        hit_test(&doc, abs, row.text_col + 1),
        Some(DocPos { row: abs, col: 1 })
    );
    // Clicking the gutter itself clamps to the first character.
    assert_eq!(hit_test(&doc, abs, 0), Some(DocPos { row: abs, col: 0 }));
}

#[test]
fn anchors_survive_a_document_rebuild_after_appending_a_message() {
    let first = doc_of(
        &[msg(
            "m1",
            "user",
            vec![part("text", Some("alpha beta gamma"))],
        )],
        80,
    );
    let anchor = first
        .rows()
        .find_map(|(_, row)| {
            row.filter(|r| r.text == "alpha beta gamma")
                .map(|r| r.anchor())
        })
        .expect("body row present");

    let second = doc_of(
        &[
            msg("m1", "user", vec![part("text", Some("alpha beta gamma"))]),
            msg("m2", "assistant", vec![part("text", Some("later reply"))]),
        ],
        80,
    );
    let resolved = resolve_anchor(&second, &anchor).expect("anchor resolves");
    assert_eq!(
        second.row_text(resolved.row),
        Some("alpha beta gamma"),
        "anchor landed on the wrong row"
    );
    assert_eq!(resolved.col, 0);
}

#[test]
fn anchors_survive_a_width_change() {
    let text = "alpha beta gamma delta";
    let wide = doc_of(&[msg("m1", "user", vec![part("text", Some(text))])], 80);
    let anchor = wide
        .rows()
        .find_map(|(_, row)| row.filter(|r| r.text == text).map(|r| r.anchor()))
        .expect("body row present");

    let narrow = doc_of(&[msg("m1", "user", vec![part("text", Some(text))])], 20);
    assert!(
        narrow.total_rows() > wide.total_rows(),
        "expected reflow: {} vs {}",
        narrow.total_rows(),
        wide.total_rows()
    );

    let resolved = resolve_anchor(&narrow, &anchor).expect("anchor resolves after reflow");
    assert_eq!(resolved.col, 0);
    assert!(
        narrow
            .row_text(resolved.row)
            .is_some_and(|t| t.starts_with("alpha")),
        "anchor landed on {:?}",
        narrow.row_text(resolved.row)
    );
}
