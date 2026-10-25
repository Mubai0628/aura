use aura_domain_common::{CorrelationId, ErrorCategory, ErrorEnvelope};
use aura_execution_contract::{SideEffectStateV1, VerifiedAttemptSideEffectEvidenceV1};
use aura_tool_contract::{
    StdioProtocolFrameV1, ToolCancellationReasonV1, ToolIndeterminateReasonV1,
    ToolResultContractV1, ToolRuntimeErrorCode,
};
use serde_json::Value;
use thiserror::Error;

use crate::{PreparedToolResultContractV1, ToolResultBodyErrorV1, ValidatedToolResultBodyV1};

/// Runtime facts observed for exactly one dispatched `tea_`. Runtime adapters
/// may report only candidate facts; they cannot select the public ToolResult
/// outcome or aggregate side-effect state.
#[derive(Clone, Debug, PartialEq)]
pub enum ToolRuntimeCandidateV1 {
    /// Result bytes returned by one closed, signed Core Built-in handler.
    /// This is an internal candidate path and does not create a public Runtime
    /// descriptor or Package Manifest variant.
    CoreBuiltinHandlerCompleted {
        response_body: Vec<u8>,
    },
    CommandExited {
        exit_code: i32,
        success_exit_codes: Vec<i32>,
        stdout: Vec<u8>,
        process_tree_terminated: bool,
    },
    HttpCompleted {
        status: u16,
        success_status_ranges: Vec<(u16, u16)>,
        response_body: Vec<u8>,
    },
    StdioCompleted {
        terminal: Option<StdioProtocolFrameV1>,
        process_tree_terminated: bool,
    },
    DeterministicFailure {
        code: ToolRuntimeErrorCode,
    },
    /// A deterministic failure decided by another authoritative Core domain
    ///. Its stable error namespace is
    /// retained instead of being copied into a synthetic `tool.runtime.*`
    /// code.
    DeterministicExternalFailure {
        error: ErrorEnvelope,
    },
    CancelledAndTerminated,
    TimedOutAndTerminated,
    DispatchOutcomeUnknown,
    RuntimeTerminationUnknown,
    RuntimeTerminalEvidenceInvalid,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ToolAttemptTerminalDecisionV1 {
    Succeeded { body: ValidatedToolResultBodyV1 },
    Failed { error: ErrorEnvelope },
    Cancelled { reason: ToolCancellationReasonV1 },
    Indeterminate { reason: ToolIndeterminateReasonV1, error: ErrorEnvelope },
}

#[derive(Clone, Debug, PartialEq)]
pub struct EvaluatedToolRuntimeAttemptV1 {
    pub terminal: ToolAttemptTerminalDecisionV1,
    pub side_effect_state: SideEffectStateV1,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ToolRuntimeEvaluationErrorV1 {
    #[error("attempt-local side-effect evidence is invalid")]
    InvalidSideEffectEvidence,
    #[error("the exact Tool result contract cannot be compiled")]
    ResultContractInvalid,
}

/// Converts exact Runtime candidate facts into a legal attempt terminal.
/// `NotExecuted` is intentionally absent: once this function is called the
/// Runtime boundary has already been crossed.
pub fn evaluate_tool_runtime_candidate_v1(
    candidate: ToolRuntimeCandidateV1,
    result_contract: &ToolResultContractV1,
    side_effect: &VerifiedAttemptSideEffectEvidenceV1,
    cancellation_intent: Option<ToolCancellationReasonV1>,
    correlation_id: CorrelationId,
) -> Result<EvaluatedToolRuntimeAttemptV1, ToolRuntimeEvaluationErrorV1> {
    side_effect.validate().map_err(|_| ToolRuntimeEvaluationErrorV1::InvalidSideEffectEvidence)?;
    let side_effect_state = side_effect.state();
    let result_contract = PreparedToolResultContractV1::compile(result_contract)
        .map_err(|_| ToolRuntimeEvaluationErrorV1::ResultContractInvalid)?;

    let terminal = match candidate {
        ToolRuntimeCandidateV1::CoreBuiltinHandlerCompleted { response_body } => {
            evaluate_success_body(&result_contract, &response_body, correlation_id)
        }
        ToolRuntimeCandidateV1::CommandExited {
            exit_code,
            success_exit_codes,
            stdout,
            process_tree_terminated,
        } => {
            if !process_tree_terminated {
                indeterminate(
                    ToolIndeterminateReasonV1::RuntimeTerminationUnknown,
                    ToolRuntimeErrorCode::ProcessTreeTerminationUnknown,
                    correlation_id,
                )
            } else if success_exit_codes.contains(&exit_code) {
                evaluate_success_body(&result_contract, &stdout, correlation_id)
            } else {
                failed(
                    ToolRuntimeErrorCode::DispatchFailed,
                    "The tool process returned a configured failure exit code.",
                    false,
                    correlation_id,
                )
            }
        }
        ToolRuntimeCandidateV1::HttpCompleted { status, success_status_ranges, response_body } => {
            if success_status_ranges.iter().any(|(start, end)| (*start..=*end).contains(&status)) {
                evaluate_success_body(&result_contract, &response_body, correlation_id)
            } else {
                failed(
                    ToolRuntimeErrorCode::HttpGatewayFailed,
                    "The tool HTTP response status is outside its success contract.",
                    false,
                    correlation_id,
                )
            }
        }
        ToolRuntimeCandidateV1::StdioCompleted { terminal, process_tree_terminated } => {
            if !process_tree_terminated {
                indeterminate(
                    ToolIndeterminateReasonV1::RuntimeTerminationUnknown,
                    ToolRuntimeErrorCode::ProcessTreeTerminationUnknown,
                    correlation_id,
                )
            } else {
                match terminal {
                    Some(StdioProtocolFrameV1::FinalResult { payload, .. }) => {
                        let bytes = canonical_result_bytes(&payload.result)
                            .map_err(|_| ToolRuntimeEvaluationErrorV1::ResultContractInvalid)?;
                        evaluate_success_body(&result_contract, &bytes, correlation_id)
                    }
                    Some(StdioProtocolFrameV1::FinalFailure { .. }) => failed(
                        ToolRuntimeErrorCode::DispatchFailed,
                        "The tool runtime returned a deterministic failure.",
                        false,
                        correlation_id,
                    ),
                    None => indeterminate(
                        ToolIndeterminateReasonV1::RuntimeTerminalMissing,
                        ToolRuntimeErrorCode::ProtocolTerminalMissing,
                        correlation_id,
                    ),
                    Some(_) => indeterminate(
                        ToolIndeterminateReasonV1::ResultEvidenceInvalid,
                        ToolRuntimeErrorCode::ProtocolTerminalConflict,
                        correlation_id,
                    ),
                }
            }
        }
        ToolRuntimeCandidateV1::DeterministicFailure { code } => failed(
            code,
            "The tool runtime failed before producing a valid result.",
            false,
            correlation_id,
        ),
        ToolRuntimeCandidateV1::DeterministicExternalFailure { mut error } => {
            // External authorities supply only the safe error identity. Core
            // derives retry eligibility from closed, typed Gateway evidence.
            error.retryable = false;
            ToolAttemptTerminalDecisionV1::Failed { error }
        }
        ToolRuntimeCandidateV1::CancelledAndTerminated => match cancellation_intent {
            Some(reason) => ToolAttemptTerminalDecisionV1::Cancelled { reason },
            None => indeterminate(
                ToolIndeterminateReasonV1::ResultEvidenceInvalid,
                ToolRuntimeErrorCode::ProtocolTerminalConflict,
                correlation_id,
            ),
        },
        ToolRuntimeCandidateV1::TimedOutAndTerminated => match cancellation_intent {
            Some(ToolCancellationReasonV1::Timeout) => ToolAttemptTerminalDecisionV1::Cancelled {
                reason: ToolCancellationReasonV1::Timeout,
            },
            _ => indeterminate(
                ToolIndeterminateReasonV1::ResultEvidenceInvalid,
                ToolRuntimeErrorCode::ProtocolTerminalConflict,
                correlation_id,
            ),
        },
        ToolRuntimeCandidateV1::DispatchOutcomeUnknown => indeterminate(
            ToolIndeterminateReasonV1::DispatchOutcomeUnknown,
            ToolRuntimeErrorCode::DispatchOutcomeUnknown,
            correlation_id,
        ),
        ToolRuntimeCandidateV1::RuntimeTerminationUnknown => indeterminate(
            ToolIndeterminateReasonV1::RuntimeTerminationUnknown,
            ToolRuntimeErrorCode::ProcessTreeTerminationUnknown,
            correlation_id,
        ),
        ToolRuntimeCandidateV1::RuntimeTerminalEvidenceInvalid => indeterminate(
            ToolIndeterminateReasonV1::ResultEvidenceInvalid,
            ToolRuntimeErrorCode::ProtocolTerminalConflict,
            correlation_id,
        ),
    };
    Ok(EvaluatedToolRuntimeAttemptV1 { terminal, side_effect_state })
}

fn evaluate_success_body(
    result_contract: &PreparedToolResultContractV1,
    bytes: &[u8],
    correlation_id: CorrelationId,
) -> ToolAttemptTerminalDecisionV1 {
    match result_contract.validate_candidate(bytes) {
        Ok(body) => ToolAttemptTerminalDecisionV1::Succeeded { body },
        Err(error) => failed_result_validation(error, correlation_id),
    }
}

fn failed_result_validation(
    error: ToolResultBodyErrorV1,
    correlation_id: CorrelationId,
) -> ToolAttemptTerminalDecisionV1 {
    ToolAttemptTerminalDecisionV1::Failed {
        error: ErrorEnvelope::new(
            error.error_code().error_code(),
            ErrorCategory::Protocol,
            "The tool result did not satisfy its frozen result contract.",
            false,
            correlation_id,
            None,
        ),
    }
}

fn canonical_result_bytes(value: &Value) -> Result<Vec<u8>, ()> {
    aura_domain_common::jcs::canonical_json_jcs(value).map_err(|_| ())
}

fn failed(
    code: ToolRuntimeErrorCode,
    message: &'static str,
    retryable: bool,
    correlation_id: CorrelationId,
) -> ToolAttemptTerminalDecisionV1 {
    ToolAttemptTerminalDecisionV1::Failed {
        error: ErrorEnvelope::new(
            code.error_code(),
            ErrorCategory::Unavailable,
            message,
            retryable,
            correlation_id,
            None,
        ),
    }
}

fn indeterminate<C>(
    reason: ToolIndeterminateReasonV1,
    code: C,
    correlation_id: CorrelationId,
) -> ToolAttemptTerminalDecisionV1
where
    C: StableToolErrorCode,
{
    ToolAttemptTerminalDecisionV1::Indeterminate {
        reason,
        error: ErrorEnvelope::new(
            code.error_code(),
            ErrorCategory::Protocol,
            "The tool execution terminal state could not be proven.",
            false,
            correlation_id,
            None,
        ),
    }
}

trait StableToolErrorCode {
    fn error_code(self) -> aura_domain_common::ErrorCode;
}

impl StableToolErrorCode for ToolRuntimeErrorCode {
    fn error_code(self) -> aura_domain_common::ErrorCode {
        self.error_code()
    }
}

#[cfg(test)]
mod tests {
    use aura_domain_common::ArtifactId;
    use aura_domain_common::jcs::CanonicalSha256HashV1;
    use aura_domain_common::{ToolCallId, ToolExecutionAttemptId};
    use aura_execution_contract::{AttemptSideEffectProofV1, SideEffectEvidenceProducerV1};
    use aura_tool_contract::{
        STDIO_PROTOCOL_VERSION_V1, StdioFinalResultPayloadV1, ToolResultErrorCode,
        ToolStructuredResultSchemaV1,
    };
    use serde_json::json;

