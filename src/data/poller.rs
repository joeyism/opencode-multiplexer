use std::{
    collections::HashMap,
    path::PathBuf,
    sync::mpsc::{self, Sender},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::app::sessions::SessionStatus;
use crate::data::{
    db::reader::DbReader,
    discovery::{
        cwd::cwd_for_pid,
        find_best_project,
        ps::{scan_processes, scan_serve_processes},
    },
};
use crate::registry::{load_managed_sessions, load_serve_registry};
use chrono::DateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoverySource {
    TuiExplicit,
    TuiHeuristic,
    Serve,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredSessionInfo {
    pub session_id: String,
    pub cwd: PathBuf,
    pub title: String,
    pub status: SessionStatus,
    pub process_pid: Option<u32>,
    pub model: Option<String>,
    pub preview: Option<String>,
    pub time_updated: Option<i64>,
    pub has_children: bool,
    pub children: Vec<ChildSessionInfo>,
    pub serve_port: Option<u16>,
    pub source: DiscoverySource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildSessionInfo {
    pub session_id: String,
    pub cwd: PathBuf,
    pub title: String,
    pub status: SessionStatus,
    pub time_updated: Option<i64>,
    pub has_children: bool,
    pub children: Vec<ChildSessionInfo>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PollSnapshot {
    pub sessions: Vec<DiscoveredSessionInfo>,
}

pub struct ServeSessionInfo {
    pub is_top_level: bool,
    pub is_managed: bool,
    pub status: SessionStatus,
}

pub fn should_include_serve_session(session: &ServeSessionInfo) -> bool {
    session.is_top_level
}

pub struct PollerHandle {
    stop_tx: Sender<()>,
    join_handle: thread::JoinHandle<()>,
}

impl PollerHandle {
    pub fn stop(self) {
        let _ = self.stop_tx.send(());
        let _ = self.join_handle.join();
    }
}

pub fn start_poller(poll_tx: Sender<PollSnapshot>) -> PollerHandle {
    let (stop_tx, stop_rx) = mpsc::channel();
    let join_handle = thread::spawn(move || {
        let mut last_full_poll: Option<std::time::Instant> = None;
        let mut cached_serve: Vec<DiscoveredSessionInfo> = Vec::new();

        loop {
            if stop_rx.try_recv().is_ok() {
                break;
            }

            let due_full = last_full_poll
                .map(|t| t.elapsed() >= Duration::from_secs(30))
                .unwrap_or(true);

            let snapshot = if due_full {
                match poll_full() {
                    Ok(full) => {
                        cached_serve = extract_serve_only_sessions(&full);
                        last_full_poll = Some(std::time::Instant::now());
                        full
                    }
                    Err(_) => {
                        if stop_rx.recv_timeout(Duration::from_secs(1)).is_ok() {
                            break;
                        }
                        continue;
                    }
                }
            } else {
                match poll_fast() {
                    Ok(fast) => match merge_cached_serve_sessions(fast, &cached_serve) {
                        Ok(merged) => merged,
                        Err(_) => {
                            if stop_rx.recv_timeout(Duration::from_secs(1)).is_ok() {
                                break;
                            }
                            continue;
                        }
                    },
                    Err(_) => {
                        if stop_rx.recv_timeout(Duration::from_secs(1)).is_ok() {
                            break;
                        }
                        continue;
                    }
                }
            };

            let _ = poll_tx.send(snapshot);
            if stop_rx.recv_timeout(Duration::from_secs(1)).is_ok() {
                break;
            }
        }
    });

    PollerHandle {
        stop_tx,
        join_handle,
    }
}

pub fn poll_fast() -> anyhow::Result<PollSnapshot> {
    let reader = DbReader::open_default()?;
    let projects = reader.get_projects()?;
    let processes = scan_processes()?;
    let managed_sessions = load_managed_sessions().unwrap_or_default();
    let mut sessions = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut offsets: HashMap<String, usize> = HashMap::new();
    let managed_tui_pids: std::collections::HashSet<u32> = load_serve_registry()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|e| e.tui_pid)
        .collect();

    // TUI processes (explicit -s flag + heuristic guessing)
    for process in processes {
        if process.session_id.is_none() && managed_tui_pids.contains(&process.pid) {
            continue;
        }
        let Some(cwd) = cwd_for_pid(process.pid)? else {
            continue;
        };
        let Some(project) = find_best_project(&cwd, &projects) else {
            continue;
        };

        let (session_id, source) = if let Some(sid) = process.session_id.as_deref() {
            (sid.to_string(), DiscoverySource::TuiExplicit)
        } else {
            let offset = offsets.entry(project.id.clone()).or_insert(0);
            let session = reader.get_most_recent_session(&project.id, *offset)?;
            *offset += 1;
            match session {
                Some(s) => (s.id, DiscoverySource::TuiHeuristic),
                None => continue,
            }
        };

        if !seen.insert(session_id.clone()) {
            continue;
        }

        let title_fallback = project
            .worktree
            .file_name()
            .map(|name| name.to_string_lossy().to_string());
        if let Some(info) = hydrate_session(
            &reader,
            &session_id,
            Some(process.pid),
            None,
            source,
            Some(cwd.clone()),
            title_fallback,
        )? {
            sessions.push(info);
        }
    }

    // Managed sessions (even without active TUI)
    for managed_id in managed_sessions {
        if !seen.insert(managed_id.clone()) {
            continue;
        }
        let Some(proj) = projects
            .iter()
            .find(|project| {
                reader
                    .get_session_by_id(&managed_id)
                    .ok()
                    .flatten()
                    .is_some_and(|s| s.project_id == project.id)
            })
            .cloned()
        else {
            continue;
        };
        if let Some(info) = hydrate_session(
            &reader,
            &managed_id,
            None,
            None,
            DiscoverySource::TuiExplicit,
            Some(proj.worktree.clone()),
            None,
        )
        .ok()
        .flatten()
        {
            sessions.push(info);
        }
    }

    Ok(PollSnapshot { sessions })
}

pub fn poll_full() -> anyhow::Result<PollSnapshot> {
    let mut snapshot = poll_fast()?;
    let mut seen: std::collections::HashSet<String> = snapshot
        .sessions
        .iter()
        .map(|s| s.session_id.clone())
        .collect();

    let reader = DbReader::open_default()?;
    let projects = reader.get_projects()?;
    let serve_processes = scan_serve_processes().unwrap_or_default();

    let serve_sessions_by_port: HashMap<u16, Vec<String>> = std::thread::scope(|s| {
        let mut handles = Vec::new();
        for serve_process in &serve_processes {
            let port = serve_process.port;
            handles.push((
                port,
                s.spawn(move || fetch_recent_serve_session_ids(port).unwrap_or_default()),
            ));
        }
        let mut results = HashMap::new();
        for (port, handle) in handles {
            results.insert(port, handle.join().unwrap_or_default());
        }
        results
    });

    for serve_process in serve_processes {
        for serve_session_id in serve_sessions_by_port
            .get(&serve_process.port)
            .cloned()
            .unwrap_or_default()
        {
            if seen.contains(&serve_session_id) {
                continue;
            }
            // Skip sessions that fail DB queries instead of aborting the entire
            // snapshot. With 20+ serve processes hitting the same SQLite DB,
            // individual queries can fail with "database is locked". A single
            // failure should not cause us to lose all other serve discoveries.
            let Some(session) = reader.get_session_by_id(&serve_session_id).ok().flatten() else {
                continue;
            };
            if !reader
                .is_top_level_session(&serve_session_id)
                .unwrap_or(false)
            {
                continue;
            }
            let status = reader
                .get_session_status(&serve_session_id)
                .unwrap_or(crate::app::sessions::SessionStatus::Idle);
            seen.insert(serve_session_id.clone());
            let cwd = if session.directory.as_os_str().is_empty() {
                if let Some(proj) = projects
                    .iter()
                    .find(|project| project.id == session.project_id)
                {
                    proj.worktree.clone()
                } else {
                    continue;
                }
            } else {
                session.directory.clone()
            };
            snapshot.sessions.push(DiscoveredSessionInfo {
                session_id: serve_session_id.clone(),
                cwd,
                title: session.title.clone(),
                status,
                process_pid: Some(serve_process.pid),
                model: reader.get_session_model(&serve_session_id).ok().flatten(),
                preview: reader
                    .get_last_message_preview(&serve_session_id)
                    .ok()
                    .flatten()
                    .map(|preview| preview.text),
                time_updated: Some(session.time_updated),
                has_children: reader
                    .has_child_sessions(&serve_session_id)
                    .unwrap_or(false),
                children: collect_children(&reader, &serve_session_id, 2).unwrap_or_default(),
                serve_port: Some(serve_process.port),
                source: DiscoverySource::Serve,
            });
        }
    }

    Ok(snapshot)
}

/// Backwards-compatible alias — polls fast path + serve discovery.
pub fn poll_once() -> anyhow::Result<PollSnapshot> {
    poll_full()
}

fn extract_serve_only_sessions(snapshot: &PollSnapshot) -> Vec<DiscoveredSessionInfo> {
    snapshot
        .sessions
        .iter()
        .filter(|s| matches!(s.source, DiscoverySource::Serve))
        .cloned()
        .collect()
}

/// Merge cached serve sessions into a fast poll snapshot, validating each
/// cached entry against the DB to ensure the session still exists.
pub fn merge_cached_serve_sessions(
    fast: PollSnapshot,
    cached: &[DiscoveredSessionInfo],
) -> anyhow::Result<PollSnapshot> {
    let reader = DbReader::open_default()?;
    merge_cached_serve_sessions_with_reader(fast, cached, &reader)
}

/// Testable variant that accepts an existing [`DbReader`].
pub fn merge_cached_serve_sessions_with_reader(
    mut fast: PollSnapshot,
    cached: &[DiscoveredSessionInfo],
    reader: &DbReader,
) -> anyhow::Result<PollSnapshot> {
    let fast_ids: std::collections::HashSet<String> =
        fast.sessions.iter().map(|s| s.session_id.clone()).collect();
    for entry in cached {
        if !fast_ids.contains(&entry.session_id) {
            // Validate the cached session still exists in the DB
            if reader.get_session_by_id(&entry.session_id)?.is_some() {
                fast.sessions.push(entry.clone());
            }
        }
    }
    Ok(fast)
}

fn collect_children(
    reader: &DbReader,
    parent_id: &str,
    max_depth: usize,
) -> anyhow::Result<Vec<ChildSessionInfo>> {
    if max_depth == 0 {
        return Ok(vec![]);
    }
    let children = reader.get_child_sessions(parent_id, 10, 0)?;
    children
        .into_iter()
        .map(|child| {
            let status = reader.get_session_status(&child.id)?;
            let has_children = reader.has_child_sessions(&child.id)?;
            let nested = if has_children {
                collect_children(reader, &child.id, max_depth - 1)?
            } else {
                vec![]
            };
            Ok(ChildSessionInfo {
                session_id: child.id,
                cwd: child.directory.clone(),
                title: child.title,
                status,
                time_updated: Some(child.time_updated),
                has_children,
                children: nested,
            })
        })
        .collect()
}

fn fetch_recent_serve_session_ids(port: u16) -> anyhow::Result<Vec<String>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(200))
        .build()?;
    let response = client
        .get(format!("http://localhost:{port}/session"))
        .send()?;
    if !response.status().is_success() {
        return Ok(vec![]);
    }
    let json: serde_json::Value = response.json()?;
    let cutoff = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_secs()
        .saturating_sub(24 * 60 * 60);
    let sessions = json
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let id = entry.get("id")?.as_str()?.to_string();
            let updated = entry.pointer("/time/updated")?;
            let updated_epoch = if let Some(value) = updated.as_u64() {
                normalize_epoch_secs(value)
            } else if let Some(value) = updated.as_str() {
                normalize_epoch_secs(
                    value
                        .parse::<u64>()
                        .ok()
                        .or_else(|| chrono_like_epoch(value))?,
                )
            } else {
                return None;
            };
            (updated_epoch > cutoff).then_some(id)
        })
        .collect();
    Ok(sessions)
}

