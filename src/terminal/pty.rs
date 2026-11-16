use std::{
    io::{Read, Write},
    path::Path,
    sync::mpsc::{self, Receiver},
    thread,
};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::{
    ops::opencode::{build_managed_session_command, build_replica_command},
    terminal::{input::key_event_to_bytes, surface::TerminalSurface},
};
use crossterm::event::KeyEvent;

const OUTPUT_CHANNEL_CAPACITY: usize = 32;

pub struct PtySession {
    child: Box<dyn Child + Send + Sync>,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    rx: Receiver<Vec<u8>>,
    pub surface: TerminalSurface,
    process_group_id: Option<i32>,
}

impl PtySession {
    pub fn spawn_shell(rows: u16, cols: u16) -> anyhow::Result<Self> {
        Self::spawn_shell_at(None, rows, cols)
    }

    pub fn spawn_shell_at(cwd: Option<&Path>, rows: u16, cols: u16) -> anyhow::Result<Self> {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
        let mut cmd = CommandBuilder::new(shell);
        if let Some(cwd) = cwd {
            cmd.cwd(cwd);
        }
        Self::spawn_command(cmd, rows, cols)
    }

    pub fn spawn_managed(cwd: &Path, port: u16, rows: u16, cols: u16) -> anyhow::Result<Self> {
        let cmd = build_managed_session_command(cwd, port);
        Self::spawn_command(cmd, rows, cols)
    }

    pub fn spawn_replica(
        cwd: &Path,
        session_id: &str,
        port: Option<u16>,
        rows: u16,
        cols: u16,
    ) -> anyhow::Result<Self> {
        let cmd = build_replica_command(cwd, session_id, port);
        Self::spawn_command(cmd, rows, cols)
    }

    fn spawn_command(cmd: CommandBuilder, rows: u16, cols: u16) -> anyhow::Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let child = pair.slave.spawn_command(cmd)?;
        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        // Bound queued output from hidden panes. If the UI can't keep up, the
        // reader pauses and the PTY's normal backpressure reaches the child.
        let (tx, rx) = mpsc::sync_channel(OUTPUT_CHANNEL_CAPACITY);

