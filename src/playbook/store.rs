use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context;

use super::schema::{Playbook, parse_playbook};

#[derive(Debug, Default)]
pub struct LoadedPlaybooks {
    pub playbooks: Vec<Playbook>,
    pub skipped: Vec<(PathBuf, String)>,
}

pub fn default_playbooks_dir() -> anyhow::Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".config/ocmux/playbooks"))
}

pub fn load_playbooks(dir: &Path) -> anyhow::Result<LoadedPlaybooks> {
    if !dir.exists() {
        return Ok(LoadedPlaybooks::default());
    }

    let mut result = LoadedPlaybooks::default();

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str());
        if ext != Some("yaml") && ext != Some("yml") {
            continue;
        }
        let raw = fs::read_to_string(&path)?;
        match parse_playbook(&raw) {
            Ok(pb) => result.playbooks.push(pb),
            Err(e) => result.skipped.push((path, format!("{e:#}"))),
        }
    }

    result.playbooks.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ocmux-playbooks-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn load_skips_invalid_yaml_and_non_yaml() {
        let dir = temp_dir();
        fs::write(
            dir.join("good.yaml"),
            "name: good\nsteps:\n  - send:\n      agent: chat\n      message: hi\n",
        )
        .unwrap();
        fs::write(dir.join("bad.yaml"), "name: [\n").unwrap();
        fs::write(dir.join("notes.txt"), "ignore me").unwrap();
        fs::write(
            dir.join("no-max.yaml"),
            "name: loop\nsteps:\n  - until: test 1\n    steps:\n      - send:\n          agent: chat\n          message: x\n",
        )
        .unwrap();

        let loaded = load_playbooks(&dir).unwrap();
        assert_eq!(loaded.playbooks.len(), 1);
        assert_eq!(loaded.playbooks[0].name, "good");
        assert_eq!(loaded.skipped.len(), 2);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn missing_directory_is_empty_not_error() {
        let dir = std::env::temp_dir().join(format!("ocmux-missing-{}", uuid::Uuid::new_v4()));
        let loaded = load_playbooks(&dir).unwrap();
        assert!(loaded.playbooks.is_empty());
        assert!(loaded.skipped.is_empty());
    }
}
