use std::{collections::HashMap, path::Path};

use crossterm::event::KeyEvent;

use super::pty::PtySession;

#[derive(Default)]
pub struct ShellManager {
    next_id: u64,
    shells: HashMap<u64, PtySession>,
}

impl ShellManager {
    pub fn spawn(&mut self, cwd: &Path, rows: u16, cols: u16) -> anyhow::Result<u64> {
        let shell = PtySession::spawn_shell_at(Some(cwd), rows, cols)?;
        let id = self.next_id;
        self.next_id += 1;
        self.shells.insert(id, shell);
        Ok(id)
    }

    pub fn terminal(&self, id: u64) -> Option<&PtySession> {
        self.shells.get(&id)
    }

    pub fn terminal_mut(&mut self, id: u64) -> Option<&mut PtySession> {
        self.shells.get_mut(&id)
    }

    pub fn drain_all_output(&mut self) {
        for shell in self.shells.values_mut() {
            shell.drain_output();
        }
    }

    pub fn resize(&mut self, id: u64, rows: u16, cols: u16) -> anyhow::Result<()> {
        let shell = self.shells.get_mut(&id).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "shell pane not found")
        })?;
        shell.resize(rows, cols)?;
        Ok(())
    }

    pub fn send_key(&mut self, id: u64, key: KeyEvent) -> anyhow::Result<()> {
        let shell = self.shells.get_mut(&id).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "shell pane not found")
        })?;
        shell.send_key(key)?;
        Ok(())
    }

    pub fn is_alive(&mut self, id: u64) -> bool {
        self.shells.get_mut(&id).is_some_and(PtySession::is_alive)
    }

    pub fn close(&mut self, id: u64) -> anyhow::Result<()> {
        if let Some(mut shell) = self.shells.remove(&id)
            && let Err(error) = shell.kill_process_group()
        {
            self.shells.insert(id, shell);
            return Err(error.into());
        }
        Ok(())
    }

    pub fn reap_exited(&mut self) -> Vec<u64> {
        let mut exited = Vec::new();
        for (&id, shell) in &mut self.shells {
            if !shell.is_alive() {
                let _ = shell.kill_process_group();
                exited.push(id);
            }
        }
        for id in &exited {
            self.shells.remove(id);
        }
        exited
    }
}

impl Drop for ShellManager {
    fn drop(&mut self) {
        for shell in self.shells.values_mut() {
            let _ = shell.kill_process_group();
        }
    }
}
