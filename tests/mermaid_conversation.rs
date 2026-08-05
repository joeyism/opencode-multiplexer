use opencode_multiplexer::app::conversation::ConversationViewState;
use opencode_multiplexer::data::db::models::{DbConversationMessage, DbConversationPart};
use opencode_multiplexer::ui::conversation::build_conversation_document;
use opencode_multiplexer::ui::conversation::diagram::cache::MermaidCache;
use opencode_multiplexer::ui::conversation::diagram::halfblock::png_to_halfblock_lines;
use opencode_multiplexer::ui::conversation::diagram::hash::hash_source;
use opencode_multiplexer::ui::conversation::diagram::layout::{DiagramLayout, compute_layout};
use opencode_multiplexer::ui::conversation::diagram::mmdc::{
    MermaidError, MermaidRenderConfig, render_with_mmdc,
};
use opencode_multiplexer::ui::conversation::diagram::scheduler::{
    DiagramScheduler, RenderFinished,
};
use opencode_multiplexer::ui::conversation::diagram::slot::{DiagramIndex, DiagramSlot};
use opencode_multiplexer::ui::conversation::document::ConversationDocument;
use ratatui::text::Line;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use opencode_multiplexer::ui::conversation::diagram::protocol::screen_rect_for_slot;
use ratatui::layout::Rect;

#[allow(dead_code)]
fn test_fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

#[allow(dead_code)]
fn tiny_png_bytes() -> Vec<u8> {
    fs::read(test_fixtures_dir().join("tiny.png")).expect("tiny.png fixture missing")
}

fn temp_cache_dir() -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("ocmux-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&p).unwrap();
    p
}

fn test_cfg() -> MermaidRenderConfig {
    let cache_dir = temp_cache_dir();
    MermaidRenderConfig {
        mmdc_path: test_fixtures_dir().join("fake_mmdc.sh"),
        cache_dir: cache_dir.clone(),
        timeout: Duration::from_secs(5),
        max_rows: 36,
        prefetch_viewports: 1,
        protocol_enabled: true,
        invocation_log: Some(cache_dir.join("invocations.log")),
    }
}

fn invocation_count(cfg: &MermaidRenderConfig) -> usize {
    let Some(path) = cfg.invocation_log.as_ref() else {
        return 0;
    };
    if !path.exists() {
        return 0;
    }
    fs::read_to_string(path).unwrap().lines().count()
}

#[allow(dead_code)]
fn msg(role: &str, parts: Vec<DbConversationPart>) -> DbConversationMessage {
    DbConversationMessage {
        id: "m".into(),
        role: role.into(),
        time_created: 1_700_000_000_000,
        completed: Some(1_700_000_001_000),
        model_id: None,
        agent: None,
        parts,
    }
}

#[allow(dead_code)]
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

#[test]
fn mermaid_hash_stable_for_same_source() {
    let a = hash_source("graph TD; A-->B");
    let b = hash_source("graph TD; A-->B");
    assert_eq!(a, b);
    assert_eq!(a.len(), 64); // hex sha256
}

#[test]
fn mermaid_hash_changes_when_source_changes() {
    let a = hash_source("graph TD; A-->B");
    let b = hash_source("graph TD; A-->C");
    assert_ne!(a, b);
}

