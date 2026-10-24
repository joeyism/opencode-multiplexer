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
    pub follow_selection: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            sidebar_width: 30,
            keybindings: Keybindings::default(),
            notifications: true,
            spawn_maxdepth: 5,
            follow_selection: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyChord {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: char,
}

impl KeyChord {
    pub fn parse(raw: &str) -> anyhow::Result<Self> {
        let mut ctrl = false;
        let mut alt = false;
        let mut shift = false;
        let mut key = None;
        for token in raw.split('-') {
            match token.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => ctrl = true,
                "alt" | "option" => alt = true,
                "shift" => shift = true,
                one if one.chars().count() == 1 => {
                    key = one.chars().next();
                }
                other => anyhow::bail!("unknown key token '{other}'"),
            }
        }
        let key = key.context("chord missing a key")?;
        Ok(Self {
            ctrl,
            alt,
            shift,
            key,
        })
    }

    pub fn matches(&self, ctrl: bool, alt: bool, shift: bool, ch: char) -> bool {
        ch.eq_ignore_ascii_case(&self.key)
            && ctrl == self.ctrl
            && alt == self.alt
            && (self.shift == shift || (!self.shift && shift))
    }

    pub fn display(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push_str("Ctrl");
        }
        if self.alt {
            if !s.is_empty() {
                s.push('-');
            }
            s.push_str("Alt");
        }
        if self.shift {
            if !s.is_empty() {
                s.push('-');
            }
            s.push_str("Shift");
        }
        if !s.is_empty() {
            s.push('-');
        }
        s.push(self.key);
        s
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
    pub sessions: char,
    pub agents: char,
    pub playbook: char,
    pub move_session: char,
    pub session_next: KeyChord,
    pub session_prev: KeyChord,
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
            sessions: 's',
            agents: 'a',
            playbook: 'p',
            move_session: 'm',
            session_next: KeyChord {
                ctrl: true,
                alt: true,
                shift: false,
                key: 'j',
            },
            session_prev: KeyChord {
                ctrl: true,
                alt: true,
                shift: false,
                key: 'k',
            },
        }
    }
}

#[derive(Debug, Deserialize)]
struct PartialConfig {
    sidebar_width: Option<u16>,
    keybindings: Option<PartialKeybindings>,
    notifications: Option<bool>,
    spawn_maxdepth: Option<u32>,
    follow_selection: Option<bool>,
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
    sessions: Option<String>,
    agents: Option<String>,
    playbook: Option<String>,
    move_session: Option<String>,
    session_next: Option<String>,
    session_prev: Option<String>,
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
        apply_keybinding(&mut config.keybindings.sessions, bindings.sessions);
        apply_keybinding(&mut config.keybindings.agents, bindings.agents);
        apply_keybinding(&mut config.keybindings.playbook, bindings.playbook);
        apply_keybinding(&mut config.keybindings.move_session, bindings.move_session);
        if let Some(raw) = bindings.session_next {
            config.keybindings.session_next = KeyChord::parse(&raw)?;
        }
        if let Some(raw) = bindings.session_prev {
            config.keybindings.session_prev = KeyChord::parse(&raw)?;
        }
    }
    if let Some(notifications) = partial.notifications {
        config.notifications = notifications;
    }
    if let Some(spawn_maxdepth) = partial.spawn_maxdepth {
        config.spawn_maxdepth = spawn_maxdepth;
    }
    if let Some(follow_selection) = partial.follow_selection {
        config.follow_selection = follow_selection;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_playbook_key_is_p() {
        assert_eq!(Keybindings::default().playbook, 'p');
    }

    #[test]
    fn default_move_session_key_is_m_and_configurable() {
        assert_eq!(Keybindings::default().move_session, 'm');
        let dir = std::env::temp_dir().join(format!("ocmux-cfg-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        std::fs::write(&path, r#"{"keybindings":{"move_session":"M"}}"#).unwrap();
        let cfg = load_config_from_path(&path).unwrap();
        assert_eq!(cfg.keybindings.move_session, 'M');
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn key_chord_parses_ctrl_alt_j() {
        let chord = KeyChord::parse("ctrl-alt-j").unwrap();
        assert!(chord.ctrl);
        assert!(chord.alt);
        assert!(!chord.shift);
        assert_eq!(chord.key, 'j');
    }

    #[test]
    fn key_chord_parse_is_case_insensitive_and_order_flexible() {
        let a = KeyChord::parse("ALT-CTRL-J").unwrap();
        let b = KeyChord::parse("ctrl-alt-j").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn key_chord_matches_ignores_char_case_and_extra_shift() {
        let chord = KeyChord::parse("ctrl-alt-j").unwrap();
        assert!(chord.matches(true, true, false, 'j'));
        assert!(chord.matches(true, true, true, 'J'));
        assert!(!chord.matches(true, false, false, 'j'));
        assert!(!chord.matches(true, true, false, 'k'));
    }

    #[test]
    fn key_chord_parse_rejects_garbage() {
        assert!(KeyChord::parse("ctrl-alt").is_err());
        assert!(KeyChord::parse("ctrl-alt-jj").is_err());
        assert!(KeyChord::parse("super-j").is_err());
    }

    #[test]
    fn key_chord_display_ctrl_alt_j() {
        assert_eq!(
            KeyChord::parse("ctrl-alt-j").unwrap().display(),
            "Ctrl-Alt-j"
        );
    }

    #[test]
    fn default_session_cycle_chords_are_ctrl_alt_j_k() {
        let keys = Keybindings::default();
        assert_eq!(keys.session_next, KeyChord::parse("ctrl-alt-j").unwrap());
        assert_eq!(keys.session_prev, KeyChord::parse("ctrl-alt-k").unwrap());
    }

    #[test]
    fn default_follow_selection_is_false() {
        assert!(!AppConfig::default().follow_selection);
    }

    #[test]
    fn config_parses_follow_selection_and_session_chords() {
        let dir = std::env::temp_dir().join(format!("ocmux-cfg-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        std::fs::write(
            &path,
            r#"{
          "follow_selection": true,
          "keybindings": { "session_next": "ctrl-alt-n", "session_prev": "ctrl-alt-p" }
        }"#,
        )
        .unwrap();
        let cfg = load_config_from_path(&path).unwrap();
        assert!(cfg.follow_selection);
        assert_eq!(cfg.keybindings.session_next.key, 'n');
        assert_eq!(cfg.keybindings.session_prev.key, 'p');
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn config_parses_playbook_key() {
        let dir = std::env::temp_dir().join(format!("ocmux-cfg-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        std::fs::write(&path, r#"{"keybindings":{"playbook":"m"}}"#).unwrap();
        let cfg = load_config_from_path(&path).unwrap();
        assert_eq!(cfg.keybindings.playbook, 'm');
        std::fs::remove_dir_all(&dir).ok();
    }
}
