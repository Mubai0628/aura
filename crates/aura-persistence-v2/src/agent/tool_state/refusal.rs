//! What the model reads when a task-state tool refuses a call: the cause, that nothing was
//! changed, and what to do next. A refusal that corrected arguments fix names the argument and
//! keeps the shared argument-correction wording; one its arguments alone do not decide (the state
//! the call met) has its own words.
use aura_agent_domain::DeterministicFailure;
use aura_domain_common::ErrorCategory::{self, Conflict, Internal, Unavailable, Validation};
use aura_domain_common::ErrorCode;
use aura_tool_contract::{ModelSafeToolErrorV1, ToolRuntimeErrorCode, ToolSchemaErrorCode};

/// How much of a library's own text a message quotes.
const MAX_DETAIL_CHARS: usize = 200;

/// Refusals raised as a bare code: the code, the code the model reads, its category, whether the
/// same call may succeed later, and the words.
const KNOWN: &[(&str, &str, ErrorCategory, bool, &str)] = &[
    (
        "agent.mode_disposition_required",
        "task_lineage.disposition_not_allowed",
        Validation,
        false,
        "This call was not run: the session's special mode needs a decision first. Nothing was \
         changed. Call mode.disposition alone (no other calls in the same response), then repeat \
         this call.",
    ),
    (
        "agent.mode_disposition_alone",
        "task_lineage.disposition_not_allowed",
        Validation,
        false,
        "This call was not run: mode.disposition must be the only call in its response. Nothing \
         was changed. Call mode.disposition alone, then make the other calls in the next \
         response.",
    ),
    (
        "agent.mode_disposition_reason_required",
        "task_lineage.disposition_not_allowed",
        Validation,
        false,
        "mode.disposition needs a reason of 1 to 500 characters. Nothing was decided. Call it \
         again with a short reason.",
    ),
    (
        "agent.mode_disposition_not_waiting",
        "task_lineage.disposition_not_allowed",
        Validation,
        false,
        "mode.disposition: no special-mode decision is waiting in this turn (it was already \
         made, or the mode ended). Nothing was changed. Continue the task without it.",
    ),
    (
        "agent.mode_disposition_turn_ended",
        "tool.batch.control_state_mismatch",
        Conflict,
        false,
        "mode.disposition: this turn has already ended, so there is nothing to decide in it. \
         Nothing was changed.",
    ),
    (
        "query.user_question.pending",
        "tool.batch.control_state_mismatch",
        Conflict,
        false,
        "query.user_question: a question to the user is already waiting for an answer in this \
         turn. Nothing was asked. Wait for that answer; ask further questions together in one \
         call.",
    ),
    (
        "context.read_tool_result.unfinished",
        "tool.batch.control_state_mismatch",
        Conflict,
        true,
        "context.read_tool_result: that call has no result yet; it is still running, or it never \
         finished. Nothing was changed. If it is still running, read its result after it \
         finishes.",
    ),
    (
        "not a task-state handler",
        "tool.runtime.dispatch_failed",
        Internal,
        false,
        "Aura has no handler for this tool here (an internal error in Aura). Nothing was \
         changed. Do it another way, and tell the user if it happens again.",
    ),
    (
        "tools.search_unavailable",
        "tool.registry.snapshot_unavailable",
        Unavailable,
        true,
        "tools.search: the list of tools this conversation can see could not be read for this \
         search. Nothing was changed. Try the search again in a moment.",
    ),
    (
        "tools.internal",
        "tool.registry.snapshot_corrupted",
        Internal,
        false,
        "Aura could not read its own list of tools for this call (an internal error in Aura). \
         Nothing was changed. Try the call once more; if it fails again, continue without it and \
         tell the user.",
    ),
];

/// The refusal for a message that needs its own words rather than the argument-correction text.
pub(super) fn known(message: &str) -> Option<ModelSafeToolErrorV1> {
    let (_, code, category, retryable, text) =
        KNOWN.iter().find(|(known, ..)| *known == message)?;
    Some(ModelSafeToolErrorV1 {
        code: ErrorCode::new(*code).expect("registered syntax"),
        category: *category,
        message: (*text).to_owned(),
        retryable: *retryable,
    })
}

