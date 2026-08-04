use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use opencode_multiplexer::{app::sessions::SessionStatus, data::db::reader::DbReader};
use rusqlite::{Connection, params};

#[test]
fn reads_projects_and_most_recent_session() {
    let db_path = temp_db_path("projects");
    let conn = init_db(&db_path);

    conn.execute(
        "INSERT INTO project (id, worktree, name, time_created, time_updated) VALUES (?1, ?2, 'repo', 1, 2)",
        params!["proj_1", "/tmp/repo"],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, title, directory, permission, time_created, time_updated, time_archived) VALUES (?1, ?2, NULL, 'title', '/tmp/repo', '{}', 1, 10, NULL)",
        params!["sess_1", "proj_1"],
    )
    .unwrap();

    let reader = DbReader::open(&db_path).unwrap();
    let projects = reader.get_projects().unwrap();
    let session = reader
        .get_most_recent_session("proj_1", 0)
        .unwrap()
        .unwrap();

    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].worktree, PathBuf::from("/tmp/repo"));
    assert_eq!(session.id, "sess_1");

    fs::remove_file(db_path).ok();
}

#[test]
fn session_status_prefers_needs_input_over_other_states() {
    let db_path = temp_db_path("status");
    let conn = init_db(&db_path);

    conn.execute(
        "INSERT INTO project (id, worktree, name, time_created, time_updated) VALUES ('proj_1', '/tmp/repo', 'repo', 1, 2)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, title, directory, permission, time_created, time_updated, time_archived) VALUES ('sess_1', 'proj_1', NULL, 'title', '/tmp/repo', '{}', 1, 10, NULL)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, data, time_created) VALUES ('msg_1', 'sess_1', '{\"role\":\"assistant\",\"time\":{\"completed\":false}}', 1)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO part (id, session_id, message_id, data, time_created) VALUES ('part_1', 'sess_1', 'msg_1', '{\"type\":\"tool\",\"tool\":\"question\",\"state\":{\"status\":\"running\"}}', 1)",
        [],
    )
    .unwrap();

    let reader = DbReader::open(&db_path).unwrap();

    assert_eq!(
        reader.get_session_status("sess_1", None).unwrap(),
        SessionStatus::NeedsInput
    );

    fs::remove_file(db_path).ok();
}

#[test]
fn reads_model_and_last_message_preview() {
    let db_path = temp_db_path("preview");
    let conn = init_db(&db_path);

    conn.execute(
        "INSERT INTO project (id, worktree, name, time_created, time_updated) VALUES ('proj_1', '/tmp/repo', 'repo', 1, 2)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, title, directory, permission, time_created, time_updated, time_archived) VALUES ('sess_1', 'proj_1', NULL, 'title', '/tmp/repo', '{}', 1, 10, NULL)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, data, time_created) VALUES ('msg_1', 'sess_1', '{\"role\":\"assistant\",\"modelID\":\"gpt-5\",\"time\":{\"completed\":true}}', 1)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO part (id, session_id, message_id, data, time_created) VALUES ('part_1', 'sess_1', 'msg_1', '{\"type\":\"text\",\"text\":\"hello world\"}', 1)",
        [],
    )
    .unwrap();

    let reader = DbReader::open(&db_path).unwrap();

    assert_eq!(
        reader.get_session_model("sess_1").unwrap().as_deref(),
        Some("gpt-5")
    );
    let preview = reader.get_last_message_preview("sess_1").unwrap().unwrap();
    assert_eq!(preview.text, "hello world");
    assert_eq!(preview.role, "assistant");

    fs::remove_file(db_path).ok();
}

#[test]
fn reads_child_sessions() {
    let db_path = temp_db_path("children");
    let conn = init_db(&db_path);

    conn.execute(
        "INSERT INTO project (id, worktree, name, time_created, time_updated) VALUES ('proj_1', '/tmp/repo', 'repo', 1, 2)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, title, directory, permission, time_created, time_updated, time_archived) VALUES ('parent', 'proj_1', NULL, 'parent title', '/tmp/repo', '{}', 1, 10, NULL)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, title, directory, permission, time_created, time_updated, time_archived) VALUES ('child', 'proj_1', 'parent', 'child title', '/tmp/repo/child', '{}', 2, 20, NULL)",
        [],
    )
    .unwrap();

    let reader = DbReader::open(&db_path).unwrap();
    let children = reader.get_child_sessions("parent", 10, 0).unwrap();

    assert_eq!(children.len(), 1);
    assert_eq!(children[0].id, "child");
    assert!(reader.has_child_sessions("parent").unwrap());

    fs::remove_file(db_path).ok();
}

