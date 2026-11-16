//! Compartmentalized vitals section widgets. Each `VitalsSection` renders
//! independently into `Vec<Line<'static>>` for a given width; `compose_vitals`
//! stacks them (Option B) or pages them (Option A) with whole-section pruning.

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use crate::data::vitals::{
    CacheWarmth, MetricBar, SessionVitals, format_compact_duration, format_compact_tokens,
};

pub const SIGNAL_CYAN: Color = Color::Rgb(0, 229, 255);
pub const SECONDARY_VIOLET: Color = Color::Rgb(158, 132, 232);
pub const TERTIARY_INDIGO: Color = Color::Rgb(112, 126, 190);
pub const LIVE_ORCHID: Color = Color::Rgb(196, 132, 218);
pub const CONTEXT_ICE: Color = Color::Rgb(137, 193, 210);
pub const SYNTHETIC_SLATE: Color = Color::Rgb(89, 101, 130);
pub const LABEL_SILVER: Color = Color::Rgb(169, 179, 195);
pub const VALUE_SILVER: Color = Color::Rgb(220, 228, 238);
pub const MUTED_TEXT: Color = Color::Rgb(132, 145, 166);
pub const SECTION_CHROME: Color = Color::Rgb(125, 141, 168);
pub const TRACK_SLATE: Color = Color::Rgb(38, 46, 63);
pub const DIVIDER_SLATE: Color = Color::Rgb(62, 74, 96);
const REAL_BAR_PALETTE: [Color; 3] = [SIGNAL_CYAN, SECONDARY_VIOLET, TERTIARY_INDIGO];

/// One independently-renderable vitals widget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VitalsSection {
    CacheAndSparkline,
    TokensByAgent,
    TokensByModel,
    ContextCulprits,
    TimeSplit,
}

impl VitalsSection {
    pub const ALL: [VitalsSection; 5] = [
        VitalsSection::CacheAndSparkline,
        VitalsSection::TokensByAgent,
        VitalsSection::TokensByModel,
        VitalsSection::ContextCulprits,
        VitalsSection::TimeSplit,
    ];

