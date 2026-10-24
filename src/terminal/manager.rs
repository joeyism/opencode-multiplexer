use std::{
    collections::HashMap,
    fmt::Display,
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    app::sessions::{SessionList, SessionOrigin, SessionStatus, SessionSummary},
    data::db::reader::DbReader,
    data::poller::{ChildSessionInfo, DiscoverySource, PollSnapshot},
    registry::is_pid_alive,
    ui::sidebar::{ChildSidebarEntry, SidebarEntry},
};

use super::pty::PtySession;

fn identity_log(msg: impl Display) {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let path = PathBuf::from(home).join(".config/ocmux/identity-debug.log");
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| writeln!(file, "{ts} {msg}"));
}

#[derive(Default)]
pub struct PtyManager {
    sessions: SessionList,
    ptys: HashMap<u64, Option<PtySession>>,
}

impl PtyManager {
    #[allow(clippy::too_many_arguments)]
    pub fn register_placeholder(
        &mut self,
        cwd: PathBuf,
        title: String,
        status: SessionStatus,
        session_id: Option<String>,
        origin: SessionOrigin,
        process_pid: Option<u32>,
        serve_pid: Option<u32>,
        serve_port: Option<u16>,
        model: Option<String>,
        preview: Option<String>,
        time_updated: Option<i64>,
        has_children: bool,
        children: Vec<crate::data::poller::ChildSessionInfo>,
    ) -> u64 {
        let id = self.sessions.push(
            cwd,
            title,
            status,
            session_id,
            origin,
            process_pid,
            serve_pid,
            serve_port,
            model,
            preview,
            time_updated,
            has_children,
            children,
        );
        self.ptys.insert(id, None);
        id
    }

    pub fn spawn_managed(
        &mut self,
        cwd: PathBuf,
        title: String,
        rows: u16,
        cols: u16,
    ) -> anyhow::Result<u64> {
        use crate::ops::opencode::{find_available_port, spawn_serve_daemon, wait_for_serve_ready};
        use crate::registry::{register_serve_process, update_serve_registry_tui_pid};

        // Spawn serve daemon as persistent backend
        let port = find_available_port(4200);
        let serve_pid = spawn_serve_daemon(&cwd, port)?;
        register_serve_process(port, serve_pid, &cwd)?;

        // Wait for serve to be ready
        if !wait_for_serve_ready(port, 10) {
            anyhow::bail!("opencode serve did not start within 10s on port {port}");
        }

        // Spawn TUI client attached to our serve via `opencode attach`.
        // The session_id will be resolved by the SSE subscriber listening for
        // `session.created` events on this serve's /event stream — the TUI
        // creates a session lazily (on first message, not startup), so the
        // old before/after diff approach would time out.
        let pty = PtySession::spawn_managed(&cwd, port, rows, cols)?;
        let process_pid = pty.process_id();
        if let Some(pid) = process_pid {
            let _ = update_serve_registry_tui_pid(port, pid);
        }

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let cwd_for_log = cwd.clone();
        let id = self.sessions.push(
            cwd,
            title,
            SessionStatus::Working,
            None,
            SessionOrigin::Managed,
            process_pid,
            Some(serve_pid),
            Some(port),
            None,
            None,
            Some(now_ms),
            false,
            vec![],
        );
        self.ptys.insert(id, Some(pty));
        identity_log(format!(
            "spawn_managed entry={id} port={port} cwd={cwd_for_log:?} initial session_id=None"
        ));
        self.sessions.select_last();
        self.sessions.activate_selected();
        Ok(id)
    }

