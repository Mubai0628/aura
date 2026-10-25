#![forbid(unsafe_code)]
mod broker;
pub use broker::*;

mod activity;
mod agent_configuration;
pub use agent_configuration::*;
mod batch;
mod builtins;
mod definition;
mod entry;
pub use entry::{
    AgentToolCallInput, AgentToolCallResult, TOOLS_CALL_NAME, entry_callable, entry_target,
    expand_entry_calls, is_entry_call,
};
mod errors;
mod network_proxy;
pub use network_proxy::{ToolNetworkProxyErrorV1, ToolNetworkProxyProtocolV1, ToolNetworkProxyV1};
mod events;
mod model_view;
mod package;
mod registry;
mod resource;
mod result;
mod runtime;
mod schema;
mod version;

pub use activity::{
    DEFAULT_TOOL_ACTIVITY_PAGE_SIZE_V1, MAX_TOOL_ACTIVITY_PAGE_SIZE_V1,
    ToolActivityContractErrorV1, ToolActivityCursorBindingV1, ToolActivityCursorKindV1,
    ToolActivityCursorV1, ToolActivityPageRequestV1,
};
pub use batch::{
    CoreInvocationIdentityV1, MAX_TOOL_EXECUTION_ATTEMPTS, MAX_TOOL_PURPOSE_BYTES,
    NormalizedToolCallInputDiagnosticV1, NormalizedToolCallInputIssueV1,
    NormalizedToolCallInputStateV1, NormalizedToolCallV1, ToolBatchContractError,
    ToolBatchRequestV1, ToolBatchResultContractError, ToolBatchResultV1, ToolBatchStatusV1,
    ToolCallStatusV1, ToolExecutionAttemptContractError, ToolExecutionAttemptRecordV1,
    ToolExecutionAttemptStatusV1, ToolExecutionPlanCallV1, ToolExecutionPlanContractError,
    ToolExecutionPlanV1, ToolPurposeTextError, ToolPurposeTextV1,
};
pub use builtins::{
    BuiltinToolCapabilityV1, BuiltinToolDefinitionV1, BuiltinToolScopeRequirementV1,
    TOOLS_DESCRIBE_MODEL_DESCRIPTION_V1, TOOLS_DESCRIBE_MODEL_SUMMARY_V1,
    TOOLS_LIST_LOADED_MODEL_DESCRIPTION_V1, TOOLS_LIST_LOADED_MODEL_SUMMARY_V1,
    TOOLS_LOAD_MODEL_DESCRIPTION_V1, TOOLS_LOAD_MODEL_SUMMARY_V1,
    TOOLS_SEARCH_MODEL_DESCRIPTION_V1, TOOLS_SEARCH_MODEL_SUMMARY_V1,
    TOOLS_UNLOAD_MODEL_DESCRIPTION_V1, TOOLS_UNLOAD_MODEL_SUMMARY_V1, ToolCapabilitySummaryV1,
    ToolDescribeResultV1, ToolListLoadedRequestV1, ToolListLoadedResultV1, ToolLoadedEntryV1,
    ToolResourceClaimSummaryV1, ToolSafeIssueViewV1,
};
pub use definition::{
    EffectiveToolBindingSnapshot, NormalizedMcpToolContractFingerprintV1,
    NormalizedPluginToolContractFingerprintV1, SignedBuiltinToolContractFingerprintV1,
    ToolAudienceV1, ToolCanonicalName, ToolDefinitionContractError, ToolDefinitionSnapshotRef,
    ToolExposureV1, ToolRegistrySnapshotRef, ToolResultContractV1, ToolSchemaSetSnapshotV1,
    ToolSchemaSnapshotV1, ToolSourceContractFingerprintV1, ToolSourceKindV1,
};
pub use errors::{
    ToolBatchErrorCode, ToolConformanceErrorCode, ToolDraftErrorCode, ToolErrorCatalogEntryV1,
    ToolErrorCatalogV1, ToolErrorRetryClassV1, ToolErrorVisibilityV1, ToolHookErrorCode,
    ToolPackageErrorCode, ToolRegistryErrorCode, ToolResourceErrorCode, ToolResultErrorCode,
    ToolRuntimeErrorCode, ToolSchedulerErrorCode, ToolSchemaErrorCode, tool_error_catalog_v1,
    validate_tool_error_catalog,
};
pub use events::{
    SessionToolPinChangedPayloadV1, ToolBatchAdmissionRejectedPayloadV1,
    ToolBatchAdmittedPayloadV1, ToolEnabledChangedPayloadV1,
    ToolExecutionAttemptCancellationRequestedPayloadV1, ToolExecutionAttemptCancelledPayloadV1,
    ToolExecutionAttemptDispatchPreparedPayloadV1, ToolExecutionAttemptFailedPayloadV1,
    ToolExecutionAttemptIndeterminatePayloadV1, ToolExecutionAttemptStartedPayloadV1,
    ToolExecutionAttemptSucceededPayloadV1, ToolInstanceConfigurationChangedPayloadV1,
    ToolInstanceCredentialBindingChangedPayloadV1, ToolLifecycleEventV1,
    ToolPackageDeletedPayloadV1, ToolPackageInstalledPayloadV1, ToolPackageUpdatedPayloadV1,
    ToolRegistrySnapshotCreatedPayloadV1, ToolResultFinalizedPayloadV1, ToolTrustChangedPayloadV1,
    WorkspaceToolPinChangedPayloadV1,
};
pub use package::{
    MAX_TOOL_PACKAGE_DECLARED_FILES_V1, PACKAGE_RUNTIME_CLOSURE_POLICY_VERSION_V1,
    PackageFingerprintV1, PackageKey, PackageRuntimeClosureHashV1,
    PackageRuntimeClosurePolicyVersionV1, PackageRuntimeVisibilityPolicyV1,
    PackageSecurityFingerprintV1, TOOL_PACKAGE_IMPORT_POLICY_V1, TOOL_PACKAGE_MANIFEST_NAME,
    ToolContractFingerprintV1, ToolKey, ToolPackageConformanceCaseNameV1, ToolPackageContractError,
    ToolPackageFileEntryV1, ToolPackageFileRoleV1, ToolPackageFileSha256V1,
    ToolPackageImportPolicyV1, ToolPackageTombstoneV1, ToolPackageToolIdentityV1,
    ToolPackageVersionRefV1, ToolTombstoneV1, validate_closed_file_manifest,
};
pub use registry::{
    DEFAULT_MAX_CONCURRENT_TOOL_CALLS, DEFAULT_MAX_DYNAMIC_TOOL_SCHEMAS, DEFAULT_MAX_TOOL_RETRIES,
    DEFAULT_TOOL_TIMEOUT_MS, MAX_MAX_CONCURRENT_TOOL_CALLS, MAX_TOOL_RETRIES,
    MAX_TOOL_SEARCH_PAGE_SIZE, MAX_TOOL_SEARCH_QUERY_BYTES, MAX_TOOL_TIMEOUT_MS,
    MIN_MAX_CONCURRENT_TOOL_CALLS, MIN_TOOL_TIMEOUT_MS, TOOL_SEARCH_ALGORITHM_VERSION_V1,
    ToolDescribeRequestV1, ToolLoadRequestV1, ToolLoadResultV1, ToolLoadStateV1,
    ToolPinResolutionStateV1, ToolPinV1, ToolRegistryCatalogEntryV1, ToolRegistryContractError,
    ToolRegistryScopeV1, ToolRegistrySnapshotV1, ToolSearchCursorV1, ToolSearchHitV1,
    ToolSearchPageV1, ToolSearchQueryV1, ToolSearchRequestV1, ToolSettingsSnapshotV1,
    ToolUnloadRequestV1, ToolUnloadResultV1,
};
pub use resource::{
    ArgumentJsonPointer, ArtifactResourceAccessV1, FileResourceAccessV1, GitRefResourceAccessV1,
    GitSequencerResourceAccessV1, NetworkResourceAccessV1, PackageArtifactSelectorV1,
    PackageAttemptStagingArtifactAccessV1, PackageDeclaredResourceClaimV1,
    PackageExternalFileSelectorV1, PackageGitRefSelectorV1, PackageGitRepositorySelectorV1,
    PackageNetworkTargetSelectorV1, PackagePathScopeV1, PackageProcessSelectorV1,
    PackageResourceClaimKeyV1, PackageTerminalSessionSelectorV1, PackageWorkspacePathSelectorV1,
    PackageWorkspaceRootPathScopeV1, ProcessResourceAccessV1, ReadWriteResourceAccessV1,
    RepositoryResourceAccessV1, ResolvedResourceAccessV1, ResolvedResourceClaimV1,
    ResolvedResourceKindV1, ResourceClaimSelectorError, ResourceClaimSelectorV1, ResourceClaimV1,
    ResourceInvocationContextV1, TerminalResourceAccessV1,
};
pub use result::{
    ModelSafeToolErrorV1, ModelVisibleToolArtifactRefV1, ModelVisibleToolResultEnvelopeV1,
    ModelVisibleToolResultOutcomeV1, ModelVisibleToolSuccessPayloadV1,
    TOOL_SIDE_EFFECT_UNVERIFIED_WARNING_V1, ToolCancellationReasonV1,
    ToolExecutionAttemptOutcomeRefV1, ToolIndeterminateReasonV1, ToolNotExecutedReasonV1,
    ToolResultContractError, ToolResultOutcomeSummaryV1, ToolResultOutcomeV1, ToolResultV1,
    UserAuditAttemptSummaryV1, UserAuditToolResultEnvelopeV1, UserReadableToolArtifactView,
};
pub use runtime::{
    AURA_STDIO_TOOL_CALL_ID_ENVIRONMENT_VARIABLE_V1,
    AURA_STDIO_TOOL_EXECUTION_ATTEMPT_ID_ENVIRONMENT_VARIABLE_V1,
    AURA_TOOL_OUTPUT_ROOT_ENVIRONMENT_VARIABLE_V1, CommandArgumentV1,
    CommandCredentialEnvironmentSlotV1, CommandEnvironmentEntryV1, CommandEnvironmentValueV1,
    CommandFlagV1, CommandLiteralV1, CommandOptionNameV1, CommandWorkingDirectoryV1,
    CredentialSlotKey, DeclarativeCommandV1, DeclarativeHttpBodyV1, DeclarativeHttpMethodV1,
    DeclarativeHttpTargetV1, DeclarativeHttpV1, EnvironmentLiteralValueV1,
    EnvironmentVariableNameV1, FixedHttpsOriginV1, HttpBasePathV1, HttpCredentialSlotV1,
    HttpEndpointSlotKeyV1, HttpHeaderEntryV1, HttpHeaderLiteralValueV1, HttpHeaderNameV1,
    HttpHeaderValueSourceV1, HttpJsonPropertiesV1, HttpJsonPropertyNameV1, HttpJsonPropertyV1,
    HttpJsonTemplateV1, HttpPathArgumentSegmentV1, HttpPathLiteralSegmentV1, HttpPathSegmentV1,
    HttpQueryEntryV1, HttpQueryLiteralValueV1, HttpQueryNameV1, HttpStatusRangeV1,
    HttpValueSourceV1, InstanceConfigurationKeyV1, MAX_ATTEMPT_LOGICAL_IO_BYTES,
    MAX_COMMAND_COMBINED_OUTPUT_BYTES, MAX_COMMAND_STDERR_BYTES, MAX_COMMAND_STDOUT_BYTES,
    MAX_HTTP_REQUEST_BODY_BYTES, MAX_HTTP_RESPONSE_DECOMPRESSED_BYTES,
    MAX_HTTP_RESPONSE_WIRE_BYTES, MAX_STDIO_FRAME_BYTES, MAX_STDIO_STDOUT_WIRE_BYTES,
    MAX_TOOL_ARTIFACT_BYTES, MAX_TOOL_OUTPUT_ARTIFACTS_V1, PackageRelativePath,
    STDIO_PROTOCOL_VERSION_V1, StdioArtifactReadyPayloadV1, StdioEmptyPayloadV1,
    StdioExecutePayloadV1, StdioFinalFailurePayloadV1, StdioFinalResultPayloadV1,
    StdioHelloPayloadV1, StdioProgressPayloadV1, StdioProtocolDirectionV1, StdioProtocolFrameV1,
    ToolOutputRelativePathV1, ToolOutputSlotKeyV1, ToolRuntimeContractError,
    ToolRuntimeDescriptorV1, ToolRuntimeOutputAuthorizationV1, validate_command_environment_v1,
    validate_http_header_bindings_v1,
};
pub use schema::{
    CANONICAL_TOOL_SCHEMA_POLICY_VERSION, CanonicalJsonSchemaDocumentV1, CompiledToolSchemaV1,
    MAX_CANONICAL_SCHEMA_BYTES, MAX_PROVIDER_ARGUMENTS_BYTES, MAX_PUBLIC_SCHEMA_ERRORS,
    PROVIDER_PURPOSE_FIELD, ToolInputSchemaV1, ToolSchemaDialect, ToolSchemaError,
    ToolSchemaValidationIssueV1, ToolSchemaValidationReportV1, ToolStructuredResultSchemaV1,
};
pub use version::{ToolVersion, ToolVersionError};
