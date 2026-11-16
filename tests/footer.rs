use opencode_multiplexer::ui::hints::compose_footer;
use ratatui::text::{Line, Span};

fn line(text: &str) -> Line<'static> {
    Line::from(Span::raw(text.to_string()))
}

fn flat(l: &Line<'_>) -> String {
    l.spans.iter().map(|s| s.content.as_ref()).collect()
}

#[test]
fn compose_footer_right_aligns_status() {
    let hints = line("j/k scroll");
    let status = line(" · 81% · 1301/1602");
    let width = 80u16;
    let footer = compose_footer(hints, Some(status), width);
    assert_eq!(footer.width(), width as usize);
    let t = flat(&footer);
    assert!(
        t.ends_with("1301/1602"),
        "status should hug the right edge: {t:?}"
    );
    assert!(t.contains("j/k scroll"));
}

#[test]
fn compose_footer_truncates_hints_when_narrow() {
    let hints = line("j/k scroll G end g top / search n/N next/prev");
    let status = line(" · 81%");
    let width = 40u16;
    let footer = compose_footer(hints, Some(status), width);
    assert_eq!(footer.width(), width as usize);
    let t = flat(&footer);
    assert!(
        t.contains('…'),
        "hints should be truncated with ellipsis: {t:?}"
    );
    assert!(t.contains("81%"), "status must survive truncation: {t:?}");
}

#[test]
fn compose_footer_without_status_unchanged() {
    let hints = line("j/k scroll");
    let footer = compose_footer(hints.clone(), None, 80);
    assert_eq!(flat(&footer), "j/k scroll");
}

#[test]
fn compose_footer_empty_hints_keeps_status() {
    let hints = Line::default();
    let status = line(" · 81%");
    let width = 20u16;
    let footer = compose_footer(hints, Some(status), width);
    let t = flat(&footer);
    assert!(t.contains("81%"));
    assert!(footer.width() <= width as usize);
}