    /// Render this section standalone at `width` columns. Sections with no
    /// underlying data render nothing so the composer can skip them cleanly.
    /// Blank spacer lines between sections are inserted by `compose_vitals`,
    /// never by individual sections.
    pub fn render(self, v: &SessionVitals, width: u16) -> Vec<Line<'static>> {
        match self {
            VitalsSection::CacheAndSparkline => self.render_cache_sparkline(v, width),
            VitalsSection::TokensByAgent => self.render_bars(
                "by agent",
                &v.tokens_by_agent,
                4,
                false,
                width,
                Some(v.latest_context_tokens),
            ),
            VitalsSection::TokensByModel => self.render_bars(
                "by model",
                &v.tokens_by_model,
                3,
                false,
                width,
                Some(v.latest_context_tokens),
            ),
            VitalsSection::ContextCulprits => self.render_bars(
                "context",
                &v.context_culprits,
                4,
                false,
                width,
                Some(v.latest_context_tokens),
            ),
            VitalsSection::TimeSplit => self.render_time_split(v, width),
        }
    }

    fn render_bars(
        self,
        header: &'static str,
        bars_data: &[MetricBar],
        max_rows: usize,
        _unused: bool,
        width: u16,
        scale_max: Option<u64>,
    ) -> Vec<Line<'static>> {
        if bars_data.is_empty() {
            return Vec::new();
        }
        let mut lines = vec![header_line(header)];
        lines.extend(bar_lines(bars_data, max_rows, false, width, scale_max));
        lines
    }

    fn render_time_split(&self, v: &SessionVitals, width: u16) -> Vec<Line<'static>> {
        if v.tools_by_time.is_empty() && v.llm_wall_ms == 0 && v.tool_wall_ms == 0 {
            return Vec::new();
        }
        let mut lines = vec![Line::from(Span::styled(
            format!(
                "time · llm {} · tool {}",
                format_header_duration(v.llm_wall_ms),
                format_header_duration(v.tool_wall_ms)
            ),
            Style::default().fg(SECTION_CHROME),
        ))];
        lines[0] = truncate_line(lines[0].clone(), width);
        lines.extend(bar_lines(&v.tools_by_time, 3, true, width, None));
        lines
    }

    fn render_cache_sparkline(&self, v: &SessionVitals, width: u16) -> Vec<Line<'static>> {
        // Line 1: cache hit % + cache warmth + current context window.
        let (pct_text, pct_color) = match v.cache_hit_pct {
            Some(p) => (
                format!("{p}%"),
                if p >= 80 {
                    SIGNAL_CYAN
                } else if p >= 20 {
                    SECONDARY_VIOLET
                } else {
                    TERTIARY_INDIGO
                },
            ),
            None => ("--".to_string(), MUTED_TEXT),
        };
        let (warm_text, warm_color) = warmth_display(v.warmth);
        let header = Line::from(vec![
            Span::styled("cache ", Style::default().fg(LABEL_SILVER)),
            Span::styled(pct_text, Style::default().fg(pct_color)),
            Span::styled(" · ", Style::default().fg(DIVIDER_SLATE)),
            Span::styled(warm_text, Style::default().fg(warm_color)),
            Span::styled(" · ctx ", Style::default().fg(LABEL_SILVER)),
            Span::styled(
                format_compact_tokens(v.latest_context_tokens),
                Style::default().fg(CONTEXT_ICE),
            ),
        ]);
        let header = truncate_line(header, width);

        // Line 2: all-history sparkline downsampled to the available columns.
        let suffix = format!(
            "  {} turn{}",
            v.turn_count,
            if v.turn_count == 1 { "" } else { "s" }
        );
        let suffix_w = suffix.chars().count() as u16;
        let spark_w = width.saturating_sub(suffix_w).max(1) as usize;
        let points = resample_sparkline(&v.sparkline, spark_w);
        let max_total = points
            .iter()
            .map(|p| p.total_context_tokens)
            .max()
            .unwrap_or(0);
        const GLYPHS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut used = 0usize;
        let mut markers = 0usize;
        let marker_limit = spark_w / 3;
        for (i, p) in points.iter().enumerate() {
            if p.compacted_before && i > 0 && markers < marker_limit && used + 1 < spark_w {
                spans.push(Span::styled("│", Style::default().fg(DIVIDER_SLATE)));
                used += 1;
                markers += 1;
            }
            if used >= spark_w {
                break;
            }
            let level = if max_total == 0 {
                0
            } else {
                ((p.total_context_tokens as f64 / max_total as f64) * 7.0).round() as usize
            };
            let color = if p.prompt_tokens == 0 && p.total_context_tokens == 0 {
                MUTED_TEXT
            } else if p.cache_hit_ratio >= 0.8 {
                SIGNAL_CYAN
            } else if p.cache_hit_ratio >= 0.2 {
                SECONDARY_VIOLET
            } else {
                TERTIARY_INDIGO
            };
            spans.push(Span::styled(
                GLYPHS[level.min(7)].to_string(),
                Style::default().fg(color),
            ));
            used += 1;
        }
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            v.turn_count.to_string(),
            Style::default().fg(VALUE_SILVER),
        ));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            if v.turn_count == 1 { "turn" } else { "turns" },
            Style::default().fg(MUTED_TEXT),
        ));
        let spark = truncate_line(Line::from(spans), width);
        vec![header, spark]
    }
}

/// Layout mode: stacked (Option B) or paged (Option A).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VitalsMode {
    StackedAll,
    Paged(usize),
}

impl VitalsMode {
    fn sections(self) -> Vec<VitalsSection> {
        match self {
            VitalsMode::StackedAll => VitalsSection::ALL.to_vec(),
            VitalsMode::Paged(p) => {
                if p % 2 == 0 {
                    vec![
                        VitalsSection::CacheAndSparkline,
                        VitalsSection::TokensByAgent,
                        VitalsSection::TokensByModel,
                    ]
                } else {
                    vec![
                        VitalsSection::CacheAndSparkline,
                        VitalsSection::ContextCulprits,
                        VitalsSection::TimeSplit,
                    ]
                }
            }
        }
    }
}

