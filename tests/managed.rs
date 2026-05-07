use std::{collections::HashSet, path::PathBuf};

use opencode_multiplexer::{
    app::sessions::{SessionList, SessionOrigin, SessionStatus},
    data::poller::{DiscoveredSessionInfo, DiscoverySource, PollSnapshot},
    ops::opencode::{build_managed_session_command, build_replica_command, display_title_for_cwd},
    terminal::{manager::PtyManager, pty::PtySession},
    ui::sidebar::flatten_sidebar_entries,
};
use portable_pty::CommandBuilder;

#[test]
fn flatten_sidebar_entries_hides_and_shows_children_based_on_expansion() {
    let mut manager = PtyManager::default();
    manager.apply_poll_snapshot(PollSnapshot {
        sessions: vec![DiscoveredSessionInfo {
            session_id: "parent".into(),
            cwd: PathBuf::from("/tmp/parent"),
            title: "parent".into(),
            status: SessionStatus::Idle,
            process_pid: Some(1),
            model: None,
            preview: None,
            time_updated: None,
            has_children: true,
            children: vec![opencode_multiplexer::data::poller::ChildSessionInfo {
                session_id: "child".into(),
                cwd: PathBuf::from("/tmp/parent"),
                title: "child".into(),
                status: SessionStatus::NeedsInput,
                time_updated: None,
                has_children: false,
                children: vec![],
            }],
            serve_port: None,
            source: DiscoverySource::Serve,
        }],
    });

    let entries = manager.sidebar_entries();
    let collapsed = flatten_sidebar_entries(&entries, &HashSet::new());
    assert_eq!(collapsed.len(), 1);

    let expanded = flatten_sidebar_entries(&entries, &HashSet::from([String::from("parent")]));
    assert_eq!(expanded.len(), 2);
    assert_eq!(expanded[1].depth, 1);
    assert_eq!(expanded[1].status, SessionStatus::NeedsInput);
}

#[test]
fn first_session_becomes_active_and_selected() {
    let mut sessions = SessionList::default();

    let first = sessions.push(
        PathBuf::from("/tmp/project-a"),
        "project-a".into(),
        SessionStatus::Idle,
        None,
        SessionOrigin::Managed,
        None,
        None,
        None,
        None,
        None,
        None,
        false,
        vec![],
    );
    let second = sessions.push(
        PathBuf::from("/tmp/project-b"),
        "project-b".into(),
        SessionStatus::Idle,
        None,
        SessionOrigin::Managed,
        None,
        None,
        None,
        None,
        None,
        None,
        false,
        vec![],
    );

    assert_eq!(sessions.active_id(), Some(first));
    assert_eq!(sessions.selected_id(), Some(first));
    assert_ne!(Some(second), sessions.active_id());
}

#[test]
fn selecting_next_and_activating_switches_active_session() {
    let mut sessions = SessionList::default();

    sessions.push(
        PathBuf::from("/tmp/project-a"),
        "project-a".into(),
        SessionStatus::Idle,
        None,
        SessionOrigin::Managed,
        None,
        None,
        None,
        None,
        None,
        None,
        false,
        vec![],
    );
    let second = sessions.push(
        PathBuf::from("/tmp/project-b"),
        "project-b".into(),
        SessionStatus::Working,
        None,
        SessionOrigin::Managed,
        None,
        None,
        None,
        None,
        None,
        None,
        false,
        vec![],
    );

    sessions.select_next();
    sessions.activate_selected();

    assert_eq!(sessions.active_id(), Some(second));
    assert_eq!(sessions.selected_id(), Some(second));
}

#[test]
fn confirming_kill_removes_selected_and_promotes_neighbor() {
    let mut sessions = SessionList::default();

    let first = sessions.push(
        PathBuf::from("/tmp/project-a"),
        "project-a".into(),
        SessionStatus::Idle,
        None,
        SessionOrigin::Managed,
        None,
        None,
        None,
        None,
        None,
        None,
        false,
        vec![],
    );
    let second = sessions.push(
        PathBuf::from("/tmp/project-b"),
        "project-b".into(),
        SessionStatus::Working,
        None,
        SessionOrigin::Managed,
        None,
        None,
        None,
        None,
        None,
        None,
        false,
        vec![],
    );

    sessions.select_next();
    sessions.request_kill_selected();
    let killed = sessions.confirm_kill();

    assert_eq!(killed, Some(second));
    assert_eq!(sessions.active_id(), Some(first));
    assert_eq!(sessions.selected_id(), Some(first));
    assert_eq!(sessions.len(), 1);
}

#[test]
fn managed_session_command_uses_opencode_binary_in_target_directory() {
    let cwd = PathBuf::from("/tmp/example-repo");
    let command = build_managed_session_command(&cwd);

    assert_eq!(command.get_argv()[0].to_string_lossy(), "opencode");
    assert_eq!(
        command.get_cwd().map(|p| p.to_string_lossy().to_string()),
        Some(cwd.display().to_string())
    );
}

