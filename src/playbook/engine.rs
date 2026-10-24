use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::Sender;

use super::interp::interpolate;
use super::schema::{Playbook, Step};

pub trait PromptClient {
    fn send(&self, req: SendRequest) -> Result<(), EngineError>;
    fn abort(&self, session_id: &str, port: u16) -> Result<(), EngineError>;
    /// Inspect the completed turn. `Ok` lets the playbook continue;
    /// `Err(Aborted)` when the user stopped it, any other `Err` fails the playbook.
    fn after_send(&self, _session_id: &str) -> Result<(), EngineError> {
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct SendRequest {
    pub session_id: String,
    pub port: u16,
    pub agent: String,
    pub message: String,
    pub timeout: std::time::Duration,
}

pub trait Shell {
    fn run(&self, cwd: &Path, env: &HashMap<String, String>, script: &str) -> ShellResult;
}

#[derive(Debug, Clone)]
pub struct ShellResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug)]
pub struct RunContext {
    pub session_id: String,
    pub port: u16,
    pub cwd: PathBuf,
    pub vars: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub enum EngineError {
    UnknownVar(String),
    SendFailed(String),
    TimedOut,
    Aborted,
    RunFailed { exit_code: i32, stderr: String },
    LoopLimit { max: u32 },
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineError::UnknownVar(v) => write!(f, "unknown variable: {v}"),
            EngineError::SendFailed(e) => write!(f, "send failed: {e}"),
            EngineError::TimedOut => write!(f, "timed out"),
            EngineError::Aborted => write!(f, "aborted"),
            EngineError::RunFailed { exit_code, stderr } => {
                write!(f, "run failed (exit {exit_code}): {stderr}")
            }
            EngineError::LoopLimit { max } => write!(f, "loop limit (max {max}) reached"),
        }
    }
}

impl std::error::Error for EngineError {}

pub fn run_playbook(
    pb: &Playbook,
    ctx: &mut RunContext,
    http: &dyn PromptClient,
    shell: &dyn Shell,
) -> Result<(), EngineError> {
    run_steps(&pb.steps, ctx, http, shell, &AtomicBool::new(false), None)
}

