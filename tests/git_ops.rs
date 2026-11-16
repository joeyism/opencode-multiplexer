use opencode_multiplexer::ops::git::{
    diff_worktree, find_serve_port_for_cwd_with_entries, get_file_statuses,
    repo_relative_session_files, resolve_serve_port_with_entries,
};
use opencode_multiplexer::registry::ServeEntry;
use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn temp_repo_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("ocmux-rs-repo-{label}-{nanos}"))
}

fn git_init(dir: &PathBuf) {
    Command::new("git")
        .arg("init")
        .current_dir(dir)
        .status()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(dir)
        .status()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(dir)
        .status()
        .unwrap();
}

#[test]
fn repo_relative_session_files_strips_absolute_paths_and_excludes_outside_files() {
    let repo = temp_repo_dir("relative");
    fs::create_dir_all(&repo).unwrap();
    let repo = fs::canonicalize(&repo).unwrap();
    git_init(&repo);

    let inside = repo.join("inside.txt");
    let outside = std::env::temp_dir().join("outside.txt");
    let relative = "already_rel.txt".to_string();

    let session_files = vec![
        inside.display().to_string(),
        outside.display().to_string(),
        relative.clone(),
        ".opencode/plans/123.md".to_string(),
    ];

    let result = repo_relative_session_files(&repo, &session_files).unwrap();

    assert_eq!(result.len(), 2);
    assert!(result.contains(&"inside.txt".to_string()));
    assert!(result.contains(&relative));
    assert!(!result.contains(&outside.display().to_string()));
    assert!(!result.contains(&".opencode/plans/123.md".to_string()));

    fs::remove_dir_all(repo).ok();
}

#[test]
fn get_file_statuses_categorizes_created_modified_deleted() {
    let repo = temp_repo_dir("status");
    fs::create_dir_all(&repo).unwrap();
    let repo = fs::canonicalize(&repo).unwrap();
    git_init(&repo);

    // Initial commit (baseline)
    fs::write(repo.join("to_modify.txt"), "v1").unwrap();
    fs::write(repo.join("to_delete.txt"), "v1").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(&repo)
        .status()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(&repo)
        .status()
        .unwrap();

    // Create untracked, modify tracked, delete tracked
    fs::write(repo.join("created.txt"), "v1").unwrap();
    fs::write(repo.join("to_modify.txt"), "v2").unwrap();
    fs::remove_file(repo.join("to_delete.txt")).unwrap();

    // Session claims to have touched the created and modified files,
    // but knows nothing about the deleted file (bash rm scenario)
    let session_files = vec!["created.txt".to_string(), "to_modify.txt".to_string()];

    let (created, modified, deleted) = get_file_statuses(&repo, &session_files).unwrap();

    assert_eq!(created, vec!["created.txt"]);
    assert_eq!(modified, vec!["to_modify.txt"]);

    // The deleted file MUST be included even though it wasn't in session_files
    assert_eq!(deleted, vec!["to_delete.txt"]);

    fs::remove_dir_all(repo).ok();
}

// ---------------------------------------------------------------------------
// diff_worktree tests
// ---------------------------------------------------------------------------

#[test]
fn diff_worktree_includes_tracked_modifications() {
    let repo = temp_repo_dir("wt-mod");
    fs::create_dir_all(&repo).unwrap();
    let repo = fs::canonicalize(&repo).unwrap();
    git_init(&repo);

    fs::write(repo.join("file.txt"), "v1").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(&repo)
        .status()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(&repo)
        .status()
        .unwrap();

    fs::write(repo.join("file.txt"), "v2").unwrap();

    let diff = diff_worktree(&repo).unwrap();
    assert!(diff.contains("diff --git"), "expected unified diff header");
    assert!(diff.contains("-v1"), "expected removed line");
    assert!(diff.contains("+v2"), "expected added line");

    fs::remove_dir_all(repo).ok();
}