#[test]
fn replica_command_uses_session_flag() {
    let cwd = PathBuf::from("/tmp/example-repo");
    let command = build_replica_command(&cwd, "sess_123");

    assert_eq!(command.get_argv()[0].to_string_lossy(), "opencode");
    assert_eq!(command.get_argv()[1].to_string_lossy(), "-s");
    assert_eq!(command.get_argv()[2].to_string_lossy(), "sess_123");
}

#[test]
fn manager_can_attach_arbitrary_session() {
    let mut manager = PtyManager::default();
    let result = manager.attach_arbitrary_session(
        "sess_xyz".into(),
        PathBuf::from("/tmp/xyz"),
        "Arbitrary".into(),
        SessionStatus::Idle,
        Some(1234567890),
        24,
        80,
    );

    if let Err(e) = &result {
        let err_str = e.to_string();
        if err_str.contains("No such file or directory")
            || err_str.contains("not found")
            || err_str.contains("No viable candidates found in PATH")
            || err_str.contains("The system cannot find the file specified")
        {
            return;
        }
    }
    result.unwrap();

    let _active = manager.active_session().unwrap();
    let summary = manager.selected_summary().unwrap();

    assert_eq!(summary.session_id.as_deref(), Some("sess_xyz"));
    assert_eq!(summary.title, "Arbitrary");
    assert_eq!(manager.len(), 1);
}

#[test]
fn cwd_title_uses_directory_name() {
    assert_eq!(
        display_title_for_cwd(PathBuf::from("/tmp/example-repo").as_path()),
        "example-repo"
    );
}

#[test]
fn pty_manager_kill_selected_updates_active_session() {
    let mut manager = PtyManager::default();
    let first = manager.register_placeholder(
        PathBuf::from("/tmp/project-a"),
        "project-a".into(),
        SessionStatus::Idle,
        None,
        SessionOrigin::Managed,
        None,
        None,
        None,
        None,
        None,
        None,
        false,
        vec![],
    );
    let second = manager.register_placeholder(
        PathBuf::from("/tmp/project-b"),
        "project-b".into(),
        SessionStatus::Working,
        None,
        SessionOrigin::Managed,
        None,
        None,
        None,
        None,
        None,
        None,
        false,
        vec![],
    );

    manager.select_next();
    manager.activate_selected();
    let killed = manager.kill_selected_placeholder();

    assert_eq!(killed, Some(second));
    assert_eq!(manager.active_id(), Some(first));
    assert_eq!(manager.selected_id(), Some(first));
}

#[test]
fn applying_poll_snapshot_adds_and_updates_discovered_sessions() {
    let mut manager = PtyManager::default();

    manager.apply_poll_snapshot(PollSnapshot {
        sessions: vec![DiscoveredSessionInfo {
            session_id: "sess_discovered".into(),
            cwd: PathBuf::from("/tmp/discovered"),
            title: "discovered".into(),
            status: SessionStatus::NeedsInput,
            process_pid: Some(42),
            model: Some("gpt-5".into()),
            preview: Some("need answer".into()),
            time_updated: None,
            has_children: true,
            children: vec![],
            serve_port: None,
            source: DiscoverySource::Serve,
        }],
    });

    let summary = manager.selected_summary().unwrap();
    assert_eq!(summary.session_id.as_deref(), Some("sess_discovered"));
    assert_eq!(summary.origin, SessionOrigin::Discovered);
    assert_eq!(summary.status, SessionStatus::NeedsInput);
    assert_eq!(summary.model.as_deref(), Some("gpt-5"));
    assert!(summary.has_children);
}

#[test]
fn applying_poll_snapshot_removes_stale_discovered_placeholders() {
    let mut manager = PtyManager::default();
    manager.apply_poll_snapshot(PollSnapshot {
        sessions: vec![DiscoveredSessionInfo {
            session_id: "sess_old".into(),
            cwd: PathBuf::from("/tmp/old"),
            title: "old".into(),
            status: SessionStatus::Idle,
            process_pid: Some(11),
            model: None,
            preview: None,
            time_updated: None,
            has_children: false,
            children: vec![],
            serve_port: None,
            source: DiscoverySource::TuiExplicit,
        }],
    });

    manager.apply_poll_snapshot(PollSnapshot { sessions: vec![] });

    assert!(manager.is_empty());
}

#[test]
fn sidebar_entries_include_child_sessions() {
    let mut manager = PtyManager::default();
    manager.apply_poll_snapshot(PollSnapshot {
        sessions: vec![DiscoveredSessionInfo {
            session_id: "parent".into(),
            cwd: PathBuf::from("/tmp/parent"),
            title: "parent".into(),
            status: SessionStatus::Working,
            process_pid: Some(7),
            model: None,
            preview: None,
            time_updated: None,
            has_children: true,
            children: vec![opencode_multiplexer::data::poller::ChildSessionInfo {
                session_id: "child".into(),
                cwd: PathBuf::from("/tmp/parent"),
                title: "child".into(),
                status: SessionStatus::NeedsInput,
                time_updated: None,
                has_children: false,
                children: vec![],
            }],
            serve_port: None,
            source: DiscoverySource::Serve,
        }],
    });

    let entries = manager.sidebar_entries();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].has_children);
    assert_eq!(entries[0].children.len(), 1);
    assert_eq!(entries[0].children[0].status, SessionStatus::NeedsInput);
}

