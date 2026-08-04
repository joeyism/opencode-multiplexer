use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use opencode_multiplexer::data::db::writer::DbWriter;
use rusqlite::{Connection, params};

fn temp_db_path(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("ocmux-rs-writer-{label}-{nanos}.db"))
}

/// Schema closer to OpenCode: FKs + CASCADE. parent_id intentionally has NO FK.
fn init_db(path: &PathBuf) -> Connection {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        r#"
        PRAGMA foreign_keys = ON;
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
            time_archived INTEGER,
            FOREIGN KEY (project_id) REFERENCES project(id)
        );
        CREATE TABLE message (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            data TEXT NOT NULL,
            time_created INTEGER,
            time_updated INTEGER DEFAULT 0,
            FOREIGN KEY (session_id) REFERENCES session(id) ON DELETE CASCADE
        );
        CREATE TABLE part (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            message_id TEXT NOT NULL,
            data TEXT NOT NULL,
            time_created INTEGER,
            time_updated INTEGER DEFAULT 0,
            FOREIGN KEY (message_id) REFERENCES message(id) ON DELETE CASCADE
        );
        CREATE TABLE todo (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            data TEXT,
            FOREIGN KEY (session_id) REFERENCES session(id) ON DELETE CASCADE
        );
        "#,
    )
    .unwrap();
    conn
}

fn insert_project(conn: &Connection, id: &str, worktree: &str) {
    conn.execute(
        "INSERT INTO project (id, worktree, name, time_created, time_updated) VALUES (?1, ?2, 'repo', 1, 2)",
        params![id, worktree],
    )
    .unwrap();
}

fn insert_session(
    conn: &Connection,
    id: &str,
    project_id: &str,
    parent_id: Option<&str>,
    title: &str,
    time_updated: i64,
) {
    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, title, directory, permission, time_created, time_updated, time_archived)
         VALUES (?1, ?2, ?3, ?4, '/tmp/repo', '{}', 1, ?5, NULL)",
        params![id, project_id, parent_id, title, time_updated],
    )
    .unwrap();
}

