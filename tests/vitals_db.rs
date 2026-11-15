use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use opencode_multiplexer::data::vitals::{CacheWarmth, VitalsCache, compute_vitals};
use rusqlite::{Connection, params};

fn temp_db_path(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("ocmux-rs-vitals-{label}-{nanos}.db"))
}

fn init_db(path: &PathBuf) -> Connection {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        r#"
        CREATE TABLE project (id TEXT PRIMARY KEY, worktree TEXT NOT NULL, name TEXT, time_created INTEGER, time_updated INTEGER);
        CREATE TABLE session (id TEXT PRIMARY KEY, project_id TEXT NOT NULL, parent_id TEXT, title TEXT, directory TEXT, permission TEXT, time_created INTEGER, time_updated INTEGER, time_archived INTEGER);
        CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, data TEXT NOT NULL, time_created INTEGER);
        CREATE TABLE part (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, message_id TEXT NOT NULL, data TEXT NOT NULL, time_created INTEGER);
        "#,
    )
    .unwrap();
    conn.execute(
        "INSERT INTO project VALUES ('p1', '/tmp/proj', 'proj', 100, 200)",
        [],
    )
    .unwrap();
    conn
}

fn add_session(conn: &Connection, id: &str, parent: Option<&str>) {
    conn.execute(
        "INSERT INTO session VALUES (?1, 'p1', ?2, 'Title', '/tmp/proj', NULL, 100, 200, NULL)",
        params![id, parent],
    )
    .unwrap();
}

fn add_msg(conn: &Connection, id: &str, sid: &str, data: &str, t: i64) {
    conn.execute(
        "INSERT INTO message VALUES (?1, ?2, ?3, ?4)",
        params![id, sid, data, t],
    )
    .unwrap();
}

fn add_part(conn: &Connection, id: &str, sid: &str, mid: &str, data: String, t: i64) {
    conn.execute(
        "INSERT INTO part VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, sid, mid, data, t],
    )
    .unwrap();
}

fn assistant(
    agent: &str,
    model: &str,
    input: u64,
    output: u64,
    reasoning: u64,
    cr: u64,
    cw: u64,
) -> String {
    format!(
        r#"{{"role":"assistant","agent":"{agent}","modelID":"{model}","tokens":{{"input":{input},"output":{output},"reasoning":{reasoning},"cache":{{"read":{cr},"write":{cw}}}}}}}"#
    )
}

#[test]
fn aggregates_tokens_by_agent_and_model_across_subagent_tree_only_sparkline_from_primary() {
    let path = temp_db_path("tree");
    let conn = init_db(&path);
    add_session(&conn, "parent", None);
    add_session(&conn, "child", Some("parent"));
    add_session(&conn, "unrelated", None);
    // Parent: build/gpt-6-luna, mostly cached.
    add_msg(
        &conn,
        "m1",
        "parent",
        &assistant("build", "gpt-6-luna", 100, 50, 0, 900, 0),
        1000,
    );
    // Child subagent: explorer/gemini-4-carbon.
    add_msg(
        &conn,
        "m2",
        "child",
        &assistant("explorer", "gemini-4-carbon", 200, 100, 0, 700, 0),
        2000,
    );
    // Unrelated session must never contribute.
    add_msg(
        &conn,
        "m3",
        "unrelated",
        &assistant("build", "gpt-6-luna", 9999, 9999, 0, 9999, 0),
        3000,
    );

    let v = compute_vitals(&conn, "parent", &["child".to_string()], false, 6000).unwrap();
    assert_eq!(v.session_id, "parent");
    // Tree-wide allocations are shares of the current primary context (1050),
    // proportional to total tokens in each agent's turns.
    let build = v
        .tokens_by_agent
        .iter()
        .find(|b| b.label == "build")
        .unwrap();
    assert!(
        (530..=545).contains(&build.value),
        "build share: {}",
        build.value
    );
    let explorer = v
        .tokens_by_agent
        .iter()
        .find(|b| b.label == "explorer")
        .unwrap();
    assert!(
        (505..=520).contains(&explorer.value),
        "explorer share: {}",
        explorer.value
    );
    assert_eq!(build.value + explorer.value, 1050);
    assert_eq!(
        v.tokens_by_model
            .iter()
            .map(|b| b.label.as_str())
            .collect::<Vec<_>>(),
        vec!["gpt-6-luna", "gemini-4-carbon"]
    );
    // Primary-only: sparkline has just the parent turn, cache 900/1000 = 90%.
    assert_eq!(v.sparkline.len(), 1);
    assert_eq!(v.cache_hit_pct, Some(90));
    assert_eq!(v.uncached_input_tokens, 100);
    assert_eq!(v.latest_context_tokens, 1050);
    std::fs::remove_file(path).ok();
}