#[test]
fn sidebar_entries_sort_top_level_sessions_by_recent_update_first() {
    let mut manager = PtyManager::default();

    let older = manager.register_placeholder(
        PathBuf::from("/tmp/project-older"),
        "older".into(),
        SessionStatus::Idle,
        Some("sess_old".into()),
        SessionOrigin::Discovered,
        None,
        None,
        None,
        None,
        None,
        Some(100),
        false,
        vec![],
    );
    let newer = manager.register_placeholder(
        PathBuf::from("/tmp/project-newer"),
        "newer".into(),
        SessionStatus::Idle,
        Some("sess_new".into()),
        SessionOrigin::Discovered,
        None,
        None,
        None,
        None,
        None,
        Some(200),
        false,
        vec![],
    );

    let entries = manager.sidebar_entries();

    assert_eq!(entries[0].top_level_id, newer);
    assert_eq!(entries[1].top_level_id, older);
}

#[test]
fn reap_exited_ptys_clears_dead_slot_keeps_entry() {
    let mut manager = PtyManager::default();

    // Register a placeholder session
    let id = manager.register_placeholder(
        PathBuf::from("/tmp/test"),
        "test".into(),
        SessionStatus::Working,
        None,
        SessionOrigin::Managed,
        None,
        None,
        None,
        None,
        None,
        None,
        false,
        vec![],
    );

    // Spawn a short-lived process that exits immediately
    #[cfg(unix)]
    let (shell, arg) = (
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into()),
        "-c",
    );
    #[cfg(windows)]
    let (shell, arg) = ("cmd.exe".to_string(), "/c");

    let mut cmd = CommandBuilder::new(shell);
    cmd.args([arg, "exit 0"]);
    let pty = PtySession::spawn_test_command(cmd, 24, 80).expect("spawn test command");
    manager.insert_pty_for_session(id, pty);

    // Activate so active_session() returns something initially
    manager.select_top_level(id);
    manager.activate_selected();
    assert!(
        manager.active_session().is_some(),
        "PTY should be active before exit"
    );

    // Wait for the child to exit (up to 2 seconds)
    let mut exited = vec![];
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        exited = manager.reap_exited_ptys();
        if exited.contains(&id) {
            break;
        }
    }
    assert!(exited.contains(&id), "should report exited session id");

    // PTY slot cleared but sidebar entry preserved
    assert!(
        manager.active_session().is_none(),
        "PTY should be cleared after reap"
    );
    assert_eq!(manager.len(), 1, "sidebar entry should be preserved");

    // Second call returns empty — nothing left to reap
    let exited_again = manager.reap_exited_ptys();
    assert!(exited_again.is_empty(), "second reap should find nothing");
}

#[test]
fn find_by_process_pid_matches_serve_pid() {
    let mut sessions = SessionList::default();

    let id = sessions.push(
        PathBuf::from("/tmp/project"),
        "project".into(),
        SessionStatus::Idle,
        Some("sess_1".into()),
        SessionOrigin::Managed,
        Some(200),
        Some(100),
        None,
        None,
        None,
        None,
        false,
        vec![],
    );

    assert_eq!(sessions.find_by_process_pid(200), Some(id));
    assert_eq!(sessions.find_by_process_pid(100), Some(id));
    assert_eq!(sessions.find_by_process_pid(999), None);
}

#[test]
fn apply_poll_snapshot_updates_via_serve_pid() {
    let mut manager = PtyManager::default();

    let _id = manager.register_placeholder(
        PathBuf::from("/tmp/project"),
        "project".into(),
        SessionStatus::Idle,
        None,
        SessionOrigin::Managed,
        Some(200),
        Some(std::process::id()), // live serve PID
        None,
        None,
        None,
        None,
        false,
        vec![],
    );

    manager.apply_poll_snapshot(PollSnapshot {
        sessions: vec![DiscoveredSessionInfo {
            session_id: "sess_correct".into(),
            cwd: PathBuf::from("/tmp/project"),
            title: "Correct Title".into(),
            status: SessionStatus::Working,
            process_pid: Some(std::process::id()), // serve PID (live)
            model: None,
            preview: None,
            time_updated: None,
            has_children: false,
            children: vec![],
            serve_port: None,
            source: DiscoverySource::Serve,
        }],
    });

    let summary = manager.selected_summary().unwrap();
    assert_eq!(summary.session_id.as_deref(), Some("sess_correct"));
    assert_eq!(summary.title, "Correct Title");
    assert_eq!(summary.status, SessionStatus::Working);
    assert_eq!(
        summary.process_pid,
        Some(200),
        "process_pid should remain the TUI PID"
    );
    assert_eq!(summary.serve_pid, Some(std::process::id()));
}

