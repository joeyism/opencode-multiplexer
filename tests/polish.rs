use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use opencode_multiplexer::{
    config::{AppConfig, load_config_from_path},
    ui::sidebar::{
        display_session_label, format_sidebar_text, relative_time_from_updated, relative_time_label,
    },
};

#[test]
fn default_config_has_expected_values() {
    let config = AppConfig::default();
    assert_eq!(config.sidebar_width, 30);
    assert_eq!(config.keybindings.up, 'k');
    assert_eq!(config.keybindings.down, 'j');
    assert_eq!(config.keybindings.sessions, 's');
}

#[test]
fn load_config_merges_partial_json() {
    let path = temp_json_path("partial");
    fs::write(
        &path,
        r#"{
      "sidebar_width": 40,
      "keybindings": { "spawn": "s" }
    }"#,
    )
    .unwrap();

    let config = load_config_from_path(&path).unwrap();
    assert_eq!(config.sidebar_width, 40);
    assert_eq!(config.keybindings.spawn, 's');
    assert_eq!(config.keybindings.help, '?');

    fs::remove_file(path).ok();
}

#[test]
fn expanded_sidebar_label_uses_folder_and_title() {
    let label = display_session_label(
        PathBuf::from("/tmp/delorean").as_path(),
        "ADO-2228 build flux",
    );
    assert!(label.starts_with("del/ADO-2228 build flux"));
}

#[test]
fn relative_time_label_formats_recent_values() {
    assert_eq!(relative_time_label(30), "1m");
    assert_eq!(relative_time_label(120), "2m");
    assert_eq!(relative_time_label(3600), "1h");
    assert_eq!(relative_time_label(172800), "2d");
}

#[test]
fn relative_time_from_updated_handles_milliseconds() {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    assert_eq!(relative_time_from_updated(Some(now_ms - 3_600_000)), "1h");
}

#[test]
fn expanded_sidebar_label_uses_repo_root_when_cwd_is_nested() {
    let root = std::env::temp_dir().join("ocmux-polish-repo-root");
    let nested = root.join("apps/service");
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::create_dir_all(&nested).unwrap();
    let label = display_session_label(nested.as_path(), "ADO-2228 build flux");
    assert!(label.starts_with("ocm/ADO-2228 build flux"));
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn expanded_sidebar_text_keeps_time_visible_when_width_is_small() {
    let root = std::env::temp_dir().join("ocmux-polish-row");
    let nested = root.join("apps/service");
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::create_dir_all(&nested).unwrap();
    let text = format_sidebar_text(
        nested.as_path(),
        "ADO-2228 build flux capacitor",
        "2m",
        28,
        0,
        false,
        false,
        false,
        false,
    );

    assert!(text.ends_with("2m"));
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn sidebar_text_pads_left_side_so_time_is_right_aligned() {
    let root = std::env::temp_dir().join("ocmux-polish-right-align");
    let nested = root.join("apps/service");
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::create_dir_all(&nested).unwrap();
    let text = format_sidebar_text(
        nested.as_path(),
        "ADO-2228 build flux capacitor",
        "70d",
        24,
        0,
        false,
        false,
        false,
        false,
    );
    let count = text.chars().count();
    assert!((20..=22).contains(&count), "got {count}");
    assert!(text.ends_with("70d"));
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn worktree_label_uses_common_repo_root_name() {
    let repo = std::env::temp_dir().join("delorean");
    let worktree = repo.join(".worktrees/ado-2228-core-123");
    std::fs::create_dir_all(repo.join(".git/worktrees/ado-2228-core-123")).unwrap();
    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::write(
        worktree.join(".git"),
        format!(
            "gitdir: {}\n",
            repo.join(".git/worktrees/ado-2228-core-123").display()
        ),
    )
    .unwrap();

    let label = display_session_label(worktree.as_path(), "ADO-2228 build flux");
    assert!(label.starts_with("del/ADO-2228 build flux"));

    std::fs::remove_dir_all(&repo).ok();
}

#[test]
fn child_rows_do_not_include_repo_prefix() {
    let text = format_sidebar_text(
        PathBuf::from("/tmp/delorean").as_path(),
        "Implement analyzer",
        "1h",
        32,
        1,
        false,
        false,
        false,
        true,
    );
    assert!(text.contains("Implement analyzer"));
    assert!(!text.contains("del/"));
}

fn temp_json_path(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("ocmux-rs-{label}-{nanos}.json"))
}

#[test]
fn config_can_override_sessions_keybinding() {
    let path = temp_json_path("sessions_key");
    fs::write(
        &path,
        r#"{
      "keybindings": { "sessions": "m" }
    }"#,
    )
    .unwrap();

    let config = load_config_from_path(&path).unwrap();
    assert_eq!(config.keybindings.sessions, 'm');

    fs::remove_file(path).ok();
}
