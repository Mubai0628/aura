use std::collections::BTreeSet;

use aura_domain_common::{ErrorCategory, ErrorCode};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ToolNotExecutedReasonV1;

macro_rules! define_error_domain {
    ($name:ident, { $($variant:ident => $code:literal),+ $(,)? }) => {
        #[derive(
            Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
        )]
        pub enum $name {
            $(#[serde(rename = $code)] $variant),+
        }

        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $code),+
                }
            }

            #[must_use]
            pub fn error_code(self) -> ErrorCode {
                ErrorCode::new(self.as_str()).expect("P07 error codes are compile-time validated")
            }
        }
    };
}

define_error_domain!(ToolSchemaErrorCode, {
    UnsupportedDialect => "tool.schema.unsupported_dialect",
    UnsupportedKeyword => "tool.schema.unsupported_keyword",
    ExternalReferenceDenied => "tool.schema.external_reference_denied",
    UnsupportedFormat => "tool.schema.unsupported_format",
    InvalidPattern => "tool.schema.invalid_pattern",
    ComplexityLimitExceeded => "tool.schema.complexity_limit_exceeded",
    MetaSchemaInvalid => "tool.schema.meta_schema_invalid",
    ValidatorCompilationFailed => "tool.schema.validator_compilation_failed",
    InputRootNotObject => "tool.schema.input_root_not_object",
    ReservedProviderMetadataProperty => "tool.schema.reserved_provider_metadata_property",
    ResultRootTypeInvalid => "tool.schema.result_root_type_invalid",
    HashMismatch => "tool.schema.hash_mismatch",
    ArgumentsTooLarge => "tool.schema.arguments_too_large",
    ArgumentsInvalid => "tool.schema.arguments_invalid",
});

define_error_domain!(ToolRegistryErrorCode, {
    SettingsInvalid => "tool.registry.settings_invalid",
    SnapshotNotFound => "tool.registry.snapshot_not_found",
    SnapshotUnavailable => "tool.registry.snapshot_unavailable",
    SnapshotCorrupted => "tool.registry.snapshot_corrupted",
    SnapshotVersionMismatch => "tool.registry.snapshot_version_mismatch",
    PinUnresolved => "tool.registry.pin_unresolved",
    PinDegraded => "tool.registry.pin_degraded",
    DynamicSchemaLimitBelowPinnedCount => "tool.registry.dynamic_schema_limit_below_pinned_count",
    CursorExpired => "tool.registry.cursor_expired",
    CursorUnavailable => "tool.registry.cursor_unavailable",
    CursorMismatch => "tool.registry.cursor_mismatch",
    SearchRequestInvalid => "tool.registry.search_request_invalid",
    ToolNotFound => "tool.registry.tool_not_found",
    VersionMismatch => "tool.registry.version_mismatch",
    TrustRequired => "tool.registry.trust_required",
    Disabled => "tool.registry.disabled",
    AudienceDenied => "tool.registry.audience_denied",
    CapacityExceeded => "tool.registry.capacity_exceeded",
});

define_error_domain!(ToolPackageErrorCode, {
    ManifestMissing => "tool.package.manifest_missing",
    ManifestInvalid => "tool.package.manifest_invalid",
    ManifestTooLarge => "tool.package.manifest_too_large",
    PackageKeyMismatch => "tool.package.package_key_mismatch",
    VersionContentConflict => "tool.package.version_content_conflict",
    ArchiveTooLarge => "tool.package.archive_too_large",
    ExpandedSizeExceeded => "tool.package.expanded_size_exceeded",
    EntryCountExceeded => "tool.package.entry_count_exceeded",
    FileSizeExceeded => "tool.package.file_size_exceeded",
    CompressionRatioExceeded => "tool.package.compression_ratio_exceeded",
    PathDepthExceeded => "tool.package.path_depth_exceeded",
    InvalidPath => "tool.package.invalid_path",
    CaseCollision => "tool.package.case_collision",
    SpecialFileDenied => "tool.package.special_file_denied",
    EncryptedArchiveDenied => "tool.package.encrypted_archive_denied",
    UnsupportedCompression => "tool.package.unsupported_compression",
    ClosedFileSetMismatch => "tool.package.closed_file_set_mismatch",
    FileHashMismatch => "tool.package.file_hash_mismatch",
    RuntimeClosureInvalid => "tool.package.runtime_closure_invalid",
    FingerprintMismatch => "tool.package.fingerprint_mismatch",
    Corrupted => "tool.package.corrupted",
    PayloadUnavailable => "tool.package.payload_unavailable",
    ImportJournalFailed => "tool.package.import_journal_failed",
    DeletionApprovalInvalid => "tool.package.deletion_approval_invalid",
    DeletionImpactChanged => "tool.package.deletion_impact_changed",
    DeletionPending => "tool.package.deletion_pending",
    DeletionJournalFailed => "tool.package.deletion_journal_failed",
    SourceUnavailable => "tool.package.source_unavailable",
});

