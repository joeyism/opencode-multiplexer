use std::{
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{
    app::sessions::SessionStatus,
    data::db::{reader::DbReader, writer::DbWriter},
    ops::opencode::ensure_serve_for_cwd,
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
    if session_ids.is_empty() {
        anyhow::bail!("session not found");
    }
    for id in &session_ids {
        let status = reader.get_session_status(id, None)?;
        if matches!(
            status,
            SessionStatus::Working | SessionStatus::SubagentsWorking | SessionStatus::NeedsInput
        ) {
            anyhow::bail!("session tree is busy ({id}: {status:?})");
        }
    }

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
