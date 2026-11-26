use std::{
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{
    app::sessions::SessionStatus,
    data::db::{reader::DbReader, writer::DbWriter},
    data::discovery::{
        cwd::cwd_for_pid,
        ps::{scan_processes, scan_serve_processes},
    },
    ops::opencode::ensure_serve_for_cwd,
    registry::{is_pid_alive, load_serve_registry},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveResult {
    pub session_ids: Vec<String>,
    pub dirty_source: bool,
    pub destination: PathBuf,
    pub warning: Option<String>,
}

pub fn project_relative_path(worktree: &Path, directory: &Path) -> anyhow::Result<String> {
    let worktree = canonical_or_original(worktree);
    let directory = canonical_or_original(directory);
    let relative = directory.strip_prefix(&worktree)?;
    Ok(relative.to_string_lossy().replace('\\', "/"))
}

pub fn move_session_tree(session_id: &str, destination: &Path) -> anyhow::Result<MoveResult> {
    let destination = std::fs::canonicalize(destination)?;
    if !destination.is_dir() {
        anyhow::bail!("destination is not a directory");
    }

    let reader = DbReader::open_default()?;
    let source_session = reader
        .get_session_by_id(session_id)?
        .ok_or_else(|| anyhow::anyhow!("session not found"))?;
    let source_directory = source_session.directory.clone();
    let source_directory = canonical_or_original(&source_directory);
    if source_directory == destination {
        anyhow::bail!("session is already in that directory");
    }

    let session_ids = reader.get_session_tree_ids(session_id)?;
    let process_start_time = source_process_start_time(&source_directory, &session_ids);
    ensure_session_tree_not_busy(&reader, &session_ids, process_start_time)?;

    let dirty_source = git_tree_is_dirty(&source_directory);
    let (destination_project, worktree) = resolve_destination_project(&destination)?;
    let path = project_relative_path(&worktree, &destination)?;
    let mut warning = None;

    let moved_ids = if source_session.project_id == destination_project {
        let (port, _) = ensure_serve_for_cwd(&source_directory)?;
        let mut moved = Vec::new();
        for id in &session_ids {
            if let Err(error) = move_session_on_serve(port, id, &destination) {
                if moved.is_empty() {
                    return Err(error);
                }
                warning = Some(format!(
                    "partially moved {}/{} sessions to {}; remaining sessions stayed in the source: {error}",
                    moved.len(),
                    session_ids.len(),
                    destination.display()
                ));
                break;
            }
            moved.push(id.clone());
        }
        moved
    } else {
        let mut writer = DbWriter::open_default()?;
        let moved = writer.move_sessions(
            &session_ids,
            &destination.to_string_lossy(),
            &path,
            &destination_project,
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64,
        )?;
        if moved != session_ids.len() {
            anyhow::bail!("moved {moved} of {} sessions", session_ids.len());
        }
        session_ids.clone()
    };

    Ok(MoveResult {
        session_ids: moved_ids,
        dirty_source,
        destination,
        warning,
    })
}

fn ensure_session_tree_not_busy(
    reader: &DbReader,
    session_ids: &[String],
    process_start_time: Option<i64>,
) -> anyhow::Result<()> {
    if session_ids.is_empty() {
        anyhow::bail!("session not found");
    }
    for id in session_ids {
        let status = reader.get_session_own_status(id, process_start_time)?;
        if matches!(
            status,
            SessionStatus::Working | SessionStatus::SubagentsWorking | SessionStatus::NeedsInput
        ) {
            anyhow::bail!("session tree is busy ({id}: {status:?})");
        }
    }
    Ok(())
}

fn source_process_start_time(source_directory: &Path, session_ids: &[String]) -> Option<i64> {
    let processes = scan_processes().ok()?;
    let serve_processes = scan_serve_processes().ok()?;
    let serve_registry = load_serve_registry().unwrap_or_default();
    let canonical_source = canonical_or_original(source_directory);
    let mut matching_start_times = Vec::new();

    for process in processes {
        if !is_pid_alive(process.pid) {
            continue;
        }
        let matches_session = process
            .session_id
            .as_deref()
            .is_some_and(|id| session_ids.iter().any(|session_id| session_id == id));
        let matches_directory = if process.session_id.is_none() {
            cwd_for_pid(process.pid).ok().flatten().is_none_or(|cwd| {
                paths_share_hierarchy(&canonical_source, &canonical_or_original(&cwd))
            })
        } else {
            false
        };
        if matches_session || matches_directory {
            matching_start_times.push(process.start_time);
        }
    }

    for process in serve_processes {
        if !is_pid_alive(process.pid) {
            continue;
        }
        let registered_directory = serve_registry
            .iter()
            .find(|entry| entry.pid == process.pid)
            .map(|entry| PathBuf::from(&entry.cwd));
        let matches_directory =
            match registered_directory.or_else(|| cwd_for_pid(process.pid).ok().flatten()) {
                Some(cwd) => paths_share_hierarchy(&canonical_source, &canonical_or_original(&cwd)),
                None => true,
            };
        if matches_directory {
            matching_start_times.push(process.start_time);
        }
    }

    if matching_start_times.is_empty() {
        Some(i64::MAX)
    } else if matching_start_times.contains(&0) {
        None
    } else {
        matching_start_times.into_iter().min()
    }
}

fn paths_share_hierarchy(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

fn resolve_destination_project(destination: &Path) -> anyhow::Result<(String, PathBuf)> {
    let find = || -> anyhow::Result<Option<(String, PathBuf)>> {
        let reader = DbReader::open_default()?;
        Ok(reader
            .get_project_for_directory(destination)?
            .map(|project| (project.id, canonical_or_original(&project.worktree))))
    };

    if let Some(project) = find()? {
        return Ok(project);
    }

    ensure_serve_for_cwd(destination)?;
    for _ in 0..25 {
        if let Some(project) = find()? {
            return Ok(project);
        }
        thread::sleep(Duration::from_millis(200));
    }
    anyhow::bail!(
        "OpenCode did not register a project for {}",
        destination.display()
    )
}

fn move_session_on_serve(port: u16, session_id: &str, destination: &Path) -> anyhow::Result<()> {
    let response = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?
        .post(format!(
            "http://localhost:{port}/experimental/control-plane/move-session"
        ))
        .json(&move_session_payload(session_id, destination))
        .send()?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        anyhow::bail!("OpenCode move failed ({status}): {body}");
    }
    Ok(())
}

fn move_session_payload(session_id: &str, destination: &Path) -> serde_json::Value {
    serde_json::json!({
        "sessionID": session_id,
        "destination": { "directory": destination.to_string_lossy() },
        "moveChanges": false,
    })
}

fn git_tree_is_dirty(directory: &Path) -> bool {
    Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(directory)
        .output()
        .is_ok_and(|output| output.status.success() && !output.stdout.is_empty())
}

fn canonical_or_original(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn reader_with_sessions(label: &str, sessions: &str, messages: &str) -> (DbReader, PathBuf) {
        let path =
            std::env::temp_dir().join(format!("ocmux-move-{label}-{}.db", uuid::Uuid::new_v4()));
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE project (id TEXT PRIMARY KEY, worktree TEXT NOT NULL, name TEXT, time_created INTEGER, time_updated INTEGER);
             CREATE TABLE session (id TEXT PRIMARY KEY, project_id TEXT NOT NULL, parent_id TEXT, title TEXT, directory TEXT, permission TEXT, time_created INTEGER, time_updated INTEGER, time_archived INTEGER);
             CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, data TEXT NOT NULL, time_created INTEGER);
             CREATE TABLE part (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, message_id TEXT NOT NULL, data TEXT NOT NULL, time_created INTEGER);
             INSERT INTO project VALUES ('proj', '/tmp/repo', 'repo', 1, 2);",
        )
        .unwrap();
        conn.execute_batch(sessions).unwrap();
        conn.execute_batch(messages).unwrap();
        drop(conn);
        (DbReader::open(&path).unwrap(), path)
    }

    #[test]
    fn move_guard_allows_superseded_child_session() {
        let (reader, path) = reader_with_sessions(
            "superseded-child",
            "INSERT INTO session VALUES ('parent', 'proj', NULL, 'Parent', '/tmp/repo', '{}', 1, 5000, NULL);
             INSERT INTO session VALUES ('child', 'proj', 'parent', 'Child', '/tmp/repo', '{}', 2, 2000, NULL);",
            "INSERT INTO message VALUES ('parent-user', 'parent', '{\"role\":\"user\"}', 5000);
             INSERT INTO message VALUES ('parent-answer', 'parent', '{\"role\":\"assistant\",\"time\":{\"completed\":5001}}', 5001);
             INSERT INTO message VALUES ('child-user', 'child', '{\"role\":\"user\"}', 2000);",
        );
        let ids = reader.get_session_tree_ids("parent").unwrap();
        assert!(ensure_session_tree_not_busy(&reader, &ids, None).is_ok());
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn move_guard_reports_active_sibling_even_if_parent_is_in_error() {
        let (reader, path) = reader_with_sessions(
            "active-sibling",
            "INSERT INTO session VALUES ('parent', 'proj', NULL, 'Parent', '/tmp/repo', '{}', 1, 5000, NULL);
             INSERT INTO session VALUES ('working', 'proj', 'parent', 'Working', '/tmp/repo', '{}', 2, 6000, NULL);",
            "INSERT INTO message VALUES ('parent-error', 'parent', '{\"role\":\"assistant\",\"error\":{\"name\":\"ProviderError\"}}', 5000);
             INSERT INTO message VALUES ('working-user', 'working', '{\"role\":\"user\"}', 6000);",
        );
        let ids = reader.get_session_tree_ids("parent").unwrap();
        let error = ensure_session_tree_not_busy(&reader, &ids, None).unwrap_err();
        assert_eq!(error.to_string(), "session tree is busy (working: Working)");
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn relative_path_is_empty_at_worktree_root() {
        let temp = std::env::temp_dir().join(format!("ocmux-move-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp).unwrap();
        assert_eq!(project_relative_path(&temp, &temp).unwrap(), "");
        std::fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn relative_path_uses_forward_slashes_for_nested_directory() {
        let temp = std::env::temp_dir().join(format!("ocmux-move-{}", uuid::Uuid::new_v4()));
        let nested = temp.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        assert_eq!(project_relative_path(&temp, &nested).unwrap(), "nested");
        std::fs::remove_dir_all(temp).ok();
    }

    #[test]
    fn api_payload_never_moves_working_tree_changes() {
        assert_eq!(
            move_session_payload("ses_123", Path::new("/tmp/destination")),
            serde_json::json!({
                "sessionID": "ses_123",
                "destination": { "directory": "/tmp/destination" },
                "moveChanges": false,
            })
        );
    }
}