#[test]
fn marks_compactions_on_sparkline_and_scopes_context_culprits_after_tail_start() {
    let path = temp_db_path("compact");
    let conn = init_db(&path);
    add_session(&conn, "s1", None);
    add_msg(
        &conn,
        "m1",
        "s1",
        &assistant("build", "gpt-6-luna", 100, 10, 0, 900, 0),
        1000,
    );
    add_msg(
        &conn,
        "m2",
        "s1",
        &assistant("build", "gpt-6-luna", 100, 10, 0, 900, 0),
        2000,
    );
    add_msg(
        &conn,
        "m3",
        "s1",
        &assistant("build", "gpt-6-luna", 100, 10, 0, 900, 0),
        3000,
    );
    // Big pre-compaction read part on m1 (must be excluded from culprits).
    let pre_read = format!(
        r#"{{"type":"tool","tool":"read","state":{{"output":"{}"}}}}"#,
        "x".repeat(39_900)
    );
    add_part(&conn, "p1", "s1", "m1", pre_read, 1100);
    // Compaction recorded on m2's parts, tail starts at m3.
    add_part(
        &conn,
        "p2",
        "s1",
        "m2",
        r#"{"type":"compaction","auto":true,"tail_start_id":"m3"}"#.to_string(),
        2100,
    );
    // Post-compaction read part on m3 (8_000 bytes -> ~2_000 est tokens).
    let post_read = format!(
        r#"{{"type":"tool","tool":"read","state":{{"output":"{}"}}}}"#,
        "y".repeat(7_900)
    );
    add_part(&conn, "p3", "s1", "m3", post_read, 3100);

    let v = compute_vitals(&conn, "s1", &[], false, 4000).unwrap();
    assert_eq!(v.sparkline.len(), 3);
    assert!(!v.sparkline[0].compacted_before);
    assert!(
        v.sparkline[2].compacted_before,
        "first turn after tail_start must be marked"
    );
    let read = v
        .context_culprits
        .iter()
        .find(|b| b.label == "read")
        .expect("post-compaction read must be counted");
    assert!(
        read.value > 1_000 && read.value <= 2_100,
        "got {}",
        read.value
    );
    std::fs::remove_file(path).ok();
}

#[test]
fn computes_parallel_tool_and_llm_wall_time_without_double_counting_task_wrapper() {
    let path = temp_db_path("time");
    let conn = init_db(&path);
    add_session(&conn, "s1", None);
    // Assistant turn spans 1000..9000 with completed=9000.
    let turn = r#"{"role":"assistant","agent":"build","modelID":"gpt-6-luna","tokens":{"input":10,"output":10,"cache":{"read":80}},"time":{"completed":9000}}"#;
    add_msg(&conn, "m1", "s1", turn, 1000);
    // Two parallel bash tools: 1000..4000 and 1500..5000 -> merged 4000ms.
    add_part(
        &conn,
        "p1",
        "s1",
        "m1",
        r#"{"type":"tool","tool":"bash","state":{"time":{"start":1000,"end":4000}}}"#.to_string(),
        1100,
    );
    add_part(
        &conn,
        "p2",
        "s1",
        "m1",
        r#"{"type":"tool","tool":"bash","state":{"time":{"start":1500,"end":5000}}}"#.to_string(),
        1600,
    );
    // task wrapper must be excluded from tool time.
    add_part(
        &conn,
        "p3",
        "s1",
        "m1",
        r#"{"type":"tool","tool":"task","state":{"time":{"start":1000,"end":9000}}}"#.to_string(),
        1700,
    );

    let v = compute_vitals(&conn, "s1", &[], false, 10_000).unwrap();
    assert_eq!(v.tool_wall_ms, 4000);
    // Turn window is 8000ms, tools covered 4000ms of it.
    assert_eq!(v.llm_wall_ms, 4000);
    let bash = v.tools_by_time.iter().find(|b| b.label == "bash").unwrap();
    assert_eq!(bash.value, 4000);
    assert_eq!(bash.count, Some(2));
    assert!(v.tools_by_time.iter().all(|b| b.label != "task"));
    std::fs::remove_file(path).ok();
}