#[test]
fn reads_all_sessions_including_archived_with_user_message_times() {
    let db_path = temp_db_path("all_sessions");
    let conn = init_db(&db_path);

    conn.execute(
        "INSERT INTO project (id, worktree, name, time_created, time_updated) VALUES ('proj_1', '/tmp/repo', 'repo', 1, 2)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, title, directory, permission, time_created, time_updated, time_archived) VALUES ('sess_1', 'proj_1', NULL, 'title', '/tmp/repo', '{}', 10, 50, NULL)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, data, time_created) VALUES ('msg_1', 'sess_1', '{\"role\":\"user\"}', 25)",
        [],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, title, directory, permission, time_created, time_updated, time_archived) VALUES ('sess_2', 'proj_1', NULL, 'old', '/tmp/repo', '{}', 5, 20, 99)",
        [],
    )
    .unwrap();

    let reader = DbReader::open(&db_path).unwrap();
    let all = reader.get_all_sessions().unwrap();

    assert_eq!(all.len(), 2);
    assert_eq!(all[0].id, "sess_1");
    assert_eq!(all[0].worktree, PathBuf::from("/tmp/repo"));
    assert_eq!(all[0].time_updated, 25);

    assert_eq!(all[1].id, "sess_2");
    assert!(all[1].archived);
    assert_eq!(all[1].time_updated, 5); // Fallback to session.time_created

    std::fs::remove_file(db_path).ok();
}

#[test]
fn reads_session_modified_files() {
    let db_path = temp_db_path("modified_files");
    let conn = init_db(&db_path);

    conn.execute(
        "INSERT INTO project (id, worktree, name, time_created, time_updated) VALUES ('proj_1', '/tmp/repo', 'repo', 1, 2)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, title, directory, permission, time_created, time_updated, time_archived) VALUES ('sess_1', 'proj_1', NULL, 'title', '/tmp/repo', '{}', 10, 50, NULL)",
        [],
    )
    .unwrap();
    conn.execute(
        r#"INSERT INTO message (id, session_id, data, time_created) VALUES ('m1', 'sess_1', '{"role":"assistant"}', 1)"#,
        [],
    )
    .unwrap();
    conn.execute(
        r#"INSERT INTO part (id, session_id, message_id, data, time_created) VALUES ('p1', 'sess_1', 'm1', '{"type":"tool","tool":"edit","state":{"input":{"filePath":"/a.txt"}}}', 1)"#,
        [],
    )
    .unwrap();
    conn.execute(
        r#"INSERT INTO part (id, session_id, message_id, data, time_created) VALUES ('p2', 'sess_1', 'm1', '{"type":"tool","tool":"write","state":{"metadata":{"filepath":"/b.txt"}}}', 2)"#,
        [],
    )
    .unwrap();
    conn.execute(
        r#"INSERT INTO part (id, session_id, message_id, data, time_created) VALUES ('p3', 'sess_1', 'm1', '{"type":"tool","tool":"bash","state":{"input":{"command":"ls"}}}', 3)"#,
        [],
    )
    .unwrap();

    // Duplicate path should only be returned once
    conn.execute(
        r#"INSERT INTO part (id, session_id, message_id, data, time_created) VALUES ('p4', 'sess_1', 'm1', '{"type":"tool","tool":"edit","state":{"input":{"filePath":"/a.txt"}}}', 4)"#,
        [],
    )
    .unwrap();

    let reader = opencode_multiplexer::data::db::reader::DbReader::open(&db_path).unwrap();
    let files = reader.get_session_modified_files("sess_1").unwrap();

    assert_eq!(files.len(), 2);
    assert!(files.contains(&"/a.txt".to_string()));
    assert!(files.contains(&"/b.txt".to_string()));

    std::fs::remove_file(db_path).ok();
}

