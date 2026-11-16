use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::data::discovery::ps::scan_serve_processes;
use crate::registry::{ServeEntry, load_serve_registry};

pub fn diff_head_files(cwd: &Path, files: &[String]) -> anyhow::Result<String> {
    let relative = repo_relative_session_files(cwd, files)?;
    if relative.is_empty() {
        return Ok(String::new());
    }
    let output = Command::new("git")
        .arg("diff")
        .arg("--no-ext-diff")
        .arg("--no-color")
        .arg("HEAD")
        .arg("--")
        .args(&relative)
        .current_dir(cwd)
        .output()?;
    // git diff exits 0 on no diff and 1 on diff (with --exit-code), but
    // without --exit-code it always exits 0. Treat any output as success.
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Produce a unified diff of all uncommitted changes in the working tree,
/// including both tracked modifications/deletions and untracked new files.
pub fn diff_worktree(cwd: &Path) -> anyhow::Result<String> {
    let mut result = String::new();

    // 1. Tracked changes (modified, deleted, staged).
    let tracked = Command::new("git")
        .args(["diff", "--no-ext-diff", "--no-color", "HEAD"])
        .current_dir(cwd)
        .output()?;
    let tracked_diff = String::from_utf8_lossy(&tracked.stdout);
    result.push_str(&tracked_diff);

    // 2. Untracked files.
    let ls_output = Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard"])
        .current_dir(cwd)
        .output()?;
    let untracked_list = String::from_utf8_lossy(&ls_output.stdout);

    for file in untracked_list.lines() {
        let file = file.trim();
        if file.is_empty() {
            continue;
        }
        // Skip .opencode/ internal files.
        if file.starts_with(".opencode/") || file.starts_with(".opencode\\") {
            continue;
        }

        // `git diff --no-index` exits 1 when files differ (not an error).
        let no_index = Command::new("git")
            .args([
                "diff",
                "--no-ext-diff",
                "--no-color",
                "--no-index",
                "--",
                "/dev/null",
                file,
            ])
            .current_dir(cwd)
            .output()?;

        let chunk = String::from_utf8_lossy(&no_index.stdout);
        if !chunk.is_empty() {
            if !result.is_empty() && !result.ends_with('\n') {
                result.push('\n');
            }
            result.push_str(&chunk);
        }
    }

    Ok(result)
}

pub fn fetch_session_diff_from_serve(session_id: &str, session_cwd: &Path) -> Option<String> {
    let port = find_serve_port_for_cwd(session_cwd)?;

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .ok()?;

    let url = format!("http://localhost:{port}/session/{session_id}/diff");
    let response = client.get(&url).send().ok()?;

    if !response.status().is_success() {
        return None;
    }

    let json: serde_json::Value = response.json().ok()?;
    let diffs = json.as_array()?;

    let mut combined = String::new();
    for entry in diffs {
        if let Some(diff_text) = entry.get("diff").and_then(|d| d.as_str())
            && !diff_text.is_empty()
        {
            if !combined.is_empty() {
                combined.push('\n');
            }
            combined.push_str(diff_text);
        }
    }

    // Only return Some if we actually got non-empty diff content.
    // An empty array or array with only empty diff fields means the API
    // has no useful data — fall through to the git-based diff.
    if combined.is_empty() {
        None
    } else {
        Some(combined)
    }
}

/// Find the serve process port that matches the given session working directory.
///
/// Checks both the serve registry file and live `ps` output. Prefers an exact
/// canonical path match; falls back to the entry sharing the longest common
/// ancestor with `session_cwd`.
pub fn find_serve_port_for_cwd(session_cwd: &Path) -> Option<u16> {
    let canon_session =
        std::fs::canonicalize(session_cwd).unwrap_or_else(|_| session_cwd.to_path_buf());

    let mut candidates: Vec<(u16, PathBuf)> = Vec::new();

    if let Ok(entries) = load_serve_registry() {
        for entry in &entries {
            let entry_path = PathBuf::from(&entry.cwd);
            let canon_entry = std::fs::canonicalize(&entry_path).unwrap_or(entry_path);
            candidates.push((entry.port, canon_entry));
        }
    }

    // Supplement with live processes — the registry entry has the cwd but
    // `scan_serve_processes` only gives us pid+port. Cross-reference them
    // against registry entries we already collected (registry is the only
    // source of cwd for a serve process).
    if let Ok(live) = scan_serve_processes() {
        let live_ports: Vec<u16> = live.iter().map(|p| p.port).collect();
        // Keep only candidates whose port is actually alive.
        candidates.retain(|(port, _)| live_ports.contains(port));
    }

    // Exact match first.
    for (port, path) in &candidates {
        if *path == canon_session {
            return Some(*port);
        }
    }

    // Fall back to longest common ancestor match.
    let mut best: Option<(u16, usize)> = None;
    for (port, path) in &candidates {
        let depth = common_ancestor_depth(&canon_session, path);
        if depth > 0 && best.is_none_or(|(_, d)| depth > d) {
            best = Some((*port, depth));
        }
    }

    best.map(|(port, _)| port)
}

/// Count the number of shared leading path components between two paths.
fn common_ancestor_depth(a: &Path, b: &Path) -> usize {
    a.components()
        .zip(b.components())
        .take_while(|(ca, cb)| ca == cb)
        .count()
}

/// Find the serve port for a cwd using a caller-provided list of entries.
/// Useful when the caller already has the registry loaded or in tests.
pub fn find_serve_port_for_cwd_with_entries(
    session_cwd: &Path,
    entries: &[ServeEntry],
) -> Option<u16> {
    let canon_session =
        std::fs::canonicalize(session_cwd).unwrap_or_else(|_| session_cwd.to_path_buf());

    let candidates: Vec<(u16, PathBuf)> = entries
        .iter()
        .map(|e| {
            let p = PathBuf::from(&e.cwd);
            let canon = std::fs::canonicalize(&p).unwrap_or(p);
            (e.port, canon)
        })
        .collect();

    // Exact match first.
    for (port, path) in &candidates {
        if *path == canon_session {
            return Some(*port);
        }
    }

    // Longest common ancestor.
    let mut best: Option<(u16, usize)> = None;
    for (port, path) in &candidates {
        let depth = common_ancestor_depth(&canon_session, path);
        if depth > 0 && best.is_none_or(|(_, d)| depth > d) {
            best = Some((*port, depth));
        }
    }

    best.map(|(port, _)| port)
}

pub fn resolve_serve_port_with_entries(
    explicit: Option<u16>,
    session_cwd: &Path,
    entries: &[ServeEntry],
) -> Option<u16> {
    explicit.or_else(|| find_serve_port_for_cwd_with_entries(session_cwd, entries))
}

pub fn resolve_serve_port(explicit: Option<u16>, session_cwd: &Path) -> Option<u16> {
    explicit.or_else(|| find_serve_port_for_cwd(session_cwd))
}

pub fn commit_and_push_files(
    cwd: &Path,
    created: &[String],
    modified: &[String],
    deleted: &[String],
    message: &str,
) -> anyhow::Result<String> {
    let mut to_add = Vec::new();
    to_add.extend_from_slice(created);
    to_add.extend_from_slice(modified);
    to_add.extend_from_slice(deleted);

    if to_add.is_empty() {
        anyhow::bail!("no files to commit");
    }

    // git add <files>
    let mut add = Command::new("git");
    add.arg("add").args(&to_add).current_dir(cwd);
    let add_out = add.output()?;

    // git commit -m <msg>
    let mut commit = Command::new("git");
    commit.args(["commit", "-m", message]).current_dir(cwd);
    let commit_out = commit.output()?;

    // git push origin HEAD
    let mut push = Command::new("git");
    push.args(["push", "origin", "HEAD"]).current_dir(cwd);
    let push_out = push.output()?;

    // Combine all output
    let mut output = String::new();
    for (label, out) in [
        ("add", &add_out),
        ("commit", &commit_out),
        ("push", &push_out),
    ] {
        output.push_str(&format!("--- git {label} ---\n"));
        output.push_str(&String::from_utf8_lossy(&out.stdout));
        output.push_str(&String::from_utf8_lossy(&out.stderr));
        output.push('\n');
    }
    Ok(output)
}

pub fn get_file_statuses(
    cwd: &Path,
    session_files: &[String],
) -> anyhow::Result<(Vec<String>, Vec<String>, Vec<String>)> {
    let session_files = repo_relative_session_files(cwd, session_files)?;
    let mut created = Vec::new();
    let mut modified = Vec::new();
    let mut deleted = Vec::new();

    let output = Command::new("git")
        .arg("status")
        .arg("--porcelain")
        .current_dir(cwd)
        .output()?;

    if !output.status.success() {
        return Ok((created, modified, deleted));
    }

    let status_output = String::from_utf8_lossy(&output.stdout);
    for line in status_output.lines() {
        if line.len() < 4 {
            continue;
        }
        let code = &line[0..2];
        let file_path = &line[3..];

        if code.contains('D') {
            deleted.push(file_path.to_string());
            continue;
        }

        let mut matched = false;
        for sf in &session_files {
            if sf.ends_with(file_path) || file_path.ends_with(sf) {
                matched = true;
                break;
            }
        }
        if !matched {
            continue;
        }

        if code.contains('?') || code.contains('A') {
            created.push(file_path.to_string());
        } else if code.contains('M') {
            modified.push(file_path.to_string());
        }
    }

    Ok((created, modified, deleted))
}

fn repo_root(cwd: &Path) -> anyhow::Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd)
        .output()?;
    if !output.status.success() {
        anyhow::bail!("not inside a git repository");
    }
    let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim().to_string());
    Ok(std::fs::canonicalize(&path).unwrap_or(path))
}

pub fn repo_relative_session_files(
    cwd: &Path,
    session_files: &[String],
) -> anyhow::Result<Vec<String>> {
    let root = repo_root(cwd)?;
    let root_str = root.display().to_string();

    let mut files = Vec::new();
    for file in session_files {
        if file.starts_with(&root_str) {
            let rel = file
                .strip_prefix(&root_str)
                .unwrap()
                .trim_start_matches(['/', '\\']);
            files.push(rel.to_string());
        } else if !Path::new(file).is_absolute() {
            files.push(file.clone());
        } else {
            // Absolute path but doesn't strictly string-match root.
            // On macOS, /var symlinks to /private/var. Try fs::canonicalize.
            if let Ok(canon_file) = std::fs::canonicalize(file)
                && let Ok(canon_root) = std::fs::canonicalize(&root)
                && let Ok(rel) = canon_file.strip_prefix(&canon_root)
            {
                files.push(rel.display().to_string());
                continue;
            }
        }
    }
    files.sort();
    files.dedup();
    files.retain(|f| !f.starts_with(".opencode/") && !f.starts_with(".opencode\\"));
    Ok(files)
}
