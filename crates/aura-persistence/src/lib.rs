#![forbid(unsafe_code)]
mod broker_evidence;

mod artifact;
/// The default-off storage hold points. `cfg(debug_assertions)` gates this module
/// AND every one of its consumers below, so a release build contains no hold point
/// at all and a dev build carries them unarmed. See the module for why the class
/// stays off in both dev images without a control document.
#[cfg(debug_assertions)]
pub mod barrier;
mod context;
mod coordinator;
mod diagnostics;
mod error;
mod external_approvals;
mod legacy_configuration_transport;
mod migration;
mod migration_check;
mod model;
mod model_binding;
mod model_catalog;
mod model_config_journal;
mod model_credential;
mod model_operation;
mod model_projection;
#[cfg(test)]
mod model_projection_fixture;
mod model_runtime;
mod mutation_journal;
mod read_pool;
mod registry_purge;
mod security;
mod session;
mod session_lifecycle;
pub use session_lifecycle::{RemovedWorkspace, SESSION_ARCHIVED, SESSION_DELETING, SESSION_IDLE};
mod session_title;
mod session_write;
pub use session_write::{SessionPublicationReceipt, SessionWriteCompletion, SessionWriteReceipt};
mod migration_frozen;
mod tool;
mod tool_activity;
mod tool_builtin_release;
mod tool_command;
mod tool_credential;
mod tool_credential_journal;
pub use tool_credential_journal::{
    PackageToolCredentialFacts, ToolCredentialMutationIntentV1, ToolCredentialMutationJournalV1,
    ToolCredentialScopeSource,
};
pub mod tool_entities;
mod tool_execution;
mod tool_history;
mod tool_package_reads;
mod tool_projection;
mod tool_reference;
mod tool_registry_authority;
mod tool_settings_mutation;
mod workspace;
mod writer;
mod writer_queue;