#[test]
fn session_status_pending_tools_needs_input() {
    let db_path = temp_db_path("pending_tools");
    let conn = init_db(&db_path);

    conn.execute(
        "INSERT INTO project (id, worktree, name, time_created, time_updated) VALUES ('proj_1', '/tmp/repo', 'repo', 1, 2)",
        [],
    ).unwrap();

    let tools = ["write", "bash", "edit", "task"];
    for (i, tool) in tools.iter().enumerate() {
        let sess_id = format!("sess_pending_{i}");
        let msg_id = format!("msg_pending_{i}");
        let part_id = format!("part_pending_{i}");

        conn.execute(
            "INSERT INTO session (id, project_id, parent_id, title, directory, permission, time_created, time_updated, time_archived) VALUES (?1, 'proj_1', NULL, 'title', '/tmp/repo', '{}', 1, 10, NULL)",
            params![sess_id],
        ).unwrap();
        conn.execute(
            "INSERT INTO message (id, session_id, data, time_created) VALUES (?1, ?2, '{\"role\":\"assistant\"}', 1)",
            params![msg_id, sess_id],
        ).unwrap();
        conn.execute(
            "INSERT INTO part (id, session_id, message_id, data, time_created) VALUES (?1, ?2, ?3, ?4, 1)",
            params![part_id, sess_id, msg_id, format!("{{\"type\":\"tool\",\"tool\":\"{tool}\",\"state\":{{\"status\":\"pending\"}}}}")],
        ).unwrap();

        let reader = DbReader::open(&db_path).unwrap();
        assert_eq!(
            reader.get_session_status(&sess_id, None).unwrap(),
            SessionStatus::NeedsInput,
            "Tool {tool} should trigger NeedsInput when pending"
        );
    }

    fs::remove_file(db_path).ok();
}

#[test]
fn session_status_main_idle_active_subagent() {
    let db_path = temp_db_path("main_idle_active_subagent");
    let conn = init_db(&db_path);

    conn.execute(
        "INSERT INTO project (id, worktree, name, time_created, time_updated) VALUES ('proj_1', '/tmp/repo', 'repo', 1, 2)",
        [],
    ).unwrap();

    // Main agent (Idle)
    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, title, directory, permission, time_created, time_updated, time_archived) VALUES ('main', 'proj_1', NULL, 'Main', '/tmp/repo', '{}', 1, 10, NULL)",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, data, time_created) VALUES ('msg_main', 'main', '{\"role\":\"assistant\",\"time\":{\"completed\":10}}', 10)",
        [],
    ).unwrap();

    // Subagent (Working)
    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, title, directory, permission, time_created, time_updated, time_archived) VALUES ('sub', 'proj_1', 'main', 'Sub', '/tmp/repo/sub', '{}', 11, 20, NULL)",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, data, time_created) VALUES ('msg_sub', 'sub', '{\"role\":\"assistant\"}', 20)",
        [],
    ).unwrap();

    let reader = DbReader::open(&db_path).unwrap();
    assert_eq!(
        reader.get_session_status("main", None).unwrap(),
        SessionStatus::SubagentsWorking
    );

    fs::remove_file(db_path).ok();
}

#[test]
fn session_status_subagent_needs_input() {
    let db_path = temp_db_path("subagent_needs_input");
    let conn = init_db(&db_path);

    conn.execute(
        "INSERT INTO project (id, worktree, name, time_created, time_updated) VALUES ('proj_1', '/tmp/repo', 'repo', 1, 2)",
        [],
    ).unwrap();

    // Main agent (Idle)
    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, title, directory, permission, time_created, time_updated, time_archived) VALUES ('main', 'proj_1', NULL, 'Main', '/tmp/repo', '{}', 1, 10, NULL)",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, data, time_created) VALUES ('msg_main', 'main', '{\"role\":\"assistant\",\"time\":{\"completed\":10}}', 10)",
        [],
    ).unwrap();

    // Subagent (NeedsInput)
    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, title, directory, permission, time_created, time_updated, time_archived) VALUES ('sub', 'proj_1', 'main', 'Sub', '/tmp/repo/sub', '{}', 11, 20, NULL)",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, data, time_created) VALUES ('msg_sub', 'sub', '{\"role\":\"assistant\"}', 20)",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO part (id, session_id, message_id, data, time_created) VALUES ('part_sub', 'sub', 'msg_sub', '{\"type\":\"tool\",\"tool\":\"question\",\"state\":{\"status\":\"running\"}}', 21)",
        [],
    ).unwrap();

    let reader = DbReader::open(&db_path).unwrap();
    assert_eq!(
        reader.get_session_status("main", None).unwrap(),
        SessionStatus::NeedsInput
    );

    fs::remove_file(db_path).ok();
}

