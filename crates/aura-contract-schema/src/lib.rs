#![forbid(unsafe_code)]

mod fixture_json;
mod tool_contract;
pub use tool_contract::ToolContractFixture;
mod browser_contract;
mod connections_contract;
mod git_fixtures;
mod hook_contract;
mod mcp_contract;
mod media_contract;
mod memory_contract;
mod network_catalog;
mod package_contract;
mod plugin_contract;
mod projection_samples;
mod skills_contract;
mod terminal_opening_fixture;
mod tool_family_contracts;
use fixture_json::pretty_fixture_json;
pub use projection_samples::full_projection_samples;
mod agent_contract;
mod v2_kernel;
mod v2_persistence;
pub use v2_kernel::{V2KernelFixture, v2_kernel_fixture};
pub use v2_persistence::v2_import_report_fixture;

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{Context, Result};
use aura_artifact_contract::{
    ArtifactAccess, ArtifactExportResult, ArtifactOwnerRef, ArtifactRecord, ArtifactScope,
    CaptureWorkspaceFileResult,
};
use aura_config::PromptSettingsSnapshot;
use aura_context_domain::{
    CONTEXT_BUDGET_POLICY_V1, CompactionAnchorManifest, CompactionModelAttemptRecord,
    ContextBudgetSnapshot, ContextItemEstimate, ContextPreparationOutcome,
    ContextPreparationTriggerV1, ContextProjectionSnapshot, ContextReadEventsInput,
    ContextReadToolResultInput, ContextSearchInput, ContextSourceRef, HistoryAtomicGroupPreimageV1,
    HistoryAtomicGroupRefV1, ManualCompactionRequestRecord, ModelEgressAuthorizationBasis,
    ModelEgressItem, ModelEgressManifestV1, ModelEgressSourceKind, PromptBundleSnapshot,
    PromptInspectorProjection, PromptInspectorTaskLineageView, ProviderContinuationBindingV1,
    StructuredCompactionRecord, TokenEstimateExactness, TokenEstimatorIdentity,
    ToolResultContextProjectionV1, UTF8_BYTE_UPPER_BOUND_ESTIMATOR_ID,
};
use aura_domain_common::CanonicalSemVer;
use aura_domain_common::jcs::CanonicalHashV1;
use aura_domain_common::{
    AgentRunId, ArtifactId, AttachmentGrantId, BrowserShareId, COMMON_ID_PREFIXES, CausationId,
    CommandId, CommandInvocationId, ContentHash, ContentHashAlgorithm, ContextProjectionSnapshotId,
    ContractVersion, CorrelationId, CredentialId, CredentialRef, CredentialRevision,
    DeferredMutationId, DeleteConfirmationId, EditorReservationId, EndpointId, ErrorCategory,
    ErrorCode, ErrorEnvelope, EventId, ExpectedRevision, ExternalGrantId, FileReferenceId,
    IpcChannelId, MemoryId, ModelAvailabilityTestId, ModelContentBlockId, ModelDiscoveryRunId,
    ModelExecutionSnapshotId, ModelFailureId, ModelProfileId, ModelStepId, MutationJournalId,
    PromptBundleSnapshotId, ProviderId, ReleaseBundleId, RendererInstanceId, Revision,
    RuntimeInstanceId, SecretNonce, SessionId, SkillInstanceId, StreamId, TaskLineageId,
    TransportAttemptId, TurnTaskId, UtcTimestamp, WorkspaceId, WorkspaceLifecycleOperationId,
    WorkspacePath,
};
use aura_domain_common::{
    TOOL_ID_PREFIXES, ToolBatchId, ToolCallId, ToolConformanceRunId, ToolDraftId,
    ToolExecutionAttemptId, ToolId, ToolInstanceId, ToolPackageId, ToolRegistrySnapshotId,
};
use aura_execution_contract::VerifiedAttemptSideEffectEvidenceV1;
use aura_model_domain::{
    AssistantContentBlock, CatalogMatchState, CompiledInstructionEnvelope,
    ContextWindowClearUserOverrideRequest, ContextWindowCorrectRequest,
    ContextWindowMutationResult, ContextWindowResetRuntimeCorrectionRequest,
    CredentialBindingFingerprint, DefaultModelMutationResult, DeleteModelConfigCommitResult,
    DeleteModelConfigResult, DormantEndpointCredentialSummary, EndpointAuthScheme,
    EndpointAuthenticationKind, EndpointAuthenticationProjection, EndpointCompatibilityOverrides,
    EndpointCompatibilitySnapshot, EndpointCreatedWithCredentialResult,
    EndpointCredentialMutationResult, EndpointCredentialState, EndpointCredentialStatus,
    EndpointDetailProjection, EndpointSlug, FailureFingerprint, LogicalInstructionBlock,
    LogicalInstructionKind, LogicalModelRequest, ModelAdapterIdentity, ModelApiFormat,
    ModelAvailabilityPrerequisiteFailure, ModelAvailabilityTestCancelRequest,
    ModelAvailabilityTestCancelResult, ModelAvailabilityTestStartAccepted,
    ModelAvailabilityTestStartRequest, ModelCatalogEnsureFreshRequest, ModelCatalogGetRequest,
    ModelCatalogMetadata, ModelCatalogProviderGetRequest, ModelCatalogRefreshRequest,
    ModelCompatibilityGetRequest, ModelCompatibilityResetRequest, ModelCompatibilityResetResult,
    ModelContextWindowConfig, ModelCredentialSlot, ModelDefaultSetRequest,
    ModelDeletionMutationResult, ModelDiscoveryCancelRequest, ModelDiscoveryCancelResult,
    ModelDiscoveryMode, ModelDiscoveryStartAccepted, ModelDiscoveryStartRequest,
    ModelEndpointClearCredentialRequest, ModelEndpointConfig,
    ModelEndpointConfigureCredentialRequest, ModelEndpointCreateRequest,
    ModelEndpointCreateWithCredentialRequest, ModelEndpointCredentialClearedPayload,
    ModelEndpointCredentialConfiguredPayload, ModelEndpointDeleteRequest,
    ModelEndpointMutationResult, ModelEndpointReplaceCredentialRequest, ModelEndpointTimeouts,
    ModelEndpointUpdateRequest, ModelExecutionSnapshot, ModelFailureCategory, ModelOutputFormat,
    ModelOutputRequest, ModelProfileApplyCatalogUpdateRequest, ModelProfileConfig,
    ModelProfileCreateFromCatalogRequest, ModelProfileCreateManualRequest,
    ModelProfileDeleteRequest, ModelProfileMutationResult, ModelProfileOrigin,
    ModelProfileSetSubagentAvailabilityRequest, ModelProfileUpdateRequest, ModelProviderConfig,
    ModelProviderCreateRequest, ModelProviderDeleteRequest, ModelProviderMutationResult,
    ModelProviderRenameRequest, ModelProviderSetAllCurrentModelsSubagentAvailabilityRequest,
    ModelProviderSource, ModelRegistryConfig, ModelRequestDefaults, ModelRetryClass,
    ModelRuntimeCapabilityFacts, ModelSettingsGetSnapshotRequest, ModelStreamEvent,
    ModelStreamEventEnvelope, ModelUsage, NormalizedModelFailure, NormalizedModelOutput,
    NormalizedStopReason, NormalizedToolCall, PersistedModelFailure,
    ProviderBulkAvailabilityResult, ProviderCatalogModelCapabilities, ProviderCatalogModelEntry,
    ProviderCatalogModelPage, ProviderContinuationState, ProviderIconReadRequest,
    ProviderIconReadResult, ProviderProtocolReference, ProviderRequestEvidence,
    ProviderRequestHeaderClass, ProviderRequestHeaderMetadata, ProviderRequestTargetMetadata,
    ProviderSlug, ProviderToolCallId, ProviderToolCallProtocolBindingRecord,
    ProviderToolNameBinding, ReasoningContentBlock, ResolvedModelRequestOptions,
    SessionModelBindingMutationResult, SessionModelBindingSetRequest, TransportAttemptKind,
    TransportAttemptRecord, TransportAttemptState, TransportOperationContext, UsageSource,
};
use aura_permission_domain::{
    ApprovalRequest, ApprovalResolution, AttachExternalReadOnlyInput, AuthorizedNetworkRequest,
    AuthorizedToolSandboxExecutionV1, ConformanceSandboxProfileV1,
    CredentialReflectionScanEvidenceV1, DeriveWorkspaceExternalGrantInput, DiscardQuarantineInput,
    ExternalExtensionRiskView, ExternalGrantRecord, ExternalGrantView, ExternalProcessLaunchPlan,
    ExternalTrustRecord, GetApprovalDetailsInput, GetSessionPermissionStateInput,
    HostPreparationBeginResult, HostPreparationMutationInput, HostPreparationStatus,
    IssueQuarantineDiscardConfirmationInput, ListNativeSessionConsentsInput,
    ListPermissionGrantsInput, ListSessionExternalGrantsInput, NativeSessionConsentScopeView,
    NativeSessionConsentView, NetworkGrant, NetworkRequestAuditRecord, NetworkRequestPlan,
    NormalizedOrigin, OperationSecurityPlan, PermissionDecision, PermissionDefaults,
    PermissionGrant, PrepareQuarantineReleaseInput, PrepareQuarantineReleaseResult,
    QuarantineDiscardConfirmation, QuarantineRecordDetails, ReleaseQuarantineInput,
    ResolveApprovalInput, ResolveHostPreparationInput, RevalidateExternalGrantInput,
    RevokeExternalGrantInput, RevokeNativeSessionConsentInput, RevokePermissionGrantInput,
    SandboxExecutionPlan, SandboxHostEvent, SandboxNetworkPolicyError, SecurityProjectionChange,
    SetPermissionDefaultsInput, SetSessionPermissionModeInput, ToolConformanceRuntimeScopeV1,
    ToolConformanceSecuritySourceRefV1, ToolSandboxDispatchBindingV1, TrustFingerprintInput,
    W7ToolOperationFingerprintPreimageV1,
};
use aura_projection_contract::{
    ProjectionChange, ProjectionContractSchemaRoot, ProjectionKey, ProjectionKind, ProjectionScope,
    ProjectionSnapshotRequest, ProjectionSubscription, ProjectionUnsubscribeRequest,
};
use aura_runtime_contract::{
    ActiveInstallState, AuthenticatedPeerIdentity, BinaryStreamFrame, BuildComponent,
    BuildComponentKind, BuildComponentManifest, BundleNamespace, CancellationRequest,
    CommandOutcome, CommandOutcomeUnknown, CoreHealthProjection, CoreStartupPhase,
    CoreStartupProgress, IpcErrorEnvelope, IpcFrameHeader, IpcRequestTerminal, ReleaseChannel,
    ReleaseDescriptor, ReleaseIdentity, RuntimeHandshake, RuntimeHandshakeComplete,
    RuntimeHandshakeFinish, RuntimeHandshakeKind, RuntimeHandshakeRequest,
    RuntimeHandshakeResponse, RuntimeHandshakeTranscript, RuntimeHeartbeat, RuntimeProcessKind,
    runtime_bootstrap_proof, runtime_handshake_transcript_hash,
};
use aura_security_contract::{
    ModelChannelDataDisposition, ModelEgressPolicyVersion, PermissionSnapshot,
};
use aura_tool_contract::{
    ArgumentJsonPointer, BuiltinToolDefinitionV1, CommandWorkingDirectoryV1,
    EffectiveToolBindingSnapshot, ModelVisibleToolResultEnvelopeV1, PackageFingerprintV1,
    PackageKey, PackageRelativePath, PackageRuntimeClosureHashV1, PackageRuntimeVisibilityPolicyV1,
    PackageSecurityFingerprintV1, ResolvedResourceClaimV1, ResourceClaimV1,
    STDIO_PROTOCOL_VERSION_V1, StdioHelloPayloadV1, StdioProtocolFrameV1,
    TOOL_PACKAGE_MANIFEST_NAME, ToolActivityCursorBindingV1, ToolActivityPageRequestV1,
    ToolBatchErrorCode, ToolBatchRequestV1, ToolBatchResultV1, ToolCanonicalName,
    ToolConformanceErrorCode, ToolContractFingerprintV1, ToolDescribeRequestV1,
    ToolDescribeResultV1, ToolDraftErrorCode, ToolErrorCatalogV1, ToolExecutionAttemptRecordV1,
    ToolExecutionPlanV1, ToolInputSchemaV1, ToolKey, ToolLifecycleEventV1, ToolListLoadedRequestV1,
    ToolListLoadedResultV1, ToolLoadRequestV1, ToolLoadResultV1, ToolPackageErrorCode,
    ToolPackageFileEntryV1, ToolPackageFileRoleV1, ToolPackageFileSha256V1,
    ToolPackageVersionRefV1, ToolRegistryErrorCode, ToolRegistrySnapshotRef,
    ToolRegistrySnapshotV1, ToolResourceErrorCode, ToolResultContractV1, ToolResultErrorCode,
    ToolResultV1, ToolRuntimeDescriptorV1, ToolRuntimeErrorCode, ToolRuntimeOutputAuthorizationV1,
    ToolSchedulerErrorCode, ToolSchemaErrorCode, ToolSchemaSetSnapshotV1, ToolSchemaSnapshotV1,
    ToolSearchPageV1, ToolSearchRequestV1, ToolSettingsSnapshotV1, ToolStructuredResultSchemaV1,
    ToolVersion, UserAuditToolResultEnvelopeV1, tool_error_catalog_v1,
};
use aura_tool_domain::{
    CanonicalCommandCredentialSlotV1, CanonicalToolPackageDeclarativeCommandV1,
    CanonicalToolPackageInstanceConfigurationSchemaV1, CanonicalToolPackageManifestV1,
    CanonicalToolPackageModelContractV1, CanonicalToolPackageRuntimeV1, CanonicalToolPackageToolV1,
    CanonicalToolPackageUiDescriptionV1, CanonicalToolPackageUiV1, CanonicalToolSemanticVersionV1,
    PackageCapabilityRequirementV1, PackageToolAudienceV1, RawToolPackageManifestV1,
    ToolDraftKindV1, ToolDraftSnapshotTransferV1, ToolExecutionSemanticsV1,
    ToolMutationContractSchemaRootV1, ToolPackageCommandArgumentV1,
    ToolPackageCommandSuccessExitCodesV1, ToolPackageConformanceArgumentsV1,
    ToolPackageConformanceCaseNameV1, ToolPackageConformanceCaseV1,
    ToolPackageDeleteApprovalEvidenceV1, ToolPackageDeleteDraftSnapshotV1,
    ToolPackageManifestSchemaVersionV1, ToolPackageModelDescriptionV1, ToolPackageModelSummaryV1,
    ToolPackageToolDisplayNameV1, ToolPackageToolUiV1, ToolPackageUiDisplayNameV1,
    ToolReadContractSchemaRootV1,
};
use aura_tool_package::{
    PackageFingerprintPreimageV1, PackageRuntimeClosureV1, PackageSecurityFingerprintPreimageV1,
    ToolContractFingerprintPreimageV1, stage_directory_package_v1,
};
use aura_tool_projection::{
    ToolActivityPageV1, ToolCredentialCandidatesProjectionV1, ToolDefinitionCurrentSelectionV1,
};
use aura_workspace_domain::{
    ArtifactExportRequest, BeginWorkspaceLifecycleInput, BeginWorkspaceLifecycleResult,
    BufferEditCommand, BufferEditResult, BufferOpenCommand, BufferSaveCommand,
    CaptureWorkspaceFileRequest, CatalogFreshness, DiscardConfirmation,
    EditorLifecycleTransitionAck, FileTargetRef, FsApplyChangesetRequest, FsApplyChangesetResult,
    FsApplyPatchRequest, FsCopyRequest, FsCreateDirectoryRequest, FsDeleteRequest,
    FsFindPathsRequest, FsGlobRequest, FsGlobResult, FsHashRequest, FsHashResult, FsInspectRequest,
    FsInspectResult, FsListRequest, FsListResult, FsMoveRequest, FsMutationResult,
    FsReadBinaryRangeRequest, FsReadBinaryRangeResult, FsReadTextRequest, FsReadTextResult,
    FsSearchTextRequest, FsSearchTextResult, FsStatRequest, FsStatResult, FsWriteFileRequest,
    IssueDiscardConfirmationInput, LogicalFileSnapshot, PathSearchCommand, PathSearchResult,
    PrepareEditorLifecycleTransition, RendererLifecycleUnavailableInput,
    ResolveWorkspaceLifecycleInput, SessionSummary, WorkspaceArchiveCommand,
    WorkspaceCreateCommand, WorkspaceCreateResult, WorkspaceDeleteCommand, WorkspaceDeleteResult,
    WorkspaceFileTarget, WorkspaceFilesProjection, WorkspaceLifecycleOperationQuery,
    WorkspaceLifecyclePrepareDeliveredInput, WorkspaceRelocateCommand, WorkspaceRestoreCommand,
    WorkspaceSessionProjection, WorkspaceState, WorkspaceSummary,
};
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const SCHEMA_PATH: &str = "resources/schemas/aura-contracts.schema.json";
pub const SCHEMA_MANIFEST_PATH: &str = "resources/schemas/manifest.json";
pub const CONTRACT_FIXTURE_PATH: &str = "fixtures/contracts/canonical-contracts.json";
pub const COMPONENT_MANIFEST_EXAMPLE_PATH: &str = "resources/component-manifest.example.json";
pub const TOOL_ERROR_CATALOG_PATH: &str = "resources/errors/tool-error-catalog.v1.json";
pub const TOOL_ERROR_SCHEMA_PATH: &str = "resources/schemas/tool-error-catalog.v1.schema.json";
pub const TOOL_ERROR_FIXTURE_PATH: &str = "fixtures/contracts/tool-error-catalog.v1.json";
pub const TOOL_ERROR_TYPESCRIPT_PATH: &str = "packages/contracts/src/generated/tool-errors.ts";
pub const TOOL_ERROR_MARKDOWN_PATH: &str = "docs/reference/tool-error-catalog.v1.md";
pub const ID_PREFIX_REGISTRY_PATH: &str = "resources/identity/id-prefix-registry.v1.json";
pub const ID_PREFIX_TYPESCRIPT_PATH: &str = "packages/contracts/src/generated/id-prefixes.ts";
pub const BUILTIN_TOOL_DEFINITION_SET_PATH: &str =
    "resources/tools/builtin-tool-definition-set.v1.json";
pub const TOOL_PACKAGE_MANIFEST_SCHEMA_PATH: &str =
    "resources/schemas/aura-tool-package-manifest.v1.schema.json";
pub const TOOL_PACKAGE_CANONICAL_MANIFEST_SCHEMA_PATH: &str =
    "resources/schemas/aura-tool-package-canonical-manifest.v1.schema.json";
