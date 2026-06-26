use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context;
use crate::data::discovery::ps::{scan_serve_processes, ParsedServeProcess};

pub fn load_managed_sessions() -> anyhow::Result<HashSet<String>> {
    load_managed_sessions_from_path(&default_managed_sessions_path()?)
}

pub fn load_managed_sessions_from_path(path: &Path) -> anyhow::Result<HashSet<String>> {
    if !path.exists() {
        return Ok(HashSet::new());
    }
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let values: Vec<String> = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(values.into_iter().collect())
}

pub fn save_managed_sessions<I, S>(sessions: I) -> anyhow::Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    save_managed_sessions_to_path(&default_managed_sessions_path()?, sessions)
}

pub fn save_managed_sessions_to_path<I, S>(path: &Path, sessions: I) -> anyhow::Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut values = sessions
        .into_iter()
        .map(|value| value.as_ref().to_string())
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    fs::write(path, serde_json::to_string_pretty(&values)?)?;
    Ok(())
}

fn default_managed_sessions_path() -> anyhow::Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".config/ocmux/managed-sessions.json"))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ServeEntry {
    pub port: u16,
    pub pid: u32,
    pub cwd: String,
    pub tui_pid: Option<u32>,
}

pub fn load_serve_registry() -> anyhow::Result<Vec<ServeEntry>> {
    let path = default_serve_registry_path()?;
    if !path.exists() {
        return Ok(vec![]);
    }
    let raw = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&raw).unwrap_or_default())
}

pub fn save_serve_registry(entries: &[ServeEntry]) -> anyhow::Result<()> {
    let path = default_serve_registry_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, serde_json::to_string_pretty(entries)?)?;
    Ok(())
}

pub fn cleanup_stale_serve_entries() -> anyhow::Result<Vec<ServeEntry>> {
    let entries = load_serve_registry()?;
    let alive: Vec<ServeEntry> = entries
        .into_iter()
        .filter(|entry| is_pid_alive(entry.pid))
        .map(|mut entry| {
            if entry.tui_pid.is_some_and(|pid| !is_pid_alive(pid)) {
                entry.tui_pid = None;
            }
            entry
        })
        .collect();
    save_serve_registry(&alive)?;
    Ok(alive)
}

pub fn register_serve_process(port: u16, pid: u32, cwd: &Path) -> anyhow::Result<()> {
    let mut entries = load_serve_registry().unwrap_or_default();
    // Remove entries with the same PID (reuse) or the same port (port recycled
    // after previous serve died). This prevents stale entries from accumulating
    // and ensures update_serve_registry_tui_pid finds the correct entry.
    entries.retain(|e| e.pid != pid && e.port != port);
    entries.push(ServeEntry {
        port,
        pid,
        cwd: cwd.display().to_string(),
        tui_pid: None,
    });
    save_serve_registry(&entries)
}

pub fn update_serve_registry_tui_pid(port: u16, tui_pid: u32) -> anyhow::Result<()> {
    let mut entries = load_serve_registry().unwrap_or_default();
    if let Some(entry) = entries.iter_mut().find(|e| e.port == port) {
        entry.tui_pid = Some(tui_pid);
        save_serve_registry(&entries)?;
    }
    Ok(())
}

#[cfg(unix)]
pub(crate) fn is_pid_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[cfg(windows)]
pub(crate) fn is_pid_alive(pid: u32) -> bool {
    if let Ok(output) = std::process::Command::new("tasklist")
        .arg("/FI")
        .arg(format!("PID eq {}", pid))
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        stdout.contains(&pid.to_string())
    } else {
        true
    }
}

fn default_serve_registry_path() -> anyhow::Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".config/ocmux/serve-processes.json"))
}

#[cfg(unix)]
pub(crate) fn kill_pid(pid: u32) {
    unsafe {
        libc::kill(pid as i32, libc::SIGTERM);
    }
}

#[cfg(windows)]
pub(crate) fn kill_pid(pid: u32) {
    let _ = std::process::Command::new("taskkill")
        .arg("/F")
        .arg("/PID")
        .arg(pid.to_string())
        .output();
}

/// Testable: given a list of serve processes and registry entries,
/// return the PIDs of orphaned serves (not in the registry).
pub fn find_orphaned_serve_pids(
    serve_processes: &[ParsedServeProcess],
    registry: &[ServeEntry],
) -> Vec<u32> {
    let registry_pids: HashSet<u32> = registry.iter().map(|e| e.pid).collect();
    serve_processes
        .iter()
        .filter(|p| !registry_pids.contains(&p.pid))
        .map(|p| p.pid)
        .collect()
}

/// Kill orphaned `opencode serve` processes (not in the serve registry).
/// These are leftover daemons from previous ocmux sessions that were never
/// cleaned up. They hammer the same SQLite DB as managed serves, causing
/// lock contention. Called at startup after `cleanup_stale_serve_entries`.
pub fn cleanup_orphaned_serve_processes() -> anyhow::Result<usize> {
    let registry = load_serve_registry().unwrap_or_default();
    let serve_processes = scan_serve_processes().unwrap_or_default();
    let orphaned = find_orphaned_serve_pids(&serve_processes, &registry);
    let count = orphaned.len();
    for pid in &orphaned {
        kill_pid(*pid);
    }
    Ok(count)
}