#[test]
fn session_status_ignores_archived_subagents() {
    let db_path = temp_db_path("ignores_archived_subagents");
    let conn = init_db(&db_path);

    conn.execute(
        "INSERT INTO project (id, worktree, name, time_created, time_updated) VALUES ('proj_1', '/tmp/repo', 'repo', 1, 2)",
        [],
    ).unwrap();

    // Main agent (Idle)
    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, title, directory, permission, time_created, time_updated, time_archived) VALUES ('main', 'proj_1', NULL, 'Main', '/tmp/repo', '{}', 1, 10, NULL)",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, data, time_created) VALUES ('msg_main', 'main', '{\"role\":\"assistant\",\"time\":{\"completed\":10}}', 10)",
        [],
    ).unwrap();

    // Subagent (Archived, but Working if it wasn't archived)
    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, title, directory, permission, time_created, time_updated, time_archived) VALUES ('sub', 'proj_1', 'main', 'Sub', '/tmp/repo/sub', '{}', 11, 20, 1000)",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, data, time_created) VALUES ('msg_sub', 'sub', '{\"role\":\"assistant\"}', 20)",
        [],
    ).unwrap();

    let reader = DbReader::open(&db_path).unwrap();
    assert_eq!(
        reader.get_session_status("main", None).unwrap(),
        SessionStatus::Idle
    );

    fs::remove_file(db_path).ok();
}

#[test]
fn session_status_multi_level_rollup() {
    let db_path = temp_db_path("multi_level_rollup");
    let conn = init_db(&db_path);

    conn.execute(
        "INSERT INTO project (id, worktree, name, time_created, time_updated) VALUES ('proj_1', '/tmp/repo', 'repo', 1, 2)",
        [],
    ).unwrap();

    // A (Idle)
    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, title, directory, permission, time_created, time_updated, time_archived) VALUES ('A', 'proj_1', NULL, 'A', '/tmp/repo', '{}', 1, 10, NULL)",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, data, time_created) VALUES ('msg_A', 'A', '{\"role\":\"assistant\",\"time\":{\"completed\":10}}', 10)",
        [],
    ).unwrap();

    // B (Idle)
    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, title, directory, permission, time_created, time_updated, time_archived) VALUES ('B', 'proj_1', 'A', 'B', '/tmp/repo/B', '{}', 11, 20, NULL)",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, data, time_created) VALUES ('msg_B', 'B', '{\"role\":\"assistant\",\"time\":{\"completed\":20}}', 20)",
        [],
    ).unwrap();

    // C (Working)
    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, title, directory, permission, time_created, time_updated, time_archived) VALUES ('C', 'proj_1', 'B', 'C', '/tmp/repo/C', '{}', 21, 30, NULL)",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, data, time_created) VALUES ('msg_C', 'C', '{\"role\":\"assistant\"}', 30)",
        [],
    ).unwrap();

    let reader = DbReader::open(&db_path).unwrap();
    assert_eq!(
        reader.get_session_status("C", None).unwrap(),
        SessionStatus::Working
    );
    assert_eq!(
        reader.get_session_status("B", None).unwrap(),
        SessionStatus::SubagentsWorking
    );
    assert_eq!(
        reader.get_session_status("A", None).unwrap(),
        SessionStatus::SubagentsWorking
    );

    // Make C Need Input
    conn.execute(
        "INSERT INTO part (id, session_id, message_id, data, time_created) VALUES ('part_C', 'C', 'msg_C', '{\"type\":\"tool\",\"tool\":\"question\",\"state\":{\"status\":\"running\"}}', 31)",
        [],
    ).unwrap();

    assert_eq!(
        reader.get_session_status("A", None).unwrap(),
        SessionStatus::NeedsInput
    );

    fs::remove_file(db_path).ok();
}

