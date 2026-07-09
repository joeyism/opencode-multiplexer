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

pub struct PtySession {
    child: Box<dyn Child + Send + Sync>,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    rx: Receiver<Vec<u8>>,
    pub surface: TerminalSurface,
}

impl PtySession {
    pub fn spawn_shell(rows: u16, cols: u16) -> anyhow::Result<Self> {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
        let cmd = CommandBuilder::new(shell);
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
        let (tx, rx) = mpsc::channel();

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

        Ok(Self {
            child,
            master: pair.master,
            writer,
            rx,
            surface: TerminalSurface::new(rows as usize, cols as usize),
        })
    }

    pub fn drain_output(&mut self) {
        while let Ok(bytes) = self.rx.try_recv() {
            self.surface.process(&bytes);
        }
    }

    pub fn send_key(&mut self, key: KeyEvent) -> anyhow::Result<()> {
        if let Some(bytes) = key_event_to_bytes(key) {
            self.writer.write_all(&bytes)?;
            self.writer.flush()?;
        }
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

    pub fn process_id(&self) -> Option<u32> {
        self.child.process_id()
    }

    #[doc(hidden)]
    pub fn spawn_test_command(cmd: CommandBuilder, rows: u16, cols: u16) -> anyhow::Result<Self> {
        Self::spawn_command(cmd, rows, cols)
    }
}