#[test]
fn manager_can_attach_arbitrary_session_reuses_existing() {
    let mut manager = PtyManager::default();

    // First, Poller discovers an active session in the background and registers it
    manager.apply_poll_snapshot(PollSnapshot {
        sessions: vec![DiscoveredSessionInfo {
            session_id: "sess_existing".into(),
            cwd: PathBuf::from("/tmp/existing"),
            title: "Existing".into(),
            status: SessionStatus::Idle,
            process_pid: None,
            model: None,
            preview: None,
            time_updated: Some(100),
            has_children: false,
            children: vec![],
            serve_port: Some(4000),
            source: DiscoverySource::Serve,
        }],
    });

    assert_eq!(manager.len(), 1);
    let summary_before = manager.sessions().items()[0].clone();
    assert_eq!(summary_before.origin, SessionOrigin::Discovered);

    // User tries to attach to it via the Session Picker UI
    let result = manager.attach_arbitrary_session(
        "sess_existing".into(),
        PathBuf::from("/tmp/existing"),
        "Existing Attached".into(),
        SessionStatus::Working,
        Some(100),
        24,
        80,
    );

    if let Err(e) = &result {
        let err_str = e.to_string();
        if err_str.contains("No such file or directory")
            || err_str.contains("not found")
            || err_str.contains("No viable candidates found in PATH")
        {
            // Skip test if opencode is not installed
            return;
        }
    }
    result.unwrap();

    // The manager should REUSE the existing discovered entry, not create a second one.
    assert_eq!(
        manager.len(),
        1,
        "Should reuse existing session instead of duplicating"
    );

    let summary_after = manager.selected_summary().unwrap();
    assert_eq!(summary_after.session_id.as_deref(), Some("sess_existing"));
    assert_eq!(
        summary_after.title, "Existing Attached",
        "Should update title"
    );
    assert_eq!(
        summary_after.status,
        SessionStatus::Working,
        "Should update status"
    );
    assert_eq!(
        summary_after.origin,
        SessionOrigin::Managed,
        "Should upgrade origin to Managed"
    );
    assert!(manager.active_session().is_some(), "Should attach PTY");
}

#[test]
fn manager_apply_poll_snapshot_matches_unresolved_managed_session() {
    let mut manager = PtyManager::default();

    // Spawn managed session but serve daemon hasn't replied with an ID yet
    let id = manager.register_placeholder(
        PathBuf::from("/tmp/unresolved"),
        "Unresolved".into(),
        SessionStatus::Working,
        None, // missing session_id
        SessionOrigin::Managed,
        Some(123),
        Some(122),  // serve_pid
        Some(4001), // serve_port
        None,
        None,
        Some(100),
        false,
        vec![],
    );

    // TUI loop scans process table first and guesses the session_id
    manager.apply_poll_snapshot(PollSnapshot {
        sessions: vec![DiscoveredSessionInfo {
            session_id: "sess_guessed".into(),
            cwd: PathBuf::from("/tmp/unresolved"),
            title: "Guessed Title".into(),
            status: SessionStatus::Idle,
            process_pid: None, // Poller might not associate process pid correctly here
            model: None,
            preview: None,
            time_updated: Some(101),
            has_children: false,
            children: vec![],
            serve_port: None,
            source: DiscoverySource::TuiHeuristic,
        }],
    });

    // Should NOT duplicate, should update the unresolved Managed session
    assert_eq!(manager.len(), 1, "Should not duplicate");
    let summary = manager.sessions().items()[0].clone();
    assert_eq!(summary.id, id);
    assert_eq!(
        summary.session_id.as_deref(),
        Some("sess_guessed"),
        "Should adopt the guessed ID"
    );
    assert_eq!(summary.title, "Guessed Title");
    assert_eq!(summary.status, SessionStatus::Idle);
    assert_eq!(
        summary.origin,
        SessionOrigin::Managed,
        "Should stay managed"
    );
}

#[test]
fn serve_discovery_does_not_overwrite_managed_serve_port() {
    let mut manager = PtyManager::default();

    manager.register_placeholder(
        PathBuf::from("/tmp/project-a"),
        "project-a".into(),
        SessionStatus::Idle,
        Some("sess_managed".into()),
        SessionOrigin::Managed,
        Some(500),
        Some(std::process::id()), // live serve PID so it doesn't get marked Error
        Some(4223),
        None,
        None,
        None,
        false,
        vec![],
    );

    manager.apply_poll_snapshot(PollSnapshot {
        sessions: vec![DiscoveredSessionInfo {
            session_id: "sess_managed".into(),
            cwd: PathBuf::from("/tmp/project-a"),
            title: "Updated Title".into(),
            status: SessionStatus::Working,
            process_pid: Some(500),
            model: None,
            preview: None,
            time_updated: Some(101),
            has_children: false,
            children: vec![],
            serve_port: Some(4220),
            source: DiscoverySource::Serve,
        }],
    });

    let summary = manager.selected_summary().unwrap();
    assert_eq!(summary.serve_port, Some(4223));
    assert_eq!(summary.title, "Updated Title");
    assert_eq!(summary.status, SessionStatus::Working);
}

