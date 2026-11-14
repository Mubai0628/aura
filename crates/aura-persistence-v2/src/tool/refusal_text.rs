//! Why a call did not start, in words the model reads, from the reason a refusal carries. One
//! table serves the calls Core refuses when a dispatch fails and the calls a backend refuses as
//! a prepared rejection; the raw reason (an operation identity, a Rust value, an internal
//! message) never reaches the model.

/// Reason patterns and the sentence for each, most specific first: a reason that holds a
/// longer code is matched before one holding a code it contains.
const TABLE: &[(&[&str], &str)] = &[
    (
        &["agent.pending_addition"],
        "This call was not started: the user added a message to this turn while it was being \
         checked, and it comes in your next step. Nothing was changed. Read it, then decide \
         whether the task is done before you end the turn.",
    ),
    (
        &["agent.reservation", "agent.cancelled_or_changed"],
        "This call was not started: what it needed changed between its approval and its start. \
         Nothing was changed. Call it again if it is still needed.",
    ),
    (
        &[
            "agent.resource_changed",
            "prepared domain facts changed",
            "agent.source_or_scope_changed",
            "agent.file_changed",
        ],
        "This call was not started: its target changed between being prepared and being started \
         (another call of the same response, or another program, may have been changing it at \
         the same moment). Nothing was changed. Check the target, then call it again if it is \
         still needed.",
    ),
    (
        &["agent.mode_changed", "agent.permission_withdrawn"],
        "This call was not started: the session's permission mode changed before it started. \
         Nothing was changed. Call it again if it is still needed; it is checked against the new \
         mode.",
    ),
    (
        &[
            "compat.scope_changed",
            "compat.dispatch_revalidation_failed",
            "orchestrator.preparation_changed",
            "orchestrator.backend_changed",
        ],
        "This call was not started: the workspace, its permissions or what runs the call changed \
         after it was prepared. Nothing was changed. Call it again.",
    ),
    (
        &["compat.runtime_unavailable"],
        "This call was not started: Aura's managed shell is not available in this run of Aura \
         (its installation check did not pass). Nothing was changed. Tell the user; restarting \
         Aura may be needed.",
    ),
    (
        &["compat.preparation_failed"],
        "This call was not started: Aura could not prepare the shell to run it. Nothing was \
         changed. Try again; if it fails again, tell the user.",
    ),
    (
        &["compat.unsupported"],
        "This call was not started: the shell set up for this workspace cannot run it. Nothing \
         was changed. Do it another way.",
    ),
    (
        &["agent.directory_grant_not_in_force"],
        "This call was not started: the access the user granted to that folder has ended. \
         Nothing was changed. Ask the user to grant it again if it is still needed.",
    ),
    (
        &["agent.registry_changed", "tool.registry.version_mismatch", "orchestrator.plan_invalid"],
        "This call was not started: the tool list changed after this response was written. \
         Nothing was changed. Call it again.",
    ),
    (
        &["agent.tool_disabled"],
        "This call was not started: the user turned this tool off after it was called. Nothing \
         was changed. Do it another way.",
    ),
    (
        &["agent.scope_refused"],
        "This call was not started: Aura could not confirm that it belongs to a running task of \
         this conversation (its task may have ended). Nothing was changed. Call it again if it \
         is still needed.",
    ),
    (
        &["agent.registry_authority_unavailable"],
        "This call was not started: Aura could not confirm the tool's registration. Nothing was \
         changed. Try again in a moment.",
    ),
    (
        &["orchestrator.authorization_invalid"],
        "This call was not started: its permission was no longer valid when it was about to \
         start. Nothing was changed. Call it again; it may ask for approval.",
    ),
    (
        &["agent.resource_wait_exhausted"],
        "This call was not started: another running call held the same file longer than the \
         wait limit, so this call did nothing. Try again after that call finishes.",
    ),
    (
        &["git.remote_changed"],
        "This push or sync was not started: the branch or remote changed after it was \
         approved. Nothing was sent or fetched. Call it again to approve the current target.",
    ),
    (
        &["agent.file_parent_not_directory"],
        "This call was not started: part of the folder path is a file, not a folder. Nothing was changed. Check the path.",
    ),
    (
        &["agent.file_parent_missing"],
        "This call was not started: the path names no folder to hold the file. Nothing was \
         changed. Give the full path of the file, including its folder.",
    ),
    (
        &["agent.file_leaf_missing"],
        "This call was not started: the path does not end in a file or folder name. Nothing was \
         changed. Give the full path, including the name.",
    ),
    (
        &["agent.native_name_unverified"],
        "This call was not started: Aura could not confirm the file's real name on disk. Nothing was changed. Check the path's spelling and case.",
    ),
    (
        &["agent.worker_terminated", "agent.batch_worker", "orchestrator.control_backend"],
        "This call was not started: an internal error in Aura stopped it before it ran. Nothing \
         was changed. Try it again; if it fails again, tell the user.",
    ),
    (
        &[
            "agent.not_running_batch",
            "agent.call_already_finished",
            "orchestrator.cancelled_or_schedule_invalid",
            "orchestrator.cancelled_before_reservation",
            "orchestrator.control_cancelled",
            "agent.cancelled",
        ],
        "This call was not started: the turn it belonged to was being stopped. Nothing was \
         changed. Call it again if it is still needed.",
    ),
];