#[test]
fn handles_multi_megabyte_part_blobs_via_prefix_header_without_full_json_parse() {
    let path = temp_db_path("blob");
    let conn = init_db(&path);
    add_session(&conn, "s1", None);
    add_msg(
        &conn,
        "m1",
        "s1",
        &assistant("build", "gpt-6-luna", 10, 10, 0, 80, 0),
        1000,
    );
    // >64KiB blob whose header (type/tool) sits in the first 512 bytes.
    let mut blob = String::from(
        r#"{"type":"tool","tool":"playwright_browser_take_screenshot","state":{"output":""#,
    );
    blob.push_str(&"z".repeat(300_000));
    blob.push_str(r#""}}"#);
    assert!(blob.len() >= 65_536);
    add_part(&conn, "p1", "s1", "m1", blob, 1100);

    let v = compute_vitals(&conn, "s1", &[], false, 2000).unwrap();
    let shot = v
        .context_culprits
        .iter()
        .find(|b| b.label == "playwright_browser_take_screenshot")
        .expect("blob culprit must be attributed via prefix");
    assert_eq!(
        shot.value, v.latest_context_tokens,
        "single visible blob gets the whole context share"
    );
    std::fs::remove_file(path).ok();
}

#[test]
fn incremental_cache_reuses_unchanged_messages_and_picks_up_new_turns() {
    let path = temp_db_path("incr");
    let conn = init_db(&path);
    add_session(&conn, "s1", None);
    add_msg(
        &conn,
        "m1",
        "s1",
        &assistant("build", "gpt-6-luna", 100, 50, 0, 850, 0),
        1000,
    );
    let mut cache = VitalsCache::new();
    let v1 = cache.compute(&conn, "s1", &[], false, 2000).unwrap();
    assert_eq!(v1.turn_count, 1);
    assert_eq!(v1.latest_context_tokens, 1000);

    // Append a new turn; incremental recompute must pick it up.
    add_msg(
        &conn,
        "m2",
        "s1",
        &assistant("build", "gpt-6-luna", 200, 100, 0, 1700, 0),
        3000,
    );
    let v2 = cache.compute(&conn, "s1", &[], true, 4000).unwrap();
    assert_eq!(v2.turn_count, 2);
    assert_eq!(v2.latest_context_tokens, 2000);
    assert_eq!(v2.warmth, CacheWarmth::Live);
    std::fs::remove_file(path).ok();
}

#[test]
fn apply_patch_metadata_snapshots_do_not_inflate_context_breakdown() {
    let path = temp_db_path("patch-metadata");
    let conn = init_db(&path);
    add_session(&conn, "s1", None);
    let msg = r#"{"role":"assistant","agent":"build","modelID":"gpt-6-luna","tokens":{"total":10000,"input":2000,"output":0,"reasoning":0,"cache":{"read":8000,"write":0}}}"#;
    add_msg(&conn, "m1", "s1", msg, 1000);
    let patch = format!(
        r#"{{"type":"tool","tool":"apply_patch","state":{{"input":{{"patch":"{}"}},"output":"{}","metadata":{{"before":"{}","after":"{}"}}}}}}"#,
        "i".repeat(800),
        "o".repeat(200),
        "b".repeat(60_000),
        "a".repeat(60_000),
    );
    add_part(&conn, "p1", "s1", "m1", patch, 1100);
    add_part(
        &conn,
        "p2",
        "s1",
        "m1",
        format!(
            r#"{{"type":"tool","tool":"bash","state":{{"input":{{"command":"test"}},"output":"{}"}}}}"#,
            "b".repeat(9_000)
        ),
        1200,
    );

    let v = compute_vitals(&conn, "s1", &[], false, 2000).unwrap();
    let apply_patch = v
        .context_culprits
        .iter()
        .find(|b| b.label == "apply_patch")
        .unwrap();
    assert!(
        apply_patch.value < 2_000,
        "metadata must not count as context: {}",
        apply_patch.value
    );
    assert!(v.context_culprits.iter().map(|b| b.value).sum::<u64>() <= 10_000);
    std::fs::remove_file(path).ok();
}

#[test]
fn compacted_tool_outputs_are_excluded_from_active_context() {
    let path = temp_db_path("compacted-output");
    let conn = init_db(&path);
    add_session(&conn, "s1", None);
    let msg = r#"{"role":"assistant","agent":"build","modelID":"gpt-6-luna","tokens":{"total":10000,"input":1000,"output":0,"reasoning":0,"cache":{"read":9000,"write":0}}}"#;
    add_msg(&conn, "m1", "s1", msg, 1000);
    add_part(
        &conn,
        "p1",
        "s1",
        "m1",
        format!(
            r#"{{"type":"tool","tool":"read","state":{{"time":{{"start":1,"end":2,"compacted":3}},"input":{{"path":"old"}},"output":"{}"}}}}"#,
            "x".repeat(40_000)
        ),
        1100,
    );
    add_part(
        &conn,
        "p2",
        "s1",
        "m1",
        format!(
            r#"{{"type":"tool","tool":"read","state":{{"time":{{"start":4,"end":5}},"input":{{"path":"new"}},"output":"{}"}}}}"#,
            "y".repeat(4_000)
        ),
        1200,
    );
    add_part(
        &conn,
        "p3",
        "s1",
        "m1",
        format!(
            r#"{{"type":"tool","tool":"bash","state":{{"input":{{"command":"test"}},"output":"{}"}}}}"#,
            "z".repeat(4_000)
        ),
        1300,
    );

    let v = compute_vitals(&conn, "s1", &[], false, 2000).unwrap();
    let read = v
        .context_culprits
        .iter()
        .find(|b| b.label == "read")
        .unwrap();
    let bash = v
        .context_culprits
        .iter()
        .find(|b| b.label == "bash")
        .unwrap();
    assert!(
        read.value < 6_000,
        "compacted read output should be excluded: {}",
        read.value
    );
    assert!(
        read.value.abs_diff(bash.value) < 100,
        "compacted read output should leave input bytes only: read={} bash={}",
        read.value,
        bash.value
    );
    assert!(bash.value > 0);
    assert!(v.context_culprits.iter().map(|b| b.value).sum::<u64>() <= v.latest_context_tokens);
    std::fs::remove_file(path).ok();
}

#[test]
fn category_estimates_are_context_window_shares_not_cumulative_token_totals() {
    let path = temp_db_path("context-share");
    let conn = init_db(&path);
    add_session(&conn, "s1", None);
    for i in 0..10 {
        let (agent, model) = if i < 5 {
            ("chat", "claude-opus-5-5")
        } else {
            ("build", "gpt-6-luna")
        };
        let tokens = format!(
            r#"{{"role":"assistant","agent":"{agent}","modelID":"{model}","tokens":{{"total":500000,"input":10000,"output":0,"reasoning":0,"cache":{{"read":70000,"write":0}}}}}}"#
        );
        let id = format!("m{i}");
        add_msg(&conn, &id, "s1", &tokens, 1000 + i * 1000);
        add_part(
            &conn,
            &format!("p{i}"),
            "s1",
            &id,
            format!(
                r#"{{"type":"tool","tool":"read","state":{{"input":{{"path":"x"}},"output":"{}"}}}}"#,
                "x".repeat(4_000)
            ),
            1100 + i * 1000,
        );
    }
    let v = compute_vitals(&conn, "s1", &[], false, 20_000).unwrap();
    assert_eq!(v.latest_context_tokens, 80_000);
    assert!(v.tokens_by_agent.iter().map(|b| b.value).sum::<u64>() <= v.latest_context_tokens);
    assert!(v.tokens_by_model.iter().map(|b| b.value).sum::<u64>() <= v.latest_context_tokens);
    assert!(v.context_culprits.iter().map(|b| b.value).sum::<u64>() <= v.latest_context_tokens);
    let build = v
        .tokens_by_agent
        .iter()
        .find(|b| b.label == "build")
        .unwrap();
    assert!(
        (4_500..=5_500).contains(&build.value),
        "visible build contribution should be ~5k tokens, not cumulative call totals: {}",
        build.value
    );
    std::fs::remove_file(path).ok();
}

#[test]
fn system_overhead_comes_from_first_turn_baseline_not_residual() {
    let path = temp_db_path("system-baseline");
    let conn = init_db(&path);
    add_session(&conn, "s1", None);
    let first = r#"{"role":"assistant","agent":"build","modelID":"gpt-6-luna","tokens":{"total":40000,"input":40000,"output":0,"reasoning":0,"cache":{"read":0,"write":0}}}"#;
    add_msg(&conn, "m1", "s1", first, 1000);
    add_part(
        &conn,
        "p1",
        "s1",
        "m1",
        format!(r#"{{"type":"text","text":"{}"}}"#, "a".repeat(4000)),
        1100,
    );

    let latest = r#"{"role":"assistant","agent":"build","modelID":"gpt-6-luna","tokens":{"total":100000,"input":20000,"output":0,"reasoning":0,"cache":{"read":80000,"write":0}}}"#;
    add_msg(&conn, "m2", "s1", latest, 2000);
    add_part(
        &conn,
        "p2",
        "s1",
        "m2",
        format!(
            r#"{{"type":"tool","tool":"bash","state":{{"input":{{"command":"x"}},"output":"{}"}}}}"#,
            "b".repeat(80_000)
        ),
        2100,
    );

    let v = compute_vitals(&conn, "s1", &[], false, 3000).unwrap();
    assert_eq!(v.latest_context_tokens, 100_000);
    let system = v
        .context_culprits
        .iter()
        .find(|b| b.label == "system")
        .unwrap();
    assert!(
        (38_000..=40_000).contains(&system.value),
        "turn-one fixed overhead estimate: {}",
        system.value
    );
    assert!(!v.context_culprits.iter().any(|b| b.label == "system/other"));
    assert_eq!(
        v.context_culprits.iter().map(|b| b.value).sum::<u64>(),
        100_000
    );
    assert!((60_000..=62_000).contains(&v.tokens_by_agent.iter().map(|b| b.value).sum::<u64>()));
    std::fs::remove_file(path).ok();
}

#[test]
fn agent_and_model_bars_exclude_system_and_scale_to_conversation_budget() {
    let path = temp_db_path("system-not-agent");
    let conn = init_db(&path);
    add_session(&conn, "s1", None);
    let first = r#"{"role":"assistant","agent":"chat","modelID":"claude-opus-5-5","tokens":{"total":40000,"input":40000,"cache":{"read":0,"write":0}}}"#;
    add_msg(&conn, "m1", "s1", first, 1000);
    add_part(
        &conn,
        "p1",
        "s1",
        "m1",
        format!(r#"{{"type":"text","text":"{}"}}"#, "u".repeat(4000)),
        1100,
    );
    let latest = r#"{"role":"assistant","agent":"build","modelID":"gpt-6-luna","tokens":{"total":100000,"input":20000,"cache":{"read":80000,"write":0}}}"#;
    add_msg(&conn, "m2", "s1", latest, 2000);
    add_part(
        &conn,
        "p2",
        "s1",
        "m2",
        format!(
            r#"{{"type":"tool","tool":"read","state":{{"input":{{"path":"x"}},"output":"{}"}}}}"#,
            "r".repeat(80_000)
        ),
        2100,
    );

    let v = compute_vitals(&conn, "s1", &[], false, 3000).unwrap();
    assert!(
        v.tokens_by_agent
            .iter()
            .all(|b| b.label != "system" && b.label != "system/other")
    );
    assert!(
        v.tokens_by_model
            .iter()
            .all(|b| b.label != "system" && b.label != "system/other")
    );
    let system = v
        .context_culprits
        .iter()
        .find(|b| b.label == "system")
        .unwrap()
        .value;
    let conversation_budget = v.latest_context_tokens - system;
    assert_eq!(
        v.tokens_by_agent.iter().map(|b| b.value).sum::<u64>(),
        conversation_budget
    );
    assert_eq!(
        v.tokens_by_model.iter().map(|b| b.value).sum::<u64>(),
        conversation_budget
    );
    std::fs::remove_file(path).ok();
}

#[test]
fn system_baseline_is_clamped_so_visible_content_keeps_its_share() {
    let path = temp_db_path("system-clamp");
    let conn = init_db(&path);
    add_session(&conn, "s1", None);
    for i in 0..4 {
        let msg = format!(
            r#"{{"role":"assistant","agent":"build","modelID":"gpt-6-luna","tokens":{{"total":80000,"input":80000,"cache":{{"read":0,"write":0}}}}}}"#
        );
        let id = format!("m{i}");
        add_msg(&conn, &id, "s1", &msg, 1000 + i * 1000);
        if i == 0 {
            add_part(
                &conn,
                "p1",
                "s1",
                &id,
                format!(r#"{{"type":"text","text":"{}"}}"#, "x".repeat(40_000)),
                1100,
            );
        }
    }
    let v = compute_vitals(&conn, "s1", &[], false, 6000).unwrap();
    let system = v
        .context_culprits
        .iter()
        .find(|b| b.label == "system")
        .unwrap()
        .value;
    assert!(
        system <= 70_000,
        "baseline cannot consume the visible-content budget: {system}"
    );
    assert!(v.tokens_by_agent.iter().map(|b| b.value).sum::<u64>() >= 10_000);
    std::fs::remove_file(path).ok();
}