#[test]
fn session_id_match_takes_priority_over_serve_port() {
    let mut manager = PtyManager::default();

    let entry_a = manager.register_placeholder(
        PathBuf::from("/tmp/a"),
        "a".into(),
        SessionStatus::Idle,
        Some("sess_a".into()),
        SessionOrigin::Managed,
        Some(200),
        Some(std::process::id()), // live serve PID so it doesn't get cleaned up
        Some(4220),
        None,
        None,
        None,
        false,
        vec![],
    );
    let entry_b = manager.register_placeholder(
        PathBuf::from("/tmp/b"),
        "b".into(),
        SessionStatus::Idle,
        Some("sess_b".into()),
        SessionOrigin::Managed,
        Some(500),
        Some(std::process::id()), // live serve PID
        Some(4223),
        None,
        None,
        None,
        false,
        vec![],
    );

    manager.apply_poll_snapshot(PollSnapshot {
        sessions: vec![DiscoveredSessionInfo {
            session_id: "sess_b".into(),
            cwd: PathBuf::from("/tmp/b"),
            title: "Entry B Updated".into(),
            status: SessionStatus::Working,
            process_pid: Some(500),
            model: None,
            preview: None,
            time_updated: Some(201),
            has_children: false,
            children: vec![],
            serve_port: Some(4220),
            source: DiscoverySource::Serve,
        }],
    });

    let items = manager.sessions().items();
    let summary_a = items.iter().find(|s| s.id == entry_a).unwrap();
    let summary_b = items.iter().find(|s| s.id == entry_b).unwrap();

    assert_eq!(summary_b.title, "Entry B Updated");
    assert_eq!(summary_b.status, SessionStatus::Working);
    assert_eq!(summary_b.serve_port, Some(4223));

    assert_eq!(summary_a.title, "a");
    assert_eq!(summary_a.status, SessionStatus::Idle);
    assert_eq!(summary_a.serve_port, Some(4220));
}

#[test]
fn idle_unmanaged_serve_session_appears_in_sidebar() {
    let mut manager = PtyManager::default();
    manager.apply_poll_snapshot(PollSnapshot {
        sessions: vec![DiscoveredSessionInfo {
            session_id: "sess_idle_external".into(),
            cwd: PathBuf::from("/tmp/external-project"),
            title: "External Idle Session".into(),
            status: SessionStatus::Idle,
            process_pid: Some(99999),
            model: Some("gpt-5".into()),
            preview: Some("waiting...".into()),
            time_updated: None,
            has_children: false,
            children: vec![],
            serve_port: Some(4242),
            source: DiscoverySource::Serve,
        }],
    });

    assert!(
        !manager.is_empty(),
        "Idle serve session should appear in manager"
    );
    let entries = manager.sidebar_entries();
    assert_eq!(
        entries.len(),
        1,
        "Idle serve session should appear in sidebar"
    );
    assert_eq!(entries[0].title, "External Idle Session");
}

#[test]
fn error_unmanaged_serve_session_appears_in_sidebar() {
    let mut manager = PtyManager::default();
    manager.apply_poll_snapshot(PollSnapshot {
        sessions: vec![DiscoveredSessionInfo {
            session_id: "sess_error_external".into(),
            cwd: PathBuf::from("/tmp/error-project"),
            title: "External Error Session".into(),
            status: SessionStatus::Error,
            process_pid: Some(88888),
            model: Some("gpt-5".into()),
            preview: Some("error occurred".into()),
            time_updated: None,
            has_children: false,
            children: vec![],
            serve_port: Some(4343),
            source: DiscoverySource::Serve,
        }],
    });

    assert!(
        !manager.is_empty(),
        "Error serve session should appear in manager"
    );
    let entries = manager.sidebar_entries();
    assert_eq!(
        entries.len(),
        1,
        "Error serve session should appear in sidebar"
    );
    assert_eq!(entries[0].status, SessionStatus::Error);
}

