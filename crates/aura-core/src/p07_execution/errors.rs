use super::*;

#[derive(Debug, Error)]
pub(crate) enum ToolAttemptDispatchErrorV1 {
    #[error("the Tool credential could not be obtained under the prepared authorization")]
    CredentialUnavailable,
    #[error("the authorized Runtime is not a supported short-lived Sandbox operation")]
    RuntimeMismatch,
    #[error("the AuraStdio dispatch binding is missing its output authority")]
    OutputBindingMissing,
    #[error("the Attempt-local output root could not be created")]
    OutputRootPreparationFailed,
    #[error(transparent)]
    Sandbox(#[from] ToolSandboxExecutionError),
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error(transparent)]
    Projection(#[from] ToolExecutionProjectionErrorV1),
    #[error(transparent)]
    BuiltinHandler(#[from] ToolBuiltinHandlerErrorV1),
}

#[derive(Debug, Error)]
pub(crate) enum ToolAttemptPreDispatchErrorV1 {
    #[error("the admitted call and frozen execution plan are inconsistent")]
    PlanMismatch,
    #[error("the exact Tool execution authority is unavailable at the final dispatch guard")]
    AuthorityUnavailable,
    #[error("the exact Tool execution authority changed after P04 evaluation")]
    AuthorityChanged,
    /// The tool's **current** enablement state refuses this attempt.
    ///
    /// A disable committed after a registry snapshot was frozen
    /// must refuse an attempt that has not yet been prepared; registry eligibility alone is a
    /// point-in-time judgement and cannot carry that.
    #[error("tool {tool_id} is not enabled for this scope")]
    ToolDisabled { tool_id: aura_domain_common::ToolId },
    /// The live enablement authority could not be read.
    ///
    /// Failing to ask is never evidence that a tool is enabled, so this is a refusal with its own
    /// reason rather than a permissive default.
    #[error("the live tool enablement authority is unavailable")]
    EnablementUnavailable,
    #[error("the persisted ToolCall is not eligible to create this attempt")]
    ExecutionStateUnavailable,
    #[error("tool dispatch was cancelled before the pre-dispatch transaction")]
    CancelledBeforeDispatch,
    #[error("the exact signed Built-in handler binding is missing")]
    BuiltinBindingMissing,
    #[error("the Core-owned Built-in Registry rebuild context is inconsistent")]
    BuiltinBuildContextMismatch,
    #[error(transparent)]
    Schedule(#[from] ToolScheduleErrorV1),
    #[error(transparent)]
    Sandbox(#[from] ToolSandboxExecutionError),
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error("the pre-dispatch transaction returned no durable lifecycle evidence")]
    PersistenceEvidenceMissing,
    #[error(transparent)]
    Projection(#[from] ToolExecutionProjectionErrorV1),
    #[error(transparent)]
    Assembly(#[from] ToolSandboxAssemblyErrorV1),
    #[error(transparent)]
    HttpAssembly(#[from] ToolHttpAssemblyErrorV1),
    #[error(transparent)]
    BuiltinAssembly(#[from] ToolBuiltinAssemblyErrorV1),
}