fn header_line(text: &'static str) -> Line<'static> {
    Line::from(Span::styled(text, Style::default().fg(SECTION_CHROME)))
}

fn warmth_display(w: CacheWarmth) -> (String, Color) {
    fn dur(s: u64) -> String {
        if s < 60 {
            format!("{s}s")
        } else {
            format!("{}m", s / 60)
        }
    }
    match w {
        CacheWarmth::Empty => ("idle".to_string(), MUTED_TEXT),
        CacheWarmth::Live => ("live".to_string(), LIVE_ORCHID),
        CacheWarmth::Warm(s) => (format!("w{}", dur(s)), CONTEXT_ICE),
        CacheWarmth::Cooling(s) => (format!("~{}", dur(s)), SECONDARY_VIOLET),
        CacheWarmth::Cold(_) => ("cold".to_string(), TERTIARY_INDIGO),
    }
}

/// Truncate a line's spans to `width` terminal columns (simple char count).
fn truncate_line(line: Line<'static>, width: u16) -> Line<'static> {
    let max = width as usize;
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut remaining = max;
    for span in line.spans {
        if remaining == 0 {
            break;
        }
        let text = span.content.to_string();
        let take: String = text.chars().take(remaining).collect();
        remaining -= take.chars().count();
        out.push(Span::styled(take, span.style));
    }
    Line::from(out)
}

fn format_header_duration(ms: u64) -> String {
    if ms >= 3_600_000 {
        format!("{}h{:02}m", ms / 3_600_000, (ms % 3_600_000) / 60_000)
    } else if ms >= 600_000 {
        format!("{}m", ms / 60_000)
    } else {
        format_compact_duration(ms)
    }
}

/// Resample the full turn history into at most `max_cols` buckets. Each bucket
/// keeps its peak context size, token-weighted cache-hit ratio, and any
/// compaction marker. A single cold turn no longer paints a whole bucket as
/// cold; a sustained cold period remains visible.
fn resample_sparkline(
    points: &[crate::data::vitals::SparkPoint],
    max_cols: usize,
) -> Vec<crate::data::vitals::SparkPoint> {
    if points.is_empty() || max_cols == 0 {
        return Vec::new();
    }
    if points.len() <= max_cols {
        return points.to_vec();
    }
    let marker_count = points.iter().filter(|p| p.compacted_before).count();
    let marker_budget = marker_count.min(max_cols / 3);
    let bucket_count = max_cols
        .saturating_sub(marker_budget)
        .max(1)
        .min(points.len());
    (0..bucket_count)
        .map(|i| {
            let start = i * points.len() / bucket_count;
            let end = ((i + 1) * points.len() / bucket_count).max(start + 1);
            let bucket = &points[start..end];
            let last = bucket.last().expect("non-empty bucket");
            crate::data::vitals::SparkPoint {
                total_context_tokens: bucket
                    .iter()
                    .map(|p| p.total_context_tokens)
                    .max()
                    .unwrap_or(last.total_context_tokens),
                cache_read_tokens: bucket.iter().map(|p| p.cache_read_tokens).sum(),
                prompt_tokens: bucket.iter().map(|p| p.prompt_tokens).sum(),
                cache_hit_ratio: {
                    let prompt: u128 = bucket.iter().map(|p| p.prompt_tokens as u128).sum();
                    let read: u128 = bucket.iter().map(|p| p.cache_read_tokens as u128).sum();
                    if prompt > 0 {
                        (read as f64 / prompt as f64) as f32
                    } else {
                        let weighted_total: u128 =
                            bucket.iter().map(|p| p.total_context_tokens as u128).sum();
                        if weighted_total > 0 {
                            (bucket
                                .iter()
                                .map(|p| p.cache_hit_ratio as f64 * p.total_context_tokens as f64)
                                .sum::<f64>()
                                / weighted_total as f64) as f32
                        } else {
                            0.0
                        }
                    }
                },
                compacted_before: i > 0 && bucket.iter().any(|p| p.compacted_before),
            }
        })
        .collect()
}

/// Shared compact bar renderer. Labels are truncated with `…`, bars are
/// max-scaled within the section, and every row fits `width` columns.
fn bar_lines(
    bars: &[MetricBar],
    max_rows: usize,
    durations: bool,
    width: u16,
    scale_max: Option<u64>,
) -> Vec<Line<'static>> {
    let width = width as usize;
    if bars.is_empty() || width == 0 {
        return Vec::new();
    }
    // Roll overflow into one trailing bar.
    let mut rows: Vec<(String, u64, String)> = bars
        .iter()
        .map(|b| {
            let right = if durations {
                format_header_duration(b.value)
            } else {
                format_compact_tokens(b.value)
            };
            (b.label.clone(), b.value, right)
        })
        .collect();
    if rows.len() > max_rows {
        let tail: Vec<_> = rows.split_off(max_rows.saturating_sub(1).max(1));
        let sum: u64 = tail.iter().map(|r| r.1).sum();
        let right = if durations {
            format_header_duration(sum)
        } else {
            format_compact_tokens(sum)
        };
        rows.push((format!("+{} more", tail.len()), sum, right));
    }

    let row_max = rows.iter().map(|r| r.1).max().unwrap_or(0);
    let max_val = scale_max.filter(|v| *v > 0).unwrap_or(row_max).max(1);
    let val_w = rows.iter().map(|r| r.2.chars().count()).max().unwrap_or(0);
    let mut label_w = 12usize.min(width.saturating_sub(val_w + 3).max(1));
    let mut bar_w = width.saturating_sub(label_w + val_w + 2);
    if bar_w == 0 {
        label_w = width.saturating_sub(val_w + 2).max(1);
        bar_w = width.saturating_sub(label_w + val_w + 2).max(1);
    }
    let mut real_idx = 0usize;
    rows.iter()
        .map(|(label, value, right)| {
            let synthetic = is_synthetic_label(label);
            let (bar_color, label_color, value_color) = if synthetic {
                (SYNTHETIC_SLATE, MUTED_TEXT, MUTED_TEXT)
            } else {
                let color = REAL_BAR_PALETTE[real_idx.min(REAL_BAR_PALETTE.len() - 1)];
                real_idx += 1;
                (color, LABEL_SILVER, VALUE_SILVER)
            };
            let short = truncate_label(label, label_w);
            let padded = format!("{short:<label_w$}");
            let fill = if *value == 0 {
                0
            } else {
                (((*value as f64 / max_val as f64) * bar_w as f64).round() as usize).max(1)
            };
            let track = bar_w.saturating_sub(fill);
            Line::from(vec![
                Span::styled(padded, Style::default().fg(label_color)),
                Span::raw(" "),
                Span::styled("█".repeat(fill), Style::default().fg(bar_color)),
                Span::styled("─".repeat(track), Style::default().fg(TRACK_SLATE)),
                Span::styled(
                    format!(" {right:<val_w$}"),
                    Style::default().fg(value_color),
                ),
            ])
        })
        .collect()
}

fn is_synthetic_label(label: &str) -> bool {
    label == "system"
        || label == "system/other"
        || (label.starts_with('+') && label.ends_with(" more"))
}

fn truncate_label(label: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let chars: Vec<char> = label.chars().collect();
    if chars.len() <= max {
        return label.to_string();
    }
    if max <= 1 {
        return "…".to_string();
    }
    let mut s: String = chars[..max - 1].iter().collect();
    s.push('…');
    s
}

/// Compose vitals sections into a bottom-docked block of at most `max_height`
/// rows. Whole lower-priority sections are dropped rather than clipped.
pub fn compose_vitals(
    v: &SessionVitals,
    mode: VitalsMode,
    width: u16,
    max_height: u16,
) -> Vec<Line<'static>> {
    if max_height < 2 {
        return Vec::new();
    }
    let divider = Line::from(vec![
        Span::styled("── ", Style::default().fg(DIVIDER_SLATE)),
        Span::styled("vitals", Style::default().fg(SECTION_CHROME)),
        Span::styled(
            format!(" ─{}", "─".repeat((width as usize).saturating_sub(11))),
            Style::default().fg(DIVIDER_SLATE),
        ),
    ]);
    if v.turn_count == 0 && v.tokens_by_agent.is_empty() {
        return vec![
            divider,
            Line::from(Span::styled(
                "no turns yet",
                Style::default().fg(MUTED_TEXT),
            )),
        ];
    }
    let mut out: Vec<Line<'static>> = vec![divider];
    for section in mode.sections() {
        let lines = section.render(v, width);
        if lines.is_empty() {
            continue;
        }
        let needed = (if out.len() > 1 { 1 } else { 0 }) + lines.len();
        if out.len() + needed > max_height as usize {
            break;
        }
        if out.len() > 1 {
            out.push(Line::from(""));
        }
        out.extend(lines);
    }
    if out.len() == 1 {
        // Only the divider fit: no vitals at all.
        return Vec::new();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::vitals::{MetricBar, SessionVitals, SparkPoint};

    fn sample_vitals() -> SessionVitals {
        SessionVitals {
            session_id: "s1".to_string(),
            cache_hit_pct: Some(98),
            warmth: crate::data::vitals::CacheWarmth::Warm(65),
            uncached_input_tokens: 14_200,
            latest_context_tokens: 372_400,
            turn_count: 11,
            sparkline: vec![
                SparkPoint {
                    total_context_tokens: 50_000,
                    cache_read_tokens: 95_000,
                    prompt_tokens: 100_000,
                    cache_hit_ratio: 0.95,
                    compacted_before: false,
                },
                SparkPoint {
                    total_context_tokens: 120_000,
                    cache_read_tokens: 90_000,
                    prompt_tokens: 100_000,
                    cache_hit_ratio: 0.9,
                    compacted_before: false,
                },
                SparkPoint {
                    total_context_tokens: 372_400,
                    cache_read_tokens: 10_000,
                    prompt_tokens: 100_000,
                    cache_hit_ratio: 0.1,
                    compacted_before: true,
                },
            ],
            tokens_by_agent: vec![
                MetricBar {
                    label: "build".into(),
                    value: 812_000,
                    count: None,
                },
                MetricBar {
                    label: "explorer".into(),
                    value: 240_000,
                    count: None,
                },
                MetricBar {
                    label: "oracle".into(),
                    value: 85_000,
                    count: None,
                },
                MetricBar {
                    label: "fixer".into(),
                    value: 12_000,
                    count: None,
                },
            ],
            tokens_by_model: vec![
                MetricBar {
                    label: "gpt-6-luna".into(),
                    value: 897_000,
                    count: None,
                },
                MetricBar {
                    label: "gemini-4-carbon".into(),
                    value: 240_000,
                    count: None,
                },
                MetricBar {
                    label: "claude-sonnet-4-5".into(),
                    value: 12_000,
                    count: None,
                },
            ],
            context_culprits: vec![
                MetricBar {
                    label: "read".into(),
                    value: 140_000,
                    count: None,
                },
                MetricBar {
                    label: "bash".into(),
                    value: 95_000,
                    count: None,
                },
                MetricBar {
                    label: "text".into(),
                    value: 48_000,
                    count: None,
                },
                MetricBar {
                    label: "think".into(),
                    value: 10_000,
                    count: None,
                },
            ],
            llm_wall_ms: 100_000,
            tool_wall_ms: 252_000,
            tools_by_time: vec![
                MetricBar {
                    label: "bash".into(),
                    value: 230_000,
                    count: Some(18),
                },
                MetricBar {
                    label: "read".into(),
                    value: 18_000,
                    count: Some(24),
                },
                MetricBar {
                    label: "edit".into(),
                    value: 4_000,
                    count: Some(3),
                },
            ],
        }
    }

    fn text(lines: &[ratatui::text::Line<'static>]) -> Vec<String> {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.to_string())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn sparkline_resample_and_color_buckets_reflect_cache_health_and_compaction() {
        let v = sample_vitals();
        let lines = VitalsSection::CacheAndSparkline.render(&v, 29);
        assert_eq!(lines.len(), 2);
        let spark = &lines[1];
        let spark_text: String = spark.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(
            spark_text.contains('│'),
            "compaction marker expected: {spark_text}"
        );
        // Third point is a cold cache (<20%): use the cool indigo state, not alarm red.
        let cold = spark
            .spans
            .iter()
            .find(|s| s.style.fg == Some(TERTIARY_INDIGO))
            .unwrap();
        assert!(!cold.content.is_empty());
        assert!(text(&lines)[0].contains("ctx 372k"));
        assert!(spark_text.contains("11 turns"));
    }

    #[test]
    fn each_section_fits_strictly_within_29_columns() {
        let mut v = sample_vitals();
        v.tokens_by_model = vec![MetricBar {
            label: "anthropic/claude-opus-4-6-extended".into(),
            value: 1,
            count: None,
        }];
        v.tokens_by_agent = vec![MetricBar {
            label: "a-very-long-agent-name-xyz".into(),
            value: 1,
            count: None,
        }];
        for section in VitalsSection::ALL {
            for line in section.render(&v, 29) {
                assert!(
                    line.width() <= 29,
                    "section {section:?} overflowed: '{}' ({} cols)",
                    text(std::slice::from_ref(&line))[0],
                    line.width()
                );
            }
        }
    }

    #[test]
    fn sections_after_header_begin_with_blank_spacer_line() {
        let v = sample_vitals();
        let lines = compose_vitals(&v, VitalsMode::StackedAll, 29, 25);
        let t = text(&lines);
        // Divider + CacheAndSparkline(2) + blank + ByAgent(5) + blank + ByModel(4) + blank + Culprits(5) + blank + Time(4) = 25
        assert_eq!(lines.len(), 25, "unexpected layout: {t:?}");
        assert_eq!(t[3], "", "blank spacer before `by agent`");
        assert_eq!(t[9], "", "blank spacer before `by model`");
        assert_eq!(t[14], "", "blank spacer before `context`");
        assert_eq!(t[20], "", "blank spacer before `time`");
        assert!(t[4] == "by agent", "got {}", t[4]);
    }

    #[test]
    fn stacked_mode_drops_whole_bottom_sections_when_height_shrinks() {
        let v = sample_vitals();
        let full = compose_vitals(&v, VitalsMode::StackedAll, 29, 25);
        assert_eq!(full.len(), 25);
        let pruned = compose_vitals(&v, VitalsMode::StackedAll, 29, 20);
        // TimeSplit (header + 3 tools + 1 spacer = 5) dropped whole: 25 - 5 = 20.
        assert_eq!(pruned.len(), 20);
        let t = text(&pruned);
        assert!(
            !t.iter().any(|l| l.starts_with("time")),
            "time section must be dropped whole: {t:?}"
        );
        assert!(pruned.iter().any(|l| {
            l.spans
                .iter()
                .map(|s| s.content.to_string())
                .collect::<String>()
                == "context"
        }));
        // Too little height for even CacheAndSparkline + divider: nothing.
        assert!(compose_vitals(&v, VitalsMode::StackedAll, 29, 2).is_empty());
    }

    #[test]
    fn paged_mode_renders_page_zero_and_page_one_from_same_sections() {
        let v = sample_vitals();
        let p0 = text(&compose_vitals(&v, VitalsMode::Paged(0), 29, 25));
        assert!(p0.iter().any(|l| l == "by agent"));
        assert!(p0.iter().any(|l| l == "by model"));
        assert!(!p0.iter().any(|l| l == "context"));
        let p1 = text(&compose_vitals(&v, VitalsMode::Paged(1), 29, 25));
        assert!(p1.iter().any(|l| l == "context"));
        assert!(p1.iter().any(|l| l.starts_with("time")));
        assert!(!p1.iter().any(|l| l == "by agent"));
        // Both pages share the cache/sparkline header.
        assert!(p0.iter().any(|l| l.starts_with("cache")));
        assert!(p1.iter().any(|l| l.starts_with("cache")));
    }

    #[test]
    fn empty_vitals_renders_quiet_two_line_placeholder() {
        let v = SessionVitals::default();
        let lines = compose_vitals(&v, VitalsMode::StackedAll, 29, 10);
        assert_eq!(lines.len(), 2);
        let t = text(&lines);
        assert_eq!(t[0], format!("── vitals {}", "─".repeat(19)));
        assert_eq!(t[1], "no turns yet");
    }

    #[test]
    fn sparkline_downsamples_full_history_and_labels_turns() {
        let mut v = sample_vitals();
        v.turn_count = 200;
        v.latest_context_tokens = 600_000;
        v.sparkline = (0..200)
            .map(|i| SparkPoint {
                total_context_tokens: 10_000 + (i as u64 * 2_950),
                cache_read_tokens: if i < 10 || i == 50 || i == 80 {
                    0
                } else {
                    100_000
                },
                prompt_tokens: 100_000,
                cache_hit_ratio: if i == 50 { 0.05 } else { 0.95 },
                compacted_before: i == 100,
            })
            .collect();
        let lines = VitalsSection::CacheAndSparkline.render(&v, 29);
        let t = text(&lines);
        assert!(t[0].contains("ctx 600k"), "header context: {}", t[0]);
        assert!(
            t[1].contains("200 turns"),
            "turns must be explicit: {}",
            t[1]
        );
        assert!(t[1].contains("│"), "compaction marker: {}", t[1]);
        let glyphs: String = t[1].chars().take_while(|c| *c != ' ').collect();
        assert!(
            matches!(glyphs.chars().next(), Some('▁' | '▂')),
            "full-history curve should start low: {glyphs}"
        );
        assert!(
            glyphs.ends_with('█'),
            "full-history curve should end high: {glyphs}"
        );
        assert!(
            lines[1]
                .spans
                .iter()
                .any(|s| s.style.fg == Some(TERTIARY_INDIGO)),
            "cold bucket should stay visible after resampling"
        );
    }

    #[test]
    fn bars_are_context_proportional_with_separate_dark_track_spans() {
        let mut v = sample_vitals();
        v.latest_context_tokens = 600_000;
        v.tokens_by_agent = vec![MetricBar {
            label: "build".into(),
            value: 300_000,
            count: None,
        }];
        let lines = VitalsSection::TokensByAgent.render(&v, 29);
        let row = &lines[1];
        let fill_span = row.spans.iter().find(|s| s.content.contains('█')).unwrap();
        let track_span = row.spans.iter().find(|s| s.content.contains('─')).unwrap();
        let fill = fill_span.content.chars().count();
        let track = track_span.content.chars().count();
        assert!(
            (fill as isize - track as isize).abs() <= 1,
            "300k/600k should fill about half: fill={fill} track={track}"
        );
        assert_eq!(fill_span.style.fg, Some(SIGNAL_CYAN));
        assert_eq!(track_span.style.fg, Some(TRACK_SLATE));
        let all = compose_vitals(&v, VitalsMode::StackedAll, 29, 24);
        assert!(
            !all.iter()
                .flat_map(|l| &l.spans)
                .any(|s| s.style.fg == Some(Color::Green))
        );
    }

    #[test]
    fn time_header_stays_within_sidebar_width_and_keeps_duration_token() {
        let mut v = sample_vitals();
        v.llm_wall_ms = 82 * 60_000;
        v.tool_wall_ms = 24 * 60_000 + 9_000;
        let lines = VitalsSection::TimeSplit.render(&v, 29);
        assert!(lines[0].width() <= 29);
        let t = text(&lines);
        assert!(
            t[0].ends_with("24m"),
            "full duration, not clipped: {}",
            t[0]
        );
    }

    #[test]
    fn sparkline_bucket_uses_weighted_cache_ratio_not_single_turn_min() {
        let points: Vec<_> = (0..100)
            .map(|i| {
                let cold = i < 10 || i == 50 || i == 80;
                SparkPoint {
                    total_context_tokens: 100_000,
                    cache_read_tokens: if cold { 0 } else { 100_000 },
                    prompt_tokens: 100_000,
                    cache_hit_ratio: if cold { 0.0 } else { 1.0 },
                    compacted_before: false,
                }
            })
            .collect();
        let buckets = resample_sparkline(&points, 10);
        assert_eq!(buckets.len(), 10);
        assert!(
            buckets[0].cache_hit_ratio < 0.2,
            "sustained cold bucket stays cold"
        );
        assert!(
            buckets[4].cache_hit_ratio > 0.8,
            "one cold turn in a warm bucket should not dominate"
        );
        assert!(
            buckets[7].cache_hit_ratio > 0.8,
            "scattered cold turn should be weighted"
        );
        let unknown = resample_sparkline(&[SparkPoint::default()], 4);
        assert_eq!(unknown[0].prompt_tokens, 0);
        assert_eq!(unknown[0].total_context_tokens, 0);
    }

    #[test]
    fn synthetic_rows_use_slate_and_do_not_consume_accent_slots() {
        let bars = vec![
            MetricBar {
                label: "build".into(),
                value: 415_000,
                count: None,
            },
            MetricBar {
                label: "system".into(),
                value: 177_000,
                count: None,
            },
            MetricBar {
                label: "chat".into(),
                value: 124_000,
                count: None,
            },
            MetricBar {
                label: "plan".into(),
                value: 10_000,
                count: None,
            },
            MetricBar {
                label: "explorer".into(),
                value: 7_000,
                count: None,
            },
        ];
        let lines = bar_lines(&bars, 4, false, 29, Some(733_000));
        assert_eq!(lines.len(), 4);
        let fill = |i: usize| {
            lines[i]
                .spans
                .iter()
                .find(|s| s.content.contains('█'))
                .unwrap()
                .style
                .fg
        };
        assert_eq!(fill(0), Some(SIGNAL_CYAN));
        assert_eq!(fill(1), Some(SYNTHETIC_SLATE));
        assert_eq!(fill(2), Some(SECONDARY_VIOLET));
        assert_eq!(fill(3), Some(SYNTHETIC_SLATE));
        assert_eq!(lines[0].spans[0].style.fg, Some(LABEL_SILVER));
        assert_eq!(lines[1].spans[0].style.fg, Some(MUTED_TEXT));
        assert_eq!(lines[1].spans.last().unwrap().style.fg, Some(MUTED_TEXT));
    }

    #[test]
    fn track_is_a_thin_dash_and_palette_avoids_error_colors() {
        let mut v = sample_vitals();
        v.warmth = CacheWarmth::Live;
        v.latest_context_tokens = 2_000_000;
        let row = VitalsSection::TokensByAgent.render(&v, 29).remove(1);
        let track = row.spans.iter().find(|s| s.content.contains('─')).unwrap();
        assert_eq!(track.style.fg, Some(TRACK_SLATE));
        let all = compose_vitals(&v, VitalsMode::StackedAll, 29, 25);
        let forbidden = [
            Color::Red,
            Color::Green,
            Color::Yellow,
            Color::Rgb(255, 59, 92),
            Color::Rgb(255, 184, 0),
            Color::Rgb(255, 46, 151),
        ];
        assert!(
            !all.iter()
                .flat_map(|l| &l.spans)
                .any(|s| s.style.fg.is_some_and(|c| forbidden.contains(&c)))
        );
        v.latest_context_tokens = 372_400;
        let cache = VitalsSection::CacheAndSparkline.render(&v, 29);
        assert!(
            cache[0]
                .spans
                .iter()
                .any(|s| s.content == "live" && s.style.fg == Some(LIVE_ORCHID))
        );
        assert!(
            cache[0]
                .spans
                .iter()
                .any(|s| s.content == "372k" && s.style.fg == Some(CONTEXT_ICE))
        );
        assert!(
            cache[1]
                .spans
                .iter()
                .any(|s| s.content.contains("turns") && s.style.fg == Some(MUTED_TEXT))
        );
    }
}