#[test]
fn session_status_error_precedence() {
    let db_path = temp_db_path("error_precedence");
    let conn = init_db(&db_path);

    conn.execute(
        "INSERT INTO project (id, worktree, name, time_created, time_updated) VALUES ('proj_1', '/tmp/repo', 'repo', 1, 2)",
        [],
    ).unwrap();

    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, title, directory, permission, time_created, time_updated, time_archived) VALUES ('sess_err_work', 'proj_1', NULL, 'Title', '/tmp/repo', '{}', 1, 10, NULL)",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, data, time_created) VALUES ('msg_1', 'sess_err_work', '{\"role\":\"assistant\"}', 10)",
        [],
    ).unwrap();
    // Error part
    conn.execute(
        "INSERT INTO part (id, session_id, message_id, data, time_created) VALUES ('part_err', 'sess_err_work', 'msg_1', '{\"type\":\"tool\",\"tool\":\"edit\",\"state\":{\"status\":\"error\"}}', 11)",
        [],
    ).unwrap();
    
    let reader = DbReader::open(&db_path).unwrap();
    assert_eq!(
        reader.get_session_status("sess_err_work", None).unwrap(),
        SessionStatus::Error
    );

    // If we add a NeedsInput part, it should win over Error
    conn.execute(
        "INSERT INTO part (id, session_id, message_id, data, time_created) VALUES ('part_needs', 'sess_err_work', 'msg_1', '{\"type\":\"tool\",\"tool\":\"question\",\"state\":{\"status\":\"running\"}}', 12)",
        [],
    ).unwrap();
    
    assert_eq!(
        reader.get_session_status("sess_err_work", None).unwrap(),
        SessionStatus::NeedsInput
    );

    fs::remove_file(db_path).ok();
}

#[test]
fn session_status_full_precedence_chain() {
    let db_path = temp_db_path("precedence_chain");
    let conn = init_db(&db_path);

    conn.execute(
        "INSERT INTO project (id, worktree, name, time_created, time_updated) VALUES ('proj_1', '/tmp/repo', 'repo', 1, 2)",
        [],
    ).unwrap();

    // Start with Idle
    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, title, directory, permission, time_created, time_updated, time_archived) VALUES ('main', 'proj_1', NULL, 'Main', '/tmp/repo', '{}', 1, 10, NULL)",
        [],
    ).unwrap();
    
    let reader = DbReader::open(&db_path).unwrap();
    assert_eq!(reader.get_session_status("main", None).unwrap(), SessionStatus::Idle);

    // Add Subagent (Working) -> SubagentsWorking
    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, title, directory, permission, time_created, time_updated, time_archived) VALUES ('sub', 'proj_1', 'main', 'Sub', '/tmp/repo/sub', '{}', 11, 20, NULL)",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, data, time_created) VALUES ('msg_sub', 'sub', '{\"role\":\"user\"}', 20)",
        [],
    ).unwrap();
    assert_eq!(reader.get_session_status("main", None).unwrap(), SessionStatus::SubagentsWorking);

    // Make Main (Working) -> Working wins over SubagentsWorking
    conn.execute(
        "INSERT INTO message (id, session_id, data, time_created) VALUES ('msg_main', 'main', '{\"role\":\"user\"}', 30)",
        [],
    ).unwrap();
    assert_eq!(reader.get_session_status("main", None).unwrap(), SessionStatus::Working);

    // Make Main (Error) -> Error wins over Working
    conn.execute(
        "INSERT INTO part (id, session_id, message_id, data, time_created) VALUES ('part_err', 'main', 'msg_main', '{\"type\":\"tool\",\"tool\":\"edit\",\"state\":{\"status\":\"error\"}}', 31)",
        [],
    ).unwrap();
    assert_eq!(reader.get_session_status("main", None).unwrap(), SessionStatus::Error);

    // Make Main (NeedsInput) -> NeedsInput wins over Error
    conn.execute(
        "INSERT INTO part (id, session_id, message_id, data, time_created) VALUES ('part_needs', 'main', 'msg_main', '{\"type\":\"tool\",\"tool\":\"question\",\"state\":{\"status\":\"running\"}}', 32)",
        [],
    ).unwrap();
    assert_eq!(reader.get_session_status("main", None).unwrap(), SessionStatus::NeedsInput);

    fs::remove_file(db_path).ok();
}

