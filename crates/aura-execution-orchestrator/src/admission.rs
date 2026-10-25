//! Extracted P07 admission, planning and correlated projections, shared by the
//! production V2 application and the retained R12 Package execution adapter.
use aura_domain_common::{CausationId, CorrelationId, ErrorEnvelope, UtcTimestamp};
use aura_tool_contract::{
    ToolBatchErrorCode, ToolBatchRequestV1, ToolErrorRetryClassV1, ToolRegistrySnapshotV1,
    ToolResultV1, ToolSettingsSnapshotV1, tool_error_catalog_v1,
};
use aura_tool_domain::ToolDefinitionRecordV1;
use aura_tool_projection::{
    ToolAdmissionProjectionSetV1, ToolExecutionProjectionErrorV1, ToolExecutionProjectionStateV1,
};
use aura_tool_scheduler::*;
use thiserror::Error;

pub struct PrepareBatch<'a> {
    pub batch: ToolBatchRequestV1,
    pub registry: &'a ToolRegistrySnapshotV1,
    pub definitions: Vec<ToolDefinitionRecordV1>,
    pub admission_authority: &'a dyn BatchAdmissionAuthorityV1,
    pub settings: &'a ToolSettingsSnapshotV1,
    pub planning_inputs: Vec<FrozenToolCallPlanningInputV1>,
    pub admitted_at: UtcTimestamp,
    pub correlation_id: CorrelationId,
    pub causation_id: Option<CausationId>,
}
pub enum BatchPreparation {
    IdentityIntegrityFault { code: ToolBatchErrorCode },
    BatchRejected { code: ToolBatchErrorCode, results: Vec<ToolResultV1> },
    Prepared(Box<PreparedBatch>),
}
pub struct PreparedBatch {
    pub admitted: AdmittedToolBatchV1,
    pub planned: PlannedToolBatchV1,
    pub projection_state: ToolExecutionProjectionStateV1,
    pub projections: ToolAdmissionProjectionSetV1,
    pub not_executed_results: Vec<ToolResultV1>,
    pub admitted_at: UtcTimestamp,
    pub correlation_id: CorrelationId,
}
pub fn prepare_batch(input: PrepareBatch<'_>) -> Result<BatchPreparation, BatchPreparationError> {
    let admitted = match admit_tool_batch(
        input.batch,
        input.registry,
        input.definitions,
        input.admission_authority,
    )? {
        ToolBatchAdmissionOutcomeV1::IdentityIntegrityFault { code } => {
            return Ok(BatchPreparation::IdentityIntegrityFault { code });
        }
        ToolBatchAdmissionOutcomeV1::BatchRejected { code, calls } => {
            return Ok(BatchPreparation::BatchRejected {
                code,
                results: rejection_results(
                    &calls,
                    input.admitted_at,
                    input.correlation_id,
                    input.causation_id,
                )?,
            });
        }
        ToolBatchAdmissionOutcomeV1::Admitted(admitted) => *admitted,
    };
    let planned = build_tool_execution_plan_v1(
        &admitted,
        input.registry,
        input.settings,
        input.planning_inputs,
    )?;
    let not_executed_results = rejection_results(
        &planned.not_executed,
        input.admitted_at,
        input.correlation_id,
        input.causation_id,
    )?;
    let display_names = admitted
        .batch
        .calls
        .iter()
        .map(|call| {
            let name = input
                .registry
                .catalog_entries
                .iter()
                .find(|entry| {
                    call.definition_ref.as_ref().is_some_and(|reference| {
                        entry.tool_id == reference.tool_id
                            && entry.tool_version == reference.tool_version
                    })
                })
                .map_or_else(
                    || {
                        call.definition_ref
                            .as_ref()
                            .map_or_else(|| "Unknown tool".into(), |r| r.tool_id.to_string())
                    },
                    |entry| entry.display_name.clone(),
                );
            (call.tool_call_id, name)
        })
        .collect::<Vec<_>>();
    let (projection_state, projections) = ToolExecutionProjectionStateV1::admitted(
        &admitted.batch,
        &planned.execution_plan,
        &display_names,
        &not_executed_results,
    )?;
    Ok(BatchPreparation::Prepared(Box::new(PreparedBatch {
        admitted,
        planned,
        projection_state,
        projections,
        not_executed_results,
        admitted_at: input.admitted_at,
        correlation_id: input.correlation_id,
    })))
}

