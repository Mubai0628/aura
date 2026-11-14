//! What the model reads when a Shell command did not start.
use aura_backend_windows_user::NotStarted;
use aura_persistence_v2::tool::refusal_text;

const NOT_STARTED: &str = "command.not_started";
const AGAIN_OR_TELL: &str = "Try again; if it fails again, tell the user.";

/// The words for a Shell command recorded as never started: the launch path's code first, then
/// the cause, what that means (nothing ran) and the next step. Only the bounded code is used,
/// never the OS or Runner text behind it.
pub(super) fn message(not_started: Option<NotStarted>, code: Option<&str>) -> String {
    let named = code.unwrap_or(NOT_STARTED);
    let (cause, next) = match code {
        Some("compat.launch_failed") => {
            ("Windows could not create the command's process", "Check the command, then try again.")
        }
        Some("compat.process_policy_failed") => {
            ("Aura could not read the process rules of this session", AGAIN_OR_TELL)
        }
        Some("command.mode_changed_before_start") => (
            "the session's permission mode changed before it started",
            "Call it again; it is checked against the new mode.",
        ),
        Some("command.attempt_changed_before_start") => (
            "its start was superseded before it ran (the turn was stopped, or the call was started again)",
            "Call it again if it is still needed.",
        ),
        Some("runner.launch_window_expired") => {
            ("its start window closed before it could launch", "Call it again.")
        }
        Some(other) => {
            if let Some(sentence) = refusal_text::known(other) {
                return format!("{other}: {sentence}");
            }
            ("Aura refused to start it at the last check before launch", AGAIN_OR_TELL)
        }
        None if not_started == Some(NotStarted::Expired) => {
            ("it waited too long for its turn and its start window closed", "Call it again.")
        }
        None => ("Aura refused to start it at the last check before launch", AGAIN_OR_TELL),
    };
    format!(
        "{named}: The command did not start: {cause}. Nothing ran and nothing was changed. {next}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_launch_failure_names_its_cause_and_that_nothing_ran() {
        let said = message(Some(NotStarted::Rejected), Some("compat.launch_failed"));
        assert!(
            said.starts_with("compat.launch_failed: The command did not start: Windows"),
            "{said}"
        );
        assert!(said.contains("Nothing ran and nothing was changed."), "{said}");
        assert!(!said.contains("unknown effect"), "{said}");
    }

    #[test]
    fn a_command_without_a_code_still_says_why_it_did_not_start() {
        let expired = message(Some(NotStarted::Expired), None);
        assert!(
            expired.starts_with("command.not_started: The command did not start: it waited"),
            "{expired}"
        );
        let unnamed = message(Some(NotStarted::Rejected), Some("runner.something_new"));
        assert!(
            unnamed.starts_with("runner.something_new: The command did not start: "),
            "{unnamed}"
        );
        let known = message(Some(NotStarted::Rejected), Some("agent.directory_grant_not_in_force"));
        assert!(
            known.starts_with("agent.directory_grant_not_in_force: This call was not started"),
            "{known}"
        );
    }
}
