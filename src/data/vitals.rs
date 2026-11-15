//! Session vitals: token/cache/context telemetry for one session tree.
//! Read from opencode's SQLite store; never blocks the UI (background worker).

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::{self, JoinHandle};
use std::time::SystemTime;

use rusqlite::{Connection, params};

pub const BLOB_GUARD_BYTES: usize = 262_144;

/// Cache freshness of the primary session's prompt cache.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CacheWarmth {
    #[default]
    Empty,
    Live,
    Warm(u64),
    Cooling(u64),
    Cold(u64),
}

/// One assistant turn's context size + cache health + compaction marker.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SparkPoint {
    pub total_context_tokens: u64,
    pub cache_read_tokens: u64,
    pub prompt_tokens: u64,
    pub cache_hit_ratio: f32,
    pub compacted_before: bool,
}

/// One labelled bar row (tokens, estimated tokens, or wall-clock ms).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricBar {
    pub label: String,
    pub value: u64,
    pub count: Option<u32>,
}

/// Aggregated telemetry for the active session tree.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionVitals {
    pub session_id: String,
    pub cache_hit_pct: Option<u8>,
    pub warmth: CacheWarmth,
    pub uncached_input_tokens: u64,
    pub latest_context_tokens: u64,
    pub turn_count: usize,
    pub sparkline: Vec<SparkPoint>,
    pub tokens_by_agent: Vec<MetricBar>,
    pub tokens_by_model: Vec<MetricBar>,
    pub context_culprits: Vec<MetricBar>,
    pub llm_wall_ms: u64,
    pub tool_wall_ms: u64,
    pub tools_by_time: Vec<MetricBar>,
}

/// Format a token count compactly: `842` → `842`, `14240` → `14.2k`, `2.46M` → `2.5M`.
pub fn format_compact_tokens(n: u64) -> String {
    if n < 1000 {
        n.to_string()
    } else if n < 1_000_000 {
        let k = n as f64 / 1_000.0;
        if k >= 100.0 || k.fract() == 0.0 {
            format!("{k:.0}k")
        } else {
            format!("{k:.1}k")
        }
    } else {
        let m = n as f64 / 1_000_000.0;
        if m >= 100.0 || m.fract() == 0.0 {
            format!("{m:.0}M")
        } else {
            format!("{m:.1}M")
        }
    }
}

/// Format a wall-clock duration in milliseconds compactly.
pub fn format_compact_duration(ms: u64) -> String {
    if ms < 1000 {
        if ms == 0 {
            "0s".to_string()
        } else {
            "<1s".to_string()
        }
    } else if ms < 60_000 {
        format!("{}s", ms / 1000)
    } else if ms < 3_600_000 {
        format!("{}m{:02}s", ms / 60_000, (ms % 60_000) / 1000)
    } else {
        format!("{}h{:02}m", ms / 3_600_000, (ms % 3_600_000) / 60_000)
    }
}

/// Classify prompt-cache freshness from last assistant completion timestamp.
/// `now_ms` / `last_completed_ms` are epoch milliseconds; `Warm`/`Cooling`/`Cold`
/// carry the idle age in whole seconds.
pub fn classify_warmth(
    is_working: bool,
    last_completed_ms: Option<i64>,
    now_ms: i64,
) -> CacheWarmth {
    if is_working {
        return CacheWarmth::Live;
    }
    let Some(last) = last_completed_ms else {
        return CacheWarmth::Empty;
    };
    let age_secs = ((now_ms - last) / 1000).max(0) as u64;
    if age_secs < 240 {
        CacheWarmth::Warm(age_secs)
    } else if age_secs < 300 {
        CacheWarmth::Cooling(age_secs)
    } else {
        CacheWarmth::Cold(age_secs)
    }
}

/// Total covered milliseconds of `[start, end)` intervals, merging overlaps.
/// Intervals with `end <= start` are ignored; the slice is sorted in place.
pub fn merge_intervals_ms(intervals: &mut [(i64, i64)]) -> u64 {
    intervals.sort_by_key(|&(s, _)| s);
    let mut total: i64 = 0;
    let mut cur: Option<(i64, i64)> = None;
    for &(s, e) in intervals.iter() {
        if e <= s {
            continue;
        }
        match cur {
            None => cur = Some((s, e)),
            Some((cs, ce)) if s <= ce => cur = Some((cs, ce.max(e))),
            Some((cs, ce)) => {
                total += ce - cs;
                cur = Some((s, e));
            }
        }
    }
    if let Some((cs, ce)) = cur {
        total += ce - cs;
    }
    total.max(0) as u64
}