pub const TOOL_PACKAGE_VALID_STDIO_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/valid/aura-stdio.json";
pub const TOOL_PACKAGE_VALID_COMMAND_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/valid/declarative-command.json";
pub const TOOL_PACKAGE_VALID_HTTP_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/valid/declarative-http.json";
pub const TOOL_PACKAGE_VALID_RAW_MULTILINE_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/valid/raw-multiline-normalization.json";
pub const TOOL_PACKAGE_VALID_HTTP_BASE_PATH_ROOT_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/valid/http-base-path-root.json";
pub const TOOL_PACKAGE_VALID_HTTP_BASE_PATH_NO_TRAILING_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/valid/http-base-path-no-trailing-slash.json";
pub const TOOL_PACKAGE_VALID_HTTP_BASE_PATH_TRAILING_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/valid/http-base-path-explicit-trailing-slash.json";
pub const TOOL_PACKAGE_VALID_HTTP_BASE_PATH_PERCENT_SPACE_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/valid/http-base-path-percent-encoded-space.json";
pub const TOOL_PACKAGE_VALID_RAW_HTTP_PROPERTIES_UNSORTED_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/valid/http-json-properties-raw-unsorted.json";
pub const TOOL_PACKAGE_INVALID_VERSION_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/schema-version.json";
pub const TOOL_PACKAGE_INVALID_INTERNAL_ID_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/internal-id-field.json";
pub const TOOL_PACKAGE_INVALID_NESTED_FIELD_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/nested-unknown-field.json";
pub const TOOL_PACKAGE_INVALID_RUNTIME_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/runtime-variant.json";
pub const TOOL_PACKAGE_INVALID_EMPTY_TOOLS_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/empty-tools.json";
pub const TOOL_PACKAGE_INVALID_EMPTY_CONFORMANCE_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/empty-conformance.json";
pub const TOOL_PACKAGE_INVALID_MISSING_INSTANCE_CONFIGURATION_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/missing-instance-configuration.json";
pub const TOOL_PACKAGE_INVALID_CHILD_RUNTIME_AUDIENCE_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/child-runtime-audience.json";
pub const TOOL_PACKAGE_INVALID_RESOURCE_SELECTOR_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/process-workspace-root-selector.json";
pub const TOOL_PACKAGE_INVALID_FILE_HASH_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/file-hash-algorithm.json";
pub const TOOL_PACKAGE_INVALID_CANONICAL_SUFFIX_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/canonical-suffix.json";
pub const TOOL_PACKAGE_INVALID_FIXED_ORIGIN_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/fixed-origin.json";
pub const TOOL_PACKAGE_INVALID_RESERVED_ENVIRONMENT_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/reserved-environment.json";
pub const TOOL_PACKAGE_INVALID_LOCAL_APP_DATA_ENVIRONMENT_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/local-app-data-environment.json";
pub const TOOL_PACKAGE_INVALID_LOCAL_APP_DATA_CREDENTIAL_ENVIRONMENT_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/local-app-data-credential-environment.json";
pub const TOOL_PACKAGE_INVALID_BOOTSTRAP_CREDENTIAL_ENVIRONMENT_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/bootstrap-credential-environment.json";
pub const TOOL_PACKAGE_INVALID_INSTANCE_CONFIGURATION_KEY_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/instance-configuration-key.json";
pub const TOOL_PACKAGE_INVALID_COMMAND_LITERAL_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/command-literal.json";
pub const TOOL_PACKAGE_INVALID_COMMAND_FLAG_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/command-flag.json";
pub const TOOL_PACKAGE_INVALID_HTTP_ORIGIN_LENGTH_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/http-origin-length.json";
pub const TOOL_PACKAGE_INVALID_HTTP_PATH_SEGMENT_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/http-path-segment.json";
pub const TOOL_PACKAGE_INVALID_HTTP_QUERY_NAME_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/http-query-name.json";
pub const TOOL_PACKAGE_INVALID_HTTP_QUERY_VALUE_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/http-query-value.json";
pub const TOOL_PACKAGE_INVALID_HTTP_HEADER_LITERAL_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/http-header-literal.json";
pub const TOOL_PACKAGE_INVALID_HTTP_JSON_PROPERTY_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/http-json-property-name.json";
pub const TOOL_PACKAGE_INVALID_ENVIRONMENT_NAME_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/environment-name.json";
pub const TOOL_PACKAGE_INVALID_ENVIRONMENT_LITERAL_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/environment-literal.json";
pub const TOOL_PACKAGE_INVALID_HTTP_QUERY_NAME_CONTROL_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/http-query-name-control-character.json";
pub const TOOL_PACKAGE_INVALID_HTTP_QUERY_VALUE_CONTROL_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/http-query-value-control-character.json";
pub const TOOL_PACKAGE_INVALID_HTTP_JSON_PROPERTY_CONTROL_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/http-json-property-control-character.json";
pub const TOOL_PACKAGE_INVALID_FILE_BYTE_LENGTH_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/file-byte-length.json";
pub const TOOL_PACKAGE_INVALID_FILE_WINDOWS_NAME_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/package-file-path-windows-invalid-name.json";
pub const TOOL_PACKAGE_INVALID_RUNTIME_ENTRYPOINT_WINDOWS_NAME_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/runtime-entrypoint-windows-reserved-name.json";
pub const TOOL_PACKAGE_INVALID_HTTP_BASE_PATH_RELATIVE_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/http-base-path-relative.json";
pub const TOOL_PACKAGE_INVALID_HTTP_BASE_PATH_RAW_SPACE_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/http-base-path-raw-space.json";
pub const TOOL_PACKAGE_INVALID_HTTP_BASE_PATH_BACKSLASH_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/http-base-path-backslash.json";
pub const TOOL_PACKAGE_INVALID_HTTP_BASE_PATH_EMPTY_MIDDLE_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/http-base-path-empty-middle-segment.json";
pub const TOOL_PACKAGE_INVALID_HTTP_BASE_PATH_DOUBLE_TRAILING_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/http-base-path-double-trailing-slash.json";
pub const TOOL_PACKAGE_INVALID_HTTP_BASE_PATH_INVALID_PERCENT_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/http-base-path-invalid-percent.json";
pub const TOOL_PACKAGE_INVALID_HTTP_BASE_PATH_LOWERCASE_PERCENT_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/http-base-path-lowercase-percent.json";
pub const TOOL_PACKAGE_INVALID_HTTP_BASE_PATH_LITERAL_DOT_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/http-base-path-literal-dot.json";
pub const TOOL_PACKAGE_INVALID_HTTP_BASE_PATH_LITERAL_DOT_DOT_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/http-base-path-literal-dot-dot.json";
pub const TOOL_PACKAGE_INVALID_HTTP_BASE_PATH_ENCODED_DOT_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/http-base-path-encoded-dot.json";
pub const TOOL_PACKAGE_INVALID_HTTP_BASE_PATH_ENCODED_DOT_DOT_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/http-base-path-encoded-dot-dot.json";
pub const TOOL_PACKAGE_INVALID_HTTP_BASE_PATH_ENCODED_SLASH_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/http-base-path-encoded-forward-slash.json";
pub const TOOL_PACKAGE_INVALID_HTTP_BASE_PATH_ENCODED_BACKSLASH_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/http-base-path-encoded-backslash.json";
pub const TOOL_PACKAGE_INVALID_HTTP_PATH_SEGMENT_DOT_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/http-path-segment-dot.json";
pub const TOOL_PACKAGE_INVALID_HTTP_PATH_SEGMENT_DOT_DOT_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/http-path-segment-dot-dot.json";
pub const TOOL_PACKAGE_INVALID_HTTP_PATH_SEGMENT_SLASH_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/http-path-segment-forward-slash.json";
pub const TOOL_PACKAGE_INVALID_HTTP_PATH_SEGMENT_BACKSLASH_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/http-path-segment-backslash.json";
pub const TOOL_PACKAGE_INVALID_HTTP_PATH_SEGMENT_CONTROL_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/http-path-segment-control-character.json";
pub const TOOL_PACKAGE_SEMANTIC_DUPLICATE_CLAIM_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid-semantic/duplicate-claim-content.json";
pub const TOOL_PACKAGE_SEMANTIC_DUPLICATE_FILE_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid-semantic/duplicate-file-path.json";
pub const TOOL_PACKAGE_SEMANTIC_DUPLICATE_TOOL_KEY_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid-semantic/duplicate-tool-key.json";
pub const TOOL_PACKAGE_SEMANTIC_DUPLICATE_CANONICAL_NAME_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid-semantic/duplicate-canonical-name.json";
pub const TOOL_PACKAGE_SEMANTIC_DUPLICATE_CAPABILITY_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid-semantic/duplicate-capability.json";
pub const TOOL_PACKAGE_SEMANTIC_DUPLICATE_CLAIM_KEY_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid-semantic/duplicate-claim-key.json";
pub const TOOL_PACKAGE_SEMANTIC_DUPLICATE_CONFORMANCE_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid-semantic/duplicate-conformance-name.json";
pub const TOOL_PACKAGE_SEMANTIC_INSTANCE_CONFIGURATION_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid-semantic/instance-configuration-reference.json";
pub const TOOL_PACKAGE_SEMANTIC_HTTP_BASE_PATH_DOT_SEGMENT_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid-semantic/http-base-path-dot-segment.json";
pub const TOOL_PACKAGE_SEMANTIC_DUPLICATE_ENVIRONMENT_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid-semantic/duplicate-environment-name.json";
pub const TOOL_PACKAGE_SEMANTIC_CREDENTIAL_ENVIRONMENT_CONFLICT_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid-semantic/credential-environment-conflict.json";
pub const TOOL_PACKAGE_INVALID_CONTROLLED_HTTP_HEADER_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid/controlled-http-header.json";
pub const TOOL_PACKAGE_SEMANTIC_DUPLICATE_HTTP_HEADER_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid-semantic/duplicate-http-header.json";
pub const TOOL_PACKAGE_SEMANTIC_CREDENTIAL_HTTP_HEADER_CONFLICT_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid-semantic/credential-http-header-conflict.json";
pub const TOOL_PACKAGE_SEMANTIC_INSTANCE_CONFIG_REF_PROPERTY_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid-semantic/instance-config-ref-property.json";
pub const TOOL_PACKAGE_SEMANTIC_INSTANCE_CONFIG_DYNAMIC_REF_PROPERTY_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid-semantic/instance-config-dynamic-ref-property.json";
pub const TOOL_PACKAGE_SEMANTIC_INSTANCE_CONFIG_ALL_OF_PROPERTY_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid-semantic/instance-config-all-of-property.json";
pub const TOOL_PACKAGE_SEMANTIC_INSTANCE_CONFIG_ANY_OF_PROPERTY_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid-semantic/instance-config-any-of-property.json";
pub const TOOL_PACKAGE_SEMANTIC_INSTANCE_CONFIG_ONE_OF_PROPERTY_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid-semantic/instance-config-one-of-property.json";
pub const TOOL_PACKAGE_SEMANTIC_INSTANCE_CONFIG_NOT_PROPERTY_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid-semantic/instance-config-not-property.json";
pub const TOOL_PACKAGE_SEMANTIC_INSTANCE_CONFIG_CONDITIONAL_PROPERTY_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid-semantic/instance-config-conditional-property.json";
pub const TOOL_PACKAGE_SEMANTIC_MANIFEST_SELF_REFERENCE_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid-semantic/manifest-self-reference.json";
pub const TOOL_PACKAGE_SEMANTIC_WINDOWS_FILE_COLLISION_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid-semantic/windows-file-path-collision.json";
pub const TOOL_PACKAGE_SEMANTIC_EXPANDED_BYTES_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid-semantic/package-expanded-bytes.json";
pub const TOOL_PACKAGE_SEMANTIC_DUPLICATE_HTTP_PROPERTY_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid-semantic/duplicate-http-json-property.json";
pub const TOOL_PACKAGE_CANONICAL_MISSING_COMMAND_CREDENTIAL_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid-canonical/command-credential-omitted.json";
pub const TOOL_PACKAGE_CANONICAL_MISSING_STDIO_CREDENTIAL_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid-canonical/stdio-credential-omitted.json";
pub const TOOL_PACKAGE_CANONICAL_MISSING_HTTP_CREDENTIAL_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid-canonical/http-credential-omitted.json";
pub const TOOL_PACKAGE_CANONICAL_MISSING_INSTANCE_CONFIGURATION_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid-canonical/instance-configuration-omitted.json";
pub const TOOL_PACKAGE_CANONICAL_MISSING_UI_DESCRIPTION_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid-canonical/ui-description-omitted.json";
pub const TOOL_PACKAGE_CANONICAL_MISSING_SEMANTIC_VERSION_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid-canonical/semantic-version-omitted.json";
pub const TOOL_PACKAGE_CANONICAL_MULTILINE_CARRIAGE_RETURN_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid-canonical/multiline-carriage-return.json";
pub const TOOL_PACKAGE_CANONICAL_HTTP_PROPERTIES_UNSORTED_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid-canonical/http-json-properties-unsorted.json";
pub const TOOL_PACKAGE_CANONICAL_FILES_UNSORTED_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/invalid-canonical/files-unsorted.json";
pub const TOOL_PACKAGE_CANONICAL_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/canonical.json";
pub const TOOL_PACKAGE_CANONICAL_HTTP_PROPERTIES_SORTED_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/canonical/http-json-properties-sorted.json";
pub const TOOL_PACKAGE_CANONICAL_HTTP_PROPERTIES_NESTED_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/canonical/http-json-properties-nested.json";
pub const TOOL_PACKAGE_CANONICAL_HTTP_PROPERTIES_UNICODE_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/canonical/http-json-properties-unicode.json";
pub const TOOL_PACKAGE_CANONICAL_HTTP_ARRAY_ORDER_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/canonical/http-json-array-order-preserved.json";
pub const TOOL_PACKAGE_RUNTIME_CLOSURE_FIXTURE_PATH: &str =
    "fixtures/tool-package/manifest-v1/runtime-closure.v1.json";