#[test]
fn idle_serve_session_survives_cache_merge() {
    use opencode_multiplexer::data::db::reader::DbReader;
    use opencode_multiplexer::data::poller::merge_cached_serve_sessions_with_reader;
    use rusqlite::Connection;

    // Set up a temp DB with the cached session present
    let db_path = std::env::temp_dir().join(format!(
        "ocmux-test-merge-cache-{}.db",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let conn = Connection::open(&db_path).unwrap();
    conn.execute_batch(
        r#"
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
            time_archived INTEGER
        );
        CREATE TABLE message (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            data TEXT NOT NULL,
            time_created INTEGER
        );
        "#,
    )
    .unwrap();
    conn.execute(
        "INSERT INTO project VALUES ('proj1', '/tmp/proj', 'proj', 100, 200)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO session VALUES ('sess_idle_serve', 'proj1', NULL, 'Idle Serve', '/tmp/idle-serve', NULL, 100, 200, NULL)",
        [],
    )
    .unwrap();

    let reader = DbReader::open(&db_path).unwrap();

    let fast = PollSnapshot {
        sessions: vec![DiscoveredSessionInfo {
            session_id: "sess_tui".into(),
            cwd: PathBuf::from("/tmp/tui"),
            title: "TUI".into(),
            status: SessionStatus::Working,
            process_pid: Some(100),
            model: None,
            preview: None,
            time_updated: None,
            has_children: false,
            children: vec![],
            serve_port: None,
            source: DiscoverySource::TuiExplicit,
        }],
    };

    let cached = vec![DiscoveredSessionInfo {
        session_id: "sess_idle_serve".into(),
        cwd: PathBuf::from("/tmp/idle-serve"),
        title: "Idle Serve".into(),
        status: SessionStatus::Idle,
        process_pid: Some(200),
        model: Some("claude".into()),
        preview: None,
        time_updated: None,
        has_children: false,
        children: vec![],
        serve_port: Some(4200),
        source: DiscoverySource::Serve,
    }];

    let merged = merge_cached_serve_sessions_with_reader(fast, &cached, &reader).unwrap();
    let found = merged
        .sessions
        .iter()
        .find(|s| s.session_id == "sess_idle_serve");
    assert!(
        found.is_some(),
        "Idle serve session should survive cache merge"
    );
    assert_eq!(found.unwrap().status, SessionStatus::Idle);
}

// ============================================================================
// Bug 1 Tests: Dead managed sessions showing Working (green) should show Error
// ============================================================================

#[test]
fn managed_working_with_dead_serve_becomes_error() {
    let mut manager = PtyManager::default();
    manager.register_placeholder(
        PathBuf::from("/tmp/delorean"),
        "delorean".into(),
        SessionStatus::Working,
        Some("sess_dead".into()),
        SessionOrigin::Managed,
        None,
        Some(99999), // definitely dead PID
        Some(4200),
        None,
        None,
        None,
        false,
        vec![],
    );

    // Apply snapshot that still contains the session (e.g., from managed sessions list)
    let snapshot = PollSnapshot {
        sessions: vec![DiscoveredSessionInfo {
            session_id: "sess_dead".into(),
            cwd: PathBuf::from("/tmp/delorean"),
            title: "delorean".into(),
            status: SessionStatus::Working,
            process_pid: None,
            model: None,
            preview: None,
            time_updated: None,
            has_children: false,
            children: vec![],
            serve_port: None,
            source: DiscoverySource::TuiExplicit,
        }],
    };

    manager.apply_poll_snapshot(snapshot);

    let entries = manager.sidebar_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].status,
        SessionStatus::Error,
        "Dead Working session should flip to Error"
    );
}

#[test]
fn managed_idle_with_dead_serve_stays_idle() {
    let mut manager = PtyManager::default();
    manager.register_placeholder(
        PathBuf::from("/tmp/delorean"),
        "delorean".into(),
        SessionStatus::Idle,
        Some("sess_idle".into()),
        SessionOrigin::Managed,
        None,
        Some(99999), // dead PID
        Some(4200),
        None,
        None,
        None,
        false,
        vec![],
    );

    let snapshot = PollSnapshot {
        sessions: vec![DiscoveredSessionInfo {
            session_id: "sess_idle".into(),
            cwd: PathBuf::from("/tmp/delorean"),
            title: "delorean".into(),
            status: SessionStatus::Idle,
            process_pid: None,
            model: None,
            preview: None,
            time_updated: None,
            has_children: false,
            children: vec![],
            serve_port: None,
            source: DiscoverySource::TuiExplicit,
        }],
    };

    manager.apply_poll_snapshot(snapshot);

    let entries = manager.sidebar_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].status,
        SessionStatus::Idle,
        "Idle session should stay Idle"
    );
}

#[test]
fn managed_working_with_live_serve_stays_working() {
    let mut manager = PtyManager::default();
    manager.register_placeholder(
        PathBuf::from("/tmp/delorean"),
        "delorean".into(),
        SessionStatus::Working,
        Some("sess_live".into()),
        SessionOrigin::Managed,
        None,
        Some(std::process::id()), // live PID
        Some(4200),
        None,
        None,
        None,
        false,
        vec![],
    );

    let snapshot = PollSnapshot {
        sessions: vec![DiscoveredSessionInfo {
            session_id: "sess_live".into(),
            cwd: PathBuf::from("/tmp/delorean"),
            title: "delorean".into(),
            status: SessionStatus::Working,
            process_pid: None,
            model: None,
            preview: None,
            time_updated: None,
            has_children: false,
            children: vec![],
            serve_port: None,
            source: DiscoverySource::TuiExplicit,
        }],
    };

    manager.apply_poll_snapshot(snapshot);

    let entries = manager.sidebar_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].status,
        SessionStatus::Working,
        "Live Working session should stay Working"
    );
}

