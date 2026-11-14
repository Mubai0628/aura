//! What the model reads of a tool result. The stored envelope keeps
//! every field for audit and replay. The model reads only what bears on its next step - the
//! outcome, the side-effect state, warnings and the artifacts it may read. The call id, the tool
//! definition with its registry snapshot, the completion time and each artifact's hash were noise
//! to it, and the two hashes were the only ones a write result showed.
//!
//! An outcome that carries no message of its own - a call that was stopped, or one that was not
//! run without a notice - is given one from its reason and side-effect state, and the known
//! warning codes read as sentences. The view is built when history is sent, so stored results
//! read the same way.
use aura_execution_contract::SideEffectStateV1;
use serde_json::{Map, Value, json};

use crate::{
    ModelVisibleToolResultEnvelopeV1, ModelVisibleToolResultOutcomeV1 as Outcome,
    ToolCancellationReasonV1, ToolNotExecutedReasonV1,
};

impl ModelVisibleToolResultEnvelopeV1 {
    /// The result as the model reads it.
    #[must_use]
    pub fn model_view(&self) -> Value {
        let mut view = Map::new();
        view.insert("outcome".to_owned(), serde_json::to_value(&self.outcome).unwrap_or_default());
        view.insert(
            "side_effect_state".to_owned(),
            serde_json::to_value(self.side_effect_state).unwrap_or_default(),
        );
        if let Some(message) = self.message() {
            view.insert("message".to_owned(), Value::String(message));
        }
        if !self.warnings.is_empty() {
            let warnings = self
                .warnings
                .iter()
                .map(|code| {
                    let code = code.as_str();
                    Value::String(warning(code).unwrap_or(code).to_owned())
                })
                .collect();
            view.insert("warnings".to_owned(), Value::Array(warnings));
        }
        if !self.artifacts.is_empty() {
            let artifacts = self
                .artifacts
                .iter()
                .map(|artifact| {
                    json!({
                        "artifact_id": artifact.artifact_id,
                        "slot": artifact.slot,
                        "media_type": artifact.media_type,
                        "byte_length": artifact.byte_length,
                    })
                })
                .collect();
            view.insert("artifacts".to_owned(), Value::Array(artifacts));
        }
        Value::Object(view)
    }

    /// [`Self::model_view`] as the text a tool result message carries.
    #[must_use]
    pub fn model_text(&self) -> String {
        self.model_view().to_string()
    }

    /// Why a call that has no message of its own stopped or did not run, what it changed, and
    /// what to do next.
    fn message(&self) -> Option<String> {
        match &self.outcome {
            // Nothing of it was sent: its turn was stopped in the moment after it was marked
            // started.
            Outcome::Cancelled { reason: ToolCancellationReasonV1::TurnInterrupted }
                if matches!(
                    self.side_effect_state,
                    SideEffectStateV1::NoEffect | SideEffectStateV1::NotStarted
                ) =>
            {
                Some(not_run_message(ToolNotExecutedReasonV1::CancelledBeforeDispatch))
            }
            Outcome::Cancelled { reason } => Some(format!(
                "This call was stopped before it finished{}. {}",
                cancelled(*reason),
                effect(self.side_effect_state)
            )),
            Outcome::NotExecuted { reason, error: None } => Some(not_run_message(*reason)),
            _ => None,
        }
    }
}

/// What the model reads for a call that was not run for `reason` when nothing more specific is
/// known: why, that nothing was changed, and the next step.
#[must_use]
pub fn not_run_message(reason: ToolNotExecutedReasonV1) -> String {
    let (why, next) = not_executed(reason);
    format!("This call was not run: {why}. Nothing was changed. {next}")
}

/// What stopped a call, as a clause after "stopped before it finished". A plain stop request
/// may have come from the user, from a stop call or from the turn ending, so it names none.
fn cancelled(reason: ToolCancellationReasonV1) -> &'static str {
    use ToolCancellationReasonV1 as R;
    match reason {
        R::UserRequested => "",
        R::TurnInterrupted => ": the turn it belonged to was interrupted",
        R::PermissionRevoked => ": its permission was withdrawn while it ran",
        R::ScopeClosed => ": the workspace or folder it worked in was closed",
        R::CoreShutdown => ": Aura was shutting down",
        R::Timeout => ": it ran past its time limit",
        R::ParentAgentCancelled => ": the main agent stopped this sub-agent",
        R::BatchCancelled => ": the calls of its response were stopped together",
    }
}