    use super::*;

    fn evidence(state: SideEffectStateV1) -> VerifiedAttemptSideEffectEvidenceV1 {
        let proof = match state {
            SideEffectStateV1::NoEffect => {
                AttemptSideEffectProofV1::NoEffect { evidence_ref: ArtifactId::new() }
            }
            SideEffectStateV1::Unknown => {
                AttemptSideEffectProofV1::Unknown { evidence_ref: ArtifactId::new() }
            }
            _ => panic!("test helper supports no_effect and unknown only"),
        };
        VerifiedAttemptSideEffectEvidenceV1::from_core_evidence(
            ToolExecutionAttemptId::new(),
            CanonicalSha256HashV1::sha256(b"test effect scope"),
            SideEffectEvidenceProducerV1::StrictSandbox,
            proof,
        )
        .expect("evidence")
    }

    fn no_effect() -> VerifiedAttemptSideEffectEvidenceV1 {
        evidence(SideEffectStateV1::NoEffect)
    }

    #[test]
    fn command_exit_and_result_contract_are_both_authoritative() {
        let decision = evaluate_tool_runtime_candidate_v1(
            ToolRuntimeCandidateV1::CommandExited {
                exit_code: 0,
                success_exit_codes: vec![0],
                stdout: b"result".to_vec(),
                process_tree_terminated: true,
            },
            &ToolResultContractV1::Utf8Text,
            &no_effect(),
            None,
            CorrelationId::new(),
        )
        .expect("evaluation");
        assert!(matches!(decision.terminal, ToolAttemptTerminalDecisionV1::Succeeded { .. }));

        let failed = evaluate_tool_runtime_candidate_v1(
            ToolRuntimeCandidateV1::CommandExited {
                exit_code: 2,
                success_exit_codes: vec![0],
                stdout: Vec::new(),
                process_tree_terminated: true,
            },
            &ToolResultContractV1::Unit,
            &no_effect(),
            None,
            CorrelationId::new(),
        )
        .expect("evaluation");
        assert!(matches!(failed.terminal, ToolAttemptTerminalDecisionV1::Failed { .. }));
    }