mod generated_paths;
pub use generated_paths::RUST_GENERATED_PATHS;

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IdPrefixRegistryV1 {
    pub registry_version: u32,
    pub entries: Vec<IdPrefixRegistryEntryV1>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IdPrefixRegistryEntryV1 {
    pub prefix: String,
    pub owner_crate: String,
    pub type_name: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CommonIdFixture {
    pub tool_id: ToolId,
    pub credential_id: CredentialId,
    pub browser_share_id: BrowserShareId,
    pub attachment_grant_id: AttachmentGrantId,
    pub provider_id: ProviderId,
    pub endpoint_id: EndpointId,
    pub model_profile_id: ModelProfileId,
    pub model_execution_snapshot_id: ModelExecutionSnapshotId,
    pub transport_attempt_id: TransportAttemptId,
    pub model_content_block_id: ModelContentBlockId,
    pub model_failure_id: ModelFailureId,
    pub context_projection_snapshot_id: ContextProjectionSnapshotId,
    pub prompt_bundle_snapshot_id: PromptBundleSnapshotId,
    pub model_discovery_run_id: ModelDiscoveryRunId,
    pub model_availability_test_id: ModelAvailabilityTestId,
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub turn_task_id: TurnTaskId,
    pub task_lineage_id: TaskLineageId,
    pub agent_run_id: AgentRunId,
    pub model_step_id: ModelStepId,
    pub tool_call_id: ToolCallId,
    pub tool_instance_id: ToolInstanceId,
    pub tool_package_id: ToolPackageId,
    pub tool_draft_id: ToolDraftId,
    pub tool_batch_id: ToolBatchId,
    pub tool_execution_attempt_id: ToolExecutionAttemptId,
    pub tool_conformance_run_id: ToolConformanceRunId,
    pub tool_registry_snapshot_id: ToolRegistrySnapshotId,
    pub artifact_id: ArtifactId,
    pub memory_id: MemoryId,
    pub skill_instance_id: SkillInstanceId,
    pub command_id: CommandId,
    pub command_invocation_id: CommandInvocationId,
    pub runtime_instance_id: RuntimeInstanceId,
    pub release_bundle_id: ReleaseBundleId,
    pub stream_id: StreamId,
    pub ipc_channel_id: IpcChannelId,
    pub correlation_id: CorrelationId,
    pub causation_id: CausationId,
    pub event_id: EventId,
    pub file_reference_id: FileReferenceId,
    pub external_grant_id: ExternalGrantId,
    pub deferred_mutation_id: DeferredMutationId,
    pub mutation_journal_id: MutationJournalId,
    pub workspace_lifecycle_operation_id: WorkspaceLifecycleOperationId,
    pub editor_reservation_id: EditorReservationId,
    pub renderer_instance_id: RendererInstanceId,
    pub delete_confirmation_id: DeleteConfirmationId,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextContractFixture {
    pub prompt_settings_snapshot: PromptSettingsSnapshot,
    pub prompt_bundle_snapshot: Option<PromptBundleSnapshot>,
    pub context_projection_snapshot: Option<ContextProjectionSnapshot>,
    pub budget_snapshot: Option<ContextBudgetSnapshot>,
    pub model_egress_manifest: Option<ModelEgressManifestV1>,
    pub preparation_outcome: Option<ContextPreparationOutcome>,
    pub context_search_input: Option<ContextSearchInput>,
    pub context_read_events_input: Option<ContextReadEventsInput>,
    pub context_read_tool_result_input: Option<ContextReadToolResultInput>,
    pub compaction: Option<StructuredCompactionRecord>,
    pub compaction_anchor_manifest: Option<CompactionAnchorManifest>,
    pub compaction_model_attempt: Option<CompactionModelAttemptRecord>,
    pub manual_compaction_request: Option<ManualCompactionRequestRecord>,
    pub prompt_inspector_projection: Option<PromptInspectorProjection>,
    pub history_atomic_group_preimage: Option<HistoryAtomicGroupPreimageV1>,
    pub history_atomic_group_ref: Option<HistoryAtomicGroupRefV1>,
    pub preparation_trigger: Option<ContextPreparationTriggerV1>,
    pub provider_continuation_binding: Option<ProviderContinuationBindingV1>,
    pub tool_result_context_projection: Option<ToolResultContextProjectionV1>,
    pub p07_context_tool_schema_set: ToolSchemaSetSnapshotV1,
    pub p07_context_effective_tool_bindings: Vec<EffectiveToolBindingSnapshot>,
    pub p08_active_task_lineage: Option<PromptInspectorTaskLineageView>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelContractFixture {
    pub tool_versions: Vec<ToolVersion>,
    pub credential_revision: CredentialRevision,
    pub logical_request: Option<LogicalModelRequest>,
    pub provider_request_evidence: Option<ProviderRequestEvidence>,
    pub credential_configured_event: Option<ModelEndpointCredentialConfiguredPayload>,
    pub credential_cleared_event: Option<ModelEndpointCredentialClearedPayload>,
    pub configure_credential_request: Option<ModelEndpointConfigureCredentialRequest>,
    pub clear_credential_request: Option<ModelEndpointClearCredentialRequest>,
    pub credential_mutation_result: Option<EndpointCredentialMutationResult>,
    pub dormant_credential_summary: Option<DormantEndpointCredentialSummary>,
    pub provider_tool_call_id: Option<ProviderToolCallId>,
    pub provider_tool_call_protocol_binding: Option<ProviderToolCallProtocolBindingRecord>,
    pub registry: Option<ModelRegistryConfig>,
    pub execution_snapshot: Option<ModelExecutionSnapshot>,
    pub transport_attempt: Option<TransportAttemptRecord>,
    pub stream_event: Option<ModelStreamEventEnvelope>,
    pub output: Option<NormalizedModelOutput>,
    pub failure: Option<NormalizedModelFailure>,
    pub persisted_failure: Option<PersistedModelFailure>,
    pub runtime_capabilities: Option<ModelRuntimeCapabilityFacts>,
    pub settings_get_snapshot_request: Option<ModelSettingsGetSnapshotRequest>,
    pub catalog_get_request: Option<ModelCatalogGetRequest>,
    pub catalog_ensure_fresh_request: Option<ModelCatalogEnsureFreshRequest>,
    pub catalog_refresh_request: Option<ModelCatalogRefreshRequest>,
    pub catalog_provider_get_request: Option<ModelCatalogProviderGetRequest>,
    pub catalog_provider_get_result: Option<ProviderCatalogModelPage>,
    pub provider_icon_read_request: Option<ProviderIconReadRequest>,
    pub provider_icon_read_result: Option<ProviderIconReadResult>,
    pub provider_create_request: Option<ModelProviderCreateRequest>,
    pub provider_rename_request: Option<ModelProviderRenameRequest>,
    pub provider_delete_request: Option<ModelProviderDeleteRequest>,
    pub provider_mutation_result: Option<ModelProviderMutationResult>,
    pub provider_bulk_availability_result: Option<ProviderBulkAvailabilityResult>,
    pub endpoint_create_request: Option<ModelEndpointCreateRequest>,
    pub endpoint_create_with_credential_request: Option<ModelEndpointCreateWithCredentialRequest>,
    pub endpoint_create_with_credential_result: Option<EndpointCreatedWithCredentialResult>,
    pub endpoint_update_request: Option<ModelEndpointUpdateRequest>,
    pub endpoint_delete_request: Option<ModelEndpointDeleteRequest>,
    pub endpoint_replace_credential_request: Option<ModelEndpointReplaceCredentialRequest>,
    pub discovery_start_request: Option<ModelDiscoveryStartRequest>,
    pub discovery_start_accepted: Option<ModelDiscoveryStartAccepted>,
    pub discovery_cancel_request: Option<ModelDiscoveryCancelRequest>,
    pub discovery_cancel_result: Option<ModelDiscoveryCancelResult>,
    pub endpoint_mutation_result: Option<ModelEndpointMutationResult>,
    pub availability_test_start_request: Option<ModelAvailabilityTestStartRequest>,
    pub availability_test_start_accepted: Option<ModelAvailabilityTestStartAccepted>,
    pub availability_test_cancel_request: Option<ModelAvailabilityTestCancelRequest>,
    pub availability_test_cancel_result: Option<ModelAvailabilityTestCancelResult>,
    pub availability_prerequisite_failure: Option<ModelAvailabilityPrerequisiteFailure>,
    pub profile_create_manual_request: Option<ModelProfileCreateManualRequest>,
    pub profile_create_from_catalog_request: Option<ModelProfileCreateFromCatalogRequest>,
    pub profile_apply_catalog_update_request: Option<ModelProfileApplyCatalogUpdateRequest>,
    pub profile_update_request: Option<ModelProfileUpdateRequest>,
    pub profile_delete_request: Option<ModelProfileDeleteRequest>,
    pub profile_set_subagent_availability_request:
        Option<ModelProfileSetSubagentAvailabilityRequest>,
    pub profile_mutation_result: Option<ModelProfileMutationResult>,
    pub provider_set_all_current_models_subagent_availability_request:
        Option<ModelProviderSetAllCurrentModelsSubagentAvailabilityRequest>,
    pub deletion_mutation_result: Option<ModelDeletionMutationResult>,
    pub default_set_request: Option<ModelDefaultSetRequest>,
    pub default_mutation_result: Option<DefaultModelMutationResult>,
    pub session_set_binding_request: Option<SessionModelBindingSetRequest>,
    pub session_binding_mutation_result: Option<SessionModelBindingMutationResult>,
    pub context_window_correct_request: Option<ContextWindowCorrectRequest>,
    pub context_window_clear_user_override_request: Option<ContextWindowClearUserOverrideRequest>,
    pub context_window_reset_runtime_correction_request:
        Option<ContextWindowResetRuntimeCorrectionRequest>,
    pub context_window_mutation_result: Option<ContextWindowMutationResult>,
    pub compatibility_get_request: Option<ModelCompatibilityGetRequest>,
    pub compatibility_snapshot: Option<EndpointCompatibilitySnapshot>,
    pub compatibility_reset_request: Option<ModelCompatibilityResetRequest>,
    pub compatibility_reset_result: Option<ModelCompatibilityResetResult>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeContractFixture {
    pub frame_header: IpcFrameHeader,
    pub handshake_request: RuntimeHandshakeRequest,
    pub handshake_response: RuntimeHandshakeResponse,
    pub handshake_transcript: RuntimeHandshakeTranscript,
    pub handshake_finish: RuntimeHandshakeFinish,
    pub handshake_complete: RuntimeHandshakeComplete,
    pub authenticated_peer_identity: AuthenticatedPeerIdentity,
    pub command_outcome: CommandOutcome,
    pub startup_progress: CoreStartupProgress,
    pub heartbeat: RuntimeHeartbeat,
    pub projection_subscription: ProjectionSubscription,
    pub projection_unsubscribe_request: ProjectionUnsubscribeRequest,
    pub projection_snapshot_request: ProjectionSnapshotRequest,
    pub projection_change: ProjectionChange,
    pub projection_keys: Vec<ProjectionKey>,
    pub binary_stream_frame: BinaryStreamFrame,
    pub ipc_error: IpcErrorEnvelope,
    pub cancellation: CancellationRequest,
    pub active_install: ActiveInstallState,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PersistenceContractFixture {
    pub credential_ref: CredentialRef,
    pub artifact_record: ArtifactRecord,
    pub workspace_session_projection_change: ProjectionChange,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceFilesContractFixture {
    pub internal_file_target: FileTargetRef,
    pub model_visible_workspace_target: WorkspaceFileTarget,
    pub workspace_files_projection_change: ProjectionChange,
    pub logical_file_snapshot: Option<LogicalFileSnapshot>,
    pub workspace_create: Option<WorkspaceCreateCommand>,
    pub workspace_create_result: Option<WorkspaceCreateResult>,
    pub workspace_restore: Option<WorkspaceRestoreCommand>,
    pub workspace_relocate: Option<WorkspaceRelocateCommand>,
    pub workspace_archive: Option<WorkspaceArchiveCommand>,
    pub workspace_delete: Option<WorkspaceDeleteCommand>,
    pub workspace_delete_result: Option<WorkspaceDeleteResult>,
    pub path_search: Option<PathSearchCommand>,
    pub path_search_result: Option<PathSearchResult>,
    pub buffer_open: Option<BufferOpenCommand>,
    pub buffer_edit: Option<BufferEditCommand>,
    pub buffer_edit_result: Option<BufferEditResult>,
    pub buffer_save: Option<BufferSaveCommand>,
    pub lifecycle_begin: Option<BeginWorkspaceLifecycleInput>,
    pub lifecycle_begin_result: Option<BeginWorkspaceLifecycleResult>,
    pub lifecycle_prepare: Option<PrepareEditorLifecycleTransition>,
    pub lifecycle_prepare_delivered: Option<WorkspaceLifecyclePrepareDeliveredInput>,
    pub lifecycle_query: Option<WorkspaceLifecycleOperationQuery>,
    pub lifecycle_ack: Option<EditorLifecycleTransitionAck>,
    pub lifecycle_issue_discard_confirmation: Option<IssueDiscardConfirmationInput>,
    pub lifecycle_discard_confirmation: Option<DiscardConfirmation>,
    pub lifecycle_resolve: Option<ResolveWorkspaceLifecycleInput>,
    pub lifecycle_renderer_unavailable: Option<RendererLifecycleUnavailableInput>,
    pub fs_stat: Option<FsStatRequest>,
    pub fs_stat_result: Option<FsStatResult>,
    pub fs_list: Option<FsListRequest>,
    pub fs_list_result: Option<FsListResult>,
    pub fs_find_paths: Option<FsFindPathsRequest>,
    pub fs_glob: Option<FsGlobRequest>,
    pub fs_glob_result: Option<FsGlobResult>,
    pub fs_read_text: Option<FsReadTextRequest>,
    pub fs_read_text_result: Option<FsReadTextResult>,
    pub fs_search_text: Option<FsSearchTextRequest>,
    pub fs_search_text_result: Option<FsSearchTextResult>,
    pub fs_hash: Option<FsHashRequest>,
    pub fs_hash_result: Option<FsHashResult>,
    pub fs_inspect: Option<FsInspectRequest>,
    pub fs_inspect_result: Option<FsInspectResult>,
    pub fs_read_binary_range: Option<FsReadBinaryRangeRequest>,
    pub fs_read_binary_range_result: Option<FsReadBinaryRangeResult>,
    pub fs_write_file: Option<FsWriteFileRequest>,
    pub fs_apply_patch: Option<FsApplyPatchRequest>,
    pub fs_create_directory: Option<FsCreateDirectoryRequest>,
    pub fs_copy: Option<FsCopyRequest>,
    pub fs_move: Option<FsMoveRequest>,
    pub fs_delete: Option<FsDeleteRequest>,
    pub fs_apply_changeset: Option<FsApplyChangesetRequest>,
    pub fs_apply_changeset_result: Option<FsApplyChangesetResult>,
    pub fs_mutation_result: Option<FsMutationResult>,
    pub artifact_capture_workspace_file: Option<CaptureWorkspaceFileRequest>,
    pub artifact_capture_workspace_file_result: Option<CaptureWorkspaceFileResult>,
    pub artifact_export: Option<ArtifactExportRequest>,
    pub artifact_export_result: Option<ArtifactExportResult>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityContractFixture {
    pub model_channel_data_disposition: ModelChannelDataDisposition,
    pub model_egress_policy_version: ModelEgressPolicyVersion,
    pub operation_security_plan: Option<OperationSecurityPlan>,
    pub permission_snapshot: Option<PermissionSnapshot>,
    pub permission_decision: Option<PermissionDecision>,
    pub permission_grant: Option<PermissionGrant>,
    pub get_session_state: Option<GetSessionPermissionStateInput>,
    pub set_session_mode: Option<SetSessionPermissionModeInput>,
    pub permission_defaults: Option<PermissionDefaults>,
    pub set_permission_defaults: Option<SetPermissionDefaultsInput>,
    pub list_grants: Option<ListPermissionGrantsInput>,
    pub revoke_grant: Option<RevokePermissionGrantInput>,
    pub approval_request: Option<ApprovalRequest>,
    pub approval_resolve: Option<ResolveApprovalInput>,
    pub approval_resolution: Option<ApprovalResolution>,
    pub approval_details: Option<GetApprovalDetailsInput>,
    pub trust_fingerprint: Option<TrustFingerprintInput>,
    pub trust_record: Option<ExternalTrustRecord>,
    pub external_extension_risk: Option<ExternalExtensionRiskView>,
    pub sandbox_execution_plan: Option<SandboxExecutionPlan>,
    pub w7_tool_operation: Option<W7ToolOperationFingerprintPreimageV1>,
    pub tool_sandbox_dispatch_binding: Option<ToolSandboxDispatchBindingV1>,
    pub tool_conformance_source: Option<ToolConformanceSecuritySourceRefV1>,
    pub tool_conformance_runtime_scope: Option<ToolConformanceRuntimeScopeV1>,
    pub conformance_sandbox_profile: Option<ConformanceSandboxProfileV1>,
    pub credential_reflection_scan: Option<CredentialReflectionScanEvidenceV1>,
    pub authorized_tool_sandbox_execution: Option<AuthorizedToolSandboxExecutionV1>,
    pub external_process_launch_plan: Option<ExternalProcessLaunchPlan>,
    pub sandbox_host_event: Option<SandboxHostEvent>,
    pub sandbox_network_policy_error: Option<SandboxNetworkPolicyError>,
    pub host_preparation_status: Option<HostPreparationStatus>,
    pub host_preparation_begin: Option<HostPreparationBeginResult>,
    pub host_preparation_mutation: Option<HostPreparationMutationInput>,
    pub host_preparation_resolve: Option<ResolveHostPreparationInput>,
    pub external_grant_record: Option<ExternalGrantRecord>,
    pub external_grant_view: Option<ExternalGrantView>,
    pub file_session_consent_view: Option<aura_permission_domain::FileSessionConsentView>,
    pub file_session_consent_list: Option<aura_permission_domain::ListFileSessionConsentsInput>,
    pub file_session_consent_revoke: Option<aura_permission_domain::RevokeFileSessionConsentInput>,
    pub native_session_consent_view: Option<NativeSessionConsentView>,
    pub native_session_consent_scope: Option<NativeSessionConsentScopeView>,
    pub native_session_consent_list: Option<ListNativeSessionConsentsInput>,
    pub native_session_consent_revoke: Option<RevokeNativeSessionConsentInput>,
    pub external_grant_attach_read_only: Option<AttachExternalReadOnlyInput>,
    pub external_grant_derive_workspace: Option<DeriveWorkspaceExternalGrantInput>,
    pub external_grant_list: Option<ListSessionExternalGrantsInput>,
    pub external_grant_revalidate: Option<RevalidateExternalGrantInput>,
    pub external_grant_revoke: Option<RevokeExternalGrantInput>,
    pub network_request_plan: Option<NetworkRequestPlan>,
    pub authorized_network_request: Option<AuthorizedNetworkRequest>,
    pub network_grant: Option<NetworkGrant>,
    pub network_audit: Option<NetworkRequestAuditRecord>,
    pub quarantine_record: Option<QuarantineRecordDetails>,
    pub quarantine_prepare_release: Option<PrepareQuarantineReleaseInput>,
    pub quarantine_prepare_release_result: Option<PrepareQuarantineReleaseResult>,
    pub quarantine_release: Option<ReleaseQuarantineInput>,
    pub quarantine_issue_discard_confirmation: Option<IssueQuarantineDiscardConfirmationInput>,
    pub quarantine_discard_confirmation: Option<QuarantineDiscardConfirmation>,
    pub quarantine_discard: Option<DiscardQuarantineInput>,
    pub projection_change: Option<SecurityProjectionChange>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalContractFixture {
    pub contract_version: ContractVersion,
    pub ids: CommonIdFixture,
    pub revision: Revision,
    pub expected_revision: ExpectedRevision,
    pub content_hash: ContentHash,
    pub timestamp: UtcTimestamp,
    pub error: ErrorEnvelope,
    pub component_manifest: BuildComponentManifest,
    pub projection_contract: ProjectionContractSchemaRoot,
    pub runtime: RuntimeContractFixture,
    pub persistence: PersistenceContractFixture,
    pub workspace_files: WorkspaceFilesContractFixture,
    pub security: SecurityContractFixture,
    pub models: ModelContractFixture,
    pub context: ContextContractFixture,
    pub tools: ToolContractFixture,
    pub windows_path: String,
}

#[derive(Debug, Serialize)]
struct SchemaManifest {
    manifest_version: ContractVersion,
    schemas: Vec<SchemaManifestEntry>,
}

#[derive(Debug, Serialize)]
struct SchemaManifestEntry {
    id: &'static str,
    path: &'static str,
    content_hash: ContentHash,
}

pub fn tool_package_manifest_schema_value() -> Result<Value> {
    let mut value = serde_json::to_value(schema_for!(RawToolPackageManifestV1))?;
    let object = value.as_object_mut().context("tool package manifest schema must be an object")?;
    object.insert(
        "$id".to_owned(),
        Value::String(
            "https://schemas.aura.local/contracts/v1/aura-tool-package-manifest.v1.schema.json"
                .to_owned(),
        ),
    );
    object.insert("title".to_owned(), Value::String("AuraToolPackageManifestV1".to_owned()));
    Ok(value)
}

pub fn canonical_tool_package_manifest_schema_value() -> Result<Value> {
    let mut value = serde_json::to_value(schema_for!(CanonicalToolPackageManifestV1))?;
    let object = value
        .as_object_mut()
        .context("canonical tool package manifest schema must be an object")?;
    object.insert(
        "$id".to_owned(),
        Value::String(
            "https://schemas.aura.local/contracts/v1/aura-tool-package-canonical-manifest.v1.schema.json"
                .to_owned(),
        ),
    );
    object
        .insert("title".to_owned(), Value::String("AuraToolPackageCanonicalManifestV1".to_owned()));
    Ok(value)
}

fn raw_package_file(path: &str, bytes: &[u8], role: &str) -> Value {
    json!({
        "path": path,
        "byte_length": bytes.len(),
        "sha256": {
            "algorithm": "sha256",
            "hex": ContentHash::sha256(bytes).hex,
        },
        "role": role,
    })
}

fn raw_model_contract(input_schema: Value, result_contract: Value) -> Value {
    json!({
        "canonical_suffix": "fixture",
        "description": "Execute the package fixture with validated arguments.",
        "summary": "Execute a deterministic package fixture.",
        "input_schema": input_schema,
        "strict": true,
        "result_contract": result_contract,
    })
}

fn raw_tool_contract(runtime: Value, input_schema: Value, result_contract: Value) -> Value {
    json!({
        "tool_key": "fixture",
        "ui": { "display_name": "Fixture Tool" },
        "model": raw_model_contract(input_schema, result_contract),
        "instance_configuration": null,
        "runtime": runtime,
        "capability_requirements": ["process_execute"],
        "resource_claims": [],
        "execution_semantics": {
            "automatic_retry_allowed": false,
            "repeat_is_idempotent": true,
            "may_create_persistent_effects": false,
        },
        "conformance_cases": [{
            "name": "deterministic fixture",
            "arguments": {},
        }],
    })
}

fn raw_manifest(package_key: &str, file: Value, tool: Value) -> Value {
    json!({
        "schema_version": 1,
        "package_key": package_key,
        "package_version": "1.0.0",
        "minimum_aura_version": "0.1.0",
        "ui": {
            "display_name": "Aura Fixture Package",
            "description": null
        },
        "files": [file],
        "tools": [tool],
    })
}

pub fn valid_tool_package_manifest_fixtures() -> Result<Vec<(&'static str, Value)>> {
    let stdio = raw_manifest(
        "org.aura.fixture.stdio",
        raw_package_file("bin/fixture.exe", b"stdio fixture runtime", "runtime_executable"),
        raw_tool_contract(
            json!({
                "type": "aura_stdio",
                "protocol_version": 1,
                "executable": "bin/fixture.exe",
                "arguments": [],
                "working_directory": { "type": "package_root" },
                "environment": [],
                "credential_slot": null
            }),
            json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "additionalProperties": false,
            }),
            json!({ "type": "unit" }),
        ),
    );

    let mut command_tool = raw_tool_contract(
        json!({
            "type": "declarative_command",
            "command": {
                "type": "powershell_script",
                "script": "scripts/fixture.ps1",
                "arguments": [
                    { "type": "literal", "value": "--label" },
                    { "type": "scalar", "source": "/label" },
                    { "type": "repeated", "source": "/values" },
                    {
                        "type": "conditional_flag",
                        "source": "/verbose",
                        "flag": "--verbose"
                    },
                    {
                        "type": "named_option",
                        "name": "--optional",
                        "source": "/optional",
                        "omit_when_missing_or_null": true
                    }
                ],
                "working_directory": { "type": "workspace_root" },
                "environment": [{
                    "name": "AURA_LABEL",
                    "value": { "type": "scalar", "source": "/label" }
                }],
                "credential_slot": null
            }
        }),
        json!({
            "type": "object",
            "properties": {
                "label": { "type": "string" },
                "values": { "type": "array", "items": { "type": "string" } },
                "verbose": { "type": "boolean" },
                "optional": { "type": ["string", "null"] }
            },
            "required": ["label", "values", "verbose"],
            "additionalProperties": false
        }),
        json!({ "type": "utf8_text" }),
    );
    command_tool["semantic_version"] = json!("1.2.3");
    command_tool["audience"] = json!("parent_agent_only");
    command_tool["instance_configuration"] = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": {
            "output_mode": {
                "type": "string",
                "minLength": 1,
                "maxLength": 16,
                "pattern": "^[a-z_]+$",
                "enum": ["compact", "expanded"]
            }
        },
        "required": ["output_mode"],
        "additionalProperties": false
    });
    command_tool["runtime"]["command"]["environment"] = json!([
        {
            "name": "AURA_LABEL",
            "value": { "type": "scalar", "source": "/label" }
        },
        {
            "name": "AURA_OUTPUT_MODE",
            "value": { "type": "instance_configuration", "key": "output_mode" }
        }
    ]);
    command_tool["conformance_cases"][0]["arguments"] = json!({
        "label": "fixture",
        "values": ["one", "two"],
        "verbose": false
    });
    let command = raw_manifest(
        "org.aura.fixture.command",
        raw_package_file("scripts/fixture.ps1", b"command fixture runtime", "runtime_script"),
        command_tool,
    );

    let mut http_tool = raw_tool_contract(
        json!({
            "type": "declarative_http",
            "request": {
                "target": {
                    "type": "fixed_https",
                    "origin": "https://example.invalid",
                    "base_path": "/v1"
                },
                "method": "POST",
                "path": [
                    { "type": "literal", "value": "echo" },
                    { "type": "argument", "source": "/resource" }
                ],
                "query": [{
                    "name": "tag",
                    "value": { "type": "repeated", "source": "/tags" }
                }],
                "headers": [{
                    "name": "x-aura-fixture",
                    "value": { "type": "literal", "value": "manifest-v1" }
                }],
                "body": {
                    "type": "application_json",
                    "template": {
                        "type": "object",
                        "properties": [{
                            "name": "payload",
                            "value": { "type": "argument", "source": "/payload" }
                        }]
                    }
                },
                "credential_slot": {
                    "type": "bearer_authorization",
                    "slot_key": "api_key"
                },
                "success_status_ranges": [
                    { "start": 200, "end": 204 },
                    { "start": 206, "end": 206 }
                ]
            }
        }),
        json!({
            "type": "object",
            "properties": {
                "resource": { "type": "string" },
                "tags": { "type": "array", "items": { "type": "string" } },
                "payload": {}
            },
            "required": ["resource", "tags", "payload"],
            "additionalProperties": false
        }),
        json!({
            "type": "structured_json",
            "result_schema": {
                "type": ["object", "null"]
            }
        }),
    );
    http_tool["capability_requirements"] = json!(["data_egress", "public_network_read"]);
    http_tool["resource_claims"] = json!([{
        "type": "network_target",
        "claim_key": "provider_target",
        "selector": { "type": "runtime_http_target" },
        "access": "write"
    }]);
    http_tool["conformance_cases"][0]["arguments"] = json!({
        "resource": "fixture",
        "tags": [],
        "payload": null
    });
    let http = raw_manifest(
        "org.aura.fixture.http",
        raw_package_file("runtime/http.json", b"http fixture runtime", "runtime_resource"),
        http_tool,
    );

    let mut raw_multiline = stdio.clone();
    raw_multiline["package_key"] = json!("org.aura.fixture.multiline");
    raw_multiline["ui"]["description"] = json!("Package line one\r\nline two\rline three");
    raw_multiline["tools"][0]["model"]["description"] =
        json!("Model line one\r\nline two\rline three");

    let http_with_base_path = |package_key: &str, base_path: &str| {
        let mut fixture = http.clone();
        fixture["package_key"] = json!(package_key);
        fixture["tools"][0]["runtime"]["request"]["target"]["base_path"] = json!(base_path);
        fixture
    };
    let http_base_path_root = http_with_base_path("org.aura.fixture.http.root", "/");
    let http_base_path_no_trailing =
        http_with_base_path("org.aura.fixture.http.no-trailing", "/v1/items");
    let http_base_path_trailing =
        http_with_base_path("org.aura.fixture.http.trailing", "/v1/items/");
    let http_base_path_percent_space =
        http_with_base_path("org.aura.fixture.http.percent-space", "/v1/a%20b");

    let mut raw_http_properties_unsorted = http.clone();
    raw_http_properties_unsorted["package_key"] = json!("org.aura.fixture.http.raw-properties");
    raw_http_properties_unsorted["tools"][0]["runtime"]["request"]["body"]["template"] = json!({
        "type": "object",
        "properties": [
            { "name": "z", "value": { "type": "literal", "value": 1 } },
            {
                "name": "a",
                "value": {
                    "type": "object",
                    "properties": [
                        { "name": "nested_z", "value": { "type": "literal", "value": 3 } },
                        { "name": "nested_a", "value": { "type": "literal", "value": 4 } }
                    ]
                }
            },
            {
                "name": "array",
                "value": {
                    "type": "array",
                    "items": [
                        { "type": "literal", "value": "second" },
                        { "type": "literal", "value": "first" }
                    ]
                }
            }
        ]
    });

    let fixtures = vec![
        (TOOL_PACKAGE_VALID_STDIO_FIXTURE_PATH, stdio),
        (TOOL_PACKAGE_VALID_COMMAND_FIXTURE_PATH, command),
        (TOOL_PACKAGE_VALID_HTTP_FIXTURE_PATH, http),
        (TOOL_PACKAGE_VALID_RAW_MULTILINE_FIXTURE_PATH, raw_multiline),
        (TOOL_PACKAGE_VALID_HTTP_BASE_PATH_ROOT_FIXTURE_PATH, http_base_path_root),
        (TOOL_PACKAGE_VALID_HTTP_BASE_PATH_NO_TRAILING_FIXTURE_PATH, http_base_path_no_trailing),
        (TOOL_PACKAGE_VALID_HTTP_BASE_PATH_TRAILING_FIXTURE_PATH, http_base_path_trailing),
        (
            TOOL_PACKAGE_VALID_HTTP_BASE_PATH_PERCENT_SPACE_FIXTURE_PATH,
            http_base_path_percent_space,
        ),
        (
            TOOL_PACKAGE_VALID_RAW_HTTP_PROPERTIES_UNSORTED_FIXTURE_PATH,
            raw_http_properties_unsorted,
        ),
    ];
    for (path, value) in &fixtures {
        serde_json::from_value::<RawToolPackageManifestV1>(value.clone())
            .with_context(|| format!("valid tool package manifest fixture {path}"))?;
    }
    Ok(fixtures)
}

pub fn invalid_tool_package_manifest_fixtures() -> Result<Vec<(&'static str, Value)>> {
    let fixtures = valid_tool_package_manifest_fixtures()?;
    let base = fixtures
        .iter()
        .find(|(path, _)| *path == TOOL_PACKAGE_VALID_STDIO_FIXTURE_PATH)
        .map(|(_, value)| value.clone())
        .context("stdio manifest fixture must exist")?;
    let command_base = fixtures
        .iter()
        .find(|(path, _)| *path == TOOL_PACKAGE_VALID_COMMAND_FIXTURE_PATH)
        .map(|(_, value)| value.clone())
        .context("command manifest fixture must exist")?;
    let http_base = fixtures
        .iter()
        .find(|(path, _)| *path == TOOL_PACKAGE_VALID_HTTP_FIXTURE_PATH)
        .map(|(_, value)| value.clone())
        .context("HTTP manifest fixture must exist")?;

    let mut version = base.clone();
    version["schema_version"] = json!(2);
    let mut internal_id = base.clone();
    internal_id["tool_package_id"] = json!("tpk_01ARZ3NDEKTSV4RRFFQ69G5FAV");
    let mut nested = base.clone();
    nested["tools"][0]["ui"]["model_description"] = json!("wrong contract layer");
    let mut runtime = base.clone();
    runtime["tools"][0]["runtime"] = json!({
        "type": "external_source",
        "source_contract_key": "not-allowed-in-package"
    });
    let mut empty_tools = base.clone();
    empty_tools["tools"] = json!([]);
    let mut empty_conformance = base.clone();
    empty_conformance["tools"][0]["conformance_cases"] = json!([]);
    let mut missing_instance_configuration = base.clone();
    missing_instance_configuration["tools"][0]
        .as_object_mut()
        .context("tool must be an object")?
        .remove("instance_configuration");
    let mut child_runtime_audience = base.clone();
    child_runtime_audience["tools"][0]["audience"] = json!("child_runtime_required");
    let mut invalid_resource_selector = base.clone();
    invalid_resource_selector["tools"][0]["resource_claims"] = json!([{
        "type": "process",
        "claim_key": "process_target",
        "selector": { "type": "workspace_root" },
        "access": "execute"
    }]);
    let mut invalid_file_hash = base.clone();
    invalid_file_hash["files"][0]["sha256"]["algorithm"] = json!("blake3");
    let mut invalid_canonical_suffix = base.clone();
    invalid_canonical_suffix["tools"][0]["model"]["canonical_suffix"] = json!("Aura.Invalid");
    let mut invalid_fixed_origin = http_base.clone();
    invalid_fixed_origin["tools"][0]["runtime"]["request"]["target"]["origin"] =
        json!("http://example.invalid");
    let mut invalid_reserved_environment = command_base.clone();
    invalid_reserved_environment["tools"][0]["runtime"]["command"]["environment"][0]["name"] =
        json!("AURA_TOOL_CALL_ID");
    // `LOCALAPPDATA` is bound by Core to the attempt's own isolated directory, so a
    // package may not choose it — neither as an ordinary environment entry nor as a
    // credential destination. Both fixtures must be rejected, never sanitized.
    let mut invalid_local_app_data_environment = command_base.clone();
    invalid_local_app_data_environment["tools"][0]["runtime"]["command"]["environment"][0]["name"] =
        json!("LOCALAPPDATA");
    let mut invalid_local_app_data_credential_environment = command_base.clone();
    invalid_local_app_data_credential_environment["tools"][0]["runtime"]["command"]["credential_slot"] = json!({
        "slot_key": "fixture",
        "environment_variable": "LOCALAPPDATA"
    });
    let mut invalid_bootstrap_credential_environment = command_base.clone();
    invalid_bootstrap_credential_environment["tools"][0]["runtime"]["command"]["credential_slot"] = json!({
        "slot_key": "fixture",
        "environment_variable": "AURA_TOOL_EXECUTION_ATTEMPT_ID"
    });

    let mut invalid_instance_configuration_key = command_base.clone();
    invalid_instance_configuration_key["tools"][0]["runtime"]["command"]["environment"][1]["value"]
        ["key"] = json!("Invalid.Key");

    let mut invalid_command_literal = command_base.clone();
    invalid_command_literal["tools"][0]["runtime"]["command"]["arguments"][0]["value"] =
        json!("contains\0nul");

    let mut invalid_command_flag = command_base.clone();
    invalid_command_flag["tools"][0]["runtime"]["command"]["arguments"][3]["flag"] =
        json!("--verbose\n");

    let mut invalid_http_origin_length = http_base.clone();
    invalid_http_origin_length["tools"][0]["runtime"]["request"]["target"]["origin"] =
        json!(format!("https://{}.invalid", "a".repeat(2_040)));

    let mut invalid_http_path_segment = http_base.clone();
    invalid_http_path_segment["tools"][0]["runtime"]["request"]["path"][0]["value"] =
        json!("not/a/segment");

    let mut invalid_http_query_name = http_base.clone();
    invalid_http_query_name["tools"][0]["runtime"]["request"]["query"][0]["name"] = json!("");

    let mut invalid_http_query_value = http_base.clone();
    invalid_http_query_value["tools"][0]["runtime"]["request"]["query"][0]["value"] =
        json!({ "type": "literal", "value": "q".repeat(8_193) });

    let mut invalid_http_header_literal = http_base.clone();
    invalid_http_header_literal["tools"][0]["runtime"]["request"]["headers"][0]["value"] =
        json!({ "type": "literal", "value": "contains\ttab" });

    let mut invalid_http_json_property = http_base.clone();
    invalid_http_json_property["tools"][0]["runtime"]["request"]["body"]["template"]["properties"]
        [0]["name"] = json!("");

    let mut invalid_environment_name = command_base.clone();
    invalid_environment_name["tools"][0]["runtime"]["command"]["environment"][0]["name"] =
        json!("1INVALID");

    let mut invalid_environment_literal = command_base;
    invalid_environment_literal["tools"][0]["runtime"]["command"]["environment"][0]["value"] =
        json!({ "type": "literal", "value": "contains\nnewline" });

    let mut invalid_controlled_http_header = http_base.clone();
    invalid_controlled_http_header["tools"][0]["runtime"]["request"]["headers"][0]["name"] =
        json!("set-cookie");

    let controls = ['\0', '\t', '\r', '\n', '\u{007f}'];
    let mut invalid_http_query_name_control = http_base.clone();
    invalid_http_query_name_control["tools"][0]["runtime"]["request"]["query"] = Value::Array(
        controls
            .iter()
            .map(|control| {
                json!({
                    "name": format!("q{control}name"),
                    "value": { "type": "literal", "value": "fixture" }
                })
            })
            .collect(),
    );
    let mut invalid_http_query_value_control = http_base.clone();
    invalid_http_query_value_control["tools"][0]["runtime"]["request"]["query"] = Value::Array(
        controls
            .iter()
            .map(|control| {
                json!({
                    "name": "query",
                    "value": { "type": "literal", "value": format!("v{control}alue") }
                })
            })
            .collect(),
    );
    let mut invalid_http_json_property_control = http_base.clone();
    invalid_http_json_property_control["tools"][0]["runtime"]["request"]["body"] = json!({
        "type": "application_json",
        "template": {
            "type": "object",
            "properties": controls
                .iter()
                .map(|control| json!({
                    "name": format!("p{control}name"),
                    "value": { "type": "literal", "value": null }
                }))
                .collect::<Vec<_>>()
        }
    });

    let mut invalid_file_byte_length = base.clone();
    invalid_file_byte_length["files"][0]["byte_length"] = json!(67_108_865_u64);

    let mut invalid_file_windows_name = base.clone();
    invalid_file_windows_name["files"][0]["path"] = json!("runtime/bad?.dll");

    let mut invalid_runtime_entrypoint_windows_name = base.clone();
    invalid_runtime_entrypoint_windows_name["tools"][0]["runtime"]["executable"] =
        json!("bin/COM¹.exe");

    let invalid_base_path = |path: &str| {
        let mut fixture = http_base.clone();
        fixture["tools"][0]["runtime"]["request"]["target"]["base_path"] = json!(path);
        fixture
    };
    let invalid_path_segment = |value: &str| {
        let mut fixture = http_base.clone();
        fixture["tools"][0]["runtime"]["request"]["path"][0]["value"] = json!(value);
        fixture
    };

    Ok(vec![
        (TOOL_PACKAGE_INVALID_VERSION_FIXTURE_PATH, version),
        (TOOL_PACKAGE_INVALID_INTERNAL_ID_FIXTURE_PATH, internal_id),
        (TOOL_PACKAGE_INVALID_NESTED_FIELD_FIXTURE_PATH, nested),
        (TOOL_PACKAGE_INVALID_RUNTIME_FIXTURE_PATH, runtime),
        (TOOL_PACKAGE_INVALID_EMPTY_TOOLS_FIXTURE_PATH, empty_tools),
        (TOOL_PACKAGE_INVALID_EMPTY_CONFORMANCE_FIXTURE_PATH, empty_conformance),
        (
            TOOL_PACKAGE_INVALID_MISSING_INSTANCE_CONFIGURATION_FIXTURE_PATH,
            missing_instance_configuration,
        ),
        (TOOL_PACKAGE_INVALID_CHILD_RUNTIME_AUDIENCE_FIXTURE_PATH, child_runtime_audience),
        (TOOL_PACKAGE_INVALID_RESOURCE_SELECTOR_FIXTURE_PATH, invalid_resource_selector),
        (TOOL_PACKAGE_INVALID_FILE_HASH_FIXTURE_PATH, invalid_file_hash),
        (TOOL_PACKAGE_INVALID_CANONICAL_SUFFIX_FIXTURE_PATH, invalid_canonical_suffix),
        (TOOL_PACKAGE_INVALID_FIXED_ORIGIN_FIXTURE_PATH, invalid_fixed_origin),
        (TOOL_PACKAGE_INVALID_RESERVED_ENVIRONMENT_FIXTURE_PATH, invalid_reserved_environment),
        (
            TOOL_PACKAGE_INVALID_LOCAL_APP_DATA_ENVIRONMENT_FIXTURE_PATH,
            invalid_local_app_data_environment,
        ),
        (
            TOOL_PACKAGE_INVALID_LOCAL_APP_DATA_CREDENTIAL_ENVIRONMENT_FIXTURE_PATH,
            invalid_local_app_data_credential_environment,
        ),
        (
            TOOL_PACKAGE_INVALID_BOOTSTRAP_CREDENTIAL_ENVIRONMENT_FIXTURE_PATH,
            invalid_bootstrap_credential_environment,
        ),
        (
            TOOL_PACKAGE_INVALID_INSTANCE_CONFIGURATION_KEY_FIXTURE_PATH,
            invalid_instance_configuration_key,
        ),
        (TOOL_PACKAGE_INVALID_COMMAND_LITERAL_FIXTURE_PATH, invalid_command_literal),
        (TOOL_PACKAGE_INVALID_COMMAND_FLAG_FIXTURE_PATH, invalid_command_flag),
        (TOOL_PACKAGE_INVALID_HTTP_ORIGIN_LENGTH_FIXTURE_PATH, invalid_http_origin_length),
        (TOOL_PACKAGE_INVALID_HTTP_PATH_SEGMENT_FIXTURE_PATH, invalid_http_path_segment),
        (TOOL_PACKAGE_INVALID_HTTP_QUERY_NAME_FIXTURE_PATH, invalid_http_query_name),
        (TOOL_PACKAGE_INVALID_HTTP_QUERY_VALUE_FIXTURE_PATH, invalid_http_query_value),
        (TOOL_PACKAGE_INVALID_HTTP_HEADER_LITERAL_FIXTURE_PATH, invalid_http_header_literal),
        (TOOL_PACKAGE_INVALID_HTTP_JSON_PROPERTY_FIXTURE_PATH, invalid_http_json_property),
        (TOOL_PACKAGE_INVALID_ENVIRONMENT_NAME_FIXTURE_PATH, invalid_environment_name),
        (TOOL_PACKAGE_INVALID_ENVIRONMENT_LITERAL_FIXTURE_PATH, invalid_environment_literal),
        (TOOL_PACKAGE_INVALID_CONTROLLED_HTTP_HEADER_FIXTURE_PATH, invalid_controlled_http_header),
        (
            TOOL_PACKAGE_INVALID_HTTP_QUERY_NAME_CONTROL_FIXTURE_PATH,
            invalid_http_query_name_control,
        ),
        (
            TOOL_PACKAGE_INVALID_HTTP_QUERY_VALUE_CONTROL_FIXTURE_PATH,
            invalid_http_query_value_control,
        ),
        (
            TOOL_PACKAGE_INVALID_HTTP_JSON_PROPERTY_CONTROL_FIXTURE_PATH,
            invalid_http_json_property_control,
        ),
        (TOOL_PACKAGE_INVALID_FILE_BYTE_LENGTH_FIXTURE_PATH, invalid_file_byte_length),
        (TOOL_PACKAGE_INVALID_FILE_WINDOWS_NAME_FIXTURE_PATH, invalid_file_windows_name),
        (
            TOOL_PACKAGE_INVALID_RUNTIME_ENTRYPOINT_WINDOWS_NAME_FIXTURE_PATH,
            invalid_runtime_entrypoint_windows_name,
        ),
        (TOOL_PACKAGE_INVALID_HTTP_BASE_PATH_RELATIVE_FIXTURE_PATH, invalid_base_path("v1")),
        (
            TOOL_PACKAGE_INVALID_HTTP_BASE_PATH_RAW_SPACE_FIXTURE_PATH,
            invalid_base_path("/v1/raw space"),
        ),
        (
            TOOL_PACKAGE_INVALID_HTTP_BASE_PATH_BACKSLASH_FIXTURE_PATH,
            invalid_base_path("/v1\\items"),
        ),
        (
            TOOL_PACKAGE_INVALID_HTTP_BASE_PATH_EMPTY_MIDDLE_FIXTURE_PATH,
            invalid_base_path("/v1//items"),
        ),
        (
            TOOL_PACKAGE_INVALID_HTTP_BASE_PATH_DOUBLE_TRAILING_FIXTURE_PATH,
            invalid_base_path("/v1/items//"),
        ),
        (
            TOOL_PACKAGE_INVALID_HTTP_BASE_PATH_INVALID_PERCENT_FIXTURE_PATH,
            invalid_base_path("/v1/%2"),
        ),
        (
            TOOL_PACKAGE_INVALID_HTTP_BASE_PATH_LOWERCASE_PERCENT_FIXTURE_PATH,
            invalid_base_path("/v1/%2f"),
        ),
        (
            TOOL_PACKAGE_INVALID_HTTP_BASE_PATH_LITERAL_DOT_FIXTURE_PATH,
            invalid_base_path("/v1/./items"),
        ),
        (
            TOOL_PACKAGE_INVALID_HTTP_BASE_PATH_LITERAL_DOT_DOT_FIXTURE_PATH,
            invalid_base_path("/v1/../items"),
        ),
        (
            TOOL_PACKAGE_INVALID_HTTP_BASE_PATH_ENCODED_DOT_FIXTURE_PATH,
            invalid_base_path("/v1/%2E/items"),
        ),
        (
            TOOL_PACKAGE_INVALID_HTTP_BASE_PATH_ENCODED_DOT_DOT_FIXTURE_PATH,
            invalid_base_path("/v1/%2E%2E/items"),
        ),
        (
            TOOL_PACKAGE_INVALID_HTTP_BASE_PATH_ENCODED_SLASH_FIXTURE_PATH,
            invalid_base_path("/v1/group%2Fproject"),
        ),
        (
            TOOL_PACKAGE_INVALID_HTTP_BASE_PATH_ENCODED_BACKSLASH_FIXTURE_PATH,
            invalid_base_path("/v1/path%5Cname"),
        ),
        (TOOL_PACKAGE_INVALID_HTTP_PATH_SEGMENT_DOT_FIXTURE_PATH, invalid_path_segment(".")),
        (TOOL_PACKAGE_INVALID_HTTP_PATH_SEGMENT_DOT_DOT_FIXTURE_PATH, invalid_path_segment("..")),
        (
            TOOL_PACKAGE_INVALID_HTTP_PATH_SEGMENT_SLASH_FIXTURE_PATH,
            invalid_path_segment("group/project"),
        ),
        (
            TOOL_PACKAGE_INVALID_HTTP_PATH_SEGMENT_BACKSLASH_FIXTURE_PATH,
            invalid_path_segment("path\\name"),
        ),
        (
            TOOL_PACKAGE_INVALID_HTTP_PATH_SEGMENT_CONTROL_FIXTURE_PATH,
            invalid_path_segment("path\nname"),
        ),
    ])
}

/// Fixtures whose invariants require the Rust semantic validator and cannot be
/// completely expressed by standard JSON Schema keywords.
pub fn semantic_invalid_tool_package_manifest_fixtures() -> Result<Vec<(&'static str, Value)>> {
    let fixtures = valid_tool_package_manifest_fixtures()?;
    let stdio_base = fixtures
        .iter()
        .find(|(path, _)| *path == TOOL_PACKAGE_VALID_STDIO_FIXTURE_PATH)
        .map(|(_, value)| value.clone())
        .context("stdio manifest fixture must exist")?;
    let command_base = fixtures
        .iter()
        .find(|(path, _)| *path == TOOL_PACKAGE_VALID_COMMAND_FIXTURE_PATH)
        .map(|(_, value)| value.clone())
        .context("command manifest fixture must exist")?;
    let http_base = fixtures
        .iter()
        .find(|(path, _)| *path == TOOL_PACKAGE_VALID_HTTP_FIXTURE_PATH)
        .map(|(_, value)| value.clone())
        .context("HTTP manifest fixture must exist")?;

    let mut duplicate_file = stdio_base.clone();
    duplicate_file["files"] =
        json!([duplicate_file["files"][0].clone(), duplicate_file["files"][0].clone()]);

    let mut duplicate_tool_key = stdio_base.clone();
    duplicate_tool_key["tools"] =
        json!([duplicate_tool_key["tools"][0].clone(), duplicate_tool_key["tools"][0].clone()]);

    let mut duplicate_canonical_name = stdio_base.clone();
    let mut second_tool = duplicate_canonical_name["tools"][0].clone();
    second_tool["tool_key"] = json!("fixture_two");
    duplicate_canonical_name["tools"] =
        json!([duplicate_canonical_name["tools"][0].clone(), second_tool]);

    let mut duplicate_capability = stdio_base.clone();
    duplicate_capability["tools"][0]["capability_requirements"] =
        json!(["process_execute", "process_execute"]);

    let mut duplicate_claim_key = stdio_base.clone();
    duplicate_claim_key["tools"][0]["resource_claims"] = json!([
        {
            "type": "workspace_path",
            "claim_key": "workspace",
            "selector": { "type": "workspace_root", "path_scope": "subtree" },
            "access": "read"
        },
        {
            "type": "workspace_path",
            "claim_key": "workspace",
            "selector": { "type": "workspace_root", "path_scope": "subtree" },
            "access": "write"
        }
    ]);

    let mut duplicate_conformance = stdio_base.clone();
    duplicate_conformance["tools"][0]["conformance_cases"] = json!([
        { "name": "deterministic fixture", "arguments": {} },
        { "name": "deterministic fixture", "arguments": { "second": true } }
    ]);

    let mut duplicate_claim = stdio_base.clone();
    duplicate_claim["tools"][0]["resource_claims"] = json!([
        {
            "type": "workspace_path",
            "claim_key": "workspace_a",
            "selector": { "type": "workspace_root", "path_scope": "subtree" },
            "access": "read"
        },
        {
            "type": "workspace_path",
            "claim_key": "workspace_b",
            "selector": { "type": "workspace_root", "path_scope": "subtree" },
            "access": "read"
        }
    ]);

    let mut invalid_configuration_reference = command_base.clone();
    invalid_configuration_reference["tools"][0]["runtime"]["command"]["environment"][1]["value"]
        ["key"] = json!("not_declared");

    let mut invalid_dot_segment = http_base.clone();
    invalid_dot_segment["tools"][0]["runtime"]["request"]["target"]["base_path"] =
        json!("/v1/%2e%2e/secrets");

    let mut duplicate_environment = command_base.clone();
    duplicate_environment["tools"][0]["runtime"]["command"]["environment"]
        .as_array_mut()
        .context("command environment must be an array")?
        .push(json!({
            "name": "AURA_LABEL",
            "value": { "type": "literal", "value": "duplicate" }
        }));

    let mut credential_environment_conflict = command_base.clone();
    credential_environment_conflict["tools"][0]["runtime"]["command"]["credential_slot"] = json!({
        "slot_key": "api_key",
        "environment_variable": "AURA_LABEL"
    });

    let mut duplicate_http_header = http_base.clone();
    duplicate_http_header["tools"][0]["runtime"]["request"]["headers"]
        .as_array_mut()
        .context("HTTP headers must be an array")?
        .push(json!({
            "name": "X-Aura-Fixture",
            "value": { "type": "literal", "value": "duplicate" }
        }));

    let mut credential_http_header_conflict = http_base.clone();
    credential_http_header_conflict["tools"][0]["runtime"]["request"]["credential_slot"] = json!({
        "type": "fixed_header",
        "slot_key": "api_key",
        "header_name": "X-Aura-Fixture"
    });

    let instance_config_with_property = |property: Value| {
        let mut fixture = command_base.clone();
        fixture["tools"][0]["instance_configuration"]["properties"]["output_mode"] = property;
        fixture
    };
    let instance_config_ref = instance_config_with_property(json!({
        "type": "string",
        "$ref": "#/$defs/output_mode"
    }));
    let instance_config_dynamic_ref = instance_config_with_property(json!({
        "type": "string",
        "$dynamicRef": "#output_mode"
    }));
    let instance_config_all_of = instance_config_with_property(json!({
        "type": "string",
        "allOf": [{ "minLength": 1 }]
    }));
    let instance_config_any_of = instance_config_with_property(json!({
        "type": "string",
        "anyOf": [{ "type": "string" }]
    }));
    let instance_config_one_of = instance_config_with_property(json!({
        "type": "string",
        "oneOf": [{ "const": "fixture" }]
    }));
    let instance_config_not = instance_config_with_property(json!({
        "type": "string",
        "not": { "const": "forbidden" }
    }));
    let instance_config_conditional = instance_config_with_property(json!({
        "type": "string",
        "if": { "const": "one" },
        "then": { "minLength": 1 },
        "else": { "maxLength": 10 }
    }));

    let mut manifest_self_reference = stdio_base.clone();
    manifest_self_reference["files"][0]["path"] = json!("aura-tool-package.json");

    let mut windows_file_collision = stdio_base.clone();
    let mut collision_file = windows_file_collision["files"][0].clone();
    windows_file_collision["files"][0]["path"] = json!("Runtime/File.bin");
    collision_file["path"] = json!("runtime/file.BIN");
    windows_file_collision["files"] =
        json!([windows_file_collision["files"][0].clone(), collision_file]);

    let mut expanded_bytes = stdio_base.clone();
    let source_file = expanded_bytes["files"][0].clone();
    expanded_bytes["files"] = Value::Array(
        (0..5)
            .map(|index| {
                let mut file = source_file.clone();
                file["path"] = json!(format!("runtime/{index}.bin"));
                file["byte_length"] = json!(67_108_864_u64);
                file
            })
            .collect(),
    );

    let mut duplicate_http_property = http_base;
    let property =
        duplicate_http_property["tools"][0]["runtime"]["request"]["body"]["template"]["properties"]
            [0]
        .clone();
    duplicate_http_property["tools"][0]["runtime"]["request"]["body"]["template"]["properties"] =
        json!([property.clone(), property]);

    Ok(vec![
        (TOOL_PACKAGE_SEMANTIC_DUPLICATE_FILE_FIXTURE_PATH, duplicate_file),
        (TOOL_PACKAGE_SEMANTIC_DUPLICATE_TOOL_KEY_FIXTURE_PATH, duplicate_tool_key),
        (TOOL_PACKAGE_SEMANTIC_DUPLICATE_CANONICAL_NAME_FIXTURE_PATH, duplicate_canonical_name),
        (TOOL_PACKAGE_SEMANTIC_DUPLICATE_CAPABILITY_FIXTURE_PATH, duplicate_capability),
        (TOOL_PACKAGE_SEMANTIC_DUPLICATE_CLAIM_KEY_FIXTURE_PATH, duplicate_claim_key),
        (TOOL_PACKAGE_SEMANTIC_DUPLICATE_CLAIM_FIXTURE_PATH, duplicate_claim),
        (TOOL_PACKAGE_SEMANTIC_DUPLICATE_CONFORMANCE_FIXTURE_PATH, duplicate_conformance),
        (
            TOOL_PACKAGE_SEMANTIC_INSTANCE_CONFIGURATION_FIXTURE_PATH,
            invalid_configuration_reference,
        ),
        (TOOL_PACKAGE_SEMANTIC_HTTP_BASE_PATH_DOT_SEGMENT_FIXTURE_PATH, invalid_dot_segment),
        (TOOL_PACKAGE_SEMANTIC_DUPLICATE_ENVIRONMENT_FIXTURE_PATH, duplicate_environment),
        (
            TOOL_PACKAGE_SEMANTIC_CREDENTIAL_ENVIRONMENT_CONFLICT_FIXTURE_PATH,
            credential_environment_conflict,
        ),
        (TOOL_PACKAGE_SEMANTIC_DUPLICATE_HTTP_HEADER_FIXTURE_PATH, duplicate_http_header),
        (
            TOOL_PACKAGE_SEMANTIC_CREDENTIAL_HTTP_HEADER_CONFLICT_FIXTURE_PATH,
            credential_http_header_conflict,
        ),
        (TOOL_PACKAGE_SEMANTIC_INSTANCE_CONFIG_REF_PROPERTY_FIXTURE_PATH, instance_config_ref),
        (
            TOOL_PACKAGE_SEMANTIC_INSTANCE_CONFIG_DYNAMIC_REF_PROPERTY_FIXTURE_PATH,
            instance_config_dynamic_ref,
        ),
        (
            TOOL_PACKAGE_SEMANTIC_INSTANCE_CONFIG_ALL_OF_PROPERTY_FIXTURE_PATH,
            instance_config_all_of,
        ),
        (
            TOOL_PACKAGE_SEMANTIC_INSTANCE_CONFIG_ANY_OF_PROPERTY_FIXTURE_PATH,
            instance_config_any_of,
        ),
        (
            TOOL_PACKAGE_SEMANTIC_INSTANCE_CONFIG_ONE_OF_PROPERTY_FIXTURE_PATH,
            instance_config_one_of,
        ),
        (TOOL_PACKAGE_SEMANTIC_INSTANCE_CONFIG_NOT_PROPERTY_FIXTURE_PATH, instance_config_not),
        (
            TOOL_PACKAGE_SEMANTIC_INSTANCE_CONFIG_CONDITIONAL_PROPERTY_FIXTURE_PATH,
            instance_config_conditional,
        ),
        (TOOL_PACKAGE_SEMANTIC_MANIFEST_SELF_REFERENCE_FIXTURE_PATH, manifest_self_reference),
        (TOOL_PACKAGE_SEMANTIC_WINDOWS_FILE_COLLISION_FIXTURE_PATH, windows_file_collision),
        (TOOL_PACKAGE_SEMANTIC_EXPANDED_BYTES_FIXTURE_PATH, expanded_bytes),
        (TOOL_PACKAGE_SEMANTIC_DUPLICATE_HTTP_PROPERTY_FIXTURE_PATH, duplicate_http_property),
    ])
}

pub fn invalid_canonical_tool_package_manifest_fixtures() -> Result<Vec<(&'static str, Value)>> {
    let base = serde_json::to_value(canonical_tool_package_manifest_fixture()?)?;

    let mut command_credential = base.clone();
    command_credential["tools"][0]["runtime"]["command"]
        .as_object_mut()
        .context("canonical command must be an object")?
        .remove("credential_slot");

    let mut stdio_credential = base.clone();
    stdio_credential["tools"][0]["runtime"] = json!({
        "type": "aura_stdio",
        "protocol_version": 1,
        "executable": "bin/fixture.exe",
        "arguments": [],
        "working_directory": { "type": "package_root" },
        "environment": []
    });

    let mut http_credential = base.clone();
    http_credential["tools"][0]["runtime"] = json!({
        "type": "declarative_http",
        "request": {
            "target": {
                "type": "fixed_https",
                "origin": "https://example.invalid",
                "base_path": "/v1"
            },
            "method": "POST",
            "path": [],
            "query": [],
            "headers": [],
            "body": { "type": "none" },
            "success_status_ranges": [{ "start": 200, "end": 299 }]
        }
    });

    let mut instance_configuration = base.clone();
    instance_configuration["tools"][0]
        .as_object_mut()
        .context("canonical tool must be an object")?
        .remove("instance_configuration");

    let mut ui_description = base.clone();
    ui_description["ui"]
        .as_object_mut()
        .context("canonical UI must be an object")?
        .remove("description");

    let mut multiline_carriage_return = base.clone();
    multiline_carriage_return["tools"][0]["model"]["description"] = json!("line one\r\nline two");

    let mut semantic_version = base;
    semantic_version["tools"][0]
        .as_object_mut()
        .context("canonical tool must be an object")?
        .remove("semantic_version");

    Ok(vec![
        (TOOL_PACKAGE_CANONICAL_MISSING_COMMAND_CREDENTIAL_FIXTURE_PATH, command_credential),
        (TOOL_PACKAGE_CANONICAL_MISSING_STDIO_CREDENTIAL_FIXTURE_PATH, stdio_credential),
        (TOOL_PACKAGE_CANONICAL_MISSING_HTTP_CREDENTIAL_FIXTURE_PATH, http_credential),
        (
            TOOL_PACKAGE_CANONICAL_MISSING_INSTANCE_CONFIGURATION_FIXTURE_PATH,
            instance_configuration,
        ),
        (TOOL_PACKAGE_CANONICAL_MISSING_UI_DESCRIPTION_FIXTURE_PATH, ui_description),
        (TOOL_PACKAGE_CANONICAL_MISSING_SEMANTIC_VERSION_FIXTURE_PATH, semantic_version),
        (TOOL_PACKAGE_CANONICAL_MULTILINE_CARRIAGE_RETURN_FIXTURE_PATH, multiline_carriage_return),
    ])
}

pub fn semantic_invalid_canonical_tool_package_manifest_fixtures()
-> Result<Vec<(&'static str, Value)>> {
    let raw_http = valid_tool_package_manifest_fixtures()?
        .into_iter()
        .find(|(path, _)| *path == TOOL_PACKAGE_VALID_HTTP_FIXTURE_PATH)
        .map(|(_, value)| value)
        .context("HTTP manifest fixture must exist")?;
    let canonical_http = serde_json::from_value::<RawToolPackageManifestV1>(raw_http)?
        .into_canonical_contract()
        .map_err(anyhow::Error::msg)?;
    let mut http_properties_unsorted = serde_json::to_value(canonical_http)?;
    http_properties_unsorted["tools"][0]["runtime"]["request"]["body"]["template"] = json!({
        "type": "object",
        "properties": [
            { "name": "z", "value": { "type": "literal", "value": 1 } },
            { "name": "a", "value": { "type": "literal", "value": 2 } }
        ]
    });

    let mut files_unsorted = serde_json::to_value(canonical_tool_package_manifest_fixture()?)?;
    let mut preceding_file = files_unsorted["files"][0].clone();
    preceding_file["path"] = json!("a.txt");
    files_unsorted["files"] = json!([files_unsorted["files"][0].clone(), preceding_file]);

    Ok(vec![
        (TOOL_PACKAGE_CANONICAL_HTTP_PROPERTIES_UNSORTED_FIXTURE_PATH, http_properties_unsorted),
        (TOOL_PACKAGE_CANONICAL_FILES_UNSORTED_FIXTURE_PATH, files_unsorted),
    ])
}

pub fn canonical_tool_package_manifest_fixture() -> Result<CanonicalToolPackageManifestV1> {
    let runtime_bytes = b"canonical command fixture runtime";
    Ok(CanonicalToolPackageManifestV1 {
        schema_version: ToolPackageManifestSchemaVersionV1,
        package_key: PackageKey::new("org.aura.fixture.canonical")?,
        package_version: CanonicalSemVer::new("1.0.0")?,
        minimum_aura_version: CanonicalSemVer::new("0.1.0")?,
        ui: CanonicalToolPackageUiV1 {
            display_name: ToolPackageUiDisplayNameV1::new("Canonical Fixture Package")?,
            description: CanonicalToolPackageUiDescriptionV1(None),
        },
        files: vec![ToolPackageFileEntryV1 {
            path: PackageRelativePath::new("bin/fixture.exe")?,
            byte_length: u64::try_from(runtime_bytes.len())?,
            sha256: ToolPackageFileSha256V1::sha256(runtime_bytes),
            role: ToolPackageFileRoleV1::RuntimeExecutable,
        }],
        tools: vec![CanonicalToolPackageToolV1 {
            tool_key: ToolKey::new("fixture")?,
            semantic_version: CanonicalToolSemanticVersionV1(None),
            ui: ToolPackageToolUiV1 {
                display_name: ToolPackageToolDisplayNameV1::new("Fixture Tool")?,
            },
            model: CanonicalToolPackageModelContractV1 {
                canonical_name: ToolCanonicalName::new("user.org.aura.fixture.canonical.fixture")?,
                description: ToolPackageModelDescriptionV1::new(
                    "Execute the package fixture with validated arguments.",
                )?,
                summary: ToolPackageModelSummaryV1::new(
                    "Execute a deterministic package fixture.",
                )?,
                input_schema: ToolInputSchemaV1::new(json!({
                    "$schema": "https://json-schema.org/draft/2020-12/schema",
                    "type": "object",
                    "properties": { "text": { "type": "string" } },
                    "required": ["text"],
                    "additionalProperties": false
                }))?,
                strict: true,
                result_contract: ToolResultContractV1::Utf8Text,
            },
            runtime: CanonicalToolPackageRuntimeV1::DeclarativeCommand {
                command: CanonicalToolPackageDeclarativeCommandV1::Argv {
                    executable: PackageRelativePath::new("bin/fixture.exe")?,
                    arguments: vec![ToolPackageCommandArgumentV1::Scalar {
                        source: ArgumentJsonPointer::new("/text")?,
                    }],
                    working_directory: CommandWorkingDirectoryV1::PackageRoot,
                    environment: Vec::new(),
                    credential_slot: CanonicalCommandCredentialSlotV1(None),
                    success_exit_codes: ToolPackageCommandSuccessExitCodesV1::default_v1(),
                },
            },
            instance_configuration: CanonicalToolPackageInstanceConfigurationSchemaV1(None),
            capability_requirements: vec![PackageCapabilityRequirementV1::ProcessExecute],
            resource_claims: Vec::new(),
            audience: PackageToolAudienceV1::SubagentConfigurable,
            execution_semantics: ToolExecutionSemanticsV1 {
                automatic_retry_allowed: false,
                repeat_is_idempotent: true,
                may_create_persistent_effects: false,
            },
            conformance_cases: vec![ToolPackageConformanceCaseV1 {
                name: ToolPackageConformanceCaseNameV1::new("echo text")?,
                arguments: ToolPackageConformanceArgumentsV1::new(json!({ "text": "fixture" }))
                    .map_err(anyhow::Error::msg)?,
            }],
        }],
    })
}

pub fn additional_canonical_tool_package_manifest_fixtures() -> Result<Vec<(&'static str, Value)>> {
    let raw_http = valid_tool_package_manifest_fixtures()?
        .into_iter()
        .find(|(path, _)| *path == TOOL_PACKAGE_VALID_HTTP_FIXTURE_PATH)
        .map(|(_, value)| value)
        .context("HTTP manifest fixture must exist")?;
    let convert = |package_key: &str, template: Value| -> Result<Value> {
        let mut raw = raw_http.clone();
        raw["package_key"] = json!(package_key);
        raw["tools"][0]["runtime"]["request"]["body"]["template"] = template;
        let canonical = serde_json::from_value::<RawToolPackageManifestV1>(raw)?
            .into_canonical_contract()
            .map_err(anyhow::Error::msg)?;
        Ok(serde_json::to_value(canonical)?)
    };

    let sorted = convert(
        "org.aura.fixture.http.canonical-sorted",
        json!({
            "type": "object",
            "properties": [
                { "name": "z", "value": { "type": "literal", "value": 1 } },
                { "name": "a", "value": { "type": "literal", "value": 2 } }
            ]
        }),
    )?;
    let nested = convert(
        "org.aura.fixture.http.canonical-nested",
        json!({
            "type": "object",
            "properties": [
                { "name": "z", "value": { "type": "literal", "value": 1 } },
                {
                    "name": "a",
                    "value": {
                        "type": "object",
                        "properties": [
                            { "name": "nested_z", "value": { "type": "literal", "value": 3 } },
                            { "name": "nested_a", "value": { "type": "literal", "value": 4 } }
                        ]
                    }
                }
            ]
        }),
    )?;
    let unicode = convert(
        "org.aura.fixture.http.canonical-unicode",
        json!({
            "type": "object",
            "properties": [
                { "name": "\u{e000}", "value": { "type": "literal", "value": 1 } },
                { "name": "\u{10000}", "value": { "type": "literal", "value": 2 } }
            ]
        }),
    )?;
    let array = convert(
        "org.aura.fixture.http.canonical-array",
        json!({
            "type": "array",
            "items": [
                { "type": "literal", "value": "second" },
                { "type": "literal", "value": "first" }
            ]
        }),
    )?;

    let fixtures = vec![
        (TOOL_PACKAGE_CANONICAL_HTTP_PROPERTIES_SORTED_FIXTURE_PATH, sorted),
        (TOOL_PACKAGE_CANONICAL_HTTP_PROPERTIES_NESTED_FIXTURE_PATH, nested),
        (TOOL_PACKAGE_CANONICAL_HTTP_PROPERTIES_UNICODE_FIXTURE_PATH, unicode),
        (TOOL_PACKAGE_CANONICAL_HTTP_ARRAY_ORDER_FIXTURE_PATH, array),
    ];
    for (path, value) in &fixtures {
        serde_json::from_value::<CanonicalToolPackageManifestV1>(value.clone())
            .with_context(|| format!("valid canonical tool package manifest fixture {path}"))?;
    }
    Ok(fixtures)
}

pub fn package_runtime_closure_fixture() -> Result<Value> {
    Ok(staged_package_fixture()?.runtime_closure)
}

struct StagedPackageContractFixtureV1 {
    canonical_manifest: CanonicalToolPackageManifestV1,
    runtime_closure: Value,
    runtime_closure_hash: PackageRuntimeClosureHashV1,
    tool_contract_fingerprint_preimage: ToolContractFingerprintPreimageV1,
    tool_contract_fingerprint: ToolContractFingerprintV1,
    package_fingerprint_preimage: PackageFingerprintPreimageV1,
    package_fingerprint: PackageFingerprintV1,
    package_security_fingerprint_preimage: PackageSecurityFingerprintPreimageV1,
    package_security_fingerprint: PackageSecurityFingerprintV1,
}

fn staged_package_fixture() -> Result<StagedPackageContractFixtureV1> {
    const MANIFEST: &[u8] =
        include_bytes!("../../../fixtures/tool-package/manifest-v1/valid/aura-stdio.json");
    const RUNTIME: &[u8] = b"stdio fixture runtime";

    let source = tempfile::tempdir()?;
    let staging = tempfile::tempdir()?;
    fs::create_dir_all(source.path().join("bin"))?;
    fs::write(source.path().join(TOOL_PACKAGE_MANIFEST_NAME), MANIFEST)?;
    fs::write(source.path().join("bin/fixture.exe"), RUNTIME)?;
    let staged = stage_directory_package_v1(source.path(), staging.path())?;
    let evidence = staged.fingerprint_evidence()?;
    let tool_contract_fingerprint_preimage = evidence
        .tool_contract_preimages()
        .first()
        .context("staged fixture must contain one Tool fingerprint preimage")?
        .clone();
    let tool_contract_fingerprint = evidence
        .tool_contract_fingerprints()
        .first()
        .context("staged fixture must contain one Tool fingerprint")?
        .fingerprint()
        .clone();
    Ok(StagedPackageContractFixtureV1 {
        canonical_manifest: staged.canonical_manifest().clone(),
        runtime_closure: serde_json::to_value(staged.runtime_closure())?,
        runtime_closure_hash: staged.runtime_closure().canonical_hash()?,
        tool_contract_fingerprint_preimage,
        tool_contract_fingerprint,
        package_fingerprint_preimage: evidence.package_preimage().clone(),
        package_fingerprint: evidence.package_fingerprint().clone(),
        package_security_fingerprint_preimage: evidence.package_security_preimage().clone(),
        package_security_fingerprint: evidence.package_security_fingerprint().clone(),
    })
}

pub fn canonical_fixture() -> Result<CanonicalContractFixture> {
    const ULID_BODY: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

    let parse_id = |prefix: &str| format!("{prefix}_{ULID_BODY}");
    let parse_alt_id = |prefix: &str| format!("{prefix}_01ARZ3NDEKTSV4RRFFQ69G5FAW");
    let parse_third_id = |prefix: &str| format!("{prefix}_01ARZ3NDEKTSV4RRFFQ69G5FAX");
    let correlation_id = CorrelationId::from_str(&parse_id("cor"))?;
    let causation_id = CausationId::from_str(&parse_id("cau"))?;
    let runtime_instance_id = RuntimeInstanceId::from_str(&parse_id("run"))?;
    let parent_runtime_instance_id = RuntimeInstanceId::from_str("run_01ARZ3NDEKTSV4RRFFQ69G5FAW")?;
    let release_bundle_id = ReleaseBundleId::from_str(&parse_id("rel"))?;
    let bundle_namespace: BundleNamespace = "aura.desktop.test.contracts".parse()?;
    let release_descriptor = ReleaseDescriptor {
        bundle_namespace: bundle_namespace.clone(),
        identity: ReleaseIdentity {
            release_bundle_id,
            component_manifest_hash: ContentHash::new(
                ContentHashAlgorithm::Sha256,
                "66".repeat(32),
            )?,
        },
    };
    let stream_id = StreamId::from_str(&parse_id("str"))?;
    let ipc_channel_id = IpcChannelId::from_str(&parse_id("chn"))?;
    let command_invocation_id = CommandInvocationId::from_str(&parse_id("civ"))?;
    let route_endpoint_id = EndpointId::from_str(&parse_alt_id("end"))?;
    let route_model_profile_id = ModelProfileId::from_str(&parse_alt_id("mdl"))?;
    let route_session_id = SessionId::from_str(&parse_alt_id("ses"))?;
    let route_model_availability_test_id = ModelAvailabilityTestId::from_str(&parse_alt_id("mav"))?;

    let error = ErrorEnvelope::new(
        ErrorCode::new("contract.example_error")?,
        ErrorCategory::Validation,
        "Example typed contract error.",
        false,
        correlation_id,
        Some(causation_id),
    )
    .with_detail("field", json!("example"));

    let component_manifest = BuildComponentManifest {
        manifest_version: ContractVersion::CURRENT,
        bundle_namespace,
        release_bundle_id,
        release_channel: ReleaseChannel::Nightly,
        product_version: "0.1.0".to_owned(),
        build_commit: "0000000000000000000000000000000000000000".to_owned(),
        target_triple: "x86_64-pc-windows-msvc".to_owned(),
        components: vec![
            BuildComponent {
                component_id: "aura-desktop".to_owned(),
                kind: BuildComponentKind::Desktop,
                version: "0.1.0".to_owned(),
                relative_path: "Aura.exe".to_owned(),
                content_hash: ContentHash::new(ContentHashAlgorithm::Sha256, "11".repeat(32))?,
                protocol_version: Some(ContractVersion::CURRENT),
            },
            BuildComponent {
                component_id: "aura-core".to_owned(),
                kind: BuildComponentKind::Core,
                version: "0.1.0".to_owned(),
                relative_path: "aura-core.exe".to_owned(),
                content_hash: ContentHash::new(ContentHashAlgorithm::Sha256, "22".repeat(32))?,
                protocol_version: Some(ContractVersion::CURRENT),
            },
            BuildComponent {
                component_id: "aura-main-pipe".to_owned(),
                kind: BuildComponentKind::MainPipeBridge,
                version: "0.1.0".to_owned(),
                relative_path: "resources/native/aura-main-pipe.node".to_owned(),
                content_hash: ContentHash::new(ContentHashAlgorithm::Sha256, "66".repeat(32))?,
                protocol_version: Some(ContractVersion::CURRENT),
            },
            BuildComponent {
                component_id: "aura-sandbox-host".to_owned(),
                kind: BuildComponentKind::SandboxHost,
                version: "0.1.0".to_owned(),
                relative_path: "aura-sandbox-host.exe".to_owned(),
                content_hash: ContentHash::new(ContentHashAlgorithm::Sha256, "33".repeat(32))?,
                protocol_version: Some(ContractVersion::CURRENT),
            },
            BuildComponent {
                component_id: "aura-egress-proxy".to_owned(),
                kind: BuildComponentKind::EgressProxy,
                version: "0.1.0".to_owned(),
                relative_path: "aura-egress-proxy.exe".to_owned(),
                content_hash: ContentHash::new(ContentHashAlgorithm::Sha256, "44".repeat(32))?,
                protocol_version: Some(ContractVersion::CURRENT),
            },
            BuildComponent {
                component_id: "aura-elevated-helper".to_owned(),
                kind: BuildComponentKind::ElevatedHelper,
                version: "0.1.0".to_owned(),
                relative_path: "aura-elevated-helper.exe".to_owned(),
                content_hash: ContentHash::new(ContentHashAlgorithm::Sha256, "55".repeat(32))?,
                protocol_version: Some(ContractVersion::CURRENT),
            },
        ],
    };

    let client_handshake = RuntimeHandshake {
        protocol_version: ContractVersion::CURRENT,
        process_kind: RuntimeProcessKind::ElectronMain,
        component_version: "0.1.0".to_owned(),
        build_commit: component_manifest.build_commit.clone(),
        release_descriptor: release_descriptor.clone(),
        runtime_instance_id: parent_runtime_instance_id,
        parent_runtime_instance_id: None,
        supported_features: vec![
            "aura.ipc.v1".to_owned(),
            "aura.projection.core-health.v1".to_owned(),
            "aura.security-control-plane.v1".to_owned(),
        ],
    };
    let server_handshake = RuntimeHandshake {
        protocol_version: ContractVersion::CURRENT,
        process_kind: RuntimeProcessKind::Core,
        component_version: "0.1.0".to_owned(),
        build_commit: component_manifest.build_commit.clone(),
        release_descriptor: release_descriptor.clone(),
        runtime_instance_id,
        parent_runtime_instance_id: Some(parent_runtime_instance_id),
        supported_features: vec![
            "aura.ipc.v1".to_owned(),
            "aura.ipc.authenticated-channel.v1".to_owned(),
            "aura.runtime.command-outcome.v1".to_owned(),
        ],
    };
    let client_nonce = SecretNonce::from_bytes([0x11; 32]);
    let server_nonce = SecretNonce::from_bytes([0x22; 32]);
    let handshake_transcript = RuntimeHandshakeTranscript {
        handshake_kind: RuntimeHandshakeKind::Initial,
        channel_id: ipc_channel_id,
        connection_epoch: 1,
        client_nonce: client_nonce.clone(),
        server_nonce: server_nonce.clone(),
        client_handshake: client_handshake.clone(),
        server_handshake: server_handshake.clone(),
        negotiated_protocol_version: ContractVersion::CURRENT,
    };
    let transcript_hash = runtime_handshake_transcript_hash(&handshake_transcript)?;
    let bootstrap_proof = runtime_bootstrap_proof(&[0x33; 32], &transcript_hash)?;
    let projection = CoreHealthProjection::ready("Aura Core is ready.");
    let projection_change = ProjectionChange::CoreHealth {
        runtime_instance_id,
        projection_scope: ProjectionScope::Global,
        revision: Revision::new(42),
        payload: projection,
    };
    let workspace_session_projection_change = ProjectionChange::WorkspaceSession {
        runtime_instance_id,
        projection_scope: ProjectionScope::Global,
        revision: Revision::new(1),
        payload: WorkspaceSessionProjection {
            workspaces: vec![WorkspaceSummary {
                workspace_id: WorkspaceId::from_str(&parse_id("wsp"))?,
                display_name: "Aura Workspace".to_owned(),
                selected_root: r"C:\Users\测试用户\Aura Workspace".to_owned(),
                canonical_root: r"C:\Users\测试用户\Aura Workspace".to_owned(),
                root_identity: None,
                state: WorkspaceState::Online,
                revision: Revision::INITIAL,
                catalog_freshness: CatalogFreshness::Fresh,
                authorization_epoch: Revision::INITIAL,
            }],
            sessions: vec![SessionSummary {
                session_id: SessionId::from_str(&parse_id("ses"))?,
                workspace_id: WorkspaceId::from_str(&parse_id("wsp"))?,
                title: "Architecture review".to_owned(),
                state: "idle".to_owned(),
            }],
        },
    };
    let mut ipc_details = BTreeMap::new();
    ipc_details.insert("transport".to_owned(), json!("named_pipe"));
    let ipc_error = IpcErrorEnvelope {
        code: ErrorCode::new("ipc.transport_lost")?,
        category: ErrorCategory::Transport,
        message: "The Core transport disconnected.".to_owned(),
        retryable: true,
        correlation_id,
        terminal: IpcRequestTerminal::TransportLost,
        details: ipc_details,
    };
    let runtime = RuntimeContractFixture {
        frame_header: IpcFrameHeader {
            channel_id: ipc_channel_id,
            connection_epoch: 1,
            sequence: 2,
        },
        handshake_request: RuntimeHandshakeRequest {
            handshake_kind: RuntimeHandshakeKind::Initial,
            handshake: client_handshake.clone(),
            channel_id: ipc_channel_id,
            connection_epoch: 1,
            client_nonce: client_nonce.clone(),
        },
        handshake_response: RuntimeHandshakeResponse {
            handshake_kind: RuntimeHandshakeKind::Initial,
            handshake: server_handshake,
            channel_id: ipc_channel_id,
            connection_epoch: 1,
            client_nonce,
            server_nonce,
            negotiated_protocol_version: ContractVersion::CURRENT,
            transcript_hash: transcript_hash.clone(),
        },
        handshake_transcript,
        handshake_finish: RuntimeHandshakeFinish {
            handshake_kind: RuntimeHandshakeKind::Initial,
            channel_id: ipc_channel_id,
            connection_epoch: 1,
            transcript_hash: transcript_hash.clone(),
            bootstrap_proof: Some(bootstrap_proof),
        },
        handshake_complete: RuntimeHandshakeComplete {
            handshake_kind: RuntimeHandshakeKind::Initial,
            channel_id: ipc_channel_id,
            connection_epoch: 1,
            transcript_hash,
        },
        authenticated_peer_identity: AuthenticatedPeerIdentity {
            process_id: 4242,
            process_creation_time_100ns: 133_700_000,
            runtime_instance_id: parent_runtime_instance_id,
            release_descriptor: release_descriptor.clone(),
            windows_session_id: 1,
        },
        command_outcome: CommandOutcome::Unknown {
            command_invocation_id,
            reason: CommandOutcomeUnknown::NotRecordedOrIncomplete,
        },
        startup_progress: CoreStartupProgress {
            runtime_instance_id,
            sequence: 3,
            phase: CoreStartupPhase::Ready,
            message: "core.ready".to_owned(),
            occurred_at: UtcTimestamp::from_str("2026-08-29T00:00:00Z")?,
        },
        heartbeat: RuntimeHeartbeat {
            runtime_instance_id,
            sequence: 7,
            sent_at: UtcTimestamp::from_str("2026-08-29T00:00:02Z")?,
        },
        projection_subscription: ProjectionSubscription {
            projection_kind: ProjectionKind::CoreHealth,
            projection_scope: ProjectionScope::Global,
            known_runtime_instance_id: Some(runtime_instance_id),
            after_revision: Some(Revision::new(41)),
        },
        projection_unsubscribe_request: ProjectionUnsubscribeRequest {
            projection_kind: ProjectionKind::CoreHealth,
            projection_scope: ProjectionScope::Global,
        },
        projection_snapshot_request: ProjectionSnapshotRequest {
            projection_kind: ProjectionKind::CoreHealth,
            projection_scope: ProjectionScope::Global,
            known_runtime_instance_id: Some(runtime_instance_id),
        },
        projection_change,
        projection_keys: vec![
            ProjectionKey {
                runtime_instance_id,
                projection_kind: ProjectionKind::CoreHealth,
                projection_scope: ProjectionScope::Global,
            },
            ProjectionKey {
                runtime_instance_id,
                projection_kind: ProjectionKind::WorkspaceSession,
                projection_scope: ProjectionScope::Global,
            },
            ProjectionKey {
                runtime_instance_id,
                projection_kind: ProjectionKind::WorkspaceFiles,
                projection_scope: ProjectionScope::Global,
            },
            ProjectionKey {
                runtime_instance_id,
                projection_kind: ProjectionKind::ModelSettings,
                projection_scope: ProjectionScope::Global,
            },
            ProjectionKey {
                runtime_instance_id,
                projection_kind: ProjectionKind::ProviderCatalog,
                projection_scope: ProjectionScope::Global,
            },
            ProjectionKey {
                runtime_instance_id,
                projection_kind: ProjectionKind::EndpointDetail,
                projection_scope: ProjectionScope::Endpoint { endpoint_id: route_endpoint_id },
            },
            ProjectionKey {
                runtime_instance_id,
                projection_kind: ProjectionKind::ModelProfile,
                projection_scope: ProjectionScope::ModelProfile {
                    model_profile_id: route_model_profile_id,
                },
            },
            ProjectionKey {
                runtime_instance_id,
                projection_kind: ProjectionKind::SessionModelSelector,
                projection_scope: ProjectionScope::Session { session_id: route_session_id },
            },
            ProjectionKey {
                runtime_instance_id,
                projection_kind: ProjectionKind::ModelAvailabilityTest,
                projection_scope: ProjectionScope::ModelAvailabilityTest {
                    model_availability_test_id: route_model_availability_test_id,
                },
            },
            ProjectionKey {
                runtime_instance_id,
                projection_kind: ProjectionKind::ContextWindowCorrection,
                projection_scope: ProjectionScope::Session { session_id: route_session_id },
            },
        ],
        binary_stream_frame: BinaryStreamFrame {
            stream_id,
            chunk_sequence: 0,
            end_of_message: true,
            data_length: 262_144,
        },
        ipc_error,
        cancellation: CancellationRequest { request_id: correlation_id },
        active_install: ActiveInstallState {
            active: "0.1.0".to_owned(),
            release_descriptor,
            previous: None,
            pending: None,
            last_known_good: Some("0.1.0".to_owned()),
        },
    };
    let persistence = PersistenceContractFixture {
        credential_ref: "wincred://aura/model/main".parse()?,
        artifact_record: ArtifactRecord {
            artifact_id: ArtifactId::from_str(&parse_id("art"))?,
            owner: ArtifactOwnerRef::Session { session_id: route_session_id },
            scope: ArtifactScope::Session,
            media_type: "text/plain".to_owned(),
            access: ArtifactAccess::USER_READ.union(ArtifactAccess::MODEL_READ),
            byte_length: 12,
            content_hash: ContentHash::blake3(b"hello, Aura!"),
            inline: true,
            created_at: UtcTimestamp::from_str("2026-08-29T00:00:03Z")?,
        },
        workspace_session_projection_change,
    };
    let workspace_path = WorkspacePath::parse("src/main.rs")?;
    let workspace_files = WorkspaceFilesContractFixture {
        internal_file_target: FileTargetRef::Workspace { path: workspace_path.clone() },
        model_visible_workspace_target: WorkspaceFileTarget { path: workspace_path },
        workspace_files_projection_change: ProjectionChange::WorkspaceFiles {
            runtime_instance_id,
            projection_scope: ProjectionScope::Global,
            revision: Revision::INITIAL,
            payload: WorkspaceFilesProjection {
                workspaces: Vec::new(),
                lifecycle_operations: Vec::new(),
            },
        },
        logical_file_snapshot: None,
        workspace_create: None,
        workspace_create_result: None,
        workspace_restore: None,
        workspace_relocate: None,
        workspace_archive: None,
        workspace_delete: None,
        workspace_delete_result: None,
        path_search: None,
        path_search_result: None,
        buffer_open: None,
        buffer_edit: None,
        buffer_edit_result: None,
        buffer_save: None,
        lifecycle_begin: None,
        lifecycle_begin_result: None,
        lifecycle_prepare: None,
        lifecycle_prepare_delivered: None,
        lifecycle_query: None,
        lifecycle_ack: None,
        lifecycle_issue_discard_confirmation: None,
        lifecycle_discard_confirmation: None,
        lifecycle_resolve: None,
        lifecycle_renderer_unavailable: None,
        fs_stat: None,
        fs_stat_result: None,
        fs_list: None,
        fs_list_result: None,
        fs_find_paths: None,
        fs_glob: None,
        fs_glob_result: None,
        fs_read_text: None,
        fs_read_text_result: None,
        fs_search_text: None,
        fs_search_text_result: None,
        fs_hash: None,
        fs_hash_result: None,
        fs_inspect: None,
        fs_inspect_result: None,
        fs_read_binary_range: None,
        fs_read_binary_range_result: None,
        fs_write_file: None,
        fs_apply_patch: None,
        fs_create_directory: None,
        fs_copy: None,
        fs_move: None,
        fs_delete: None,
        fs_apply_changeset: None,
        fs_apply_changeset_result: None,
        fs_mutation_result: None,
        artifact_capture_workspace_file: None,
        artifact_capture_workspace_file_result: None,
        artifact_export: None,
        artifact_export_result: None,
    };
    let security = SecurityContractFixture {
        model_channel_data_disposition: ModelChannelDataDisposition::AlwaysDenied,
        model_egress_policy_version: ModelEgressPolicyVersion::CURRENT,
        operation_security_plan: None,
        permission_snapshot: None,
        permission_decision: None,
        permission_grant: None,
        get_session_state: None,
        set_session_mode: None,
        permission_defaults: None,
        set_permission_defaults: None,
        list_grants: None,
        revoke_grant: None,
        approval_request: None,
        approval_resolve: None,
        approval_resolution: None,
        approval_details: None,
        trust_fingerprint: None,
        trust_record: None,
        external_extension_risk: None,
        sandbox_execution_plan: None,
        w7_tool_operation: None,
        tool_sandbox_dispatch_binding: None,
        tool_conformance_source: None,
        tool_conformance_runtime_scope: None,
        conformance_sandbox_profile: None,
        credential_reflection_scan: None,
        authorized_tool_sandbox_execution: None,
        external_process_launch_plan: None,
        sandbox_host_event: None,
        sandbox_network_policy_error: None,
        host_preparation_status: None,
        host_preparation_begin: None,
        host_preparation_mutation: None,
        host_preparation_resolve: None,
        external_grant_record: None,
        external_grant_view: None,
        file_session_consent_view: None,
        file_session_consent_list: None,
        file_session_consent_revoke: None,
        native_session_consent_view: None,
        native_session_consent_scope: None,
        native_session_consent_list: None,
        native_session_consent_revoke: None,
        external_grant_attach_read_only: None,
        external_grant_derive_workspace: None,
        external_grant_list: None,
        external_grant_revalidate: None,
        external_grant_revoke: None,
        network_request_plan: None,
        authorized_network_request: None,
        network_grant: None,
        network_audit: None,
        quarantine_record: None,
        quarantine_prepare_release: None,
        quarantine_prepare_release_result: None,
        quarantine_release: None,
        quarantine_issue_discard_confirmation: None,
        quarantine_discard_confirmation: None,
        quarantine_discard: None,
        projection_change: None,
    };

    let model_provider_id = ProviderId::from_str(&parse_id("prv"))?;
    let model_endpoint_id = EndpointId::from_str(&parse_id("end"))?;
    let model_profile_id = ModelProfileId::from_str(&parse_id("mdl"))?;
    let credential_id = CredentialId::from_str(&parse_id("crd"))?;
    let model_execution_snapshot_id = ModelExecutionSnapshotId::from_str(&parse_id("mex"))?;
    let transport_attempt_id = TransportAttemptId::from_str(&parse_id("tat"))?;
    let model_step_id = ModelStepId::from_str(&parse_id("stp"))?;
    let context_snapshot_id = ContextProjectionSnapshotId::from_str(&parse_id("cps"))?;
    let prompt_bundle_snapshot_id = PromptBundleSnapshotId::from_str(&parse_id("pbs"))?;
    let task_lineage_id = TaskLineageId::from_str(&parse_id("lin"))?;
    let provider_tool_call_id = ProviderToolCallId::new("Call_Contract_ExactCase")?;
    let model_created_at = UtcTimestamp::from_str("2026-08-29T00:00:04Z")?;
    let model_updated_at = UtcTimestamp::from_str("2026-08-29T00:00:05Z")?;
    let endpoint_fingerprint = ContentHash::blake3(b"canonical-openai-responses-endpoint");
    let credential_revision = CredentialRevision::new(Revision::new(4))?;
    let tool_id = ToolId::from_str(&parse_id("tol"))?;
    let declared_tool_version = ToolVersion::new(
        Some(CanonicalSemVer::new("1.2.3-alpha.1+contract.5")?),
        Revision::new(4),
    )?;
    let unversioned_tool_version = ToolVersion::new(None, Revision::INITIAL)?;
    let context_item_hash = ContentHash::sha256(b"canonical context contract item");
    let context_budget = ContextBudgetSnapshot {
        policy: CONTEXT_BUDGET_POLICY_V1.to_owned(),
        runtime_effective_window_tokens: 256_000,
        estimated_input_tokens: 31,
        effective_max_output_tokens: 512,
        output_reserve_tokens: 512,
        safety_reserve_tokens: 12_800,
        request_input_budget_tokens: 242_688,
        item_estimates: vec![ContextItemEstimate {
            ordinal: 1,
            content_hash: context_item_hash.clone(),
            byte_length: 31,
            estimated_tokens: 31,
            estimator: TokenEstimatorIdentity {
                estimator_id: UTF8_BYTE_UPPER_BOUND_ESTIMATOR_ID.to_owned(),
                estimator_version: 1,
                exactness: TokenEstimateExactness::UpperBound,
            },
        }],
    };
    let model_egress_manifest = ModelEgressManifestV1::new(
        context_snapshot_id,
        vec![ModelEgressItem {
            ordinal: 1,
            source_ref: ContextSourceRef::PromptModule {
                module_id: "core.identity".to_owned(),
                content_hash: context_item_hash.clone(),
            },
            source_kind: ModelEgressSourceKind::AuraCore,
            data_class: aura_security_contract::NetworkDataClass::PublicContent,
            content_hash: context_item_hash,
            byte_length: 31,
            authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
        }],
    )?;
    let p08_task_lineage = aura_context_fixtures::p08_active_task_lineage_fixture()?;
    let (p07_context_tool_schema_set, p07_context_effective_tool_bindings) =
        aura_context_fixtures::p07_context_tool_schema_set()?;
    let context = ContextContractFixture {
        prompt_settings_snapshot: PromptSettingsSnapshot {
            revision: 3,
            custom_system_addendum: Some("Preserve exact user-authored constraints.".to_owned()),
        },
        prompt_bundle_snapshot: None,
        context_projection_snapshot: None,
        budget_snapshot: Some(context_budget),
        model_egress_manifest: Some(model_egress_manifest),
        preparation_outcome: None,
        context_search_input: None,
        context_read_events_input: None,
        context_read_tool_result_input: None,
        compaction: None,
        compaction_anchor_manifest: None,
        compaction_model_attempt: None,
        manual_compaction_request: None,
        prompt_inspector_projection: None,
        history_atomic_group_preimage: None,
        history_atomic_group_ref: None,
        preparation_trigger: None,
        provider_continuation_binding: None,
        tool_result_context_projection: None,
        p07_context_tool_schema_set,
        p07_context_effective_tool_bindings,
        p08_active_task_lineage: Some(p08_task_lineage.view),
    };
    let tool_registry_snapshot_ref = ToolRegistrySnapshotRef {
        tool_registry_snapshot_id: ToolRegistrySnapshotId::from_str(&parse_id("trs"))?,
        registry_revision: Revision::new(11),
        content_hash: CanonicalHashV1::sha256(b"contract-model-tool-registry"),
    };
    let tool_schema = ToolSchemaSnapshotV1::new(
        tool_id,
        declared_tool_version.clone(),
        "fs.read_text".parse()?,
        "Read UTF-8 text from an authorized workspace file.".to_owned(),
        ToolInputSchemaV1::new(json!({
            "type": "object",
            "properties": {"path": {"type": "string"}},
            "required": ["path"],
            "additionalProperties": false
        }))?,
        true,
    )?;
    let tool_schema_set = ToolSchemaSetSnapshotV1::new(
        tool_registry_snapshot_ref.clone(),
        vec![tool_schema.clone()],
    )?;
    let (_, provider_wrapped_schema_hash) =
        aura_model_domain::provider_arguments_envelope_schema_v3(&tool_schema.input_schema)?;
    let tool_name_binding = ProviderToolNameBinding::from_snapshot(
        &tool_registry_snapshot_ref,
        &tool_schema,
        provider_wrapped_schema_hash,
    );
    let ordered_blocks = [
        LogicalInstructionKind::Core,
        LogicalInstructionKind::Permission,
        LogicalInstructionKind::ToolSummary,
        LogicalInstructionKind::WorkspaceAgents,
        LogicalInstructionKind::HistoricalSnapshot,
        LogicalInstructionKind::AgentMode,
        LogicalInstructionKind::Hook,
        LogicalInstructionKind::TaskState,
    ]
    .into_iter()
    .map(|kind| {
        let content = format!("contract {kind:?}");
        LogicalInstructionBlock {
            kind,
            content_hash: ContentHash::sha256(content.as_bytes()),
            content,
            source_event_ids: Vec::new(),
        }
    })
    .collect::<Vec<_>>();
    let canonical_instruction_text =
        ordered_blocks.iter().map(|block| block.content.as_str()).collect::<Vec<_>>().join("\n");
    let logical_request = LogicalModelRequest {
        fixed_tool_ids: vec![],
        model_step_id,
        execution_snapshot_id: model_execution_snapshot_id,
        context_snapshot_id,
        instruction_envelope: CompiledInstructionEnvelope::new(
            1,
            ordered_blocks,
            canonical_instruction_text,
        )?,
        conversation_items: Vec::new(),
        tool_schema_set,
        attachments: Vec::new(),
        provider_continuation: None,
        output_request: ModelOutputRequest {
            format: ModelOutputFormat::Text,
            effective_max_output_tokens: 512,
        },
    };
    logical_request.validate_contract_shape()?;
    let provider_request_evidence = ProviderRequestEvidence {
        body_sha256: ContentHash::new(ContentHashAlgorithm::Sha256, "88".repeat(32))?,
        body_byte_length: 2,
        target: ProviderRequestTargetMetadata {
            method: "POST".to_owned(),
            origin: NormalizedOrigin {
                scheme: "https".to_owned(),
                host: "api.openai.example".to_owned(),
                port: 443,
            },
            path: "/v1/responses".to_owned(),
            query_parameter_names: BTreeSet::new(),
        },
        headers: vec![
            ProviderRequestHeaderMetadata {
                name: "authorization".to_owned(),
                class: ProviderRequestHeaderClass::CredentialInjected,
                non_secret_value: None,
            },
            ProviderRequestHeaderMetadata {
                name: "content-type".to_owned(),
                class: ProviderRequestHeaderClass::ContentMetadata,
                non_secret_value: Some("application/json".to_owned()),
            },
        ],
        compatibility_removals: BTreeSet::new(),
        applied_output_token_field_alias: None,
        adapter: ModelAdapterIdentity::v1(ModelApiFormat::OpenAiResponses),
        operation_context: TransportOperationContext::AgentModelAttempt {
            model_step_id,
            model_execution_snapshot_id,
            context_snapshot_id,
            prompt_bundle_snapshot_id,
        },
        tool_registry_snapshot_ref: Some(tool_registry_snapshot_ref),
        tool_name_bindings: vec![tool_name_binding],
        historical_tool_call_wire_bindings: Vec::new(),
        canonical_body_artifact_id: None,
    };
    provider_request_evidence.validate()?;
    let credential_binding_fingerprint =
        CredentialBindingFingerprint(ContentHash::sha256(b"canonical-credential-binding"));
    let configured_credential_status = EndpointCredentialStatus {
        endpoint_id: model_endpoint_id,
        slot_name: ModelCredentialSlot::ApiKey,
        credential_id,
        state: EndpointCredentialState::Configured,
        active_credential_revision: Some(credential_revision),
        last_committed_credential_revision: Some(credential_revision),
        row_revision: Revision::new(7),
        error_code: None,
    };
    configured_credential_status.validate()?;
    let credential_configured_event = ModelEndpointCredentialConfiguredPayload {
        endpoint_id: model_endpoint_id,
        credential_id,
        active_credential_revision: credential_revision,
        row_revision: Revision::new(7),
        credential_binding_fingerprint: credential_binding_fingerprint.clone(),
        models_revision: Revision::new(3),
    };
    let credential_cleared_event = ModelEndpointCredentialClearedPayload {
        endpoint_id: model_endpoint_id,
        credential_id,
        last_committed_credential_revision: credential_revision,
        row_revision: Revision::new(8),
        previous_credential_binding_fingerprint: credential_binding_fingerprint.clone(),
        models_revision: Revision::new(3),
    };

    let model_registry = ModelRegistryConfig {
        provider_order: vec![model_provider_id],
        providers: BTreeMap::from([(
            model_provider_id,
            ModelProviderConfig {
                id: model_provider_id,
                display_name: "OpenAI Contract Provider".to_owned(),
                slug: ProviderSlug::new("openai-contract")?,
                source: ModelProviderSource::ModelsDev { catalog_provider_id: "openai".to_owned() },
                endpoint_order: vec![model_endpoint_id],
                created_at: model_created_at,
                updated_at: model_updated_at,
                default_endpoint_id: None,
                backup_endpoint_ids: vec![],
            },
        )]),
        endpoints: BTreeMap::from([(
            model_endpoint_id,
            ModelEndpointConfig {
                id: model_endpoint_id,
                revision: Revision::INITIAL,
                provider_id: model_provider_id,
                display_name: "Responses API".to_owned(),
                slug: EndpointSlug::new("responses")?,
                api_format: ModelApiFormat::OpenAiResponses,
                base_url: "https://api.openai.example/v1".to_owned(),
                discovery_mode: ModelDiscoveryMode::Automatic,
                chat_output_token_field: None,
                chat_instruction_role: None,
                auth_scheme: EndpointAuthScheme::OpenAiBearerApiKey,
                openai_organization_id: Some("org_contract".to_owned()),
                openai_project_id: Some("proj_contract".to_owned()),
                anthropic_workspace_id: None,
                timeouts: ModelEndpointTimeouts {
                    connect_ms: 10_000,
                    first_event_ms: 30_000,
                    stream_idle_ms: 60_000,
                    total_ms: Some(300_000),
                },
                max_parallel_model_requests: 2,
                model_order: vec![model_profile_id],
                compatibility: Some(EndpointCompatibilityOverrides {
                    endpoint_fingerprint: endpoint_fingerprint.clone(),
                    unsupported_parameters: BTreeSet::new(),
                    unsupported_features: BTreeSet::from(["streaming_unsupported".to_owned()]),
                    output_token_field_aliases: Vec::new(),
                    learned_at: Some(model_updated_at),
                }),
                created_at: model_created_at,
                updated_at: model_updated_at,
            },
        )]),
        models: BTreeMap::from([(
            model_profile_id,
            ModelProfileConfig {
                id: model_profile_id,
                revision: Revision::INITIAL,
                endpoint_id: model_endpoint_id,
                provider_model_name: "gpt-contract".to_owned(),
                display_name: "GPT Contract".to_owned(),
                notes: None,
                origin: ModelProfileOrigin::Discovered,
                enabled: true,
                available_for_subagents: false,
                catalog: Some(ModelCatalogMetadata {
                    catalog_provider_id: "openai".to_owned(),
                    catalog_model_id: "gpt-contract".to_owned(),
                    catalog_revision: Revision::INITIAL,
                    catalog_content_hash: endpoint_fingerprint.clone(),
                    catalog_object_hash: endpoint_fingerprint.clone(),
                    snapshot: ProviderCatalogModelEntry {
                        catalog_model_id: "gpt-contract".to_owned(),
                        display_name: "GPT Contract".to_owned(),
                        family: Some("gpt".to_owned()),
                        description: Some("Contract fixture model".to_owned()),
                        aliases: BTreeSet::new(),
                        context_window_tokens: Some(128_000),
                        input_limit_tokens: None,
                        output_limit_tokens: Some(4_096),
                        input_modalities: BTreeSet::from(["text".to_owned()]),
                        output_modalities: BTreeSet::from(["text".to_owned()]),
                        capabilities: ProviderCatalogModelCapabilities {
                            attachment: false,
                            reasoning: false,
                            tool_call: true,
                            structured_output: true,
                            temperature: true,
                            interleaved: None,
                        },
                        reasoning_options: BTreeSet::new(),
                        knowledge_cutoff: None,
                        release_date: None,
                        last_updated: None,
                        open_weights: Some(false),
                        status: None,
                        price: None,
                    },
                    matched_at: Some(model_updated_at),
                    match_state: CatalogMatchState::Matched,
                }),
                context: ModelContextWindowConfig {
                    catalog_window_tokens: Some(128_000),
                    user_override_window_tokens: None,
                    user_override_revision: 0,
                    runtime_corrected_window_tokens: None,
                    runtime_correction_basis: None,
                    runtime_effective_window_tokens: 128_000,
                    historical_snapshot_max_tokens: 1_024,
                },
                request_defaults: ModelRequestDefaults {
                    max_output_tokens: Some(4_096),
                    temperature: Some(0.2),
                    top_p: None,
                    chat_output_token_field: None,
                    chat_instruction_role: None,
                    reasoning_effort: None,
                    reasoning_efforts: None,
                    image_input: None,
                },
                created_at: model_created_at,
                updated_at: model_updated_at,
            },
        )]),
    };
    let endpoint_projection = EndpointDetailProjection {
        endpoint_id: model_endpoint_id,
        models_revision: Revision::new(3),
        provider: model_registry.providers[&model_provider_id].clone(),
        endpoint: model_registry.endpoints[&model_endpoint_id].clone(),
        authentication: EndpointAuthenticationProjection {
            auth_kind: EndpointAuthenticationKind::Secret,
            active_credential: Some(configured_credential_status.clone()),
            dormant_credential: None,
        },
        models: vec![model_registry.models[&model_profile_id].clone()],
        latest_discovery_run: None,
    };
    let model_execution_snapshot = ModelExecutionSnapshot {
        id: model_execution_snapshot_id,
        model_step_id,
        provider_id: model_provider_id,
        endpoint_id: model_endpoint_id,
        model_profile_id,
        provider_display_name: "OpenAI Contract Provider".to_owned(),
        endpoint_display_name: "Responses API".to_owned(),
        model_name: "gpt-contract".to_owned(),
        readable_ref: "OpenAI Contract Provider / Responses API / gpt-contract".to_owned(),
        api_format: ModelApiFormat::OpenAiResponses,
        chat_output_token_field: None,
        chat_instruction_role: None,
        auth_scheme: EndpointAuthScheme::OpenAiBearerApiKey,
        base_url: "https://api.openai.example/v1".to_owned(),
        resolved_request_url: "https://api.openai.example/v1/responses".to_owned(),
        openai_organization_id: Some("org_contract".to_owned()),
        openai_project_id: Some("proj_contract".to_owned()),
        anthropic_workspace_id: None,
        endpoint_config_revision: 2,
        model_config_revision: 3,
        runtime_effective_window_tokens: 128_000,
        historical_snapshot_max_tokens: 1_024,
        request_options: ResolvedModelRequestOptions {
            effective_max_output_tokens: 4_096,
            temperature: Some(0.2),
            top_p: None,
            reasoning_effort: None,
            reasoning_by_effort: false,
            image_input: false,
        },
        historical_tool_call_wire_bindings: Vec::new(),
        credential_id: Some(credential_id),
        credential_revision: Some(credential_revision),
        credential_binding_fingerprint: Some(credential_binding_fingerprint.clone()),
        context_snapshot_id,
        prompt_bundle_snapshot_id,
        endpoint_fingerprint: endpoint_fingerprint.clone(),
        created_at: model_updated_at,
    };
    let raw_stream_artifact_id = ArtifactId::from_str(&parse_third_id("art"))?;
    let mut attempt_request_evidence = provider_request_evidence.clone();
    attempt_request_evidence.compatibility_removals = BTreeSet::new();
    attempt_request_evidence.canonical_body_artifact_id = None;
    let transport_attempt = TransportAttemptRecord {
        id: transport_attempt_id,
        attempt_number: 2,
        attempt_kind: TransportAttemptKind::TransparentRetry,
        client_request_id: "net_01ARZ3NDEKTSV4RRFFQ69G5FAV".to_owned(),
        provider_request_id: Some("resp_Contract_ExactCase".to_owned()),
        endpoint_id: model_endpoint_id,
        endpoint_fingerprint: endpoint_fingerprint.clone(),
        request_evidence: attempt_request_evidence,
        state: TransportAttemptState::Completed,
        started_at: model_updated_at,
        first_byte_at: Some(UtcTimestamp::from_str("2026-08-29T00:00:06Z")?),
        first_model_event_at: Some(UtcTimestamp::from_str("2026-08-29T00:00:07Z")?),
        finished_at: Some(UtcTimestamp::from_str("2026-08-29T00:00:08Z")?),
        raw_stream_artifact_id: Some(raw_stream_artifact_id),
        failure_id: None,
    };
    let model_stream_event = ModelStreamEventEnvelope {
        model_step_id,
        transport_attempt_id,
        sequence: 3,
        received_at: UtcTimestamp::from_str("2026-08-29T00:00:07Z")?,
        event: ModelStreamEvent::ToolCallStarted {
            provider_call_id: provider_tool_call_id.clone(),
            tool_name: "workspace.read".to_owned(),
            position: 0,
        },
    };
    let normalized_model_output = NormalizedModelOutput {
        model_step_id,
        assistant_blocks: vec![AssistantContentBlock {
            block_id: ModelContentBlockId::from_str(&parse_id("mcb"))?,
            text: "I will inspect the requested file.".to_owned(),
            provider_reference: Some(ProviderProtocolReference {
                item_id: Some("msg_Contract_ExactCase".to_owned()),
                block_id: None,
                index: Some(0),
            }),
        }],
        reasoning_blocks: vec![ReasoningContentBlock {
            block_id: ModelContentBlockId::from_str(&parse_alt_id("mcb"))?,
            text: "Inspect the file before answering.".to_owned(),
            summary: Some("File inspection required.".to_owned()),
            provider_reference: Some(ProviderProtocolReference {
                item_id: Some("rs_Contract_ExactCase".to_owned()),
                block_id: None,
                index: Some(0),
            }),
        }],
        tool_calls: vec![NormalizedToolCall {
            tool_call_id: ToolCallId::from_str(&parse_id("tcl"))?,
            provider_call_id: provider_tool_call_id.clone(),
            tool_name: "workspace.read".to_owned(),
            arguments_json: r#"{"path":"README.md"}"#.to_owned(),
            position: 0,
        }],
        refusal: None,
        stop_reason: NormalizedStopReason::ToolCalls,
        usage: ModelUsage {
            input_tokens: 120,
            output_tokens: 24,
            total_tokens: 144,
            reasoning_tokens: Some(8),
            cached_input_tokens: Some(32),
            cache_write_tokens: None,
            source: UsageSource::ProviderReported,
        },
        provider_continuation: Some(ProviderContinuationState::OpenAiResponses {
            state_ref: ArtifactId::from_str(&parse_alt_id("art"))?,
        }),
        provider_request_id: Some("resp_Contract_ExactCase".to_owned()),
        raw_response_ref: Some(raw_stream_artifact_id),
    };
    let normalized_model_failure = NormalizedModelFailure {
        category: ModelFailureCategory::ContextWindowExceeded,
        code: "context_length_exceeded".to_owned(),
        message: "The canonical request exceeds the model context window.".to_owned(),
        retry_class: ModelRetryClass::NotRetryable,
        provider_status: Some(400),
        provider_request_id: Some("resp_Failure_ExactCase".to_owned()),
        provider_error_ref: Some(ArtifactId::from_str(&parse_third_id("art"))?),
        partial_output_ref: None,
        transport_attempt_id: Some(transport_attempt_id),
    };
    let persisted_model_failure = PersistedModelFailure {
        id: ModelFailureId::from_str(&parse_id("mfl"))?,
        fingerprint: FailureFingerprint(ContentHash::blake3(b"canonical-context-window-failure")),
        failure: normalized_model_failure.clone(),
        created_at: UtcTimestamp::from_str("2026-08-29T00:00:09Z")?,
    };
    let models = ModelContractFixture {
        tool_versions: vec![declared_tool_version, unversioned_tool_version],
        credential_revision,
        logical_request: Some(logical_request),
        provider_request_evidence: Some(provider_request_evidence),
        credential_configured_event: Some(credential_configured_event),
        credential_cleared_event: Some(credential_cleared_event),
        configure_credential_request: Some(ModelEndpointConfigureCredentialRequest {
            expected_models_revision: ExpectedRevision::new(3),
            endpoint_id: model_endpoint_id,
            credential_slot: ModelCredentialSlot::ApiKey,
            credential_id,
            expected_row_revision: ExpectedRevision::new(7),
        }),
        clear_credential_request: Some(ModelEndpointClearCredentialRequest {
            expected_models_revision: ExpectedRevision::new(3),
            endpoint_id: model_endpoint_id,
            credential_slot: ModelCredentialSlot::ApiKey,
            credential_id,
            expected_row_revision: ExpectedRevision::new(7),
        }),
        credential_mutation_result: Some(EndpointCredentialMutationResult {
            endpoint_id: model_endpoint_id,
            changed: true,
            models_revision: Revision::new(3),
            credential_status: configured_credential_status,
        }),
        dormant_credential_summary: Some(DormantEndpointCredentialSummary {
            endpoint_id: model_endpoint_id,
            slot_name: ModelCredentialSlot::ApiKey,
            credential_id,
            retained_state: EndpointCredentialState::Configured,
            last_committed_credential_revision: Some(credential_revision),
            row_revision: Revision::new(8),
            error_code: None,
        }),
        provider_tool_call_id: Some(provider_tool_call_id),
        provider_tool_call_protocol_binding: None,
        registry: Some(model_registry),
        execution_snapshot: Some(model_execution_snapshot),
        transport_attempt: Some(transport_attempt),
        stream_event: Some(model_stream_event),
        output: Some(normalized_model_output),
        failure: Some(normalized_model_failure),
        persisted_failure: Some(persisted_model_failure),
        runtime_capabilities: Some(ModelRuntimeCapabilityFacts {
            api_format: ModelApiFormat::OpenAiResponses,
            native_tool_channel_exposed: true,
            native_attachment_parts_present: false,
            reasoning_channel_observed: true,
            native_capability_names: vec!["function_call".to_owned()],
        }),
        settings_get_snapshot_request: None,
        catalog_get_request: None,
        catalog_ensure_fresh_request: None,
        catalog_refresh_request: None,
        catalog_provider_get_request: None,
        catalog_provider_get_result: None,
        provider_icon_read_request: None,
        provider_icon_read_result: None,
        provider_create_request: None,
        provider_rename_request: None,
        provider_delete_request: None,
        provider_mutation_result: None,
        provider_bulk_availability_result: None,
        endpoint_create_request: None,
        endpoint_create_with_credential_request: None,
        endpoint_create_with_credential_result: Some(EndpointCreatedWithCredentialResult {
            models_revision: Revision::new(3),
            endpoint_projection,
        }),
        endpoint_update_request: None,
        endpoint_delete_request: None,
        endpoint_replace_credential_request: None,
        discovery_start_request: None,
        discovery_start_accepted: None,
        discovery_cancel_request: None,
        discovery_cancel_result: None,
        endpoint_mutation_result: None,
        availability_test_start_request: None,
        availability_test_start_accepted: None,
        availability_test_cancel_request: None,
        availability_test_cancel_result: None,
        availability_prerequisite_failure: None,
        profile_create_manual_request: None,
        profile_create_from_catalog_request: None,
        profile_apply_catalog_update_request: None,
        profile_update_request: None,
        profile_delete_request: None,
        profile_set_subagent_availability_request: None,
        profile_mutation_result: None,
        provider_set_all_current_models_subagent_availability_request: None,
        deletion_mutation_result: Some(ModelDeletionMutationResult {
            changed: true,
            revision: Revision::new(4),
            value: DeleteModelConfigCommitResult {
                deleted: DeleteModelConfigResult {
                    deleted_provider_ids: Vec::new(),
                    deleted_endpoint_ids: vec![model_endpoint_id],
                    deleted_model_profile_ids: vec![model_profile_id],
                },
                default_model_cleared: true,
                credential_cleanup_pending: true,
            },
        }),
        default_set_request: None,
        default_mutation_result: None,
        session_set_binding_request: None,
        session_binding_mutation_result: None,
        context_window_correct_request: None,
        context_window_clear_user_override_request: None,
        context_window_reset_runtime_correction_request: None,
        context_window_mutation_result: None,
        compatibility_get_request: None,
        compatibility_snapshot: None,
        compatibility_reset_request: None,
        compatibility_reset_result: None,
    };
    let package_fixture = staged_package_fixture()?;

    Ok(CanonicalContractFixture {
        contract_version: ContractVersion::CURRENT,
        ids: CommonIdFixture {
            tool_id: ToolId::from_str(&parse_id("tol"))?,
            credential_id,
            browser_share_id: BrowserShareId::from_str(&parse_id("bsh"))?,
            attachment_grant_id: AttachmentGrantId::from_str(&parse_id("atg"))?,
            provider_id: ProviderId::from_str(&parse_id("prv"))?,
            endpoint_id: EndpointId::from_str(&parse_id("end"))?,
            model_profile_id: ModelProfileId::from_str(&parse_id("mdl"))?,
            model_execution_snapshot_id: ModelExecutionSnapshotId::from_str(&parse_id("mex"))?,
            transport_attempt_id: TransportAttemptId::from_str(&parse_id("tat"))?,
            model_content_block_id: ModelContentBlockId::from_str(&parse_id("mcb"))?,
            model_failure_id: ModelFailureId::from_str(&parse_id("mfl"))?,
            context_projection_snapshot_id: ContextProjectionSnapshotId::from_str(&parse_id(
                "cps",
            ))?,
            prompt_bundle_snapshot_id: PromptBundleSnapshotId::from_str(&parse_id("pbs"))?,
            model_discovery_run_id: ModelDiscoveryRunId::from_str(&parse_id("mdr"))?,
            model_availability_test_id: ModelAvailabilityTestId::from_str(&parse_id("mav"))?,
            workspace_id: WorkspaceId::from_str(&parse_id("wsp"))?,
            session_id: SessionId::from_str(&parse_id("ses"))?,
            turn_task_id: TurnTaskId::from_str(&parse_id("trn"))?,
            task_lineage_id,
            agent_run_id: AgentRunId::from_str(&parse_id("agt"))?,
            model_step_id: ModelStepId::from_str(&parse_id("stp"))?,
            tool_call_id: ToolCallId::from_str(&parse_id("tcl"))?,
            tool_instance_id: ToolInstanceId::from_str(&parse_id("tin"))?,
            tool_package_id: ToolPackageId::from_str(&parse_id("tpk"))?,
            tool_draft_id: ToolDraftId::from_str(&parse_id("tdr"))?,
            tool_batch_id: ToolBatchId::from_str(&parse_id("tbt"))?,
            tool_execution_attempt_id: ToolExecutionAttemptId::from_str(&parse_id("tea"))?,
            tool_conformance_run_id: ToolConformanceRunId::from_str(&parse_id("tcr"))?,
            tool_registry_snapshot_id: ToolRegistrySnapshotId::from_str(&parse_id("trs"))?,
            artifact_id: ArtifactId::from_str(&parse_id("art"))?,
            memory_id: MemoryId::from_str(&parse_id("mem"))?,
            skill_instance_id: SkillInstanceId::from_str(&parse_id("skl"))?,
            command_id: CommandId::from_str(&parse_id("cmd"))?,
            command_invocation_id,
            runtime_instance_id,
            release_bundle_id,
            stream_id,
            ipc_channel_id,
            correlation_id,
            causation_id,
            event_id: EventId::from_str(&parse_id("evt"))?,
            file_reference_id: FileReferenceId::from_str(&parse_id("fre"))?,
            external_grant_id: ExternalGrantId::from_str(&parse_id("xgr"))?,
            deferred_mutation_id: DeferredMutationId::from_str(&parse_id("dfm"))?,
            mutation_journal_id: MutationJournalId::from_str(&parse_id("jnl"))?,
            workspace_lifecycle_operation_id: WorkspaceLifecycleOperationId::from_str(&parse_id(
                "wlo",
            ))?,
            editor_reservation_id: EditorReservationId::from_str(&parse_id("ers"))?,
            renderer_instance_id: RendererInstanceId::from_str(&parse_id("rnd"))?,
            delete_confirmation_id: DeleteConfirmationId::from_str(&parse_id("dcn"))?,
        },
        revision: Revision::new(42),
        expected_revision: ExpectedRevision::new(41),
        content_hash: ContentHash::new(ContentHashAlgorithm::Blake3, "ab".repeat(32))?,
        timestamp: UtcTimestamp::from_str("2026-08-28T00:00:00Z")?,
        error,
        component_manifest,
        projection_contract: projection_samples::canonical_projection_contract(&runtime),
        runtime,
        persistence,
        workspace_files,
        security,
        models,
        context,
        tools: tool_contract::fixture(package_fixture)?,
        windows_path: r"C:\Users\测试用户\Aura Workspace\src\main.rs".to_owned(),
    })
}

pub fn canonical_schema_value() -> Result<Value> {
    let mut generator = schemars::SchemaGenerator::default();
    generator.subschema_for::<aura_security_contract::AuthorizationGrant>();
    generator.subschema_for::<V2KernelFixture>();
    agent_contract::register(&mut generator);
    generator.subschema_for::<aura_persistence_core::import_report::ImportReport>();
    generator.subschema_for::<aura_projection_contract::RuntimeReadiness>();
    generator.subschema_for::<aura_persistence_core::operation::OperationState>();
    generator.subschema_for::<aura_security_contract::AuthorizationDecision>();
    generator.subschema_for::<aura_security_contract::AuthorizationApproval>();
    generator.subschema_for::<aura_security_contract::PolicyAuthorization>();
    generator.subschema_for::<aura_execution_contract::DispatchRequest>();
    generator.subschema_for::<aura_execution_contract::CancellationState>();
    generator.subschema_for::<aura_execution_contract::ExecutionAttempt>();
    generator.subschema_for::<aura_execution_contract::PrepareRequest>();
    generator.subschema_for::<aura_execution_contract::BrokerEvidence>();
    generator.subschema_for::<aura_windows_user_contract::ManagedShellCommand>();
    generator.subschema_for::<aura_windows_user_contract::WindowsUserPreparation>();
    generator.subschema_for::<aura_windows_user_contract::CoreRunnerMessage>();
    generator.subschema_for::<aura_windows_user_contract::RunnerCoreMessage>();
    generator.subschema_for::<aura_windows_user_contract::InstallationManifest>();
    generator.subschema_for::<aura_windows_user_contract::ExecutionHostAcceptanceReport>();
    generator.subschema_for::<aura_windows_user_contract::MaintenanceConfig>();
    generator.subschema_for::<aura_windows_user_contract::MaintenanceJournal>();
    tool_family_contracts::register(&mut generator);
    generator.subschema_for::<aura_tool_contract::ToolBrokerInvokeRequest>();
    generator.subschema_for::<aura_tool_contract::ToolBrokerInvokeResult>();
    generator.subschema_for::<aura_context_domain::ContextBrokerReadRequest>();
    generator.subschema_for::<aura_context_domain::ContextBrokerReadResult>();
    generator.subschema_for::<aura_permission_domain::BrokerHttpReadRequest>();
    generator.subschema_for::<aura_permission_domain::BrokerHttpReadResult>();
    generator.subschema_for::<aura_execution_contract::CancelReceipt>();
    generator.subschema_for::<aura_execution_contract::ReconcileResult>();
    generator.subschema_for::<aura_execution_contract::BackendError>();
    generator.subschema_for::<aura_security_contract::SecuritySourceRef>();
    generator.subschema_for::<aura_security_contract::ResourceDescriptor>();

    generator.subschema_for::<aura_config::recovery_contract::RecoveryNotice>();
    generator.subschema_for::<aura_config::recovery_contract::RecoveryReport>();
    generator.subschema_for::<aura_config::recovery_contract::ConfigRecoveryPlanRequest>();
    generator.subschema_for::<aura_config::recovery_contract::ConfigRecoveryApplyRequest>();
    generator.subschema_for::<aura_config::recovery::BundleRecoveryPlan>();
    generator.subschema_for::<aura_tool_domain::ToolCredentialMutationResultV1>();
    generator.subschema_for::<aura_tool_domain::ToolCredentialOutcomeRequestV1>();
    generator.subschema_for::<aura_tool_domain::ToolCredentialOutcomeV1>();
    generator.subschema_for::<aura_tool_domain::ToolCredentialListResultV1>();
    generator.subschema_for::<aura_tool_domain::ToolCredentialManagementTargetV1>();
    generator.subschema_for::<aura_tool_domain::ToolCredentialChangeRequestV1>();
    generator.subschema_for::<aura_tool_domain::ToolCredentialCreateRequestV1>();
    let mut value =
        serde_json::to_value(generator.into_root_schema_for::<CanonicalContractFixture>())?;
    let object = value.as_object_mut().context("canonical schema root must be an object")?;
    object.insert(
        "$id".to_owned(),
        Value::String(
            "https://schemas.aura.local/contracts/v1/aura-contracts.schema.json".to_owned(),
        ),
    );
    object.insert("title".to_owned(), Value::String("AuraContractCatalog".to_owned()));
    Ok(value)
}

pub fn write_rust_generated_artifacts(root: &Path) -> Result<Vec<PathBuf>> {
    let schema_bytes = pretty_json(&canonical_schema_value()?)?;
    let tool_package_manifest_schema_bytes = pretty_json(&tool_package_manifest_schema_value()?)?;
    let tool_package_canonical_manifest_schema_bytes =
        pretty_json(&canonical_tool_package_manifest_schema_value()?)?;
    let valid_tool_package_manifest_fixtures = valid_tool_package_manifest_fixtures()?;
    let invalid_tool_package_manifest_fixtures = invalid_tool_package_manifest_fixtures()?;
    let semantic_invalid_tool_package_manifest_fixtures =
        semantic_invalid_tool_package_manifest_fixtures()?;
    let invalid_canonical_tool_package_manifest_fixtures =
        invalid_canonical_tool_package_manifest_fixtures()?;
    let semantic_invalid_canonical_tool_package_manifest_fixtures =
        semantic_invalid_canonical_tool_package_manifest_fixtures()?;
    let additional_canonical_tool_package_manifest_fixtures =
        additional_canonical_tool_package_manifest_fixtures()?;
    let canonical_tool_package_manifest_fixture = canonical_tool_package_manifest_fixture()?;
    let canonical_tool_package_manifest_fixture_bytes =
        pretty_fixture_json(&canonical_tool_package_manifest_fixture)?;
    let package_runtime_closure_fixture_bytes =
        pretty_fixture_json(&package_runtime_closure_fixture()?)?;
    let fixture = canonical_fixture()?;
    let fixture_bytes = pretty_json(&fixture)?;
    let component_manifest_bytes = pretty_json(&fixture.component_manifest)?;
    let tool_error_catalog = tool_error_catalog_v1();
    aura_tool_contract::validate_tool_error_catalog(&tool_error_catalog)
        .map_err(anyhow::Error::msg)?;
    let tool_error_catalog_bytes = pretty_json(&tool_error_catalog)?;
    let tool_error_schema_bytes = pretty_json(&tool_error_schema_value()?)?;
    let tool_error_typescript_bytes = tool_error_typescript(&tool_error_catalog).into_bytes();
    let tool_error_markdown_bytes = tool_error_markdown(&tool_error_catalog).into_bytes();
    let id_prefix_registry = id_prefix_registry_v1()?;
    let id_prefix_registry_bytes = pretty_json(&id_prefix_registry)?;
    let id_prefix_typescript_bytes = id_prefix_typescript(&id_prefix_registry).into_bytes();
    let builtin_tool_definition_set_bytes =
        aura_tool_registry::generated_builtin_tool_definition_set_source_v1()
            .map_err(anyhow::Error::msg)?;
    // The agent release is generated from the **same** Rust-built definition set, not by reading the
    // artifact back off disk. Reading it back made the generator depend on a file the generator
    // produces, so on the very change that adds a definition the stale file made generation fail
    // before it could be corrected. The build is from the identical source the bytes above come
    // from, so the produced manifest is byte-for-byte what it always was.
    let builtin_definition_set =
        aura_tool_registry::built_definition_set_v1().map_err(anyhow::Error::msg)?;
    let agent_release_bytes = pretty_json(
        &aura_tool_registry::agent_release::generate_manifest_from_entries(&builtin_definition_set)
            .map_err(anyhow::Error::msg)?,
    )?;

    let manifest = SchemaManifest {
        manifest_version: ContractVersion::CURRENT,
        schemas: vec![
            SchemaManifestEntry {
                id: "aura.contracts",
                path: "aura-contracts.schema.json",
                content_hash: ContentHash::blake3(&schema_bytes),
            },
            SchemaManifestEntry {
                id: "aura.tool-error-catalog.v1",
                path: "tool-error-catalog.v1.schema.json",
                content_hash: ContentHash::blake3(&tool_error_schema_bytes),
            },
            SchemaManifestEntry {
                id: "aura.tool-package-manifest.v1",
                path: "aura-tool-package-manifest.v1.schema.json",
                content_hash: ContentHash::blake3(&tool_package_manifest_schema_bytes),
            },
            SchemaManifestEntry {
                id: "aura.tool-package-canonical-manifest.v1",
                path: "aura-tool-package-canonical-manifest.v1.schema.json",
                content_hash: ContentHash::blake3(&tool_package_canonical_manifest_schema_bytes),
            },
        ],
    };
    let manifest_bytes = pretty_json(&manifest)?;

    let mut artifacts = BTreeMap::from([
        (terminal_opening_fixture::PATH, terminal_opening_fixture::bytes()?),
        projection_samples::generated_artifact()?,
        ("fixtures/contracts/v2-kernel.json", pretty_json(&v2_kernel_fixture()?)?),
        ("fixtures/contracts/v2-import-report.json", pretty_json(&v2_import_report_fixture())?),
        (
            "resources/ipc/routes.v1.json",
            pretty_json(&aura_ipc::routes::registry().collect::<Vec<_>>())?,
        ),
        (SCHEMA_PATH, schema_bytes),
        (SCHEMA_MANIFEST_PATH, manifest_bytes),
        (CONTRACT_FIXTURE_PATH, fixture_bytes),
        (COMPONENT_MANIFEST_EXAMPLE_PATH, component_manifest_bytes),
        (TOOL_ERROR_CATALOG_PATH, tool_error_catalog_bytes.clone()),
        (TOOL_ERROR_SCHEMA_PATH, tool_error_schema_bytes),
        (TOOL_ERROR_FIXTURE_PATH, tool_error_catalog_bytes),
        (TOOL_ERROR_TYPESCRIPT_PATH, tool_error_typescript_bytes),
        (TOOL_ERROR_MARKDOWN_PATH, tool_error_markdown_bytes),
        (ID_PREFIX_REGISTRY_PATH, id_prefix_registry_bytes),
        (ID_PREFIX_TYPESCRIPT_PATH, id_prefix_typescript_bytes),
        (BUILTIN_TOOL_DEFINITION_SET_PATH, builtin_tool_definition_set_bytes),
        ("resources/tools/builtin-agent-release.v1.json", agent_release_bytes),
        (TOOL_PACKAGE_MANIFEST_SCHEMA_PATH, tool_package_manifest_schema_bytes),
        (TOOL_PACKAGE_CANONICAL_MANIFEST_SCHEMA_PATH, tool_package_canonical_manifest_schema_bytes),
        (TOOL_PACKAGE_CANONICAL_FIXTURE_PATH, canonical_tool_package_manifest_fixture_bytes),
        (TOOL_PACKAGE_RUNTIME_CLOSURE_FIXTURE_PATH, package_runtime_closure_fixture_bytes),
    ]);
    artifacts.extend(git_fixtures::outputs()?);
    artifacts.insert(package_contract::PATH, package_contract::bytes()?);
    for (path, fixture) in valid_tool_package_manifest_fixtures
        .into_iter()
        .chain(invalid_tool_package_manifest_fixtures)
        .chain(semantic_invalid_tool_package_manifest_fixtures)
        .chain(invalid_canonical_tool_package_manifest_fixtures)
        .chain(semantic_invalid_canonical_tool_package_manifest_fixtures)
        .chain(additional_canonical_tool_package_manifest_fixtures)
    {
        artifacts.insert(path, pretty_fixture_json(&fixture)?);
    }

    let mut written = Vec::with_capacity(artifacts.len());
    for (relative_path, bytes) in artifacts {
        let path = root.join(relative_path);
        write_if_changed(&path, &bytes)?;
        written.push(path);
    }

    Ok(written)
}

fn id_prefix_registry_v1() -> Result<IdPrefixRegistryV1> {
    let mut registered = BTreeMap::<&str, &str>::new();
    for (owner_crate, prefixes) in
        [("aura-domain-common", COMMON_ID_PREFIXES), ("aura-domain-common", TOOL_ID_PREFIXES)]
    {
        for prefix in prefixes {
            if prefix.len() != 3 || !prefix.bytes().all(|byte| byte.is_ascii_lowercase()) {
                anyhow::bail!("invalid typed ID prefix '{prefix}' registered by {owner_crate}");
            }
            if let Some(existing_owner) = registered.insert(prefix, owner_crate) {
                anyhow::bail!(
                    "typed ID prefix collision for '{prefix}': {existing_owner} and {owner_crate}"
                );
            }
        }
    }
    Ok(IdPrefixRegistryV1 {
        registry_version: 1,
        entries: registered
            .into_iter()
            .map(|(prefix, owner_crate)| IdPrefixRegistryEntryV1 {
                prefix: prefix.to_owned(),
                owner_crate: owner_crate.to_owned(),
                type_name: aura_domain_common::COMMON_ID_TYPES
                    .iter()
                    .chain(aura_domain_common::TOOL_ID_TYPES.iter())
                    .find(|(_, p)| *p == prefix)
                    .expect("registered typed prefix")
                    .0
                    .to_owned(),
            })
            .collect(),
    })
}

fn id_prefix_typescript(registry: &IdPrefixRegistryV1) -> String {
    let prefixes = registry
        .entries
        .iter()
        .map(|entry| {
            serde_json::to_string(&entry.prefix).expect("typed ID prefix is JSON serializable")
        })
        .collect::<Vec<_>>()
        .join(",\n  ");
    format!(
        "// Generated by aura-contract-schema. Do not edit.\n\n\
         export const AURA_ID_PREFIXES_V1 = [\n  {prefixes}\n] as const;\n\n\
         export type AuraIdPrefixV1 = (typeof AURA_ID_PREFIXES_V1)[number];\n"
    )
}

fn tool_error_schema_value() -> Result<Value> {
    let mut value = serde_json::to_value(schema_for!(ToolErrorCatalogV1))?;
    let catalog = tool_error_catalog_v1();
    let object = value.as_object_mut().context("tool error schema root must be an object")?;
    object.insert(
        "$id".to_owned(),
        Value::String(
            "https://schemas.aura.local/contracts/v1/tool-error-catalog.v1.schema.json".to_owned(),
        ),
    );
    object.insert("title".to_owned(), Value::String("AuraToolErrorCatalogV1".to_owned()));
    let definitions = object
        .get_mut("$defs")
        .and_then(Value::as_object_mut)
        .context("tool error schema definitions must be an object")?;
    definitions.insert(
        "ErrorCode".to_owned(),
        json!({
            "type": "string",
            "enum": catalog
                .entries
                .iter()
                .map(|entry| entry.code.as_str())
                .collect::<Vec<_>>()
        }),
    );
    object.insert(
        "properties".to_owned(),
        json!({
            "catalog_version": { "const": 1, "type": "integer" },
            "entries": {
                "type": "array",
                "items": { "$ref": "#/$defs/ToolErrorCatalogEntryV1" },
                "minItems": catalog.entries.len(),
                "maxItems": catalog.entries.len()
            }
        }),
    );
    Ok(value)
}

fn tool_error_typescript(catalog: &ToolErrorCatalogV1) -> String {
    let codes = catalog
        .entries
        .iter()
        .map(|entry| {
            serde_json::to_string(entry.code.as_str()).expect("ErrorCode is JSON serializable")
        })
        .collect::<Vec<_>>()
        .join(",\n  ");
    let catalog_json = serde_json::to_string_pretty(catalog)
        .expect("closed tool error catalog is JSON serializable");
    format!(
        "// Generated by aura-contract-schema. Do not edit.\n\n\
         export const TOOL_ERROR_CODES_V1 = [\n  {codes}\n] as const;\n\n\
         export type ToolErrorCodeV1 = (typeof TOOL_ERROR_CODES_V1)[number];\n\n\
         export const TOOL_ERROR_CATALOG_V1 = {catalog_json} as const;\n"
    )
}

fn tool_error_markdown(catalog: &ToolErrorCatalogV1) -> String {
    let mut output = String::from(
        "<!-- Generated by aura-contract-schema. Do not edit. -->\n\n\
         # P07 Tool Error Catalog V1\n\n\
         | Code | Category | Visibility | Retry | Safe message key | Suggested NotExecuted |\n\
         | --- | --- | --- | --- | --- | --- |\n",
    );
    for entry in &catalog.entries {
        let category = serde_json::to_value(entry.category)
            .ok()
            .and_then(|value| value.as_str().map(ToOwned::to_owned))
            .unwrap_or_else(|| "invalid".to_owned());
        let visibility = serde_json::to_value(entry.visibility)
            .ok()
            .and_then(|value| value.as_str().map(ToOwned::to_owned))
            .unwrap_or_else(|| "invalid".to_owned());
        let retry = serde_json::to_value(entry.retry_class)
            .ok()
            .and_then(|value| value.as_str().map(ToOwned::to_owned))
            .unwrap_or_else(|| "invalid".to_owned());
        let suggested = entry
            .suggested_not_executed_reason
            .and_then(|value| serde_json::to_value(value).ok())
            .and_then(|value| value.as_str().map(ToOwned::to_owned))
            .unwrap_or_default();
        output.push_str(&format!(
            "| `{}` | `{category}` | `{visibility}` | `{retry}` | `{}` | `{suggested}` |\n",
            entry.code.as_str(),
            entry.safe_message_key
        ));
    }
    output
}

fn pretty_json(value: &impl Serialize) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

// Prettier keeps short arrays of JSON primitives on one line. Golden fixtures are
// checked by both the Rust generator and the repository-wide Prettier gate, so the
// generator must emit that stable representation directly.
fn write_if_changed(path: &Path, bytes: &[u8]) -> Result<()> {
    if fs::read(path).is_ok_and(|current| current == bytes) {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(path, bytes).with_context(|| format!("failed to write {}", path.display()))
}
