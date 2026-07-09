use std::path::PathBuf;

use opencode_multiplexer::{
    app::sessions::{SessionOrigin, SessionStatus},
    data::poller::{DiscoveredSessionInfo, DiscoverySource, PollSnapshot},
    ops::opencode_events::SessionCreatedEvent,
    terminal::manager::PtyManager,
};

fn register_managed(
    manager: &mut PtyManager,
    session_id: Option<&str>,
    serve_port: Option<u16>,
    title: &str,
) {
    manager.register_placeholder(
        PathBuf::from("/tmp/project"),
        title.into(),
        SessionStatus::Working,
        session_id.map(String::from),
        SessionOrigin::Managed,
        None,
        Some(std::process::id()),
        serve_port,
        None,
        None,
        None,
        false,
        vec![],
    );
}

fn discovered(session_id: &str, title: &str) -> DiscoveredSessionInfo {
    DiscoveredSessionInfo {
        session_id: session_id.into(),
        cwd: PathBuf::from("/tmp/project"),
        title: title.into(),
        status: SessionStatus::Idle,
        process_pid: None,
        model: None,
        preview: None,
        time_updated: Some(200),
        has_children: false,
        children: vec![],
        serve_port: None,
        source: DiscoverySource::TuiExplicit,
    }
}

fn discovered_with_source(
    session_id: &str,
    title: &str,
    source: DiscoverySource,
    time_updated: Option<i64>,
) -> DiscoveredSessionInfo {
    DiscoveredSessionInfo {
        session_id: session_id.into(),
        cwd: PathBuf::from("/tmp/project"),
        title: title.into(),
        status: SessionStatus::Idle,
        process_pid: None,
        model: None,
        preview: None,
        time_updated,
        has_children: false,
        children: vec![],
        serve_port: Some(4200),
        source,
    }
}

#[test]
fn apply_session_event_resolves_unresolved_managed_session() {
    let mut manager = PtyManager::default();
    register_managed(&mut manager, None, Some(4200), "ledger-extraction");

    let event = SessionCreatedEvent {
        session_id: "sess_new".into(),
        parent_id: None,
    };
    let dirty = manager.apply_session_event(4200, &event);

    assert!(dirty, "should be dirty (session_id resolved None→Some)");
    let summary = manager.sessions().items()[0].clone();
    assert_eq!(summary.session_id.as_deref(), Some("sess_new"));
}

#[test]
fn apply_session_event_replaces_session_id_for_new_session() {
    let mut manager = PtyManager::default();
    register_managed(&mut manager, Some("old_session"), Some(4200), "Old Title");

    let event = SessionCreatedEvent {
        session_id: "new_session".into(),
        parent_id: None,
    };
    let dirty = manager.apply_session_event(4200, &event);

    assert!(
        dirty,
        "dirty: /new replaces the old session_id, so managed-sessions.json must be re-saved"
    );
    let summary = manager.sessions().items()[0].clone();
    assert_eq!(summary.session_id.as_deref(), Some("new_session"));
}

#[test]
fn apply_session_event_ignores_child_sessions() {
    let mut manager = PtyManager::default();
    register_managed(&mut manager, Some("parent_session"), Some(4200), "Parent");

    let event = SessionCreatedEvent {
        session_id: "child_session".into(),
        parent_id: Some("parent_session".into()),
    };
    let dirty = manager.apply_session_event(4200, &event);

    assert!(!dirty, "child session events should be ignored");
    let summary = manager.sessions().items()[0].clone();
    assert_eq!(
        summary.session_id.as_deref(),
        Some("parent_session"),
        "session_id should not change for child events"
    );
}

#[test]
fn apply_session_event_ignores_unknown_port() {
    let mut manager = PtyManager::default();
    register_managed(&mut manager, Some("sess_a"), Some(4200), "Project A");

    let event = SessionCreatedEvent {
        session_id: "sess_b".into(),
        parent_id: None,
    };
    let dirty = manager.apply_session_event(9999, &event);

    assert!(!dirty, "no managed entry on this port");
    let summary = manager.sessions().items()[0].clone();
    assert_eq!(summary.session_id.as_deref(), Some("sess_a"));
}

#[test]
fn apply_session_event_then_poll_updates_title() {
    let mut manager = PtyManager::default();
    register_managed(&mut manager, None, Some(4200), "ledger-extraction");

    // SSE resolves the session_id
    let event = SessionCreatedEvent {
        session_id: "sess_real".into(),
        parent_id: None,
    };
    let dirty = manager.apply_session_event(4200, &event);
    assert!(dirty);

    // Now the poller should hydrate the title via the resolved session_id
    manager.apply_poll_snapshot(PollSnapshot {
        sessions: vec![discovered(
            "sess_real",
            "ADO-3225 - CPTP to Cost per ThruPlay",
        )],
    });

    let summary = manager.sessions().items()[0].clone();
    assert_eq!(summary.session_id.as_deref(), Some("sess_real"));
    assert_eq!(
        summary.title, "ADO-3225 - CPTP to Cost per ThruPlay",
        "title should update after session_id resolution + poll"
    );
}

#[test]
fn apply_session_event_for_new_session_then_poll_updates_title() {
    let mut manager = PtyManager::default();
    register_managed(&mut manager, Some("old_sess"), Some(4200), "Old Title");

    // User does /new — SSE fires session.created for the new session
    let event = SessionCreatedEvent {
        session_id: "new_sess".into(),
        parent_id: None,
    };
    manager.apply_session_event(4200, &event);

    // Poller picks up the new session's metadata
    manager.apply_poll_snapshot(PollSnapshot {
        sessions: vec![discovered("new_sess", "New Session Title")],
    });

    let summary = manager.sessions().items()[0].clone();
    assert_eq!(summary.session_id.as_deref(), Some("new_sess"));
    assert_eq!(summary.title, "New Session Title");
}

#[test]
fn serve_discovery_does_not_claim_unresolved_managed_entry() {
    let mut manager = PtyManager::default();
    register_managed(&mut manager, None, Some(4200), "ledger-extraction");

    manager.apply_poll_snapshot(PollSnapshot {
        sessions: vec![discovered_with_source(
            "sess_other",
            "Other Session",
            DiscoverySource::Serve,
            Some(200),
        )],
    });

    let summary = manager.sessions().items()[0].clone();
    assert_eq!(summary.session_id.as_deref(), None);
    assert_eq!(summary.title, "ledger-extraction");
}

#[test]
fn apply_session_event_resolves_unresolved_entry() {
    let mut manager = PtyManager::default();
    register_managed(&mut manager, None, Some(4200), "ledger-extraction");

    // A real session.created event resolves the unresolved managed entry.
    let event = SessionCreatedEvent {
        session_id: "sess_reconciled".into(),
        parent_id: None,
    };
    assert!(manager.apply_session_event(4200, &event));

    let summary = manager.sessions().items()[0].clone();
    assert_eq!(summary.session_id.as_deref(), Some("sess_reconciled"));
}