/// The refusal for a task-state call whose state did not match what it was read as: a revision
/// or identity conflict the call's own arguments do not explain.
pub(super) fn conflict(tool: &str) -> ModelSafeToolErrorV1 {
    let next = match tool {
        "todo.get" | "todo.replace" | "todo.update" => {
            "Read the to-do with todo.get, then call again if it is still needed."
        }
        "goal.get" | "goal.check" => {
            "Read the goal with goal.get, then call again if it is still needed."
        }
        "task_lineage.list_open" | "task_lineage.read_sources" | "task_lineage.reconcile" => {
            "Reload task_lineage.list_open, then call again with the ids and revisions it shows."
        }
        "mode.disposition" => {
            "Continue the task; call mode.disposition again only if a decision is still asked for."
        }
        "context.read_tool_result" => {
            "The result may belong to a sub-agent's run, which this conversation cannot read; \
             otherwise call again."
        }
        tool if tool.starts_with("context.") || tool.starts_with("history.") => {
            "Call it again without a cursor."
        }
        tool if tool.starts_with("tools.") => "Search the tools again with tools.search.",
        _ => "Check the current state, then call it again if it is still needed.",
    };
    ModelSafeToolErrorV1 {
        code: ErrorCode::new("tool.batch.control_state_mismatch").expect("registered syntax"),
        category: Conflict,
        message: format!(
            "{tool} was refused: what it works on changed or ended while it ran (a revision or \
             identity conflict). Nothing was changed. {next}"
        ),
        retryable: false,
    }
}

/// The refusal for arguments that do not decode as the tool's input.
///
/// A JSON value decoded into a type reports no line, and the handlers decode their arguments
/// from a value; a record parsed from stored text reports its line, so it is not taken for one.
pub(super) fn arguments(tool: &str, error: &serde_json::Error) -> Option<ModelSafeToolErrorV1> {
    if error.line() != 0 || !error.is_data() {
        return None;
    }
    let text = error.to_string();
    let detail = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>();
    let detail = if detail.chars().count() > MAX_DETAIL_CHARS {
        format!("{}…", detail.chars().take(MAX_DETAIL_CHARS - 1).collect::<String>())
    } else {
        detail
    };
    Some(ModelSafeToolErrorV1 {
        code: ToolSchemaErrorCode::ArgumentsInvalid.error_code(),
        category: Validation,
        message: format!(
            "{tool}: the arguments do not match this tool's input: {}. Nothing was changed. \
             Correct them and call again.",
            detail.trim().trim_end_matches('.')
        ),
        retryable: false,
    })
}

/// `detail` follows the message: what this session's records tell about it, or nothing.
pub(super) fn classify_invalid(
    message: &'static str,
    detail: &str,
) -> Option<(ModelSafeToolErrorV1, Option<DeterministicFailure>)> {
    let failure = lineage_failure(message);
    // A refusal its arguments alone do not decide has its own words; a repeated one still counts
    // towards the turn's correction limit.
    if let Some(error) = known(message) {
        return Some((error, failure));
    }
    let (code, category) = match message {
        "task_lineage.arguments_invalid"
        | "task_lineage.constraint.expire_forbids_policy"
        | "task_lineage.constraint.expire_forbids_relations"
        | "task_lineage.constraint.reference_forbids_policy"
        | "task_lineage.constraint.cancel_requires_excerpt" => {
            (ToolSchemaErrorCode::ArgumentsInvalid.error_code(), ErrorCategory::Validation)
        }
        "task_lineage.identity_mismatch" | "task_lineage.target_not_in_session" => (
            ErrorCode::new("tool.batch.identity_mismatch").expect("registered syntax"),
            ErrorCategory::Validation,
        ),
        "task_lineage.revision_stale" => (
            ErrorCode::new("tool.batch.control_state_mismatch").expect("registered syntax"),
            ErrorCategory::Conflict,
        ),
        "task_lineage.disposition_not_allowed" | "task_lineage.current_required" => (
            ErrorCode::new("task_lineage.disposition_not_allowed").expect("registered syntax"),
            ErrorCategory::Validation,
        ),
        // The workspace or the service the call needs is not there now; the same call may work
        // later, so it is not an argument mistake.
        other if NOT_NOW.iter().any(|code| other.starts_with(code)) => {
            (ToolRuntimeErrorCode::Unavailable.error_code(), ErrorCategory::Unavailable)
        }
        other
            if [
                "tools.",
                "task_lineage.",
                "skills.",
                "plugins.",
                "shell.",
                "todo.",
                "goal.",
                "query.",
                "activity.",
                "context.",
                "history.",
                "tool.arguments_invalid",
            ]
            .iter()
            .any(|prefix| other.starts_with(prefix)) =>
        {
            (ToolSchemaErrorCode::ArgumentsInvalid.error_code(), ErrorCategory::Validation)
        }
        _ => return None,
    };
    let retryable = category == ErrorCategory::Unavailable;
    // The handler's changes were rolled back, so the call changed nothing. A family that words its
    // refusals in Chinese already says what it left as it was and what to do next.
    let text = failure.as_ref().map(DeterministicFailure::model_message).unwrap_or_else(|| {
        if chinese(message) {
            format!("{message}。{detail}")
        } else {
            format!("{message}.{detail} Nothing was changed. retryable:false means the identical arguments will fail; corrected arguments may be submitted.")
        }
    });
    Some((ModelSafeToolErrorV1 { code, category, message: text, retryable }, failure))
}

