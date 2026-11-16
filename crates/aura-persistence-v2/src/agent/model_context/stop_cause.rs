//! What stopped a turn before it finished, from its stop reason and the facts written with the
//! stop: the provider's failure of the run's last step, the run's own failure record, the
//! decision of the user's Hook that blocked it, or the request to stop it and the note that
//! request left. The model reads it in English in the note of the
//! stopped turn, with the next step when one helps; a sub-agent's report, which the user reads
//! too, says it in Chinese. Every fact read here is written in the transaction that stops the
//! run and never changes afterwards, so the note is the same whenever it is built.
use crate::model::{failure_text, transient_retry};
use aura_domain_common::AgentRunId;
use aura_model_domain::{ModelFailureCategory, NormalizedModelFailure, PersistedModelFailure};
use aura_persistence_core::StoreResult;
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

/// The fact a request to stop a run leaves when it says who stopped the run and why.
pub(crate) const STOP_NOTE: &str = "agent.stop_note";

/// Who stopped a run and why, in English, when it was neither the user nor Aura shutting down:
/// the clause after "stopped here", and what to do when the work goes on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StopNote {
    pub clause: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

/// When a user's Hook stopped a turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookStop {
    BeforeTurn,
    BeforeStep,
    AtCompletion,
}

/// What stopped a turn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StopCause {
    Restarted,
    UserStopped,
    /// Aura shut down while the run was going: its stop fired with no request to stop.
    Shutdown,
    /// A stop that left a note saying who stopped the run and why.
    StoppedBy(StopNote),
    ModePaused,
    ModeEnded,
    /// A user's Hook blocked the turn; its name and reason, when they were kept.
    Hook {
        when: HookStop,
        block: Option<(String, String)>,
    },
    NoFinishingCall,
    RepeatedMistake,
    /// The provider failed the run's last step; `asked_again` when a step of it was retried.
    Provider {
        failure: NormalizedModelFailure,
        asked_again: bool,
    },
    GoalBudget,
    ToolOutcomeUnknown,
    ModelUnavailable,
    ModelDisabled,
    ConnectionChanged,
    BranchCopying,
    CompactionFailed,
    CompactionNoProgress,
    Internal,
    /// A stop reason this list does not know.
    Other(String),
}

/// What stopped run `run`, whose stop fact gave `reason` and `cancelled`.
pub fn stop_cause(
    db: &Connection,
    run: AgentRunId,
    reason: &str,
    cancelled: bool,
) -> StoreResult<StopCause> {
    Ok(match reason {
        "agent.core_restarted" if !cancelled => StopCause::Restarted,
        "agent.cancelled" | "agent.core_restarted" => stopped(db, run)?,
        "agent.mode_paused" => StopCause::ModePaused,
        "agent.mode_terminated" => StopCause::ModeEnded,
        "hooks.turn_blocked" => StopCause::Hook {
            when: HookStop::BeforeTurn,
            block: hook_block(db, run, &["turn.before_start"])?,
        },
        "hooks.step_blocked" => StopCause::Hook {
            when: HookStop::BeforeStep,
            block: hook_block(db, run, &["context.before_build", "model.before_call"])?,
        },
        "hooks.completion_blocked" => StopCause::Hook {
            when: HookStop::AtCompletion,
            block: hook_block(db, run, &["turn.before_complete"])?,
        },
        "agent.protocol_not_completed" => StopCause::NoFinishingCall,
        "agent.deterministic_correction_exhausted" => StopCause::RepeatedMistake,
        "agent.execution_interrupted" => execution_stop(db, run)?,
        other => StopCause::Other(other.to_owned()),
    })
}

