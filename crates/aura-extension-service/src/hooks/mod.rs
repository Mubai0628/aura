//! Hooks in Core. A Hook is the user's own program: it runs only at the exact content
//! the user trusted, as the user in a Job with time and output limits, and it can only
//! continue, transform within its event's boundary or block. Every call is recorded.
mod exec;
mod manage;
mod registry;
#[cfg(test)]
mod tests;

pub use manage::HookContext;

use aura_backend_windows_user::compat::CompatBackend;
use aura_config::SettingsFacade;
use aura_core_broker::broker;
use aura_domain_common::{HookInvocationId, SessionId, ToolCallId, TurnTaskId, UtcTimestamp};
use aura_hooks::{
    HookDecision, HookEffect, HookEvent, HookFailurePolicy, HookInvocationView, HookOutcome,
    HookTransform,
};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};
use std::time::Instant;

/// One occurrence of an event.
pub struct HookCall {
    pub event: HookEvent,
    pub session_id: Option<SessionId>,
    pub turn_task_id: Option<TurnTaskId>,
    pub tool_call_id: Option<ToolCallId>,
    /// For `tool.*` events: the canonical tool name the Hooks' `match.tools` is checked against.
    pub tool_name: Option<String>,
    pub workspace_root: Option<PathBuf>,
    /// What a pre-event decision applies to, for replay lookups.
    pub subject: Option<String>,
    pub payload: Value,
}

/// Which Hook stopped the action (or the later Hooks), and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookBlock {
    pub hook_name: String,
    pub reason: String,
    /// The Hook failed and its policy is fail-closed, rather than answering `block`.
    pub failed: bool,
}

pub struct HookRunOutcome {
    /// The payload after every applied transform, in registry order.
    pub payload: Value,
    pub transformed: bool,
    pub block: Option<HookBlock>,
    pub invocations: Vec<HookInvocationView>,
}

/// Runs Hooks for events. It holds no state of its own between calls: the registry is read
/// fresh each time, so a change in the settings page applies to the next event.
pub struct HookRunner {
    pub settings: Arc<SettingsFacade>,
    pub journal: Arc<broker::BrokerJournal>,
    /// The directory runs get their TEMP under; each run's folder is removed afterwards.
    pub scratch_root: PathBuf,
    /// Ready plugins' Hooks run after the user's own.
    pub plugins: Option<Arc<crate::plugins::PluginRuntime>>,
}

impl HookRunner {
    /// Every Hook that hears the event, the user's first.
    fn candidates(&self, event: HookEvent, tool: Option<&str>) -> Vec<registry::Candidate> {
        let mut candidates = registry::candidates(&self.settings.hook_registry(), event, tool);
        if let Some(plugins) = &self.plugins {
            candidates.extend(registry::plugin_candidates(plugins, event, tool));
        }
        candidates
    }

    /// Whether any registered, enabled and trusted Hook hears this (non-tool) event.
    pub fn hears(&self, event: HookEvent) -> bool {
        !self.candidates(event, None).is_empty()
    }

    /// Runs every Hook that hears `call`, in order, on the calling blocking thread.
    pub fn run(
        &self,
        call: HookCall,
        powershell: Option<&CompatBackend>,
        cancelled: &AtomicBool,
    ) -> anyhow::Result<HookRunOutcome> {
        let candidates = self.candidates(call.event, call.tool_name.as_deref());
        let mut payload = call.payload;
        let mut outcome = HookRunOutcome {
            payload: Value::Null,
            transformed: false,
            block: None,
            invocations: Vec::new(),
        };
        for hook in &candidates {
            let started_at = UtcTimestamp::now();
            let began = Instant::now();
            let attempt = exec::invoke(
                hook,
                &exec::ExecContext {
                    event: call.event,
                    session_id: call.session_id,
                    turn_task_id: call.turn_task_id,
                    workspace_root: call.workspace_root.as_deref(),
                    scratch_root: &self.scratch_root,
                    powershell,
                    cancelled,
                },
                &payload,
            );
            let name = hook.definition.name.clone();
            let action = if call.event.blocks_action() {
                HookEffect::ActionBlocked
            } else {
                HookEffect::LaterHooksStopped
            };
            let (result, failure, effect, reason, summary) = match attempt.decision {
                Ok(HookDecision::Continue) => {
                    (HookOutcome::Continued, None, HookEffect::None, None, None)
                }
                Ok(HookDecision::Transform(transform)) => {
                    match apply(call.event, &mut payload, &transform) {
                        Ok(summary) => {
                            outcome.transformed = true;
                            (
                                HookOutcome::Transformed,
                                None,
                                HookEffect::Applied,
                                None,
                                Some(summary),
                            )
                        }
                        Err(detail) => self.failure(
                            &hook.definition.failure_policy,
                            &name,
                            action,
                            aura_hooks::HookFailureKind::InvalidResult,
                            detail,
                            &mut outcome,
                        ),
                    }
                }
                Ok(HookDecision::Block { reason }) => {
                    outcome.block = Some(HookBlock {
                        hook_name: name.clone(),
                        reason: reason.clone(),
                        failed: false,
                    });
                    (HookOutcome::Blocked, None, action, Some(reason), None)
                }
                Err((kind, detail)) => self.failure(
                    &hook.definition.failure_policy,
                    &name,
                    action,
                    kind,
                    detail,
                    &mut outcome,
                ),
            };
            let view = HookInvocationView {
                invocation_id: HookInvocationId::new(),
                path: hook.path.clone(),
                hook_name: name,
                hook_version: hook.definition.version.clone(),
                fingerprint: hook.trusted_fingerprint.clone(),
                event: call.event,
                session_id: call.session_id,
                turn_task_id: call.turn_task_id,
                tool_call_id: call.tool_call_id,
                started_at,
                duration_ms: u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX),
                outcome: result,
                failure,
                effect,
                reason,
                summary,
                stderr: attempt.stderr,
            };
            self.record(&view, call.subject.as_deref(), &attempt.input, &attempt.output)?;
            outcome.invocations.push(view);
            if outcome.block.is_some() {
                break;
            }
        }
        outcome.payload = payload;
        Ok(outcome)
    }

    #[allow(clippy::too_many_arguments)]
    fn failure(
        &self,
        policy: &HookFailurePolicy,
        name: &str,
        action: HookEffect,
        kind: aura_hooks::HookFailureKind,
        detail: String,
        outcome: &mut HookRunOutcome,
    ) -> (
        HookOutcome,
        Option<aura_hooks::HookFailureKind>,
        HookEffect,
        Option<String>,
        Option<String>,
    ) {
        match policy {
            HookFailurePolicy::FailOpen => {
                (HookOutcome::Failed, Some(kind), HookEffect::None, Some(detail), None)
            }
            HookFailurePolicy::FailClosed => {
                let reason = format!("Hook {name} 失败（{detail}），按“失败即阻断”处理");
                outcome.block = Some(HookBlock {
                    hook_name: name.to_owned(),
                    reason: reason.clone(),
                    failed: true,
                });
                (HookOutcome::Failed, Some(kind), action, Some(reason), None)
            }
        }
    }

    fn record(
        &self,
        view: &HookInvocationView,
        subject: Option<&str>,
        input: &str,
        output: &str,
    ) -> anyhow::Result<()> {
        let mut store = self.journal.lock()?;
        store.database.unit_of_work(|tx| {
            aura_persistence_v2::hooks::record(
                tx,
                &aura_persistence_v2::hooks::HookInvocationRecord { view, subject, input, output },
            )
        })?;
        Ok(())
    }
}