/// Strip leading `provider/` prefix and trailing `-YYYYMMDD` date stamps (8+ digits).
pub fn short_model_label(model_id: &str) -> String {
    let base = model_id.rsplit('/').next().unwrap_or(model_id);
    if let Some(dash) = base.rfind('-') {
        let suffix = &base[dash + 1..];
        if suffix.len() >= 8 && suffix.chars().all(|c| c.is_ascii_digit()) {
            return base[..dash].to_string();
        }
    }
    base.to_string()
}

#[derive(Debug, Clone)]
struct MsgRec {
    id: String,
    time_created: i64,
    role: String,
    agent: String,
    model: String,
    completed: Option<i64>,
    input: u64,
    output: u64,
    reasoning: u64,
    cache_read: u64,
    cache_write: u64,
}

impl MsgRec {
    fn total(&self) -> u64 {
        self.input + self.output + self.reasoning + self.cache_read + self.cache_write
    }
}

#[derive(Debug, Clone)]
struct PartRec {
    typ: String,
    tool: String,
    subagent_type: Option<String>,
    tail_start_id: Option<String>,
    t_start: Option<i64>,
    t_end: Option<i64>,
    visible_bytes: u64,
}

#[derive(Debug, Default)]
struct SessionRaw {
    /// Ordered by (time_created, id).
    messages: BTreeMap<(i64, String), MsgRec>,
    parts_by_msg: HashMap<String, Vec<PartRec>>,
    last_time_created: i64,
}

/// Incremental per-session cache over opencode's `message`/`part` tables.
///
/// Queries only messages newer than the cached cursor (with a 60s overlap
/// window so in-place `message.updated` token fills are re-read), and only
/// fetches parts for messages that were inserted or replaced. Multi-megabyte
/// part blobs never go through full JSON parsing: header fields are read via
/// `json_extract` only for rows under `BLOB_GUARD_BYTES`, and larger rows are
/// attributed from a 512-byte prefix scan.
#[derive(Debug, Default)]
pub struct VitalsCache {
    sessions: HashMap<String, SessionRaw>,
}