impl StopCause {
    /// As a clause for the model.
    #[must_use]
    pub fn english(&self) -> String {
        match self {
            Self::Restarted => "Aura restarted while it was running".into(),
            Self::UserStopped => "the user stopped it".into(),
            Self::Shutdown => "Aura shut down while it was running".into(),
            Self::StoppedBy(note) => note.clause.clone(),
            Self::ModePaused => "the user paused its mode".into(),
            Self::ModeEnded => "the user ended its mode".into(),
            Self::Hook { when, block: Some((hook, reason)) } => match when {
                HookStop::BeforeTurn => {
                    format!("the user's Hook {hook} blocked it before it began: {reason}")
                }
                HookStop::BeforeStep => {
                    format!("the user's Hook {hook} blocked its next step: {reason}")
                }
                HookStop::AtCompletion => format!(
                    "the user's Hook {hook} did not accept it as finished; its last reason was: {reason}"
                ),
            },
            Self::Hook { when, block: None } => match when {
                HookStop::BeforeTurn => "one of the user's Hooks blocked it before it began",
                HookStop::BeforeStep => "one of the user's Hooks blocked its next step",
                HookStop::AtCompletion => "one of the user's Hooks did not accept it as finished",
            }
            .into(),
            Self::NoFinishingCall => {
                "it ended twice without the call that finishes a turn in its mode, even after a reminder"
                    .into()
            }
            Self::RepeatedMistake => {
                "the same mistake in a tool call's arguments came again after it had been pointed out twice"
                    .into()
            }
            Self::Provider { failure, asked_again } => {
                let again = if *asked_again { ", also when asked again" } else { "" };
                match (failure.code.as_str(), failure.category) {
                    ("model_reasoning_loop", _) => format!(
                        "the model's reasoning for its last step kept returning to the same passage without starting an answer{again}"
                    ),
                    (_, ModelFailureCategory::Persistence) => {
                        format!("Aura could not record the answer to its last step{again}")
                    }
                    _ => format!(
                        "the model provider could not answer its last step ({}{again})",
                        failure_text::cause(failure)
                    ),
                }
            }
            Self::GoalBudget => "the Goal's token budget is used up, so no further step was sent".into(),
            Self::ToolOutcomeUnknown => {
                "Aura could not tell whether one of its tool calls took effect".into()
            }
            Self::ModelUnavailable => {
                "no model is chosen for this conversation any more, so its next step was not sent"
                    .into()
            }
            Self::ModelDisabled => {
                "the model chosen for this conversation was disabled, so its next step was not sent"
                    .into()
            }
            Self::ConnectionChanged => {
                "the model's connection settings changed while it was running, so its next step was not sent"
                    .into()
            }
            Self::BranchCopying => {
                "the history of this branch had not finished copying, so its first step was not sent"
                    .into()
            }
            Self::CompactionFailed => {
                "the summary for compacting the history could not be made, so the history was not compacted"
                    .into()
            }
            Self::CompactionNoProgress => {
                "compacting again would not have made the history smaller, so it was left as it was"
                    .into()
            }
            Self::Internal => "an internal error in Aura stopped it while it was running".into(),
            Self::Other(reason) => format!("it stopped before it finished ({reason})"),
        }
    }

    /// What the model may do about it next, when something helps.
    #[must_use]
    pub fn next_step(&self) -> Option<&str> {
        Some(match self {
            Self::StoppedBy(note) => return note.next.as_deref(),
            Self::Provider { failure, .. } => match (failure.code.as_str(), failure.category) {
                ("model_transport_streamidle_timeout", _) => {
                    "None of that answer was kept and none of its tool calls ran. If it was one very large tool call, split its content into several smaller calls."
                }
                ("model_transport_total_timeout", _) => {
                    "None of that answer was kept. Keep the answer shorter, and split large content into several smaller calls."
                }
                ("model_reasoning_loop", _) => {
                    "None of it was kept. Decide the next step early and keep the reasoning short."
                }
                (
                    _,
                    ModelFailureCategory::Authentication
                    | ModelFailureCategory::Authorization
                    | ModelFailureCategory::QuotaOrBalance,
                ) => {
                    "The user has to fix the connection or its account before another step can be sent."
                }
                _ => return None,
            },
            Self::Hook { .. } => "Do what the Hook asks, or tell the user why it cannot be done.",
            Self::GoalBudget => {
                "Tell the user what is done and what remains; the user can raise the budget or end the Goal."
            }
            Self::ToolOutcomeUnknown => {
                "Check that call's effect before doing it again; context.read_tool_result reads what is known."
            }
            Self::ModelUnavailable => "The user needs to choose a model for this conversation.",
            Self::ModelDisabled => "The user needs to re-enable that model or choose another one.",
            Self::CompactionFailed | Self::CompactionNoProgress => {
                "The history above is as it was."
            }
            Self::Internal => "If it happens again, tell the user.",
            _ => return None,
        })
    }

