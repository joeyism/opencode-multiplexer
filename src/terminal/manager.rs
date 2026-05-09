use std::{collections::HashMap, path::PathBuf};

use crate::{
    app::sessions::{SessionList, SessionOrigin, SessionStatus, SessionSummary},
    data::poller::{ChildSessionInfo, DiscoverySource, PollSnapshot},
    registry::is_pid_alive,
    ui::sidebar::{ChildSidebarEntry, SidebarEntry},
};

use super::pty::PtySession;

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
        use crate::ops::opencode::{
            fetch_serve_session_ids, find_available_port, spawn_serve_daemon,
            wait_for_new_session_id, wait_for_serve_ready,
        };
        use crate::registry::{register_serve_process, update_serve_registry_tui_pid};

        // Spawn serve daemon as persistent backend
        let port = find_available_port(4200);
        let serve_pid = spawn_serve_daemon(&cwd, port)?;
        register_serve_process(port, serve_pid, &cwd)?;

        // Wait for serve to be ready
        if !wait_for_serve_ready(port, 10) {
            anyhow::bail!("opencode serve did not start within 10s on port {port}");
        }

        // Snapshot session IDs on this serve BEFORE spawning TUI
        let before_ids = fetch_serve_session_ids(port).unwrap_or_default();

        // Spawn TUI client as a disposable PTY (always fresh, no -s flag)
        let pty = PtySession::spawn_managed(&cwd, rows, cols)?;
        let process_pid = pty.process_id();
        if let Some(pid) = process_pid {
            let _ = update_serve_registry_tui_pid(port, pid);
        }

        // Resolve session_id via before/after diff on the home serve port.
        // The TUI creates a new session on this serve — detect it by diffing.
        let session_id = wait_for_new_session_id(port, &before_ids, 10);

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let id = self.sessions.push(
            cwd,
            title,
            SessionStatus::Working,
            session_id,
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
                let pty = PtySession::spawn_replica(&summary.cwd, session_id, rows, cols)?;
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
        let pty = PtySession::spawn_replica(&cwd, &session_id, rows, cols)?;
        let process_pid = pty.process_id();

        if let Some(existing_id) = self.sessions.find_by_session_id(&session_id) {
            if let Some(summary) = self.sessions.get_mut(existing_id) {
                summary.origin = SessionOrigin::Managed;
                summary.title = title;
                summary.status = status;
                summary.process_pid = process_pid;
                summary.time_updated = time_updated;
                // DO NOT overwrite serve_pid / serve_port here, they might be valid.
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
            None,
            None,
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
        let keep_placeholder = session
            .as_ref()
            .is_some_and(|s| s.origin == SessionOrigin::Discovered);

        if let Some(Some(pty)) = self.ptys.get_mut(&id) {
            let _ = pty.kill();
        }

        if let Some(session) = session
            && session.origin == SessionOrigin::Managed
            && let Some(serve_pid) = session.serve_pid
        {
            crate::registry::kill_pid(serve_pid);
        }

        if keep_placeholder {
            self.sessions.cancel_kill();
            if let Some(pty) = self.ptys.get_mut(&id) {
                *pty = None;
            }
            return Ok(Some(id));
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

    pub fn apply_poll_snapshot(&mut self, snapshot: PollSnapshot) -> bool {
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum MatchKind {
            SessionId,
            ProcessPid,
            ServePort,
            ManagedCwd,
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

        for discovered in snapshot.sessions {
            // Try to find an existing session to update, in order of reliability:
            // 1. By session_id (exact DB identity — most reliable)
            // 2. By process PID (matches process_pid or serve_pid)
            // 3. By serve port (only if not already matched — can be wrong when
            //    multiple serves share the same DB)
            // 4. By cwd for managed sessions with unresolved session_id
            let matched = self
                .sessions
                .find_by_session_id(&discovered.session_id)
                .map(|id| (id, MatchKind::SessionId))
                .or_else(|| {
                    discovered.process_pid.and_then(|pid| {
                        self.sessions
                            .find_by_process_pid(pid)
                            .map(|id| (id, MatchKind::ProcessPid))
                    })
                })
                .or_else(|| {
                    discovered.serve_port.and_then(|port| {
                        self.sessions
                            .find_by_serve_port(port)
                            .map(|id| (id, MatchKind::ServePort))
                    })
                })
                .or_else(|| {
                    // Match managed sessions that don't have a session_id yet by cwd.
                    self.sessions
                        .items()
                        .iter()
                        .find(|s| {
                            s.origin == SessionOrigin::Managed
                                && s.session_id.is_none()
                                && s.cwd == discovered.cwd
                        })
                        .map(|s| (s.id, MatchKind::ManagedCwd))
                });

            if let Some((id, match_kind)) = matched {
                if let Some(summary) = self.sessions.get_mut(id) {
                    // Guard: do not let a heuristic TUI discovery claim a managed
                    // session that has a serve backend. The heuristic
                    // (get_most_recent_session) can guess the wrong session.
                    let is_heuristic = matches!(discovered.source, DiscoverySource::TuiHeuristic);
                    let has_serve_backend =
                        summary.origin == SessionOrigin::Managed && summary.serve_port.is_some();

                    // Allow the heuristic if the session doesn't have an ID yet, to adopt the guessed ID
                    if is_heuristic
                        && has_serve_backend
                        && summary.session_id.is_some()
                        && !matches!(match_kind, MatchKind::SessionId)
                    {
                        continue;
                    }

                    summary.session_id = Some(discovered.session_id.clone());
                    summary.cwd = discovered.cwd.clone();
                    summary.title = discovered.title.clone();
                    summary.status = discovered.status;
                    if summary.serve_pid != discovered.process_pid {
                        summary.process_pid = discovered.process_pid;
                    }
                    summary.model = discovered.model.clone();
                    summary.preview = discovered.preview.clone();
                    summary.time_updated = discovered.time_updated;
                    summary.has_children = discovered.has_children;
                    summary.children = discovered.children.clone();
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

            self.register_placeholder(
                discovered.cwd,
                discovered.title,
                discovered.status,
                Some(discovered.session_id),
                SessionOrigin::Discovered,
                discovered.process_pid,
                None,
                discovered.serve_port,
                discovered.model,
                discovered.preview,
                discovered.time_updated,
                discovered.has_children,
                discovered.children,
            );
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
                    && session.status == SessionStatus::Working
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
                        dead_serve && no_pty && (not_in_snapshot || replaced_in_cwd)
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
                .is_some_and(|s| {
                    s.origin == SessionOrigin::Managed && s.session_id.is_some()
                })
        });

        self.sessions
            .retain(|session| !stale_ids.contains(&session.id));
        for id in stale_ids {
            self.ptys.remove(&id);
        }

        registry_dirty
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
        let pty = PtySession::spawn_replica(&summary.cwd, session_id, rows, cols)?;
        if let Some(slot) = self.ptys.get_mut(&active_id) {
            *slot = Some(pty);
        }

        Ok(true)
    }

    pub fn shutdown_local_ptys(&mut self) {
        // Only kill PTY clients (TUI viewers), NOT serve daemons.
        // Serve daemons persist in the background for session continuity.
        for pty in self.ptys.values_mut().filter_map(Option::as_mut) {
            let _ = pty.kill();
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