    pub fn activate_or_attach_selected(&mut self, rows: u16, cols: u16) -> anyhow::Result<()> {
        let Some(selected_id) = self.selected_id() else {
            return Ok(());
        };

        let needs_attach = self.ptys.get(&selected_id).is_some_and(|pty| pty.is_none());
        if needs_attach {
            let Some(summary) = self.selected_summary().cloned() else {
                return Ok(());
            };
            if let Some(session_id) = summary.session_id.as_deref() {
                let pty = PtySession::spawn_replica(
                    &summary.cwd,
                    session_id,
                    summary.serve_port,
                    rows,
                    cols,
                )?;
                let process_pid = pty.process_id();
                if let Some(slot) = self.ptys.get_mut(&selected_id) {
                    *slot = Some(pty);
                }
                if let Some(summary) = self.sessions.get_mut(selected_id) {
                    summary.origin = SessionOrigin::Managed;
                    summary.process_pid = process_pid;
                }
            }
        }

        self.activate_selected();
        self.resize_active(rows, cols)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn attach_arbitrary_session(
        &mut self,
        session_id: String,
        cwd: PathBuf,
        title: String,
        status: SessionStatus,
        time_updated: Option<i64>,
        rows: u16,
        cols: u16,
    ) -> anyhow::Result<()> {
        let (serve_port, serve_pid) = crate::ops::opencode::ensure_serve_for_cwd(&cwd)?;
        self.attach_arbitrary_session_to_serve(
            session_id,
            cwd,
            title,
            status,
            time_updated,
            serve_port,
            serve_pid,
            rows,
            cols,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn attach_arbitrary_session_to_serve(
        &mut self,
        session_id: String,
        cwd: PathBuf,
        title: String,
        status: SessionStatus,
        time_updated: Option<i64>,
        serve_port: u16,
        serve_pid: u32,
        rows: u16,
        cols: u16,
    ) -> anyhow::Result<()> {
        let pty = PtySession::spawn_replica(&cwd, &session_id, Some(serve_port), rows, cols)?;
        let process_pid = pty.process_id();

        if let Some(existing_id) = self.sessions.find_by_session_id(&session_id) {
            if let Some(summary) = self.sessions.get_mut(existing_id) {
                summary.origin = SessionOrigin::Managed;
                summary.cwd = cwd.clone();
                summary.title = title;
                summary.status = status;
                summary.process_pid = process_pid;
                summary.time_updated = time_updated;
                summary.serve_pid = Some(serve_pid);
                summary.serve_port = Some(serve_port);
            }
            self.ptys.insert(existing_id, Some(pty));
            self.sessions.select_id(existing_id);
            self.sessions.activate_selected();
            return Ok(());
        }

        let id = self.sessions.push(
            cwd,
            title,
            status,
            Some(session_id),
            SessionOrigin::Managed,
            process_pid,
            Some(serve_pid),
            Some(serve_port),
            None,
            None,
            time_updated,
            false,
            vec![],
        );
        self.ptys.insert(id, Some(pty));
        self.sessions.select_last();
        self.sessions.activate_selected();
        Ok(())
    }

    pub fn reconnect_moved_session_tree(
        &mut self,
        session_ids: &[String],
        focused_session_id: &str,
        destination: PathBuf,
        rows: u16,
        cols: u16,
    ) -> anyhow::Result<()> {
        let focused = crate::data::db::reader::DbReader::open_default()?
            .get_session_by_id(focused_session_id)?
            .ok_or_else(|| anyhow::anyhow!("moved session disappeared from the database"))?;
        for summary in self.sessions.items_mut() {
            let Some(session_id) = summary.session_id.as_deref() else {
                continue;
            };
            if !session_ids.iter().any(|id| id == session_id) {
                continue;
            }
            if let Some(Some(pty)) = self.ptys.get_mut(&summary.id) {
                let _ = pty.kill();
            }
            self.ptys.insert(summary.id, None);
            summary.cwd = destination.clone();
        }

        self.attach_arbitrary_session(
            focused_session_id.to_owned(),
            destination.clone(),
            focused.title,
            SessionStatus::Idle,
            Some(focused.time_updated),
            rows,
            cols,
        )
    }

    pub fn session_tree_has_busy_status(&self, session_id: &str) -> bool {
        self.sessions.items().iter().any(|summary| {
            if summary.session_id.as_deref() == Some(session_id) {
                return is_busy_status(summary.status)
                    || summary.children.iter().any(child_tree_is_busy);
            }
            summary
                .children
                .iter()
                .any(|child| child_subtree_busy(child, session_id))
        })
    }

    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    pub fn sessions(&self) -> &SessionList {
        &self.sessions
    }

    pub fn active_id(&self) -> Option<u64> {
        self.sessions.active_id()
    }

    pub fn active_session_id(&self) -> Option<String> {
        let id = self.sessions.active_id()?;
        self.sessions
            .items()
            .iter()
            .find(|session| session.id == id)
            .and_then(|session| session.session_id.clone())
    }

    pub fn selected_id(&self) -> Option<u64> {
        self.sessions.selected_id()
    }

    pub fn pending_kill(&self) -> Option<u64> {
        self.sessions.pending_kill()
    }

    pub fn selected_index(&self) -> usize {
        self.sessions.selected_index()
    }

    pub fn selected_summary(&self) -> Option<&SessionSummary> {
        let selected = self.sessions.selected_id()?;
        self.sessions
            .items()
            .iter()
            .find(|session| session.id == selected)
    }

    pub fn select_next(&mut self) {
        self.sessions.select_next();
    }

    pub fn select_prev(&mut self) {
        self.sessions.select_prev();
    }

    pub fn select_top_level(&mut self, id: u64) {
        self.sessions.select_id(id);
    }

    pub fn detach_active(&mut self) {
        self.sessions.deactivate();
    }

    pub fn activate_selected(&mut self) {
        self.sessions.activate_selected();
    }

    pub fn request_kill_selected(&mut self) {
        self.sessions.request_kill_selected();
    }

    pub fn cancel_kill(&mut self) {
        self.sessions.cancel_kill();
    }

    pub fn kill_selected(&mut self) -> anyhow::Result<Option<u64>> {
        let id = match self.sessions.pending_kill() {
            Some(id) => id,
            None => return Ok(None),
        };

        let session = self.sessions.items().iter().find(|s| s.id == id).cloned();
        if let Some(Some(pty)) = self.ptys.get_mut(&id) {
            let _ = pty.kill();
        }

        if let Some(session) = session {
            let serve_pid = session.serve_pid.or_else(|| {
                session
                    .serve_port
                    .and_then(|port| crate::registry::serve_pid_for_port(port).ok().flatten())
            });
            let serve_session = SessionSummary {
                serve_pid,
                ..session.clone()
            };

            if !has_other_serve_reference(self.sessions.items(), &serve_session)
                && let Some(pid) = serve_pid
            {
                crate::registry::kill_and_unregister_serve(pid)?;
            }
        }

        let killed = self.sessions.confirm_kill();
        if let Some(id) = killed {
            self.ptys.remove(&id);
        }
        Ok(killed)
    }

    pub fn kill_selected_placeholder(&mut self) -> Option<u64> {
        self.sessions.request_kill_selected();
        let killed = self.sessions.confirm_kill();
        if let Some(id) = killed {
            self.ptys.remove(&id);
        }
        killed
    }

    pub fn active_session_mut(&mut self) -> Option<&mut PtySession> {
        let id = self.sessions.active_id()?;
        self.ptys.get_mut(&id)?.as_mut()
    }

    pub fn active_session(&self) -> Option<&PtySession> {
        let id = self.sessions.active_id()?;
        self.ptys.get(&id)?.as_ref()
    }

    pub fn active_summary(&self) -> Option<&SessionSummary> {
        let active_id = self.sessions.active_id()?;
        self.sessions
            .items()
            .iter()
            .find(|session| session.id == active_id)
    }

    pub fn apply_open_requests_overlay(
        &mut self,
        open_requests: &crate::ops::open_requests::OpenRequestTracker,
    ) {
        for summary in self.sessions.items_mut() {
            if let Some(session_id) = &summary.session_id {
                summary.status =
                    open_requests.overlay_status(session_id, &summary.children, summary.status);
                open_requests.overlay_child_tree(&mut summary.children);
            }
        }
    }

    pub fn reap_exited_ptys(&mut self) -> Vec<u64> {
        let dead_ids: Vec<u64> = self
            .ptys
            .iter_mut()
            .filter_map(|(&id, slot)| {
                if let Some(pty) = slot.as_mut() {
                    if !pty.is_alive() { Some(id) } else { None }
                } else {
                    None
                }
            })
            .collect();

        for &id in &dead_ids {
            if let Some(slot) = self.ptys.get_mut(&id) {
                *slot = None;
            }
        }

        dead_ids
    }

    pub fn drain_all_output(&mut self) {
        for pty in self.ptys.values_mut().filter_map(Option::as_mut) {
            pty.drain_output();
        }
    }

    pub fn resize_active(&mut self, rows: u16, cols: u16) -> anyhow::Result<()> {
        if let Some(pty) = self.active_session_mut() {
            pty.resize(rows, cols)?;
        }
        Ok(())
    }

    pub fn sidebar_entries(&self) -> Vec<SidebarEntry> {
        let mut sessions = self.sessions.items().iter().collect::<Vec<_>>();
        sessions.sort_by(|a, b| match (a.time_updated, b.time_updated) {
            (Some(a), Some(b)) => b.cmp(&a),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        });

        sessions
            .into_iter()
            .map(|session| SidebarEntry {
                top_level_id: session.id,
                session_id: session.session_id.clone(),
                cwd: session.cwd.clone(),
                title: session.title.clone(),
                status: session.status,
                active: self.sessions.active_id() == Some(session.id),
                origin: session.origin,
                time_updated: session.time_updated,
                has_children: session.has_children,
                children: session.children.iter().map(convert_child).collect(),
            })
            .collect()
    }

    pub fn apply_poll_snapshot(
        &mut self,
        snapshot: PollSnapshot,
        open_requests: &crate::ops::open_requests::OpenRequestTracker,
    ) -> bool {
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum MatchKind {
            SessionId,
        }

        let keep_ids: std::collections::HashSet<String> = snapshot
            .sessions
            .iter()
            .map(|info| info.session_id.clone())
            .collect();
        let snapshot_cwd_ids: std::collections::HashMap<
            PathBuf,
            std::collections::HashSet<String>,
        > = snapshot
            .sessions
            .iter()
            .fold(HashMap::new(), |mut map, info| {
                map.entry(info.cwd.clone())
                    .or_default()
                    .insert(info.session_id.clone());
                map
            });

        let mut matched_ids: std::collections::HashSet<u64> = std::collections::HashSet::new();

        for discovered in snapshot.sessions {
            // Match existing sessions only by exact session_id.
            // Each entry can only be matched once per snapshot.
            let matched = self
                .sessions
                .find_by_session_id(&discovered.session_id)
                .filter(|id| !matched_ids.contains(id))
                .map(|id| (id, MatchKind::SessionId));

            if let Some((id, match_kind)) = matched {
                if let Some(summary) = self.sessions.get_mut(id) {
                    // Guard: once an entry has a session_id, only a discovery
                    // with the SAME session_id may overwrite it. Non-session-id
                    // matches can guess the wrong session — both the heuristic
                    // (get_most_recent_session) and Serve discoveries (which
                    // return all sessions from the serve's project, each tagged
                    // with the serve's PID/port).
                    if summary.session_id.is_some() && !matches!(match_kind, MatchKind::SessionId) {
                        continue;
                    }

                    matched_ids.insert(id);
                    let old = summary.session_id.clone();
                    summary.session_id = Some(discovered.session_id.clone());
                    identity_log(format!(
                        "poll_session_id_match entry={}: {:?} -> {:?} (title={:?})",
                        id, old, summary.session_id, summary.title
                    ));
                    summary.cwd = discovered.cwd.clone();
                    summary.title = discovered.title.clone();
                    summary.status = open_requests.overlay_status(
                        &discovered.session_id,
                        &discovered.children,
                        discovered.status,
                    );
                    if let Some(pid) = discovered.process_pid
                        && Some(pid) != summary.serve_pid
                    {
                        summary.process_pid = Some(pid);
                    }
                    summary.model = discovered.model.clone();
                    summary.preview = discovered.preview.clone();
                    summary.time_updated = discovered.time_updated;
                    summary.has_children = discovered.has_children;
                    let mut children = discovered.children.clone();
                    open_requests.overlay_child_tree(&mut children);
                    summary.children = children;
                    // Only update serve_port for non-managed sessions.
                    // Managed sessions have an authoritative serve_port set at
                    // spawn time — a Serve discovery may report a different port
                    // when multiple serves share the same DB.
                    if discovered.serve_port.is_some() && summary.origin != SessionOrigin::Managed {
                        summary.serve_port = discovered.serve_port;
                    }
                } else {
                    continue;
                }

                continue;
            }

            // Only create sidebar entries for explicitly identified sessions
            // (TUI processes with -s flag). Serve discovery and heuristic TUI
            // guessing are for metadata refresh of existing entries only — they
            // should never create new entries. Serves sharing a DB can return
            // hundreds of sessions, and heuristic guessing can pick wrong
            // sessions entirely. Sessions enter the sidebar through
            // spawn_managed, the session picker, or the managed sessions file.
            if matches!(discovered.source, DiscoverySource::TuiExplicit) {
                let status = open_requests.overlay_status(
                    &discovered.session_id,
                    &discovered.children,
                    discovered.status,
                );
                let mut children = discovered.children.clone();
                open_requests.overlay_child_tree(&mut children);

                let placeholder_id = self.register_placeholder(
                    discovered.cwd,
                    discovered.title,
                    status,
                    Some(discovered.session_id),
                    SessionOrigin::Discovered,
                    discovered.process_pid,
                    None,
                    discovered.serve_port,
                    discovered.model,
                    discovered.preview,
                    discovered.time_updated,
                    discovered.has_children,
                    children,
                );
                matched_ids.insert(placeholder_id);
            }
        }

        // Pass 1: Flip dead managed "Working" sessions to Error.
        // If the DB says Working but the serve process is dead, the session
        // was interrupted — that's an error, not idle.
        let dead_working_ids: Vec<u64> = self
            .sessions
            .items()
            .iter()
            .filter(|session| {
                session.origin == SessionOrigin::Managed
                    && (session.status == SessionStatus::Working
                        || session.status == SessionStatus::SubagentsWorking)
                    && session.serve_pid.is_some_and(|pid| !is_pid_alive(pid))
            })
            .map(|session| session.id)
            .collect();
        for id in dead_working_ids {
            if let Some(session) = self.sessions.get_mut(id) {
                session.status = SessionStatus::Error;
            }
        }

        // Pass 2: Prune stale sessions.
        // Discovered placeholders are removed when not in the snapshot and
        // they have no live PTY. Managed sessions are also removed when they
        // have a known-dead serve backend, no live PTY, and either are not in
        // the snapshot or have been replaced by a different session in the
        // same cwd.
        let stale_ids: Vec<u64> = self
            .sessions
            .items()
            .iter()
            .filter(|session| {
                let no_pty = self.ptys.get(&session.id).is_none_or(|pty| pty.is_none());
                let not_in_snapshot = session
                    .session_id
                    .as_deref()
                    .is_none_or(|sid| !keep_ids.contains(sid));

                match session.origin {
                    SessionOrigin::Discovered => no_pty && not_in_snapshot,
                    SessionOrigin::Managed => {
                        let dead_serve = session.serve_pid.is_some_and(|pid| !is_pid_alive(pid));
                        let replaced_in_cwd = session.session_id.as_deref().is_some_and(|sid| {
                            snapshot_cwd_ids
                                .get(&session.cwd)
                                .is_some_and(|ids| ids.iter().any(|id| id != sid))
                        });
                        session.session_id.is_some()
                            && dead_serve
                            && no_pty
                            && (not_in_snapshot || replaced_in_cwd)
                    }
                }
            })
            .map(|session| session.id)
            .collect();

        let registry_dirty = stale_ids.iter().any(|id| {
            self.sessions
                .items()
                .iter()
                .find(|s| s.id == *id)
                .is_some_and(|s| s.origin == SessionOrigin::Managed && s.session_id.is_some())
        });

        self.sessions
            .retain(|session| !stale_ids.contains(&session.id));
        for id in stale_ids {
            self.ptys.remove(&id);
        }

        registry_dirty
    }

    /// Apply an authoritative `session.created` event from the serve's SSE
    /// stream.
    ///
    /// This is the primary identity-resolution path for managed sessions using
    /// `opencode attach`.  When the TUI creates a new session (either on first
    /// message or via `/new`), the serve emits `session.created` on its SSE
    /// stream.  This method updates the managed entry's `session_id`,
    /// bypassing the immutability guard in `apply_poll_snapshot` because SSE
    /// events from the serve the TUI is attached to are authoritative — not
    /// guesses.
    ///
    /// Returns `true` when the managed-sessions registry must be persisted
    /// (i.e. the `session_id` actually changed — `None` to a new id, or one
    /// id to a different id).  Re-emit events for the same id return `false`.
    /// Without this, a `/new` would leave the previous `session_id` orphaned
    /// in `managed-sessions.json`, and the next poll would resurrect it as a
    /// duplicate sidebar entry.
    pub fn apply_session_event(
        &mut self,
        port: u16,
        event: &crate::ops::opencode_events::SessionCreatedEvent,
    ) -> bool {
        if event.parent_id.is_some() {
            return false;
        }

        let Some(summary) = self
            .sessions
            .items_mut()
            .iter_mut()
            .find(|s| s.origin == SessionOrigin::Managed && s.serve_port == Some(port))
        else {
            return false;
        };

        let old = summary.session_id.clone();
        let new_id = event.session_id.clone();

        // Defensive: if the entry already has a session_id that's still active
        // in the DB, don't switch to a new one.  This prevents plugin-created
        // auxiliary sessions (e.g. opencode-ledger extraction sessions) from
        // displacing the user's primary session.  The plugin's primary fix is
        // to set `parentID` on those sessions (so the parent_id filter at the
        // top of this method rejects them) — this check is belt-and-suspenders
        // for plugins that don't set parentID.
        //
        // Tradeoff: if opencode does NOT archive the old session on /new,
        // this will block the legitimate /new switch.  If observed in
        // practice, loosen the check (e.g. only block when the new session
        // has zero user messages).
        //
        // Safe fallback: if the DB is unavailable, the check is skipped and
        // the existing behavior is preserved.
        if let Some(old_id) = old.as_deref()
            && old_id != new_id.as_str()
        {
            let old_active = DbReader::open_default()
                .ok()
                .and_then(|r| r.session_is_active(old_id).ok())
                .unwrap_or(false);
            if old_active {
                identity_log(format!(
                    "apply_session_event DEFENSIVE SKIP port={port} entry={}: \
                         old session {old_id:?} still active, ignoring new session {new_id:?}",
                    summary.id
                ));
                return false;
            }
        }

        let dirty = old.as_deref() != Some(new_id.as_str());
        summary.session_id = Some(new_id.clone());
        identity_log(format!(
            "apply_session_event port={port} entry={}: {:?} -> {:?} (dirty={dirty})",
            summary.id, old, summary.session_id
        ));
        if old.is_none() {
            eprintln!("managed session resolved on port {port} -> {new_id}");
        }
        dirty
    }

    pub fn refresh_active(&mut self, rows: u16, cols: u16) -> anyhow::Result<bool> {
        let Some(active_id) = self.sessions.active_id() else {
            return Ok(false);
        };
        let summary = self
            .sessions
            .items()
            .iter()
            .find(|s| s.id == active_id)
            .cloned();
        let Some(summary) = summary else {
            return Ok(false);
        };
        let Some(session_id) = summary.session_id.as_deref() else {
            return Ok(false);
        };

        // Kill existing PTY
        if let Some(Some(pty)) = self.ptys.get_mut(&active_id) {
            let _ = pty.kill();
        }

        // Spawn fresh replica
        let pty =
            PtySession::spawn_replica(&summary.cwd, session_id, summary.serve_port, rows, cols)?;
        if let Some(slot) = self.ptys.get_mut(&active_id) {
            *slot = Some(pty);
        }

        Ok(true)
    }

    pub fn shutdown_local_ptys(&mut self) {
        // Only kill PTY clients (TUI viewers).
        for pty in self.ptys.values_mut().filter_map(Option::as_mut) {
            let _ = pty.kill();
        }
    }

    pub fn shutdown_managed_serves(&self) -> anyhow::Result<()> {
        crate::registry::kill_all_registered_serves()
    }

    pub fn set_session_serve(&mut self, id: u64, port: u16, pid: u32) {
        if let Some(summary) = self.sessions.get_mut(id) {
            summary.serve_port = Some(port);
            summary.serve_pid = Some(pid);
            summary.origin = SessionOrigin::Managed;
        }
    }

    pub fn set_session_id(&mut self, id: u64, session_id: String) {
        if let Some(summary) = self.sessions.get_mut(id) {
            summary.session_id = Some(session_id);
            summary.origin = SessionOrigin::Managed;
        }
    }

    pub fn managed_session_ids(&self) -> Vec<String> {
        self.sessions
            .items()
            .iter()
            .filter(|session| session.origin == SessionOrigin::Managed)
            .filter_map(|session| session.session_id.clone())
            .collect()
    }

    #[doc(hidden)]
    pub fn insert_pty_for_session(&mut self, id: u64, pty: PtySession) {
        self.ptys.insert(id, Some(pty));
    }
}

fn child_subtree_busy(child: &ChildSessionInfo, session_id: &str) -> bool {
    if child.session_id == session_id {
        is_busy_status(child.status) || child.children.iter().any(child_tree_is_busy)
    } else {
        child
            .children
            .iter()
            .any(|nested| child_subtree_busy(nested, session_id))
    }
}

fn child_tree_is_busy(child: &ChildSessionInfo) -> bool {
    is_busy_status(child.status) || child.children.iter().any(child_tree_is_busy)
}

fn is_busy_status(status: SessionStatus) -> bool {
    matches!(
        status,
        SessionStatus::Working | SessionStatus::SubagentsWorking | SessionStatus::NeedsInput
    )
}

fn convert_child(child: &ChildSessionInfo) -> ChildSidebarEntry {
    ChildSidebarEntry {
        session_id: child.session_id.clone(),
        cwd: child.cwd.clone(),
        title: child.title.clone(),
        status: child.status,
        time_updated: child.time_updated,
        has_children: child.has_children,
        children: child.children.iter().map(convert_child).collect(),
    }
}

fn has_other_serve_reference(sessions: &[SessionSummary], target: &SessionSummary) -> bool {
    sessions.iter().any(|session| {
        session.id != target.id
            && ((target.serve_pid.is_some() && session.serve_pid == target.serve_pid)
                || (target.serve_port.is_some() && session.serve_port == target.serve_port))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_tree_busy_status_includes_selected_child_descendants() {
        let mut manager = PtyManager::default();
        let child = ChildSessionInfo {
            session_id: "child".into(),
            cwd: PathBuf::from("/tmp/repo"),
            title: "child".into(),
            status: SessionStatus::Idle,
            time_updated: None,
            has_children: true,
            children: vec![ChildSessionInfo {
                session_id: "grandchild".into(),
                cwd: PathBuf::from("/tmp/repo"),
                title: "grandchild".into(),
                status: SessionStatus::NeedsInput,
                time_updated: None,
                has_children: false,
                children: vec![],
            }],
        };
        manager.register_placeholder(
            PathBuf::from("/tmp/repo"),
            "parent".into(),
            SessionStatus::Idle,
            Some("parent".into()),
            SessionOrigin::Managed,
            None,
            None,
            None,
            None,
            None,
            None,
            true,
            vec![child],
        );

        assert!(manager.session_tree_has_busy_status("child"));
        assert!(manager.session_tree_has_busy_status("parent"));
    }

    #[test]
    fn set_session_serve_persists_port_and_pid() {
        let mut manager = PtyManager::default();
        let id = manager.register_placeholder(
            PathBuf::from("/tmp/playbook"),
            "Playbook".into(),
            SessionStatus::Idle,
            Some("session".into()),
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

        manager.set_session_serve(id, 4200, 99);

        let summary = manager.sessions().items().first().unwrap();
        assert_eq!(summary.serve_port, Some(4200));
        assert_eq!(summary.serve_pid, Some(99));
    }

    #[test]
    fn shared_serve_reference_ignores_target_and_matches_pid_or_port() {
        let target = SessionSummary {
            id: 1,
            cwd: PathBuf::new(),
            title: String::new(),
            status: SessionStatus::Idle,
            session_id: None,
            origin: SessionOrigin::Managed,
            process_pid: None,
            serve_pid: Some(10),
            serve_port: Some(4200),
            model: None,
            preview: None,
            time_updated: None,
            has_children: false,
            children: vec![],
        };
        let same_pid = SessionSummary {
            id: 2,
            serve_pid: Some(10),
            ..target.clone()
        };
        let same_port = SessionSummary {
            id: 3,
            serve_pid: None,
            serve_port: Some(4200),
            ..target.clone()
        };
        let unrelated = SessionSummary {
            id: 4,
            serve_pid: Some(11),
            serve_port: Some(4201),
            ..target.clone()
        };

        assert!(has_other_serve_reference(
            &[target.clone(), same_pid.clone()],
            &target
        ));
        assert!(has_other_serve_reference(
            &[target.clone(), same_port],
            &target
        ));
        assert!(!has_other_serve_reference(
            &[target.clone(), unrelated],
            &target
        ));
    }

    fn register_managed(
        manager: &mut PtyManager,
        session_id: Option<&str>,
        serve_port: Option<u16>,
    ) -> u64 {
        manager.register_placeholder(
            PathBuf::from("/tmp/proj"),
            "title".into(),
            SessionStatus::Working,
            session_id.map(String::from),
            SessionOrigin::Managed,
            None,
            Some(1234),
            serve_port,
            None,
            None,
            None,
            false,
            vec![],
        )
    }

    #[test]
    fn apply_session_event_signals_dirty_on_session_id_change() {
        let mut manager = PtyManager::default();
        register_managed(&mut manager, Some("ses_old"), Some(4200));

        let event = crate::ops::opencode_events::SessionCreatedEvent {
            session_id: "ses_new".into(),
            parent_id: None,
        };
        let dirty = manager.apply_session_event(4200, &event);

        assert!(
            dirty,
            "Some -> Some-different transition must signal dirty so managed-sessions.json is re-saved"
        );
        assert_eq!(
            manager.sessions().items()[0].session_id.as_deref(),
            Some("ses_new")
        );
    }

    #[test]
    fn apply_session_event_noop_returns_false() {
        let mut manager = PtyManager::default();
        register_managed(&mut manager, Some("ses_same"), Some(4200));

        let event = crate::ops::opencode_events::SessionCreatedEvent {
            session_id: "ses_same".into(),
            parent_id: None,
        };
        let dirty = manager.apply_session_event(4200, &event);

        assert!(!dirty, "no-op event must not signal dirty");
    }

    #[test]
    fn apply_session_event_none_to_some_still_returns_true() {
        let mut manager = PtyManager::default();
        register_managed(&mut manager, None, Some(4200));

        let event = crate::ops::opencode_events::SessionCreatedEvent {
            session_id: "ses_first".into(),
            parent_id: None,
        };
        let dirty = manager.apply_session_event(4200, &event);

        assert!(
            dirty,
            "None -> Some transition must continue to signal dirty"
        );
        assert_eq!(
            manager.sessions().items()[0].session_id.as_deref(),
            Some("ses_first")
        );
    }

    #[test]
    fn apply_session_event_no_matching_port_returns_false() {
        let mut manager = PtyManager::default();
        register_managed(&mut manager, Some("ses_x"), Some(4200));

        let event = crate::ops::opencode_events::SessionCreatedEvent {
            session_id: "ses_y".into(),
            parent_id: None,
        };
        let dirty = manager.apply_session_event(4201, &event);

        assert!(!dirty, "no managed entry on this port returns false");
        assert_eq!(
            manager.sessions().items()[0].session_id.as_deref(),
            Some("ses_x"),
            "unrelated entry must not be mutated"
        );
    }

    #[test]
    fn apply_session_event_child_event_returns_false() {
        let mut manager = PtyManager::default();
        register_managed(&mut manager, Some("ses_parent"), Some(4200));

        let event = crate::ops::opencode_events::SessionCreatedEvent {
            session_id: "ses_child".into(),
            parent_id: Some("ses_parent".into()),
        };
        let dirty = manager.apply_session_event(4200, &event);

        assert!(!dirty, "child session events must be ignored");
        assert_eq!(
            manager.sessions().items()[0].session_id.as_deref(),
            Some("ses_parent")
        );
    }

    #[test]
    fn apply_session_event_defensive_check_inactive_without_db() {
        // Documents the safe-fallback behavior: in a test environment (no real
        // opencode DB at the default path), the defensive check cannot
        // determine whether the old session is active, so it falls through
        // and allows the reassignment.  This preserves the original behavior
        // when the DB is unavailable.  In production with a real opencode DB,
        // the check blocks reassignments when the old session is still active.
        let mut manager = PtyManager::default();
        register_managed(&mut manager, Some("ses_old"), Some(4200));

        let event = crate::ops::opencode_events::SessionCreatedEvent {
            session_id: "ses_new".into(),
            parent_id: None,
        };
        let dirty = manager.apply_session_event(4200, &event);

        assert!(dirty, "without DB access, reassignment proceeds");
        assert_eq!(
            manager.sessions().items()[0].session_id.as_deref(),
            Some("ses_new")
        );
    }
}