define_error_domain!(ToolBatchErrorCode, {
    ToolCallIdentityConflict => "tool.batch.tool_call_identity_conflict",
    IdentityMismatch => "tool.batch.identity_mismatch",
    ContentHashMismatch => "tool.batch.content_hash_mismatch",
    InvocationBoundaryInvalid => "tool.batch.invocation_boundary_invalid",
    RegistrySnapshotMismatch => "tool.batch.registry_snapshot_mismatch",
    ControlStateMismatch => "tool.batch.control_state_mismatch",
    DependencyCycle => "tool.batch.dependency_cycle",
    AtomicGroupInvalid => "tool.batch.atomic_group_invalid",
    ProviderMappingAmbiguous => "tool.batch.provider_mapping_ambiguous",
    CompletionBatchInvalid => "tool.batch.completion_batch_invalid",
    PlanInvalidated => "tool.batch.plan_invalidated",
    Rejected => "tool.batch.rejected",
});

define_error_domain!(ToolResourceErrorCode, {
    HttpModelAuthenticationForbidden => "tool.resource.http_model_authentication_forbidden",
    HttpRequestInvalid => "tool.resource.http_request_invalid",
    HttpCredentialUnavailable => "tool.resource.http_credential_unavailable",
    HttpCredentialRebindRequired => "tool.resource.http_credential_rebind_required",
    HttpCredentialOriginDenied => "tool.resource.http_credential_origin_denied",
    SelectorMissing => "tool.resource.selector_missing",
    SelectorTypeMismatch => "tool.resource.selector_type_mismatch",
    CanonicalizationFailed => "tool.resource.canonicalization_failed",
    IdentityResolutionFailed => "tool.resource.identity_resolution_failed",
    ScopeResolutionFailed => "tool.resource.scope_resolution_failed",
    LockKeyInvalid => "tool.resource.lock_key_invalid",
    UnsupportedClaim => "tool.resource.unsupported_claim",
    ResolverUnavailable => "tool.resource.resolver_unavailable",
});

define_error_domain!(ToolSchedulerErrorCode, {
    ConcurrencyLimitInvalid => "tool.scheduler.concurrency_limit_invalid",
    TimeoutInvalid => "tool.scheduler.timeout_invalid",
    RetryLimitInvalid => "tool.scheduler.retry_limit_invalid",
    ResourceWaitCancelled => "tool.scheduler.resource_wait_cancelled",
    ExecutionPlanInvalid => "tool.scheduler.execution_plan_invalid",
    ExecutionPlanHashMismatch => "tool.scheduler.execution_plan_hash_mismatch",
    DispatchStateInvalid => "tool.scheduler.dispatch_state_invalid",
    OperationAlreadyTerminal => "tool.scheduler.operation_already_terminal",
    OperationNotCancellable => "tool.scheduler.operation_not_cancellable",
});

define_error_domain!(ToolRuntimeErrorCode, {
    HttpModelAuthenticationForbidden => "tool.runtime.http_model_authentication_forbidden",
    HttpControlEndpointDenied => "tool.runtime.http_control_endpoint_denied",
    HttpCredentialReflectionBlocked => "tool.runtime.http_credential_reflection_blocked",
    Unavailable => "tool.runtime.unavailable",
    DispatchOutcomeUnknown => "tool.runtime.dispatch_outcome_unknown",
    DispatchFailed => "tool.runtime.dispatch_failed",
    Timeout => "tool.runtime.timeout",
    IoLimitExceeded => "tool.runtime.io_limit_exceeded",
    ProcessLaunchFailed => "tool.runtime.process_launch_failed",
    ProcessTreeTerminationUnknown => "tool.runtime.process_tree_termination_unknown",
    ProtocolFrameInvalid => "tool.runtime.protocol_frame_invalid",
    ProtocolSequenceInvalid => "tool.runtime.protocol_sequence_invalid",
    ProtocolHandshakeInvalid => "tool.runtime.protocol_handshake_invalid",
    ProtocolTerminalMissing => "tool.runtime.protocol_terminal_missing",
    ProtocolTerminalConflict => "tool.runtime.protocol_terminal_conflict",
    ArtifactInvalid => "tool.runtime.artifact_invalid",
    HttpRequestInvalid => "tool.runtime.http_request_invalid",
    HttpRedirectDenied => "tool.runtime.http_redirect_denied",
    HttpGatewayFailed => "tool.runtime.http_gateway_failed",
    CredentialBindingInvalid => "tool.runtime.credential_binding_invalid",
    PersistenceOutcomeUnknown => "tool.runtime.persistence_outcome_unknown",
});

