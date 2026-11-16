use std::path::{Path, PathBuf};

use portable_pty::CommandBuilder;

use crate::registry::{ServeEntry, is_pid_alive, load_serve_registry, register_serve_process};

pub fn build_managed_session_command(cwd: &Path, port: u16) -> CommandBuilder {
    let mut command = CommandBuilder::new("opencode");
    command.args(["attach", &format!("http://localhost:{port}")]);
    command.cwd(cwd);
    command
}

pub fn build_replica_command(cwd: &Path, session_id: &str, port: Option<u16>) -> CommandBuilder {
    let mut command = CommandBuilder::new("opencode");
    match port {
        Some(p) => {
            command.args([
                "attach",
                &format!("http://localhost:{p}"),
                "--session",
                session_id,
            ]);
        }
        None => {
            command.args(["-s", session_id]);
        }
    }
    command.cwd(cwd);
    command
}

pub fn display_title_for_cwd(cwd: &Path) -> String {
    cwd.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| cwd.display().to_string())
}

pub fn select_exact_serve_for_cwd(
    session_cwd: &Path,
    entries: &[ServeEntry],
) -> Option<(u16, u32)> {
    let canonical_cwd =
        std::fs::canonicalize(session_cwd).unwrap_or_else(|_| PathBuf::from(session_cwd));
    entries.iter().find_map(|entry| {
        let entry_cwd = PathBuf::from(&entry.cwd);
        let canonical_entry = std::fs::canonicalize(&entry_cwd).unwrap_or(entry_cwd);
        (canonical_entry == canonical_cwd).then_some((entry.port, entry.pid))
    })
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

pub fn ensure_serve_for_cwd(cwd: &Path) -> anyhow::Result<(u16, u32)> {
    if let Some((port, pid)) =
        select_exact_serve_for_cwd(cwd, &load_serve_registry().unwrap_or_default())
        && is_pid_alive(pid)
    {
        return Ok((port, pid));
    }

    let port = find_available_port(4200);
    let pid = spawn_serve_daemon(cwd, port)?;
    register_serve_process(port, pid, cwd)?;
    if !wait_for_serve_ready(port, 10) {
        anyhow::bail!("opencode serve did not start within 10s on port {port}");
    }
    Ok((port, pid))
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

pub fn fetch_pending_permissions(port: u16) -> anyhow::Result<Vec<(String, String)>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()?;
    let resp = client
        .get(format!("http://localhost:{port}/permission"))
        .send()?;
    if !resp.status().is_success() {
        return Ok(vec![]);
    }
    let json: serde_json::Value = resp.json()?;
    // Handle both top-level array and { "data": [...] }
    let items = if let Some(arr) = json.as_array() {
        arr
    } else if let Some(arr) = json.get("data").and_then(|v| v.as_array()) {
        arr
    } else {
        return Ok(vec![]);
    };

    let mut result = Vec::new();
    for item in items {
        if let (Some(id), Some(session_id)) = (
            item.get("id").and_then(|v| v.as_str()),
            item.get("sessionID").and_then(|v| v.as_str()),
        ) {
            result.push((id.to_string(), session_id.to_string()));
        }
    }
    Ok(result)
}

pub fn fetch_pending_questions(port: u16) -> anyhow::Result<Vec<(String, String)>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()?;
    let resp = client
        .get(format!("http://localhost:{port}/question"))
        .send()?;
    if !resp.status().is_success() {
        return Ok(vec![]);
    }
    let json: serde_json::Value = resp.json()?;
    let items = if let Some(arr) = json.as_array() {
        arr
    } else if let Some(arr) = json.get("data").and_then(|v| v.as_array()) {
        arr
    } else {
        return Ok(vec![]);
    };

    let mut result = Vec::new();
    for item in items {
        if let (Some(id), Some(session_id)) = (
            item.get("id").and_then(|v| v.as_str()),
            item.get("sessionID").and_then(|v| v.as_str()),
        ) {
            result.push((id.to_string(), session_id.to_string()));
        }
    }
    Ok(result)
}