    #[test]
    fn structured_result_failure_is_failed_not_indeterminate() {
        let schema = ToolStructuredResultSchemaV1::new(json!({"type": "integer"})).expect("schema");
        let decision = evaluate_tool_runtime_candidate_v1(
            ToolRuntimeCandidateV1::HttpCompleted {
                status: 200,
                success_status_ranges: vec![(200, 299)],
                response_body: br#""wrong""#.to_vec(),
            },
            &ToolResultContractV1::StructuredJson { result_schema: schema },
            &no_effect(),
            None,
            CorrelationId::new(),
        )
        .expect("evaluation");
        let ToolAttemptTerminalDecisionV1::Failed { error } = decision.terminal else {
            panic!("schema failure must be deterministic")
        };
        assert_eq!(error.code, ToolResultErrorCode::SchemaValidationFailed.error_code());
    }

    #[test]
    fn external_failure_cannot_self_authorize_automatic_retry() {
        let mut supplied = ErrorEnvelope::new(
            ToolRuntimeErrorCode::DispatchFailed.error_code(),
            ErrorCategory::Transport,
            "Untrusted retry claim.",
            true,
            CorrelationId::new(),
            None,
        );
        supplied.details.insert("retryable".to_owned(), json!(true));
        let decision = evaluate_tool_runtime_candidate_v1(
            ToolRuntimeCandidateV1::DeterministicExternalFailure { error: supplied },
            &ToolResultContractV1::Unit,
            &no_effect(),
            None,
            CorrelationId::new(),
        )
        .expect("evaluation");
        let ToolAttemptTerminalDecisionV1::Failed { error } = decision.terminal else {
            panic!("external deterministic failure must stay failed")
        };
        assert!(!error.retryable);
    }