/// The sentence for a refusal whose reason text holds a code this table knows.
#[must_use]
pub fn known(reason: &str) -> Option<&'static str> {
    TABLE
        .iter()
        .find(|(patterns, _)| patterns.iter().any(|pattern| reason.contains(pattern)))
        .map(|(_, sentence)| *sentence)
}

/// A line that already says, as `<code>: <sentence>`, why a call did not start: the words a
/// family's own refusal gives the model, kept as they are.
#[must_use]
pub fn readable_line(text: &str) -> Option<&str> {
    let line = text.lines().next()?.trim();
    let (code, rest) = line.split_once(':')?;
    let rest = rest.trim();
    let worded = rest.chars().count() >= 8
        && (rest.contains(' ') || rest.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)));
    (is_code(code)
        && worded
        && line.len() <= 512
        && !line.contains(['{', '}'])
        && !line.chars().any(char::is_control))
    .then_some(line)
}

/// The first code-like token of `text` (`family.reason`), to name an unknown reason.
#[must_use]
pub fn code_in(text: &str) -> Option<&str> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '_'))
        .map(|token| token.trim_matches('.'))
        .find(|token| is_code(token))
}

fn is_code(token: &str) -> bool {
    let token = token.trim();
    token.len() <= 96
        && token.contains('.')
        && token.starts_with(|c: char| c.is_ascii_lowercase())
        && token.split('.').all(|part| {
            !part.is_empty()
                && part.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reservation_change_is_not_read_as_a_stopped_turn() {
        let said = known("agent.reservation:op_1:agent.cancelled_or_changed").expect("known");
        assert!(said.contains("between its approval and its start"), "{said}");
        assert!(known("agent.cancelled").expect("known").contains("being stopped"));
    }

    /// Each refusal reads as what really stopped it: a task that may have ended, a path without a
    /// folder or a name, a control call whose turn was being stopped or an internal error.
    #[test]
    fn a_refusal_names_what_really_stopped_the_call() {
        let task = known("agent.scope_refused: tool.scope.task_ended").expect("known");
        assert!(task.contains("running task") && !task.contains("outside"), "{task}");
        let folder = known("agent.file_parent_missing").expect("known");
        assert!(
            folder.contains("names no folder") && !folder.contains("does not exist"),
            "{folder}"
        );
        let name = known("agent.file_leaf_missing").expect("known");
        assert!(name.contains("file or folder name") && !name.contains("does not exist"), "{name}");
        assert!(known("orchestrator.control_cancelled").expect("known").contains("being stopped"));
        assert!(known("orchestrator.control_backend").expect("known").contains("internal error"));
    }

    #[test]
    fn a_familys_own_sentence_is_kept_and_a_rust_value_is_not() {
        let line = "agents.reported: 你已经向主 Agent 提了问题，这一轮不能再调用工具。";
        assert_eq!(readable_line(line), Some(line));
        assert_eq!(readable_line("PreparationRejected { reason: \"x.y\" }"), None);
        assert_eq!(readable_line("agent.control.preflight"), None);
        assert_eq!(readable_line("compat.preparation_failed: io"), None, "too short to explain");
    }

    #[test]
    fn an_unknown_reason_is_named_by_its_code() {
        assert_eq!(
            code_in("dispatch was rejected before execution: browser.unsupported_operation"),
            Some("browser.unsupported_operation")
        );
        assert_eq!(code_in("broker class"), None);
    }
}