        thread::spawn(move || {
            let mut buffer = [0u8; 8192];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => {
                        if tx.send(buffer[..count].to_vec()).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        #[cfg(unix)]
        let process_group_id = process_group_id_for_child(child.as_ref())
            .or_else(|| pair.master.process_group_leader());
        #[cfg(not(unix))]
        let process_group_id = None;

        Ok(Self {
            child,
            master: pair.master,
            writer,
            rx,
            surface: TerminalSurface::new(rows as usize, cols as usize),
            process_group_id,
        })
    }

    pub fn drain_output(&mut self) {
        while let Ok(bytes) = self.rx.try_recv() {
            self.surface.process(&bytes);
        }
    }

    pub fn send_key(&mut self, key: KeyEvent) -> anyhow::Result<()> {
        if let Some(bytes) = key_event_to_bytes(key) {
            self.send_bytes(&bytes)?;
        }
        Ok(())
    }

    pub fn send_bytes(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        self.writer.write_all(bytes)?;
        self.writer.flush()?;
        Ok(())
    }

    pub fn resize(&mut self, rows: u16, cols: u16) -> anyhow::Result<()> {
        self.surface.resize(rows as usize, cols as usize);
        self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }

    pub fn send_paste(&mut self, text: &str) -> anyhow::Result<()> {
        // Wrap in bracketed paste escape sequences
        self.writer.write_all(b"[200~")?;
        self.writer.write_all(text.as_bytes())?;
        self.writer.write_all(b"[201~")?;
        self.writer.flush()?;
        Ok(())
    }

    pub fn is_alive(&mut self) -> bool {
        match self.child.try_wait() {
            Ok(Some(_)) => false, // exited
            Ok(None) => true,     // still running
            Err(_) => false,      // can't check → treat as dead
        }
    }

    pub fn kill(&mut self) -> std::io::Result<()> {
        self.child.kill()
    }

    pub fn kill_process_group(&mut self) -> std::io::Result<()> {
        #[cfg(unix)]
        if let Some(process_group) = self.process_group_id {
            if signal_process_group(process_group, libc::SIGHUP).is_ok() {
                wait_for_process_group_exit(process_group, std::time::Duration::from_millis(250));
                if process_group_exists(process_group) {
                    let _ = signal_process_group(process_group, libc::SIGTERM);
                    wait_for_process_group_exit(
                        process_group,
                        std::time::Duration::from_millis(250),
                    );
                }
                if process_group_exists(process_group) {
                    let _ = signal_process_group(process_group, libc::SIGKILL);
                    wait_for_process_group_exit(
                        process_group,
                        std::time::Duration::from_millis(250),
                    );
                }
            }

            for _ in 0..5 {
                if self.child.try_wait()?.is_some() {
                    return Ok(());
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        match self.child.kill() {
            Ok(()) => Ok(()),
            Err(_error) if self.child.try_wait()?.is_some() => Ok(()),
            Err(error) => Err(error),
        }
    }

    pub fn process_id(&self) -> Option<u32> {
        self.child.process_id()
    }

    #[cfg(test)]
    fn process_group_id(&self) -> Option<i32> {
        self.process_group_id
    }

    #[doc(hidden)]
    pub fn spawn_test_command(cmd: CommandBuilder, rows: u16, cols: u16) -> anyhow::Result<Self> {
        Self::spawn_command(cmd, rows, cols)
    }
}

#[cfg(unix)]
fn process_group_id_for_child(child: &dyn portable_pty::Child) -> Option<i32> {
    let pid = child.process_id()? as i32;
    let pgid = unsafe { libc::getpgid(pid) };
    if pgid >= 0 { Some(pgid) } else { None }
}

#[cfg(unix)]
fn signal_process_group(process_group: i32, signal: i32) -> std::io::Result<()> {
    let result = unsafe { libc::kill(-process_group, signal) };
    if result == 0 {
        Ok(())
    } else {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(())
        } else {
            Err(error)
        }
    }
}

#[cfg(unix)]
fn process_group_exists(process_group: i32) -> bool {
    unsafe { libc::kill(-process_group, 0) == 0 }
}

#[cfg(unix)]
fn wait_for_process_group_exit(process_group: i32, timeout: std::time::Duration) {
    let deadline = std::time::Instant::now() + timeout;
    while process_group_exists(process_group) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::{OUTPUT_CHANNEL_CAPACITY, PtySession};
    use portable_pty::CommandBuilder;
    use std::{thread, time::Duration};

    #[test]
    fn kill_process_group_terminates_shell_and_background_child() {
        let mut command = CommandBuilder::new("/bin/sh");
        command.args(["-c", "sleep 30 & wait"]);
        let mut pty = PtySession::spawn_test_command(command, 12, 40).unwrap();
        let process_group = pty.process_group_id().unwrap();
        thread::sleep(Duration::from_millis(50));

        pty.kill_process_group().unwrap();

        assert!(!pty.is_alive());
        assert_eq!(unsafe { libc::kill(-process_group, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }

    #[test]
    fn unread_terminal_output_has_a_bounded_pending_queue() {
        let command = CommandBuilder::new("/usr/bin/yes");
        let mut pty = PtySession::spawn_test_command(command, 12, 40).unwrap();
        thread::sleep(Duration::from_millis(250));
        let _ = pty.kill_process_group();

        let pending_chunks = pty.rx.try_iter().count();

        assert!(
            pending_chunks < OUTPUT_CHANNEL_CAPACITY * 32,
            "queued {pending_chunks} output chunks"
        );
    }

    fn wait_until(timeout: Duration, mut condition: impl FnMut() -> bool) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if condition() {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return condition();
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn killing_a_process_group_after_its_shell_exits_cleans_background_jobs() {
        let mut command = CommandBuilder::new("/bin/sh");
        command.args(["-c", "trap '' HUP; sleep 30 & exit 0"]);
        let mut pty = PtySession::spawn_test_command(command, 12, 40).unwrap();
        let process_group = pty
            .process_group_id()
            .expect("spawn should capture the process group before the shell exits");
        assert!(
            wait_until(Duration::from_secs(2), || !pty.is_alive()),
            "shell should have exited naturally"
        );
        assert!(
            wait_until(Duration::from_secs(2), || unsafe {
                libc::kill(-process_group, 0) == 0
            }),
            "background job should keep the process group alive"
        );

        pty.kill_process_group().unwrap();

        assert!(
            wait_until(Duration::from_secs(2), || unsafe {
                libc::kill(-process_group, 0) == -1
            }),
            "process group should be gone after kill"
        );
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }
}