/// What a stopped call changed, and what to do before trying it again.
fn effect(state: SideEffectStateV1) -> &'static str {
    match state {
        SideEffectStateV1::NotStarted | SideEffectStateV1::NoEffect => {
            "Nothing was changed. Call it again if it is still needed."
        }
        SideEffectStateV1::FullyApplied => "Its changes had been made before it stopped.",
        SideEffectStateV1::Compensated => "What it had changed was undone.",
        SideEffectStateV1::PartiallyApplied => {
            "Part of its changes may have been made; check the target before trying again."
        }
        SideEffectStateV1::Unknown => {
            "Aura cannot tell what it changed; check the target before trying again."
        }
        SideEffectStateV1::Unassessed => {
            "Aura did not check what it changed; check the target before trying again."
        }
    }
}

/// Why a call was not run, and the next step.
fn not_executed(reason: ToolNotExecutedReasonV1) -> (&'static str, &'static str) {
    use ToolNotExecutedReasonV1 as R;
    match reason {
        R::SchemaValidationFailed => (
            "its arguments did not match the tool's input schema",
            "Correct them and call it again.",
        ),
        R::ToolNotFound => {
            ("no tool by this name is available in this step", "Use one of the tools you have.")
        }
        R::VersionMismatch => {
            ("the tool changed after this response was written", "Call it again.")
        }
        R::NotLoaded => (
            "the tool is not in this step's tool list",
            "Call it through tools.call, or add it with tools.load.",
        ),
        R::Disabled => (
            "the user has turned this tool off",
            "Do it another way, or ask the user to turn the tool on.",
        ),
        R::TrustRequired => (
            "the user has not yet trusted where this tool comes from",
            "Do it another way, or ask the user to trust it.",
        ),
        R::AudienceDenied => {
            ("this tool is not available in this conversation", "Do it another way.")
        }
        R::PurposeRequired => (
            "it needs aura_purpose, one sentence saying what the call is for",
            "Call it again with aura_purpose.",
        ),
        R::PermissionDenied => (
            "it was not permitted",
            "Do not repeat it unchanged; continue without it, or ask the user.",
        ),
        R::ApprovalDenied => (
            "the user denied it when asked for approval",
            "Do not repeat it unchanged; continue without it, or ask the user what they want \
             instead.",
        ),
        R::RuntimeUnavailable => {
            ("what runs this tool is not available now", "Try again later, or do it another way.")
        }
        R::DependencyNotExecuted => (
            "a call it depends on did not succeed",
            "Deal with that call first, then make this one again.",
        ),
        R::AtomicGroupNotExecuted => (
            "another call of its group was not run, and a group runs whole or not at all",
            "Call the group again once that is fixed.",
        ),
        R::LineageReconciliation => {
            ("the rest of its response could not go ahead", "Call it again if it is still needed.")
        }
        R::CompletionReview => {
            ("the turn's completion was under review", "Call it again if it is still needed.")
        }
        R::CancelledBeforeDispatch => {
            ("the turn was stopped before it started", "Call it again if it is still needed.")
        }
        R::BatchRejected => (
            "the calls of this response were refused together",
            "Read the other calls' results for the cause, then call again.",
        ),
        R::ResourceClaimResolutionFailed => (
            "Aura could not tell which files or other resources it would use",
            "Check its paths and call it again.",
        ),
    }
}

/// A known warning code as a sentence.
fn warning(code: &str) -> Option<&'static str> {
    Some(match code {
        "execution.isolation_reduced" => "It ran with the user's own rights, not in a sandbox.",
        "tool.side_effect.unverified" => {
            "Aura could not confirm what it changed; check before repeating it."
        }
        "tool.cleanup.incomplete" => {
            "Aura could not fully take back the file access it gave the call."
        }
        "tool.outcome.unsettled" => {
            "Aura has not settled this call's outcome yet; check before repeating it."
        }
        "tool.package.changed_during_call" => {
            "The tool's package changed while the call ran; check the result before relying on it."
        }
        "execution.not_dispatched" => "It was never started.",
        _ => return None,
    })
}

#[cfg(test)]
#[path = "model_view_tests.rs"]
mod tests;
