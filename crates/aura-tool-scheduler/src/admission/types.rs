use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolPreflightRejectionV1 {
    pub tool_call_id: ToolCallId,
    pub ordinal: u32,
    pub reason: ToolNotExecutedReasonV1,
    pub error_code: Option<ErrorCode>,
    pub schema_validation: Option<ToolSchemaValidationReportV1>,
    /// The normalization issue, when the rejection came from the argument envelope
    /// rather than from the tool's own schema.
    ///
    /// It is carried through instead of being flattened into "schema validation
    /// failed": the correction the model has to make is different for every issue, and
    /// recovering it later would mean parsing the English message back into the fact it
    /// was generated from.
    pub input_issue: Option<NormalizedToolCallInputIssueV1>,
    /// Where that issue was found, bounded and structure-only. `None` when the issue
    /// concerns the whole envelope. Boxed because this rejection travels inside a
    /// `Result` through the admission path, and the diagnostic is the largest thing the
    /// rejection carries; the error's own size is what callers pay for on every step.
    pub input_diagnostic: Option<Box<aura_tool_contract::NormalizedToolCallInputDiagnosticV1>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedToolCallV1 {
    pub call: aura_tool_contract::NormalizedToolCallV1,
    pub definition: ToolDefinitionRecordV1,
    pub binding: aura_tool_contract::EffectiveToolBindingSnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdmittedToolBatchV1 {
    pub batch: ToolBatchRequestV1,
    pub validated_calls: Vec<ValidatedToolCallV1>,
    pub not_executed: Vec<ToolPreflightRejectionV1>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolBatchAdmissionOutcomeV1 {
    /// Duplicate Aura ToolCallId means there is no valid call identity to pair
    /// with a synthetic Result. The caller must move the ModelStep/TurnTask to
    /// RecoveryRequired.
    IdentityIntegrityFault {
        code: ToolBatchErrorCode,
    },
    /// A batch-level invariant failed after ToolCall identity uniqueness was
    /// established, so every call can safely receive batch_rejected.
    BatchRejected {
        code: ToolBatchErrorCode,
        calls: Vec<ToolPreflightRejectionV1>,
    },
    Admitted(Box<AdmittedToolBatchV1>),
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ToolBatchAdmissionInputError {
    #[error("the definition set contains duplicate exact Tool versions")]
    DuplicateDefinition,
    #[error("the definition set contains an invalid or mismatched record")]
    InvalidDefinition,
}
