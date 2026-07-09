use std::path::Path;

use portable_pty::CommandBuilder;

pub fn build_managed_session_command(cwd: &Path, port: u16) -> CommandBuilder {
    let mut command = CommandBuilder::new("opencode");
    command.args(["attach", &format!("http://localhost:{port}")]);
    command.cwd(cwd);
    command
}

pub fn build_replica_command(cwd: &Path, session_id: &str, port: Option<u16>) -> CommandBuilder {
    let mut command = CommandBuilder::new("opencode");
    match port {
        Some(p) => {
            command.args([
                "attach",
                &format!("http://localhost:{p}"),
                "--session",
                session_id,
            ]);
        }
        None => {
            command.args(["-s", session_id]);
        }
    }
    command.cwd(cwd);
    command
}

pub fn display_title_for_cwd(cwd: &Path) -> String {
    cwd.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| cwd.display().to_string())
}

use std::net::TcpListener;
use std::process::{Command, Stdio};

pub fn find_available_port(start: u16) -> u16 {
    for port in start..start + 100 {
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return port;
        }
    }
    start
}

pub fn spawn_serve_daemon(cwd: &Path, port: u16) -> anyhow::Result<u32> {
    let child = Command::new("opencode")
        .args(["serve", "--port", &port.to_string()])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(child.id())
}

pub fn wait_for_serve_ready(port: u16, timeout_secs: u64) -> bool {
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(timeout_secs);
    while start.elapsed() < timeout {
        if let Ok(resp) = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_millis(500))
            .build()
            .and_then(|c| c.get(format!("http://localhost:{port}/session")).send())
            && resp.status().is_success()
        {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    false
}
