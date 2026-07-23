use opencode_multiplexer::app::sessions::SessionStatus;
use opencode_multiplexer::notify::Notifier;
use std::thread;
use std::time::Duration;

#[test]
fn working_to_idle_is_interesting() {
    assert!(Notifier::is_interesting_transition(
        SessionStatus::Working,
        SessionStatus::Idle
    ));
}

#[test]
fn working_to_needs_input_is_interesting() {
    assert!(Notifier::is_interesting_transition(
        SessionStatus::Working,
        SessionStatus::NeedsInput
    ));
}

#[test]
fn working_to_error_is_interesting() {
    assert!(Notifier::is_interesting_transition(
        SessionStatus::Working,
        SessionStatus::Error
    ));
}

#[test]
fn idle_to_working_is_not_interesting() {
    assert!(!Notifier::is_interesting_transition(
        SessionStatus::Idle,
        SessionStatus::Working
    ));
}

#[test]
fn same_status_is_not_interesting() {
    assert!(!Notifier::is_interesting_transition(
        SessionStatus::Working,
        SessionStatus::Working
    ));
}

#[test]
fn cooldown_prevents_duplicate_notifications() {
    let mut notifier = Notifier::new(true);
    notifier.record_notification("sess_1");
    assert!(notifier.is_on_cooldown("sess_1"));
    assert!(!notifier.is_on_cooldown("sess_2"));
}

#[test]
fn cooldown_expires_after_interval() {
    let mut notifier = Notifier::new(true);
    notifier.record_notification("sess_1");
    assert!(notifier.is_on_cooldown("sess_1"));
    thread::sleep(Duration::from_secs(6));
    assert!(!notifier.is_on_cooldown("sess_1"));
}

#[test]
fn disabled_notifier_does_not_notify() {
    let notifier = Notifier::new(false);
    // This should simply not panic and do nothing.
    notifier.notify("title", "body");
}

#[test]
fn format_body_varies_by_status() {
    assert_eq!(
        Notifier::format_body(SessionStatus::Idle),
        "Session finished"
    );
    assert_eq!(
        Notifier::format_body(SessionStatus::NeedsInput),
        "Session needs your input"
    );
    assert_eq!(
        Notifier::format_body(SessionStatus::Error),
        "Session encountered an error"
    );
    assert_eq!(
        Notifier::format_body(SessionStatus::Working),
        "Session is working"
    );
    assert_eq!(
        Notifier::format_body(SessionStatus::SubagentsWorking),
        "Subagents are working"
    );
}

#[test]
fn subagents_working_to_idle_is_interesting() {
    assert!(Notifier::is_interesting_transition(
        SessionStatus::SubagentsWorking,
        SessionStatus::Idle
    ));
}

#[test]
fn subagents_working_to_needs_input_is_interesting() {
    assert!(Notifier::is_interesting_transition(
        SessionStatus::SubagentsWorking,
        SessionStatus::NeedsInput
    ));
}

#[test]
fn subagents_working_to_error_is_interesting() {
    assert!(Notifier::is_interesting_transition(
        SessionStatus::SubagentsWorking,
        SessionStatus::Error
    ));
}
