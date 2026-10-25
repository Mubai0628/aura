use super::*;

pub(super) fn reject_batch(
    batch: &ToolBatchRequestV1,
    code: ToolBatchErrorCode,
) -> ToolBatchAdmissionOutcomeV1 {
    ToolBatchAdmissionOutcomeV1::BatchRejected {
        code,
        calls: batch
            .calls
            .iter()
            .map(|call| {
                rejection(
                    call,
                    ToolNotExecutedReasonV1::BatchRejected,
                    Some(code.error_code()),
                    None,
                )
            })
            .collect(),
    }
}

pub(super) fn rejection(
    call: &aura_tool_contract::NormalizedToolCallV1,
    reason: ToolNotExecutedReasonV1,
    error_code: Option<ErrorCode>,
    schema_validation: Option<ToolSchemaValidationReportV1>,
) -> ToolPreflightRejectionV1 {
    ToolPreflightRejectionV1 {
        tool_call_id: call.tool_call_id,
        ordinal: call.ordinal,
        reason,
        error_code,
        schema_validation,
        input_issue: None,
        input_diagnostic: None,
    }
}

/// The rejection for an argument envelope the normalizer refused.
///
/// The issue and its bounded location travel with the rejection so the correction can
/// name the field that has to move. The tool's own schema report stays `None`: nothing
/// was validated against it, and claiming a schema error here is what made every
/// envelope mistake look like a business-argument mistake.
pub(super) fn input_rejection(
    call: &aura_tool_contract::NormalizedToolCallV1,
    issue: NormalizedToolCallInputIssueV1,
    diagnostic: Option<Box<aura_tool_contract::NormalizedToolCallInputDiagnosticV1>>,
) -> ToolPreflightRejectionV1 {
    let purpose_issue = matches!(
        issue,
        NormalizedToolCallInputIssueV1::AuraMetadataMissing
            | NormalizedToolCallInputIssueV1::PurposeMissing
            | NormalizedToolCallInputIssueV1::PurposeInvalid
            | NormalizedToolCallInputIssueV1::PurposeTooLong
    );
    ToolPreflightRejectionV1 {
        tool_call_id: call.tool_call_id,
        ordinal: call.ordinal,
        reason: if purpose_issue {
            ToolNotExecutedReasonV1::PurposeRequired
        } else {
            ToolNotExecutedReasonV1::SchemaValidationFailed
        },
        error_code: Some(ToolSchemaErrorCode::ArgumentsInvalid.error_code()),
        schema_validation: None,
        input_issue: Some(issue),
        input_diagnostic: diagnostic,
    }
}
