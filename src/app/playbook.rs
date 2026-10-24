use std::{collections::HashMap, path::PathBuf, sync::Arc};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::playbook::{
    engine::{PlaybookEvent, PlaybookHandle, PromptClient, RunContext, Shell},
    schema::{Playbook, Step},
};

pub struct SessionTarget {
    pub session_id: Option<String>,
    pub serve_port: Option<u16>,
    pub cwd: PathBuf,
}

pub fn should_refresh_pty_after_playbook(
    previous_serve_port: Option<u16>,
    previous_session_id: Option<&str>,
    session_entry_id: u64,
    active_id: Option<u64>,
) -> bool {
    (previous_serve_port.is_none() || previous_session_id.is_none())
        && active_id == Some(session_entry_id)
}

pub struct PlaybookRun {
    pub name: String,
    pub handle: PlaybookHandle,
    client: Arc<dyn PromptClient + Send + Sync>,
    session_id: String,
    port: u16,
}

impl PlaybookRun {
    pub fn session_id(&self) -> &str {
        &self.session_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaybookPoll {
    Quiet,
    Banner(String),
    Finished(String),
}

pub fn poll_playbook(run: &PlaybookRun) -> PlaybookPoll {
    let mut latest = PlaybookPoll::Quiet;
    while let Ok(event) = run.handle.rx.try_recv() {
        latest = match event {
            PlaybookEvent::Step { label } => {
                PlaybookPoll::Banner(format!("playbook: {}  {}", run.name, label))
            }
            PlaybookEvent::Done => PlaybookPoll::Finished(format!("playbook done: {}", run.name)),
            PlaybookEvent::Failed(error) => {
                PlaybookPoll::Finished(format!("playbook failed: {error}"))
            }
            PlaybookEvent::Aborted => {
                PlaybookPoll::Finished(format!("playbook aborted: {}", run.name))
            }
        };
    }
    latest
}

pub fn abort_playbook(run: &PlaybookRun) {
    run.handle.abort();
    let client = run.client.clone();
    let session_id = run.session_id.clone();
    let port = run.port;
    std::thread::spawn(move || {
        let _ = client.abort(&session_id, port);
    });
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaybookUiAction {
    Consumed,
    Close,
    OpenForm(ParamFormState),
    Start {
        playbook_name: String,
        steps: Vec<Step>,
        vars: HashMap<String, String>,
    },
}

pub fn start_playbook(
    name: String,
    steps: Vec<Step>,
    vars: HashMap<String, String>,
    target: &SessionTarget,
    http: Arc<dyn PromptClient + Send + Sync>,
    shell: Arc<dyn Shell + Send + Sync>,
) -> Result<PlaybookRun, String> {
    let session_id = target
        .session_id
        .clone()
        .ok_or_else(|| "playbooks need a session ID".to_string())?;
    let port = target
        .serve_port
        .ok_or_else(|| "no opencode serve found for this session directory".to_string())?;
    let playbook = Playbook {
        name: name.clone(),
        description: None,
        params: Vec::new(),
        steps,
    };
    let handle = crate::playbook::engine::spawn_playbook(
        playbook,
        RunContext {
            session_id: session_id.clone(),
            port,
            cwd: target.cwd.clone(),
            vars,
        },
        http.clone(),
        shell,
    );
    Ok(PlaybookRun {
        name,
        handle,
        client: http,
        session_id,
        port,
    })
}

#[derive(Debug)]
pub struct PlaybookPickerState {
    pub query: String,
    pub selected: usize,
    playbooks: Vec<Playbook>,
}

impl PlaybookPickerState {
    pub fn from_playbooks(playbooks: Vec<Playbook>) -> Self {
        Self {
            query: String::new(),
            selected: 0,
            playbooks,
        }
    }

    pub fn insert_char(&mut self, c: char) {
        self.query.push(c);
        self.selected = 0;
    }

    pub fn backspace(&mut self) {
        self.query.pop();
        self.selected = 0;
    }

    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn move_down(&mut self) {
        let max = self.visible_indices().len().saturating_sub(1);
        self.selected = (self.selected + 1).min(max);
    }

    fn visible_indices(&self) -> Vec<usize> {
        let q = self.query.to_lowercase();
        self.playbooks
            .iter()
            .enumerate()
            .filter(|(_, pb)| {
                if q.is_empty() {
                    return true;
                }
                pb.name.to_lowercase().contains(&q)
                    || pb
                        .description
                        .as_deref()
                        .map(|d| d.to_lowercase().contains(&q))
                        .unwrap_or(false)
            })
            .map(|(i, _)| i)
            .collect()
    }

    pub fn visible(&self) -> Vec<(usize, &Playbook)> {
        self.visible_indices()
            .into_iter()
            .map(|i| (i, &self.playbooks[i]))
            .collect()
    }

    pub fn selected_playbook(&self) -> Option<&Playbook> {
        let idx = self.visible_indices().get(self.selected).copied()?;
        self.playbooks.get(idx)
    }

    pub fn total_count(&self) -> usize {
        self.playbooks.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormField {
    pub name: String,
    pub prompt: String,
    pub required: bool,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamFormState {
    pub fields: Vec<FormField>,
    pub focused: usize,
    playbook_name: String,
    playbook_description: Option<String>,
    steps: Vec<Step>,
}

impl ParamFormState {
    pub fn from_playbook(pb: &Playbook) -> Self {
        let fields = pb
            .params
            .iter()
            .map(|(name, p)| FormField {
                name: name.clone(),
                prompt: p.prompt.clone(),
                required: p.required,
                value: p.default.clone().unwrap_or_default(),
            })
            .collect();
        Self {
            fields,
            focused: 0,
            playbook_name: pb.name.clone(),
            playbook_description: pb.description.clone(),
            steps: pb.steps.clone(),
        }
    }

    pub fn needs_params(pb: &Playbook) -> bool {
        !pb.params.is_empty()
    }

    pub fn insert_char(&mut self, c: char) {
        if let Some(f) = self.fields.get_mut(self.focused) {
            f.value.push(c);
        }
    }

    pub fn insert_str(&mut self, text: &str) {
        let cleaned = text.trim();
        if cleaned.is_empty() {
            return;
        }
        if let Some(f) = self.fields.get_mut(self.focused) {
            f.value.push_str(cleaned);
        }
    }

    pub fn backspace(&mut self) {
        if let Some(f) = self.fields.get_mut(self.focused) {
            f.value.pop();
        }
    }

    pub fn next_field(&mut self) {
        if self.focused + 1 < self.fields.len() {
            self.focused += 1;
        }
    }

    pub fn prev_field(&mut self) {
        self.focused = self.focused.saturating_sub(1);
    }

    pub fn submit(&self) -> Option<HashMap<String, String>> {
        let mut vars = HashMap::new();
        for f in &self.fields {
            if f.required && f.value.is_empty() {
                return None;
            }
            vars.insert(f.name.clone(), f.value.clone());
        }
        Some(vars)
    }

    pub fn playbook_name(&self) -> &str {
        &self.playbook_name
    }

    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    pub fn description(&self) -> Option<&str> {
        self.playbook_description.as_deref()
    }
}

pub fn handle_picker_key(key: KeyEvent, picker: &mut PlaybookPickerState) -> PlaybookUiAction {
    match key.code {
        KeyCode::Esc => PlaybookUiAction::Close,
        KeyCode::Enter => {
            let Some(pb) = picker.selected_playbook() else {
                return PlaybookUiAction::Consumed;
            };
            if ParamFormState::needs_params(pb) {
                PlaybookUiAction::OpenForm(ParamFormState::from_playbook(pb))
            } else {
                PlaybookUiAction::Start {
                    playbook_name: pb.name.clone(),
                    steps: pb.steps.clone(),
                    vars: HashMap::new(),
                }
            }
        }
        KeyCode::Up => {
            picker.move_up();
            PlaybookUiAction::Consumed
        }
        KeyCode::Down => {
            picker.move_down();
            PlaybookUiAction::Consumed
        }
        KeyCode::Backspace => {
            picker.backspace();
            PlaybookUiAction::Consumed
        }
        KeyCode::Char(c) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            picker.insert_char(c);
            PlaybookUiAction::Consumed
        }
        _ => PlaybookUiAction::Consumed,
    }
}

pub fn handle_form_key(key: KeyEvent, form: &mut ParamFormState) -> PlaybookUiAction {
    match key.code {
        KeyCode::Esc => PlaybookUiAction::Close,
        KeyCode::Enter => match form.submit() {
            Some(vars) => PlaybookUiAction::Start {
                playbook_name: form.playbook_name().to_string(),
                steps: form.steps().to_vec(),
                vars,
            },
            None => PlaybookUiAction::Consumed,
        },
        KeyCode::Tab => {
            form.next_field();
            PlaybookUiAction::Consumed
        }
        KeyCode::BackTab => {
            form.prev_field();
            PlaybookUiAction::Consumed
        }
        KeyCode::Backspace => {
            form.backspace();
            PlaybookUiAction::Consumed
        }
        KeyCode::Char(c) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            form.insert_char(c);
            PlaybookUiAction::Consumed
        }
        _ => PlaybookUiAction::Consumed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playbook::engine::{EngineError, SendRequest};
    use crate::playbook::schema::parse_playbook;
    use std::{path::Path, sync::Mutex};

    fn fake(name: &str, desc: &str) -> Playbook {
        parse_playbook(&format!(
            "name: {name}\ndescription: {desc}\nsteps:\n  - send:\n      agent: chat\n      message: hi\n"
        ))
        .unwrap()
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn refreshes_active_pty_when_playbook_adds_serve() {
        assert!(should_refresh_pty_after_playbook(
            None,
            Some("s"),
            7,
            Some(7)
        ));
        assert!(should_refresh_pty_after_playbook(
            Some(4200),
            None,
            7,
            Some(7)
        ));
        assert!(!should_refresh_pty_after_playbook(
            Some(4200),
            Some("s"),
            7,
            Some(7)
        ));
        assert!(!should_refresh_pty_after_playbook(
            None,
            Some("s"),
            7,
            Some(8)
        ));
        assert!(!should_refresh_pty_after_playbook(None, Some("s"), 7, None));
    }

    #[derive(Default)]
    struct RecordingHttp {
        requests: Mutex<Vec<SendRequest>>,
    }

    impl PromptClient for RecordingHttp {
        fn send(&self, request: SendRequest) -> Result<(), EngineError> {
            self.requests.lock().unwrap().push(request);
            Ok(())
        }

        fn abort(&self, _session_id: &str, _port: u16) -> Result<(), EngineError> {
            Ok(())
        }
    }

    struct NoopShell;

    impl Shell for NoopShell {
        fn run(
            &self,
            _cwd: &Path,
            _env: &HashMap<String, String>,
            _script: &str,
        ) -> crate::playbook::engine::ShellResult {
            crate::playbook::engine::ShellResult {
                exit_code: 0,
                stdout: String::new(),
                stderr: String::new(),
            }
        }
    }

    #[test]
    fn start_playbook_without_serve_port_does_not_send() {
        let http = Arc::new(RecordingHttp::default());
        let result = start_playbook(
            "test".into(),
            vec![],
            HashMap::new(),
            &SessionTarget {
                session_id: Some("sess_1".into()),
                serve_port: None,
                cwd: PathBuf::from("/tmp"),
            },
            http.clone(),
            Arc::new(NoopShell),
        );
        assert!(result.is_err());
        assert!(http.requests.lock().unwrap().is_empty());
    }

    #[test]
    fn start_playbook_sends_interpolated_message_to_prompt_client() {
        let http = Arc::new(RecordingHttp::default());
        let playbook = parse_playbook(
            r#"
name: test
steps:
  - send:
      agent: chat
      message: review ${URL}
"#,
        )
        .unwrap();
        let mut vars = HashMap::new();
        vars.insert("URL".into(), "https://github.com/org/repo/pull/1".into());
        let run = start_playbook(
            playbook.name,
            playbook.steps,
            vars,
            &SessionTarget {
                session_id: Some("sess_1".into()),
                serve_port: Some(4200),
                cwd: PathBuf::from("/tmp"),
            },
            http.clone(),
            Arc::new(NoopShell),
        )
        .unwrap();
        let step = run
            .handle
            .rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        assert!(matches!(step, PlaybookEvent::Step { .. }));
        let event = run
            .handle
            .rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        assert!(matches!(
            event,
            crate::playbook::engine::PlaybookEvent::Done
        ));
        let requests = http.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].agent, "chat");
        assert_eq!(
            requests[0].message,
            "review https://github.com/org/repo/pull/1"
        );
    }

    #[test]
    fn picker_filters_by_name_and_description() {
        let mut p = PlaybookPickerState::from_playbooks(vec![
            fake("pr-comments", "Review PR comments"),
            fake("commit", "Commit and push"),
        ]);
        p.insert_char('p');
        p.insert_char('r');
        let visible = p.visible();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].1.name, "pr-comments");
    }

    #[test]
    fn picker_enter_returns_selected() {
        let mut p = PlaybookPickerState::from_playbooks(vec![fake("a", "A"), fake("b", "B")]);
        p.move_down();
        assert_eq!(p.selected_playbook().unwrap().name, "b");
    }

    #[test]
    fn picker_enter_without_params_starts() {
        let mut p = PlaybookPickerState::from_playbooks(vec![fake("a", "A")]);
        let action = handle_picker_key(key(KeyCode::Enter), &mut p);
        match action {
            PlaybookUiAction::Start {
                playbook_name,
                steps: _,
                vars,
            } => {
                assert_eq!(playbook_name, "a");
                assert!(vars.is_empty());
            }
            other => panic!("expected Start, got {other:?}"),
        }
    }

    #[test]
    fn picker_enter_with_params_opens_form() {
        let pb = parse_playbook(
            r#"
name: t
params:
  X:
    prompt: x
    required: true
steps:
  - send:
      agent: chat
      message: ${X}
"#,
        )
        .unwrap();
        let mut p = PlaybookPickerState::from_playbooks(vec![pb]);
        let action = handle_picker_key(key(KeyCode::Enter), &mut p);
        assert!(matches!(action, PlaybookUiAction::OpenForm(_)));
    }

    #[test]
    fn form_esc_closes() {
        let pb = parse_playbook(
            "name: t\nparams:\n  X:\n    prompt: x\n    required: true\nsteps:\n  - send:\n      agent: chat\n      message: ${X}\n",
        )
        .unwrap();
        let mut form = ParamFormState::from_playbook(&pb);
        let action = handle_form_key(key(KeyCode::Esc), &mut form);
        assert_eq!(action, PlaybookUiAction::Close);
    }

    #[test]
    fn enter_blocked_while_required_empty() {
        let pb = parse_playbook(
            r#"
name: t
params:
  GITHUB_PR_URL:
    prompt: GitHub PR URL
    required: true
steps:
  - send:
      agent: chat
      message: ${GITHUB_PR_URL}
"#,
        )
        .unwrap();
        let mut form = ParamFormState::from_playbook(&pb);
        assert!(form.submit().is_none());
        form.insert_char('h');
        let vars = form.submit().unwrap();
        assert_eq!(vars.get("GITHUB_PR_URL").unwrap(), "h");
    }

    #[test]
    fn optional_param_uses_default() {
        let pb = parse_playbook(
            r#"
name: t
params:
  BRANCH:
    prompt: branch
    required: false
    default: main
steps:
  - send:
      agent: chat
      message: ${BRANCH}
"#,
        )
        .unwrap();
        let form = ParamFormState::from_playbook(&pb);
        let vars = form.submit().unwrap();
        assert_eq!(vars.get("BRANCH").unwrap(), "main");
    }

    #[test]
    fn tab_moves_fields_and_edits_focus() {
        let pb = parse_playbook(
            r#"
name: t
params:
  A:
    prompt: first
    required: true
  B:
    prompt: second
    required: true
steps:
  - send:
      agent: chat
      message: ${A} ${B}
"#,
        )
        .unwrap();
        let mut form = ParamFormState::from_playbook(&pb);
        form.insert_char('x');
        form.next_field();
        form.insert_char('y');
        let vars = form.submit().unwrap();
        assert_eq!(vars.get("A").unwrap(), "x");
        assert_eq!(vars.get("B").unwrap(), "y");
    }

    #[test]
    fn paste_inserts_into_focused_field() {
        let pb = parse_playbook(
            r#"
name: t
params:
  GITHUB_PR_URL:
    prompt: GitHub PR URL
    required: true
steps:
  - send:
      agent: chat
      message: ${GITHUB_PR_URL}
"#,
        )
        .unwrap();
        let mut form = ParamFormState::from_playbook(&pb);
        form.insert_str("https://github.com/org/repo/pull/1\n");
        let vars = form.submit().unwrap();
        assert_eq!(
            vars.get("GITHUB_PR_URL").unwrap(),
            "https://github.com/org/repo/pull/1"
        );
    }

    #[test]
    fn form_enter_with_values_starts() {
        let pb = parse_playbook(
            "name: t\nparams:\n  X:\n    prompt: x\n    required: true\nsteps:\n  - send:\n      agent: chat\n      message: ${X}\n",
        )
        .unwrap();
        let mut form = ParamFormState::from_playbook(&pb);
        form.insert_char('v');
        let action = handle_form_key(key(KeyCode::Enter), &mut form);
        match action {
            PlaybookUiAction::Start {
                playbook_name,
                steps: _,
                vars,
            } => {
                assert_eq!(playbook_name, "t");
                assert_eq!(vars.get("X").unwrap(), "v");
            }
            other => panic!("expected Start, got {other:?}"),
        }
    }
}