pub fn rejection_results(
    rejections: &[ToolPreflightRejectionV1],
    at: UtcTimestamp,
    correlation: CorrelationId,
    causation: Option<CausationId>,
) -> Result<Vec<ToolResultV1>, BatchPreparationError> {
    rejections
        .iter()
        .map(|rejection| {
            let error = rejection
                .error_code
                .as_ref()
                .map(|code| {
                    let entry = tool_error_catalog_v1()
                        .entries
                        .into_iter()
                        .find(|entry| &entry.code == code)
                        .ok_or(BatchPreparationError::UnregisteredErrorCode)?;
                    Ok::<_, BatchPreparationError>(
                        ErrorEnvelope::new(
                            entry.code,
                            entry.category,
                            rejection_message(rejection),
                            entry.retry_class == ToolErrorRetryClassV1::Retryable,
                            correlation,
                            causation,
                        )
                        .with_detail(
                            "safe_message_key",
                            serde_json::Value::String(entry.safe_message_key),
                        ),
                    )
                })
                .transpose()?;
            preflight_rejection_result_v1(rejection, at, error).map_err(BatchPreparationError::from)
        })
        .collect()
}

fn rejection_message(rejection: &ToolPreflightRejectionV1) -> String {
    if let Some(message) = input_issue_message(rejection) {
        return message;
    }
    if let Some(report) = &rejection.schema_validation {
        let fields = report
            .errors
            .iter()
            .take(8)
            .map(|issue| {
                format!(
                    "{}: {} (schema {})",
                    issue.instance_path,
                    issue.issue_kind.chars().take(512).collect::<String>(),
                    issue.schema_path
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        return format!("Arguments were not executed. Correct the following fields: {fields}");
    }
    if matches!(
        rejection.reason,
        aura_tool_contract::ToolNotExecutedReasonV1::PurposeRequired
            | aura_tool_contract::ToolNotExecutedReasonV1::SchemaValidationFailed
    ) {
        return "Arguments were not executed. Send one JSON object with the tool's own fields and `aura_purpose` (one short sentence: why you make the call), and obey the supplied schema.".into();
    }
    // A resource that could not be resolved has a class, and each class needs a
    // different correction. Saying only "preflight failed" would leave the model
    // with nothing to change.
    if let Some(code) = &rejection.error_code {
        if code
            == &aura_tool_contract::ToolResourceErrorCode::HttpCredentialRebindRequired.error_code()
        {
            return "凭据要求已变化，当前绑定不能使用。请在工具设置中重新绑定凭据，并核对允许网站。本次调用未执行，尚未发送请求。".into();
        }
        if code
            == &aura_tool_contract::ToolResourceErrorCode::HttpModelAuthenticationForbidden
                .error_code()
        {
            return "认证信息只能通过工具凭据提供（工具设置里配置凭据与允许网站）。本次调用未执行；请移除请求中的认证信息，再选择已配置的工具凭据。".into();
        }
        if code == &aura_tool_contract::ToolResourceErrorCode::ScopeResolutionFailed.error_code() {
            return "The path was not executed: it names a location outside this session's authorized workspace. Use a path relative to the workspace root, or an absolute path that lies inside it. Do not retry the same path.".into();
        }
        if code == &aura_tool_contract::ToolResourceErrorCode::CanonicalizationFailed.error_code() {
            return "The path was not executed because the path argument itself is not a usable workspace path. Correct the path field and retry: remove parent traversal, device or stream syntax, and reserved names.".into();
        }
        if code == &aura_tool_contract::ToolResourceErrorCode::ResolverUnavailable.error_code() {
            return "The path was not executed because no resolver is available in this session's current state. Check the workspace's state before retrying.".into();
        }
    }
    "The tool call was not executed during preflight.".into()
}

/// The correction for an argument envelope the normalizer refused.
///
/// Each issue names the one thing to change. The generic template below says "send
/// exactly `_aura`/`arguments`", which is true but does not tell a model that put
/// `path` inside `_aura` which field has to move — and the repeated Shell failures were
/// exactly that shape. The offending names come from the bounded diagnostic, so this
/// never repeats the argument text.
fn input_issue_message(rejection: &ToolPreflightRejectionV1) -> Option<String> {
    use aura_tool_contract::NormalizedToolCallInputIssueV1 as Issue;
    let issue = rejection.input_issue?;
    let names = rejection
        .input_diagnostic
        .as_ref()
        .map(|detail| detail.fields.join(", "))
        .filter(|names| !names.is_empty());
    let at = |fallback: &str| {
        rejection
            .input_diagnostic
            .as_ref()
            .map(|detail| detail.pointer.clone())
            .unwrap_or_else(|| fallback.to_owned())
    };
    let message = match issue {
        Issue::AuraMetadataUnexpectedField => format!(
            "The call was not executed. `_aura` may contain only `purpose`; {} must not be inside it. Keep `_aura` as {{\"purpose\":\"...\"}} and put every tool field beside `_aura`, inside `arguments`.",
            match &names {
                Some(names) => format!("the field(s) {names}"),
                None => "extra fields".to_owned(),
            }
        ),
        Issue::AuraMetadataShapeInvalid => format!(
            "The call was not executed. {} must be an object holding only `purpose`, for example {{\"_aura\":{{\"purpose\":\"...\"}},\"arguments\":{{...}}}}.",
            at("/_aura")
        ),
        Issue::AuraMetadataMissing => "The call was not executed: `_aura` is missing. Send exactly {\"_aura\":{\"purpose\":\"short purpose\"},\"arguments\":{...tool fields...}}.".to_owned(),
        // Envelope 3 is the only one new calls are bound to; the `_aura` messages above and
        // below can only describe a call bound before it.
        Issue::PurposeMissing => "The call was not executed: `aura_purpose` is missing. Add \"aura_purpose\": \"<why you make this call>\" beside the tool's own fields.".to_owned(),
        Issue::PurposeInvalid => format!(
            "The call was not executed: {} must be a non-empty text without control characters.",
            at("/aura_purpose")
        ),
        Issue::PurposeTooLong => format!(
            "The call was not executed: {} is longer than 4096 bytes. Use one short sentence.",
            at("/aura_purpose")
        ),
        Issue::EnvelopeUnexpectedField => format!(
            "The call was not executed. Send the tool's fields directly at the top level beside `aura_purpose`, without an `_aura` or `arguments` wrapper; remove {}.",
            match &names {
                Some(names) => format!("the field(s) {names}"),
                None => "the wrapper".to_owned(),
            }
        ),
        Issue::ArgumentsMissing => "The call was not executed: `arguments` is missing. Put the tool's fields inside it, beside `_aura`.".to_owned(),
        Issue::ArgumentsRootInvalid => format!(
            "The call was not executed: {} must be an object holding the tool's fields.",
            at("/arguments")
        ),
        Issue::ArgumentsJsonDuplicateKey => format!(
            "The call was not executed: {} appears twice in the same JSON object. Send each key once.",
            match &names {
                Some(names) => format!("the key {names}"),
                None => "a key".to_owned(),
            }
        ),
        Issue::ArgumentsJsonInvalid => "The call was not executed: the arguments were not a complete JSON value. Send one well-formed JSON object.".to_owned(),
        Issue::EnvelopeRootInvalid => "The call was not executed: the arguments must be one JSON object holding the tool's fields and `aura_purpose`.".to_owned(),
        Issue::EnvelopeShapeInvalid | Issue::AuraMetadataInvalid => "The call was not executed. Send one JSON object (at most 512 KiB) with the tool's own fields and `aura_purpose`.".to_owned(),
    };
    Some(message)
}
#[derive(Debug, Error)]
pub enum BatchPreparationError {
    #[error(transparent)]
    Admission(#[from] ToolBatchAdmissionInputError),
    #[error(transparent)]
    Planning(#[from] ToolExecutionPlanningErrorV1),
    #[error(transparent)]
    Projection(#[from] ToolExecutionProjectionErrorV1),
    #[error("a preflight rejection references an error outside the closed Tool catalog")]
    UnregisteredErrorCode,
}

#[cfg(test)]
mod tests {
    use super::rejection_message;
    use aura_tool_contract::{ToolNotExecutedReasonV1, ToolResourceErrorCode};
    use aura_tool_scheduler::ToolPreflightRejectionV1;

    fn rejection(code: ToolResourceErrorCode) -> ToolPreflightRejectionV1 {
        ToolPreflightRejectionV1 {
            tool_call_id: aura_domain_common::ToolCallId::new(),
            ordinal: 1,
            reason: ToolNotExecutedReasonV1::ResourceClaimResolutionFailed,
            error_code: Some(code.error_code()),
            schema_validation: None,
            input_issue: None,
            input_diagnostic: None,
        }
    }

    /// The correction for an argument-envelope mistake names the field to move.
    ///
    /// The generic template says "send exactly `_aura`/`arguments`", which is true and
    /// useless to a model that put `path` inside `_aura`: it has to be told which field
    /// is in the wrong place, and the message must not become a copy of the arguments.
    #[test]
    fn an_envelope_issue_names_the_field_that_has_to_move() {
        let rejection = ToolPreflightRejectionV1 {
            tool_call_id: aura_domain_common::ToolCallId::new(),
            ordinal: 1,
            reason: ToolNotExecutedReasonV1::SchemaValidationFailed,
            error_code: Some(
                aura_tool_contract::ToolSchemaErrorCode::ArgumentsInvalid.error_code(),
            ),
            schema_validation: None,
            input_issue: Some(
                aura_tool_contract::NormalizedToolCallInputIssueV1::AuraMetadataUnexpectedField,
            ),
            input_diagnostic: Some(Box::new(
                aura_tool_contract::NormalizedToolCallInputDiagnosticV1::new(
                    "/_aura",
                    vec!["limit".to_owned(), "path".to_owned()],
                ),
            )),
        };
        let message = rejection_message(&rejection);
        assert!(message.contains("_aura"), "{message}");
        assert!(message.contains("limit"), "{message}");
        assert!(message.contains("path"), "{message}");
        assert!(
            message.contains("arguments"),
            "the correction must say where the fields belong: {message}"
        );
        assert!(message.len() < 512, "the correction stays bounded: {}", message.len());
    }

    #[test]
    fn a_missing_purpose_is_not_reported_as_a_permission_problem() {
        let rejection = ToolPreflightRejectionV1 {
            tool_call_id: aura_domain_common::ToolCallId::new(),
            ordinal: 1,
            reason: ToolNotExecutedReasonV1::PurposeRequired,
            error_code: Some(
                aura_tool_contract::ToolSchemaErrorCode::ArgumentsInvalid.error_code(),
            ),
            schema_validation: None,
            input_issue: Some(aura_tool_contract::NormalizedToolCallInputIssueV1::PurposeMissing),
            input_diagnostic: None,
        };
        let message = rejection_message(&rejection);
        assert!(message.contains("purpose"), "{message}");
        assert!(!message.contains("permission"), "{message}");
    }

    /// Envelope 3 corrections describe the flat object: the purpose sits beside the tool's
    /// own fields, and the earlier `_aura`/`arguments` wrapper is what has to go.
    #[test]
    fn envelope_3_corrections_describe_the_flat_object() {
        use aura_tool_contract::{
            NormalizedToolCallInputDiagnosticV1 as Diagnostic,
            NormalizedToolCallInputIssueV1 as Issue,
        };
        let rejection = |issue, diagnostic: Diagnostic| ToolPreflightRejectionV1 {
            tool_call_id: aura_domain_common::ToolCallId::new(),
            ordinal: 1,
            reason: ToolNotExecutedReasonV1::SchemaValidationFailed,
            error_code: Some(
                aura_tool_contract::ToolSchemaErrorCode::ArgumentsInvalid.error_code(),
            ),
            schema_validation: None,
            input_issue: Some(issue),
            input_diagnostic: Some(Box::new(diagnostic)),
        };
        let missing = rejection_message(&rejection(
            Issue::PurposeMissing,
            Diagnostic::new("/aura_purpose", vec![]),
        ));
        assert!(missing.contains("\"aura_purpose\""), "{missing}");
        assert!(!missing.contains("_aura."), "no earlier envelope wording: {missing}");
        let wrapped = rejection_message(&rejection(
            Issue::EnvelopeUnexpectedField,
            Diagnostic::new("$", vec!["_aura".to_owned()]),
        ));
        assert!(wrapped.contains("top level") && wrapped.contains("aura_purpose"), "{wrapped}");
        assert!(wrapped.contains("the field(s) _aura"), "{wrapped}");
        let generic = rejection_message(&ToolPreflightRejectionV1 {
            input_issue: None,
            input_diagnostic: None,
            ..rejection(Issue::PurposeMissing, Diagnostic::new("$", vec![]))
        });
        assert!(generic.contains("aura_purpose") && !generic.contains("\"_aura\""), "{generic}");
    }

    /// Each class of resource failure tells the model what to change. Collapsing
    /// them into one generic sentence is the behaviour this guards against.
    #[test]
    fn a_resource_failure_message_names_the_class_and_the_correction() {
        let scope = rejection_message(&rejection(ToolResourceErrorCode::ScopeResolutionFailed));
        assert!(scope.contains("outside"), "{scope}");
        assert!(scope.contains("workspace"), "{scope}");
        assert!(scope.contains("Do not retry the same path"), "{scope}");

        let canonical =
            rejection_message(&rejection(ToolResourceErrorCode::CanonicalizationFailed));
        assert!(canonical.contains("path argument"), "{canonical}");
        assert!(canonical.contains("parent traversal"), "{canonical}");

        let resolver = rejection_message(&rejection(ToolResourceErrorCode::ResolverUnavailable));
        assert!(resolver.contains("resolver"), "{resolver}");

        // A class with no specific guidance still says something true rather than
        // borrowing another class's wording.
        let generic =
            rejection_message(&rejection(ToolResourceErrorCode::IdentityResolutionFailed));
        assert_eq!(generic, "The tool call was not executed during preflight.");
    }

    /// Different classes must not share one message, or the model cannot tell them
    /// apart.
    #[test]
    fn distinct_resource_classes_produce_distinct_messages() {
        let scope = rejection_message(&rejection(ToolResourceErrorCode::ScopeResolutionFailed));
        let canonical =
            rejection_message(&rejection(ToolResourceErrorCode::CanonicalizationFailed));
        assert_ne!(scope, canonical);
    }

    /// The rejection becomes a `not_executed` result that explains the envelope.
    ///
    /// This is the end of the production chain the three observed mistakes travelled:
    /// the rejection is turned into the stored Tool result here, and the properties that
    /// matter are that nothing ran — no attempt, no side effect — and that the stored
    /// message names the field instead of the generic template.
    #[test]
    fn an_envelope_rejection_becomes_a_not_executed_result_that_names_the_field() {
        use aura_tool_contract::{
            NormalizedToolCallInputDiagnosticV1 as Diagnostic,
            NormalizedToolCallInputIssueV1 as Issue, ToolResultOutcomeV1,
        };
        let rejection = ToolPreflightRejectionV1 {
            tool_call_id: aura_domain_common::ToolCallId::new(),
            ordinal: 1,
            reason: ToolNotExecutedReasonV1::SchemaValidationFailed,
            error_code: Some(
                aura_tool_contract::ToolSchemaErrorCode::ArgumentsInvalid.error_code(),
            ),
            schema_validation: None,
            input_issue: Some(Issue::AuraMetadataUnexpectedField),
            input_diagnostic: Some(Box::new(Diagnostic::new("/_aura", vec!["path".to_owned()]))),
        };
        let at = aura_domain_common::UtcTimestamp::now();
        let results = super::rejection_results(
            std::slice::from_ref(&rejection),
            at,
            aura_domain_common::CorrelationId::new(),
            None,
        )
        .expect("the rejection produces a result");
        assert_eq!(results.len(), 1);
        let result = &results[0];
        let ToolResultOutcomeV1::NotExecuted { reason, error } = &result.outcome else {
            panic!("a preflight rejection is not executed: {:?}", result.outcome);
        };
        assert_eq!(*reason, ToolNotExecutedReasonV1::SchemaValidationFailed);
        assert!(result.attempts.is_empty(), "a rejected call never gained an Attempt");
        assert_eq!(
            result.side_effect_state,
            aura_execution_contract::SideEffectStateV1::NotStarted,
            "nothing was executed, so nothing may claim a side effect"
        );
        let message = error.as_ref().expect("the refusal is explained").message.clone();
        assert!(message.contains("_aura"), "{message}");
        assert!(message.contains("path"), "{message}");
        assert!(!message.contains("permission"), "{message}");
    }
}