impl VitalsCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Compute vitals for `primary` rolled up with `children`.
    pub fn compute(
        &mut self,
        conn: &Connection,
        primary: &str,
        children: &[String],
        is_working: bool,
        now_ms: i64,
    ) -> anyhow::Result<SessionVitals> {
        let mut tree: Vec<String> = vec![primary.to_string()];
        for c in children {
            if c != primary {
                tree.push(c.clone());
            }
        }
        for sid in &tree {
            self.refresh_session(conn, sid)?;
        }
        Ok(rollup(&self.sessions, primary, &tree, is_working, now_ms))
    }

    fn refresh_session(&mut self, conn: &Connection, sid: &str) -> anyhow::Result<()> {
        let entry = self.sessions.entry(sid.to_string()).or_default();
        let since = entry.last_time_created - 60_000;
        let mut stmt = conn.prepare(
            "SELECT id, time_created,
                    json_extract(data, '$.role'),
                    json_extract(data, '$.agent'),
                    json_extract(data, '$.modelID'),
                    json_extract(data, '$.time.completed'),
                    json_extract(data, '$.tokens.input'),
                    json_extract(data, '$.tokens.output'),
                    json_extract(data, '$.tokens.reasoning'),
                    json_extract(data, '$.tokens.cache.read'),
                    json_extract(data, '$.tokens.cache.write')
             FROM message
             WHERE session_id = ?1 AND time_created > ?2
             ORDER BY time_created ASC, id ASC",
        )?;
        let rows: Vec<MsgRec> = stmt
            .query_map(params![sid, since], |row| {
                Ok(MsgRec {
                    id: row.get(0)?,
                    time_created: row.get(1)?,
                    role: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    agent: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    model: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    completed: row.get(5)?,
                    input: row.get::<_, Option<u64>>(6)?.unwrap_or(0),
                    output: row.get::<_, Option<u64>>(7)?.unwrap_or(0),
                    reasoning: row.get::<_, Option<u64>>(8)?.unwrap_or(0),
                    cache_read: row.get::<_, Option<u64>>(9)?.unwrap_or(0),
                    cache_write: row.get::<_, Option<u64>>(10)?.unwrap_or(0),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);

        for msg in rows {
            let key = (msg.time_created, msg.id.clone());
            entry.messages.insert(key, msg.clone());
            entry.last_time_created = entry.last_time_created.max(msg.time_created);
            let parts = load_parts(conn, &msg.id)?;
            entry.parts_by_msg.insert(msg.id.clone(), parts);
        }
        Ok(())
    }
}

/// Compute vitals using a fresh (non-incremental) cache.
pub fn compute_vitals(
    conn: &Connection,
    primary: &str,
    children: &[String],
    is_working: bool,
    now_ms: i64,
) -> anyhow::Result<SessionVitals> {
    VitalsCache::new().compute(conn, primary, children, is_working, now_ms)
}

fn load_parts(conn: &Connection, message_id: &str) -> anyhow::Result<Vec<PartRec>> {
    let mut stmt = conn.prepare(
        "SELECT id, length(data),
                CASE WHEN length(data) < 262144 THEN json_extract(data, '$.type') ELSE NULL END,
                CASE WHEN length(data) < 262144 THEN json_extract(data, '$.tool') ELSE NULL END,
                CASE WHEN length(data) < 262144 THEN json_extract(data, '$.tail_start_id') ELSE NULL END,
                CASE WHEN length(data) < 262144 THEN json_extract(data, '$.state.time.start') ELSE NULL END,
                CASE WHEN length(data) < 262144 THEN json_extract(data, '$.state.time.end') ELSE NULL END,
                CASE WHEN length(data) < 262144 THEN json_extract(data, '$.state.time.compacted') ELSE NULL END,
                CASE WHEN length(data) < 262144 THEN COALESCE(length(json_extract(data, '$.text')), 0) ELSE 0 END,
                CASE WHEN length(data) < 262144 THEN COALESCE(length(json_extract(data, '$.state.input')), 0) ELSE 0 END,
                CASE WHEN length(data) < 262144 THEN COALESCE(length(json_extract(data, '$.state.output')), 0) ELSE 0 END,
                CASE WHEN length(data) < 262144 THEN json_extract(data, '$.state.input.subagent_type') ELSE NULL END,
                CASE WHEN length(data) >= 262144 THEN substr(data, 1, 512) ELSE NULL END
         FROM part
         WHERE message_id = ?1
         ORDER BY id ASC",
    )?;
    let rows = stmt
        .query_map(params![message_id], |row| {
            Ok((
                row.get::<_, u64>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<i64>>(5)?,
                row.get::<_, Option<i64>>(6)?,
                row.get::<_, Option<i64>>(7)?,
                row.get::<_, u64>(8)?,
                row.get::<_, u64>(9)?,
                row.get::<_, u64>(10)?,
                row.get::<_, Option<String>>(11)?,
                row.get::<_, Option<String>>(12)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    Ok(rows
        .into_iter()
        .map(
            |(
                byte_len,
                typ,
                tool,
                tail,
                ts,
                te,
                compacted_at,
                text_len,
                input_len,
                output_len,
                subagent_type,
                prefix,
            )| {
                let (mut typ, mut tool) = (typ.unwrap_or_default(), tool.unwrap_or_default());
                if byte_len >= BLOB_GUARD_BYTES as u64
                    && let Some(p) = prefix.as_deref()
                {
                    if typ.is_empty() {
                        typ = scan_prefix_field(p, "type").unwrap_or_default();
                    }
                    if tool.is_empty() {
                        tool = scan_prefix_field(p, "tool").unwrap_or_default();
                    }
                }
                let compacted = compacted_at.is_some();
                let visible_bytes = if byte_len >= BLOB_GUARD_BYTES as u64 {
                    match typ.as_str() {
                        "file" => 6_000,
                        "tool" if matches!(tool.as_str(), "apply_patch" | "edit" | "write") => {
                            16_000
                        }
                        "tool" | "text" => byte_len,
                        _ => 0,
                    }
                } else {
                    match typ.as_str() {
                        "text" => text_len,
                        "tool" => input_len + if compacted { 0 } else { output_len },
                        "file" => byte_len.min(6_000),
                        _ => 0,
                    }
                };
                PartRec {
                    typ,
                    tool,
                    subagent_type,
                    tail_start_id: tail,
                    t_start: ts,
                    t_end: te,
                    visible_bytes,
                }
            },
        )
        .collect())
}

/// Tiny `"key":"value"` scanner for the first 512 bytes of huge blobs.
fn scan_prefix_field(prefix: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":");
    let start = prefix.find(&needle)? + needle.len();
    let rest = prefix[start..].trim_start();
    if !rest.starts_with('"') {
        return None;
    }
    let rest = &rest[1..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn rollup(
    sessions: &HashMap<String, SessionRaw>,
    primary: &str,
    tree: &[String],
    is_working: bool,
    now_ms: i64,
) -> SessionVitals {
    // Tree-wide time rollups. Token/model bars are a composition of the
    // primary session's currently visible context, computed below.
    let mut by_agent: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_model: BTreeMap<String, u64> = BTreeMap::new();
    let mut tool_intervals: Vec<(i64, i64)> = Vec::new();
    let mut per_tool: BTreeMap<String, Vec<(i64, i64)>> = BTreeMap::new();
    let mut per_tool_count: BTreeMap<String, u32> = BTreeMap::new();
    let mut turn_intervals: Vec<(i64, i64)> = Vec::new();

    for sid in tree {
        let Some(raw) = sessions.get(sid) else {
            continue;
        };
        for msg in raw.messages.values() {
            if msg.role != "assistant" {
                continue;
            }
            if let Some(end) = msg.completed
                && end > msg.time_created
            {
                turn_intervals.push((msg.time_created, end));
            }
            if let Some(parts) = raw.parts_by_msg.get(&msg.id) {
                for p in parts {
                    if p.typ != "tool" || p.tool == "task" {
                        continue;
                    }
                    if let (Some(s), Some(e)) = (p.t_start, p.t_end) {
                        tool_intervals.push((s, e));
                        per_tool.entry(p.tool.clone()).or_default().push((s, e));
                        *per_tool_count.entry(p.tool.clone()).or_insert(0) += 1;
                    }
                }
            }
        }
    }

    // Primary-scoped cache / sparkline / context metrics.
    let empty_raw = SessionRaw::default();
    let primary_raw = sessions.get(primary).unwrap_or(&empty_raw);

    let mut cache_read_sum: u64 = 0;
    let mut denom_sum: u64 = 0;
    let mut uncached: u64 = 0;
    let mut last_completed: Option<i64> = None;
    let mut latest_context: u64 = 0;
    let mut primary_assistants: Vec<&MsgRec> = Vec::new();
    for msg in primary_raw.messages.values() {
        if msg.role != "assistant" {
            continue;
        }
        let denom = msg.input + msg.cache_read + msg.cache_write;
        cache_read_sum += msg.cache_read;
        denom_sum += denom;
        uncached += msg.input + msg.cache_write;
        if let Some(c) = msg.completed {
            last_completed = Some(last_completed.map_or(c, |l: i64| l.max(c)));
        }
        if msg.total() > 0 {
            latest_context = msg.total();
            primary_assistants.push(msg);
        }
    }
    let cache_hit_pct = if denom_sum > 0 {
        Some(((cache_read_sum as f64 * 100.0 / denom_sum as f64).round() as u8).min(100))
    } else {
        None
    };

    // Latest compaction tail boundary on the primary session.
    let mut tail_start: Option<(i64, String)> = None;
    for (key, msg) in primary_raw.messages.iter() {
        if let Some(parts) = primary_raw.parts_by_msg.get(&msg.id) {
            for p in parts {
                if p.typ == "compaction"
                    && let Some(tail) = p.tail_start_id.clone()
                {
                    let cand = (key.0, tail);
                    tail_start = Some(match tail_start {
                        None => cand,
                        Some(cur) => cur.max(cand),
                    });
                }
            }
        }
    }

    // Attribute only content that is still represented in this primary
    // context window. Repeatedly re-sent cached prefixes do not multiply the
    // apparent share of an agent/model across hundreds of turns.
    let mut active_agent = "?".to_string();
    let mut active_model = "?".to_string();
    let mut visible_context_tokens = 0u64;
    for (key, msg) in primary_raw.messages.iter() {
        if let Some((tail_time, tail_id)) = &tail_start
            && key < &(*tail_time, tail_id.clone())
        {
            continue;
        }
        if msg.role == "assistant" {
            if !msg.agent.is_empty() {
                active_agent.clone_from(&msg.agent);
            }
            if !msg.model.is_empty() {
                active_model = short_model_label(&msg.model);
            }
        }
        let Some(parts) = primary_raw.parts_by_msg.get(&msg.id) else {
            continue;
        };
        for part in parts {
            let tokens = part.visible_bytes / 4;
            if tokens == 0 {
                continue;
            }
            visible_context_tokens = visible_context_tokens.saturating_add(tokens);
            let agent = if part.tool == "task" {
                part.subagent_type
                    .clone()
                    .unwrap_or_else(|| active_agent.clone())
            } else {
                active_agent.clone()
            };
            *by_agent.entry(agent).or_insert(0) += tokens;
            *by_model.entry(active_model.clone()).or_insert(0) += tokens;
        }
    }
    if visible_context_tokens == 0 {
        // Older/minimal OpenCode DBs may have messages but no persisted parts.
        // Fall back to token records rather than rendering an empty dashboard.
        for sid in tree {
            if let Some(raw) = sessions.get(sid) {
                for msg in raw.messages.values().filter(|m| m.role == "assistant") {
                    let total = msg.total();
                    if total == 0 {
                        continue;
                    }
                    let agent = if msg.agent.is_empty() {
                        "?".to_string()
                    } else {
                        msg.agent.clone()
                    };
                    let model = if msg.model.is_empty() {
                        "?".to_string()
                    } else {
                        short_model_label(&msg.model)
                    };
                    *by_agent.entry(agent).or_insert(0) += total;
                    *by_model.entry(model).or_insert(0) += total;
                }
            }
        }
    }
    let first_prompt = primary_raw
        .messages
        .values()
        .find(|msg| msg.role == "assistant" && msg.input + msg.cache_read + msg.cache_write > 0);
    let first_prompt_tokens = first_prompt
        .map(|msg| msg.input + msg.cache_read + msg.cache_write)
        .unwrap_or(0);
    let first_prompt_key = first_prompt.map(|msg| (msg.time_created, msg.id.clone()));
    let user_context_before_first_call: u64 = first_prompt_key
        .as_ref()
        .map(|first_key| {
            primary_raw
                .messages
                .iter()
                .take_while(|(key, _)| *key <= first_key)
                .filter(|(_, msg)| msg.role == "user")
                .filter_map(|(_, msg)| primary_raw.parts_by_msg.get(&msg.id))
                .flat_map(|parts| parts.iter())
                .map(|part| part.visible_bytes / 4)
                .sum()
        })
        .unwrap_or(0);
    let system_ceiling = latest_context.saturating_sub(visible_context_tokens.min(latest_context));
    let system_tokens = if visible_context_tokens == 0 {
        0
    } else {
        first_prompt_tokens
            .saturating_sub(user_context_before_first_call)
            .min(system_ceiling)
    };
    let conversation_budget = latest_context.saturating_sub(system_tokens);
    let sparkline: Vec<SparkPoint> = primary_assistants
        .iter()
        .map(|m| {
            let denom = m.input + m.cache_read + m.cache_write;
            let ratio = if denom > 0 {
                m.cache_read as f32 / denom as f32
            } else {
                0.0
            };
            let compacted_before = tail_start
                .as_ref()
                .is_some_and(|(t, id)| (m.time_created, m.id.clone()) >= (*t, id.clone()));
            SparkPoint {
                total_context_tokens: m.total(),
                cache_read_tokens: m.cache_read,
                prompt_tokens: denom,
                cache_hit_ratio: ratio,
                compacted_before,
            }
        })
        .collect();

    // Context culprits since the compaction tail boundary (primary only).
    let mut culprits: BTreeMap<String, u64> = BTreeMap::new();
    for (key, msg) in primary_raw.messages.iter() {
        if let Some((t, id)) = &tail_start
            && key < &(*t, id.clone())
        {
            continue;
        }
        if let Some(parts) = primary_raw.parts_by_msg.get(&msg.id) {
            for p in parts {
                let bucket = if p.typ == "tool" {
                    if p.tool.is_empty() {
                        "tool".to_string()
                    } else {
                        p.tool.clone()
                    }
                } else if p.typ.is_empty() {
                    continue;
                } else {
                    match p.typ.as_str() {
                        "text" => "text".to_string(),
                        "reasoning" => "think".to_string(),
                        "file" => "file".to_string(),
                        other => other.to_string(),
                    }
                };
                *culprits.entry(bucket).or_insert(0) += p.visible_bytes / 4;
            }
        }
    }
    let context_culprits = if visible_context_tokens == 0 && latest_context > 0 {
        // Parts are absent in older/minimal databases; preserve the old
        // unknown-content fallback, but keep it in the context section only.
        culprits.insert("system".to_string(), latest_context);
        sorted_bars(culprits)
    } else {
        let mut scaled: BTreeMap<String, u64> = normalize_bars(culprits, conversation_budget)
            .into_iter()
            .map(|bar| (bar.label, bar.value))
            .collect();
        if system_tokens > 0 {
            scaled.insert("system".to_string(), system_tokens);
        }
        sorted_bars(scaled)
    };

    // Wall-clock time: merge tool spans; LLM time = turn window minus overlap.
    let tool_wall_ms = merge_intervals_ms(&mut tool_intervals);
    let turn_wall_ms = merge_intervals_ms(&mut turn_intervals.clone());
    let overlap = interval_intersection_ms(&tool_intervals, &turn_intervals);
    let llm_wall_ms = turn_wall_ms.saturating_sub(overlap);

    let mut tools_by_time: Vec<MetricBar> = per_tool
        .into_iter()
        .map(|(label, mut spans)| MetricBar {
            value: merge_intervals_ms(&mut spans),
            count: per_tool_count.get(&label).copied(),
            label,
        })
        .collect();
    tools_by_time.sort_by(|a, b| b.value.cmp(&a.value).then_with(|| a.label.cmp(&b.label)));

    // Agent/model allocations describe conversation content only; the system
    // baseline appears solely in the context section.
    let latest_context_tokens = latest_context;
    let tokens_by_agent = normalize_bars(by_agent, conversation_budget);
    let tokens_by_model = normalize_bars(by_model, conversation_budget);

    SessionVitals {
        session_id: primary.to_string(),
        cache_hit_pct,
        warmth: classify_warmth(is_working, last_completed, now_ms),
        uncached_input_tokens: uncached,
        latest_context_tokens,
        turn_count: primary_assistants.len(),
        sparkline,
        tokens_by_agent,
        tokens_by_model,
        context_culprits,
        llm_wall_ms,
        tool_wall_ms,
        tools_by_time,
    }
}

fn normalize_bars(map: BTreeMap<String, u64>, budget: u64) -> Vec<MetricBar> {
    let sum: u128 = map.values().map(|v| *v as u128).sum();
    if sum == 0 || budget == 0 {
        return sorted_bars(map);
    }
    let mut values: Vec<(String, u64)> = map
        .into_iter()
        .map(|(label, value)| {
            let scaled = ((value as u128 * budget as u128) / sum).min(u64::MAX as u128) as u64;
            (label, scaled)
        })
        .collect();
    // Allocate integer-division remainder deterministically to the largest
    // original group so all rows sum exactly to the context budget.
    let allocated: u64 = values.iter().map(|(_, v)| *v).sum();
    if allocated < budget
        && let Some((_, value)) = values.iter_mut().max_by_key(|(_, v)| *v)
    {
        *value = value.saturating_add(budget - allocated);
    }
    sorted_bars(values.into_iter().collect())
}

fn sorted_bars(map: BTreeMap<String, u64>) -> Vec<MetricBar> {
    let mut bars: Vec<MetricBar> = map
        .into_iter()
        .map(|(label, value)| MetricBar {
            label,
            value,
            count: None,
        })
        .collect();
    bars.sort_by(|a, b| b.value.cmp(&a.value).then_with(|| a.label.cmp(&b.label)));
    bars
}

/// Milliseconds covered by both interval sets (after merging each set).
fn interval_intersection_ms(a: &[(i64, i64)], b: &[(i64, i64)]) -> u64 {
    fn merged(spans: &[(i64, i64)]) -> Vec<(i64, i64)> {
        let mut v: Vec<(i64, i64)> = spans.iter().copied().filter(|(s, e)| e > s).collect();
        v.sort_by_key(|s| s.0);
        let mut out: Vec<(i64, i64)> = Vec::new();
        for (s, e) in v {
            if let Some(last) = out.last_mut()
                && s <= last.1
            {
                last.1 = last.1.max(e);
                continue;
            }
            out.push((s, e));
        }
        out
    }
    let ma = merged(a);
    let mb = merged(b);
    let mut total: i64 = 0;
    let (mut i, mut j) = (0usize, 0usize);
    while i < ma.len() && j < mb.len() {
        let lo = ma[i].0.max(mb[j].0);
        let hi = ma[i].1.min(mb[j].1);
        if hi > lo {
            total += hi - lo;
        }
        if ma[i].1 < mb[j].1 {
            i += 1;
        } else {
            j += 1;
        }
    }
    total.max(0) as u64
}

// Keep OptionalExtension referenced for future time_updated-aware refreshes.
#[allow(dead_code)]
fn _unused(_: &Connection) {}

/// Request for one vitals computation on the background worker.
#[derive(Debug, Clone)]
pub struct VitalsRequest {
    pub primary_session_id: String,
    pub child_session_ids: Vec<String>,
    pub is_working: bool,
}

enum WorkerMsg {
    Compute(VitalsRequest),
    Shutdown,
}

/// Background SQLite worker for session vitals.
///
/// The worker coalesces bursts of requests down to the latest pending one, so
/// rapid session switches never queue stale work. All failures (missing DB,
/// bad schema) are swallowed — the UI simply keeps the last snapshot.
pub struct VitalsWorker {
    req_tx: Sender<WorkerMsg>,
    resp_rx: Receiver<SessionVitals>,
    handle: Option<JoinHandle<()>>,
}

impl VitalsWorker {
    pub fn spawn(db_path: PathBuf) -> Self {
        let (req_tx, req_rx) = channel::<WorkerMsg>();
        let (resp_tx, resp_rx) = channel::<SessionVitals>();
        let handle = thread::spawn(move || {
            let conn = Connection::open_with_flags(
                &db_path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
            );
            let mut cache = VitalsCache::new();
            while let Ok(msg) = req_rx.recv() {
                match msg {
                    WorkerMsg::Shutdown => break,
                    WorkerMsg::Compute(req) => {
                        // Coalesce: drain anything newer already queued.
                        let mut latest = req;
                        while let Ok(WorkerMsg::Compute(newer)) = req_rx.try_recv() {
                            latest = newer;
                        }
                        let now_ms = SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as i64)
                            .unwrap_or(0);
                        if let Ok(conn) = conn.as_ref() {
                            let _ = conn.busy_timeout(std::time::Duration::from_secs(2));
                            if let Ok(v) = cache.compute(
                                conn,
                                &latest.primary_session_id,
                                &latest.child_session_ids,
                                latest.is_working,
                                now_ms,
                            ) {
                                let _ = resp_tx.send(v);
                            }
                        }
                    }
                }
            }
        });
        Self {
            req_tx,
            resp_rx,
            handle: Some(handle),
        }
    }

    /// Enqueue a request (drops silently if the worker has exited).
    pub fn request(&self, req: VitalsRequest) {
        let _ = self.req_tx.send(WorkerMsg::Compute(req));
    }

    /// Drain the most recent available vitals snapshot, if any.
    pub fn try_recv(&self) -> Option<SessionVitals> {
        let mut latest = None;
        while let Ok(v) = self.resp_rx.try_recv() {
            latest = Some(v);
        }
        latest
    }
}

impl Drop for VitalsWorker {
    fn drop(&mut self) {
        let _ = self.req_tx.send(WorkerMsg::Shutdown);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_tokens_scales_across_units() {
        assert_eq!(format_compact_tokens(0), "0");
        assert_eq!(format_compact_tokens(842), "842");
        assert_eq!(format_compact_tokens(14_240), "14.2k");
        assert_eq!(format_compact_tokens(372_400), "372k");
        assert_eq!(format_compact_tokens(2_460_000), "2.5M");
    }

    #[test]
    fn compact_duration_formats_seconds_minutes_hours() {
        assert_eq!(format_compact_duration(0), "0s");
        assert_eq!(format_compact_duration(400), "<1s");
        assert_eq!(format_compact_duration(18_200), "18s");
        assert_eq!(format_compact_duration(100_000), "1m40s");
        assert_eq!(format_compact_duration(3_900_000), "1h05m");
    }

    #[test]
    fn warmth_transitions_from_live_to_warm_cooling_and_cold() {
        let now = 1_000_000_000i64;
        assert_eq!(
            classify_warmth(true, Some(now - 600_000), now),
            CacheWarmth::Live
        );
        assert_eq!(classify_warmth(false, None, now), CacheWarmth::Empty);
        assert_eq!(
            classify_warmth(false, Some(now - 65_000), now),
            CacheWarmth::Warm(65)
        );
        assert_eq!(
            classify_warmth(false, Some(now - 250_000), now),
            CacheWarmth::Cooling(250)
        );
        assert_eq!(
            classify_warmth(false, Some(now - 305_000), now),
            CacheWarmth::Cold(305)
        );
    }

    #[test]
    fn merge_intervals_collapses_parallel_overlaps() {
        let mut spans = vec![(1000, 4000), (1500, 3500), (3800, 5000), (7000, 8000)];
        assert_eq!(merge_intervals_ms(&mut spans), 5000);
    }

    #[test]
    fn worker_round_trip_returns_vitals_for_primary_session() {
        use std::path::PathBuf;
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path: PathBuf = std::env::temp_dir().join(format!("ocmux-rs-vitals-worker-{nanos}.db"));
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE session (id TEXT PRIMARY KEY, project_id TEXT NOT NULL, parent_id TEXT, title TEXT, directory TEXT, permission TEXT, time_created INTEGER, time_updated INTEGER, time_archived INTEGER);
                 CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, data TEXT NOT NULL, time_created INTEGER);
                 CREATE TABLE part (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, message_id TEXT NOT NULL, data TEXT NOT NULL, time_created INTEGER);
                 INSERT INTO session VALUES ('s1', 'p1', NULL, 'T', '/tmp', NULL, 1, 1, NULL);",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO message VALUES ('m1', 's1', '{\"role\":\"assistant\",\"agent\":\"build\",\"modelID\":\"gpt-6-luna\",\"tokens\":{\"input\":10,\"output\":10,\"cache\":{\"read\":80}}}', 1000)",
                [],
            )
            .unwrap();
        }
        let worker = VitalsWorker::spawn(path.clone());
        worker.request(VitalsRequest {
            primary_session_id: "s1".to_string(),
            child_session_ids: vec![],
            is_working: false,
        });
        let mut got = None;
        for _ in 0..100 {
            if let Some(v) = worker.try_recv() {
                got = Some(v);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let v = got.expect("worker must respond");
        assert_eq!(v.session_id, "s1");
        assert_eq!(v.turn_count, 1);
        assert_eq!(v.cache_hit_pct, Some(89));
        drop(worker);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn worker_coalesces_rapid_requests_to_latest() {
        use std::path::PathBuf;
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path: PathBuf =
            std::env::temp_dir().join(format!("ocmux-rs-vitals-coalesce-{nanos}.db"));
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE session (id TEXT PRIMARY KEY, project_id TEXT NOT NULL, parent_id TEXT, title TEXT, directory TEXT, permission TEXT, time_created INTEGER, time_updated INTEGER, time_archived INTEGER);
                 CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, data TEXT NOT NULL, time_created INTEGER);
                 CREATE TABLE part (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, message_id TEXT NOT NULL, data TEXT NOT NULL, time_created INTEGER);
                 INSERT INTO session VALUES ('s1', 'p1', NULL, 'T', '/tmp', NULL, 1, 1, NULL);
                 INSERT INTO session VALUES ('s2', 'p1', NULL, 'T', '/tmp', NULL, 1, 1, NULL);",
            )
            .unwrap();
        }
        let worker = VitalsWorker::spawn(path.clone());
        for sid in ["s1", "s1", "s1", "s2"] {
            worker.request(VitalsRequest {
                primary_session_id: sid.to_string(),
                child_session_ids: vec![],
                is_working: false,
            });
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
        let mut last = None;
        while let Some(v) = worker.try_recv() {
            last = Some(v.session_id);
        }
        assert_eq!(last.as_deref(), Some("s2"), "latest pending request wins");
        drop(worker);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn short_model_strips_provider_and_date_stamp() {
        assert_eq!(
            short_model_label("google/gemini-4-carbon"),
            "gemini-4-carbon"
        );
        assert_eq!(
            short_model_label("anthropic/claude-sonnet-4-20250514"),
            "claude-sonnet-4"
        );
        assert_eq!(short_model_label("gpt-6-luna"), "gpt-6-luna");
    }
}
