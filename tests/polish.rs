use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use opencode_multiplexer::{
    app::sessions::SessionStatus,
    config::{AppConfig, Keybindings, load_config_from_path},
    data::poller::{ServeSessionInfo, should_include_serve_session},
    ops::worktree::{WorktreePlan, build_worktree_plan, worktree_target_dir},
    registry::{load_managed_sessions_from_path, save_managed_sessions_to_path},
    ui::sidebar::{
        display_session_label, format_sidebar_text, relative_time_from_updated, relative_time_label,
    },
};

#[test]
fn config_loader_deep_merges_defaults() {
    let path = temp_json_path("config");
    fs::write(
        &path,
        r#"{
          "sidebar_width": 34,
          "keybindings": { "spawn": "s" }
        }"#,
    )
    .unwrap();

    let config = load_config_from_path(&path).unwrap();

    assert_eq!(config.sidebar_width, 34);
    assert_eq!(config.keybindings.spawn, 's');
    assert_eq!(config.keybindings.help, '?');

    fs::remove_file(path).ok();
}

#[test]
fn worktree_target_dir_uses_dot_worktrees_branch_layout() {
    let target = worktree_target_dir(PathBuf::from("/tmp/repo").as_path(), "feat-x");

    assert_eq!(target, PathBuf::from("/tmp/repo/.worktrees/feat-x"));
}

#[test]
fn worktree_plan_uses_existing_branch_when_present() {
    let plan = build_worktree_plan(PathBuf::from("/tmp/repo").as_path(), "feat-x", true, "main");

    assert_eq!(
        plan,
        WorktreePlan {
            target_dir: PathBuf::from("/tmp/repo/.worktrees/feat-x"),
            args: vec![
                "worktree".into(),
                "add".into(),
                "/tmp/repo/.worktrees/feat-x".into(),
                "feat-x".into(),
            ],
        }
    );
}

#[test]
fn worktree_plan_creates_new_branch_from_base_when_missing() {
    let plan = build_worktree_plan(
        PathBuf::from("/tmp/repo").as_path(),
        "feat-x",
        false,
        "main",
    );

    assert_eq!(
        plan.args,
        vec![
            "worktree",
            "add",
            "-b",
            "feat-x",
            "/tmp/repo/.worktrees/feat-x",
            "main",
        ]
    );
}

fn serve_visibility_depends_only_on_top_level() {
    for status in [
        SessionStatus::Working,
        SessionStatus::NeedsInput,
        SessionStatus::Idle,
        SessionStatus::Error,
    ] {
        let info = ServeSessionInfo {
            is_top_level: true,
            is_managed: false,
            status,
        };
        assert!(
            should_include_serve_session(&info),
            "top-level should be included regardless of status {:?}",
            status
        );
    }
    for status in [
        SessionStatus::Working,
        SessionStatus::NeedsInput,
        SessionStatus::Idle,
        SessionStatus::Error,
    ] {
        let info = ServeSessionInfo {
            is_top_level: true,
            is_managed: true,
            status,
        };
        assert!(
            should_include_serve_session(&info),
            "top-level managed should be included regardless of status {:?}",
            status
        );
    }
    for status in [
        SessionStatus::Working,
        SessionStatus::NeedsInput,
        SessionStatus::Idle,
        SessionStatus::Error,
    ] {
        let info = ServeSessionInfo {
            is_top_level: false,
            is_managed: false,
            status,
        };
        assert!(
            !should_include_serve_session(&info),
            "non-top-level should be excluded regardless of status {:?}",
            status
        );
    }
    for status in [
        SessionStatus::Working,
        SessionStatus::NeedsInput,
        SessionStatus::Idle,
        SessionStatus::Error,
    ] {
        let info = ServeSessionInfo {
            is_top_level: false,
            is_managed: true,
            status,
        };
        assert!(
            !should_include_serve_session(&info),
            "non-top-level should be excluded regardless of managed {:?}",
            status
        );
    }
}

#[test]
fn serve_filter_matches_ts_visibility_rule() {
    serve_visibility_depends_only_on_top_level();
}

#[test]
fn config_defaults_are_stable() {
    let config = AppConfig::default();
    let bindings = Keybindings::default();

    assert_eq!(config.keybindings.help, bindings.help);
    assert_eq!(config.keybindings.worktree, 't');
}

#[test]
fn managed_sessions_round_trip_json_file() {
    let path = temp_json_path("managed-sessions");
    save_managed_sessions_to_path(&path, ["sess_a", "sess_b"]).unwrap();

    let managed = load_managed_sessions_from_path(&path).unwrap();

    assert!(managed.contains("sess_a"));
    assert!(managed.contains("sess_b"));

    fs::remove_file(path).ok();
}

#[test]
fn expanded_sidebar_label_uses_folder_and_title() {
    let label = display_session_label(
        PathBuf::from("/tmp/delorean").as_path(),
        "ADO-2228 build flux",
        false,
    );
    assert!(label.starts_with("del/ADO-2228 build flux"));
}

#[test]
fn collapsed_sidebar_label_uses_repo_tag_and_longer_prefix() {
    let label = display_session_label(
        PathBuf::from("/tmp/delorean").as_path(),
        "ADO-2228 build flux",
        true,
    );
    assert_eq!(label, "de·ADO-2…");
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
    let label = display_session_label(nested.as_path(), "ADO-2228 build flux", false);
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
        false,
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
fn collapsed_sidebar_text_has_no_trailing_space_after_time() {
    let root = std::env::temp_dir().join("ocmux-polish-fixed-time");
    let nested = root.join("apps/service");
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::create_dir_all(&nested).unwrap();
    let text = format_sidebar_text(
        nested.as_path(),
        "build flux capacitor",
        true,
        "1m",
        15,
        0,
        false,
        false,
        false,
        false,
    );
    assert!(text.ends_with("1m"));
    assert_eq!(text.trim_end(), text);
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
        false,
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

    let label = display_session_label(worktree.as_path(), "ADO-2228 build flux", false);
    assert!(label.starts_with("del/ADO-2228 build flux"));

    std::fs::remove_dir_all(&repo).ok();
}

#[test]
fn child_rows_do_not_include_repo_prefix() {
    let text = format_sidebar_text(
        PathBuf::from("/tmp/delorean").as_path(),
        "Implement analyzer",
        false,
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
    assert!(!text.contains("del/"));
}

fn temp_json_path(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("ocmux-rs-{label}-{nanos}.json"))
}
