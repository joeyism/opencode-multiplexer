use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    Working,
    NeedsInput,
    Idle,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionOrigin {
    Managed,
    Discovered,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    pub id: u64,
    pub cwd: PathBuf,
    pub title: String,
    pub status: SessionStatus,
    pub session_id: Option<String>,
    pub origin: SessionOrigin,
    pub process_pid: Option<u32>,
    pub serve_pid: Option<u32>,
    pub serve_port: Option<u16>,
    pub model: Option<String>,
    pub preview: Option<String>,
    pub time_updated: Option<i64>,
    pub has_children: bool,
    pub children: Vec<crate::data::poller::ChildSessionInfo>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SessionList {
    sessions: Vec<SessionSummary>,
    selected: usize,
    active_id: Option<u64>,
    pending_kill: Option<u64>,
    next_id: u64,
}

impl SessionList {
    #[allow(clippy::too_many_arguments)]
    pub fn push(
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
        let id = self.next_id;
        self.next_id += 1;
        self.sessions.push(SessionSummary {
            id,
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
        });
        if self.active_id.is_none() {
            self.active_id = Some(id);
            self.selected = self.sessions.len().saturating_sub(1);
        }
        id
    }

    pub fn find_by_session_id(&self, session_id: &str) -> Option<u64> {
        self.sessions
            .iter()
            .find(|session| session.session_id.as_deref() == Some(session_id))
            .map(|session| session.id)
    }

    pub fn find_by_process_pid(&self, process_pid: u32) -> Option<u64> {
        self.sessions
            .iter()
            .find(|session| {
                session.process_pid == Some(process_pid) || session.serve_pid == Some(process_pid)
            })
            .map(|session| session.id)
    }

    pub fn find_by_serve_port(&self, serve_port: u16) -> Option<u64> {
        self.sessions
            .iter()
            .find(|session| session.serve_port == Some(serve_port))
            .map(|session| session.id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut SessionSummary> {
        self.sessions.iter_mut().find(|session| session.id == id)
    }

    pub fn update_status(&mut self, id: u64, status: SessionStatus) {
        if let Some(session) = self.get_mut(id) {
            session.status = status;
        }
    }

    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    pub fn items(&self) -> &[SessionSummary] {
        &self.sessions
    }

    pub fn items_mut(&mut self) -> &mut [SessionSummary] {
        &mut self.sessions
    }

    pub fn selected_index(&self) -> usize {
        self.selected.min(self.sessions.len().saturating_sub(1))
    }

    pub fn selected_id(&self) -> Option<u64> {
        self.sessions
            .get(self.selected_index())
            .map(|session| session.id)
    }

    pub fn active_id(&self) -> Option<u64> {
        self.active_id
    }

    pub fn active(&self) -> Option<&SessionSummary> {
        let active_id = self.active_id?;
        self.sessions.iter().find(|session| session.id == active_id)
    }

    pub fn active_mut(&mut self) -> Option<&mut SessionSummary> {
        let active_id = self.active_id?;
        self.sessions
            .iter_mut()
            .find(|session| session.id == active_id)
    }

    pub fn select_next(&mut self) {
        if !self.sessions.is_empty() {
            self.selected = (self.selected + 1).min(self.sessions.len() - 1);
        }
    }

    pub fn select_prev(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn select_id(&mut self, id: u64) {
        if let Some(index) = self.sessions.iter().position(|session| session.id == id) {
            self.selected = index;
        }
    }

    pub fn activate_selected(&mut self) {
        self.active_id = self.selected_id();
    }

    pub fn select_last(&mut self) {
        if !self.sessions.is_empty() {
            self.selected = self.sessions.len() - 1;
        }
    }

    pub fn request_kill_selected(&mut self) {
        self.pending_kill = self.selected_id();
    }

    pub fn pending_kill(&self) -> Option<u64> {
        self.pending_kill
    }

    pub fn cancel_kill(&mut self) {
        self.pending_kill = None;
    }

    pub fn confirm_kill(&mut self) -> Option<u64> {
        let id = self.pending_kill.take()?;
        self.remove(id);
        Some(id)
    }

    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&SessionSummary) -> bool,
    {
        let active_id = self.active_id;
        let selected_id = self.selected_id();
        self.sessions.retain(|session| f(session));

        if self.sessions.is_empty() {
            self.selected = 0;
            self.active_id = None;
            self.pending_kill = None;
            return;
        }

        self.active_id =
            active_id.filter(|id| self.sessions.iter().any(|session| session.id == *id));
        if self.active_id.is_none() {
            self.active_id = Some(self.sessions[0].id);
        }

        self.selected = selected_id
            .and_then(|id| self.sessions.iter().position(|session| session.id == id))
            .unwrap_or(0);
        self.pending_kill = self
            .pending_kill
            .filter(|id| self.sessions.iter().any(|session| session.id == *id));
    }

    pub fn remove(&mut self, id: u64) {
        if let Some(index) = self.sessions.iter().position(|session| session.id == id) {
            self.sessions.remove(index);
            if self.sessions.is_empty() {
                self.selected = 0;
                self.active_id = None;
                return;
            }

            if self.selected >= self.sessions.len() {
                self.selected = self.sessions.len() - 1;
            }

            if self.active_id == Some(id) {
                self.active_id = self.sessions.get(self.selected).map(|session| session.id);
            }
        }
    }
}