    /// As a clause for a report the user reads too.
    #[must_use]
    pub fn chinese(&self) -> String {
        match self {
            Self::Restarted => "运行中 Aura 重启了".into(),
            Self::UserStopped => "用户停止了它".into(),
            Self::Shutdown => "运行中 Aura 关闭了".into(),
            Self::StoppedBy(_) => "它被停下了".into(),
            Self::ModePaused => "用户暂停了它的模式".into(),
            Self::ModeEnded => "用户结束了它的模式".into(),
            Self::Hook { when, block: Some((hook, reason)) } => match when {
                HookStop::BeforeTurn => format!("用户的 Hook {hook} 在开始前拦下了它：{reason}"),
                HookStop::BeforeStep => format!("用户的 Hook {hook} 拦下了它的下一步：{reason}"),
                HookStop::AtCompletion => format!("用户的 Hook {hook} 不认可它已完成：{reason}"),
            },
            Self::Hook { when, block: None } => match when {
                HookStop::BeforeTurn => "用户的一个 Hook 在开始前拦下了它",
                HookStop::BeforeStep => "用户的一个 Hook 拦下了它的下一步",
                HookStop::AtCompletion => "用户的一个 Hook 不认可它已完成",
            }
            .into(),
            Self::NoFinishingCall => {
                "它两次结束时都没有调用这个模式结束一轮要用的工具，提醒后也一样".into()
            }
            Self::RepeatedMistake => "同一个工具参数错误指出两次后又出现了".into(),
            Self::Provider { failure, asked_again } => format!(
                "{}{}",
                failure_text::cause_zh(failure),
                if *asked_again { "，重试后也一样" } else { "" }
            ),
            Self::GoalBudget => "目标的 token 预算用完了，没有再发下一步".into(),
            Self::ToolOutcomeUnknown => "Aura 无法确定它的一个工具调用是否生效".into(),
            Self::ModelUnavailable => "这个会话不再有选中的模型，下一步没有发出".into(),
            Self::ModelDisabled => "这个会话选中的模型已停用，下一步没有发出".into(),
            Self::ConnectionChanged => "模型的连接设置在运行中变了，下一步没有发出".into(),
            Self::BranchCopying => "分支的历史还没复制完，第一步没有发出".into(),
            Self::CompactionFailed => "压缩历史用的总结没能生成，历史没有压缩".into(),
            Self::CompactionNoProgress => "再压缩也不会让历史变短，历史保持原样".into(),
            Self::Internal => "Aura 内部出错，中断了它".into(),
            Self::Other(reason) => format!("它没做完就停了（{reason}）"),
        }
    }
}

