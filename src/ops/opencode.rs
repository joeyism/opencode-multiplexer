use std::path::Path;

use portable_pty::CommandBuilder;

pub fn build_managed_session_command(cwd: &Path) -> CommandBuilder {
    let mut command = CommandBuilder::new("opencode");
    command.cwd(cwd);
    command
}

pub fn build_replica_command(cwd: &Path, session_id: &str) -> CommandBuilder {
    let mut command = CommandBuilder::new("opencode");
    command.args(["-s", session_id]);
    command.cwd(cwd);
    command
}

pub fn display_title_for_cwd(cwd: &Path) -> String {
    cwd.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| cwd.display().to_string())
}

use std::net::TcpListener;
use std::process::{Command, Stdio};

pub fn find_available_port(start: u16) -> u16 {
    for port in start..start + 100 {
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return port;
        }
    }
    start
}

pub fn spawn_serve_daemon(cwd: &Path, port: u16) -> anyhow::Result<u32> {
    let child = Command::new("opencode")
        .args(["serve", "--port", &port.to_string()])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(child.id())
}

pub fn wait_for_serve_ready(port: u16, timeout_secs: u64) -> bool {
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(timeout_secs);
    while start.elapsed() < timeout {
        if let Ok(resp) = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_millis(500))
            .build()
            .and_then(|c| c.get(format!("http://localhost:{port}/session")).send())
            && resp.status().is_success()
        {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    false
}

pub fn fetch_serve_session_ids(port: u16) -> anyhow::Result<std::collections::HashSet<String>> {
    let resp = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()?
        .get(format!("http://localhost:{port}/session"))
        .send()?;
    if !resp.status().is_success() {
        return Ok(std::collections::HashSet::new());
    }
    let json: serde_json::Value = resp.json()?;
    let ids = json
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| Some(entry.get("id")?.as_str()?.to_string()))
        .collect();
    Ok(ids)
}

/// Poll the serve until the session count stabilizes across consecutive polls.
/// A freshly started serve may still be loading sessions from the DB, so a
/// single fetch can return an incomplete set. This waits for the count to
/// remain unchanged for 3 consecutive polls (~600ms of stability) before
/// returning, with a 5-second timeout as a safety net.
pub fn fetch_stable_session_ids(
    port: u16,
) -> anyhow::Result<std::collections::HashSet<String>> {
    let mut prev_count = 0usize;
    let mut stable_ticks = 0u8;
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(5);
    loop {
        let ids = fetch_serve_session_ids(port)?;
        if ids.len() == prev_count {
            stable_ticks += 1;
            if stable_ticks >= 3 {
                return Ok(ids);
            }
        } else {
            stable_ticks = 0;
            prev_count = ids.len();
        }
        if start.elapsed() >= timeout {
            return Ok(ids); // best effort
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

/// Wait for a new session to appear on the serve port that wasn't in `before_ids`.
/// Returns the new session ID if found within the timeout.
///
/// When multiple new IDs appear (e.g. the serve was still loading when
/// `before_ids` was captured), prefers the most recently updated session
/// via a DB lookup rather than picking an arbitrary one.
pub fn wait_for_new_session_id(
    port: u16,
    before_ids: &std::collections::HashSet<String>,
    timeout_secs: u64,
) -> Option<String> {
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(timeout_secs);
    while start.elapsed() < timeout {
        if let Ok(current_ids) = fetch_serve_session_ids(port) {
            let new_ids: Vec<_> = current_ids.difference(before_ids).cloned().collect();
            if new_ids.len() == 1 {
                return Some(new_ids.into_iter().next().unwrap());
            }
            if new_ids.len() > 1 {
                // Multiple new sessions — pick the most recently updated one.
                // This handles the case where the serve was still loading
                // when before_ids was captured.
                if let Some(id) = pick_most_recent_session_id(&new_ids) {
                    return Some(id);
                }
                // Fallback: return first if DB lookup fails
                return new_ids.into_iter().next();
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    None
}

/// Given a list of session IDs, query the DB and return the one with the
/// highest `time_updated`. Returns `None` if the DB can't be opened or
/// none of the sessions exist.
pub fn pick_most_recent_session_id(new_ids: &[String]) -> Option<String> {
    use crate::data::db::reader::DbReader;

    let reader = DbReader::open_default().ok()?;
    pick_most_recent_session_id_with_reader(new_ids, &reader)
}

/// Testable variant that accepts an existing [`DbReader`].
pub fn pick_most_recent_session_id_with_reader(
    new_ids: &[String],
    reader: &crate::data::db::reader::DbReader,
) -> Option<String> {
    let mut best: Option<(String, i64)> = None;
    for id in new_ids {
        if let Ok(Some(session)) = reader.get_session_by_id(id) {
            if best
                .as_ref()
                .is_none_or(|(_, t)| session.time_updated > *t)
            {
                best = Some((id.clone(), session.time_updated));
            }
        }
    }
    best.map(|(id, _)| id)
}