// The target of an HTTP tool could not be reached, by kind. Except after a reset, the request
// was not sent, and trying again is the next step once the cause is fixed.
define_error_domain!(ToolNetworkErrorCode, {
    ConnectionRefused => "tool.network.connection_refused",
    HostNotFound => "tool.network.host_not_found",
    TlsFailed => "tool.network.tls_failed",
    ConnectionReset => "tool.network.connection_reset",
    ConnectionFailed => "tool.network.connection_failed",
});

define_error_domain!(ToolResultErrorCode, {
    Utf8Invalid => "tool.result.utf8_invalid",
    JsonInvalid => "tool.result.json_invalid",
    SchemaValidationFailed => "tool.result.schema_validation_failed",
    ArtifactMissing => "tool.result.artifact_missing",
    EvidenceInvalid => "tool.result.evidence_invalid",
    SideEffectEvidenceInsufficient => "tool.result.side_effect_evidence_insufficient",
    OutcomeStateInvalid => "tool.result.outcome_state_invalid",
    ReconciliationInconclusive => "tool.result.reconciliation_inconclusive",
});

define_error_domain!(ToolConformanceErrorCode, {
    CaseMissing => "tool.conformance.case_missing",
    CaseInvalid => "tool.conformance.case_invalid",
    IsolationUnavailable => "tool.conformance.isolation_unavailable",
    RunFailed => "tool.conformance.run_failed",
    EvidenceInvalid => "tool.conformance.evidence_invalid",
});

// A user's Hook stopped the call before it ran. The message names the Hook and its
// reason; the call is refused like a permission denial and never executed.
define_error_domain!(ToolHookErrorCode, {
    Blocked => "tool.hook.blocked",
});