/// The provider's failure of the run's last step, when that is what stopped it.
fn last_provider_failure(
    db: &Connection,
    run: AgentRunId,
) -> StoreResult<Option<NormalizedModelFailure>> {
    let payload: Option<String> = db
        .query_row(
            "SELECT payload FROM events WHERE kind='model.failure' AND owner=(SELECT id FROM model_steps WHERE agent_run_id=?1 ORDER BY ordinal DESC LIMIT 1) ORDER BY sequence DESC LIMIT 1",
            [run.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    Ok(match payload {
        Some(payload) => Some(serde_json::from_str::<PersistedModelFailure>(&payload)?.failure),
        None => None,
    })
}

/// Attribute the stop to the stage recorded with it, including a refusal after an earlier
/// failed provider attempt. Older records wrote the stage only in structured diagnostics.
fn execution_stop(db: &Connection, run: AgentRunId) -> StoreResult<StopCause> {
    let recorded = crate::agent::facts::latest::<serde_json::Value>(
        db,
        &run.to_string(),
        "agent.application_error",
    )?;
    let text = recorded.as_ref().and_then(|value| value["error"].as_str()).unwrap_or_default();
    let provider_stopped_run = recorded.as_ref().is_none_or(|value| {
        value["stage"].as_str().or_else(|| value["diagnostic"]["stage"].as_str()).map_or_else(
            || {
                text.starts_with("agent.provider_failed:")
                    || text.starts_with("provider_tool_call_binding_invalid")
            },
            |stage| stage == "provider",
        )
    });
    if provider_stopped_run && let Some(failure) = last_provider_failure(db, run)? {
        return Ok(StopCause::Provider {
            failure,
            asked_again: transient_retry::asked_again_before_last(db, run)?,
        });
    }
    own_failure(db, run, text)
}

fn own_failure(db: &Connection, run: AgentRunId, text: &str) -> StoreResult<StopCause> {
    if text.starts_with("model profile is disabled:") {
        return Ok(StopCause::ModelDisabled);
    }
    let code = text.split([':', ' ']).next().unwrap_or_default();
    Ok(match code {
        "agent.cancelled" => stopped(db, run)?,
        "agent.goal_budget_exhausted" => StopCause::GoalBudget,
        "agent.tool_outcome_unknown" => StopCause::ToolOutcomeUnknown,
        "agent.model_binding_required" | "model.profile_missing" => StopCause::ModelUnavailable,
        "model.profile_disabled" => StopCause::ModelDisabled,
        "model.credential_changed" | "model.endpoint_snapshot_mismatch" => {
            StopCause::ConnectionChanged
        }
        "agent.branch_copy_failed" | "agent.branch_copying" => StopCause::BranchCopying,
        "context.compaction_failed" => StopCause::CompactionFailed,
        "context.compaction_no_progress" => StopCause::CompactionNoProgress,
        _ => StopCause::Internal,
    })
}

/// Who stopped a run whose stop was a cancellation: the one its stop note names, the user when
/// a stop was asked for, and otherwise Aura shutting down, which fires a run's stop without
/// writing a request.
fn stopped(db: &Connection, run: AgentRunId) -> StoreResult<StopCause> {
    if let Some(note) = crate::agent::facts::latest::<StopNote>(db, &run.to_string(), STOP_NOTE)? {
        return Ok(StopCause::StoppedBy(note));
    }
    Ok(if crate::agent::application::cancellation_requested(db, run)? {
        StopCause::UserStopped
    } else {
        StopCause::Shutdown
    })
}

/// The latest blocking decision of the user's Hooks for one of `events` in run `run`: on its
/// turn (a turn's start) or on one of its steps.
fn hook_block(
    db: &Connection,
    run: AgentRunId,
    events: &[&str],
) -> StoreResult<Option<(String, String)>> {
    let mut found = None;
    for event in events {
        let block: Option<(String, String, String)> = db
            .query_row(
                "SELECT json_extract(decision,'$.hook'), json_extract(decision,'$.reason'), decided_at FROM hook_decisions WHERE event=?2 AND json_extract(decision,'$.kind')='blocked' AND (subject=(SELECT turn_task_id FROM agent_runs WHERE id=?1) OR subject IN (SELECT id FROM model_steps WHERE agent_run_id=?1)) ORDER BY decided_at DESC LIMIT 1",
                rusqlite::params![run.to_string(), event],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        if let Some((hook, reason, at)) = block {
            let newer =
                found.as_ref().is_none_or(|(_, _, seen): &(String, String, String)| at > *seen);
            if newer {
                found = Some((hook, reason, at));
            }
        }
    }
    Ok(found.map(|(hook, reason, _)| (hook, reason)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_model_domain::{ModelFailureCategory, ModelRetryClass};

    fn failure(code: &str, category: ModelFailureCategory) -> NormalizedModelFailure {
        NormalizedModelFailure {
            category,
            code: code.to_owned(),
            message: "x".to_owned(),
            retry_class: ModelRetryClass::NotRetryable,
            provider_status: None,
            provider_request_id: None,
            provider_error_ref: None,
            partial_output_ref: None,
            transport_attempt_id: None,
        }
    }

    #[test]
    fn a_provider_stop_says_what_the_provider_did_in_both_languages() {
        let cause = StopCause::Provider {
            failure: failure("model_transport_streamidle_timeout", ModelFailureCategory::Transport),
            asked_again: true,
        };
        assert_eq!(
            cause.english(),
            "the model provider could not answer its last step (it sent nothing for too long in the middle of an answer, also when asked again)"
        );
        assert!(cause.next_step().is_some_and(|next| next.contains("smaller calls")));
        assert_eq!(cause.chinese(), "模型服务回答到一半时太久没有再发来内容，重试后也一样");
    }

    /// The loop is in the model's own reasoning, not something the provider did.
    #[test]
    fn a_reasoning_loop_is_the_models_own_and_says_how_to_avoid_it() {
        let cause = StopCause::Provider {
            failure: failure("model_reasoning_loop", ModelFailureCategory::StreamInterrupted),
            asked_again: false,
        };
        assert!(!cause.english().contains("provider"), "{}", cause.english());
        assert!(cause.next_step().is_some_and(|next| next.contains("keep the reasoning short")));
    }

    /// A run whose stop fired with no request to stop was cut by Aura shutting down, not by the
    /// user; a stop that left a note is told in the note's words.
    #[test]
    fn a_stop_names_who_stopped_the_run() {
        let db = Connection::open_in_memory().expect("database");
        db.execute_batch(
            "CREATE TABLE events(sequence INTEGER PRIMARY KEY AUTOINCREMENT, event_id TEXT NOT NULL UNIQUE, owner TEXT NOT NULL, kind TEXT NOT NULL, version INTEGER NOT NULL, payload TEXT NOT NULL, created_at TEXT NOT NULL);",
        )
        .expect("events table");
        let fact = |run: AgentRunId, kind: &str, payload: &str| {
            db.execute(
                "INSERT INTO events(event_id,owner,kind,version,payload,created_at) VALUES(?1,?2,?3,1,?4,'2026-11-10T00:00:00Z')",
                rusqlite::params![format!("{run}-{kind}"), run.to_string(), kind, payload],
            )
            .expect("fact");
        };
        let shut_down = AgentRunId::new();
        assert_eq!(
            stop_cause(&db, shut_down, "agent.cancelled", true).expect("cause"),
            StopCause::Shutdown
        );
        let user = AgentRunId::new();
        fact(user, "agent.cancel_requested", "true");
        assert_eq!(
            stop_cause(&db, user, "agent.cancelled", true).expect("cause"),
            StopCause::UserStopped
        );
        let watched = AgentRunId::new();
        fact(watched, "agent.cancel_requested", "true");
        fact(
            watched,
            STOP_NOTE,
            r#"{"clause":"Aura's watchdog stopped it","next":"Report sooner."}"#,
        );
        let cause = stop_cause(&db, watched, "agent.cancelled", true).expect("cause");
        assert_eq!(cause.english(), "Aura's watchdog stopped it");
        assert_eq!(cause.next_step(), Some("Report sooner."));
    }

    #[test]
    fn a_hook_stop_names_the_hook_and_its_reason() {
        let cause = StopCause::Hook {
            when: HookStop::BeforeStep,
            block: Some(("lint-gate".to_owned(), "run the linter first".to_owned())),
        };
        assert_eq!(
            cause.english(),
            "the user's Hook lint-gate blocked its next step: run the linter first"
        );
        assert_eq!(cause.chinese(), "用户的 Hook lint-gate 拦下了它的下一步：run the linter first");
    }
}