/// Refusals for a workspace or a service that is not there now, which a later call may find.
const NOT_NOW: &[&str] = &[
    "skills.workspace_unavailable",
    "skills.unavailable_now",
    "plugins.workspace_unavailable",
    "plugins.unavailable_now",
];

fn chinese(text: &str) -> bool {
    text.chars().any(|ch| (0x4e00..=0x9fff).contains(&u32::from(ch)))
}

fn lineage_failure(message: &str) -> Option<DeterministicFailure> {
    Some(match message {
        "task_lineage.constraint.expire_forbids_policy" => DeterministicFailure::schema_constraint(
            "task_lineage.reconcile.completion_policy_update",
            "expire_prior forbids completion_policy_update",
            "completion_policy_update set",
            "Omit completion_policy_update when marking a prior lineage expired.",
        ),
        "task_lineage.constraint.expire_forbids_relations" => {
            DeterministicFailure::schema_constraint(
                "task_lineage.reconcile.relations",
                "expire_prior forbids relations",
                "relations not empty",
                "Omit relations when marking a prior lineage expired.",
            )
        }
        "task_lineage.constraint.reference_forbids_policy" => {
            DeterministicFailure::schema_constraint(
                "task_lineage.reconcile.completion_policy_update",
                "reference_prior forbids completion_policy_update",
                "completion_policy_update set",
                "Omit completion_policy_update when only referencing a prior lineage.",
            )
        }
        "task_lineage.constraint.cancel_requires_excerpt" => {
            DeterministicFailure::schema_constraint(
                "task_lineage.reconcile.relations",
                "cancel_existing requires a later excerpt with kind cancels_task",
                "missing cancels_task excerpt",
                "Supply a later_excerpt on a cancels_task relation grounded in the current user message.",
            )
        }
        "task_lineage.arguments_invalid" => DeterministicFailure::schema_constraint(
            "task_lineage.reconcile",
            "arguments must satisfy the published combination rules",
            "invalid combination",
            "Resubmit only the decision, optional grounded relations, and reason. Core binds identity and revision.",
        ),
        "task_lineage.disposition_not_allowed" => DeterministicFailure::disposition(
            "task_lineage.reconcile.decision",
            "interrupted and topic-switched lineages cannot become Active; expire or reference them from the current task",
            "disallowed reactivation or target",
            "Keep the current Active lineage. Use expire_prior or reference_prior for background records.",
        ),
        "task_lineage.identity_mismatch" | "task_lineage.target_not_in_session" => {
            DeterministicFailure::disposition(
                "task_lineage.reconcile",
                "target must belong to this session and the current user event",
                message,
                "Reload task_lineage.list_open and use an id from this session.",
            )
        }
        "task_lineage.revision_stale" => DeterministicFailure::disposition(
            "task_lineage.reconcile",
            "the bound lineage revision must match the live record",
            "stale revision",
            "Reload current task state and resubmit. Do not copy a previous revision.",
        ),
        // Counted on its own, so a repeated decision with none waiting stops the turn as before;
        // the model reads the refusal's own words.
        "agent.mode_disposition_not_waiting" => DeterministicFailure::disposition(
            "mode.disposition",
            "a special-mode decision must be waiting in this turn",
            "no decision waiting",
            "Continue the task without mode.disposition.",
        ),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A refusal its arguments do not decide reads in its own words and, when the same call can
    /// come again, still counts towards the turn's correction limit; one that corrected
    /// arguments fix keeps the shared wording and says that nothing was changed.
    #[test]
    fn refusals_read_as_words_and_keep_their_codes() {
        let (error, failure) =
            classify_invalid("agent.mode_disposition_not_waiting", "").expect("classified");
        assert_eq!(error.code.as_str(), "task_lineage.disposition_not_allowed");
        assert!(error.message.starts_with("mode.disposition: no special-mode decision"));
        assert!(failure.is_some(), "a repeated decision with none waiting still counts");
        let (error, failure) =
            classify_invalid("goal.expected_goal_revision: it is stale", "").expect("classified");
        assert_eq!(error.code.as_str(), "tool.schema.arguments_invalid");
        assert!(error.message.contains("Nothing was changed."), "{}", error.message);
        assert!(failure.is_none());
        let cited = "tool.arguments_invalid: an artifact id in evidence_refs is not one of this \
                     run's results";
        let (error, _) = classify_invalid(cited, "").expect("classified");
        assert_eq!(error.code.as_str(), "tool.schema.arguments_invalid");
        assert!(classify_invalid("persistent detail", "").is_none());
    }

    /// A Skill or plugin refusal for a workspace or service that is not there now may succeed
    /// later: it is not an argument mistake, and its own Chinese sentence is not followed by the
    /// argument-correction text.
    #[test]
    fn a_refusal_for_what_is_not_there_now_may_be_tried_again() {
        let (error, _) = classify_invalid(
            "skills.unavailable_now: Skill 功能现在没有在运行，什么都没读。请稍后再试；反复出现时告诉用户",
            "",
        )
        .expect("classified");
        assert_eq!(error.code.as_str(), "tool.runtime.unavailable");
        assert!(error.retryable);
        assert!(error.message.ends_with("告诉用户。"), "{}", error.message);
        assert!(!error.message.contains("retryable:false"), "{}", error.message);
        let (error, _) = classify_invalid(
            "plugins.arguments_invalid: plugins.logs 只接受 name（插件清单里的名字）和可选的 limit（条数，默认 50，最多 200），什么都没读",
            "",
        )
        .expect("classified");
        assert_eq!(error.code.as_str(), "tool.schema.arguments_invalid");
        assert!(!error.retryable);
        assert!(error.message.ends_with("什么都没读。"), "{}", error.message);
    }

    /// Each code-only refusal reads as words with what was left as it was, never as its code or
    /// the argument-correction text, and keeps the code the model read for it before.
    #[test]
    fn a_code_only_refusal_reads_as_words() {
        for (message, ..) in KNOWN {
            let error = known(message).expect("known");
            assert!(!error.message.contains(message), "{message}: {}", error.message);
            assert!(!error.message.contains("retryable:false"), "{}", error.message);
            assert!(error.message.contains("Nothing was"), "{}", error.message);
        }
        let batched = known("agent.mode_disposition_alone").expect("known");
        assert_eq!(batched.code.as_str(), "task_lineage.disposition_not_allowed");
        assert!(batched.message.contains("only call in its response"), "{}", batched.message);
        assert!(known("todo.title: blank").is_none());
    }

    /// Arguments that do not decode name the field and the problem; a stored record that does
    /// not parse is not taken for the call's arguments.
    #[test]
    fn arguments_that_do_not_decode_name_the_problem() {
        #[derive(Debug, serde::Deserialize)]
        #[allow(dead_code)]
        struct Input {
            title: String,
        }
        let decoded = serde_json::from_value::<Input>(serde_json::json!({"name": 1}))
            .expect_err("a missing field");
        let error = arguments("todo.replace", &decoded).expect("the call's arguments");
        assert_eq!(error.code.as_str(), "tool.schema.arguments_invalid");
        assert!(error.message.starts_with("todo.replace: the arguments do not match"));
        assert!(error.message.contains("missing field `title`"), "{}", error.message);
        let parsed = serde_json::from_str::<Input>("{\"name\": 1}").expect_err("stored text");
        assert!(arguments("todo.replace", &parsed).is_none());
    }

    /// A conflict names the tool, that nothing changed, and what to read again.
    #[test]
    fn a_conflict_names_what_to_read_again() {
        let error = conflict("goal.check");
        assert_eq!(error.code.as_str(), "tool.batch.control_state_mismatch");
        assert!(error.message.starts_with("goal.check was refused"), "{}", error.message);
        assert!(error.message.contains("Nothing was changed. Read the goal with goal.get"));
    }
}