#[test]
fn managed_without_serve_pid_is_not_marked_error() {
    let mut manager = PtyManager::default();
    manager.register_placeholder(
        PathBuf::from("/tmp/delorean"),
        "delorean".into(),
        SessionStatus::Working,
        Some("sess_no_serve".into()),
        SessionOrigin::Managed,
        None,
        None, // no serve_pid
        None,
        None,
        None,
        None,
        false,
        vec![],
    );

    let snapshot = PollSnapshot {
        sessions: vec![DiscoveredSessionInfo {
            session_id: "sess_no_serve".into(),
            cwd: PathBuf::from("/tmp/delorean"),
            title: "delorean".into(),
            status: SessionStatus::Working,
            process_pid: None,
            model: None,
            preview: None,
            time_updated: None,
            has_children: false,
            children: vec![],
            serve_port: None,
            source: DiscoverySource::TuiExplicit,
        }],
    };

    manager.apply_poll_snapshot(snapshot);

    let entries = manager.sidebar_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].status,
        SessionStatus::Working,
        "Session without serve_pid should not be marked Error"
    );
}

// ============================================================================
// Bug 2 Tests: Stale managed session removal
// ============================================================================

#[test]
fn managed_dead_serve_not_in_snapshot_is_removed() {
    let mut manager = PtyManager::default();
    manager.register_placeholder(
        PathBuf::from("/tmp/delorean"),
        "old".into(),
        SessionStatus::Working,
        Some("sess_old".into()),
        SessionOrigin::Managed,
        None,
        Some(99999), // dead
        Some(4200),
        None,
        None,
        None,
        false,
        vec![],
    );

    // Empty snapshot — session is not rediscovered
    let snapshot = PollSnapshot { sessions: vec![] };
    let registry_dirty = manager.apply_poll_snapshot(snapshot);

    assert!(
        registry_dirty,
        "Removing managed session should signal registry dirty"
    );
    assert!(
        manager.is_empty(),
        "Dead managed session not in snapshot should be removed"
    );
}

#[test]
fn managed_dead_serve_replaced_in_same_cwd_is_removed() {
    let mut manager = PtyManager::default();
    manager.register_placeholder(
        PathBuf::from("/tmp/delorean"),
        "old".into(),
        SessionStatus::Working,
        Some("sess_old".into()),
        SessionOrigin::Managed,
        None,
        Some(99999), // dead
        Some(4200),
        None,
        None,
        None,
        false,
        vec![],
    );

    // New session in the same cwd
    let snapshot = PollSnapshot {
        sessions: vec![DiscoveredSessionInfo {
            session_id: "sess_new".into(),
            cwd: PathBuf::from("/tmp/delorean"),
            title: "new".into(),
            status: SessionStatus::Working,
            process_pid: None,
            model: None,
            preview: None,
            time_updated: None,
            has_children: false,
            children: vec![],
            serve_port: None,
            source: DiscoverySource::TuiExplicit,
        }],
    };
    let registry_dirty = manager.apply_poll_snapshot(snapshot);

    assert!(
        registry_dirty,
        "Replacing managed session should signal registry dirty"
    );
    let entries = manager.sidebar_entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].session_id,
        Some("sess_new".into()),
        "New session should remain"
    );
    assert!(
        !manager
            .sessions()
            .items()
            .iter()
            .any(|s| s.session_id.as_deref() == Some("sess_old")),
        "Old dead session should be removed"
    );
}

#[test]
fn managed_dead_serve_with_live_pty_is_kept() {
    let mut manager = PtyManager::default();
    let id = manager.register_placeholder(
        PathBuf::from("/tmp/delorean"),
        "delorean".into(),
        SessionStatus::Working,
        Some("sess_pty".into()),
        SessionOrigin::Managed,
        None,
        Some(99999), // dead serve
        Some(4200),
        None,
        None,
        None,
        false,
        vec![],
    );

    // Spawn a live PTY for this session
    let mut cmd = CommandBuilder::new("sleep");
    cmd.args(["60"]);
    let pty = PtySession::spawn_test_command(cmd, 24, 80).unwrap();
    manager.insert_pty_for_session(id, pty);

    let snapshot = PollSnapshot { sessions: vec![] };
    let registry_dirty = manager.apply_poll_snapshot(snapshot);

    assert!(
        !registry_dirty,
        "Session with live PTY should not trigger registry dirty"
    );
    assert_eq!(
        manager.len(),
        1,
        "Managed session with live PTY should be kept"
    );
}

#[test]
fn discovered_stale_cleanup_still_works() {
    let mut manager = PtyManager::default();
    manager.register_placeholder(
        PathBuf::from("/tmp/delorean"),
        "discovered".into(),
        SessionStatus::Idle,
        Some("sess_discovered".into()),
        SessionOrigin::Discovered,
        None,
        None,
        None,
        None,
        None,
        None,
        false,
        vec![],
    );

    // Empty snapshot — discovered session is stale
    let snapshot = PollSnapshot { sessions: vec![] };
    let registry_dirty = manager.apply_poll_snapshot(snapshot);

    assert!(
        !registry_dirty,
        "Discovered cleanup should not mark registry dirty"
    );
    assert!(
        manager.is_empty(),
        "Stale discovered session should be removed"
    );
}