    #[test]
    fn unknown_effect_preserves_a_successful_business_result() {
        let decision = evaluate_tool_runtime_candidate_v1(
            ToolRuntimeCandidateV1::HttpCompleted {
                status: 204,
                success_status_ranges: vec![(200, 299)],
                response_body: Vec::new(),
            },
            &ToolResultContractV1::Unit,
            &evidence(SideEffectStateV1::Unknown),
            None,
            CorrelationId::new(),
        )
        .expect("evaluation");
        assert!(matches!(decision.terminal, ToolAttemptTerminalDecisionV1::Succeeded { .. }));
        assert_eq!(decision.side_effect_state, SideEffectStateV1::Unknown);
    }

    #[test]
    fn cancellation_requires_trusted_core_intent_and_proven_termination() {
        let no_intent = evaluate_tool_runtime_candidate_v1(
            ToolRuntimeCandidateV1::CancelledAndTerminated,
            &ToolResultContractV1::Unit,
            &no_effect(),
            None,
            CorrelationId::new(),
        )
        .expect("evaluation");
        assert!(matches!(no_intent.terminal, ToolAttemptTerminalDecisionV1::Indeterminate { .. }));

        let cancelled = evaluate_tool_runtime_candidate_v1(
            ToolRuntimeCandidateV1::CancelledAndTerminated,
            &ToolResultContractV1::Unit,
            &no_effect(),
            Some(ToolCancellationReasonV1::UserRequested),
            CorrelationId::new(),
        )
        .expect("evaluation");
        assert_eq!(
            cancelled.terminal,
            ToolAttemptTerminalDecisionV1::Cancelled {
                reason: ToolCancellationReasonV1::UserRequested
            }
        );
    }

    #[test]
    fn stdio_terminal_is_candidate_until_core_schema_and_tree_validation() {
        let terminal = StdioProtocolFrameV1::FinalResult {
            protocol_version: STDIO_PROTOCOL_VERSION_V1,
            tool_call_id: ToolCallId::new(),
            tool_execution_attempt_id: ToolExecutionAttemptId::new(),
            sequence: 2,
            payload: StdioFinalResultPayloadV1 { result: json!({"ok": true}) },
        };
        let decision = evaluate_tool_runtime_candidate_v1(
            ToolRuntimeCandidateV1::StdioCompleted {
                terminal: Some(terminal),
                process_tree_terminated: true,
            },
            &ToolResultContractV1::StructuredJson {
                result_schema: ToolStructuredResultSchemaV1::new(json!({
                    "type": "object",
                    "required": ["ok"],
                    "properties": {"ok": {"type": "boolean"}},
                    "additionalProperties": false
                }))
                .expect("schema"),
            },
            &no_effect(),
            None,
            CorrelationId::new(),
        )
        .expect("evaluation");
        assert!(matches!(decision.terminal, ToolAttemptTerminalDecisionV1::Succeeded { .. }));
    }
}
