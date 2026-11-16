use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use super::engine::{Shell, ShellResult};

#[derive(Debug, Clone)]
pub struct PosixShell {
    pub timeout: std::time::Duration,
}

impl Default for PosixShell {
    fn default() -> Self {
        Self {
            timeout: std::time::Duration::from_secs(60),
        }
    }
}

impl Shell for PosixShell {
    fn run(&self, cwd: &Path, env: &HashMap<String, String>, script: &str) -> ShellResult {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(script).current_dir(cwd);
        for (k, v) in env {
            cmd.env(k, v);
        }
        let output = match cmd.output() {
            Ok(o) => o,
            Err(e) => {
                return ShellResult {
                    exit_code: -1,
                    stdout: String::new(),
                    stderr: e.to_string(),
                };
            }
        };
        ShellResult {
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posix_shell_sees_env_and_returns_stdout_and_exit() {
        let mut env = HashMap::new();
        env.insert("FOO".into(), "bar".into());
        let shell = PosixShell::default();
        let result = shell.run(
            std::env::temp_dir().as_path(),
            &env,
            r#"echo "$FOO"; exit 3"#,
        );
        assert_eq!(result.stdout.trim(), "bar");
        assert_eq!(result.exit_code, 3);
    }

    #[test]
    fn posix_shell_inherits_path() {
        let env = HashMap::new();
        let shell = PosixShell::default();
        let result = shell.run(std::env::temp_dir().as_path(), &env, "which sh");
        assert_eq!(result.exit_code, 0);
    }
}