#[test]
fn session_status_cycle_detection() {
    let db_path = temp_db_path("cycle");
    let conn = init_db(&db_path);

    conn.execute(
        "INSERT INTO project (id, worktree, name, time_created, time_updated) VALUES ('proj_1', '/tmp/repo', 'repo', 1, 2)",
        [],
    ).unwrap();

    // A -> B -> A
    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, title, directory, permission, time_created, time_updated, time_archived) VALUES ('A', 'proj_1', 'B', 'A', '/tmp/repo', '{}', 1, 10, NULL)",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, title, directory, permission, time_created, time_updated, time_archived) VALUES ('B', 'proj_1', 'A', 'B', '/tmp/repo', '{}', 1, 10, NULL)",
        [],
    ).unwrap();

    let reader = DbReader::open(&db_path).unwrap();
    // Should not hang
    let status = reader.get_session_status("A", None).unwrap();
    assert_eq!(status, SessionStatus::Idle);

    fs::remove_file(db_path).ok();
}

#[test]
fn session_status_working_user_message() {
    let db_path = temp_db_path("working_user");
    let conn = init_db(&db_path);

    conn.execute(
        "INSERT INTO project (id, worktree, name, time_created, time_updated) VALUES ('proj_1', '/tmp/repo', 'repo', 1, 2)",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, title, directory, permission, time_created, time_updated, time_archived) VALUES ('sess_1', 'proj_1', NULL, 'Title', '/tmp/repo', '{}', 1, 10, NULL)",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, data, time_created) VALUES ('msg_1', 'sess_1', '{\"role\":\"user\"}', 10)",
        [],
    ).unwrap();

    let reader = DbReader::open(&db_path).unwrap();
    assert_eq!(
        reader.get_session_status("sess_1", None).unwrap(),
        SessionStatus::Working
    );

    fs::remove_file(db_path).ok();
}

#[test]
fn session_status_working_incomplete_assistant_message() {
    let db_path = temp_db_path("working_assistant");
    let conn = init_db(&db_path);

    conn.execute(
        "INSERT INTO project (id, worktree, name, time_created, time_updated) VALUES ('proj_1', '/tmp/repo', 'repo', 1, 2)",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, title, directory, permission, time_created, time_updated, time_archived) VALUES ('sess_1', 'proj_1', NULL, 'Title', '/tmp/repo', '{}', 1, 10, NULL)",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, data, time_created) VALUES ('msg_1', 'sess_1', '{\"role\":\"assistant\"}', 10)",
        [],
    ).unwrap();

    let reader = DbReader::open(&db_path).unwrap();
    assert_eq!(
        reader.get_session_status("sess_1", None).unwrap(),
        SessionStatus::Working
    );

    fs::remove_file(db_path).ok();
}

#[test]
fn session_status_idle_no_messages() {
    let db_path = temp_db_path("idle_no_msg");
    let conn = init_db(&db_path);

    conn.execute(
        "INSERT INTO project (id, worktree, name, time_created, time_updated) VALUES ('proj_1', '/tmp/repo', 'repo', 1, 2)",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, title, directory, permission, time_created, time_updated, time_archived) VALUES ('sess_1', 'proj_1', NULL, 'Title', '/tmp/repo', '{}', 1, 10, NULL)",
        [],
    ).unwrap();

    let reader = DbReader::open(&db_path).unwrap();
    assert_eq!(
        reader.get_session_status("sess_1", None).unwrap(),
        SessionStatus::Idle
    );

    fs::remove_file(db_path).ok();
}

#[test]
fn uncompleted_assistant_message_older_than_process_start_is_idle() {
    let db_path = temp_db_path("stale_assistant");
    let conn = init_db(&db_path);

    conn.execute(
        "INSERT INTO project (id, worktree, name, time_created, time_updated) VALUES ('proj_1', '/tmp/repo', 'repo', 1, 2)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, title, directory, permission, time_created, time_updated, time_archived) VALUES ('sess_1', 'proj_1', NULL, 'Title', '/tmp/repo', '{}', 1000, 1000, NULL)",
        [],
    )
    .unwrap();
    // Uncompleted assistant message from a previous process lifetime
    conn.execute(
        "INSERT INTO message (id, session_id, data, time_created) VALUES ('msg_1', 'sess_1', '{\"role\":\"assistant\"}', 1000)",
        [],
    )
    .unwrap();

    let reader = DbReader::open(&db_path).unwrap();

    // Without cutoff: Working
    assert_eq!(
        reader.get_session_status("sess_1", None).unwrap(),
        SessionStatus::Working
    );

    // With process start after message: Idle (stale)
    assert_eq!(
        reader.get_session_status("sess_1", Some(5000)).unwrap(),
        SessionStatus::Idle
    );

    // With process start before message: still Working
    assert_eq!(
        reader.get_session_status("sess_1", Some(500)).unwrap(),
        SessionStatus::Working
    );

    fs::remove_file(db_path).ok();
}

