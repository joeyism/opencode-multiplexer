use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Context;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    pub sidebar_width: u16,
    pub keybindings: Keybindings,
    pub notifications: bool,
    pub spawn_maxdepth: u32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            sidebar_width: 30,
            keybindings: Keybindings::default(),
            notifications: true,
            spawn_maxdepth: 5,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keybindings {
    pub up: char,
    pub down: char,
    pub spawn: char,
    pub kill: char,
    pub help: char,
    pub worktree: char,
    pub quit: char,
    pub view: char,
    pub files: char,
    pub diff: char,
    pub history: char,
}

impl Default for Keybindings {
    fn default() -> Self {
        Self {
            up: 'k',
            down: 'j',
            spawn: 'n',
            kill: 'x',
            help: '?',
            worktree: 't',
            quit: 'q',
            view: 'v',
            files: 'f',
            diff: 'd',
            history: 'h',
        }
    }
}

#[derive(Debug, Deserialize)]
struct PartialConfig {
    sidebar_width: Option<u16>,
    keybindings: Option<PartialKeybindings>,
    notifications: Option<bool>,
    spawn_maxdepth: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct PartialKeybindings {
    up: Option<String>,
    down: Option<String>,
    spawn: Option<String>,
    kill: Option<String>,
    help: Option<String>,
    worktree: Option<String>,
    quit: Option<String>,
    view: Option<String>,
    files: Option<String>,
    diff: Option<String>,
    history: Option<String>,
}

pub fn load_config() -> anyhow::Result<AppConfig> {
    load_config_from_path(&default_config_path()?)
}

pub fn load_config_from_path(path: &Path) -> anyhow::Result<AppConfig> {
    if !path.exists() {
        return Ok(AppConfig::default());
    }

    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read config {}", path.display()))?;
    let partial: PartialConfig = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse config {}", path.display()))?;

    let mut config = AppConfig::default();
    if let Some(sidebar_width) = partial.sidebar_width {
        config.sidebar_width = sidebar_width;
    }
    if let Some(bindings) = partial.keybindings {
        apply_keybinding(&mut config.keybindings.up, bindings.up);
        apply_keybinding(&mut config.keybindings.down, bindings.down);
        apply_keybinding(&mut config.keybindings.spawn, bindings.spawn);
        apply_keybinding(&mut config.keybindings.kill, bindings.kill);
        apply_keybinding(&mut config.keybindings.help, bindings.help);
        apply_keybinding(&mut config.keybindings.worktree, bindings.worktree);
        apply_keybinding(&mut config.keybindings.quit, bindings.quit);
        apply_keybinding(&mut config.keybindings.view, bindings.view);
        apply_keybinding(&mut config.keybindings.files, bindings.files);
        apply_keybinding(&mut config.keybindings.diff, bindings.diff);
        apply_keybinding(&mut config.keybindings.history, bindings.history);
    }
    if let Some(notifications) = partial.notifications {
        config.notifications = notifications;
    }
    if let Some(spawn_maxdepth) = partial.spawn_maxdepth {
        config.spawn_maxdepth = spawn_maxdepth;
    }

    Ok(config)
}

fn apply_keybinding(slot: &mut char, incoming: Option<String>) {
    if let Some(value) = incoming.and_then(|value| value.chars().next()) {
        *slot = value;
    }
}

fn default_config_path() -> anyhow::Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".config/ocmux/config.json"))
}
