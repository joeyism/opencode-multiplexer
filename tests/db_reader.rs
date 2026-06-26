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
        reader.get_session_status("sess_1").unwrap(),
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