#[test]
fn managed_live_serve_is_never_removed() {
    let mut manager = PtyManager::default();
    manager.register_placeholder(
        PathBuf::from("/tmp/delorean"),
        "live".into(),
        SessionStatus::Working,
        Some("sess_live".into()),
        SessionOrigin::Managed,
        None,
        Some(std::process::id()), // live serve
        Some(4200),
        None,
        None,
        None,
        false,
        vec![],
    );

    let snapshot = PollSnapshot { sessions: vec![] };
    let registry_dirty = manager.apply_poll_snapshot(snapshot);

    assert!(
        !registry_dirty,
        "Live managed session should not trigger registry dirty"
    );
    assert_eq!(
        manager.len(),
        1,
        "Managed session with live serve should be kept"
    );
}

fn heuristic_discovery(
    session_id: &str,
    process_pid: Option<u32>,
    title: &str,
    status: SessionStatus,
    time_updated: i64,
) -> DiscoveredSessionInfo {
    DiscoveredSessionInfo {
        session_id: session_id.into(),
        cwd: PathBuf::from("/tmp/proj"),
        title: title.into(),
        status,
        process_pid,
        model: Some("new-model".into()),
        preview: Some("new-preview".into()),
        time_updated: Some(time_updated),
        has_children: false,
        children: vec![],
        serve_port: None,
        source: DiscoverySource::TuiHeuristic,
    }
}

#[test]
fn heuristic_with_session_id_match_updates_metadata() {
    let mut manager = PtyManager::default();
    let id = manager.register_placeholder(
        PathBuf::from("/tmp/proj"),
        "Old title".into(),
        SessionStatus::Working,
        Some("sess-1".into()),
        SessionOrigin::Managed,
        Some(111),
        None,
        Some(4200),
        Some("old-model".into()),
        Some("old-preview".into()),
        Some(10),
        false,
        vec![],
    );

    manager.apply_poll_snapshot(PollSnapshot {
        sessions: vec![heuristic_discovery(
            "sess-1",
            Some(222),
            "New title",
            SessionStatus::Idle,
            20,
        )],
    });

    let summary = manager
        .sessions()
        .items()
        .iter()
        .find(|s| s.id == id)
        .unwrap();
    assert_eq!(summary.session_id.as_deref(), Some("sess-1"));
    assert_eq!(summary.title, "New title");
    assert_eq!(summary.status, SessionStatus::Idle);
    assert_eq!(summary.model.as_deref(), Some("new-model"));
    assert_eq!(summary.preview.as_deref(), Some("new-preview"));
    assert_eq!(summary.time_updated, Some(20));
}

#[test]
fn heuristic_with_pid_match_blocks_update() {
    let mut manager = PtyManager::default();
    let id = manager.register_placeholder(
        PathBuf::from("/tmp/proj"),
        "Correct title".into(),
        SessionStatus::Working,
        Some("sess-1".into()),
        SessionOrigin::Managed,
        Some(111),
        None,
        Some(4200),
        Some("old-model".into()),
        Some("old-preview".into()),
        Some(10),
        false,
        vec![],
    );

    manager.apply_poll_snapshot(PollSnapshot {
        sessions: vec![heuristic_discovery(
            "sess-wrong",
            Some(111),
            "Wrong title",
            SessionStatus::Idle,
            20,
        )],
    });

    let summary = manager
        .sessions()
        .items()
        .iter()
        .find(|s| s.id == id)
        .unwrap();
    assert_eq!(summary.session_id.as_deref(), Some("sess-1"));
    assert_eq!(summary.title, "Correct title");
    assert_eq!(summary.status, SessionStatus::Working);
    assert_eq!(summary.model.as_deref(), Some("old-model"));
    assert_eq!(summary.preview.as_deref(), Some("old-preview"));
    assert_eq!(summary.time_updated, Some(10));
}

#[test]
fn heuristic_adopts_session_id_when_none() {
    let mut manager = PtyManager::default();
    let id = manager.register_placeholder(
        PathBuf::from("/tmp/proj"),
        "Old title".into(),
        SessionStatus::Working,
        None,
        SessionOrigin::Managed,
        None,
        None,
        Some(4200),
        Some("old-model".into()),
        Some("old-preview".into()),
        Some(10),
        false,
        vec![],
    );

    manager.apply_poll_snapshot(PollSnapshot {
        sessions: vec![heuristic_discovery(
            "sess-adopted",
            None,
            "New title",
            SessionStatus::Idle,
            20,
        )],
    });

    let summary = manager
        .sessions()
        .items()
        .iter()
        .find(|s| s.id == id)
        .unwrap();
    assert_eq!(summary.session_id.as_deref(), Some("sess-adopted"));
    assert_eq!(summary.title, "New title");
    assert_eq!(summary.status, SessionStatus::Idle);
    assert_eq!(summary.model.as_deref(), Some("new-model"));
    assert_eq!(summary.preview.as_deref(), Some("new-preview"));
    assert_eq!(summary.time_updated, Some(20));
}