#[test]
fn running_tool_part_older_than_process_start_is_stale() {
    let db_path = temp_db_path("stale_tool");
    let conn = init_db(&db_path);

    conn.execute(
        "INSERT INTO project (id, worktree, name, time_created, time_updated) VALUES ('proj_1', '/tmp/repo', 'repo', 1, 2)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, title, directory, permission, time_created, time_updated, time_archived) VALUES ('sess_1', 'proj_1', NULL, 'Title', '/tmp/repo', '{}', 1000, 1000, NULL)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, data, time_created) VALUES ('msg_1', 'sess_1', '{\"role\":\"assistant\"}', 1000)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO part (id, session_id, message_id, data, time_created) VALUES ('part_1', 'sess_1', 'msg_1', '{\"type\":\"tool\",\"tool\":\"question\",\"state\":{\"status\":\"running\",\"time\":{\"start\":1000}}}', 1000)",
        [],
    )
    .unwrap();

    let reader = DbReader::open(&db_path).unwrap();

    // Without cutoff: NeedsInput
    assert_eq!(
        reader.get_session_status("sess_1", None).unwrap(),
        SessionStatus::NeedsInput
    );

    // With process start after tool start: stale → falls through to Idle
    // (assistant message also older than cutoff)
    assert_eq!(
        reader.get_session_status("sess_1", Some(5000)).unwrap(),
        SessionStatus::Idle
    );

    fs::remove_file(db_path).ok();
}

#[test]
fn subagent_uncompleted_message_with_newer_parent_user_message_is_idle() {
    let db_path = temp_db_path("abandoned_subagent");
    let conn = init_db(&db_path);

    conn.execute(
        "INSERT INTO project (id, worktree, name, time_created, time_updated) VALUES ('proj_1', '/tmp/repo', 'repo', 1, 2)",
        [],
    )
    .unwrap();

    // Parent: idle after a newer user message at t=5000
    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, title, directory, permission, time_created, time_updated, time_archived) VALUES ('main', 'proj_1', NULL, 'Main', '/tmp/repo', '{}', 1, 5000, NULL)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, data, time_created) VALUES ('msg_user', 'main', '{\"role\":\"user\"}', 5000)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, data, time_created) VALUES ('msg_main', 'main', '{\"role\":\"assistant\",\"time\":{\"completed\":5001}}', 5001)",
        [],
    )
    .unwrap();

    // Subagent: still "working" but last updated at t=2000 (before parent user msg)
    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, title, directory, permission, time_created, time_updated, time_archived) VALUES ('sub', 'proj_1', 'main', 'Sub', '/tmp/repo/sub', '{}', 1000, 2000, NULL)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, data, time_created) VALUES ('msg_sub', 'sub', '{\"role\":\"assistant\"}', 2000)",
        [],
    )
    .unwrap();

    let reader = DbReader::open(&db_path).unwrap();

    // Parent should be Idle — abandoned subagent must not roll up as SubagentsWorking
    assert_eq!(
        reader.get_session_status("main", None).unwrap(),
        SessionStatus::Idle
    );

    fs::remove_file(db_path).ok();
}

