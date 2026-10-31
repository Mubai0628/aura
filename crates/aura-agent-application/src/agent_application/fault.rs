//! One run's own step failure, projected by **type** instead of by message text.
//!
//! WHY THIS IS ITS OWN MODULE
//! The drive loop has to answer three different questions about a failed step: which class it
//! was (a definite Provider failure, a tool call whose real outcome is Unknown, or the loop's own
//! bounded refusal), which stage it happened in, and what bounded evidence the run keeps. Those
//! are projection decisions, not execution decisions, so they live here instead of being spread
//! through the worker or through the persistence layer.
//!
//! CLASSIFICATION IS BY TYPE, NEVER BY TEXT
//! A Provider failure arrives as [`worker::ModelStepFailure`] - the transport already classified
//! it and, for a refused Provider output, already built the bounded structured diagnostic. A tool
//! call that was really dispatched and never reached a terminal receipt arrives as
//! [`ToolOutcomeUnknown`], which keeps the batch and the Attempt identities. Everything else stays
//! [`StepFault::Step`]: a bounded refusal with no structured evidence, reported as such.
//!
//! The previous shape of this decision searched the error chain for the text `wire name ` or
//! `provider_tool_call_binding_invalid` and stored whatever line matched. That inferred an
//! execution stage from message content: any unrelated error that happened to quote those words
//! would have been recorded as a Provider binding refusal, and a binding refusal whose text
//! changed would have been recorded as an unclassified step failure. Nothing here reads a message
//! to decide anything.
use super::*;
use aura_execution_contract::AttemptRef;

/// A tool call that was really admitted and dispatched, and for which the store holds no terminal
/// result. It is not a refusal (something may have happened) and not a failure (nothing
/// established that it failed), so it is its own class.
///
/// The Attempt identities are kept: they are the only handle a later reader has on the real work
/// that may or may not have happened, and the run must never re-dispatch them.
#[derive(Debug, thiserror::Error)]
#[error(
    "agent.tool_outcome_unknown: {} tool call(s) of batch {batch} were admitted and dispatched, \
     and the store holds no terminal result for them",
    attempts.len()
)]
pub struct ToolOutcomeUnknown {
    pub run: AgentRunId,
    pub batch: ToolBatchId,
    pub attempts: Vec<AttemptRef>,
}

impl ToolOutcomeUnknown {
    /// The bounded evidence a reviewer or the UI reads: the batch and the real Attempt identities,
    /// never a Provider message and never tool arguments.
    pub(super) fn evidence(&self) -> serde_json::Value {
        serde_json::json!({
            "code": "agent.tool_outcome_unknown",
            "stage": "tools",
            "agent_run_id": self.run.to_string(),
            "tool_batch_id": self.batch.to_string(),
            "attempt_count": self.attempts.len(),
            "attempts": self.attempts.iter().map(|attempt| serde_json::json!({
                "operation_id": attempt.operation_id.to_string(),
                "ordinal": attempt.ordinal.get(),
            })).collect::<Vec<_>>(),
            // Stated once, here, so no reader has to infer it from the class name.
            "replay": "never",
        })
    }
}

/// One run's own step failure, classified by the value that produced it.
pub(super) enum StepFault<'a> {
    /// The Provider transport answered with a definite failure.
    Provider(&'a worker::ModelStepFailure),
    /// A tool call was really dispatched and its outcome could not be established.
    ToolUnknown(&'a ToolOutcomeUnknown),
    /// Any other bounded refusal of the run's own step.
    Step(&'a anyhow::Error),
}

impl<'a> StepFault<'a> {
    /// Classify by the error's own type. The error's text is never consulted.
    pub(super) fn of(error: &'a anyhow::Error) -> Self {
        if let Some(failure) = error.downcast_ref::<worker::ModelStepFailure>() {
            return Self::Provider(failure);
        }
        if let Some(unknown) = error.downcast_ref::<ToolOutcomeUnknown>() {
            return Self::ToolUnknown(unknown);
        }
        Self::Step(error)
    }

    /// Which stage of the run this fault happened in. It is a property of the class.
    pub(super) fn stage(&self) -> &'static str {
        match self {
            Self::Provider(_) => "provider",
            Self::ToolUnknown(_) => "tools",
            Self::Step(_) => "step",
        }
    }

    /// The bounded line persisted on the run.
    ///
    /// A Provider failure with a structured diagnostic stores the diagnostic's own bounded
    /// summary; a tool Unknown stores its own line; anything else stores its own error text,
    /// which is a bounded refusal the loop produced and never a Provider message by construction.
    pub(super) fn stored_text(&self) -> String {
        match self {
            Self::Provider(failure) => match &failure.diagnostic {
                Some(diagnostic) => diagnostic.summary(),
                None => failure.to_string(),
            },
            Self::ToolUnknown(unknown) => unknown.to_string(),
            Self::Step(error) => error.to_string(),
        }
    }

    /// The structured, bounded evidence, when this class has any.
    ///
    /// A class that carries evidence names its own stage inside that record, so a reader of the
    /// durable fact never has to infer the stage from the class name, and never from text.
    pub(super) fn structured(&self) -> Option<serde_json::Value> {
        let mut value = match self {
            Self::Provider(failure) => failure.diagnostic.as_ref().map(|value| value.to_json())?,
            Self::ToolUnknown(unknown) => unknown.evidence(),
            Self::Step(_) => return None,
        };
        if let Some(object) = value.as_object_mut() {
            object.insert("stage".to_owned(), serde_json::Value::String(self.stage().to_owned()));
        }
        Some(value)
    }
}
