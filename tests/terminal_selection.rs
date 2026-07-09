use opencode_multiplexer::terminal::{
    renderer::TerminalWidget,
    selection::{SelectionPoint, SelectionRange, TerminalSelection, extract_selection_text},
    surface::TerminalSurface,
};
use ratatui::{Terminal, backend::TestBackend, layout::Rect, style::Color};

#[test]
fn selection_extracts_multiline_text_from_surface() {
    let mut surface = TerminalSurface::new(3, 10);
    surface.process(b"line one  \r\nline two  \r\nline three");

    let snapshot = surface.snapshot();

    // Select from "one" to "two"
    // "line one" (row 0, col 5..7)
    // "line two" (row 1, col 0..7)
    let range = SelectionRange::new(
        SelectionPoint { row: 0, col: 5 },
        SelectionPoint { row: 1, col: 7 },
    );

    let ranges_wrapped = surface.wrapped_rows();
    let extracted = extract_selection_text(&snapshot, range, &ranges_wrapped);
    // Row 0: "one" (trimmed)
    // Row 1: "line two" (trimmed)
    assert_eq!(extracted, "one\nline two");
}

#[test]
fn reverse_drag_extracts_same_text() {
    let mut surface = TerminalSurface::new(2, 10);
    surface.process(b"hello\r\nworld");
    let snapshot = surface.snapshot();

    let range_fwd = SelectionRange::new(
        SelectionPoint { row: 0, col: 0 },
        SelectionPoint { row: 1, col: 4 },
    );
    let range_rev = SelectionRange::new(
        SelectionPoint { row: 1, col: 4 },
        SelectionPoint { row: 0, col: 0 },
    );

    assert_eq!(range_fwd, range_rev);
    assert_eq!(
        extract_selection_text(&snapshot, range_fwd, &surface.wrapped_rows()),
        "hello\nworld"
    );
}

#[test]
fn wrapped_text_extracts_as_single_line() {
    let mut surface = TerminalSurface::new(3, 10);
    // "1234567890123456789012345"
    // row 0: 1234567890 (wrap)
    // row 1: 1234567890 (wrap)
    // row 2: 12345       (no wrap)
    surface.process(b"1234567890123456789012345");
    let snapshot = surface.snapshot();
    let wrapped = surface.wrapped_rows();

    let range = SelectionRange::new(
        SelectionPoint { row: 0, col: 0 },
        SelectionPoint { row: 2, col: 4 },
    );

    let extracted = extract_selection_text(&snapshot, range, &wrapped);
    assert_eq!(extracted, "1234567890123456789012345");
}

#[test]
fn selection_highlight_renders_to_buffer() {
    let mut surface = TerminalSurface::new(2, 5);
    surface.process(b"abcde\r\nfghij");

    let range = SelectionRange::new(
        SelectionPoint { row: 0, col: 1 }, // 'b'
        SelectionPoint { row: 1, col: 1 }, // 'g'
    );

    let backend = TestBackend::new(5, 2);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|frame| {
            let widget = TerminalWidget::new(&surface).with_selection(Some(range));
            frame.render_widget(widget, Rect::new(0, 0, 5, 2));
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let highlight_bg = Color::Rgb(50, 50, 80);

    // Row 0: a (no), b (yes), c (yes), d (yes), e (yes)
    assert_ne!(buffer[(0, 0)].bg, highlight_bg);
    assert_eq!(buffer[(1, 0)].bg, highlight_bg);
    assert_eq!(buffer[(2, 0)].bg, highlight_bg);
    assert_eq!(buffer[(3, 0)].bg, highlight_bg);
    assert_eq!(buffer[(4, 0)].bg, highlight_bg);

    // Row 1: f (yes), g (yes), h (no), i (no), j (no)
    assert_eq!(buffer[(0, 1)].bg, highlight_bg);
    assert_eq!(buffer[(1, 1)].bg, highlight_bg);
    assert_ne!(buffer[(2, 1)].bg, highlight_bg);
}

#[test]
fn selection_clear_removes_highlight() {
    let mut surface = TerminalSurface::new(1, 5);
    surface.process(b"abcde");

    let mut sel = TerminalSelection::default();
    sel.begin(SelectionPoint { row: 0, col: 0 });
    sel.update(SelectionPoint { row: 0, col: 2 });

    let backend = TestBackend::new(5, 1);
    let mut terminal = Terminal::new(backend).unwrap();
    let highlight_bg = Color::Rgb(50, 50, 80);

    // Draw with selection
    terminal
        .draw(|frame| {
            let widget = TerminalWidget::new(&surface).with_selection(sel.range());
            frame.render_widget(widget, Rect::new(0, 0, 5, 1));
        })
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(0, 0)].bg, highlight_bg);

    // Clear and draw
    sel.clear();
    terminal
        .draw(|frame| {
            let widget = TerminalWidget::new(&surface).with_selection(sel.range());
            frame.render_widget(widget, Rect::new(0, 0, 5, 1));
        })
        .unwrap();
    assert_ne!(terminal.backend().buffer()[(0, 0)].bg, highlight_bg);
}