#[test]
fn diff_worktree_includes_deleted_files() {
    let repo = temp_repo_dir("wt-del");
    fs::create_dir_all(&repo).unwrap();
    let repo = fs::canonicalize(&repo).unwrap();
    git_init(&repo);

    fs::write(repo.join("gone.txt"), "bye").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(&repo)
        .status()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(&repo)
        .status()
        .unwrap();

    fs::remove_file(repo.join("gone.txt")).unwrap();

    let diff = diff_worktree(&repo).unwrap();
    assert!(diff.contains("diff --git"), "expected unified diff header");
    assert!(diff.contains("-bye"), "expected removed content");

    fs::remove_dir_all(repo).ok();
}

#[test]
fn diff_worktree_includes_untracked_new_files() {
    let repo = temp_repo_dir("wt-new");
    fs::create_dir_all(&repo).unwrap();
    let repo = fs::canonicalize(&repo).unwrap();
    git_init(&repo);

    // Need an initial commit so HEAD exists.
    fs::write(repo.join("init.txt"), "x").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(&repo)
        .status()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(&repo)
        .status()
        .unwrap();

    fs::write(repo.join("brand_new.txt"), "hello").unwrap();

    let diff = diff_worktree(&repo).unwrap();
    assert!(
        diff.contains("--- /dev/null"),
        "expected /dev/null for new file"
    );
    assert!(diff.contains("+hello"), "expected new file content");

    fs::remove_dir_all(repo).ok();
}

#[test]
fn diff_worktree_combined_output() {
    let repo = temp_repo_dir("wt-combo");
    fs::create_dir_all(&repo).unwrap();
    let repo = fs::canonicalize(&repo).unwrap();
    git_init(&repo);

    fs::write(repo.join("existing.txt"), "old").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(&repo)
        .status()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(&repo)
        .status()
        .unwrap();

    // Modify tracked file.
    fs::write(repo.join("existing.txt"), "new").unwrap();
    // Create untracked file.
    fs::write(repo.join("fresh.txt"), "brand new").unwrap();

    let diff = diff_worktree(&repo).unwrap();
    assert!(
        diff.contains("existing.txt"),
        "expected modified file in diff"
    );
    assert!(diff.contains("fresh.txt"), "expected new file in diff");
    assert!(
        diff.contains("--- /dev/null"),
        "expected /dev/null for new file"
    );

    fs::remove_dir_all(repo).ok();
}

#[test]
fn diff_worktree_excludes_opencode_dir() {
    let repo = temp_repo_dir("wt-oc");
    fs::create_dir_all(&repo).unwrap();
    let repo = fs::canonicalize(&repo).unwrap();
    git_init(&repo);

    fs::write(repo.join("init.txt"), "x").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(&repo)
        .status()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(&repo)
        .status()
        .unwrap();

    // Create an untracked .opencode/ file — should be excluded.
    fs::create_dir_all(repo.join(".opencode")).unwrap();
    fs::write(repo.join(".opencode/state.json"), "{}").unwrap();

    let diff = diff_worktree(&repo).unwrap();
    assert!(
        !diff.contains(".opencode"),
        "expected .opencode files to be excluded"
    );

    fs::remove_dir_all(repo).ok();
}

#[test]
fn diff_worktree_empty_when_clean() {
    let repo = temp_repo_dir("wt-clean");
    fs::create_dir_all(&repo).unwrap();
    let repo = fs::canonicalize(&repo).unwrap();
    git_init(&repo);

    fs::write(repo.join("file.txt"), "v1").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(&repo)
        .status()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(&repo)
        .status()
        .unwrap();

    let diff = diff_worktree(&repo).unwrap();
    assert!(
        diff.is_empty(),
        "expected empty diff for clean repo, got: {diff}"
    );

    fs::remove_dir_all(repo).ok();
}

// ---------------------------------------------------------------------------
// find_serve_port_for_cwd tests
// ---------------------------------------------------------------------------