#[test]
fn cache_put_get_roundtrip_on_real_fs() {
    let dir = temp_cache_dir();
    let cache = MermaidCache::new(dir.clone());
    let key = hash_source("graph TD; A-->B");
    let png = tiny_png_bytes();

    assert!(cache.get(&key).is_none());
    cache.put(&key, &png).unwrap();
    assert_eq!(cache.get(&key).unwrap(), png);

    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cache_miss_when_file_deleted() {
    let dir = temp_cache_dir();
    let cache = MermaidCache::new(dir.clone());
    let key = hash_source("x");
    cache.put(&key, b"png").unwrap();

    fs::remove_file(cache.path_for(&key)).unwrap();
    assert!(cache.get(&key).is_none());

    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn mmdc_writes_png_via_fake_cli() {
    let cfg = test_cfg();
    let png = render_with_mmdc("graph TD; A-->B", &cfg).unwrap();
    assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
    fs::remove_dir_all(cfg.cache_dir).unwrap();
}

#[test]
fn mmdc_failure_returns_error() {
    let cfg = test_cfg();
    let err = render_with_mmdc("FORCE_FAIL\ngraph TD; A-->B", &cfg).unwrap_err();
    assert!(!err.to_string().is_empty());
    fs::remove_dir_all(cfg.cache_dir).unwrap();
}

#[test]
fn mmdc_timeout_returns_error() {
    let mut cfg = test_cfg();
    cfg.timeout = Duration::from_millis(200);
    let err = render_with_mmdc("FORCE_SLEEP\ngraph TD; A-->B", &cfg).unwrap_err();
    assert!(matches!(err, MermaidError::Timeout(_)) || err.to_string().contains("timeout"));
    fs::remove_dir_all(cfg.cache_dir).unwrap();
}

#[test]
fn mmdc_invoked_with_temp_input_file_not_shell_interpolation() {
    let cfg = test_cfg();
    let pwn_path = cfg.cache_dir.join("SHOULD_NOT_EXIST");
    let src = format!(
        "graph TD; A-->B; message[\"$(touch {})\"]",
        pwn_path.display()
    );
    let _ = render_with_mmdc(&src, &cfg);
    assert!(!pwn_path.exists());
    fs::remove_dir_all(cfg.cache_dir).unwrap();
}

#[test]
fn layout_fits_width_in_terminal_cells() {
    let layout = compute_layout(64, 32, 32, 36);
    assert_eq!(layout.row_height, 8); // 32px * (32/64) = 16px. 16px / 2 = 8 rows.
    assert!(layout.row_height <= 36);
}

#[test]
fn layout_soft_cap_shrinks_tall_diagram() {
    let layout = compute_layout(100, 4000, 80, 36);
    assert_eq!(layout.row_height, 36);
    assert!(layout.out_height_px <= 36 * 2);
}

#[test]
fn layout_preserves_aspect_does_not_use_full_width_for_tall() {
    // Tall flowchart: must shrink col_width rather than stretch to full pane.
    let layout = compute_layout(400, 900, 100, 40);
    assert!(layout.col_width < 100, "col_width={}", layout.col_width);
    assert_eq!(layout.row_height, 40);
    // aspect in cells: col/(row*2) ≈ 400/900
    let cell_aspect = layout.col_width as f32 / (layout.row_height as f32 * 2.0);
    let img_aspect = 400.0 / 900.0;
    assert!(
        (cell_aspect - img_aspect).abs() < 0.08,
        "cell_aspect={cell_aspect} img_aspect={img_aspect}"
    );
}

#[test]
fn layout_zero_cols_is_safe() {
    let layout = compute_layout(64, 32, 0, 36);
    assert_eq!(layout.row_height, 1); // minimum placeholder
}

#[test]
fn halfblock_line_count_matches_layout_row_height() {
    let png = tiny_png_bytes();
    // tiny.png is 1x1.
    let layout = DiagramLayout {
        out_width_px: 1,
        out_height_px: 1,
        row_height: 1,
        col_width: 1,
    };
    let lines = png_to_halfblock_lines(&png, &layout).unwrap();
    assert_eq!(lines.len(), 1);
    assert!(!lines[0].spans.is_empty());
}

/// Regression: mmdc emits light diagrams on transparent backgrounds. After
/// resize, low-alpha edge pixels must not vanish into Color::Reset (black on
/// dark terminals) — composite onto a dark canvas first.
#[test]
fn halfblock_transparent_light_diagram_stays_visible() {
    let png = fs::read(test_fixtures_dir().join("transparent_diagram.png")).unwrap();
    // Downscale like production (wide terminal cells << source px). This is
    // where transparent edges get averaged to low alpha and used to vanish.
    let layout = DiagramLayout {
        out_width_px: 20,
        out_height_px: 10,
        row_height: 5,
        col_width: 20,
    };
    let lines = png_to_halfblock_lines(&png, &layout).unwrap();
    assert_eq!(lines.len(), 5);

    // Count spans with an explicit RGB foreground (not Reset).
    let mut rgb_fg = 0usize;
    let mut lightish = 0usize;
    for line in &lines {
        for span in &line.spans {
            if let Some(ratatui::style::Color::Rgb(r, g, b)) = span.style.fg {
                rgb_fg += 1;
                if r as u16 + g as u16 + b as u16 > 400 {
                    lightish += 1;
                }
            }
        }
    }
    assert!(
        rgb_fg > 20,
        "expected many visible RGB halfblocks after compositing, got {rgb_fg}"
    );
    assert!(
        lightish > 5,
        "expected light diagram fills to survive downscale, got lightish={lightish}"
    );
}

#[test]
fn document_total_rows_sums_blocks() {
    let mut doc = ConversationDocument::new();
    doc.push_lines(vec![Line::from("a"), Line::from("b")]); // 2
    doc.push_diagram(DiagramSlot::placeholder("h1", "graph TD; A-->B", 5));
    doc.push_lines(vec![Line::from("c")]); // 1
    assert_eq!(doc.total_rows(), 8);
}

#[test]
fn document_visible_lines_skips_offscreen_and_expands_diagram_placeholder() {
    let mut doc = ConversationDocument::new();
    doc.push_lines(vec![Line::from("L1"), Line::from("L2")]);
    doc.push_diagram(DiagramSlot::placeholder("h1", "graph TD; A-->B", 5));
    doc.push_lines(vec![Line::from("L3")]);

    // Total 8 lines.
    // scroll 0, vp 3 -> L1, L2, Placeholder row 1
    let visible = doc.visible_lines(0, 3, false);
    assert_eq!(visible.len(), 3);

    // scroll 2, vp 3 -> Placeholder row 1, 2, 3
    let visible = doc.visible_lines(2, 3, false);
    assert_eq!(visible.len(), 3);
}

#[test]
fn document_slot_at_row_finds_diagram() {
    let mut doc = ConversationDocument::new();
    doc.push_lines(vec![Line::from("L1"), Line::from("L2")]);
    doc.push_diagram(DiagramSlot::placeholder("h1", "graph TD; A-->B", 5));

    let slot_info = doc.diagram_covering_row(3).unwrap();
    assert_eq!(slot_info.start_row, 2);
    assert_eq!(slot_info.slot.source, "graph TD; A-->B");
}

#[test]
fn mermaid_fence_becomes_diagram_slot_not_syntect_lines() {
    let m = msg(
        "assistant",
        vec![part(
            "text",
            Some("Intro\n\n```mermaid\ngraph TD; A-->B\n```\n\nOutro"),
        )],
    );
    let doc = build_conversation_document(&[m], 80, &DiagramIndex::default());

    // Check if we have a diagram block
    let mut diagram_count = 0;
    for block in &doc.blocks {
        if let opencode_multiplexer::ui::conversation::document::DocBlock::Diagram(slot) = block {
            diagram_count += 1;
            assert_eq!(slot.source.trim(), "graph TD; A-->B");
        }
    }
    assert_eq!(diagram_count, 1);
}

#[test]
fn mermaid_lang_is_case_insensitive() {
    let m = msg(
        "assistant",
        vec![part("text", Some("```Mermaid\ngraph TD; A-->B\n```"))],
    );
    let doc = build_conversation_document(&[m], 80, &DiagramIndex::default());
    let diagram_count = doc
        .blocks
        .iter()
        .filter(|b| {
            matches!(
                b,
                opencode_multiplexer::ui::conversation::document::DocBlock::Diagram(_)
            )
        })
        .count();
    assert_eq!(diagram_count, 1);
}

#[test]
fn rust_fence_is_not_diagram() {
    let m = msg(
        "assistant",
        vec![part("text", Some("```rust\nfn main() {}\n```"))],
    );
    let doc = build_conversation_document(&[m], 80, &DiagramIndex::default());
    let diagram_count = doc
        .blocks
        .iter()
        .filter(|b| {
            matches!(
                b,
                opencode_multiplexer::ui::conversation::document::DocBlock::Diagram(_)
            )
        })
        .count();
    assert_eq!(diagram_count, 0);
}

#[test]
fn index_reuses_ready_slot_across_rebuilds() {
    let mut index = DiagramIndex::default();
    let source = "graph TD; A-->B";
    let h = hash_source(source);

    // Simulate a ready slot in index
    let png = Arc::new(tiny_png_bytes());
    let slot = Arc::new(DiagramSlot {
        hash: h.clone(),
        source: source.to_string(),
        phase: opencode_multiplexer::ui::conversation::diagram::slot::DiagramPhase::Ready {
            png,
            png_w: 1,
            png_h: 1,
            width: 80,
            halfblocks: vec![Line::from("Ready")],
            row_height: 1,
            col_width: 80,
        },
        row_height: 1,
    });
    index.slots.insert(h.clone(), slot);

    let m = msg(
        "assistant",
        vec![part("text", Some("```mermaid\ngraph TD; A-->B\n```"))],
    );
    let doc = build_conversation_document(&[m], 80, &index);

    let diagram = doc
        .blocks
        .iter()
        .find_map(|b| {
            if let opencode_multiplexer::ui::conversation::document::DocBlock::Diagram(s) = b {
                Some(s)
            } else {
                None
            }
        })
        .unwrap();
    assert!(matches!(
        diagram.phase,
        opencode_multiplexer::ui::conversation::diagram::slot::DiagramPhase::Ready { .. }
    ));
}

#[test]
fn scheduler_does_not_render_far_offscreen_diagram() {
    let cfg = test_cfg();
    let mut sched = DiagramScheduler::new(cfg.clone());
    let mut doc = ConversationDocument::new();
    doc.push_lines(vec![Line::from("text"); 200]);
    doc.push_diagram(DiagramSlot::placeholder("h1", "graph TD; A-->B", 8));

    // scroll 0, vp 20. window [0, 40] (1 prefetch vp). diagram at 200.
    sched.tick(&doc, 0, 20, 80);
    std::thread::sleep(Duration::from_millis(200));

    assert_eq!(invocation_count(&cfg), 0);
    fs::remove_dir_all(cfg.cache_dir).unwrap();
}

#[test]
fn scheduler_prefetches_within_one_viewport_ahead() {
    let cfg = test_cfg();
    let mut sched = DiagramScheduler::new(cfg.clone());
    let mut doc = ConversationDocument::new();
    doc.push_lines(vec![Line::from("text"); 30]);
    doc.push_diagram(DiagramSlot::placeholder("h1", "graph TD; A-->B", 8));

    // scroll 0, vp 20. window [0, 40]. diagram at 30. Should render.
    sched.tick(&doc, 0, 20, 80);

    let mut finished = Vec::new();
    let start = std::time::Instant::now();
    while finished.is_empty() && start.elapsed() < Duration::from_secs(5) {
        finished.extend(sched.poll_completions());
        std::thread::sleep(Duration::from_millis(50));
    }

    assert!(!finished.is_empty());
    assert_eq!(invocation_count(&cfg), 1);
    fs::remove_dir_all(cfg.cache_dir).unwrap();
}

#[test]
fn scheduler_cache_hit_skips_mmdc() {
    let cfg = test_cfg();
    let mut sched = DiagramScheduler::new(cfg.clone());
    let source = "graph TD; A-->B";
    let h = hash_source(source);

    // Prime cache
    let cache = MermaidCache::new(cfg.cache_dir.clone());
    cache.put(&h, &tiny_png_bytes()).unwrap();

    let mut doc = ConversationDocument::new();
    doc.push_diagram(DiagramSlot::placeholder(&h, source, 8));

    sched.tick(&doc, 0, 20, 80);

    let mut finished = Vec::new();
    let start = std::time::Instant::now();
    while finished.is_empty() && start.elapsed() < Duration::from_secs(2) {
        finished.extend(sched.poll_completions());
        std::thread::sleep(Duration::from_millis(50));
    }

    assert!(!finished.is_empty());
    assert!(matches!(
        finished[0].result,
        opencode_multiplexer::ui::conversation::diagram::slot::DiagramPhase::Ready { .. }
    ));
    assert_eq!(invocation_count(&cfg), 0, "cache hit must not spawn mmdc");
    fs::remove_dir_all(cfg.cache_dir).unwrap();
}

#[test]
fn ready_diagram_reflows_height_and_keeps_follow_tail_at_bottom() {
    let mut conv = ConversationViewState::default();
    conv.open(
        "s1".into(),
        "t".into(),
        opencode_multiplexer::app::focus::AppFocus::Sidebar,
    );

    let mut doc = ConversationDocument::new();
    doc.push_lines(vec![Line::from("L1")]);
    doc.push_diagram(DiagramSlot::placeholder("h1", "src", 8));

    conv.replace_document(doc, 10);
    assert_eq!(conv.scroll_offset(), 0);

    let finished = RenderFinished {
        hash: "h1".into(),
        result: opencode_multiplexer::ui::conversation::diagram::slot::DiagramPhase::Ready {
            png: Arc::new(vec![]),
            png_w: 40,
            png_h: 40,
            width: 80,
            halfblocks: vec![Line::from("H"); 20],
            row_height: 20,
            col_width: 40,
        },
    };
    conv.apply_diagram_update(finished, 10);

    // total = 1 (text) + 20 (diagram) = 21.
    // max_scroll = 21 - 10 = 11.
    assert_eq!(conv.scroll_offset(), 11);
}

#[test]
fn diagram_screen_rect_moves_with_scroll() {
    // slot starts at doc row 10, height 5
    // content_area.y = 2, scroll = 8 → first visible diagram row = 10-8 = 2 → screen_y = 2+2 = 4
    let area = Rect::new(0, 2, 80, 20);
    let r1 = screen_rect_for_slot(10, 5, 8, area, 80);
    assert_eq!(r1, Some(Rect::new(0, 4, 80, 5)));

    // scroll = 9 → first visible diagram row = 10-9 = 1 → screen_y = 2+1 = 3
    let r2 = screen_rect_for_slot(10, 5, 9, area, 80);
    assert_eq!(r2, Some(Rect::new(0, 3, 80, 5)));
}

#[test]
fn diagram_fully_above_viewport_returns_none() {
    let area = Rect::new(0, 2, 80, 20);
    assert!(screen_rect_for_slot(0, 5, 10, area, 80).is_none());
}

#[test]
fn diagram_fully_below_viewport_returns_none() {
    let area = Rect::new(0, 2, 80, 20);
    assert!(screen_rect_for_slot(50, 5, 0, area, 80).is_none());
}

#[test]
fn diagram_partially_visible_clips_height_and_y() {
    // slot 18..28 (len 10), scroll 15, vp 10.
    // Content rows 15..25 are visible.
    // Slot rows 18..25 are visible.
    // Screen relative: start = 18-15 = 3. Height = 25-18 = 7.
    let area = Rect::new(0, 0, 80, 10);
    let r = screen_rect_for_slot(18, 10, 15, area, 80).unwrap();
    assert_eq!(r.y, 3);
    assert_eq!(r.height, 7);
}

#[test]
fn scheduler_timeout_marks_failed_not_hang() {
    let mut cfg = test_cfg();
    cfg.timeout = Duration::from_millis(150);
    let mut sched = DiagramScheduler::new(cfg.clone());
    let mut doc = ConversationDocument::new();
    doc.push_diagram(DiagramSlot::placeholder(
        "h1",
        "FORCE_SLEEP\ngraph TD; A-->B",
        8,
    ));

    sched.tick(&doc, 0, 20, 80);

    // Wait for completion
    let mut finished = Vec::new();
    let start = std::time::Instant::now();
    while finished.is_empty() && start.elapsed() < Duration::from_secs(5) {
        finished.extend(sched.poll_completions());
        std::thread::sleep(Duration::from_millis(50));
    }

    assert!(!finished.is_empty());
    assert!(matches!(
        finished[0].result,
        opencode_multiplexer::ui::conversation::diagram::slot::DiagramPhase::Failed { .. }
    ));
    fs::remove_dir_all(cfg.cache_dir).unwrap();
}