pub fn run_playbook_with_abort(
    pb: &Playbook,
    ctx: &mut RunContext,
    http: &dyn PromptClient,
    shell: &dyn Shell,
    abort: &AtomicBool,
) -> Result<(), EngineError> {
    run_steps(&pb.steps, ctx, http, shell, abort, None)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaybookEvent {
    Step { label: String },
    Done,
    Failed(String),
    Aborted,
}

pub struct PlaybookHandle {
    pub rx: Receiver<PlaybookEvent>,
    abort: Arc<AtomicBool>,
}

impl PlaybookHandle {
    pub fn abort(&self) {
        self.abort.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

pub fn spawn_playbook(
    pb: Playbook,
    mut ctx: RunContext,
    http: Arc<dyn PromptClient + Send + Sync>,
    shell: Arc<dyn Shell + Send + Sync>,
) -> PlaybookHandle {
    let (tx, rx) = std::sync::mpsc::channel();
    let abort = Arc::new(AtomicBool::new(false));
    let abort_clone = abort.clone();

    std::thread::spawn(move || {
        let result = run_steps(
            &pb.steps,
            &mut ctx,
            http.as_ref(),
            shell.as_ref(),
            &abort_clone,
            Some(&tx),
        );
        let event = match result {
            Ok(()) => PlaybookEvent::Done,
            Err(EngineError::Aborted) => PlaybookEvent::Aborted,
            Err(e) => PlaybookEvent::Failed(e.to_string()),
        };
        let _ = tx.send(event);
    });

    PlaybookHandle { rx, abort }
}

pub fn spawn_playbook_with_abort(
    pb: Playbook,
    mut ctx: RunContext,
    http: Arc<dyn PromptClient + Send + Sync>,
    shell: Arc<dyn Shell + Send + Sync>,
    abort: Arc<AtomicBool>,
) -> PlaybookHandle {
    let (tx, rx) = std::sync::mpsc::channel();
    let abort_clone = abort.clone();

    std::thread::spawn(move || {
        let result = run_steps(
            &pb.steps,
            &mut ctx,
            http.as_ref(),
            shell.as_ref(),
            &abort_clone,
            Some(&tx),
        );
        let event = match result {
            Ok(()) => PlaybookEvent::Done,
            Err(EngineError::Aborted) => PlaybookEvent::Aborted,
            Err(e) => PlaybookEvent::Failed(e.to_string()),
        };
        let _ = tx.send(event);
    });

    PlaybookHandle { rx, abort }
}

pub fn gate_serve_port(port: Option<u16>) -> Result<u16, &'static str> {
    port.ok_or("playbooks need a managed session with serve")
}

pub fn playbook_banner(name: &str, step: Option<&str>) -> String {
    match step {
        Some(s) => format!("playbook: {name}  {s}"),
        None => format!("playbook: {name}"),
    }
}

fn run_steps(
    steps: &[Step],
    ctx: &mut RunContext,
    http: &dyn PromptClient,
    shell: &dyn Shell,
    abort: &AtomicBool,
    events: Option<&Sender<PlaybookEvent>>,
) -> Result<(), EngineError> {
    use std::sync::atomic::Ordering;
    for step in steps {
        if abort.load(Ordering::SeqCst) {
            return Err(EngineError::Aborted);
        }
        match step {
            Step::Send { send } => {
                if let Some(events) = events {
                    let _ = events.send(PlaybookEvent::Step {
                        label: format!("send:{}", send.agent),
                    });
                }
                let message = interpolate(&send.message, &ctx.vars)
                    .map_err(|e| EngineError::UnknownVar(e.to_string()))?;
                let timeout = send
                    .timeout_secs
                    .map(std::time::Duration::from_secs)
                    .unwrap_or(std::time::Duration::from_secs(1800));
                let result = http.send(SendRequest {
                    session_id: ctx.session_id.clone(),
                    port: ctx.port,
                    agent: send.agent.clone(),
                    message,
                    timeout,
                });
                if let Err(EngineError::TimedOut) = result {
                    let _ = http.abort(&ctx.session_id, ctx.port);
                    return Err(EngineError::TimedOut);
                }
                result?;
                http.after_send(&ctx.session_id)?;
            }
            Step::Run { run, register } => {
                if let Some(events) = events {
                    let _ = events.send(PlaybookEvent::Step {
                        label: "run".into(),
                    });
                }
                let env = build_env(ctx);
                let result = shell.run(&ctx.cwd, &env, run);
                if result.exit_code != 0 {
                    return Err(EngineError::RunFailed {
                        exit_code: result.exit_code,
                        stderr: result.stderr,
                    });
                }
                if let Some(name) = register {
                    ctx.vars
                        .insert(name.clone(), result.stdout.trim().to_string());
                }
            }
            Step::If {
                cond,
                then,
                else_steps,
            } => {
                let env = build_env(ctx);
                let result = shell.run(&ctx.cwd, &env, cond);
                let branch = if result.exit_code == 0 {
                    then
                } else {
                    else_steps
                };
                run_steps(branch, ctx, http, shell, abort, events)?;
            }
            Step::Until {
                until,
                max,
                steps: body,
            } => {
                let mut i = 0u32;
                loop {
                    let env = build_env(ctx);
                    let result = shell.run(&ctx.cwd, &env, until);
                    if result.exit_code == 0 {
                        break;
                    }
                    if i >= *max {
                        return Err(EngineError::LoopLimit { max: *max });
                    }
                    run_steps(body, ctx, http, shell, abort, events)?;
                    i += 1;
                }
            }
        }
    }
    Ok(())
}

fn build_env(ctx: &RunContext) -> HashMap<String, String> {
    let mut env = ctx.vars.clone();
    env.insert("SESSION_ID".into(), ctx.session_id.clone());
    env.insert("SESSION_CWD".into(), ctx.cwd.display().to_string());
    env
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeHttp {
        recorded: Mutex<Vec<SendRequest>>,
        after_send_error: Option<EngineError>,
    }

    impl PromptClient for FakeHttp {
        fn send(&self, req: SendRequest) -> Result<(), EngineError> {
            self.recorded.lock().unwrap().push(req);
            Ok(())
        }
        fn abort(&self, _sid: &str, _port: u16) -> Result<(), EngineError> {
            Ok(())
        }
        fn after_send(&self, _session_id: &str) -> Result<(), EngineError> {
            self.after_send_error.clone().map_or(Ok(()), Err)
        }
    }

    fn parse(yaml: &str) -> Playbook {
        super::super::schema::parse_playbook(yaml).unwrap()
    }

    fn ctx(vars: HashMap<String, String>) -> RunContext {
        RunContext {
            session_id: "sess_1".into(),
            port: 4200,
            cwd: PathBuf::from("/tmp"),
            vars,
        }
    }

    struct PanicShell;
    impl Shell for PanicShell {
        fn run(&self, _: &Path, _: &HashMap<String, String>, _: &str) -> ShellResult {
            panic!("shell should not be called");
        }
    }

    #[test]
    fn engine_sends_interpolated_message_to_named_agent() {
        let pb = parse(
            r#"
name: t
params:
  URL:
    prompt: url
steps:
  - send:
      agent: plan
      message: fix ${URL}
"#,
        );
        let http = FakeHttp::default();
        let mut vars = HashMap::new();
        vars.insert("URL".into(), "https://pr/1".into());
        run_playbook(&pb, &mut ctx(vars), &http, &PanicShell).unwrap();
        let rec = http.recorded.lock().unwrap();
        assert_eq!(rec.len(), 1);
        assert_eq!(rec[0].agent, "plan");
        assert_eq!(rec[0].message, "fix https://pr/1");
    }

    #[test]
    fn aborted_turn_stops_following_playbook_steps() {
        let pb = parse(
            r#"
name: t
steps:
  - send:
      agent: chat
      message: first
  - send:
      agent: plan
      message: second
"#,
        );
        let http = FakeHttp {
            after_send_error: Some(EngineError::Aborted),
            ..Default::default()
        };

        assert!(matches!(
            run_playbook(&pb, &mut ctx(HashMap::new()), &http, &PanicShell),
            Err(EngineError::Aborted)
        ));
        assert_eq!(http.recorded.lock().unwrap().len(), 1);
    }

    #[test]
    fn failed_turn_stops_following_playbook_steps() {
        let pb = parse(
            r#"
name: t
steps:
  - send:
      agent: chat
      message: first
  - send:
      agent: plan
      message: second
"#,
        );
        let http = FakeHttp {
            after_send_error: Some(EngineError::SendFailed("provider".into())),
            ..Default::default()
        };

        assert!(matches!(
            run_playbook(&pb, &mut ctx(HashMap::new()), &http, &PanicShell),
            Err(EngineError::SendFailed(_))
        ));
        assert_eq!(http.recorded.lock().unwrap().len(), 1);
    }

    #[test]
    fn engine_fails_on_unknown_message_var_before_http() {
        let pb = parse(
            r#"
name: t
steps:
  - send:
      agent: chat
      message: ${MISSING}
"#,
        );
        let http = FakeHttp::default();
        let err = run_playbook(&pb, &mut ctx(HashMap::new()), &http, &PanicShell).unwrap_err();
        assert!(matches!(err, EngineError::UnknownVar(_)));
        assert!(http.recorded.lock().unwrap().is_empty());
    }

    struct ScriptedShell {
        calls: Mutex<Vec<(String, HashMap<String, String>)>>,
        results: Vec<ShellResult>,
        func: Option<Box<dyn Fn(&str, &HashMap<String, String>) -> ShellResult + Send + Sync>>,
    }

    impl ScriptedShell {
        fn new(results: Vec<ShellResult>) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                results,
                func: None,
            }
        }

        fn from_fn<F>(f: F) -> Self
        where
            F: Fn(&str, &HashMap<String, String>) -> ShellResult + Send + Sync + 'static,
        {
            Self {
                calls: Mutex::new(Vec::new()),
                results: Vec::new(),
                func: Some(Box::new(f)),
            }
        }
    }

    impl Shell for ScriptedShell {
        fn run(&self, _: &Path, env: &HashMap<String, String>, script: &str) -> ShellResult {
            let mut calls = self.calls.lock().unwrap();
            let idx = calls.len();
            calls.push((script.to_string(), env.clone()));
            if let Some(f) = &self.func {
                return f(script, env);
            }
            self.results.get(idx).cloned().unwrap_or(ShellResult {
                exit_code: 0,
                stdout: String::new(),
                stderr: String::new(),
            })
        }
    }

    #[test]
    fn run_exports_trimmed_stdout_as_env_for_later_steps() {
        let pb = parse(
            r#"
name: t
steps:
  - run: echo unused
    register: COMMENTS
  - send:
      agent: chat
      message: count ${COMMENTS}
"#,
        );
        let http = FakeHttp::default();
        let shell = ScriptedShell::new(vec![ShellResult {
            exit_code: 0,
            stdout: "  3\n".into(),
            stderr: String::new(),
        }]);
        run_playbook(&pb, &mut ctx(HashMap::new()), &http, &shell).unwrap();
        assert_eq!(http.recorded.lock().unwrap()[0].message, "count 3");
        let env = &shell.calls.lock().unwrap()[0].1;
        assert_eq!(env.get("SESSION_ID").unwrap(), "sess_1");
    }

    #[test]
    fn run_nonzero_fails_the_playbook() {
        let pb = parse("name: t\nsteps:\n  - run: \"exit 1\"\n");
        let shell = ScriptedShell::new(vec![ShellResult {
            exit_code: 2,
            stdout: String::new(),
            stderr: "boom".into(),
        }]);
        let err =
            run_playbook(&pb, &mut ctx(HashMap::new()), &FakeHttp::default(), &shell).unwrap_err();
        assert!(matches!(err, EngineError::RunFailed { exit_code: 2, .. }));
    }

    #[test]
    fn if_uses_exit_code_and_does_not_fail_on_nonzero_condition() {
        let pb = parse(
            r#"
name: t
steps:
  - if: test -n "$COMMENTS"
    then:
      - send:
          agent: plan
          message: yes
    else:
      - send:
          agent: chat
          message: no
"#,
        );
        let http = FakeHttp::default();
        let shell = ScriptedShell::new(vec![ShellResult {
            exit_code: 1,
            stdout: String::new(),
            stderr: String::new(),
        }]);
        run_playbook(&pb, &mut ctx(HashMap::new()), &http, &shell).unwrap();
        assert_eq!(http.recorded.lock().unwrap()[0].agent, "chat");
        assert_eq!(http.recorded.lock().unwrap()[0].message, "no");
    }

    #[test]
    fn run_script_is_not_interpolated() {
        let pb = parse("name: t\nsteps:\n  - run: echo ${GITHUB_PR_URL}\n");
        let shell = ScriptedShell::new(vec![ShellResult {
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
        }]);
        run_playbook(&pb, &mut ctx(HashMap::new()), &FakeHttp::default(), &shell).unwrap();
        assert_eq!(shell.calls.lock().unwrap()[0].0, "echo ${GITHUB_PR_URL}");
    }

    #[test]
    fn until_stops_when_condition_succeeds_without_running_body() {
        let pb = parse(
            r#"
name: t
steps:
  - until: test "$COMMENTS" = "0"
    max: 5
    steps:
      - send:
          agent: chat
          message: should not send
"#,
        );
        let mut vars = HashMap::new();
        vars.insert("COMMENTS".into(), "0".into());
        let http = FakeHttp::default();
        let shell = ScriptedShell::from_fn(|_script, env| {
            if env.get("COMMENTS").map(|s| s.as_str()) == Some("0") {
                ShellResult {
                    exit_code: 0,
                    stdout: String::new(),
                    stderr: String::new(),
                }
            } else {
                ShellResult {
                    exit_code: 1,
                    stdout: String::new(),
                    stderr: String::new(),
                }
            }
        });
        run_playbook(&pb, &mut ctx(vars), &http, &shell).unwrap();
        assert!(http.recorded.lock().unwrap().is_empty());
    }

    #[test]
    fn until_runs_body_until_register_clears() {
        let pb = parse(
            r#"
name: t
steps:
  - run: echo 2
    register: COMMENTS
  - until: test "$COMMENTS" = "0"
    max: 5
    steps:
      - send:
          agent: chat
          message: fix
      - run: echo 0
        register: COMMENTS
"#,
        );
        let http = FakeHttp::default();
        let shell = ScriptedShell::new(vec![
            ShellResult {
                exit_code: 0,
                stdout: "2".into(),
                stderr: String::new(),
            },
            ShellResult {
                exit_code: 1,
                stdout: String::new(),
                stderr: String::new(),
            },
            ShellResult {
                exit_code: 0,
                stdout: "0".into(),
                stderr: String::new(),
            },
            ShellResult {
                exit_code: 0,
                stdout: String::new(),
                stderr: String::new(),
            },
        ]);
        run_playbook(&pb, &mut ctx(HashMap::new()), &http, &shell).unwrap();
        let rec = http.recorded.lock().unwrap();
        assert_eq!(rec.len(), 1);
        assert_eq!(rec[0].message, "fix");
    }

    #[test]
    fn until_hits_max_and_fails() {
        let pb = parse(
            r#"
name: t
steps:
  - until: test 1 -eq 2
    max: 2
    steps:
      - send:
          agent: chat
          message: x
"#,
        );
        let http = FakeHttp::default();
        let shell = ScriptedShell::from_fn(|_, _| ShellResult {
            exit_code: 1,
            stdout: String::new(),
            stderr: String::new(),
        });
        let err = run_playbook(&pb, &mut ctx(HashMap::new()), &http, &shell).unwrap_err();
        assert!(matches!(err, EngineError::LoopLimit { max: 2 }));
        assert_eq!(http.recorded.lock().unwrap().len(), 2);
    }

    #[derive(Default)]
    struct FakeHttpFull {
        recorded: Mutex<Vec<SendRequest>>,
        abort_called: Mutex<Option<(String, u16)>>,
        on_send: Option<Box<dyn Fn() + Send + Sync>>,
        fail_msg: Option<String>,
        timeout: bool,
    }

    impl FakeHttpFull {
        fn with_on_send<F: Fn() + Send + Sync + 'static>(f: F) -> Self {
            Self {
                on_send: Some(Box::new(f)),
                ..Default::default()
            }
        }
        fn fail_send(msg: &str) -> Self {
            Self {
                fail_msg: Some(msg.to_string()),
                ..Default::default()
            }
        }
        fn timeout_send() -> Self {
            Self {
                timeout: true,
                ..Default::default()
            }
        }
    }

    impl PromptClient for FakeHttpFull {
        fn send(&self, req: SendRequest) -> Result<(), EngineError> {
            self.recorded.lock().unwrap().push(req);
            if let Some(f) = &self.on_send {
                f();
            }
            if self.timeout {
                return Err(EngineError::TimedOut);
            }
            if let Some(msg) = &self.fail_msg {
                return Err(EngineError::SendFailed(msg.clone()));
            }
            Ok(())
        }
        fn abort(&self, sid: &str, port: u16) -> Result<(), EngineError> {
            *self.abort_called.lock().unwrap() = Some((sid.to_string(), port));
            Ok(())
        }
    }

    #[test]
    fn abort_flag_stops_before_next_send() {
        let pb = parse(
            r#"
name: t
steps:
  - send:
      agent: chat
      message: one
  - send:
      agent: plan
      message: two
"#,
        );
        let abort = Arc::new(AtomicBool::new(false));
        let http = FakeHttpFull::with_on_send({
            let abort = abort.clone();
            move || abort.store(true, std::sync::atomic::Ordering::SeqCst)
        });
        let err =
            run_playbook_with_abort(&pb, &mut ctx(HashMap::new()), &http, &PanicShell, &abort)
                .unwrap_err();
        assert!(matches!(err, EngineError::Aborted));
        assert_eq!(http.recorded.lock().unwrap().len(), 1);
    }

    #[test]
    fn send_error_fails_playbook() {
        let pb = parse("name: t\nsteps:\n  - send:\n      agent: chat\n      message: x\n");
        let http = FakeHttpFull::fail_send("503");
        let err = run_playbook(&pb, &mut ctx(HashMap::new()), &http, &PanicShell).unwrap_err();
        assert!(matches!(err, EngineError::SendFailed(_)));
    }

    #[test]
    fn send_timeout_calls_client_abort() {
        let pb = parse("name: t\nsteps:\n  - send:\n      agent: chat\n      message: x\n");
        let http = FakeHttpFull::timeout_send();
        let err = run_playbook(&pb, &mut ctx(HashMap::new()), &http, &PanicShell).unwrap_err();
        assert!(matches!(err, EngineError::TimedOut));
        assert!(http.abort_called.lock().unwrap().is_some());
    }

    #[test]
    fn spawn_emits_done_after_sends() {
        let pb = parse("name: t\nsteps:\n  - send:\n      agent: chat\n      message: x\n");
        let http: Arc<dyn PromptClient + Send + Sync> = Arc::new(FakeHttp::default());
        let shell: Arc<dyn Shell + Send + Sync> = Arc::new(PanicShell);
        let handle = spawn_playbook(pb, ctx(HashMap::new()), http, shell);
        let mut events = Vec::new();
        for ev in handle.rx.try_iter() {
            events.push(ev);
        }
        // Wait for terminal event
        loop {
            match handle.rx.recv_timeout(std::time::Duration::from_secs(5)) {
                Ok(PlaybookEvent::Done) => {
                    events.push(PlaybookEvent::Done);
                    break;
                }
                Ok(ev) => events.push(ev),
                Err(_) => panic!("timeout waiting for Done"),
            }
        }
        assert!(matches!(events.last(), Some(PlaybookEvent::Done)));
    }

    #[test]
    fn spawn_emits_step_then_done() {
        let pb = parse("name: t\nsteps:\n  - send:\n      agent: chat\n      message: x\n");
        let http: Arc<dyn PromptClient + Send + Sync> = Arc::new(FakeHttp::default());
        let shell: Arc<dyn Shell + Send + Sync> = Arc::new(PanicShell);
        let handle = spawn_playbook(pb, ctx(HashMap::new()), http, shell);
        let first = handle
            .rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        assert_eq!(
            first,
            PlaybookEvent::Step {
                label: "send:chat".into()
            }
        );
        assert_eq!(
            handle
                .rx
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap(),
            PlaybookEvent::Done
        );
    }

    #[test]
    fn spawn_abort_emits_aborted() {
        let pb = parse(
            "name: t\nsteps:\n  - send:\n      agent: chat\n      message: one\n  - send:\n      agent: plan\n      message: two\n",
        );
        let abort_flag = Arc::new(AtomicBool::new(false));
        let http: Arc<dyn PromptClient + Send + Sync> = Arc::new(FakeHttpFull::with_on_send({
            let abort = abort_flag.clone();
            move || abort.store(true, std::sync::atomic::Ordering::SeqCst)
        }));
        let shell: Arc<dyn Shell + Send + Sync> = Arc::new(PanicShell);
        let handle = spawn_playbook_with_abort(pb, ctx(HashMap::new()), http, shell, abort_flag);
        loop {
            match handle.rx.recv_timeout(std::time::Duration::from_secs(5)) {
                Ok(PlaybookEvent::Aborted) => break,
                Ok(_) => {}
                Err(_) => panic!("timeout waiting for Aborted"),
            }
        }
    }

    #[test]
    fn gate_serve_port_none_is_error() {
        assert!(gate_serve_port(None).is_err());
    }

    #[test]
    fn gate_serve_port_some_is_ok() {
        assert_eq!(gate_serve_port(Some(4200)), Ok(4200));
    }

    #[test]
    fn banner_shows_playbook_and_step() {
        assert_eq!(
            playbook_banner("pr-comments", Some("send:plan")),
            "playbook: pr-comments  send:plan"
        );
        assert_eq!(
            playbook_banner("pr-comments", None),
            "playbook: pr-comments"
        );
    }

    #[test]
    fn pr_comments_playbook_loop() {
        let pb = parse(
            r#"
name: pr-comments
description: Review, plan, fix, resolve PR comments
params:
  GITHUB_PR_URL:
    prompt: GitHub PR URL
    required: true
steps:
  - run: |
      gh api "$GITHUB_PR_URL/comments" --jq 'length'
    register: COMMENTS
  - until: test "$COMMENTS" = "0"
    max: 5
    steps:
      - send:
          agent: chat
          message: |
            review comments from ${GITHUB_PR_URL}
            don't fix anything, just summarize it and tell me what they mean, and are they worth fixing
      - send:
          agent: plan
          message: make a plan to fix this
      - send:
          agent: build
          message: Proceed with implementation
      - send:
          agent: orchestrator
          message: reply in thread and resolve comments
      - run: |
          gh api "$GITHUB_PR_URL/comments" --jq 'length'
        register: COMMENTS
"#,
        );
        let http = FakeHttp::default();
        // ScriptedShell: seed returns "2", until-check fails (COMMENTS != 0), body runs,
        // inner run returns "0", until-check succeeds.
        let shell = ScriptedShell::new(vec![
            ShellResult {
                exit_code: 0,
                stdout: "2".into(),
                stderr: String::new(),
            },
            ShellResult {
                exit_code: 1,
                stdout: String::new(),
                stderr: String::new(),
            },
            ShellResult {
                exit_code: 0,
                stdout: "0".into(),
                stderr: String::new(),
            },
            ShellResult {
                exit_code: 0,
                stdout: String::new(),
                stderr: String::new(),
            },
        ]);
        let mut vars = HashMap::new();
        vars.insert(
            "GITHUB_PR_URL".into(),
            "https://github.com/org/repo/pull/1".into(),
        );
        run_playbook(&pb, &mut ctx(vars), &http, &shell).unwrap();
        let rec = http.recorded.lock().unwrap();
        let agents: Vec<&str> = rec.iter().map(|s| s.agent.as_str()).collect();
        assert_eq!(agents, ["chat", "plan", "build", "orchestrator"]);
        assert!(
            rec[0]
                .message
                .contains("https://github.com/org/repo/pull/1")
        );
    }
}
