use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::app::sessions::SessionStatus;

/// Minimum time between notifications for the same session.
const COOLDOWN_SECS: u64 = 5;

pub struct Notifier {
    enabled: bool,
    cooldowns: HashMap<String, Instant>,
}

impl Notifier {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            cooldowns: HashMap::new(),
        }
    }

    /// Returns true if `from -> to` is a transition worth notifying about.
    pub fn is_interesting_transition(from: SessionStatus, to: SessionStatus) -> bool {
        matches!(
            (from, to),
            (SessionStatus::Working, SessionStatus::Idle)
                | (SessionStatus::Working, SessionStatus::NeedsInput)
                | (SessionStatus::Working, SessionStatus::Error)
                | (SessionStatus::SubagentsWorking, SessionStatus::Idle)
                | (SessionStatus::SubagentsWorking, SessionStatus::NeedsInput)
                | (SessionStatus::SubagentsWorking, SessionStatus::Error)
        )
    }

    /// Returns true if the session is still within the cooldown window.
    pub fn is_on_cooldown(&self, session_id: &str) -> bool {
        self.cooldowns
            .get(session_id)
            .is_some_and(|t| t.elapsed() < Duration::from_secs(COOLDOWN_SECS))
    }

    /// Record that we just notified for this session, resetting its cooldown.
    pub fn record_notification(&mut self, session_id: &str) {
        self.cooldowns
            .insert(session_id.to_string(), Instant::now());
    }

    /// Show a desktop notification.
    ///
    /// On macOS we use `notify-rust` only (no shell fallback) so that the
    /// system Do Not Disturb / Focus modes are respected. On Linux we fall
    /// back to `notify-send` if the native path fails.
    pub fn notify(&self, title: &str, body: &str) {
        if !self.enabled {
            return;
        }

        let native = notify_rust::Notification::new()
            .summary(title)
            .body(body)
            .timeout(notify_rust::Timeout::Milliseconds(8000))
            .show();

        if native.is_err() {
            // Shell fallback – Linux only. We intentionally do NOT use osascript
            // on macOS because it bypasses Do Not Disturb.
            #[cfg(target_os = "linux")]
            {
                let _ = std::process::Command::new("notify-send")
                    .arg(title)
                    .arg(body)
                    .spawn();
            }
        }
    }

    /// Build a human-friendly notification body for a transition.
    pub fn format_body(status: SessionStatus) -> &'static str {
        match status {
            SessionStatus::Idle => "Session finished",
            SessionStatus::NeedsInput => "Session needs your input",
            SessionStatus::Error => "Session encountered an error",
            SessionStatus::Working => "Session is working",
            SessionStatus::SubagentsWorking => "Subagents are working",
        }
    }
}