fn normalize_epoch_secs(value: u64) -> u64 {
    if value > 10_000_000_000 {
        value / 1000
    } else {
        value
    }
}

fn chrono_like_epoch(value: &str) -> Option<u64> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.timestamp().max(0) as u64)
}

/// Hydrate a single session from the DB into a `DiscoveredSessionInfo`.
/// Returns `Ok(None)` if the session doesn't exist.
/// If the session's directory is empty, uses `cwd_fallback` if provided.
/// If the session's title is empty, uses `title_fallback` if provided.
fn hydrate_session(
    reader: &DbReader,
    session_id: &str,
    process_pid: Option<u32>,
    serve_port: Option<u16>,
    source: DiscoverySource,
    cwd_fallback: Option<PathBuf>,
    title_fallback: Option<String>,
) -> anyhow::Result<Option<DiscoveredSessionInfo>> {
    let Some(session) = reader.get_session_by_id(session_id)? else {
        return Ok(None);
    };
    let cwd = if session.directory.as_os_str().is_empty() {
        match cwd_fallback {
            Some(fallback) => fallback,
            None => return Ok(None),
        }
    } else {
        session.directory.clone()
    };
    let title = if session.title.is_empty() {
        title_fallback.unwrap_or_else(|| cwd.display().to_string())
    } else {
        session.title.clone()
    };
    let status = reader.get_session_status(session_id)?;
    Ok(Some(DiscoveredSessionInfo {
        session_id: session_id.to_string(),
        cwd,
        title,
        status,
        process_pid,
        model: reader.get_session_model(session_id)?,
        preview: reader.get_last_message_preview(session_id)?.map(|p| p.text),
        time_updated: Some(session.time_updated),
        has_children: reader.has_child_sessions(session_id)?,
        children: collect_children(reader, session_id, 2)?,
        serve_port,
        source,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::sessions::SessionStatus;
    use rusqlite::Connection;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_db_path(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("ocmux-rs-{label}-{nanos}.db"))
    }

    fn init_db(path: &PathBuf) -> Connection {
        let conn = Connection::open(path).unwrap();
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
            CREATE TABLE part (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                data TEXT NOT NULL,
                time_created INTEGER
            );
            "#,
        )
        .unwrap();
        conn
    }

    #[test]
    fn hydrate_session_builds_info_from_db() {
        let db_path = temp_db_path("hydrate");
        let conn = init_db(&db_path);
        conn.execute(
            "INSERT INTO project VALUES ('proj1', '/tmp/proj', 'proj', 100, 200)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session VALUES ('sess1', 'proj1', NULL, 'My Title', '/tmp/proj', NULL, 100, 200, NULL)",
            [],
        )
        .unwrap();
        conn.execute(
            r#"INSERT INTO message VALUES ('msg1', 'sess1', '{"role":"assistant","time":{"completed":200}}', 200)"#,
            [],
        )
        .unwrap();

        let reader = DbReader::open(&db_path).unwrap();
        let info = hydrate_session(
            &reader,
            "sess1",
            Some(123),
            Some(4200),
            DiscoverySource::Serve,
            None,
            None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(info.session_id, "sess1");
        assert_eq!(info.title, "My Title");
        assert_eq!(info.status, SessionStatus::Idle);
        assert_eq!(info.process_pid, Some(123));
        assert_eq!(info.serve_port, Some(4200));
        assert_eq!(info.source, DiscoverySource::Serve);
        assert_eq!(info.cwd, PathBuf::from("/tmp/proj"));
    }

    #[test]
    fn hydrate_session_returns_none_for_missing_session() {
        let db_path = temp_db_path("hydrate-miss");
        let _conn = init_db(&db_path);
        let reader = DbReader::open(&db_path).unwrap();
        let result = hydrate_session(
            &reader,
            "nonexistent",
            None,
            None,
            DiscoverySource::Serve,
            None,
            None,
        )
        .unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn hydrate_session_returns_none_for_empty_directory() {
        let db_path = temp_db_path("hydrate-nodir");
        let conn = init_db(&db_path);
        conn.execute(
            "INSERT INTO project VALUES ('proj1', '/tmp/proj', 'proj', 100, 200)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session VALUES ('sess1', 'proj1', NULL, 'Title', '', NULL, 100, 200, NULL)",
            [],
        )
        .unwrap();

        let reader = DbReader::open(&db_path).unwrap();
        let result = hydrate_session(
            &reader,
            "sess1",
            None,
            None,
            DiscoverySource::Serve,
            None,
            None,
        )
        .unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn normalize_epoch_secs_handles_milliseconds() {
        assert_eq!(normalize_epoch_secs(1_778_018_371_220), 1_778_018_371);
        assert_eq!(normalize_epoch_secs(1_778_018_371), 1_778_018_371);
    }

    #[test]
    fn merge_cached_serve_sessions_adds_serve_only_entries() {
        let db_path = temp_db_path("merge-adds");
        let conn = init_db(&db_path);
        conn.execute(
            "INSERT INTO project VALUES ('proj1', '/tmp/proj', 'proj', 100, 200)",
            [],
        )
        .unwrap();
        // Insert sess_c so it survives validation
        conn.execute(
            "INSERT INTO session VALUES ('sess_c', 'proj1', NULL, 'C', '/tmp/c', NULL, 100, 200, NULL)",
            [],
        )
        .unwrap();

        let reader = DbReader::open(&db_path).unwrap();
        let fast = PollSnapshot {
            sessions: vec![
                DiscoveredSessionInfo {
                    session_id: "sess_a".into(),
                    cwd: PathBuf::from("/tmp/a"),
                    title: "A".into(),
                    status: SessionStatus::Working,
                    process_pid: Some(100),
                    model: None,
                    preview: None,
                    time_updated: None,
                    has_children: false,
                    children: vec![],
                    serve_port: None,
                    source: DiscoverySource::TuiExplicit,
                },
                DiscoveredSessionInfo {
                    session_id: "sess_b".into(),
                    cwd: PathBuf::from("/tmp/b"),
                    title: "B".into(),
                    status: SessionStatus::Idle,
                    process_pid: None,
                    model: None,
                    preview: None,
                    time_updated: None,
                    has_children: false,
                    children: vec![],
                    serve_port: None,
                    source: DiscoverySource::TuiExplicit,
                },
            ],
        };
        let cached = vec![
            DiscoveredSessionInfo {
                session_id: "sess_b".into(),
                cwd: PathBuf::from("/tmp/b"),
                title: "B Old".into(),
                status: SessionStatus::Idle,
                process_pid: Some(200),
                model: None,
                preview: None,
                time_updated: None,
                has_children: false,
                children: vec![],
                serve_port: Some(4200),
                source: DiscoverySource::Serve,
            },
            DiscoveredSessionInfo {
                session_id: "sess_c".into(),
                cwd: PathBuf::from("/tmp/c"),
                title: "C".into(),
                status: SessionStatus::NeedsInput,
                process_pid: Some(300),
                model: None,
                preview: None,
                time_updated: None,
                has_children: false,
                children: vec![],
                serve_port: Some(4201),
                source: DiscoverySource::Serve,
            },
        ];
        let merged = merge_cached_serve_sessions_with_reader(fast, &cached, &reader).unwrap();
        assert_eq!(merged.sessions.len(), 3);
        // Fast B should win over cached B
        let b = merged
            .sessions
            .iter()
            .find(|s| s.session_id == "sess_b")
            .unwrap();
        assert_eq!(b.title, "B");
        assert_eq!(b.source, DiscoverySource::TuiExplicit);
        // C should be added from cache
        let c = merged
            .sessions
            .iter()
            .find(|s| s.session_id == "sess_c")
            .unwrap();
        assert_eq!(c.title, "C");
        assert_eq!(c.source, DiscoverySource::Serve);
    }

    #[test]
    fn merge_cached_serve_sessions_does_not_duplicate_when_fast_covers_all() {
        let db_path = temp_db_path("merge-no-dup");
        let _conn = init_db(&db_path);
        let reader = DbReader::open(&db_path).unwrap();

        let fast = PollSnapshot {
            sessions: vec![DiscoveredSessionInfo {
                session_id: "sess_a".into(),
                cwd: PathBuf::from("/tmp/a"),
                title: "A".into(),
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
        let cached = vec![DiscoveredSessionInfo {
            session_id: "sess_a".into(),
            cwd: PathBuf::from("/tmp/a"),
            title: "A Old".into(),
            status: SessionStatus::Idle,
            process_pid: Some(100),
            model: None,
            preview: None,
            time_updated: None,
            has_children: false,
            children: vec![],
            serve_port: Some(4200),
            source: DiscoverySource::Serve,
        }];
        let merged = merge_cached_serve_sessions_with_reader(fast, &cached, &reader).unwrap();
        assert_eq!(merged.sessions.len(), 1);
        assert_eq!(merged.sessions[0].title, "A");
        assert_eq!(merged.sessions[0].source, DiscoverySource::TuiExplicit);
    }

    #[test]
    fn merge_cached_skips_deleted_sessions() {
        let db_path = temp_db_path("merge-deleted");
        let conn = init_db(&db_path);
        conn.execute(
            "INSERT INTO project VALUES ('proj1', '/tmp/proj', 'proj', 100, 200)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session VALUES ('sess_alive', 'proj1', NULL, 'Alive', '/tmp/proj', NULL, 100, 200, NULL)",
            [],
        )
        .unwrap();
        // sess_deleted is NOT in the DB

        let reader = DbReader::open(&db_path).unwrap();
        let fast = PollSnapshot {
            sessions: vec![DiscoveredSessionInfo {
                session_id: "sess_fast".into(),
                cwd: PathBuf::from("/tmp/fast"),
                title: "Fast".into(),
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
        let cached = vec![
            DiscoveredSessionInfo {
                session_id: "sess_alive".into(),
                cwd: PathBuf::from("/tmp/alive"),
                title: "Alive".into(),
                status: SessionStatus::Idle,
                process_pid: None,
                model: None,
                preview: None,
                time_updated: None,
                has_children: false,
                children: vec![],
                serve_port: Some(4200),
                source: DiscoverySource::Serve,
            },
            DiscoveredSessionInfo {
                session_id: "sess_deleted".into(),
                cwd: PathBuf::from("/tmp/deleted"),
                title: "Deleted".into(),
                status: SessionStatus::Idle,
                process_pid: None,
                model: None,
                preview: None,
                time_updated: None,
                has_children: false,
                children: vec![],
                serve_port: Some(4201),
                source: DiscoverySource::Serve,
            },
        ];

        let merged = merge_cached_serve_sessions_with_reader(fast, &cached, &reader).unwrap();
        assert_eq!(merged.sessions.len(), 2);
        assert!(merged.sessions.iter().any(|s| s.session_id == "sess_fast"));
        assert!(merged.sessions.iter().any(|s| s.session_id == "sess_alive"));
        assert!(
            !merged
                .sessions
                .iter()
                .any(|s| s.session_id == "sess_deleted"),
            "Deleted session should be skipped"
        );
    }

    #[test]
    fn merge_cached_keeps_existing_sessions() {
        let db_path = temp_db_path("merge-keeps");
        let conn = init_db(&db_path);
        conn.execute(
            "INSERT INTO project VALUES ('proj1', '/tmp/proj', 'proj', 100, 200)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO session VALUES ('sess_cached', 'proj1', NULL, 'Cached', '/tmp/proj', NULL, 100, 200, NULL)",
            [],
        )
        .unwrap();

        let reader = DbReader::open(&db_path).unwrap();
        let fast = PollSnapshot {
            sessions: vec![DiscoveredSessionInfo {
                session_id: "sess_fast".into(),
                cwd: PathBuf::from("/tmp/fast"),
                title: "Fast".into(),
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
        let cached = vec![DiscoveredSessionInfo {
            session_id: "sess_cached".into(),
            cwd: PathBuf::from("/tmp/cached"),
            title: "Cached".into(),
            status: SessionStatus::Idle,
            process_pid: Some(200),
            model: None,
            preview: None,
            time_updated: None,
            has_children: false,
            children: vec![],
            serve_port: Some(4200),
            source: DiscoverySource::Serve,
        }];

        let merged = merge_cached_serve_sessions_with_reader(fast, &cached, &reader).unwrap();
        assert_eq!(merged.sessions.len(), 2);
        let cached_session = merged
            .sessions
            .iter()
            .find(|s| s.session_id == "sess_cached")
            .unwrap();
        assert_eq!(cached_session.title, "Cached");
        assert_eq!(cached_session.source, DiscoverySource::Serve);
    }
}