define_error_domain!(ToolDraftErrorCode, {
    NotFound => "tool.draft.not_found",
    RevisionConflict => "tool.draft.revision_conflict",
    ValidationFailed => "tool.draft.validation_failed",
    TestRequired => "tool.draft.test_required",
    ApplyFailed => "tool.draft.apply_failed",
    JournalFailed => "tool.draft.journal_failed",
    AuthenticationRequired => "tool.draft.authentication_required",
    IdentityMismatch => "tool.draft.identity_mismatch",
    SnapshotConflict => "tool.draft.snapshot_conflict",
    PreviewStale => "tool.draft.preview_stale",
    IdempotencyConflict => "tool.draft.idempotency_conflict",
    InvalidTransition => "tool.draft.invalid_transition",
    ApprovalRequired => "tool.draft.approval_required",
    ApprovalMismatch => "tool.draft.approval_mismatch",
});

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolErrorVisibilityV1 {
    InternalOnly,
    RendererSafe,
    ModelSafe,
    RendererAndModelSafe,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolErrorRetryClassV1 {
    Never,
    Conditional,
    Retryable,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolErrorCatalogEntryV1 {
    pub code: ErrorCode,
    pub category: ErrorCategory,
    pub visibility: ToolErrorVisibilityV1,
    pub retry_class: ToolErrorRetryClassV1,
    pub safe_message_key: String,
    pub suggested_not_executed_reason: Option<ToolNotExecutedReasonV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolErrorCatalogV1 {
    pub catalog_version: u32,
    pub entries: Vec<ToolErrorCatalogEntryV1>,
}

#[must_use]
pub fn tool_error_catalog_v1() -> ToolErrorCatalogV1 {
    let mut entries = Vec::new();
    push_domain(
        &mut entries,
        ToolSchemaErrorCode::ALL.iter().map(|value| value.as_str()),
        ErrorCategory::Validation,
        ToolErrorRetryClassV1::Never,
        Some(ToolNotExecutedReasonV1::SchemaValidationFailed),
    );
    push_domain(
        &mut entries,
        ToolRegistryErrorCode::ALL.iter().map(|value| value.as_str()),
        ErrorCategory::Conflict,
        ToolErrorRetryClassV1::Never,
        None,
    );
    push_domain(
        &mut entries,
        ToolPackageErrorCode::ALL.iter().map(|value| value.as_str()),
        ErrorCategory::Validation,
        ToolErrorRetryClassV1::Never,
        None,
    );
    push_domain(
        &mut entries,
        ToolBatchErrorCode::ALL.iter().map(|value| value.as_str()),
        ErrorCategory::Conflict,
        ToolErrorRetryClassV1::Never,
        Some(ToolNotExecutedReasonV1::BatchRejected),
    );
    push_domain(
        &mut entries,
        ToolResourceErrorCode::ALL.iter().map(|value| value.as_str()),
        ErrorCategory::Validation,
        ToolErrorRetryClassV1::Never,
        Some(ToolNotExecutedReasonV1::ResourceClaimResolutionFailed),
    );
    push_domain(
        &mut entries,
        ToolSchedulerErrorCode::ALL.iter().map(|value| value.as_str()),
        ErrorCategory::Conflict,
        ToolErrorRetryClassV1::Conditional,
        None,
    );
    push_domain(
        &mut entries,
        ToolRuntimeErrorCode::ALL.iter().map(|value| value.as_str()),
        ErrorCategory::Unavailable,
        ToolErrorRetryClassV1::Conditional,
        None,
    );
    push_domain(
        &mut entries,
        ToolNetworkErrorCode::ALL.iter().map(|value| value.as_str()),
        ErrorCategory::Transport,
        ToolErrorRetryClassV1::Conditional,
        None,
    );
    push_domain(
        &mut entries,
        ToolResultErrorCode::ALL.iter().map(|value| value.as_str()),
        ErrorCategory::Protocol,
        ToolErrorRetryClassV1::Never,
        None,
    );
    push_domain(
        &mut entries,
        ToolConformanceErrorCode::ALL.iter().map(|value| value.as_str()),
        ErrorCategory::Validation,
        ToolErrorRetryClassV1::Never,
        None,
    );
    push_domain(
        &mut entries,
        ToolDraftErrorCode::ALL.iter().map(|value| value.as_str()),
        ErrorCategory::Conflict,
        ToolErrorRetryClassV1::Never,
        None,
    );
    push_domain(
        &mut entries,
        ToolHookErrorCode::ALL.iter().map(|value| value.as_str()),
        ErrorCategory::Permission,
        ToolErrorRetryClassV1::Never,
        Some(ToolNotExecutedReasonV1::PermissionDenied),
    );
    entries.sort_by(|left, right| left.code.cmp(&right.code));
    ToolErrorCatalogV1 { catalog_version: 1, entries }
}

fn push_domain<'a>(
    entries: &mut Vec<ToolErrorCatalogEntryV1>,
    codes: impl Iterator<Item = &'a str>,
    category: ErrorCategory,
    retry_class: ToolErrorRetryClassV1,
    suggested_not_executed_reason: Option<ToolNotExecutedReasonV1>,
) {
    entries.extend(codes.map(|code| ToolErrorCatalogEntryV1 {
        code: ErrorCode::new(code).expect("P07 error codes are compile-time validated"),
        category,
        visibility: ToolErrorVisibilityV1::RendererAndModelSafe,
        retry_class,
        safe_message_key: code.replace('.', "_"),
        suggested_not_executed_reason,
    }));
}

pub fn validate_tool_error_catalog(catalog: &ToolErrorCatalogV1) -> Result<(), &'static str> {
    if catalog.catalog_version != 1 {
        return Err("unsupported tool error catalog version");
    }
    let expected = tool_error_catalog_v1();
    if catalog.entries != expected.entries {
        return Err("tool error catalog differs from the closed Rust registries");
    }
    let mut codes = BTreeSet::new();
    for entry in &catalog.entries {
        if !entry.code.as_str().starts_with("tool.") || !codes.insert(entry.code.clone()) {
            return Err("tool error catalog has an invalid or duplicate code");
        }
        if entry.safe_message_key.is_empty() {
            return Err("renderer-safe tool error is missing a safe message key");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ToolBatchErrorCode, tool_error_catalog_v1, validate_tool_error_catalog};

    #[test]
    fn generated_catalog_is_closed_unique_and_namespaced() {
        let catalog = tool_error_catalog_v1();
        validate_tool_error_catalog(&catalog).expect("closed catalog");
        assert!(catalog.entries.len() > 80);
    }

    #[test]
    fn domain_error_enum_serializes_as_the_complete_public_wire_code() {
        let code = ToolBatchErrorCode::ToolCallIdentityConflict;
        assert_eq!(
            serde_json::to_string(&code).expect("error code wire"),
            "\"tool.batch.tool_call_identity_conflict\""
        );
        assert_eq!(
            serde_json::from_str::<ToolBatchErrorCode>(
                "\"tool.batch.tool_call_identity_conflict\""
            )
            .expect("error code from wire"),
            code
        );
    }
}