fn insert_user_message(conn: &Connection, id: &str, session_id: &str, text: &str, t: i64) {
    conn.execute(
        "INSERT INTO message (id, session_id, data, time_created, time_updated)
         VALUES (?1, ?2, ?3, ?4, ?4)",
        params![id, session_id, format!(r#"{{"role":"user"}}"#), t],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO part (id, session_id, message_id, data, time_created, time_updated)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![
            format!("part_{id}"),
            session_id,
            id,
            format!(
                r#"{{"type":"text","text":{}}}"#,
                serde_json::to_string(text).unwrap()
            ),
            t
        ],
    )
    .unwrap();
}

#[test]
fn writer_enables_foreign_keys_on_open() {
    let db_path = temp_db_path("fk_pragma");
    let _ = init_db(&db_path);
    let writer = DbWriter::open(&db_path).unwrap();
    let on: i64 = writer.pragma_foreign_keys().expect("pragma query");
    assert_eq!(on, 1, "DbWriter must enable PRAGMA foreign_keys");
    fs::remove_file(db_path).ok();
}

#[test]
fn delete_session_removes_messages_and_parts() {
    let db_path = temp_db_path("delete_one");
    let conn = init_db(&db_path);
    insert_project(&conn, "proj", "/tmp/repo");
    insert_session(&conn, "sess_a", "proj", None, "Keep", 100);
    insert_session(&conn, "sess_b", "proj", None, "Junk", 200);
    insert_user_message(&conn, "msg_a", "sess_a", "keep me", 100);
    insert_user_message(&conn, "msg_b", "sess_b", "delete me", 200);
    drop(conn);

    let mut writer = DbWriter::open(&db_path).unwrap();
    let live = std::collections::HashSet::new();
    let result = writer.delete_sessions(&["sess_b".into()], &live).unwrap();
    assert_eq!(result.deleted_session_ids, vec!["sess_b".to_string()]);
    assert_eq!(result.skipped_live_ids.len(), 0);
    drop(writer);

    let conn = Connection::open(&db_path).unwrap();
    let sessions: i64 = conn
        .query_row("SELECT COUNT(*) FROM session", [], |r| r.get(0))
        .unwrap();
    let messages: i64 = conn
        .query_row("SELECT COUNT(*) FROM message", [], |r| r.get(0))
        .unwrap();
    let parts: i64 = conn
        .query_row("SELECT COUNT(*) FROM part", [], |r| r.get(0))
        .unwrap();
    assert_eq!(sessions, 1);
    assert_eq!(messages, 1);
    assert_eq!(parts, 1);
    let remaining: String = conn
        .query_row("SELECT id FROM session", [], |r| r.get(0))
        .unwrap();
    assert_eq!(remaining, "sess_a");
    fs::remove_file(db_path).ok();
}

#[test]
fn delete_missing_session_is_ok_and_reports_nothing() {
    let db_path = temp_db_path("delete_missing");
    let _ = init_db(&db_path);
    let mut writer = DbWriter::open(&db_path).unwrap();
    let live = std::collections::HashSet::new();
    let result = writer.delete_sessions(&["nope".into()], &live).unwrap();
    assert!(result.deleted_session_ids.is_empty());
    fs::remove_file(db_path).ok();
}

#[test]
fn delete_also_removes_todo_rows() {
    let db_path = temp_db_path("delete_todo");
    let conn = init_db(&db_path);
    insert_project(&conn, "proj", "/tmp/repo");
    insert_session(&conn, "sess", "proj", None, "T", 1);
    conn.execute(
        "INSERT INTO todo (id, session_id, data) VALUES ('t1', 'sess', '{}')",
        [],
    )
    .unwrap();
    drop(conn);

    let live = std::collections::HashSet::new();
    let mut writer = DbWriter::open(&db_path).unwrap();
    writer.delete_sessions(&["sess".into()], &live).unwrap();

    let conn = Connection::open(&db_path).unwrap();
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM todo", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0);
    fs::remove_file(db_path).ok();
}

#[test]
fn delete_parent_removes_child_and_grandchild_sessions_and_their_messages() {
    let db_path = temp_db_path("delete_subtree");
    let conn = init_db(&db_path);
    insert_project(&conn, "proj", "/tmp/repo");

    // parent -> child -> grand
    insert_session(&conn, "parent", "proj", None, "P", 1);
    insert_session(&conn, "child", "proj", Some("parent"), "C", 2);
    insert_session(&conn, "grand", "proj", Some("child"), "G", 3);

    insert_user_message(&conn, "msg_p", "parent", "p", 1);
    insert_user_message(&conn, "msg_c", "child", "c", 2);
    insert_user_message(&conn, "msg_g", "grand", "g", 3);

    // sibling
    insert_session(&conn, "sibling", "proj", None, "S", 4);
    insert_user_message(&conn, "msg_s", "sibling", "s", 4);
    drop(conn);

    let mut writer = DbWriter::open(&db_path).unwrap();
    let live = std::collections::HashSet::new();
    let result = writer.delete_sessions(&["parent".into()], &live).unwrap();

    let mut deleted = result.deleted_session_ids;
    deleted.sort();
    assert_eq!(deleted, vec!["child", "grand", "parent"]);

    let conn = Connection::open(&db_path).unwrap();
    let sessions: i64 = conn
        .query_row("SELECT COUNT(*) FROM session", [], |r| r.get(0))
        .unwrap();
    let messages: i64 = conn
        .query_row("SELECT COUNT(*) FROM message", [], |r| r.get(0))
        .unwrap();
    assert_eq!(sessions, 1); // only sibling
    assert_eq!(messages, 1); // only sibling's msg
    fs::remove_file(db_path).ok();
}

#[test]
fn delete_child_only_leaves_parent() {
    let db_path = temp_db_path("delete_child_only");
    let conn = init_db(&db_path);
    insert_project(&conn, "proj", "/tmp/repo");
    insert_session(&conn, "parent", "proj", None, "P", 1);
    insert_session(&conn, "child", "proj", Some("parent"), "C", 2);
    drop(conn);

    let mut writer = DbWriter::open(&db_path).unwrap();
    let live = std::collections::HashSet::new();
    writer.delete_sessions(&["child".into()], &live).unwrap();

    let conn = Connection::open(&db_path).unwrap();
    let sessions: i64 = conn
        .query_row("SELECT COUNT(*) FROM session", [], |r| r.get(0))
        .unwrap();
    assert_eq!(sessions, 1);
    let remaining: String = conn
        .query_row("SELECT id FROM session", [], |r| r.get(0))
        .unwrap();
    assert_eq!(remaining, "parent");
    fs::remove_file(db_path).ok();
}

#[test]
fn bulk_delete_skips_live_sessions_deletes_others() {
    let db_path = temp_db_path("delete_live_skip");
    let conn = init_db(&db_path);
    insert_project(&conn, "proj", "/tmp/repo");
    insert_session(&conn, "sess_live", "proj", None, "L", 1);
    insert_session(&conn, "sess_dead", "proj", None, "D", 2);
    drop(conn);

    let mut live = std::collections::HashSet::new();
    live.insert("sess_live".into());

    let mut writer = DbWriter::open(&db_path).unwrap();
    let result = writer
        .delete_sessions(&["sess_live".into(), "sess_dead".into()], &live)
        .unwrap();

    assert_eq!(result.deleted_session_ids, vec!["sess_dead"]);
    assert_eq!(result.skipped_live_ids, vec!["sess_live"]);

    let conn = Connection::open(&db_path).unwrap();
    let sessions: i64 = conn
        .query_row("SELECT COUNT(*) FROM session", [], |r| r.get(0))
        .unwrap();
    assert_eq!(sessions, 1);
    let remaining: String = conn
        .query_row("SELECT id FROM session", [], |r| r.get(0))
        .unwrap();
    assert_eq!(remaining, "sess_live");
    fs::remove_file(db_path).ok();
}

#[test]
fn deleting_junk_session_removes_its_messages_from_history_query() {
    let db_path = temp_db_path("history_integration");
    let conn = init_db(&db_path);
    insert_project(&conn, "proj", "/tmp/repo");

    insert_session(&conn, "sess1", "proj", None, "S1", 100);
    insert_session(&conn, "sess2", "proj", None, "S2", 200);

    insert_user_message(&conn, "m1", "sess1", "review comments from A", 100);
    insert_user_message(&conn, "m2", "sess2", "hello world", 200);
    drop(conn);

    use opencode_multiplexer::data::db::reader::DbReader;

    let reader = DbReader::open(&db_path).unwrap();
    let msgs = reader.get_all_user_messages().unwrap();
    assert_eq!(msgs.len(), 2);
    assert!(
        msgs.iter()
            .any(|m| m.text.contains("review comments from A"))
    );
    drop(reader);

    let mut writer = DbWriter::open(&db_path).unwrap();
    let live = std::collections::HashSet::new();
    writer.delete_sessions(&["sess1".into()], &live).unwrap();
    drop(writer);

    let reader = DbReader::open(&db_path).unwrap();
    let msgs = reader.get_all_user_messages().unwrap();
    assert_eq!(msgs.len(), 1);
    assert!(
        !msgs
            .iter()
            .any(|m| m.text.contains("review comments from A"))
    );
    assert_eq!(msgs[0].text, "hello world");
    fs::remove_file(db_path).ok();
}

#[test]
fn writer_sets_busy_timeout_on_open() {
    let db_path = temp_db_path("busy_timeout");
    let _ = init_db(&db_path);

    let writer = DbWriter::open(&db_path).unwrap();
    let busy_timeout = writer.busy_timeout_ms().unwrap();
    assert!(
        busy_timeout > 0,
        "DbWriter must set a non-zero busy_timeout, got: {busy_timeout}"
    );

    fs::remove_file(db_path).ok();
}
