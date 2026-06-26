use std::path::PathBuf;

use opencode_multiplexer::data::db::models::DbProject;
use opencode_multiplexer::data::discovery::{
    find_best_project,
    ps::{ParsedProcess, ParsedServeProcess, parse_process_line, parse_serve_process_line},
};

#[test]
fn parses_bare_opencode_process() {
    let parsed = parse_process_line("12345 opencode").unwrap();

    assert_eq!(
        parsed,
        ParsedProcess {
            pid: 12345,
            session_id: None,
        }
    );
}

#[test]
fn parses_opencode_process_with_session_flag() {
    let parsed = parse_process_line("12345 opencode -s sess_abc123").unwrap();

    assert_eq!(parsed.pid, 12345);
    assert_eq!(parsed.session_id.as_deref(), Some("sess_abc123"));
}

#[test]
fn parses_node_wrapped_opencode_process() {
    let parsed =
        parse_process_line("12345 node /opt/homebrew/bin/opencode -s sess_wrapped").unwrap();

    assert_eq!(parsed.pid, 12345);
    assert_eq!(parsed.session_id.as_deref(), Some("sess_wrapped"));
}

#[test]
fn parses_standalone_dot_opencode_process() {
    let parsed =
        parse_process_line("35434 /Users/joey/.nvm/versions/node/v23.10.0/lib/node_modules/opencode-ai/bin/.opencode -s ses_123").unwrap();

    assert_eq!(parsed.pid, 35434);
    assert_eq!(parsed.session_id.as_deref(), Some("ses_123"));
}

#[test]
fn parses_bare_dot_opencode_process() {
    let parsed = parse_process_line("35434 .opencode").unwrap();

    assert_eq!(parsed.pid, 35434);
    assert_eq!(parsed.session_id, None);
}

#[test]
fn prefers_longest_matching_project_worktree() {
    let projects = vec![
        DbProject {
            id: "root".into(),
            worktree: PathBuf::from("/Users/joey/Programming"),
        },
        DbProject {
            id: "nested".into(),
            worktree: PathBuf::from("/Users/joey/Programming/client"),
        },
    ];

    let matched = find_best_project(
        PathBuf::from("/Users/joey/Programming/client/app").as_path(),
        &projects,
    )
    .unwrap();

    assert_eq!(matched.id, "nested");
}

#[test]
fn parses_opencode_serve_process_with_port() {
    let parsed = parse_serve_process_line("12345 opencode serve --port 4096").unwrap();

    assert_eq!(
        parsed,
        ParsedServeProcess {
            pid: 12345,
            port: 4096,
        }
    );
}

#[test]
fn regular_process_parser_ignores_serve_processes() {
    assert!(parse_process_line("12345 opencode serve --port 4096").is_none());
}

#[test]
fn find_orphaned_serve_pids_returns_pids_not_in_registry() {
    use opencode_multiplexer::registry::{find_orphaned_serve_pids, ServeEntry};

    let serve_processes = vec![
        ParsedServeProcess { pid: 100, port: 4200 },
        ParsedServeProcess { pid: 200, port: 4201 },
        ParsedServeProcess { pid: 300, port: 4202 },
    ];

    // Registry has PID 100 and 300 — 200 is orphaned
    let registry = vec![
        ServeEntry {
            port: 4200,
            pid: 100,
            cwd: "/repo/a".into(),
            tui_pid: None,
        },
        ServeEntry {
            port: 4202,
            pid: 300,
            cwd: "/repo/c".into(),
            tui_pid: None,
        },
    ];

    let orphaned = find_orphaned_serve_pids(&serve_processes, &registry);
    assert_eq!(orphaned, vec![200]);
}

#[test]
fn find_orphaned_serve_pids_empty_when_all_registered() {
    use opencode_multiplexer::registry::{find_orphaned_serve_pids, ServeEntry};

    let serve_processes = vec![
        ParsedServeProcess { pid: 100, port: 4200 },
    ];
    let registry = vec![ServeEntry {
        port: 4200,
        pid: 100,
        cwd: "/repo/a".into(),
        tui_pid: None,
    }];

    let orphaned = find_orphaned_serve_pids(&serve_processes, &registry);
    assert!(orphaned.is_empty());
}

#[test]
fn find_orphaned_serve_pids_all_orphaned_when_registry_empty() {
    use opencode_multiplexer::registry::{find_orphaned_serve_pids, ServeEntry};

    let serve_processes = vec![
        ParsedServeProcess { pid: 100, port: 4200 },
        ParsedServeProcess { pid: 200, port: 4201 },
    ];
    let registry: Vec<ServeEntry> = vec![];

    let orphaned = find_orphaned_serve_pids(&serve_processes, &registry);
    assert_eq!(orphaned, vec![100, 200]);
}