#[test]
fn pending_permission_older_than_process_start_is_stale() {
    let db_path = temp_db_path("stale_pending");
    let conn = init_db(&db_path);

    conn.execute(
        "INSERT INTO project (id, worktree, name, time_created, time_updated) VALUES ('proj_1', '/tmp/repo', 'repo', 1, 2)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, title, directory, permission, time_created, time_updated, time_archived) VALUES ('sess_1', 'proj_1', NULL, 'Title', '/tmp/repo', '{}', 1000, 1000, NULL)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO message (id, session_id, data, time_created) VALUES ('msg_1', 'sess_1', '{\"role\":\"assistant\",\"time\":{\"completed\":1000}}', 1000)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO part (id, session_id, message_id, data, time_created) VALUES ('part_1', 'sess_1', 'msg_1', '{\"type\":\"tool\",\"tool\":\"write\",\"state\":{\"status\":\"pending\"}}', 1000)",
        [],
    )
    .unwrap();

    let reader = DbReader::open(&db_path).unwrap();

    assert_eq!(
        reader.get_session_status("sess_1", None).unwrap(),
        SessionStatus::NeedsInput
    );
    assert_eq!(
        reader.get_session_status("sess_1", Some(5000)).unwrap(),
        SessionStatus::Idle
    );

    fs::remove_file(db_path).ok();
}

fn temp_db_path(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("ocmux-rs-{label}-{nanos}.db"))
}

fn init_db(path: &PathBuf) -> Connection {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        r#"
        CREATE TABLE project (
            id TEXT PRIMARY KEY,
            worktree TEXT NOT NULL,
            name TEXT,
            time_created INTEGER,
            time_updated INTEGER
        );
        CREATE TABLE session (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            parent_id TEXT,
            title TEXT,
            directory TEXT,
            permission TEXT,
            time_created INTEGER,
            time_updated INTEGER,
            time_archived INTEGER
        );
        CREATE TABLE message (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            data TEXT NOT NULL,
            time_created INTEGER
        );
        CREATE TABLE part (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            message_id TEXT NOT NULL,
            data TEXT NOT NULL,
            time_created INTEGER
        );
        "#,
    )
    .unwrap();
    conn
}

#[test]
fn reader_sets_busy_timeout_on_open() {
    let db_path = temp_db_path("busy_timeout");
    let conn = init_db(&db_path);
    conn.execute(
        "INSERT INTO project VALUES ('proj_1', '/tmp/repo', 'repo', 1, 2)",
        [],
    )
    .unwrap();
    drop(conn);

    // Open DbReader — it must set a non-zero busy_timeout so reads
    // wait for locked writes instead of failing instantly.
    let reader = DbReader::open(&db_path).unwrap();
    let busy_timeout = reader.busy_timeout_ms().unwrap();
    assert!(
        busy_timeout > 0,
        "DbReader must set a non-zero busy_timeout, got: {busy_timeout}"
    );

    fs::remove_file(db_path).ok();
}

#[test]
fn list_sessions_for_manager_returns_top_level_with_subtree_message_counts() {
    let db_path = temp_db_path("manager_list");
    let conn = init_db(&db_path);
    conn.execute(
        "INSERT INTO project VALUES ('proj1', '/tmp/repo', 'repo', 1, 2)",
        [],
    )
    .unwrap();
    
    // parent1 -> child1 (2 msgs total: 1 parent, 1 child)
    conn.execute(
        "INSERT INTO session VALUES ('p1', 'proj1', NULL, 'P1', '/tmp/repo', NULL, 100, 100, NULL)",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO session VALUES ('c1', 'proj1', 'p1', 'C1', '/tmp/repo/c1', NULL, 110, 110, NULL)",
        [],
    ).unwrap();
    conn.execute(
        r#"INSERT INTO message VALUES ('m1', 'p1', '{"role":"user"}', 105)"#,
        [],
    ).unwrap();
    conn.execute(
        r#"INSERT INTO message VALUES ('m2', 'c1', '{"role":"user"}', 115)"#,
        [],
    ).unwrap();

    // parent2 (0 msgs)
    conn.execute(
        "INSERT INTO session VALUES ('p2', 'proj1', NULL, 'P2', '/tmp/repo', NULL, 50, 50, NULL)",
        [],
    ).unwrap();

    let reader = DbReader::open(&db_path).unwrap();
    let sessions = reader.list_sessions_for_manager().unwrap();
    
    assert_eq!(sessions.len(), 2);
    // ordered by last_interaction DESC: p1 (115) then p2 (50)
    assert_eq!(sessions[0].id, "p1");
    assert_eq!(sessions[0].user_message_count, 2);
    assert_eq!(sessions[1].id, "p2");
    assert_eq!(sessions[1].user_message_count, 0);

    fs::remove_file(db_path).ok();
}