pub use artifact::{
    ARTIFACT_CHUNK_SIZE, ARTIFACT_INLINE_LIMIT, ARTIFACT_RANGE_LIMIT, ArtifactGcReport,
    ArtifactMetadataRequest, ArtifactReadRequest, ArtifactWriteDraftV1, ArtifactWriteRequest,
};
pub use context::{
    ContextArtifactDraft, ContextCompactionAttemptTerminalRequest,
    ContextCompactionCompletedRequest, ContextCompactionCreateRequest,
    ContextCompactionRunningRequest, ContextCompactionSourceDraft, ContextCompactionStoredRecord,
    ContextCompactionTerminalRequest, ContextEventStoredQuery, ContextEventStoredRow,
    ContextInternalArtifactMetadata, ContextManualCompactionCreateRequest,
    ContextManualCompactionRunningRequest, ContextManualCompactionTerminalRequest,
    ContextProjectionCommitRequest, ContextProjectionCommitResult,
    ContextRemediationClearedRequest, ContextRemediationCommitResult,
    ContextRemediationRequiredRequest, ContextSearchChunkDraft, ContextSearchStoredPage,
    ContextSearchStoredQuery, ContextSearchStoredRow, ContextSourceEventCommitRequest,
    ContextSourceEventStoredFact, ContextStartupRecoveryReport, ContextToolResultStored,
    PromptBundleCommitRequest, PromptBundleCommitResult, StoredContextProjectionSnapshot,
};
pub use coordinator::{
    DEFAULT_SETTLED_RECEIPT_BUDGET, DEFAULT_SYNC_DEADLINE, INDEX_DEADLINE, INDEX_QUEUE_CAPACITY,
    JobSettlement, PersistenceArtifactCounters, PersistenceCoordinator, PersistenceFaultPoint,
    PersistenceOptions, PersistenceStartup, SETTLEMENT_CAPACITY, SYNC_QUEUE_CAPACITY,
    WriterSettlements, WriterState,
};
pub use error::PersistenceError;
pub use legacy_configuration_transport::{
    ConfigurationTransportRecoverySnapshot, EffectiveTransportAttemptStatus,
    HistoricalIntegrityDegradation, LEGACY_CONFIGURATION_TRANSPORT_MIGRATION_KEY,
};
pub use migration::{DatabaseOpenOptions, MigrationFaultPoint};
pub use migration_check::store_migration_pending;
pub use model::{
    CommittedProjection, DatabasePragmas, EventActor, MigrationRecoveryRequired,
    PersistentEventDraft,
};
pub use model_binding::{
    CommitSessionModelBinding, PreparedSessionModelBindingClear, SessionModelBindingCommit,
    SessionModelBindingState,
};
pub use model_catalog::{
    ProviderCatalogCacheSnapshotRecord, ProviderCatalogCacheStateRecord, ProviderCatalogCacheWrite,
};
pub use model_config_journal::{
    MODEL_CONFIG_MUTATION_JOURNAL_KIND, MODEL_CONFIG_MUTATION_JOURNAL_SCHEMA_VERSION,
    ModelConfigCredentialChildOperation, ModelConfigCredentialChildPhase,
    ModelConfigCredentialChildRef, ModelConfigCredentialDeleteJournals,
    ModelConfigCredentialJournalPair, ModelConfigCredentialMetadataCommit,
    ModelConfigMetadataCommit, ModelConfigMutationJournalPayload, ModelConfigMutationJournalRecord,
    ModelConfigMutationJournalState, PrepareModelConfigCredentialDeleteJournalsResult,
    PrepareModelConfigCredentialJournalPairResult, PrepareModelConfigMutationJournalResult,
};
pub use model_credential::{
    CreateModelCredentialSlot, CredentialMutationJournalKind, CredentialMutationJournalRecord,
    CredentialMutationJournalState, ModelCredentialDeleteMetadataCommit,
    ModelCredentialDeletionReason, ModelCredentialFormerOwner, ModelCredentialMetadataCommit,
    ModelCredentialRecord, ModelCredentialRecordState, ModelCredentialStorageBinding,
    ModelCredentialTombstone, ModelCredentialWriteIntent, ModelTransportCredentialResolution,
    PrepareModelCredentialClear, PrepareModelCredentialClearResult, PrepareModelCredentialWrite,
    PrepareModelCredentialWriteResult,
};
pub use model_operation::{
    ModelAvailabilityTransportTransition, ModelDiscoveryTerminalEventContext,
    ModelDiscoveryTransportTransition, ModelOperationCommit,
};
pub use model_projection::ModelProjectionDraft;
pub use mutation_journal::{
    MutationJournalMetadataCommit, MutationJournalRecord, MutationJournalRecoveryHandler,
    MutationJournalRecoveryRegistry, PrepareMutationJournal,
};
pub use read_pool::ReadPoolCounters;
pub use security::{
    ApprovalCommit, ArtifactOverwriteJournalBinding, ArtifactOverwriteJournalRecord,
    ArtifactOverwriteJournalState, HostPreparationExpiry, HostPreparationJournalRecord,
    HostPreparationOperationTransition, HostPreparationStoredState, SecurityStartupReport,
};
pub use tool_activity::{ToolActivityPageReadErrorV1, tool_activity_visibility_context_hash_v1};
pub use tool_builtin_release::{
    SignedBuiltinReleaseAuthorityEntryV1, SignedBuiltinReleaseMaterializationCommitV1,
};
pub use tool_command::{
    PrepareToolMutationCommandV1, PreparedToolMutationCommandV1,
    ToolMutationCommandJournalRecordV1, ToolMutationCommandJournalStateV1,
    ToolMutationCommandOutcomeV1, ToolMutationCommandSeedV1,
};
pub use tool_credential::{
    AuthorizedToolCredentialV1, ToolCredentialAuthoritySeedV1,
    ToolCredentialBindingAuthorizationV1, ToolCredentialCandidateQueryV1,
};
#[cfg(debug_assertions)]
#[doc(hidden)]
pub use tool_entities::VerifiedToolPackageInstallRequest;
pub use tool_entities::{
    SessionToolPinChangeRequest, ToolAuthoritySnapshotV1, ToolEntityMutationCommit,
    ToolInstanceEnabledChangeRequest, ToolPackageTrustChangeRequest, ToolPinTargetV1,
    WorkspaceToolPinChangeRequest, WorkspaceToolPinMutationCommitV1,
};
pub use tool_execution::{
    AuthorizedBuiltinCoreHandlerAttemptDispatchPreparedCommitRequestV1,
    AuthorizedToolExecutionAttemptDispatchPreparedCommitRequestV1, ToolBatchAdmissionCommitRequest,
    ToolExecutionAttemptCancellationRequest, ToolExecutionAttemptDispatchPreparedCommitRequest,
    ToolExecutionAttemptFinalizationRequest, ToolExecutionAttemptProgressRequest,
    ToolExecutionAttemptRetryableFailureRequest, ToolExecutionAttemptRuntimeStartedRequest,
    ToolExecutionAttemptTerminalFact, ToolExecutionEntityStateV1, ToolExecutionPersistenceCommit,
    ToolExecutionStartupRecoveryReport,
};
pub use tool_history::{
    LegacyHistoryAtomicGroupSideMappingV1, StoredToolHistoryAtomicGroupAssociationV1,
    ToolHistoryAtomicGroupAssociationRequestV1, VerifiedToolContinuationAuthorityV1,
};
pub use tool_projection::ToolProjectionDraft;
pub use tool_registry_authority::{
    ExactToolDefinitionDescriptionV1, ExactToolExecutionAuthorityV1,
    ExactToolPackageExecutionAuthorityV1, ToolInstanceConfigurationChangeRequestV1,
    ToolPackageTrustIntentApplyRequestV1, ToolRegistryBuildCommitV1, ToolRegistryBuildRequestV1,
    ToolRegistryExecutionAuthorityRecordV1, ToolRegistryMutationCommitV1,
};
pub use tool_settings_mutation::{
    ToolDefinitionEnabledChangeRequestV1, ToolInstanceConfigurationSettingsChangeRequestV1,
    ToolInstanceCredentialClearRequestV1, ToolInstanceCredentialSettingsChangeRequestV1,
    ToolSettingsMutationCommitV1,
};
pub use workspace::{
    CatalogCommitOutcome, CatalogCommitPosition, CatalogCommitRefusal, CatalogDeltaRecord,
    CatalogEntryRecord, CatalogPartitionRecord, DeferredMutationRecord, MutationAuditRecord,
    MutationOwnershipExecutionId, WorkspaceBlockers, WorkspaceDeleteReport,
    WorkspaceLifecycleRecord,
};
pub use writer::WriterHealth;
pub use writer_queue::WorkClass;