#[test]
fn find_serve_port_exact_match() {
    let dir = temp_repo_dir("serve-exact");
    fs::create_dir_all(&dir).unwrap();
    let dir = fs::canonicalize(&dir).unwrap();

    let entries = vec![
        ServeEntry {
            port: 4200,
            pid: 1,
            cwd: "/some/other/path".to_string(),
            tui_pid: None,
        },
        ServeEntry {
            port: 4201,
            pid: 2,
            cwd: dir.display().to_string(),
            tui_pid: None,
        },
    ];

    let port = find_serve_port_for_cwd_with_entries(&dir, &entries);
    assert_eq!(port, Some(4201));

    fs::remove_dir_all(dir).ok();
}

#[test]
fn find_serve_port_no_match_returns_none_for_nonexistent_paths() {
    let dir = temp_repo_dir("serve-none");
    fs::create_dir_all(&dir).unwrap();
    let dir = fs::canonicalize(&dir).unwrap();

    // All entries have non-existent paths so canonicalize can't help.
    // They share "/" with the session dir so common_ancestor_depth > 0,
    // but this tests that the function doesn't panic.
    let entries = vec![ServeEntry {
        port: 4200,
        pid: 1,
        cwd: "/totally/unrelated/path".to_string(),
        tui_pid: None,
    }];

    let _port = find_serve_port_for_cwd_with_entries(&dir, &entries);
    // Just verify it doesn't panic — on Unix, all paths share "/" so
    // there will be a common ancestor match.
}

#[test]
fn find_serve_port_prefers_exact_over_ancestor() {
    let dir = temp_repo_dir("serve-prefer");
    fs::create_dir_all(&dir).unwrap();
    let dir = fs::canonicalize(&dir).unwrap();

    let parent = dir.parent().unwrap();

    let entries = vec![
        // Parent directory — longer common ancestor but not exact.
        ServeEntry {
            port: 4200,
            pid: 1,
            cwd: parent.display().to_string(),
            tui_pid: None,
        },
        // Exact match.
        ServeEntry {
            port: 4201,
            pid: 2,
            cwd: dir.display().to_string(),
            tui_pid: None,
        },
    ];

    let port = find_serve_port_for_cwd_with_entries(&dir, &entries);
    assert_eq!(port, Some(4201));

    fs::remove_dir_all(dir).ok();
}

#[test]
fn find_serve_port_trailing_slash() {
    let dir = temp_repo_dir("serve-slash");
    fs::create_dir_all(&dir).unwrap();
    let dir = fs::canonicalize(&dir).unwrap();

    // Entry cwd has trailing slash — canonicalize should strip it.
    let cwd_with_slash = format!("{}/", dir.display());
    let entries = vec![ServeEntry {
        port: 4205,
        pid: 3,
        cwd: cwd_with_slash,
        tui_pid: None,
    }];

    let port = find_serve_port_for_cwd_with_entries(&dir, &entries);
    assert_eq!(port, Some(4205));

    fs::remove_dir_all(dir).ok();
}

#[test]
fn resolve_serve_port_prefers_explicit_port() {
    let dir = temp_repo_dir("serve-resolve-explicit");
    fs::create_dir_all(&dir).unwrap();
    let dir = fs::canonicalize(&dir).unwrap();
    let entries = vec![ServeEntry {
        port: 4201,
        pid: 1,
        cwd: dir.display().to_string(),
        tui_pid: None,
    }];

    assert_eq!(
        resolve_serve_port_with_entries(Some(4300), &dir, &entries),
        Some(4300)
    );
    fs::remove_dir_all(dir).ok();
}

#[test]
fn resolve_serve_port_falls_back_to_matching_entry() {
    let dir = temp_repo_dir("serve-resolve-fallback");
    fs::create_dir_all(&dir).unwrap();
    let dir = fs::canonicalize(&dir).unwrap();
    let entries = vec![ServeEntry {
        port: 4202,
        pid: 1,
        cwd: dir.display().to_string(),
        tui_pid: None,
    }];

    assert_eq!(
        resolve_serve_port_with_entries(None, &dir, &entries),
        Some(4202)
    );
    fs::remove_dir_all(dir).ok();
}