/// Applies one transform to the event's payload, so the next Hook sees it, and says what
/// changed. Each event has exactly one field a transform may replace or extend.
fn apply(
    event: HookEvent,
    payload: &mut Value,
    transform: &HookTransform,
) -> Result<String, String> {
    let object = payload.as_object_mut().ok_or_else(|| "事件内容不是对象".to_owned())?;
    let chars = |text: &str| text.chars().count();
    Ok(match (event, transform) {
        (HookEvent::MessageBeforeAccept | HookEvent::ModelAfterCall, HookTransform::Text(text)) => {
            object.insert("text".into(), json!(text));
            format!("改写了模型看到的文字（{} 字）", chars(text))
        }
        (HookEvent::ContextBeforeBuild, HookTransform::Inject(text)) => {
            let list = object.entry("injections").or_insert_with(|| json!([]));
            list.as_array_mut().ok_or_else(|| "injections 不是列表".to_owned())?.push(json!(text));
            format!("注入了 {} 字", chars(text))
        }
        (HookEvent::ModelBeforeCall, HookTransform::Parameters(parameters)) => {
            let current = object.entry("parameters").or_insert_with(|| json!({}));
            let current =
                current.as_object_mut().ok_or_else(|| "parameters 不是对象".to_owned())?;
            let mut changed = Vec::new();
            if let Some(value) = parameters.temperature {
                current.insert("temperature".into(), json!(value));
                changed.push("temperature");
            }
            if let Some(value) = parameters.top_p {
                current.insert("top_p".into(), json!(value));
                changed.push("top_p");
            }
            if let Some(value) = parameters.max_output_tokens {
                // Only lower: a Hook can never raise the output budget.
                let limit = current.get("max_output_tokens").and_then(Value::as_u64);
                if limit.is_some_and(|limit| u64::from(value) > limit) {
                    return Err(format!(
                        "max_output_tokens 只能调小（当前 {}）",
                        limit.unwrap_or(0)
                    ));
                }
                current.insert("max_output_tokens".into(), json!(value));
                changed.push("max_output_tokens");
            }
            format!("调整了模型参数：{}", changed.join("、"))
        }
        (HookEvent::ToolBeforeCall, HookTransform::Arguments(arguments)) => {
            object.insert("arguments".into(), arguments.clone());
            "改写了工具参数（会重新检查）".to_owned()
        }
        (HookEvent::ToolAfterCall, HookTransform::Result(text)) => {
            object.insert("result".into(), json!(text));
            format!("改写了模型看到的工具结果（{} 字）", chars(text))
        }
        (HookEvent::ToolOnError, HookTransform::Error(text)) => {
            object.insert("error".into(), json!(text));
            format!("改写了模型看到的错误说明（{} 字）", chars(text))
        }
        (HookEvent::ContextBeforeCompact, HookTransform::Focus(text)) => {
            let list = object.entry("focus").or_insert_with(|| json!([]));
            list.as_array_mut().ok_or_else(|| "focus 不是列表".to_owned())?.push(json!(text));
            format!("增加了保留重点（{} 字）", chars(text))
        }
        _ => return Err(format!("{} 不接受这种变换", event.name())),
    })
}

/// Where Hook runs get their TEMP: next to the V2 database, never inside a package.
pub fn scratch_root(journal: &broker::BrokerJournal) -> anyhow::Result<PathBuf> {
    let database = journal.database_path()?;
    let data = database
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or_else(|| anyhow::anyhow!("hooks.data_root_unavailable"))?;
    Ok(data.join("v2-hooks"))
}
