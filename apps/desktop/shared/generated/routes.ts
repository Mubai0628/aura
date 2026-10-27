/** Generated from the Rust Route Registry. Do not edit. */
import type {
  ActivityQueryRequestV1,
  ActivityQueryResponseV1,
  AgentCompactionRequest,
  AgentCompactionStartRequest,
  AgentCompactionStatus,
  AgentContextRecheckRequest,
  AgentContextRemediationView,
  AgentContextWindowResolveRequest,
  AgentHistoryRequest,
  AgentInboxMutation,
  AgentInboxProjection,
  AgentInitStartRequest,
  AgentInspectorRequest,
  AgentMessageTextPage,
  AgentMessageTextRequest,
  AgentModeControlRequest,
  AgentModeEpisodeView,
  AgentNetworkProxyTestRequest,
  AgentNetworkProxyTestResult,
  AgentPromptInspection,
  AgentQueryAnswerRequest,
  AgentQueryFileRequest,
  AgentQueryFileSelection,
  AgentQueryTouchRequest,
  AgentQueryTouchView,
  AgentRelatedTurnRequest,
  AgentResultRequest,
  AgentResultView,
  AgentRunProjection,
  AgentRunRequest,
  AgentSessionBranchRequest,
  AgentSessionBranchResult,
  AgentSessionProjection,
  AgentSessionRequest,
  AgentSessionsActivity,
  AgentSessionsActivityRequest,
  AgentStartRequest,
  AgentSubmitRequest,
  AgentToolActivityPage,
  AgentToolActivityRequest,
  AgentToolSettingsRequest,
  AgentToolSettingsUpdate,
  AgentToolSettingsView,
  ApprovalRequest,
  ApprovalResolution,
  ApprovalView,
  ArtifactExportRequest,
  ArtifactExportResult,
  AttachExternalReadOnlyInput,
  AttachmentEntryRequest,
  AttachmentLocateRequest,
  AttachmentLocation,
  AttachmentText,
  AttachmentTextRequest,
  BeginWorkspaceLifecycleInput,
  BeginWorkspaceLifecycleResult,
  BrokerHttpReadRequest,
  BrokerHttpReadResult,
  BrowserHostResultAck,
  BrowserHostResultV1,
  BrowserScreenshotImageV1,
  BrowserScreenshotReadRequest,
  BrowserSettingsSnapshot,
  BrowserSettingsUpdate,
  BufferEditCommand,
  BufferEditResult,
  BufferOpenCommand,
  BufferSaveCommand,
  BundleRecoveryPlan,
  ChromeControlStatus,
  ChromeExtensionFolder,
  ChromePairingCode,
  ChromeSessionRequest,
  ChromeSessionShares,
  ChromeSetEnabledRequest,
  ClipboardWriteRequest,
  ClipboardWritten,
  CommandCatalog,
  CommandCatalogRequest,
  CommandDeleteRequest,
  CommandDirectoryOpened,
  CommandDirectoryRequest,
  CommandSetEnabledRequest,
  ConfigRecoveryApplyRequest,
  ConfigRecoveryPlanRequest,
  ContextBrokerReadRequest,
  ContextBrokerReadResult,
  ContextWindowClearUserOverrideRequest,
  ContextWindowCorrectRequest,
  ContextWindowMutationResult,
  ContextWindowResetRuntimeCorrectionRequest,
  DefaultModelMutationResult,
  DeriveWorkspaceExternalGrantInput,
  DiscardConfirmation,
  DiscardQuarantineInput,
  EditorLifecycleTransitionAck,
  EmptyToolInput,
  EndpointCompatibilitySnapshot,
  EndpointCreatedWithCredentialResult,
  EndpointCredentialMutationResult,
  ExecutionApprovalRequest,
  ExecutionApprovalResolutionRequest,
  ExecutionApprovalView,
  ExternalGrantView,
  ExternalLinkOpenRequest,
  FileSessionConsentView,
  FsApplyChangesetRequest,
  FsApplyChangesetResult,
  FsApplyPatchRequest,
  FsCopyRequest,
  FsCreateDirectoryRequest,
  FsDeleteRequest,
  FsFindPathsRequest,
  FsGlobRequest,
  FsGlobResult,
  FsHashRequest,
  FsHashResult,
  FsInspectRequest,
  FsInspectResult,
  FsListRequest,
  FsListResult,
  FsMoveRequest,
  FsMutationResult,
  FsReadBinaryRangeRequest,
  FsReadBinaryRangeResult,
  FsReadTextRequest,
  FsReadTextResult,
  FsSearchTextRequest,
  FsSearchTextResult,
  FsStatRequest,
  FsStatResult,
  FsWriteFileRequest,
  GetApprovalDetailsInput,
  GetSessionPermissionStateInput,
  GitInstallationResultV1,
  GitReadResultV1,
  GitRuntimeStatusV1,
  GitWorkspaceReadRequestV1,
  HookInvocationList,
  HookInvocationListRequest,
  HookListRequest,
  HookMoveRequest,
  HookPathRequest,
  HookPreview,
  HookPreviewRequest,
  HookRegistryView,
  HookSelectDirectoryRequest,
  HookSelectedDirectory,
  HookSetEnabledRequest,
  HookTrustRequest,
  HostPreparationBeginResult,
  HostPreparationMutationInput,
  HostPreparationStatus,
  IssueDiscardConfirmationInput,
  IssueQuarantineDiscardConfirmationInput,
  ListFileSessionConsentsInput,
  ListMemorySessionConsentsInput,
  ListNativeSessionConsentsInput,
  ListNetworkSessionConsentsInput,
  ListPermissionGrantsInput,
  ListSessionExternalGrantsInput,
  LogicalFileStatus,
  ManagedProcessListV1,
  ManagedProcessRequestV1,
  ManagedProcessStopRequestV1,
  ManagedProcessStopResultV1,
  McpDescribeRequest,
  McpListRequest,
  McpPreview,
  McpPreviewRequest,
  McpRefreshRequest,
  McpRegistryView,
  McpSaveRequest,
  McpSecretSetRequest,
  McpSecretSetResult,
  McpServerDetail,
  McpServerRequest,
  McpSetEnabledRequest,
  McpSignInBegun,
  McpSignInRequest,
  McpSignOutRequest,
  McpTrustRequest,
  MediaHostReadRequestV1,
  MediaHostReadResultV1,
  MediaHostResultAck,
  MediaHostResultV1,
  MemoryDesktopActionResult,
  MemoryDesktopRepairApply,
  MemoryDesktopRequest,
  MemoryDesktopSnapshot,
  MemoryExportChunk,
  MemoryExportChunkRequest,
  MemoryExportCloseRequest,
  MemoryExportEntry,
  MemoryExportEntryRequest,
  MemoryExportHandle,
  MemoryRepairPreview,
  MemoryRepairResult,
  MemorySessionConsentView,
  MemorySettingsSnapshot,
  MemorySettingsUpdate,
  MemoryValidation,
  MemoryVaultRequest,
  MemoryVaultStatus,
  MentionedFileRequest,
  ModelAvailabilityTestCancelRequest,
  ModelAvailabilityTestCancelResult,
  ModelAvailabilityTestStartAccepted,
  ModelAvailabilityTestStartRequest,
  ModelCatalogEnsureFreshRequest,
  ModelCatalogGetRequest,
  ModelCatalogProviderGetRequest,
  ModelCatalogRefreshRequest,
  ModelCompatibilityGetRequest,
  ModelCompatibilityResetRequest,
  ModelCompatibilityResetResult,
  ModelDefaultSetRequest,
  ModelDeletionMutationResult,
  ModelDiscoveryCancelRequest,
  ModelDiscoveryCancelResult,
  ModelDiscoveryStartAccepted,
  ModelDiscoveryStartRequest,
  ModelEndpointClearCredentialRequest,
  ModelEndpointConfigureCredentialRequest,
  ModelEndpointCreateRequest,
  ModelEndpointCreateWithCredentialRequest,
  ModelEndpointDeleteRequest,
  ModelEndpointMutationResult,
  ModelEndpointReplaceCredentialRequest,
  ModelEndpointUpdateRequest,
  ModelProfileApplyCatalogUpdateRequest,
  ModelProfileCreateFromCatalogRequest,
  ModelProfileCreateManualRequest,
  ModelProfileDeleteRequest,
  ModelProfileMutationResult,
  ModelProfileSetSubagentAvailabilityRequest,
  ModelProfileUpdateRequest,
  ModelProviderConnectionsRequest,
  ModelProviderCreateRequest,
  ModelProviderDeleteRequest,
  ModelProviderMutationResult,
  ModelProviderRenameRequest,
  ModelProviderSetAllCurrentModelsSubagentAvailabilityRequest,
  ModelSettingsGetSnapshotRequest,
  ModelSettingsProjection,
  NativeSessionConsentView,
  NetworkSessionConsentView,
  OpenedOutside,
  PackageChangeRequestV1,
  PackageConformanceCancelRequestV1,
  PackageConformancePreparationV1,
  PackageConformancePrepareRequestV1,
  PackageConformanceStartRequestV1,
  PackageConformanceViewV1,
  PackageCredentialBindRequestV1,
  PackageCredentialClearRequestV1,
  PackageEnableRequestV1,
  PackageImportOutcomeV1,
  PackageImportSourceV1,
  PackageInstallRequestV1,
  PackageListV1,
  PackageSummaryV1,
  PackageVersionRefV1,
  PackageVersionViewV1,
  PathSearchResult,
  PermissionDefaults,
  PermissionGrant,
  PluginChangeRequest,
  PluginDirectoryOpened,
  PluginImportRequest,
  PluginListRequest,
  PluginPreview,
  PluginRegistryView,
  PluginRequest,
  PluginSelectArchiveRequest,
  PluginSelectDirectoryRequest,
  PluginSelectedArchive,
  PluginSelectedDirectory,
  PluginSetEnabledRequest,
  PluginTrustRequest,
  PrepareEditorLifecycleTransition,
  PrepareQuarantineReleaseInput,
  PrepareQuarantineReleaseResult,
  ProviderBulkAvailabilityResult,
  ProviderCatalogModelPage,
  ProviderCatalogProjection,
  ProviderIconReadRequest,
  ProviderIconReadResult,
  QuarantineDiscardConfirmation,
  QuarantineRecordDetails,
  RecoveryNotice,
  RecoveryReport,
  ReleaseQuarantineInput,
  RendererLifecycleUnavailableInput,
  ResolveApprovalInput,
  ResolveHostPreparationInput,
  ResolveWorkspaceLifecycleInput,
  RevalidateExternalGrantInput,
  RevokeExternalGrantInput,
  RevokeFileSessionConsentInput,
  RevokeMemorySessionConsentInput,
  RevokeNativeSessionConsentInput,
  RevokeNetworkSessionConsentInput,
  RevokePermissionGrantInput,
  RuntimeReadiness,
  SessionCreateRequest,
  SessionDeleteResult,
  SessionLifecycleRequest,
  SessionModelBindingMutationResult,
  SessionModelBindingSetRequest,
  SessionPermissionProjection,
  SessionRenameRequest,
  SessionSummary,
  SessionToolPinClearRequestV1,
  SessionToolPinSetRequestV1,
  SetPermissionDefaultsInput,
  SetSessionPermissionModeInput,
  ShellExecutionView,
  ShellQueryRequest,
  ShellStopRequest,
  SkillChangeRequest,
  SkillDirectoryOpened,
  SkillImportRequest,
  SkillListRequest,
  SkillPreview,
  SkillRegistryView,
  SkillRequest,
  SkillSelectArchiveRequest,
  SkillSelectDirectoryRequest,
  SkillSelectedArchive,
  SkillSelectedDirectory,
  SkillSetEnabledRequest,
  SkillTrustRequest,
  TerminalControlRequestV1,
  TerminalListV1,
  TerminalOutputBatchV1,
  TerminalRequestV1,
  TerminalResizeRequestV1,
  TerminalSensitivePendingV1,
  TerminalSensitiveReplyV1,
  TerminalSensitiveResultV1,
  TerminalSessionRequestV1,
  TerminalSubscribeRequestV1,
  TerminalViewV1,
  ToolBrokerInvokeRequest,
  ToolBrokerInvokeResult,
  ToolCredentialCandidatesProjectionV1,
  ToolCredentialCandidatesRequestV1,
  ToolCredentialChangeRequestV1,
  ToolCredentialCreateRequestV1,
  ToolCredentialListResultV1,
  ToolCredentialManagementTargetV1,
  ToolCredentialMutationResultV1,
  ToolCredentialOriginsRequestV1,
  ToolCredentialOutcomeRequestV1,
  ToolCredentialOutcomeV1,
  ToolInstanceBindCredentialRequestV1,
  ToolInstanceClearCredentialRequestV1,
  ToolInstanceUpdateConfigurationRequestV1,
  ToolMutationResultV1,
  ToolSetEnabledRequestV1,
  UiSettingsSnapshot,
  UiSettingsUpdate,
  UserTerminalOpenRequestV1,
  UserTerminalWriteRequestV1,
  WorkspaceArchiveCommand,
  WorkspaceCreateCommand,
  WorkspaceCreateResult,
  WorkspaceDeleteCommand,
  WorkspaceDirectorySelection,
  WorkspaceFileOpenRequest,
  WorkspaceIndexRebuildRequest,
  WorkspaceIndexRebuildStarted,
  WorkspaceLifecycleOperationQuery,
  WorkspaceLifecyclePrepareDeliveredInput,
  WorkspaceLifecycleSummary,
  WorkspaceRelocateCommand,
  WorkspaceRestoreCommand,
  WorkspaceSelectDirectoryRequest,
  WorkspaceSummary,
  WorkspaceToolPinClearRequestV1,
  WorkspaceToolPinSetRequestV1,
} from '@aura/contracts/generated';
import type { RendererLifecycleAck } from '../p03-validation.js';
import type { RendererIssueDiscardConfirmation } from '../p03-validation.js';
import type { RendererResolveWorkspaceLifecycle } from '../p03-validation.js';
import type { RendererAttachExternalReadOnlyInput } from '../p04-validation.js';
export type P03MainParams = {
  'session.rename': SessionRenameRequest;
  'workspace.select_directory': WorkspaceSelectDirectoryRequest;
  'session.archive': SessionLifecycleRequest;
  'session.restore': SessionLifecycleRequest;
  'session.delete': SessionLifecycleRequest;
  'session.create': SessionCreateRequest;
  'fs.apply_changeset': FsApplyChangesetRequest;
  'fs.apply_patch': FsApplyPatchRequest;
  'fs.copy': FsCopyRequest;
  'fs.create_directory': FsCreateDirectoryRequest;
  'fs.delete': FsDeleteRequest;
  'fs.find_paths': FsFindPathsRequest;
  'fs.glob': FsGlobRequest;
  'fs.hash': FsHashRequest;
  'fs.inspect': FsInspectRequest;
  'fs.list': FsListRequest;
  'fs.move': FsMoveRequest;
  'fs.read_binary_range': FsReadBinaryRangeRequest;
  'fs.read_text': FsReadTextRequest;
  'fs.search_text': FsSearchTextRequest;
  'fs.stat': FsStatRequest;
  'fs.write_file': FsWriteFileRequest;
  'workspace.archive': WorkspaceArchiveCommand;
  'workspace.buffer.edit': BufferEditCommand;
  'workspace.buffer.open': BufferOpenCommand;
  'workspace.buffer.save': BufferSaveCommand;
  'workspace.create': WorkspaceCreateCommand;
  'workspace.delete': WorkspaceDeleteCommand;
  'workspace.lifecycle.ack': RendererLifecycleAck;
  'workspace.lifecycle.issue_discard_confirmation': RendererIssueDiscardConfirmation;
  'workspace.lifecycle.resolve': RendererResolveWorkspaceLifecycle;
  'workspace.relocate': WorkspaceRelocateCommand;
  'workspace.restore': WorkspaceRestoreCommand;
};
export type P03MainResult = {
  'session.rename': SessionSummary;
  'workspace.select_directory': WorkspaceDirectorySelection;
  'session.archive': SessionSummary;
  'session.restore': SessionSummary;
  'session.delete': SessionDeleteResult;
  'session.create': SessionSummary;
  'fs.apply_changeset': FsApplyChangesetResult;
  'fs.apply_patch': FsMutationResult;
  'fs.copy': FsMutationResult;
  'fs.create_directory': FsMutationResult;
  'fs.delete': FsMutationResult;
  'fs.find_paths': PathSearchResult;
  'fs.glob': FsGlobResult;
  'fs.hash': FsHashResult;
  'fs.inspect': FsInspectResult;
  'fs.list': FsListResult;
  'fs.move': FsMutationResult;
  'fs.read_binary_range': FsReadBinaryRangeResult;
  'fs.read_text': FsReadTextResult;
  'fs.search_text': FsSearchTextResult;
  'fs.stat': FsStatResult;
  'fs.write_file': FsMutationResult;
  'workspace.archive': BeginWorkspaceLifecycleResult;
  'workspace.buffer.edit': BufferEditResult;
  'workspace.buffer.open': LogicalFileStatus;
  'workspace.buffer.save': LogicalFileStatus;
  'workspace.create': WorkspaceCreateResult;
  'workspace.delete': BeginWorkspaceLifecycleResult;
  'workspace.lifecycle.ack': WorkspaceLifecycleSummary;
  'workspace.lifecycle.issue_discard_confirmation': DiscardConfirmation;
  'workspace.lifecycle.resolve': WorkspaceLifecycleSummary;
  'workspace.relocate': BeginWorkspaceLifecycleResult;
  'workspace.restore': WorkspaceSummary;
};
export type P03MainCommand = keyof P03MainParams;
export type P03CoreParams = {
  'session.rename': SessionRenameRequest;
  'session.archive': SessionLifecycleRequest;
  'session.restore': SessionLifecycleRequest;
  'session.delete': SessionLifecycleRequest;
  'session.create': SessionCreateRequest;
  'fs.apply_changeset': FsApplyChangesetRequest;
  'fs.apply_patch': FsApplyPatchRequest;
  'fs.copy': FsCopyRequest;
  'fs.create_directory': FsCreateDirectoryRequest;
  'fs.delete': FsDeleteRequest;
  'fs.find_paths': FsFindPathsRequest;
  'fs.glob': FsGlobRequest;
  'fs.hash': FsHashRequest;
  'fs.inspect': FsInspectRequest;
  'fs.list': FsListRequest;
  'fs.move': FsMoveRequest;
  'fs.read_binary_range': FsReadBinaryRangeRequest;
  'fs.read_text': FsReadTextRequest;
  'fs.search_text': FsSearchTextRequest;
  'fs.stat': FsStatRequest;
  'fs.write_file': FsWriteFileRequest;
  'workspace.buffer.edit': BufferEditCommand;
  'workspace.buffer.open': BufferOpenCommand;
  'workspace.buffer.save': BufferSaveCommand;
  'workspace.create': WorkspaceCreateCommand;
  'workspace.lifecycle.ack': EditorLifecycleTransitionAck;
  'workspace.lifecycle.begin': BeginWorkspaceLifecycleInput;
  'workspace.lifecycle.issue_discard_confirmation': IssueDiscardConfirmationInput;
  'workspace.lifecycle.prepare': WorkspaceLifecycleOperationQuery;
  'workspace.lifecycle.prepare_delivered': WorkspaceLifecyclePrepareDeliveredInput;
  'workspace.lifecycle.renderer_unavailable': RendererLifecycleUnavailableInput;
  'workspace.lifecycle.resolve': ResolveWorkspaceLifecycleInput;
  'workspace.restore': WorkspaceRestoreCommand;
};
export type P03CoreResult = {
  'session.rename': SessionSummary;
  'session.archive': SessionSummary;
  'session.restore': SessionSummary;
  'session.delete': SessionDeleteResult;
  'session.create': SessionSummary;
  'fs.apply_changeset': FsApplyChangesetResult;
  'fs.apply_patch': FsMutationResult;
  'fs.copy': FsMutationResult;
  'fs.create_directory': FsMutationResult;
  'fs.delete': FsMutationResult;
  'fs.find_paths': PathSearchResult;
  'fs.glob': FsGlobResult;
  'fs.hash': FsHashResult;
  'fs.inspect': FsInspectResult;
  'fs.list': FsListResult;
  'fs.move': FsMutationResult;
  'fs.read_binary_range': FsReadBinaryRangeResult;
  'fs.read_text': FsReadTextResult;
  'fs.search_text': FsSearchTextResult;
  'fs.stat': FsStatResult;
  'fs.write_file': FsMutationResult;
  'workspace.buffer.edit': BufferEditResult;
  'workspace.buffer.open': LogicalFileStatus;
  'workspace.buffer.save': LogicalFileStatus;
  'workspace.create': WorkspaceCreateResult;
  'workspace.lifecycle.ack': WorkspaceLifecycleSummary;
  'workspace.lifecycle.begin': BeginWorkspaceLifecycleResult;
  'workspace.lifecycle.issue_discard_confirmation': DiscardConfirmation;
  'workspace.lifecycle.prepare': PrepareEditorLifecycleTransition;
  'workspace.lifecycle.prepare_delivered': BeginWorkspaceLifecycleResult;
  'workspace.lifecycle.renderer_unavailable': WorkspaceLifecycleSummary[];
  'workspace.lifecycle.resolve': WorkspaceLifecycleSummary;
  'workspace.restore': WorkspaceSummary;
};
export type P03CoreCommand = keyof P03CoreParams;
export type P04MainParams = {
  'permission.get_defaults': Record<string, never>;
  'permission.set_defaults': SetPermissionDefaultsInput;
  'artifact.export': ArtifactExportRequest;
  'approval.get_details': GetApprovalDetailsInput;
  'approval.list_pending': Record<string, never>;
  'approval.resolve': ResolveApprovalInput;
  'permission.get_session_state': GetSessionPermissionStateInput;
  'permission.list_grants': ListPermissionGrantsInput;
  'security.file_session_consent.list': ListFileSessionConsentsInput;
  'security.network_session_consent.list': ListNetworkSessionConsentsInput;
  'security.file_session_consent.revoke': RevokeFileSessionConsentInput;
  'security.memory_session_consent.list': ListMemorySessionConsentsInput;
  'security.memory_session_consent.revoke': RevokeMemorySessionConsentInput;
  'security.network_session_consent.revoke': RevokeNetworkSessionConsentInput;
  'security.native_session_consent.list': ListNativeSessionConsentsInput;
  'security.native_session_consent.revoke': RevokeNativeSessionConsentInput;
  'permission.revoke_grant': RevokePermissionGrantInput;
  'permission.set_session_mode': SetSessionPermissionModeInput;
  'security.host_preparation.prepare': HostPreparationMutationInput;
  'security.host_preparation.remove': HostPreparationMutationInput;
  'security.host_preparation.repair': HostPreparationMutationInput;
  'security.host_preparation.status': Record<string, never>;
  'security.external_grant.attach_read_only': RendererAttachExternalReadOnlyInput;
  'security.external_grant.derive_workspace': DeriveWorkspaceExternalGrantInput;
  'security.external_grant.list': ListSessionExternalGrantsInput;
  'security.external_grant.revalidate': RevalidateExternalGrantInput;
  'security.external_grant.revoke': RevokeExternalGrantInput;
  'security.quarantine.discard': DiscardQuarantineInput;
  'security.quarantine.issue_discard_confirmation': IssueQuarantineDiscardConfirmationInput;
  'security.quarantine.list': Record<string, never>;
  'security.quarantine.prepare_release': PrepareQuarantineReleaseInput;
  'security.quarantine.release': ReleaseQuarantineInput;
};
export type P04MainResult = {
  'permission.get_defaults': PermissionDefaults;
  'permission.set_defaults': PermissionDefaults;
  'artifact.export': ArtifactExportResult;
  'approval.get_details': ApprovalRequest;
  'approval.list_pending': ApprovalView[];
  'approval.resolve': ApprovalResolution;
  'permission.get_session_state': SessionPermissionProjection;
  'permission.list_grants': PermissionGrant[];
  'security.file_session_consent.list': FileSessionConsentView[];
  'security.network_session_consent.list': NetworkSessionConsentView[];
  'security.file_session_consent.revoke': FileSessionConsentView;
  'security.memory_session_consent.list': MemorySessionConsentView[];
  'security.memory_session_consent.revoke': MemorySessionConsentView;
  'security.network_session_consent.revoke': NetworkSessionConsentView;
  'security.native_session_consent.list': NativeSessionConsentView[];
  'security.native_session_consent.revoke': NativeSessionConsentView;
  'permission.revoke_grant': PermissionGrant;
  'permission.set_session_mode': SessionPermissionProjection;
  'security.host_preparation.prepare': HostPreparationStatus;
  'security.host_preparation.remove': HostPreparationStatus;
  'security.host_preparation.repair': HostPreparationStatus;
  'security.host_preparation.status': HostPreparationStatus;
  'security.external_grant.attach_read_only': ExternalGrantView | null;
  'security.external_grant.derive_workspace': ExternalGrantView[];
  'security.external_grant.list': ExternalGrantView[];
  'security.external_grant.revalidate': ExternalGrantView;
  'security.external_grant.revoke': ExternalGrantView;
  'security.quarantine.discard': QuarantineRecordDetails;
  'security.quarantine.issue_discard_confirmation': QuarantineDiscardConfirmation;
  'security.quarantine.list': QuarantineRecordDetails[];
  'security.quarantine.prepare_release': PrepareQuarantineReleaseResult;
  'security.quarantine.release': QuarantineRecordDetails;
};
export type P04MainCommand = keyof P04MainParams;
export type P04CoreParams = {
  'permission.get_defaults': Record<string, never>;
  'permission.set_defaults': SetPermissionDefaultsInput;
  'broker.http_read': BrokerHttpReadRequest;
  'artifact.export': ArtifactExportRequest;
  'approval.get_details': GetApprovalDetailsInput;
  'approval.list_pending': Record<string, never>;
  'approval.resolve': ResolveApprovalInput;
  'permission.get_session_state': GetSessionPermissionStateInput;
  'permission.list_grants': ListPermissionGrantsInput;
  'security.file_session_consent.list': ListFileSessionConsentsInput;
  'security.network_session_consent.list': ListNetworkSessionConsentsInput;
  'security.file_session_consent.revoke': RevokeFileSessionConsentInput;
  'security.memory_session_consent.list': ListMemorySessionConsentsInput;
  'security.memory_session_consent.revoke': RevokeMemorySessionConsentInput;
  'security.network_session_consent.revoke': RevokeNetworkSessionConsentInput;
  'security.native_session_consent.list': ListNativeSessionConsentsInput;
  'security.native_session_consent.revoke': RevokeNativeSessionConsentInput;
  'permission.revoke_grant': RevokePermissionGrantInput;
  'permission.set_session_mode': SetSessionPermissionModeInput;
  'security.host_preparation.prepare': HostPreparationMutationInput;
  'security.host_preparation.remove': HostPreparationMutationInput;
  'security.host_preparation.repair': HostPreparationMutationInput;
  'security.host_preparation.status': Record<string, never>;
  'security.external_grant.derive_workspace': DeriveWorkspaceExternalGrantInput;
  'security.external_grant.list': ListSessionExternalGrantsInput;
  'security.external_grant.revalidate': RevalidateExternalGrantInput;
  'security.external_grant.revoke': RevokeExternalGrantInput;
  'security.quarantine.discard': DiscardQuarantineInput;
  'security.quarantine.issue_discard_confirmation': IssueQuarantineDiscardConfirmationInput;
  'security.quarantine.list': Record<string, never>;
  'security.quarantine.prepare_release': PrepareQuarantineReleaseInput;
  'security.quarantine.release': ReleaseQuarantineInput;
  'security.external_grant.attach_read_only_selected': AttachExternalReadOnlyInput;
  'security.host_preparation.probe': Record<string, never>;
  'security.host_preparation.resolve': ResolveHostPreparationInput;
};
export type P04CoreResult = {
  'permission.get_defaults': PermissionDefaults;
  'permission.set_defaults': PermissionDefaults;
  'broker.http_read': BrokerHttpReadResult;
  'artifact.export': ArtifactExportResult;
  'approval.get_details': ApprovalRequest;
  'approval.list_pending': ApprovalView[];
  'approval.resolve': ApprovalResolution;
  'permission.get_session_state': SessionPermissionProjection;
  'permission.list_grants': PermissionGrant[];
  'security.file_session_consent.list': FileSessionConsentView[];
  'security.network_session_consent.list': NetworkSessionConsentView[];
  'security.file_session_consent.revoke': FileSessionConsentView;
  'security.memory_session_consent.list': MemorySessionConsentView[];
  'security.memory_session_consent.revoke': MemorySessionConsentView;
  'security.network_session_consent.revoke': NetworkSessionConsentView;
  'security.native_session_consent.list': NativeSessionConsentView[];
  'security.native_session_consent.revoke': NativeSessionConsentView;
  'permission.revoke_grant': PermissionGrant;
  'permission.set_session_mode': SessionPermissionProjection;
  'security.host_preparation.prepare': HostPreparationBeginResult;
  'security.host_preparation.remove': HostPreparationBeginResult;
  'security.host_preparation.repair': HostPreparationBeginResult;
  'security.host_preparation.status': HostPreparationStatus;
  'security.external_grant.derive_workspace': ExternalGrantView[];
  'security.external_grant.list': ExternalGrantView[];
  'security.external_grant.revalidate': ExternalGrantView;
  'security.external_grant.revoke': ExternalGrantView;
  'security.quarantine.discard': QuarantineRecordDetails;
  'security.quarantine.issue_discard_confirmation': QuarantineDiscardConfirmation;
  'security.quarantine.list': QuarantineRecordDetails[];
  'security.quarantine.prepare_release': PrepareQuarantineReleaseResult;
  'security.quarantine.release': QuarantineRecordDetails;
  'security.external_grant.attach_read_only_selected': ExternalGrantView;
  'security.host_preparation.probe': HostPreparationStatus;
  'security.host_preparation.resolve': HostPreparationStatus;
};
export type P04CoreCommand = keyof P04CoreParams;
export type P05MainParams = {
  'provider_icon.read': ProviderIconReadRequest;
  'models.catalog.get': ModelCatalogGetRequest;
  'models.catalog.ensure_fresh': ModelCatalogEnsureFreshRequest;
  'models.catalog.refresh': ModelCatalogRefreshRequest;
  'models.catalog.provider.get': ModelCatalogProviderGetRequest;
  'models.compatibility.get': ModelCompatibilityGetRequest;
  'models.compatibility.reset': ModelCompatibilityResetRequest;
  'models.context_window.clear_user_override': ContextWindowClearUserOverrideRequest;
  'models.context_window.correct': ContextWindowCorrectRequest;
  'models.context_window.reset_runtime_correction': ContextWindowResetRuntimeCorrectionRequest;
  'models.default.set': ModelDefaultSetRequest;
  'models.endpoint.clear_credential': ModelEndpointClearCredentialRequest;
  'models.endpoint.configure_credential': ModelEndpointConfigureCredentialRequest;
  'models.endpoint.create': ModelEndpointCreateRequest;
  'models.endpoint.create_with_credential': ModelEndpointCreateWithCredentialRequest;
  'models.endpoint.delete': ModelEndpointDeleteRequest;
  'models.discovery.start': ModelDiscoveryStartRequest;
  'models.discovery.cancel': ModelDiscoveryCancelRequest;
  'models.endpoint.replace_credential': ModelEndpointReplaceCredentialRequest;
  'models.endpoint.update': ModelEndpointUpdateRequest;
  'models.profile.create_manual': ModelProfileCreateManualRequest;
  'models.profile.create_from_catalog': ModelProfileCreateFromCatalogRequest;
  'models.profile.apply_catalog_update': ModelProfileApplyCatalogUpdateRequest;
  'models.profile.delete': ModelProfileDeleteRequest;
  'models.profile.set_subagent_availability': ModelProfileSetSubagentAvailabilityRequest;
  'models.profile.test_availability': ModelAvailabilityTestStartRequest;
  'models.availability_test.cancel': ModelAvailabilityTestCancelRequest;
  'models.profile.update': ModelProfileUpdateRequest;
  'models.provider.create': ModelProviderCreateRequest;
  'models.provider.delete': ModelProviderDeleteRequest;
  'models.provider.rename': ModelProviderRenameRequest;
  'models.provider.set_connections': ModelProviderConnectionsRequest;
  'models.provider.set_all_current_models_subagent_availability': ModelProviderSetAllCurrentModelsSubagentAvailabilityRequest;
  'models.session.set_binding': SessionModelBindingSetRequest;
  'models.settings.get_snapshot': ModelSettingsGetSnapshotRequest;
};
export type P05MainResult = {
  'provider_icon.read': ProviderIconReadResult;
  'models.catalog.get': ProviderCatalogProjection;
  'models.catalog.ensure_fresh': ProviderCatalogProjection;
  'models.catalog.refresh': ProviderCatalogProjection;
  'models.catalog.provider.get': ProviderCatalogModelPage;
  'models.compatibility.get': EndpointCompatibilitySnapshot;
  'models.compatibility.reset': ModelCompatibilityResetResult;
  'models.context_window.clear_user_override': ContextWindowMutationResult;
  'models.context_window.correct': ContextWindowMutationResult;
  'models.context_window.reset_runtime_correction': ContextWindowMutationResult;
  'models.default.set': DefaultModelMutationResult;
  'models.endpoint.clear_credential': EndpointCredentialMutationResult;
  'models.endpoint.configure_credential': EndpointCredentialMutationResult;
  'models.endpoint.create': ModelEndpointMutationResult;
  'models.endpoint.create_with_credential': EndpointCreatedWithCredentialResult;
  'models.endpoint.delete': ModelDeletionMutationResult;
  'models.discovery.start': ModelDiscoveryStartAccepted;
  'models.discovery.cancel': ModelDiscoveryCancelResult;
  'models.endpoint.replace_credential': EndpointCredentialMutationResult;
  'models.endpoint.update': ModelEndpointMutationResult;
  'models.profile.create_manual': ModelProfileMutationResult;
  'models.profile.create_from_catalog': ModelProfileMutationResult;
  'models.profile.apply_catalog_update': ModelProfileMutationResult;
  'models.profile.delete': ModelDeletionMutationResult;
  'models.profile.set_subagent_availability': ModelProfileMutationResult;
  'models.profile.test_availability': ModelAvailabilityTestStartAccepted;
  'models.availability_test.cancel': ModelAvailabilityTestCancelResult;
  'models.profile.update': ModelProfileMutationResult;
  'models.provider.create': ModelProviderMutationResult;
  'models.provider.delete': ModelDeletionMutationResult;
  'models.provider.rename': ModelProviderMutationResult;
  'models.provider.set_connections': ModelProviderMutationResult;
  'models.provider.set_all_current_models_subagent_availability': ProviderBulkAvailabilityResult;
  'models.session.set_binding': SessionModelBindingMutationResult;
  'models.settings.get_snapshot': ModelSettingsProjection;
};
export type P05MainCommand = keyof P05MainParams;
export type P05CoreParams = {
  'broker.context': ContextBrokerReadRequest;
  'provider_icon.read': ProviderIconReadRequest;
  'models.catalog.get': ModelCatalogGetRequest;
  'models.catalog.ensure_fresh': ModelCatalogEnsureFreshRequest;
  'models.catalog.refresh': ModelCatalogRefreshRequest;
  'models.catalog.provider.get': ModelCatalogProviderGetRequest;
  'models.compatibility.get': ModelCompatibilityGetRequest;
  'models.compatibility.reset': ModelCompatibilityResetRequest;
  'models.context_window.clear_user_override': ContextWindowClearUserOverrideRequest;
  'models.context_window.correct': ContextWindowCorrectRequest;
  'models.context_window.reset_runtime_correction': ContextWindowResetRuntimeCorrectionRequest;
  'models.default.set': ModelDefaultSetRequest;
  'models.endpoint.clear_credential': ModelEndpointClearCredentialRequest;
  'models.endpoint.configure_credential': ModelEndpointConfigureCredentialRequest;
  'models.endpoint.create': ModelEndpointCreateRequest;
  'models.endpoint.create_with_credential': ModelEndpointCreateWithCredentialRequest;
  'models.endpoint.delete': ModelEndpointDeleteRequest;
  'models.discovery.start': ModelDiscoveryStartRequest;
  'models.discovery.cancel': ModelDiscoveryCancelRequest;
  'models.endpoint.replace_credential': ModelEndpointReplaceCredentialRequest;
  'models.endpoint.update': ModelEndpointUpdateRequest;
  'models.profile.create_manual': ModelProfileCreateManualRequest;
  'models.profile.create_from_catalog': ModelProfileCreateFromCatalogRequest;
  'models.profile.apply_catalog_update': ModelProfileApplyCatalogUpdateRequest;
  'models.profile.delete': ModelProfileDeleteRequest;
  'models.profile.set_subagent_availability': ModelProfileSetSubagentAvailabilityRequest;
  'models.profile.test_availability': ModelAvailabilityTestStartRequest;
  'models.availability_test.cancel': ModelAvailabilityTestCancelRequest;
  'models.profile.update': ModelProfileUpdateRequest;
  'models.provider.create': ModelProviderCreateRequest;
  'models.provider.delete': ModelProviderDeleteRequest;
  'models.provider.rename': ModelProviderRenameRequest;
  'models.provider.set_connections': ModelProviderConnectionsRequest;
  'models.provider.set_all_current_models_subagent_availability': ModelProviderSetAllCurrentModelsSubagentAvailabilityRequest;
  'models.session.set_binding': SessionModelBindingSetRequest;
  'models.settings.get_snapshot': ModelSettingsGetSnapshotRequest;
};
export type P05CoreResult = {
  'broker.context': ContextBrokerReadResult;
  'provider_icon.read': ProviderIconReadResult;
  'models.catalog.get': ProviderCatalogProjection;
  'models.catalog.ensure_fresh': ProviderCatalogProjection;
  'models.catalog.refresh': ProviderCatalogProjection;
  'models.catalog.provider.get': ProviderCatalogModelPage;
  'models.compatibility.get': EndpointCompatibilitySnapshot;
  'models.compatibility.reset': ModelCompatibilityResetResult;
  'models.context_window.clear_user_override': ContextWindowMutationResult;
  'models.context_window.correct': ContextWindowMutationResult;
  'models.context_window.reset_runtime_correction': ContextWindowMutationResult;
  'models.default.set': DefaultModelMutationResult;
  'models.endpoint.clear_credential': EndpointCredentialMutationResult;
  'models.endpoint.configure_credential': EndpointCredentialMutationResult;
  'models.endpoint.create': ModelEndpointMutationResult;
  'models.endpoint.create_with_credential': EndpointCreatedWithCredentialResult;
  'models.endpoint.delete': ModelDeletionMutationResult;
  'models.discovery.start': ModelDiscoveryStartAccepted;
  'models.discovery.cancel': ModelDiscoveryCancelResult;
  'models.endpoint.replace_credential': EndpointCredentialMutationResult;
  'models.endpoint.update': ModelEndpointMutationResult;
  'models.profile.create_manual': ModelProfileMutationResult;
  'models.profile.create_from_catalog': ModelProfileMutationResult;
  'models.profile.apply_catalog_update': ModelProfileMutationResult;
  'models.profile.delete': ModelDeletionMutationResult;
  'models.profile.set_subagent_availability': ModelProfileMutationResult;
  'models.profile.test_availability': ModelAvailabilityTestStartAccepted;
  'models.availability_test.cancel': ModelAvailabilityTestCancelResult;
  'models.profile.update': ModelProfileMutationResult;
  'models.provider.create': ModelProviderMutationResult;
  'models.provider.delete': ModelDeletionMutationResult;
  'models.provider.rename': ModelProviderMutationResult;
  'models.provider.set_connections': ModelProviderMutationResult;
  'models.provider.set_all_current_models_subagent_availability': ProviderBulkAvailabilityResult;
  'models.session.set_binding': SessionModelBindingMutationResult;
  'models.settings.get_snapshot': ModelSettingsProjection;
};
export type P05CoreCommand = keyof P05CoreParams;
export type P07MainParams = {
  'tools.tool.set_enabled': ToolSetEnabledRequestV1;
  'tools.instance.update_configuration': ToolInstanceUpdateConfigurationRequestV1;
  'tools.instance.bind_credential': ToolInstanceBindCredentialRequestV1;
  'tools.instance.clear_credential': ToolInstanceClearCredentialRequestV1;
  'tools.pin.workspace.set': WorkspaceToolPinSetRequestV1;
  'tools.pin.workspace.clear': WorkspaceToolPinClearRequestV1;
  'tools.pin.session.set': SessionToolPinSetRequestV1;
  'tools.pin.session.clear': SessionToolPinClearRequestV1;
};
export type P07MainResult = {
  'tools.tool.set_enabled': ToolMutationResultV1;
  'tools.instance.update_configuration': ToolMutationResultV1;
  'tools.instance.bind_credential': ToolMutationResultV1;
  'tools.instance.clear_credential': ToolMutationResultV1;
  'tools.pin.workspace.set': ToolMutationResultV1;
  'tools.pin.workspace.clear': ToolMutationResultV1;
  'tools.pin.session.set': ToolMutationResultV1;
  'tools.pin.session.clear': ToolMutationResultV1;
};
export type P07MainCommand = keyof P07MainParams;
export type P07CoreParams = {
  'broker.tools': ToolBrokerInvokeRequest;
  'tools.tool.set_enabled': ToolSetEnabledRequestV1;
  'tools.instance.update_configuration': ToolInstanceUpdateConfigurationRequestV1;
  'tools.instance.bind_credential': ToolInstanceBindCredentialRequestV1;
  'tools.instance.clear_credential': ToolInstanceClearCredentialRequestV1;
  'tools.pin.workspace.set': WorkspaceToolPinSetRequestV1;
  'tools.pin.workspace.clear': WorkspaceToolPinClearRequestV1;
  'tools.pin.session.set': SessionToolPinSetRequestV1;
  'tools.pin.session.clear': SessionToolPinClearRequestV1;
};
export type P07CoreResult = {
  'broker.tools': ToolBrokerInvokeResult;
  'tools.tool.set_enabled': ToolMutationResultV1;
  'tools.instance.update_configuration': ToolMutationResultV1;
  'tools.instance.bind_credential': ToolMutationResultV1;
  'tools.instance.clear_credential': ToolMutationResultV1;
  'tools.pin.workspace.set': ToolMutationResultV1;
  'tools.pin.workspace.clear': ToolMutationResultV1;
  'tools.pin.session.set': ToolMutationResultV1;
  'tools.pin.session.clear': ToolMutationResultV1;
};
export type P07CoreCommand = keyof P07CoreParams;
export type P07ReadParams = {
  'tools.credential.candidates': ToolCredentialCandidatesRequestV1;
};
export type P07ReadResult = {
  'tools.credential.candidates': ToolCredentialCandidatesProjectionV1;
};
export type P07ReadCommand = keyof P07ReadParams;
export type GitMainParams = {
  'git.runtime.get': EmptyToolInput;
  'git.workspace.read': GitWorkspaceReadRequestV1;
  'git.installation.open': EmptyToolInput;
};
export type GitMainResult = {
  'git.runtime.get': GitRuntimeStatusV1;
  'git.workspace.read': GitReadResultV1;
  'git.installation.open': GitInstallationResultV1;
};
export type GitMainCommand = keyof GitMainParams;
export type AgentMainParams = {
  'agent.tools.settings': AgentToolSettingsRequest;
  'agent.tools.configure': AgentToolSettingsUpdate;
  'agent.tools.network_proxy.test': AgentNetworkProxyTestRequest;
  'agent.context.window.resolve': AgentContextWindowResolveRequest;
  'agent.context.remediation': AgentRunRequest;
  'agent.context.recheck': AgentContextRecheckRequest;
  'agent.compaction.request': AgentCompactionRequest;
  'agent.compaction.status': AgentRunRequest;
  'agent.message.text': AgentMessageTextRequest;
  'agent.query.select_file': AgentQueryFileRequest;
  'agent.related.start': AgentRelatedTurnRequest;
  'agent.inspector': AgentInspectorRequest;
  'agent.history': AgentHistoryRequest;
  'agent.session.branch': AgentSessionBranchRequest;
  'agent.result': AgentResultRequest;
  'agent.sessions.activity': AgentSessionsActivityRequest;
  'agent.inbox': AgentSessionRequest;
  'agent.submit': AgentSubmitRequest;
  'agent.inbox.mutate': AgentInboxMutation;
  'agent.shell.query': ShellQueryRequest;
  'agent.shell.stop': ShellStopRequest;
  'agent.tool_activity': AgentToolActivityRequest;
  'agent.activity.query': ActivityQueryRequestV1;
  'agent.commands.catalog': CommandCatalogRequest;
  'agent.commands.set_enabled': CommandSetEnabledRequest;
  'agent.commands.delete': CommandDeleteRequest;
  'agent.commands.open_directory': CommandDirectoryRequest;
  'agent.files.open_outside': WorkspaceFileOpenRequest;
  'agent.links.open': ExternalLinkOpenRequest;
  'agent.clipboard.write': ClipboardWriteRequest;
  'agent.workspace.rebuild_index': WorkspaceIndexRebuildRequest;
  'agent.start': AgentStartRequest;
  'agent.init.start': AgentInitStartRequest;
  'agent.compaction.start': AgentCompactionStartRequest;
  'agent.status': AgentRunRequest;
  'agent.cancel': AgentRunRequest;
  'agent.mode.control': AgentModeControlRequest;
  'agent.query.answer': AgentQueryAnswerRequest;
  'agent.query.touch': AgentQueryTouchRequest;
  'agent.approval.get': ExecutionApprovalRequest;
  'agent.approval.resolve': ExecutionApprovalResolutionRequest;
  'agent.hooks.list': HookListRequest;
  'agent.hooks.preview': HookPreviewRequest;
  'agent.hooks.invocations': HookInvocationListRequest;
  'agent.hooks.trust': HookTrustRequest;
  'agent.hooks.untrust': HookPathRequest;
  'agent.hooks.set_enabled': HookSetEnabledRequest;
  'agent.hooks.remove': HookPathRequest;
  'agent.hooks.move': HookMoveRequest;
  'agent.hooks.select_directory': HookSelectDirectoryRequest;
  'agent.mcp.list': McpListRequest;
  'agent.mcp.preview': McpPreviewRequest;
  'agent.mcp.describe': McpDescribeRequest;
  'agent.mcp.save': McpSaveRequest;
  'agent.mcp.trust': McpTrustRequest;
  'agent.mcp.untrust': McpServerRequest;
  'agent.mcp.set_enabled': McpSetEnabledRequest;
  'agent.mcp.restore_plugin_default': McpServerRequest;
  'agent.mcp.remove': McpServerRequest;
  'agent.mcp.refresh': McpRefreshRequest;
  'agent.mcp.secret.set': McpSecretSetRequest;
  'agent.mcp.oauth.sign_in': McpSignInRequest;
  'agent.mcp.oauth.sign_out': McpSignOutRequest;
  'agent.plugins.list': PluginListRequest;
  'agent.plugins.preview': PluginRequest;
  'agent.plugins.import': PluginImportRequest;
  'agent.plugins.trust': PluginTrustRequest;
  'agent.plugins.untrust': PluginChangeRequest;
  'agent.plugins.set_enabled': PluginSetEnabledRequest;
  'agent.plugins.discard_update': PluginChangeRequest;
  'agent.plugins.uninstall': PluginChangeRequest;
  'agent.plugins.select_directory': PluginSelectDirectoryRequest;
  'agent.plugins.select_archive': PluginSelectArchiveRequest;
  'agent.plugins.open_directory': PluginRequest;
  'agent.skills.list': SkillListRequest;
  'agent.skills.preview': SkillRequest;
  'agent.skills.trust': SkillTrustRequest;
  'agent.skills.untrust': SkillChangeRequest;
  'agent.skills.set_enabled': SkillSetEnabledRequest;
  'agent.skills.import': SkillImportRequest;
  'agent.skills.remove': SkillChangeRequest;
  'agent.skills.select_directory': SkillSelectDirectoryRequest;
  'agent.skills.select_archive': SkillSelectArchiveRequest;
  'agent.skills.open_directory': SkillRequest;
  'terminal.user.control': TerminalControlRequestV1;
  'terminal.user.open': UserTerminalOpenRequestV1;
  'terminal.user.list': TerminalSessionRequestV1;
  'terminal.user.write': UserTerminalWriteRequestV1;
  'terminal.user.resize': TerminalResizeRequestV1;
  'terminal.user.close': TerminalRequestV1;
  'terminal.user.subscribe': TerminalSubscribeRequestV1;
  'terminal.user.sensitive.pending': ManagedProcessRequestV1;
  'terminal.user.sensitive.submit': TerminalSensitiveReplyV1;
  'terminal.user.sensitive.cancel': TerminalSensitiveReplyV1;
  'agent.process.list': ManagedProcessRequestV1;
  'agent.process.stop': ManagedProcessStopRequestV1;
  'ui.settings.get': EmptyToolInput;
  'ui.settings.update': UiSettingsUpdate;
  'browser.settings.get': EmptyToolInput;
  'browser.settings.update': BrowserSettingsUpdate;
  'browser.screenshot.read': BrowserScreenshotReadRequest;
  'chrome.status': EmptyToolInput;
  'chrome.set_enabled': ChromeSetEnabledRequest;
  'chrome.pair.begin': EmptyToolInput;
  'chrome.revoke': EmptyToolInput;
  'chrome.stop': ChromeSessionRequest;
  'chrome.shares': ChromeSessionRequest;
  'chrome.assist.finish': ChromeSessionRequest;
  'chrome.extension.open_folder': EmptyToolInput;
  'agent.attachments.text': AttachmentTextRequest;
};
export type AgentMainResult = {
  'agent.tools.settings': AgentToolSettingsView;
  'agent.tools.configure': AgentToolSettingsView;
  'agent.tools.network_proxy.test': AgentNetworkProxyTestResult;
  'agent.context.window.resolve': AgentContextRemediationView;
  'agent.context.remediation': AgentContextRemediationView;
  'agent.context.recheck': AgentContextRemediationView;
  'agent.compaction.request': AgentCompactionStatus;
  'agent.compaction.status': AgentCompactionStatus;
  'agent.message.text': AgentMessageTextPage;
  'agent.query.select_file': AgentQueryFileSelection;
  'agent.related.start': AgentRunProjection;
  'agent.inspector': AgentPromptInspection;
  'agent.history': AgentSessionProjection;
  'agent.session.branch': AgentSessionBranchResult;
  'agent.result': AgentResultView;
  'agent.sessions.activity': AgentSessionsActivity;
  'agent.inbox': AgentInboxProjection;
  'agent.submit': AgentInboxProjection;
  'agent.inbox.mutate': AgentInboxProjection;
  'agent.shell.query': ShellExecutionView;
  'agent.shell.stop': ShellExecutionView;
  'agent.tool_activity': AgentToolActivityPage;
  'agent.activity.query': ActivityQueryResponseV1;
  'agent.commands.catalog': CommandCatalog;
  'agent.commands.set_enabled': CommandCatalog;
  'agent.commands.delete': CommandCatalog;
  'agent.commands.open_directory': CommandDirectoryOpened;
  'agent.files.open_outside': OpenedOutside;
  'agent.links.open': OpenedOutside;
  'agent.clipboard.write': ClipboardWritten;
  'agent.workspace.rebuild_index': WorkspaceIndexRebuildStarted;
  'agent.start': AgentRunProjection;
  'agent.init.start': AgentRunProjection;
  'agent.compaction.start': AgentRunProjection;
  'agent.status': AgentRunProjection;
  'agent.cancel': AgentRunProjection;
  'agent.mode.control': AgentModeEpisodeView;
  'agent.query.answer': AgentRunProjection;
  'agent.query.touch': AgentQueryTouchView;
  'agent.approval.get': ExecutionApprovalView;
  'agent.approval.resolve': AgentRunProjection;
  'agent.hooks.list': HookRegistryView;
  'agent.hooks.preview': HookPreview;
  'agent.hooks.invocations': HookInvocationList;
  'agent.hooks.trust': HookRegistryView;
  'agent.hooks.untrust': HookRegistryView;
  'agent.hooks.set_enabled': HookRegistryView;
  'agent.hooks.remove': HookRegistryView;
  'agent.hooks.move': HookRegistryView;
  'agent.hooks.select_directory': HookSelectedDirectory;
  'agent.mcp.list': McpRegistryView;
  'agent.mcp.preview': McpPreview;
  'agent.mcp.describe': McpServerDetail;
  'agent.mcp.save': McpRegistryView;
  'agent.mcp.trust': McpRegistryView;
  'agent.mcp.untrust': McpRegistryView;
  'agent.mcp.set_enabled': McpRegistryView;
  'agent.mcp.restore_plugin_default': McpRegistryView;
  'agent.mcp.remove': McpRegistryView;
  'agent.mcp.refresh': McpServerDetail;
  'agent.mcp.secret.set': McpSecretSetResult;
  'agent.mcp.oauth.sign_in': McpSignInBegun;
  'agent.mcp.oauth.sign_out': McpRegistryView;
  'agent.plugins.list': PluginRegistryView;
  'agent.plugins.preview': PluginPreview;
  'agent.plugins.import': PluginRegistryView;
  'agent.plugins.trust': PluginRegistryView;
  'agent.plugins.untrust': PluginRegistryView;
  'agent.plugins.set_enabled': PluginRegistryView;
  'agent.plugins.discard_update': PluginRegistryView;
  'agent.plugins.uninstall': PluginRegistryView;
  'agent.plugins.select_directory': PluginSelectedDirectory;
  'agent.plugins.select_archive': PluginSelectedArchive;
  'agent.plugins.open_directory': PluginDirectoryOpened;
  'agent.skills.list': SkillRegistryView;
  'agent.skills.preview': SkillPreview;
  'agent.skills.trust': SkillRegistryView;
  'agent.skills.untrust': SkillRegistryView;
  'agent.skills.set_enabled': SkillRegistryView;
  'agent.skills.import': SkillRegistryView;
  'agent.skills.remove': SkillRegistryView;
  'agent.skills.select_directory': SkillSelectedDirectory;
  'agent.skills.select_archive': SkillSelectedArchive;
  'agent.skills.open_directory': SkillDirectoryOpened;
  'terminal.user.control': TerminalViewV1;
  'terminal.user.open': TerminalViewV1;
  'terminal.user.list': TerminalListV1;
  'terminal.user.write': TerminalViewV1;
  'terminal.user.resize': TerminalViewV1;
  'terminal.user.close': TerminalViewV1;
  'terminal.user.subscribe': TerminalOutputBatchV1;
  'terminal.user.sensitive.pending': TerminalSensitivePendingV1;
  'terminal.user.sensitive.submit': TerminalSensitiveResultV1;
  'terminal.user.sensitive.cancel': TerminalSensitiveResultV1;
  'agent.process.list': ManagedProcessListV1;
  'agent.process.stop': ManagedProcessStopResultV1;
  'ui.settings.get': UiSettingsSnapshot;
  'ui.settings.update': UiSettingsSnapshot;
  'browser.settings.get': BrowserSettingsSnapshot;
  'browser.settings.update': BrowserSettingsSnapshot;
  'browser.screenshot.read': BrowserScreenshotImageV1;
  'chrome.status': ChromeControlStatus;
  'chrome.set_enabled': ChromeControlStatus;
  'chrome.pair.begin': ChromePairingCode;
  'chrome.revoke': ChromeControlStatus;
  'chrome.stop': ChromeSessionShares;
  'chrome.shares': ChromeSessionShares;
  'chrome.assist.finish': ChromeSessionShares;
  'chrome.extension.open_folder': ChromeExtensionFolder;
  'agent.attachments.text': AttachmentText;
};
export type AgentMainCommand = keyof AgentMainParams;
export type PackageMainParams = {
  'packages.list': EmptyToolInput;
  'packages.import_directory': EmptyToolInput;
  'packages.import_archive': EmptyToolInput;
  'packages.version.get': PackageVersionRefV1;
  'packages.conformance.prepare': PackageConformancePrepareRequestV1;
  'packages.conformance.start': PackageConformanceStartRequestV1;
  'packages.conformance.get': PackageVersionRefV1;
  'packages.conformance.cancel': PackageConformanceCancelRequestV1;
  'packages.install': PackageInstallRequestV1;
  'packages.set_enabled': PackageEnableRequestV1;
  'packages.revoke_trust': PackageChangeRequestV1;
  'packages.uninstall': PackageChangeRequestV1;
  'packages.credential.bind': PackageCredentialBindRequestV1;
  'packages.credential.clear': PackageCredentialClearRequestV1;
};
export type PackageMainResult = {
  'packages.list': PackageListV1;
  'packages.import_directory': PackageImportOutcomeV1;
  'packages.import_archive': PackageImportOutcomeV1;
  'packages.version.get': PackageVersionViewV1;
  'packages.conformance.prepare': PackageConformancePreparationV1;
  'packages.conformance.start': PackageConformanceViewV1;
  'packages.conformance.get': PackageConformanceViewV1;
  'packages.conformance.cancel': PackageConformanceViewV1;
  'packages.install': PackageSummaryV1;
  'packages.set_enabled': PackageSummaryV1;
  'packages.revoke_trust': PackageSummaryV1;
  'packages.uninstall': PackageSummaryV1;
  'packages.credential.bind': PackageSummaryV1;
  'packages.credential.clear': PackageSummaryV1;
};
export type PackageMainCommand = keyof PackageMainParams;
export type MemoryMainParams = {
  'memory.vaults.get': MemoryDesktopRequest;
  'memory.settings.get': EmptyToolInput;
  'memory.settings.update': MemorySettingsUpdate;
  'memory.vault.validate': MemoryVaultRequest;
  'memory.vault.rebuild': MemoryVaultRequest;
  'memory.vault.repair.preview': MemoryVaultRequest;
  'memory.vault.repair.apply': MemoryDesktopRepairApply;
  'memory.vault.open_directory': MemoryVaultRequest;
  'memory.maintenance.open_log': MemoryVaultRequest;
  'memory.vault.export': MemoryVaultRequest;
};
export type MemoryMainResult = {
  'memory.vaults.get': MemoryDesktopSnapshot;
  'memory.settings.get': MemorySettingsSnapshot;
  'memory.settings.update': MemorySettingsSnapshot;
  'memory.vault.validate': MemoryValidation;
  'memory.vault.rebuild': MemoryVaultStatus;
  'memory.vault.repair.preview': MemoryRepairPreview;
  'memory.vault.repair.apply': MemoryRepairResult;
  'memory.vault.open_directory': MemoryDesktopActionResult;
  'memory.maintenance.open_log': MemoryDesktopActionResult;
  'memory.vault.export': MemoryDesktopActionResult;
};
export type MemoryMainCommand = keyof MemoryMainParams;
export type GitCoreParams = {
  'git.runtime.get': EmptyToolInput;
  'git.workspace.read': GitWorkspaceReadRequestV1;
};
export type GitCoreResult = {
  'git.runtime.get': GitRuntimeStatusV1;
  'git.workspace.read': GitReadResultV1;
};
export type GitCoreCommand = keyof GitCoreParams;
export type AgentCoreParams = {
  'agent.tools.settings': AgentToolSettingsRequest;
  'agent.tools.configure': AgentToolSettingsUpdate;
  'agent.tools.network_proxy.test': AgentNetworkProxyTestRequest;
  'agent.context.window.resolve': AgentContextWindowResolveRequest;
  'agent.context.remediation': AgentRunRequest;
  'agent.context.recheck': AgentContextRecheckRequest;
  'agent.compaction.request': AgentCompactionRequest;
  'agent.compaction.status': AgentRunRequest;
  'agent.message.text': AgentMessageTextRequest;
  'agent.query.select_file': AgentQueryFileRequest;
  'agent.related.start': AgentRelatedTurnRequest;
  'agent.inspector': AgentInspectorRequest;
  'agent.history': AgentHistoryRequest;
  'agent.session.branch': AgentSessionBranchRequest;
  'agent.result': AgentResultRequest;
  'agent.sessions.activity': AgentSessionsActivityRequest;
  'agent.inbox': AgentSessionRequest;
  'agent.submit': AgentSubmitRequest;
  'agent.inbox.mutate': AgentInboxMutation;
  'agent.shell.query': ShellQueryRequest;
  'agent.shell.stop': ShellStopRequest;
  'agent.tool_activity': AgentToolActivityRequest;
  'agent.activity.query': ActivityQueryRequestV1;
  'agent.workspace.rebuild_index': WorkspaceIndexRebuildRequest;
  'agent.commands.catalog': CommandCatalogRequest;
  'agent.commands.set_enabled': CommandSetEnabledRequest;
  'agent.commands.delete': CommandDeleteRequest;
  'agent.start': AgentStartRequest;
  'agent.init.start': AgentInitStartRequest;
  'agent.compaction.start': AgentCompactionStartRequest;
  'agent.status': AgentRunRequest;
  'agent.cancel': AgentRunRequest;
  'agent.mode.control': AgentModeControlRequest;
  'agent.query.answer': AgentQueryAnswerRequest;
  'agent.query.touch': AgentQueryTouchRequest;
  'agent.approval.get': ExecutionApprovalRequest;
  'agent.approval.resolve': ExecutionApprovalResolutionRequest;
  'agent.hooks.list': HookListRequest;
  'agent.hooks.preview': HookPreviewRequest;
  'agent.hooks.invocations': HookInvocationListRequest;
  'agent.hooks.trust': HookTrustRequest;
  'agent.hooks.untrust': HookPathRequest;
  'agent.hooks.set_enabled': HookSetEnabledRequest;
  'agent.hooks.remove': HookPathRequest;
  'agent.hooks.move': HookMoveRequest;
  'agent.mcp.list': McpListRequest;
  'agent.mcp.preview': McpPreviewRequest;
  'agent.mcp.describe': McpDescribeRequest;
  'agent.mcp.save': McpSaveRequest;
  'agent.mcp.trust': McpTrustRequest;
  'agent.mcp.untrust': McpServerRequest;
  'agent.mcp.set_enabled': McpSetEnabledRequest;
  'agent.mcp.restore_plugin_default': McpServerRequest;
  'agent.mcp.remove': McpServerRequest;
  'agent.mcp.refresh': McpRefreshRequest;
  'agent.mcp.secret.set': McpSecretSetRequest;
  'agent.mcp.oauth.sign_in': McpSignInRequest;
  'agent.mcp.oauth.sign_out': McpSignOutRequest;
  'agent.plugins.list': PluginListRequest;
  'agent.plugins.preview': PluginRequest;
  'agent.plugins.import': PluginImportRequest;
  'agent.plugins.trust': PluginTrustRequest;
  'agent.plugins.untrust': PluginChangeRequest;
  'agent.plugins.set_enabled': PluginSetEnabledRequest;
  'agent.plugins.discard_update': PluginChangeRequest;
  'agent.plugins.uninstall': PluginChangeRequest;
  'agent.skills.list': SkillListRequest;
  'agent.skills.preview': SkillRequest;
  'agent.skills.trust': SkillTrustRequest;
  'agent.skills.untrust': SkillChangeRequest;
  'agent.skills.set_enabled': SkillSetEnabledRequest;
  'agent.skills.import': SkillImportRequest;
  'agent.skills.remove': SkillChangeRequest;
  'terminal.user.control': TerminalControlRequestV1;
  'terminal.user.open': UserTerminalOpenRequestV1;
  'terminal.user.list': TerminalSessionRequestV1;
  'terminal.user.write': UserTerminalWriteRequestV1;
  'terminal.user.resize': TerminalResizeRequestV1;
  'terminal.user.close': TerminalRequestV1;
  'terminal.user.subscribe': TerminalSubscribeRequestV1;
  'terminal.user.sensitive.pending': ManagedProcessRequestV1;
  'terminal.user.sensitive.submit': TerminalSensitiveReplyV1;
  'terminal.user.sensitive.cancel': TerminalSensitiveReplyV1;
  'agent.process.list': ManagedProcessRequestV1;
  'agent.process.stop': ManagedProcessStopRequestV1;
  'ui.settings.get': EmptyToolInput;
  'ui.settings.update': UiSettingsUpdate;
  'browser.settings.get': EmptyToolInput;
  'browser.settings.update': BrowserSettingsUpdate;
  'browser.screenshot.read': BrowserScreenshotReadRequest;
  'browser.host.result': BrowserHostResultV1;
  'chrome.status': EmptyToolInput;
  'chrome.set_enabled': ChromeSetEnabledRequest;
  'chrome.pair.begin': EmptyToolInput;
  'chrome.revoke': EmptyToolInput;
  'chrome.stop': ChromeSessionRequest;
  'chrome.shares': ChromeSessionRequest;
  'chrome.assist.finish': ChromeSessionRequest;
  'media.host.read': MediaHostReadRequestV1;
  'media.host.result': MediaHostResultV1;
  'agent.attachments.text': AttachmentTextRequest;
  'agent.attachments.locate': AttachmentLocateRequest;
  'agent.attachments.entry': AttachmentEntryRequest;
  'agent.files.mentioned': MentionedFileRequest;
};
export type AgentCoreResult = {
  'agent.tools.settings': AgentToolSettingsView;
  'agent.tools.configure': AgentToolSettingsView;
  'agent.tools.network_proxy.test': AgentNetworkProxyTestResult;
  'agent.context.window.resolve': AgentContextRemediationView;
  'agent.context.remediation': AgentContextRemediationView;
  'agent.context.recheck': AgentContextRemediationView;
  'agent.compaction.request': AgentCompactionStatus;
  'agent.compaction.status': AgentCompactionStatus;
  'agent.message.text': AgentMessageTextPage;
  'agent.query.select_file': AgentQueryFileSelection;
  'agent.related.start': AgentRunProjection;
  'agent.inspector': AgentPromptInspection;
  'agent.history': AgentSessionProjection;
  'agent.session.branch': AgentSessionBranchResult;
  'agent.result': AgentResultView;
  'agent.sessions.activity': AgentSessionsActivity;
  'agent.inbox': AgentInboxProjection;
  'agent.submit': AgentInboxProjection;
  'agent.inbox.mutate': AgentInboxProjection;
  'agent.shell.query': ShellExecutionView;
  'agent.shell.stop': ShellExecutionView;
  'agent.tool_activity': AgentToolActivityPage;
  'agent.activity.query': ActivityQueryResponseV1;
  'agent.workspace.rebuild_index': WorkspaceIndexRebuildStarted;
  'agent.commands.catalog': CommandCatalog;
  'agent.commands.set_enabled': CommandCatalog;
  'agent.commands.delete': CommandCatalog;
  'agent.start': AgentRunProjection;
  'agent.init.start': AgentRunProjection;
  'agent.compaction.start': AgentRunProjection;
  'agent.status': AgentRunProjection;
  'agent.cancel': AgentRunProjection;
  'agent.mode.control': AgentModeEpisodeView;
  'agent.query.answer': AgentRunProjection;
  'agent.query.touch': AgentQueryTouchView;
  'agent.approval.get': ExecutionApprovalView;
  'agent.approval.resolve': AgentRunProjection;
  'agent.hooks.list': HookRegistryView;
  'agent.hooks.preview': HookPreview;
  'agent.hooks.invocations': HookInvocationList;
  'agent.hooks.trust': HookRegistryView;
  'agent.hooks.untrust': HookRegistryView;
  'agent.hooks.set_enabled': HookRegistryView;
  'agent.hooks.remove': HookRegistryView;
  'agent.hooks.move': HookRegistryView;
  'agent.mcp.list': McpRegistryView;
  'agent.mcp.preview': McpPreview;
  'agent.mcp.describe': McpServerDetail;
  'agent.mcp.save': McpRegistryView;
  'agent.mcp.trust': McpRegistryView;
  'agent.mcp.untrust': McpRegistryView;
  'agent.mcp.set_enabled': McpRegistryView;
  'agent.mcp.restore_plugin_default': McpRegistryView;
  'agent.mcp.remove': McpRegistryView;
  'agent.mcp.refresh': McpServerDetail;
  'agent.mcp.secret.set': McpSecretSetResult;
  'agent.mcp.oauth.sign_in': McpSignInBegun;
  'agent.mcp.oauth.sign_out': McpRegistryView;
  'agent.plugins.list': PluginRegistryView;
  'agent.plugins.preview': PluginPreview;
  'agent.plugins.import': PluginRegistryView;
  'agent.plugins.trust': PluginRegistryView;
  'agent.plugins.untrust': PluginRegistryView;
  'agent.plugins.set_enabled': PluginRegistryView;
  'agent.plugins.discard_update': PluginRegistryView;
  'agent.plugins.uninstall': PluginRegistryView;
  'agent.skills.list': SkillRegistryView;
  'agent.skills.preview': SkillPreview;
  'agent.skills.trust': SkillRegistryView;
  'agent.skills.untrust': SkillRegistryView;
  'agent.skills.set_enabled': SkillRegistryView;
  'agent.skills.import': SkillRegistryView;
  'agent.skills.remove': SkillRegistryView;
  'terminal.user.control': TerminalViewV1;
  'terminal.user.open': TerminalViewV1;
  'terminal.user.list': TerminalListV1;
  'terminal.user.write': TerminalViewV1;
  'terminal.user.resize': TerminalViewV1;
  'terminal.user.close': TerminalViewV1;
  'terminal.user.subscribe': TerminalOutputBatchV1;
  'terminal.user.sensitive.pending': TerminalSensitivePendingV1;
  'terminal.user.sensitive.submit': TerminalSensitiveResultV1;
  'terminal.user.sensitive.cancel': TerminalSensitiveResultV1;
  'agent.process.list': ManagedProcessListV1;
  'agent.process.stop': ManagedProcessStopResultV1;
  'ui.settings.get': UiSettingsSnapshot;
  'ui.settings.update': UiSettingsSnapshot;
  'browser.settings.get': BrowserSettingsSnapshot;
  'browser.settings.update': BrowserSettingsSnapshot;
  'browser.screenshot.read': BrowserScreenshotImageV1;
  'browser.host.result': BrowserHostResultAck;
  'chrome.status': ChromeControlStatus;
  'chrome.set_enabled': ChromeControlStatus;
  'chrome.pair.begin': ChromePairingCode;
  'chrome.revoke': ChromeControlStatus;
  'chrome.stop': ChromeSessionShares;
  'chrome.shares': ChromeSessionShares;
  'chrome.assist.finish': ChromeSessionShares;
  'media.host.read': MediaHostReadResultV1;
  'media.host.result': MediaHostResultAck;
  'agent.attachments.text': AttachmentText;
  'agent.attachments.locate': AttachmentLocation;
  'agent.attachments.entry': AttachmentLocation;
  'agent.files.mentioned': AttachmentLocation;
};
export type AgentCoreCommand = keyof AgentCoreParams;
export type PackageCoreParams = {
  'packages.list': EmptyToolInput;
  'packages.import_directory': PackageImportSourceV1;
  'packages.import_archive': PackageImportSourceV1;
  'packages.version.get': PackageVersionRefV1;
  'packages.conformance.prepare': PackageConformancePrepareRequestV1;
  'packages.conformance.start': PackageConformanceStartRequestV1;
  'packages.conformance.get': PackageVersionRefV1;
  'packages.conformance.cancel': PackageConformanceCancelRequestV1;
  'packages.install': PackageInstallRequestV1;
  'packages.set_enabled': PackageEnableRequestV1;
  'packages.revoke_trust': PackageChangeRequestV1;
  'packages.uninstall': PackageChangeRequestV1;
  'packages.credential.bind': PackageCredentialBindRequestV1;
  'packages.credential.clear': PackageCredentialClearRequestV1;
};
export type PackageCoreResult = {
  'packages.list': PackageListV1;
  'packages.import_directory': PackageImportOutcomeV1;
  'packages.import_archive': PackageImportOutcomeV1;
  'packages.version.get': PackageVersionViewV1;
  'packages.conformance.prepare': PackageConformancePreparationV1;
  'packages.conformance.start': PackageConformanceViewV1;
  'packages.conformance.get': PackageConformanceViewV1;
  'packages.conformance.cancel': PackageConformanceViewV1;
  'packages.install': PackageSummaryV1;
  'packages.set_enabled': PackageSummaryV1;
  'packages.revoke_trust': PackageSummaryV1;
  'packages.uninstall': PackageSummaryV1;
  'packages.credential.bind': PackageSummaryV1;
  'packages.credential.clear': PackageSummaryV1;
};
export type PackageCoreCommand = keyof PackageCoreParams;
export type MemoryCoreParams = {
  'memory.vaults.get': MemoryDesktopRequest;
  'memory.settings.get': EmptyToolInput;
  'memory.settings.update': MemorySettingsUpdate;
  'memory.vault.validate': MemoryVaultRequest;
  'memory.vault.rebuild': MemoryVaultRequest;
  'memory.vault.repair.preview': MemoryVaultRequest;
  'memory.vault.repair.apply': MemoryDesktopRepairApply;
  'memory.export.prepare': MemoryVaultRequest;
  'memory.export.entry': MemoryExportEntryRequest;
  'memory.export.chunk': MemoryExportChunkRequest;
  'memory.export.close': MemoryExportCloseRequest;
};
export type MemoryCoreResult = {
  'memory.vaults.get': MemoryDesktopSnapshot;
  'memory.settings.get': MemorySettingsSnapshot;
  'memory.settings.update': MemorySettingsSnapshot;
  'memory.vault.validate': MemoryValidation;
  'memory.vault.rebuild': MemoryVaultStatus;
  'memory.vault.repair.preview': MemoryRepairPreview;
  'memory.vault.repair.apply': MemoryRepairResult;
  'memory.export.prepare': MemoryExportHandle;
  'memory.export.entry': MemoryExportEntry;
  'memory.export.chunk': MemoryExportChunk;
  'memory.export.close': MemoryDesktopActionResult;
};
export type MemoryCoreCommand = keyof MemoryCoreParams;
export type ConfigRecoveryMainParams = {
  'config.recovery.notice': Record<string, never>;
  'config.recovery.plan': ConfigRecoveryPlanRequest;
  'config.recovery.apply': ConfigRecoveryApplyRequest;
};
export type ConfigRecoveryMainResult = {
  'config.recovery.notice': RecoveryNotice;
  'config.recovery.plan': BundleRecoveryPlan;
  'config.recovery.apply': RecoveryReport;
};
export type ConfigRecoveryMainCommand = keyof ConfigRecoveryMainParams;
export type ConfigRecoveryCoreParams = {
  'config.recovery.notice': Record<string, never>;
  'config.recovery.plan': ConfigRecoveryPlanRequest;
  'config.recovery.apply': ConfigRecoveryApplyRequest;
};
export type ConfigRecoveryCoreResult = {
  'config.recovery.notice': RecoveryNotice;
  'config.recovery.plan': BundleRecoveryPlan;
  'config.recovery.apply': RecoveryReport;
};
export type ConfigRecoveryCoreCommand = keyof ConfigRecoveryCoreParams;
export type ToolCredentialMainParams = {
  'tools.credential.outcome': ToolCredentialOutcomeRequestV1;
  'tools.credential.create': ToolCredentialCreateRequestV1;
  'tools.credential.rotate': ToolCredentialChangeRequestV1;
  'tools.credential.clear': ToolCredentialChangeRequestV1;
  'tools.credential.origins': ToolCredentialOriginsRequestV1;
  'tools.credential.recover': ToolCredentialChangeRequestV1;
  'tools.credential.list': ToolCredentialManagementTargetV1;
};
export type ToolCredentialMainResult = {
  'tools.credential.outcome': ToolCredentialOutcomeV1;
  'tools.credential.create': ToolCredentialMutationResultV1;
  'tools.credential.rotate': ToolCredentialMutationResultV1;
  'tools.credential.clear': ToolCredentialMutationResultV1;
  'tools.credential.origins': ToolCredentialMutationResultV1;
  'tools.credential.recover': ToolCredentialMutationResultV1;
  'tools.credential.list': ToolCredentialListResultV1;
};
export type ToolCredentialMainCommand = keyof ToolCredentialMainParams;
export type ToolCredentialCoreParams = {
  'tools.credential.outcome': ToolCredentialOutcomeRequestV1;
  'tools.credential.create': ToolCredentialCreateRequestV1;
  'tools.credential.rotate': ToolCredentialChangeRequestV1;
  'tools.credential.clear': ToolCredentialChangeRequestV1;
  'tools.credential.origins': ToolCredentialOriginsRequestV1;
  'tools.credential.recover': ToolCredentialChangeRequestV1;
  'tools.credential.list': ToolCredentialManagementTargetV1;
};
export type ToolCredentialCoreResult = {
  'tools.credential.outcome': ToolCredentialOutcomeV1;
  'tools.credential.create': ToolCredentialMutationResultV1;
  'tools.credential.rotate': ToolCredentialMutationResultV1;
  'tools.credential.clear': ToolCredentialMutationResultV1;
  'tools.credential.origins': ToolCredentialMutationResultV1;
  'tools.credential.recover': ToolCredentialMutationResultV1;
  'tools.credential.list': ToolCredentialListResultV1;
};
export type ToolCredentialCoreCommand = keyof ToolCredentialCoreParams;
export type RuntimeMainParams = {
  'runtime.readiness': Record<string, never>;
};
export type RuntimeMainResult = {
  'runtime.readiness': RuntimeReadiness;
};
export type RuntimeMainCommand = keyof RuntimeMainParams;
export type P05SecretMainCommand =
  | 'models.endpoint.configure_credential'
  | 'models.endpoint.create_with_credential'
  | 'models.endpoint.replace_credential';
export type P05NonSecretMainCommand = Exclude<P05MainCommand, P05SecretMainCommand>;
export const MAIN_ROUTES = {
  'session.rename': {
    method: 'session.rename',
    owner: 'workspace',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'SessionRenameRequest',
    response_type: 'SessionSummary',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'workspace.select_directory': {
    method: 'workspace.select_directory',
    owner: 'workspace',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'WorkspaceSelectDirectoryRequest',
    response_type: 'WorkspaceDirectorySelection',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'session.archive': {
    method: 'session.archive',
    owner: 'workspace',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'SessionLifecycleRequest',
    response_type: 'SessionSummary',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'session.restore': {
    method: 'session.restore',
    owner: 'workspace',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'SessionLifecycleRequest',
    response_type: 'SessionSummary',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'session.delete': {
    method: 'session.delete',
    owner: 'workspace',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'SessionLifecycleRequest',
    response_type: 'SessionDeleteResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'session.create': {
    method: 'session.create',
    owner: 'workspace',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'SessionCreateRequest',
    response_type: 'SessionSummary',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'fs.apply_changeset': {
    method: 'fs.apply_changeset',
    owner: 'workspace',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'FsApplyChangesetRequest',
    response_type: 'FsApplyChangesetResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'fs.apply_patch': {
    method: 'fs.apply_patch',
    owner: 'workspace',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'FsApplyPatchRequest',
    response_type: 'FsMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'fs.copy': {
    method: 'fs.copy',
    owner: 'workspace',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'FsCopyRequest',
    response_type: 'FsMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'fs.create_directory': {
    method: 'fs.create_directory',
    owner: 'workspace',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'FsCreateDirectoryRequest',
    response_type: 'FsMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'fs.delete': {
    method: 'fs.delete',
    owner: 'workspace',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'FsDeleteRequest',
    response_type: 'FsMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'fs.find_paths': {
    method: 'fs.find_paths',
    owner: 'workspace',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'FsFindPathsRequest',
    response_type: 'PathSearchResult',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'fs.glob': {
    method: 'fs.glob',
    owner: 'workspace',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'FsGlobRequest',
    response_type: 'FsGlobResult',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'fs.hash': {
    method: 'fs.hash',
    owner: 'workspace',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'FsHashRequest',
    response_type: 'FsHashResult',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'fs.inspect': {
    method: 'fs.inspect',
    owner: 'workspace',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'FsInspectRequest',
    response_type: 'FsInspectResult',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'fs.list': {
    method: 'fs.list',
    owner: 'workspace',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'FsListRequest',
    response_type: 'FsListResult',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'fs.move': {
    method: 'fs.move',
    owner: 'workspace',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'FsMoveRequest',
    response_type: 'FsMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'fs.read_binary_range': {
    method: 'fs.read_binary_range',
    owner: 'workspace',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'FsReadBinaryRangeRequest',
    response_type: 'FsReadBinaryRangeResult',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'fs.read_text': {
    method: 'fs.read_text',
    owner: 'workspace',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'FsReadTextRequest',
    response_type: 'FsReadTextResult',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'fs.search_text': {
    method: 'fs.search_text',
    owner: 'workspace',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'FsSearchTextRequest',
    response_type: 'FsSearchTextResult',
    idempotency: 'read_only',
    cancellation: 'request_ledger',
    test_only: false,
  },
  'fs.stat': {
    method: 'fs.stat',
    owner: 'workspace',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'FsStatRequest',
    response_type: 'FsStatResult',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'fs.write_file': {
    method: 'fs.write_file',
    owner: 'workspace',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'FsWriteFileRequest',
    response_type: 'FsMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'workspace.archive': {
    method: 'workspace.archive',
    owner: 'workspace',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'WorkspaceArchiveCommand',
    response_type: 'BeginWorkspaceLifecycleResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'workspace.buffer.edit': {
    method: 'workspace.buffer.edit',
    owner: 'workspace',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'BufferEditCommand',
    response_type: 'BufferEditResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'workspace.buffer.open': {
    method: 'workspace.buffer.open',
    owner: 'workspace',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'BufferOpenCommand',
    response_type: 'LogicalFileStatus',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'workspace.buffer.save': {
    method: 'workspace.buffer.save',
    owner: 'workspace',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'BufferSaveCommand',
    response_type: 'LogicalFileStatus',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'workspace.create': {
    method: 'workspace.create',
    owner: 'workspace',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'WorkspaceCreateCommand',
    response_type: 'WorkspaceCreateResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'workspace.delete': {
    method: 'workspace.delete',
    owner: 'workspace',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'WorkspaceDeleteCommand',
    response_type: 'BeginWorkspaceLifecycleResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'workspace.lifecycle.ack': {
    method: 'workspace.lifecycle.ack',
    owner: 'workspace',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'RendererLifecycleAck',
    response_type: 'WorkspaceLifecycleSummary',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'workspace.lifecycle.issue_discard_confirmation': {
    method: 'workspace.lifecycle.issue_discard_confirmation',
    owner: 'workspace',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'RendererIssueDiscardConfirmation',
    response_type: 'DiscardConfirmation',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'workspace.lifecycle.resolve': {
    method: 'workspace.lifecycle.resolve',
    owner: 'workspace',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'RendererResolveWorkspaceLifecycle',
    response_type: 'WorkspaceLifecycleSummary',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'workspace.relocate': {
    method: 'workspace.relocate',
    owner: 'workspace',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'WorkspaceRelocateCommand',
    response_type: 'BeginWorkspaceLifecycleResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'workspace.restore': {
    method: 'workspace.restore',
    owner: 'workspace',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'WorkspaceRestoreCommand',
    response_type: 'WorkspaceSummary',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'permission.get_defaults': {
    method: 'permission.get_defaults',
    owner: 'security',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'Record<string, never>',
    response_type: 'PermissionDefaults',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'permission.set_defaults': {
    method: 'permission.set_defaults',
    owner: 'security',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'SetPermissionDefaultsInput',
    response_type: 'PermissionDefaults',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'artifact.export': {
    method: 'artifact.export',
    owner: 'security',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ArtifactExportRequest',
    response_type: 'ArtifactExportResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'approval.get_details': {
    method: 'approval.get_details',
    owner: 'security',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'GetApprovalDetailsInput',
    response_type: 'ApprovalRequest',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'approval.list_pending': {
    method: 'approval.list_pending',
    owner: 'security',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'Record<string, never>',
    response_type: 'ApprovalView[]',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'approval.resolve': {
    method: 'approval.resolve',
    owner: 'security',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ResolveApprovalInput',
    response_type: 'ApprovalResolution',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'permission.get_session_state': {
    method: 'permission.get_session_state',
    owner: 'security',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'GetSessionPermissionStateInput',
    response_type: 'SessionPermissionProjection',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'permission.list_grants': {
    method: 'permission.list_grants',
    owner: 'security',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ListPermissionGrantsInput',
    response_type: 'PermissionGrant[]',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'security.file_session_consent.list': {
    method: 'security.file_session_consent.list',
    owner: 'security',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ListFileSessionConsentsInput',
    response_type: 'FileSessionConsentView[]',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'security.network_session_consent.list': {
    method: 'security.network_session_consent.list',
    owner: 'security',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ListNetworkSessionConsentsInput',
    response_type: 'NetworkSessionConsentView[]',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'security.file_session_consent.revoke': {
    method: 'security.file_session_consent.revoke',
    owner: 'security',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'RevokeFileSessionConsentInput',
    response_type: 'FileSessionConsentView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'security.memory_session_consent.list': {
    method: 'security.memory_session_consent.list',
    owner: 'security',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ListMemorySessionConsentsInput',
    response_type: 'MemorySessionConsentView[]',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'security.memory_session_consent.revoke': {
    method: 'security.memory_session_consent.revoke',
    owner: 'security',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'RevokeMemorySessionConsentInput',
    response_type: 'MemorySessionConsentView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'security.network_session_consent.revoke': {
    method: 'security.network_session_consent.revoke',
    owner: 'security',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'RevokeNetworkSessionConsentInput',
    response_type: 'NetworkSessionConsentView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'security.native_session_consent.list': {
    method: 'security.native_session_consent.list',
    owner: 'security',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ListNativeSessionConsentsInput',
    response_type: 'NativeSessionConsentView[]',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'security.native_session_consent.revoke': {
    method: 'security.native_session_consent.revoke',
    owner: 'security',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'RevokeNativeSessionConsentInput',
    response_type: 'NativeSessionConsentView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'permission.revoke_grant': {
    method: 'permission.revoke_grant',
    owner: 'security',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'RevokePermissionGrantInput',
    response_type: 'PermissionGrant',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'permission.set_session_mode': {
    method: 'permission.set_session_mode',
    owner: 'security',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'SetSessionPermissionModeInput',
    response_type: 'SessionPermissionProjection',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'security.host_preparation.prepare': {
    method: 'security.host_preparation.prepare',
    owner: 'security',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'HostPreparationMutationInput',
    response_type: 'HostPreparationStatus',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'security.host_preparation.remove': {
    method: 'security.host_preparation.remove',
    owner: 'security',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'HostPreparationMutationInput',
    response_type: 'HostPreparationStatus',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'security.host_preparation.repair': {
    method: 'security.host_preparation.repair',
    owner: 'security',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'HostPreparationMutationInput',
    response_type: 'HostPreparationStatus',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'security.host_preparation.status': {
    method: 'security.host_preparation.status',
    owner: 'security',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'Record<string, never>',
    response_type: 'HostPreparationStatus',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'security.external_grant.attach_read_only': {
    method: 'security.external_grant.attach_read_only',
    owner: 'security',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'RendererAttachExternalReadOnlyInput',
    response_type: 'ExternalGrantView | null',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'security.external_grant.derive_workspace': {
    method: 'security.external_grant.derive_workspace',
    owner: 'security',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'DeriveWorkspaceExternalGrantInput',
    response_type: 'ExternalGrantView[]',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'security.external_grant.list': {
    method: 'security.external_grant.list',
    owner: 'security',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ListSessionExternalGrantsInput',
    response_type: 'ExternalGrantView[]',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'security.external_grant.revalidate': {
    method: 'security.external_grant.revalidate',
    owner: 'security',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'RevalidateExternalGrantInput',
    response_type: 'ExternalGrantView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'security.external_grant.revoke': {
    method: 'security.external_grant.revoke',
    owner: 'security',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'RevokeExternalGrantInput',
    response_type: 'ExternalGrantView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'security.quarantine.discard': {
    method: 'security.quarantine.discard',
    owner: 'security',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'DiscardQuarantineInput',
    response_type: 'QuarantineRecordDetails',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'security.quarantine.issue_discard_confirmation': {
    method: 'security.quarantine.issue_discard_confirmation',
    owner: 'security',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'IssueQuarantineDiscardConfirmationInput',
    response_type: 'QuarantineDiscardConfirmation',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'security.quarantine.list': {
    method: 'security.quarantine.list',
    owner: 'security',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'Record<string, never>',
    response_type: 'QuarantineRecordDetails[]',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'security.quarantine.prepare_release': {
    method: 'security.quarantine.prepare_release',
    owner: 'security',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'PrepareQuarantineReleaseInput',
    response_type: 'PrepareQuarantineReleaseResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'security.quarantine.release': {
    method: 'security.quarantine.release',
    owner: 'security',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ReleaseQuarantineInput',
    response_type: 'QuarantineRecordDetails',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'provider_icon.read': {
    method: 'provider_icon.read',
    owner: 'model',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ProviderIconReadRequest',
    response_type: 'ProviderIconReadResult',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'models.catalog.get': {
    method: 'models.catalog.get',
    owner: 'model',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ModelCatalogGetRequest',
    response_type: 'ProviderCatalogProjection',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'models.catalog.ensure_fresh': {
    method: 'models.catalog.ensure_fresh',
    owner: 'model',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ModelCatalogEnsureFreshRequest',
    response_type: 'ProviderCatalogProjection',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'models.catalog.refresh': {
    method: 'models.catalog.refresh',
    owner: 'model',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ModelCatalogRefreshRequest',
    response_type: 'ProviderCatalogProjection',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.catalog.provider.get': {
    method: 'models.catalog.provider.get',
    owner: 'model',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ModelCatalogProviderGetRequest',
    response_type: 'ProviderCatalogModelPage',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'models.compatibility.get': {
    method: 'models.compatibility.get',
    owner: 'model',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ModelCompatibilityGetRequest',
    response_type: 'EndpointCompatibilitySnapshot',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'models.compatibility.reset': {
    method: 'models.compatibility.reset',
    owner: 'model',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ModelCompatibilityResetRequest',
    response_type: 'ModelCompatibilityResetResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.context_window.clear_user_override': {
    method: 'models.context_window.clear_user_override',
    owner: 'model',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ContextWindowClearUserOverrideRequest',
    response_type: 'ContextWindowMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.context_window.correct': {
    method: 'models.context_window.correct',
    owner: 'model',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ContextWindowCorrectRequest',
    response_type: 'ContextWindowMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.context_window.reset_runtime_correction': {
    method: 'models.context_window.reset_runtime_correction',
    owner: 'model',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ContextWindowResetRuntimeCorrectionRequest',
    response_type: 'ContextWindowMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.default.set': {
    method: 'models.default.set',
    owner: 'model',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ModelDefaultSetRequest',
    response_type: 'DefaultModelMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.endpoint.clear_credential': {
    method: 'models.endpoint.clear_credential',
    owner: 'model',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ModelEndpointClearCredentialRequest',
    response_type: 'EndpointCredentialMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.endpoint.configure_credential': {
    method: 'models.endpoint.configure_credential',
    owner: 'model',
    surface: 'main',
    auth: 'secret_input',
    request_type: 'ModelEndpointConfigureCredentialRequest',
    response_type: 'EndpointCredentialMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.endpoint.create': {
    method: 'models.endpoint.create',
    owner: 'model',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ModelEndpointCreateRequest',
    response_type: 'ModelEndpointMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.endpoint.create_with_credential': {
    method: 'models.endpoint.create_with_credential',
    owner: 'model',
    surface: 'main',
    auth: 'secret_input',
    request_type: 'ModelEndpointCreateWithCredentialRequest',
    response_type: 'EndpointCreatedWithCredentialResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.endpoint.delete': {
    method: 'models.endpoint.delete',
    owner: 'model',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ModelEndpointDeleteRequest',
    response_type: 'ModelDeletionMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.discovery.start': {
    method: 'models.discovery.start',
    owner: 'model',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ModelDiscoveryStartRequest',
    response_type: 'ModelDiscoveryStartAccepted',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'models.discovery.cancel': {
    method: 'models.discovery.cancel',
    owner: 'model',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ModelDiscoveryCancelRequest',
    response_type: 'ModelDiscoveryCancelResult',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'models.endpoint.replace_credential': {
    method: 'models.endpoint.replace_credential',
    owner: 'model',
    surface: 'main',
    auth: 'secret_input',
    request_type: 'ModelEndpointReplaceCredentialRequest',
    response_type: 'EndpointCredentialMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.endpoint.update': {
    method: 'models.endpoint.update',
    owner: 'model',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ModelEndpointUpdateRequest',
    response_type: 'ModelEndpointMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.profile.create_manual': {
    method: 'models.profile.create_manual',
    owner: 'model',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ModelProfileCreateManualRequest',
    response_type: 'ModelProfileMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.profile.create_from_catalog': {
    method: 'models.profile.create_from_catalog',
    owner: 'model',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ModelProfileCreateFromCatalogRequest',
    response_type: 'ModelProfileMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.profile.apply_catalog_update': {
    method: 'models.profile.apply_catalog_update',
    owner: 'model',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ModelProfileApplyCatalogUpdateRequest',
    response_type: 'ModelProfileMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.profile.delete': {
    method: 'models.profile.delete',
    owner: 'model',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ModelProfileDeleteRequest',
    response_type: 'ModelDeletionMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.profile.set_subagent_availability': {
    method: 'models.profile.set_subagent_availability',
    owner: 'model',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ModelProfileSetSubagentAvailabilityRequest',
    response_type: 'ModelProfileMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.profile.test_availability': {
    method: 'models.profile.test_availability',
    owner: 'model',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ModelAvailabilityTestStartRequest',
    response_type: 'ModelAvailabilityTestStartAccepted',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'models.availability_test.cancel': {
    method: 'models.availability_test.cancel',
    owner: 'model',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ModelAvailabilityTestCancelRequest',
    response_type: 'ModelAvailabilityTestCancelResult',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'models.profile.update': {
    method: 'models.profile.update',
    owner: 'model',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ModelProfileUpdateRequest',
    response_type: 'ModelProfileMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.provider.create': {
    method: 'models.provider.create',
    owner: 'model',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ModelProviderCreateRequest',
    response_type: 'ModelProviderMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.provider.delete': {
    method: 'models.provider.delete',
    owner: 'model',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ModelProviderDeleteRequest',
    response_type: 'ModelDeletionMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.provider.rename': {
    method: 'models.provider.rename',
    owner: 'model',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ModelProviderRenameRequest',
    response_type: 'ModelProviderMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.provider.set_connections': {
    method: 'models.provider.set_connections',
    owner: 'model',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ModelProviderConnectionsRequest',
    response_type: 'ModelProviderMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.provider.set_all_current_models_subagent_availability': {
    method: 'models.provider.set_all_current_models_subagent_availability',
    owner: 'model',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ModelProviderSetAllCurrentModelsSubagentAvailabilityRequest',
    response_type: 'ProviderBulkAvailabilityResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.session.set_binding': {
    method: 'models.session.set_binding',
    owner: 'model',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'SessionModelBindingSetRequest',
    response_type: 'SessionModelBindingMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.settings.get_snapshot': {
    method: 'models.settings.get_snapshot',
    owner: 'model',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ModelSettingsGetSnapshotRequest',
    response_type: 'ModelSettingsProjection',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.tools.settings': {
    method: 'agent.tools.settings',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'AgentToolSettingsRequest',
    response_type: 'AgentToolSettingsView',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.tools.configure': {
    method: 'agent.tools.configure',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'AgentToolSettingsUpdate',
    response_type: 'AgentToolSettingsView',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.tools.network_proxy.test': {
    method: 'agent.tools.network_proxy.test',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'AgentNetworkProxyTestRequest',
    response_type: 'AgentNetworkProxyTestResult',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.context.window.resolve': {
    method: 'agent.context.window.resolve',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'AgentContextWindowResolveRequest',
    response_type: 'AgentContextRemediationView',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.context.remediation': {
    method: 'agent.context.remediation',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'AgentRunRequest',
    response_type: 'AgentContextRemediationView',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.context.recheck': {
    method: 'agent.context.recheck',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'AgentContextRecheckRequest',
    response_type: 'AgentContextRemediationView',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.compaction.request': {
    method: 'agent.compaction.request',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'AgentCompactionRequest',
    response_type: 'AgentCompactionStatus',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.compaction.status': {
    method: 'agent.compaction.status',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'AgentRunRequest',
    response_type: 'AgentCompactionStatus',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.message.text': {
    method: 'agent.message.text',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'AgentMessageTextRequest',
    response_type: 'AgentMessageTextPage',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.query.select_file': {
    method: 'agent.query.select_file',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'AgentQueryFileRequest',
    response_type: 'AgentQueryFileSelection',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.related.start': {
    method: 'agent.related.start',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'AgentRelatedTurnRequest',
    response_type: 'AgentRunProjection',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.inspector': {
    method: 'agent.inspector',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'AgentInspectorRequest',
    response_type: 'AgentPromptInspection',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.history': {
    method: 'agent.history',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'AgentHistoryRequest',
    response_type: 'AgentSessionProjection',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.session.branch': {
    method: 'agent.session.branch',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'AgentSessionBranchRequest',
    response_type: 'AgentSessionBranchResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.result': {
    method: 'agent.result',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'AgentResultRequest',
    response_type: 'AgentResultView',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.sessions.activity': {
    method: 'agent.sessions.activity',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'AgentSessionsActivityRequest',
    response_type: 'AgentSessionsActivity',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.inbox': {
    method: 'agent.inbox',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'AgentSessionRequest',
    response_type: 'AgentInboxProjection',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.submit': {
    method: 'agent.submit',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'AgentSubmitRequest',
    response_type: 'AgentInboxProjection',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.inbox.mutate': {
    method: 'agent.inbox.mutate',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'AgentInboxMutation',
    response_type: 'AgentInboxProjection',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.shell.query': {
    method: 'agent.shell.query',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ShellQueryRequest',
    response_type: 'ShellExecutionView',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.shell.stop': {
    method: 'agent.shell.stop',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ShellStopRequest',
    response_type: 'ShellExecutionView',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.tool_activity': {
    method: 'agent.tool_activity',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'AgentToolActivityRequest',
    response_type: 'AgentToolActivityPage',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.activity.query': {
    method: 'agent.activity.query',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ActivityQueryRequestV1',
    response_type: 'ActivityQueryResponseV1',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.commands.catalog': {
    method: 'agent.commands.catalog',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'CommandCatalogRequest',
    response_type: 'CommandCatalog',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.commands.set_enabled': {
    method: 'agent.commands.set_enabled',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'CommandSetEnabledRequest',
    response_type: 'CommandCatalog',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.commands.delete': {
    method: 'agent.commands.delete',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'CommandDeleteRequest',
    response_type: 'CommandCatalog',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.commands.open_directory': {
    method: 'agent.commands.open_directory',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'CommandDirectoryRequest',
    response_type: 'CommandDirectoryOpened',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.files.open_outside': {
    method: 'agent.files.open_outside',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'WorkspaceFileOpenRequest',
    response_type: 'OpenedOutside',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.links.open': {
    method: 'agent.links.open',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ExternalLinkOpenRequest',
    response_type: 'OpenedOutside',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.clipboard.write': {
    method: 'agent.clipboard.write',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ClipboardWriteRequest',
    response_type: 'ClipboardWritten',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.workspace.rebuild_index': {
    method: 'agent.workspace.rebuild_index',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'WorkspaceIndexRebuildRequest',
    response_type: 'WorkspaceIndexRebuildStarted',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.start': {
    method: 'agent.start',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'AgentStartRequest',
    response_type: 'AgentRunProjection',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.init.start': {
    method: 'agent.init.start',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'AgentInitStartRequest',
    response_type: 'AgentRunProjection',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.compaction.start': {
    method: 'agent.compaction.start',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'AgentCompactionStartRequest',
    response_type: 'AgentRunProjection',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.status': {
    method: 'agent.status',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'AgentRunRequest',
    response_type: 'AgentRunProjection',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.cancel': {
    method: 'agent.cancel',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'AgentRunRequest',
    response_type: 'AgentRunProjection',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.mode.control': {
    method: 'agent.mode.control',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'AgentModeControlRequest',
    response_type: 'AgentModeEpisodeView',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.query.answer': {
    method: 'agent.query.answer',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'AgentQueryAnswerRequest',
    response_type: 'AgentRunProjection',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.query.touch': {
    method: 'agent.query.touch',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'AgentQueryTouchRequest',
    response_type: 'AgentQueryTouchView',
    idempotency: 'transient_input',
    cancellation: 'none',
    test_only: false,
  },
  'agent.approval.get': {
    method: 'agent.approval.get',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ExecutionApprovalRequest',
    response_type: 'ExecutionApprovalView',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.approval.resolve': {
    method: 'agent.approval.resolve',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ExecutionApprovalResolutionRequest',
    response_type: 'AgentRunProjection',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.hooks.list': {
    method: 'agent.hooks.list',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'HookListRequest',
    response_type: 'HookRegistryView',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.hooks.preview': {
    method: 'agent.hooks.preview',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'HookPreviewRequest',
    response_type: 'HookPreview',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.hooks.invocations': {
    method: 'agent.hooks.invocations',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'HookInvocationListRequest',
    response_type: 'HookInvocationList',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.hooks.trust': {
    method: 'agent.hooks.trust',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'HookTrustRequest',
    response_type: 'HookRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.hooks.untrust': {
    method: 'agent.hooks.untrust',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'HookPathRequest',
    response_type: 'HookRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.hooks.set_enabled': {
    method: 'agent.hooks.set_enabled',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'HookSetEnabledRequest',
    response_type: 'HookRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.hooks.remove': {
    method: 'agent.hooks.remove',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'HookPathRequest',
    response_type: 'HookRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.hooks.move': {
    method: 'agent.hooks.move',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'HookMoveRequest',
    response_type: 'HookRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.hooks.select_directory': {
    method: 'agent.hooks.select_directory',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'HookSelectDirectoryRequest',
    response_type: 'HookSelectedDirectory',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.mcp.list': {
    method: 'agent.mcp.list',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'McpListRequest',
    response_type: 'McpRegistryView',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.mcp.preview': {
    method: 'agent.mcp.preview',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'McpPreviewRequest',
    response_type: 'McpPreview',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.mcp.describe': {
    method: 'agent.mcp.describe',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'McpDescribeRequest',
    response_type: 'McpServerDetail',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.mcp.save': {
    method: 'agent.mcp.save',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'McpSaveRequest',
    response_type: 'McpRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.mcp.trust': {
    method: 'agent.mcp.trust',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'McpTrustRequest',
    response_type: 'McpRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.mcp.untrust': {
    method: 'agent.mcp.untrust',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'McpServerRequest',
    response_type: 'McpRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.mcp.set_enabled': {
    method: 'agent.mcp.set_enabled',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'McpSetEnabledRequest',
    response_type: 'McpRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.mcp.restore_plugin_default': {
    method: 'agent.mcp.restore_plugin_default',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'McpServerRequest',
    response_type: 'McpRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.mcp.remove': {
    method: 'agent.mcp.remove',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'McpServerRequest',
    response_type: 'McpRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.mcp.refresh': {
    method: 'agent.mcp.refresh',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'McpRefreshRequest',
    response_type: 'McpServerDetail',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.mcp.secret.set': {
    method: 'agent.mcp.secret.set',
    owner: 'agent',
    surface: 'main',
    auth: 'secret_input',
    request_type: 'McpSecretSetRequest',
    response_type: 'McpSecretSetResult',
    idempotency: 'command_invocation',
    cancellation: 'request_ledger',
    test_only: false,
  },
  'agent.mcp.oauth.sign_in': {
    method: 'agent.mcp.oauth.sign_in',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'McpSignInRequest',
    response_type: 'McpSignInBegun',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.mcp.oauth.sign_out': {
    method: 'agent.mcp.oauth.sign_out',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'McpSignOutRequest',
    response_type: 'McpRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.plugins.list': {
    method: 'agent.plugins.list',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'PluginListRequest',
    response_type: 'PluginRegistryView',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.plugins.preview': {
    method: 'agent.plugins.preview',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'PluginRequest',
    response_type: 'PluginPreview',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.plugins.import': {
    method: 'agent.plugins.import',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'PluginImportRequest',
    response_type: 'PluginRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.plugins.trust': {
    method: 'agent.plugins.trust',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'PluginTrustRequest',
    response_type: 'PluginRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.plugins.untrust': {
    method: 'agent.plugins.untrust',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'PluginChangeRequest',
    response_type: 'PluginRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.plugins.set_enabled': {
    method: 'agent.plugins.set_enabled',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'PluginSetEnabledRequest',
    response_type: 'PluginRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.plugins.discard_update': {
    method: 'agent.plugins.discard_update',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'PluginChangeRequest',
    response_type: 'PluginRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.plugins.uninstall': {
    method: 'agent.plugins.uninstall',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'PluginChangeRequest',
    response_type: 'PluginRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.plugins.select_directory': {
    method: 'agent.plugins.select_directory',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'PluginSelectDirectoryRequest',
    response_type: 'PluginSelectedDirectory',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.plugins.select_archive': {
    method: 'agent.plugins.select_archive',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'PluginSelectArchiveRequest',
    response_type: 'PluginSelectedArchive',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.plugins.open_directory': {
    method: 'agent.plugins.open_directory',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'PluginRequest',
    response_type: 'PluginDirectoryOpened',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.skills.list': {
    method: 'agent.skills.list',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'SkillListRequest',
    response_type: 'SkillRegistryView',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.skills.preview': {
    method: 'agent.skills.preview',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'SkillRequest',
    response_type: 'SkillPreview',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.skills.trust': {
    method: 'agent.skills.trust',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'SkillTrustRequest',
    response_type: 'SkillRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.skills.untrust': {
    method: 'agent.skills.untrust',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'SkillChangeRequest',
    response_type: 'SkillRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.skills.set_enabled': {
    method: 'agent.skills.set_enabled',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'SkillSetEnabledRequest',
    response_type: 'SkillRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.skills.import': {
    method: 'agent.skills.import',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'SkillImportRequest',
    response_type: 'SkillRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.skills.remove': {
    method: 'agent.skills.remove',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'SkillChangeRequest',
    response_type: 'SkillRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.skills.select_directory': {
    method: 'agent.skills.select_directory',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'SkillSelectDirectoryRequest',
    response_type: 'SkillSelectedDirectory',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.skills.select_archive': {
    method: 'agent.skills.select_archive',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'SkillSelectArchiveRequest',
    response_type: 'SkillSelectedArchive',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.skills.open_directory': {
    method: 'agent.skills.open_directory',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'SkillRequest',
    response_type: 'SkillDirectoryOpened',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'git.runtime.get': {
    method: 'git.runtime.get',
    owner: 'git',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'EmptyToolInput',
    response_type: 'GitRuntimeStatusV1',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'git.workspace.read': {
    method: 'git.workspace.read',
    owner: 'git',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'GitWorkspaceReadRequestV1',
    response_type: 'GitReadResultV1',
    idempotency: 'read_only',
    cancellation: 'request_ledger',
    test_only: false,
  },
  'git.installation.open': {
    method: 'git.installation.open',
    owner: 'git',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'EmptyToolInput',
    response_type: 'GitInstallationResultV1',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'packages.list': {
    method: 'packages.list',
    owner: 'package',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'EmptyToolInput',
    response_type: 'PackageListV1',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'packages.import_directory': {
    method: 'packages.import_directory',
    owner: 'package',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'EmptyToolInput',
    response_type: 'PackageImportOutcomeV1',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'packages.import_archive': {
    method: 'packages.import_archive',
    owner: 'package',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'EmptyToolInput',
    response_type: 'PackageImportOutcomeV1',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'packages.version.get': {
    method: 'packages.version.get',
    owner: 'package',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'PackageVersionRefV1',
    response_type: 'PackageVersionViewV1',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'packages.conformance.prepare': {
    method: 'packages.conformance.prepare',
    owner: 'package',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'PackageConformancePrepareRequestV1',
    response_type: 'PackageConformancePreparationV1',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'packages.conformance.start': {
    method: 'packages.conformance.start',
    owner: 'package',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'PackageConformanceStartRequestV1',
    response_type: 'PackageConformanceViewV1',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'packages.conformance.get': {
    method: 'packages.conformance.get',
    owner: 'package',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'PackageVersionRefV1',
    response_type: 'PackageConformanceViewV1',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'packages.conformance.cancel': {
    method: 'packages.conformance.cancel',
    owner: 'package',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'PackageConformanceCancelRequestV1',
    response_type: 'PackageConformanceViewV1',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'packages.install': {
    method: 'packages.install',
    owner: 'package',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'PackageInstallRequestV1',
    response_type: 'PackageSummaryV1',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'packages.set_enabled': {
    method: 'packages.set_enabled',
    owner: 'package',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'PackageEnableRequestV1',
    response_type: 'PackageSummaryV1',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'packages.revoke_trust': {
    method: 'packages.revoke_trust',
    owner: 'package',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'PackageChangeRequestV1',
    response_type: 'PackageSummaryV1',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'packages.uninstall': {
    method: 'packages.uninstall',
    owner: 'package',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'PackageChangeRequestV1',
    response_type: 'PackageSummaryV1',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'packages.credential.bind': {
    method: 'packages.credential.bind',
    owner: 'package',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'PackageCredentialBindRequestV1',
    response_type: 'PackageSummaryV1',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'packages.credential.clear': {
    method: 'packages.credential.clear',
    owner: 'package',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'PackageCredentialClearRequestV1',
    response_type: 'PackageSummaryV1',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'memory.vaults.get': {
    method: 'memory.vaults.get',
    owner: 'memory',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'MemoryDesktopRequest',
    response_type: 'MemoryDesktopSnapshot',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'memory.settings.get': {
    method: 'memory.settings.get',
    owner: 'memory',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'EmptyToolInput',
    response_type: 'MemorySettingsSnapshot',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'memory.settings.update': {
    method: 'memory.settings.update',
    owner: 'memory',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'MemorySettingsUpdate',
    response_type: 'MemorySettingsSnapshot',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'memory.vault.validate': {
    method: 'memory.vault.validate',
    owner: 'memory',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'MemoryVaultRequest',
    response_type: 'MemoryValidation',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'memory.vault.rebuild': {
    method: 'memory.vault.rebuild',
    owner: 'memory',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'MemoryVaultRequest',
    response_type: 'MemoryVaultStatus',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'memory.vault.repair.preview': {
    method: 'memory.vault.repair.preview',
    owner: 'memory',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'MemoryVaultRequest',
    response_type: 'MemoryRepairPreview',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'memory.vault.repair.apply': {
    method: 'memory.vault.repair.apply',
    owner: 'memory',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'MemoryDesktopRepairApply',
    response_type: 'MemoryRepairResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'memory.vault.open_directory': {
    method: 'memory.vault.open_directory',
    owner: 'memory',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'MemoryVaultRequest',
    response_type: 'MemoryDesktopActionResult',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'memory.maintenance.open_log': {
    method: 'memory.maintenance.open_log',
    owner: 'memory',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'MemoryVaultRequest',
    response_type: 'MemoryDesktopActionResult',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'memory.vault.export': {
    method: 'memory.vault.export',
    owner: 'memory',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'MemoryVaultRequest',
    response_type: 'MemoryDesktopActionResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'tools.credential.outcome': {
    method: 'tools.credential.outcome',
    owner: 'tool',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ToolCredentialOutcomeRequestV1',
    response_type: 'ToolCredentialOutcomeV1',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'tools.credential.create': {
    method: 'tools.credential.create',
    owner: 'tool',
    surface: 'main',
    auth: 'secret_input',
    request_type: 'ToolCredentialCreateRequestV1',
    response_type: 'ToolCredentialMutationResultV1',
    idempotency: 'command_invocation',
    cancellation: 'request_ledger',
    test_only: false,
  },
  'tools.credential.rotate': {
    method: 'tools.credential.rotate',
    owner: 'tool',
    surface: 'main',
    auth: 'secret_input',
    request_type: 'ToolCredentialChangeRequestV1',
    response_type: 'ToolCredentialMutationResultV1',
    idempotency: 'command_invocation',
    cancellation: 'request_ledger',
    test_only: false,
  },
  'tools.credential.clear': {
    method: 'tools.credential.clear',
    owner: 'tool',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ToolCredentialChangeRequestV1',
    response_type: 'ToolCredentialMutationResultV1',
    idempotency: 'command_invocation',
    cancellation: 'request_ledger',
    test_only: false,
  },
  'tools.credential.origins': {
    method: 'tools.credential.origins',
    owner: 'tool',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ToolCredentialOriginsRequestV1',
    response_type: 'ToolCredentialMutationResultV1',
    idempotency: 'command_invocation',
    cancellation: 'request_ledger',
    test_only: false,
  },
  'tools.credential.recover': {
    method: 'tools.credential.recover',
    owner: 'tool',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ToolCredentialChangeRequestV1',
    response_type: 'ToolCredentialMutationResultV1',
    idempotency: 'command_invocation',
    cancellation: 'request_ledger',
    test_only: false,
  },
  'tools.credential.list': {
    method: 'tools.credential.list',
    owner: 'tool',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ToolCredentialManagementTargetV1',
    response_type: 'ToolCredentialListResultV1',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'tools.tool.set_enabled': {
    method: 'tools.tool.set_enabled',
    owner: 'tool',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ToolSetEnabledRequestV1',
    response_type: 'ToolMutationResultV1',
    idempotency: 'domain_request',
    cancellation: 'none',
    test_only: false,
  },
  'tools.instance.update_configuration': {
    method: 'tools.instance.update_configuration',
    owner: 'tool',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ToolInstanceUpdateConfigurationRequestV1',
    response_type: 'ToolMutationResultV1',
    idempotency: 'domain_request',
    cancellation: 'none',
    test_only: false,
  },
  'tools.instance.bind_credential': {
    method: 'tools.instance.bind_credential',
    owner: 'tool',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ToolInstanceBindCredentialRequestV1',
    response_type: 'ToolMutationResultV1',
    idempotency: 'domain_request',
    cancellation: 'none',
    test_only: false,
  },
  'tools.instance.clear_credential': {
    method: 'tools.instance.clear_credential',
    owner: 'tool',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ToolInstanceClearCredentialRequestV1',
    response_type: 'ToolMutationResultV1',
    idempotency: 'domain_request',
    cancellation: 'none',
    test_only: false,
  },
  'tools.pin.workspace.set': {
    method: 'tools.pin.workspace.set',
    owner: 'tool',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'WorkspaceToolPinSetRequestV1',
    response_type: 'ToolMutationResultV1',
    idempotency: 'domain_request',
    cancellation: 'none',
    test_only: false,
  },
  'tools.pin.workspace.clear': {
    method: 'tools.pin.workspace.clear',
    owner: 'tool',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'WorkspaceToolPinClearRequestV1',
    response_type: 'ToolMutationResultV1',
    idempotency: 'domain_request',
    cancellation: 'none',
    test_only: false,
  },
  'tools.pin.session.set': {
    method: 'tools.pin.session.set',
    owner: 'tool',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'SessionToolPinSetRequestV1',
    response_type: 'ToolMutationResultV1',
    idempotency: 'domain_request',
    cancellation: 'none',
    test_only: false,
  },
  'tools.pin.session.clear': {
    method: 'tools.pin.session.clear',
    owner: 'tool',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'SessionToolPinClearRequestV1',
    response_type: 'ToolMutationResultV1',
    idempotency: 'domain_request',
    cancellation: 'none',
    test_only: false,
  },
  'tools.credential.candidates': {
    method: 'tools.credential.candidates',
    owner: 'tool',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ToolCredentialCandidatesRequestV1',
    response_type: 'ToolCredentialCandidatesProjectionV1',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'tools.activity.page': {
    method: 'tools.activity.page',
    owner: 'tool',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ToolActivityPageRequestV1',
    response_type: 'ToolActivityPageV1',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'runtime.readiness': {
    method: 'runtime.readiness',
    owner: 'runtime',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'Record<string, never>',
    response_type: 'RuntimeReadiness',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'core.health.get': {
    method: 'core.health.get',
    owner: 'runtime',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'Record<string, never>',
    response_type: 'ProjectionChange',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'config.recovery.notice': {
    method: 'config.recovery.notice',
    owner: 'runtime',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'Record<string, never>',
    response_type: 'RecoveryNotice',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'config.recovery.plan': {
    method: 'config.recovery.plan',
    owner: 'runtime',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ConfigRecoveryPlanRequest',
    response_type: 'BundleRecoveryPlan',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'config.recovery.apply': {
    method: 'config.recovery.apply',
    owner: 'runtime',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ConfigRecoveryApplyRequest',
    response_type: 'RecoveryReport',
    idempotency: 'command_invocation',
    cancellation: 'request_ledger',
    test_only: false,
  },
  'workspace_session.get': {
    method: 'workspace_session.get',
    owner: 'runtime',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'Record<string, never>',
    response_type: 'ProjectionChange',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'workspace_files.get': {
    method: 'workspace_files.get',
    owner: 'runtime',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'Record<string, never>',
    response_type: 'ProjectionChange',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'projection.subscribe': {
    method: 'projection.subscribe',
    owner: 'runtime',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ProjectionSubscription',
    response_type: 'null',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'projection.unsubscribe': {
    method: 'projection.unsubscribe',
    owner: 'runtime',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ProjectionUnsubscribeRequest',
    response_type: 'null',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'projection.snapshot': {
    method: 'projection.snapshot',
    owner: 'runtime',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ProjectionSnapshotRequest',
    response_type: 'ProjectionChange',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'terminal.user.control': {
    method: 'terminal.user.control',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'TerminalControlRequestV1',
    response_type: 'TerminalViewV1',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'terminal.user.open': {
    method: 'terminal.user.open',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'UserTerminalOpenRequestV1',
    response_type: 'TerminalViewV1',
    idempotency: 'command_invocation',
    cancellation: 'request_ledger',
    test_only: false,
  },
  'terminal.user.list': {
    method: 'terminal.user.list',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'TerminalSessionRequestV1',
    response_type: 'TerminalListV1',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'terminal.user.write': {
    method: 'terminal.user.write',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'UserTerminalWriteRequestV1',
    response_type: 'TerminalViewV1',
    idempotency: 'transient_input',
    cancellation: 'none',
    test_only: false,
  },
  'terminal.user.resize': {
    method: 'terminal.user.resize',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'TerminalResizeRequestV1',
    response_type: 'TerminalViewV1',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'terminal.user.close': {
    method: 'terminal.user.close',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'TerminalRequestV1',
    response_type: 'TerminalViewV1',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'terminal.user.subscribe': {
    method: 'terminal.user.subscribe',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'TerminalSubscribeRequestV1',
    response_type: 'TerminalOutputBatchV1',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'terminal.user.sensitive.pending': {
    method: 'terminal.user.sensitive.pending',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ManagedProcessRequestV1',
    response_type: 'TerminalSensitivePendingV1',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'terminal.user.sensitive.submit': {
    method: 'terminal.user.sensitive.submit',
    owner: 'agent',
    surface: 'main',
    auth: 'secret_input',
    request_type: 'TerminalSensitiveReplyV1',
    response_type: 'TerminalSensitiveResultV1',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'terminal.user.sensitive.cancel': {
    method: 'terminal.user.sensitive.cancel',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'TerminalSensitiveReplyV1',
    response_type: 'TerminalSensitiveResultV1',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.process.list': {
    method: 'agent.process.list',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ManagedProcessRequestV1',
    response_type: 'ManagedProcessListV1',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.process.stop': {
    method: 'agent.process.stop',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ManagedProcessStopRequestV1',
    response_type: 'ManagedProcessStopResultV1',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'ui.settings.get': {
    method: 'ui.settings.get',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'EmptyToolInput',
    response_type: 'UiSettingsSnapshot',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'ui.settings.update': {
    method: 'ui.settings.update',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'UiSettingsUpdate',
    response_type: 'UiSettingsSnapshot',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'browser.settings.get': {
    method: 'browser.settings.get',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'EmptyToolInput',
    response_type: 'BrowserSettingsSnapshot',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'browser.settings.update': {
    method: 'browser.settings.update',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'BrowserSettingsUpdate',
    response_type: 'BrowserSettingsSnapshot',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'browser.screenshot.read': {
    method: 'browser.screenshot.read',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'BrowserScreenshotReadRequest',
    response_type: 'BrowserScreenshotImageV1',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'chrome.status': {
    method: 'chrome.status',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'EmptyToolInput',
    response_type: 'ChromeControlStatus',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'chrome.set_enabled': {
    method: 'chrome.set_enabled',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ChromeSetEnabledRequest',
    response_type: 'ChromeControlStatus',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'chrome.pair.begin': {
    method: 'chrome.pair.begin',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'EmptyToolInput',
    response_type: 'ChromePairingCode',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'chrome.revoke': {
    method: 'chrome.revoke',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'EmptyToolInput',
    response_type: 'ChromeControlStatus',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'chrome.stop': {
    method: 'chrome.stop',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ChromeSessionRequest',
    response_type: 'ChromeSessionShares',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'chrome.shares': {
    method: 'chrome.shares',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ChromeSessionRequest',
    response_type: 'ChromeSessionShares',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'chrome.assist.finish': {
    method: 'chrome.assist.finish',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'ChromeSessionRequest',
    response_type: 'ChromeSessionShares',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'chrome.extension.open_folder': {
    method: 'chrome.extension.open_folder',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'EmptyToolInput',
    response_type: 'ChromeExtensionFolder',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.attachments.text': {
    method: 'agent.attachments.text',
    owner: 'agent',
    surface: 'main',
    auth: 'trusted_renderer',
    request_type: 'AttachmentTextRequest',
    response_type: 'AttachmentText',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
} as const;
export function mainRoute(method: string) {
  return Object.hasOwn(MAIN_ROUTES, method)
    ? MAIN_ROUTES[method as keyof typeof MAIN_ROUTES]
    : undefined;
}
export const CORE_ROUTES = {
  'session.rename': {
    method: 'session.rename',
    owner: 'workspace',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'SessionRenameRequest',
    response_type: 'SessionSummary',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'session.archive': {
    method: 'session.archive',
    owner: 'workspace',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'SessionLifecycleRequest',
    response_type: 'SessionSummary',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'session.restore': {
    method: 'session.restore',
    owner: 'workspace',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'SessionLifecycleRequest',
    response_type: 'SessionSummary',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'session.delete': {
    method: 'session.delete',
    owner: 'workspace',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'SessionLifecycleRequest',
    response_type: 'SessionDeleteResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'session.create': {
    method: 'session.create',
    owner: 'workspace',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'SessionCreateRequest',
    response_type: 'SessionSummary',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'fs.apply_changeset': {
    method: 'fs.apply_changeset',
    owner: 'workspace',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'FsApplyChangesetRequest',
    response_type: 'FsApplyChangesetResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'fs.apply_patch': {
    method: 'fs.apply_patch',
    owner: 'workspace',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'FsApplyPatchRequest',
    response_type: 'FsMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'fs.copy': {
    method: 'fs.copy',
    owner: 'workspace',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'FsCopyRequest',
    response_type: 'FsMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'fs.create_directory': {
    method: 'fs.create_directory',
    owner: 'workspace',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'FsCreateDirectoryRequest',
    response_type: 'FsMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'fs.delete': {
    method: 'fs.delete',
    owner: 'workspace',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'FsDeleteRequest',
    response_type: 'FsMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'fs.find_paths': {
    method: 'fs.find_paths',
    owner: 'workspace',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'FsFindPathsRequest',
    response_type: 'PathSearchResult',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'fs.glob': {
    method: 'fs.glob',
    owner: 'workspace',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'FsGlobRequest',
    response_type: 'FsGlobResult',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'fs.hash': {
    method: 'fs.hash',
    owner: 'workspace',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'FsHashRequest',
    response_type: 'FsHashResult',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'fs.inspect': {
    method: 'fs.inspect',
    owner: 'workspace',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'FsInspectRequest',
    response_type: 'FsInspectResult',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'fs.list': {
    method: 'fs.list',
    owner: 'workspace',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'FsListRequest',
    response_type: 'FsListResult',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'fs.move': {
    method: 'fs.move',
    owner: 'workspace',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'FsMoveRequest',
    response_type: 'FsMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'fs.read_binary_range': {
    method: 'fs.read_binary_range',
    owner: 'workspace',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'FsReadBinaryRangeRequest',
    response_type: 'FsReadBinaryRangeResult',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'fs.read_text': {
    method: 'fs.read_text',
    owner: 'workspace',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'FsReadTextRequest',
    response_type: 'FsReadTextResult',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'fs.search_text': {
    method: 'fs.search_text',
    owner: 'workspace',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'FsSearchTextRequest',
    response_type: 'FsSearchTextResult',
    idempotency: 'read_only',
    cancellation: 'request_ledger',
    test_only: false,
  },
  'fs.stat': {
    method: 'fs.stat',
    owner: 'workspace',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'FsStatRequest',
    response_type: 'FsStatResult',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'fs.write_file': {
    method: 'fs.write_file',
    owner: 'workspace',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'FsWriteFileRequest',
    response_type: 'FsMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'workspace.buffer.edit': {
    method: 'workspace.buffer.edit',
    owner: 'workspace',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'BufferEditCommand',
    response_type: 'BufferEditResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'workspace.buffer.open': {
    method: 'workspace.buffer.open',
    owner: 'workspace',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'BufferOpenCommand',
    response_type: 'LogicalFileStatus',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'workspace.buffer.save': {
    method: 'workspace.buffer.save',
    owner: 'workspace',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'BufferSaveCommand',
    response_type: 'LogicalFileStatus',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'workspace.create': {
    method: 'workspace.create',
    owner: 'workspace',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'WorkspaceCreateCommand',
    response_type: 'WorkspaceCreateResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'workspace.lifecycle.ack': {
    method: 'workspace.lifecycle.ack',
    owner: 'workspace',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'EditorLifecycleTransitionAck',
    response_type: 'WorkspaceLifecycleSummary',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'workspace.lifecycle.begin': {
    method: 'workspace.lifecycle.begin',
    owner: 'workspace',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'BeginWorkspaceLifecycleInput',
    response_type: 'BeginWorkspaceLifecycleResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'workspace.lifecycle.issue_discard_confirmation': {
    method: 'workspace.lifecycle.issue_discard_confirmation',
    owner: 'workspace',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'IssueDiscardConfirmationInput',
    response_type: 'DiscardConfirmation',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'workspace.lifecycle.prepare': {
    method: 'workspace.lifecycle.prepare',
    owner: 'workspace',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'WorkspaceLifecycleOperationQuery',
    response_type: 'PrepareEditorLifecycleTransition',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'workspace.lifecycle.prepare_delivered': {
    method: 'workspace.lifecycle.prepare_delivered',
    owner: 'workspace',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'WorkspaceLifecyclePrepareDeliveredInput',
    response_type: 'BeginWorkspaceLifecycleResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'workspace.lifecycle.renderer_unavailable': {
    method: 'workspace.lifecycle.renderer_unavailable',
    owner: 'workspace',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'RendererLifecycleUnavailableInput',
    response_type: 'WorkspaceLifecycleSummary[]',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'workspace.lifecycle.resolve': {
    method: 'workspace.lifecycle.resolve',
    owner: 'workspace',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ResolveWorkspaceLifecycleInput',
    response_type: 'WorkspaceLifecycleSummary',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'workspace.restore': {
    method: 'workspace.restore',
    owner: 'workspace',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'WorkspaceRestoreCommand',
    response_type: 'WorkspaceSummary',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'permission.get_defaults': {
    method: 'permission.get_defaults',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'Record<string, never>',
    response_type: 'PermissionDefaults',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'permission.set_defaults': {
    method: 'permission.set_defaults',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'SetPermissionDefaultsInput',
    response_type: 'PermissionDefaults',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'broker.http_read': {
    method: 'broker.http_read',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'BrokerHttpReadRequest',
    response_type: 'BrokerHttpReadResult',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'artifact.export': {
    method: 'artifact.export',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ArtifactExportRequest',
    response_type: 'ArtifactExportResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'approval.get_details': {
    method: 'approval.get_details',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'GetApprovalDetailsInput',
    response_type: 'ApprovalRequest',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'approval.list_pending': {
    method: 'approval.list_pending',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'Record<string, never>',
    response_type: 'ApprovalView[]',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'approval.resolve': {
    method: 'approval.resolve',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ResolveApprovalInput',
    response_type: 'ApprovalResolution',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'permission.get_session_state': {
    method: 'permission.get_session_state',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'GetSessionPermissionStateInput',
    response_type: 'SessionPermissionProjection',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'permission.list_grants': {
    method: 'permission.list_grants',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ListPermissionGrantsInput',
    response_type: 'PermissionGrant[]',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'security.file_session_consent.list': {
    method: 'security.file_session_consent.list',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ListFileSessionConsentsInput',
    response_type: 'FileSessionConsentView[]',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'security.network_session_consent.list': {
    method: 'security.network_session_consent.list',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ListNetworkSessionConsentsInput',
    response_type: 'NetworkSessionConsentView[]',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'security.file_session_consent.revoke': {
    method: 'security.file_session_consent.revoke',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'RevokeFileSessionConsentInput',
    response_type: 'FileSessionConsentView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'security.memory_session_consent.list': {
    method: 'security.memory_session_consent.list',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ListMemorySessionConsentsInput',
    response_type: 'MemorySessionConsentView[]',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'security.memory_session_consent.revoke': {
    method: 'security.memory_session_consent.revoke',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'RevokeMemorySessionConsentInput',
    response_type: 'MemorySessionConsentView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'security.network_session_consent.revoke': {
    method: 'security.network_session_consent.revoke',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'RevokeNetworkSessionConsentInput',
    response_type: 'NetworkSessionConsentView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'security.native_session_consent.list': {
    method: 'security.native_session_consent.list',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ListNativeSessionConsentsInput',
    response_type: 'NativeSessionConsentView[]',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'security.native_session_consent.revoke': {
    method: 'security.native_session_consent.revoke',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'RevokeNativeSessionConsentInput',
    response_type: 'NativeSessionConsentView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'permission.revoke_grant': {
    method: 'permission.revoke_grant',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'RevokePermissionGrantInput',
    response_type: 'PermissionGrant',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'permission.set_session_mode': {
    method: 'permission.set_session_mode',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'SetSessionPermissionModeInput',
    response_type: 'SessionPermissionProjection',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'security.host_preparation.prepare': {
    method: 'security.host_preparation.prepare',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'HostPreparationMutationInput',
    response_type: 'HostPreparationBeginResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'security.host_preparation.remove': {
    method: 'security.host_preparation.remove',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'HostPreparationMutationInput',
    response_type: 'HostPreparationBeginResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'security.host_preparation.repair': {
    method: 'security.host_preparation.repair',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'HostPreparationMutationInput',
    response_type: 'HostPreparationBeginResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'security.host_preparation.status': {
    method: 'security.host_preparation.status',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'Record<string, never>',
    response_type: 'HostPreparationStatus',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'security.external_grant.derive_workspace': {
    method: 'security.external_grant.derive_workspace',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'DeriveWorkspaceExternalGrantInput',
    response_type: 'ExternalGrantView[]',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'security.external_grant.list': {
    method: 'security.external_grant.list',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ListSessionExternalGrantsInput',
    response_type: 'ExternalGrantView[]',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'security.external_grant.revalidate': {
    method: 'security.external_grant.revalidate',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'RevalidateExternalGrantInput',
    response_type: 'ExternalGrantView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'security.external_grant.revoke': {
    method: 'security.external_grant.revoke',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'RevokeExternalGrantInput',
    response_type: 'ExternalGrantView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'security.quarantine.discard': {
    method: 'security.quarantine.discard',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'DiscardQuarantineInput',
    response_type: 'QuarantineRecordDetails',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'security.quarantine.issue_discard_confirmation': {
    method: 'security.quarantine.issue_discard_confirmation',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'IssueQuarantineDiscardConfirmationInput',
    response_type: 'QuarantineDiscardConfirmation',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'security.quarantine.list': {
    method: 'security.quarantine.list',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'Record<string, never>',
    response_type: 'QuarantineRecordDetails[]',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'security.quarantine.prepare_release': {
    method: 'security.quarantine.prepare_release',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'PrepareQuarantineReleaseInput',
    response_type: 'PrepareQuarantineReleaseResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'security.quarantine.release': {
    method: 'security.quarantine.release',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ReleaseQuarantineInput',
    response_type: 'QuarantineRecordDetails',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'security.external_grant.attach_read_only_selected': {
    method: 'security.external_grant.attach_read_only_selected',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'AttachExternalReadOnlyInput',
    response_type: 'ExternalGrantView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'security.host_preparation.probe': {
    method: 'security.host_preparation.probe',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'Record<string, never>',
    response_type: 'HostPreparationStatus',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'security.host_preparation.resolve': {
    method: 'security.host_preparation.resolve',
    owner: 'security',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ResolveHostPreparationInput',
    response_type: 'HostPreparationStatus',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'broker.context': {
    method: 'broker.context',
    owner: 'model',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ContextBrokerReadRequest',
    response_type: 'ContextBrokerReadResult',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'provider_icon.read': {
    method: 'provider_icon.read',
    owner: 'model',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ProviderIconReadRequest',
    response_type: 'ProviderIconReadResult',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'models.catalog.get': {
    method: 'models.catalog.get',
    owner: 'model',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ModelCatalogGetRequest',
    response_type: 'ProviderCatalogProjection',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'models.catalog.ensure_fresh': {
    method: 'models.catalog.ensure_fresh',
    owner: 'model',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ModelCatalogEnsureFreshRequest',
    response_type: 'ProviderCatalogProjection',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'models.catalog.refresh': {
    method: 'models.catalog.refresh',
    owner: 'model',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ModelCatalogRefreshRequest',
    response_type: 'ProviderCatalogProjection',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.catalog.provider.get': {
    method: 'models.catalog.provider.get',
    owner: 'model',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ModelCatalogProviderGetRequest',
    response_type: 'ProviderCatalogModelPage',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'models.compatibility.get': {
    method: 'models.compatibility.get',
    owner: 'model',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ModelCompatibilityGetRequest',
    response_type: 'EndpointCompatibilitySnapshot',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'models.compatibility.reset': {
    method: 'models.compatibility.reset',
    owner: 'model',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ModelCompatibilityResetRequest',
    response_type: 'ModelCompatibilityResetResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.context_window.clear_user_override': {
    method: 'models.context_window.clear_user_override',
    owner: 'model',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ContextWindowClearUserOverrideRequest',
    response_type: 'ContextWindowMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.context_window.correct': {
    method: 'models.context_window.correct',
    owner: 'model',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ContextWindowCorrectRequest',
    response_type: 'ContextWindowMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.context_window.reset_runtime_correction': {
    method: 'models.context_window.reset_runtime_correction',
    owner: 'model',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ContextWindowResetRuntimeCorrectionRequest',
    response_type: 'ContextWindowMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.default.set': {
    method: 'models.default.set',
    owner: 'model',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ModelDefaultSetRequest',
    response_type: 'DefaultModelMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.endpoint.clear_credential': {
    method: 'models.endpoint.clear_credential',
    owner: 'model',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ModelEndpointClearCredentialRequest',
    response_type: 'EndpointCredentialMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.endpoint.configure_credential': {
    method: 'models.endpoint.configure_credential',
    owner: 'model',
    surface: 'core',
    auth: 'secret_input',
    request_type: 'ModelEndpointConfigureCredentialRequest',
    response_type: 'EndpointCredentialMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.endpoint.create': {
    method: 'models.endpoint.create',
    owner: 'model',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ModelEndpointCreateRequest',
    response_type: 'ModelEndpointMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.endpoint.create_with_credential': {
    method: 'models.endpoint.create_with_credential',
    owner: 'model',
    surface: 'core',
    auth: 'secret_input',
    request_type: 'ModelEndpointCreateWithCredentialRequest',
    response_type: 'EndpointCreatedWithCredentialResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.endpoint.delete': {
    method: 'models.endpoint.delete',
    owner: 'model',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ModelEndpointDeleteRequest',
    response_type: 'ModelDeletionMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.discovery.start': {
    method: 'models.discovery.start',
    owner: 'model',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ModelDiscoveryStartRequest',
    response_type: 'ModelDiscoveryStartAccepted',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'models.discovery.cancel': {
    method: 'models.discovery.cancel',
    owner: 'model',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ModelDiscoveryCancelRequest',
    response_type: 'ModelDiscoveryCancelResult',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'models.endpoint.replace_credential': {
    method: 'models.endpoint.replace_credential',
    owner: 'model',
    surface: 'core',
    auth: 'secret_input',
    request_type: 'ModelEndpointReplaceCredentialRequest',
    response_type: 'EndpointCredentialMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.endpoint.update': {
    method: 'models.endpoint.update',
    owner: 'model',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ModelEndpointUpdateRequest',
    response_type: 'ModelEndpointMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.profile.create_manual': {
    method: 'models.profile.create_manual',
    owner: 'model',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ModelProfileCreateManualRequest',
    response_type: 'ModelProfileMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.profile.create_from_catalog': {
    method: 'models.profile.create_from_catalog',
    owner: 'model',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ModelProfileCreateFromCatalogRequest',
    response_type: 'ModelProfileMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.profile.apply_catalog_update': {
    method: 'models.profile.apply_catalog_update',
    owner: 'model',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ModelProfileApplyCatalogUpdateRequest',
    response_type: 'ModelProfileMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.profile.delete': {
    method: 'models.profile.delete',
    owner: 'model',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ModelProfileDeleteRequest',
    response_type: 'ModelDeletionMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.profile.set_subagent_availability': {
    method: 'models.profile.set_subagent_availability',
    owner: 'model',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ModelProfileSetSubagentAvailabilityRequest',
    response_type: 'ModelProfileMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.profile.test_availability': {
    method: 'models.profile.test_availability',
    owner: 'model',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ModelAvailabilityTestStartRequest',
    response_type: 'ModelAvailabilityTestStartAccepted',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'models.availability_test.cancel': {
    method: 'models.availability_test.cancel',
    owner: 'model',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ModelAvailabilityTestCancelRequest',
    response_type: 'ModelAvailabilityTestCancelResult',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'models.profile.update': {
    method: 'models.profile.update',
    owner: 'model',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ModelProfileUpdateRequest',
    response_type: 'ModelProfileMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.provider.create': {
    method: 'models.provider.create',
    owner: 'model',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ModelProviderCreateRequest',
    response_type: 'ModelProviderMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.provider.delete': {
    method: 'models.provider.delete',
    owner: 'model',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ModelProviderDeleteRequest',
    response_type: 'ModelDeletionMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.provider.rename': {
    method: 'models.provider.rename',
    owner: 'model',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ModelProviderRenameRequest',
    response_type: 'ModelProviderMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.provider.set_connections': {
    method: 'models.provider.set_connections',
    owner: 'model',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ModelProviderConnectionsRequest',
    response_type: 'ModelProviderMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.provider.set_all_current_models_subagent_availability': {
    method: 'models.provider.set_all_current_models_subagent_availability',
    owner: 'model',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ModelProviderSetAllCurrentModelsSubagentAvailabilityRequest',
    response_type: 'ProviderBulkAvailabilityResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.session.set_binding': {
    method: 'models.session.set_binding',
    owner: 'model',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'SessionModelBindingSetRequest',
    response_type: 'SessionModelBindingMutationResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'models.settings.get_snapshot': {
    method: 'models.settings.get_snapshot',
    owner: 'model',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ModelSettingsGetSnapshotRequest',
    response_type: 'ModelSettingsProjection',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.tools.settings': {
    method: 'agent.tools.settings',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'AgentToolSettingsRequest',
    response_type: 'AgentToolSettingsView',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.tools.configure': {
    method: 'agent.tools.configure',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'AgentToolSettingsUpdate',
    response_type: 'AgentToolSettingsView',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.tools.network_proxy.test': {
    method: 'agent.tools.network_proxy.test',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'AgentNetworkProxyTestRequest',
    response_type: 'AgentNetworkProxyTestResult',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.context.window.resolve': {
    method: 'agent.context.window.resolve',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'AgentContextWindowResolveRequest',
    response_type: 'AgentContextRemediationView',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.context.remediation': {
    method: 'agent.context.remediation',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'AgentRunRequest',
    response_type: 'AgentContextRemediationView',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.context.recheck': {
    method: 'agent.context.recheck',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'AgentContextRecheckRequest',
    response_type: 'AgentContextRemediationView',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.compaction.request': {
    method: 'agent.compaction.request',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'AgentCompactionRequest',
    response_type: 'AgentCompactionStatus',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.compaction.status': {
    method: 'agent.compaction.status',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'AgentRunRequest',
    response_type: 'AgentCompactionStatus',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.message.text': {
    method: 'agent.message.text',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'AgentMessageTextRequest',
    response_type: 'AgentMessageTextPage',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.query.select_file': {
    method: 'agent.query.select_file',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'AgentQueryFileRequest',
    response_type: 'AgentQueryFileSelection',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.related.start': {
    method: 'agent.related.start',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'AgentRelatedTurnRequest',
    response_type: 'AgentRunProjection',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.inspector': {
    method: 'agent.inspector',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'AgentInspectorRequest',
    response_type: 'AgentPromptInspection',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.history': {
    method: 'agent.history',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'AgentHistoryRequest',
    response_type: 'AgentSessionProjection',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.session.branch': {
    method: 'agent.session.branch',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'AgentSessionBranchRequest',
    response_type: 'AgentSessionBranchResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.result': {
    method: 'agent.result',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'AgentResultRequest',
    response_type: 'AgentResultView',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.sessions.activity': {
    method: 'agent.sessions.activity',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'AgentSessionsActivityRequest',
    response_type: 'AgentSessionsActivity',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.inbox': {
    method: 'agent.inbox',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'AgentSessionRequest',
    response_type: 'AgentInboxProjection',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.submit': {
    method: 'agent.submit',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'AgentSubmitRequest',
    response_type: 'AgentInboxProjection',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.inbox.mutate': {
    method: 'agent.inbox.mutate',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'AgentInboxMutation',
    response_type: 'AgentInboxProjection',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.shell.query': {
    method: 'agent.shell.query',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ShellQueryRequest',
    response_type: 'ShellExecutionView',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.shell.stop': {
    method: 'agent.shell.stop',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ShellStopRequest',
    response_type: 'ShellExecutionView',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.tool_activity': {
    method: 'agent.tool_activity',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'AgentToolActivityRequest',
    response_type: 'AgentToolActivityPage',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.activity.query': {
    method: 'agent.activity.query',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ActivityQueryRequestV1',
    response_type: 'ActivityQueryResponseV1',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.workspace.rebuild_index': {
    method: 'agent.workspace.rebuild_index',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'WorkspaceIndexRebuildRequest',
    response_type: 'WorkspaceIndexRebuildStarted',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.commands.catalog': {
    method: 'agent.commands.catalog',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'CommandCatalogRequest',
    response_type: 'CommandCatalog',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.commands.set_enabled': {
    method: 'agent.commands.set_enabled',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'CommandSetEnabledRequest',
    response_type: 'CommandCatalog',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.commands.delete': {
    method: 'agent.commands.delete',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'CommandDeleteRequest',
    response_type: 'CommandCatalog',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.start': {
    method: 'agent.start',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'AgentStartRequest',
    response_type: 'AgentRunProjection',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.init.start': {
    method: 'agent.init.start',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'AgentInitStartRequest',
    response_type: 'AgentRunProjection',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.compaction.start': {
    method: 'agent.compaction.start',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'AgentCompactionStartRequest',
    response_type: 'AgentRunProjection',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.status': {
    method: 'agent.status',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'AgentRunRequest',
    response_type: 'AgentRunProjection',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.cancel': {
    method: 'agent.cancel',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'AgentRunRequest',
    response_type: 'AgentRunProjection',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.mode.control': {
    method: 'agent.mode.control',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'AgentModeControlRequest',
    response_type: 'AgentModeEpisodeView',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.query.answer': {
    method: 'agent.query.answer',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'AgentQueryAnswerRequest',
    response_type: 'AgentRunProjection',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.query.touch': {
    method: 'agent.query.touch',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'AgentQueryTouchRequest',
    response_type: 'AgentQueryTouchView',
    idempotency: 'transient_input',
    cancellation: 'none',
    test_only: false,
  },
  'agent.approval.get': {
    method: 'agent.approval.get',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ExecutionApprovalRequest',
    response_type: 'ExecutionApprovalView',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.approval.resolve': {
    method: 'agent.approval.resolve',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ExecutionApprovalResolutionRequest',
    response_type: 'AgentRunProjection',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.hooks.list': {
    method: 'agent.hooks.list',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'HookListRequest',
    response_type: 'HookRegistryView',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.hooks.preview': {
    method: 'agent.hooks.preview',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'HookPreviewRequest',
    response_type: 'HookPreview',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.hooks.invocations': {
    method: 'agent.hooks.invocations',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'HookInvocationListRequest',
    response_type: 'HookInvocationList',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.hooks.trust': {
    method: 'agent.hooks.trust',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'HookTrustRequest',
    response_type: 'HookRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.hooks.untrust': {
    method: 'agent.hooks.untrust',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'HookPathRequest',
    response_type: 'HookRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.hooks.set_enabled': {
    method: 'agent.hooks.set_enabled',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'HookSetEnabledRequest',
    response_type: 'HookRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.hooks.remove': {
    method: 'agent.hooks.remove',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'HookPathRequest',
    response_type: 'HookRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.hooks.move': {
    method: 'agent.hooks.move',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'HookMoveRequest',
    response_type: 'HookRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.mcp.list': {
    method: 'agent.mcp.list',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'McpListRequest',
    response_type: 'McpRegistryView',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.mcp.preview': {
    method: 'agent.mcp.preview',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'McpPreviewRequest',
    response_type: 'McpPreview',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.mcp.describe': {
    method: 'agent.mcp.describe',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'McpDescribeRequest',
    response_type: 'McpServerDetail',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.mcp.save': {
    method: 'agent.mcp.save',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'McpSaveRequest',
    response_type: 'McpRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.mcp.trust': {
    method: 'agent.mcp.trust',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'McpTrustRequest',
    response_type: 'McpRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.mcp.untrust': {
    method: 'agent.mcp.untrust',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'McpServerRequest',
    response_type: 'McpRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.mcp.set_enabled': {
    method: 'agent.mcp.set_enabled',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'McpSetEnabledRequest',
    response_type: 'McpRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.mcp.restore_plugin_default': {
    method: 'agent.mcp.restore_plugin_default',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'McpServerRequest',
    response_type: 'McpRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.mcp.remove': {
    method: 'agent.mcp.remove',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'McpServerRequest',
    response_type: 'McpRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.mcp.refresh': {
    method: 'agent.mcp.refresh',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'McpRefreshRequest',
    response_type: 'McpServerDetail',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.mcp.secret.set': {
    method: 'agent.mcp.secret.set',
    owner: 'agent',
    surface: 'core',
    auth: 'secret_input',
    request_type: 'McpSecretSetRequest',
    response_type: 'McpSecretSetResult',
    idempotency: 'command_invocation',
    cancellation: 'request_ledger',
    test_only: false,
  },
  'agent.mcp.oauth.sign_in': {
    method: 'agent.mcp.oauth.sign_in',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'McpSignInRequest',
    response_type: 'McpSignInBegun',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.mcp.oauth.sign_out': {
    method: 'agent.mcp.oauth.sign_out',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'McpSignOutRequest',
    response_type: 'McpRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.plugins.list': {
    method: 'agent.plugins.list',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'PluginListRequest',
    response_type: 'PluginRegistryView',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.plugins.preview': {
    method: 'agent.plugins.preview',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'PluginRequest',
    response_type: 'PluginPreview',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.plugins.import': {
    method: 'agent.plugins.import',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'PluginImportRequest',
    response_type: 'PluginRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.plugins.trust': {
    method: 'agent.plugins.trust',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'PluginTrustRequest',
    response_type: 'PluginRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.plugins.untrust': {
    method: 'agent.plugins.untrust',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'PluginChangeRequest',
    response_type: 'PluginRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.plugins.set_enabled': {
    method: 'agent.plugins.set_enabled',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'PluginSetEnabledRequest',
    response_type: 'PluginRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.plugins.discard_update': {
    method: 'agent.plugins.discard_update',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'PluginChangeRequest',
    response_type: 'PluginRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.plugins.uninstall': {
    method: 'agent.plugins.uninstall',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'PluginChangeRequest',
    response_type: 'PluginRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.skills.list': {
    method: 'agent.skills.list',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'SkillListRequest',
    response_type: 'SkillRegistryView',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.skills.preview': {
    method: 'agent.skills.preview',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'SkillRequest',
    response_type: 'SkillPreview',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.skills.trust': {
    method: 'agent.skills.trust',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'SkillTrustRequest',
    response_type: 'SkillRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.skills.untrust': {
    method: 'agent.skills.untrust',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'SkillChangeRequest',
    response_type: 'SkillRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.skills.set_enabled': {
    method: 'agent.skills.set_enabled',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'SkillSetEnabledRequest',
    response_type: 'SkillRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.skills.import': {
    method: 'agent.skills.import',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'SkillImportRequest',
    response_type: 'SkillRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'agent.skills.remove': {
    method: 'agent.skills.remove',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'SkillChangeRequest',
    response_type: 'SkillRegistryView',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'git.runtime.get': {
    method: 'git.runtime.get',
    owner: 'git',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'EmptyToolInput',
    response_type: 'GitRuntimeStatusV1',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'git.workspace.read': {
    method: 'git.workspace.read',
    owner: 'git',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'GitWorkspaceReadRequestV1',
    response_type: 'GitReadResultV1',
    idempotency: 'read_only',
    cancellation: 'request_ledger',
    test_only: false,
  },
  'packages.list': {
    method: 'packages.list',
    owner: 'package',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'EmptyToolInput',
    response_type: 'PackageListV1',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'packages.import_directory': {
    method: 'packages.import_directory',
    owner: 'package',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'PackageImportSourceV1',
    response_type: 'PackageImportOutcomeV1',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'packages.import_archive': {
    method: 'packages.import_archive',
    owner: 'package',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'PackageImportSourceV1',
    response_type: 'PackageImportOutcomeV1',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'packages.version.get': {
    method: 'packages.version.get',
    owner: 'package',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'PackageVersionRefV1',
    response_type: 'PackageVersionViewV1',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'packages.conformance.prepare': {
    method: 'packages.conformance.prepare',
    owner: 'package',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'PackageConformancePrepareRequestV1',
    response_type: 'PackageConformancePreparationV1',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'packages.conformance.start': {
    method: 'packages.conformance.start',
    owner: 'package',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'PackageConformanceStartRequestV1',
    response_type: 'PackageConformanceViewV1',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'packages.conformance.get': {
    method: 'packages.conformance.get',
    owner: 'package',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'PackageVersionRefV1',
    response_type: 'PackageConformanceViewV1',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'packages.conformance.cancel': {
    method: 'packages.conformance.cancel',
    owner: 'package',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'PackageConformanceCancelRequestV1',
    response_type: 'PackageConformanceViewV1',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'packages.install': {
    method: 'packages.install',
    owner: 'package',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'PackageInstallRequestV1',
    response_type: 'PackageSummaryV1',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'packages.set_enabled': {
    method: 'packages.set_enabled',
    owner: 'package',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'PackageEnableRequestV1',
    response_type: 'PackageSummaryV1',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'packages.revoke_trust': {
    method: 'packages.revoke_trust',
    owner: 'package',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'PackageChangeRequestV1',
    response_type: 'PackageSummaryV1',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'packages.uninstall': {
    method: 'packages.uninstall',
    owner: 'package',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'PackageChangeRequestV1',
    response_type: 'PackageSummaryV1',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'packages.credential.bind': {
    method: 'packages.credential.bind',
    owner: 'package',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'PackageCredentialBindRequestV1',
    response_type: 'PackageSummaryV1',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'packages.credential.clear': {
    method: 'packages.credential.clear',
    owner: 'package',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'PackageCredentialClearRequestV1',
    response_type: 'PackageSummaryV1',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'memory.vaults.get': {
    method: 'memory.vaults.get',
    owner: 'memory',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'MemoryDesktopRequest',
    response_type: 'MemoryDesktopSnapshot',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'memory.settings.get': {
    method: 'memory.settings.get',
    owner: 'memory',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'EmptyToolInput',
    response_type: 'MemorySettingsSnapshot',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'memory.settings.update': {
    method: 'memory.settings.update',
    owner: 'memory',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'MemorySettingsUpdate',
    response_type: 'MemorySettingsSnapshot',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'memory.vault.validate': {
    method: 'memory.vault.validate',
    owner: 'memory',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'MemoryVaultRequest',
    response_type: 'MemoryValidation',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'memory.vault.rebuild': {
    method: 'memory.vault.rebuild',
    owner: 'memory',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'MemoryVaultRequest',
    response_type: 'MemoryVaultStatus',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'memory.vault.repair.preview': {
    method: 'memory.vault.repair.preview',
    owner: 'memory',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'MemoryVaultRequest',
    response_type: 'MemoryRepairPreview',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'memory.vault.repair.apply': {
    method: 'memory.vault.repair.apply',
    owner: 'memory',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'MemoryDesktopRepairApply',
    response_type: 'MemoryRepairResult',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'memory.export.prepare': {
    method: 'memory.export.prepare',
    owner: 'memory',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'MemoryVaultRequest',
    response_type: 'MemoryExportHandle',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'memory.export.entry': {
    method: 'memory.export.entry',
    owner: 'memory',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'MemoryExportEntryRequest',
    response_type: 'MemoryExportEntry',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'memory.export.chunk': {
    method: 'memory.export.chunk',
    owner: 'memory',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'MemoryExportChunkRequest',
    response_type: 'MemoryExportChunk',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'memory.export.close': {
    method: 'memory.export.close',
    owner: 'memory',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'MemoryExportCloseRequest',
    response_type: 'MemoryDesktopActionResult',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'tools.credential.outcome': {
    method: 'tools.credential.outcome',
    owner: 'tool',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ToolCredentialOutcomeRequestV1',
    response_type: 'ToolCredentialOutcomeV1',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'tools.credential.create': {
    method: 'tools.credential.create',
    owner: 'tool',
    surface: 'core',
    auth: 'secret_input',
    request_type: 'ToolCredentialCreateRequestV1',
    response_type: 'ToolCredentialMutationResultV1',
    idempotency: 'command_invocation',
    cancellation: 'request_ledger',
    test_only: false,
  },
  'tools.credential.rotate': {
    method: 'tools.credential.rotate',
    owner: 'tool',
    surface: 'core',
    auth: 'secret_input',
    request_type: 'ToolCredentialChangeRequestV1',
    response_type: 'ToolCredentialMutationResultV1',
    idempotency: 'command_invocation',
    cancellation: 'request_ledger',
    test_only: false,
  },
  'tools.credential.clear': {
    method: 'tools.credential.clear',
    owner: 'tool',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ToolCredentialChangeRequestV1',
    response_type: 'ToolCredentialMutationResultV1',
    idempotency: 'command_invocation',
    cancellation: 'request_ledger',
    test_only: false,
  },
  'tools.credential.origins': {
    method: 'tools.credential.origins',
    owner: 'tool',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ToolCredentialOriginsRequestV1',
    response_type: 'ToolCredentialMutationResultV1',
    idempotency: 'command_invocation',
    cancellation: 'request_ledger',
    test_only: false,
  },
  'tools.credential.recover': {
    method: 'tools.credential.recover',
    owner: 'tool',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ToolCredentialChangeRequestV1',
    response_type: 'ToolCredentialMutationResultV1',
    idempotency: 'command_invocation',
    cancellation: 'request_ledger',
    test_only: false,
  },
  'tools.credential.list': {
    method: 'tools.credential.list',
    owner: 'tool',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ToolCredentialManagementTargetV1',
    response_type: 'ToolCredentialListResultV1',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'broker.tools': {
    method: 'broker.tools',
    owner: 'tool',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ToolBrokerInvokeRequest',
    response_type: 'ToolBrokerInvokeResult',
    idempotency: 'domain_request',
    cancellation: 'none',
    test_only: false,
  },
  'tools.tool.set_enabled': {
    method: 'tools.tool.set_enabled',
    owner: 'tool',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ToolSetEnabledRequestV1',
    response_type: 'ToolMutationResultV1',
    idempotency: 'domain_request',
    cancellation: 'none',
    test_only: false,
  },
  'tools.instance.update_configuration': {
    method: 'tools.instance.update_configuration',
    owner: 'tool',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ToolInstanceUpdateConfigurationRequestV1',
    response_type: 'ToolMutationResultV1',
    idempotency: 'domain_request',
    cancellation: 'none',
    test_only: false,
  },
  'tools.instance.bind_credential': {
    method: 'tools.instance.bind_credential',
    owner: 'tool',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ToolInstanceBindCredentialRequestV1',
    response_type: 'ToolMutationResultV1',
    idempotency: 'domain_request',
    cancellation: 'none',
    test_only: false,
  },
  'tools.instance.clear_credential': {
    method: 'tools.instance.clear_credential',
    owner: 'tool',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ToolInstanceClearCredentialRequestV1',
    response_type: 'ToolMutationResultV1',
    idempotency: 'domain_request',
    cancellation: 'none',
    test_only: false,
  },
  'tools.pin.workspace.set': {
    method: 'tools.pin.workspace.set',
    owner: 'tool',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'WorkspaceToolPinSetRequestV1',
    response_type: 'ToolMutationResultV1',
    idempotency: 'domain_request',
    cancellation: 'none',
    test_only: false,
  },
  'tools.pin.workspace.clear': {
    method: 'tools.pin.workspace.clear',
    owner: 'tool',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'WorkspaceToolPinClearRequestV1',
    response_type: 'ToolMutationResultV1',
    idempotency: 'domain_request',
    cancellation: 'none',
    test_only: false,
  },
  'tools.pin.session.set': {
    method: 'tools.pin.session.set',
    owner: 'tool',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'SessionToolPinSetRequestV1',
    response_type: 'ToolMutationResultV1',
    idempotency: 'domain_request',
    cancellation: 'none',
    test_only: false,
  },
  'tools.pin.session.clear': {
    method: 'tools.pin.session.clear',
    owner: 'tool',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'SessionToolPinClearRequestV1',
    response_type: 'ToolMutationResultV1',
    idempotency: 'domain_request',
    cancellation: 'none',
    test_only: false,
  },
  'tools.credential.candidates': {
    method: 'tools.credential.candidates',
    owner: 'tool',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ToolCredentialCandidatesRequestV1',
    response_type: 'ToolCredentialCandidatesProjectionV1',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'tools.activity.page': {
    method: 'tools.activity.page',
    owner: 'tool',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ToolActivityPageRequestV1',
    response_type: 'ToolActivityPageV1',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'runtime.readiness': {
    method: 'runtime.readiness',
    owner: 'runtime',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'Record<string, never>',
    response_type: 'RuntimeReadiness',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'runtime.prepare_exit': {
    method: 'runtime.prepare_exit',
    owner: 'runtime',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'Record<string, never>',
    response_type: 'Record<string, boolean>',
    idempotency: 'transient_input',
    cancellation: 'none',
    test_only: false,
  },
  'runtime.command_outcome': {
    method: 'runtime.command_outcome',
    owner: 'runtime',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'CommandOutcomeQuery',
    response_type: 'CommandOutcome',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'projection.subscribe': {
    method: 'projection.subscribe',
    owner: 'runtime',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ProjectionSubscription',
    response_type: 'Record<string, boolean>',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'projection.unsubscribe': {
    method: 'projection.unsubscribe',
    owner: 'runtime',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ProjectionUnsubscribeRequest',
    response_type: 'Record<string, boolean>',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'projection.snapshot': {
    method: 'projection.snapshot',
    owner: 'runtime',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ProjectionSnapshotRequest',
    response_type: 'ProjectionChange',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'aura.cancel': {
    method: 'aura.cancel',
    owner: 'runtime',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'CancellationRequest',
    response_type: 'CancelDisposition',
    idempotency: 'command_invocation',
    cancellation: 'request_ledger',
    test_only: false,
  },
  'config.recovery.notice': {
    method: 'config.recovery.notice',
    owner: 'runtime',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'Record<string, never>',
    response_type: 'RecoveryNotice',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'config.recovery.plan': {
    method: 'config.recovery.plan',
    owner: 'runtime',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ConfigRecoveryPlanRequest',
    response_type: 'BundleRecoveryPlan',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'config.recovery.apply': {
    method: 'config.recovery.apply',
    owner: 'runtime',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ConfigRecoveryApplyRequest',
    response_type: 'RecoveryReport',
    idempotency: 'command_invocation',
    cancellation: 'request_ledger',
    test_only: false,
  },
  'terminal.user.control': {
    method: 'terminal.user.control',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'TerminalControlRequestV1',
    response_type: 'TerminalViewV1',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'terminal.user.open': {
    method: 'terminal.user.open',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'UserTerminalOpenRequestV1',
    response_type: 'TerminalViewV1',
    idempotency: 'command_invocation',
    cancellation: 'request_ledger',
    test_only: false,
  },
  'terminal.user.list': {
    method: 'terminal.user.list',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'TerminalSessionRequestV1',
    response_type: 'TerminalListV1',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'terminal.user.write': {
    method: 'terminal.user.write',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'UserTerminalWriteRequestV1',
    response_type: 'TerminalViewV1',
    idempotency: 'transient_input',
    cancellation: 'none',
    test_only: false,
  },
  'terminal.user.resize': {
    method: 'terminal.user.resize',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'TerminalResizeRequestV1',
    response_type: 'TerminalViewV1',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'terminal.user.close': {
    method: 'terminal.user.close',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'TerminalRequestV1',
    response_type: 'TerminalViewV1',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'terminal.user.subscribe': {
    method: 'terminal.user.subscribe',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'TerminalSubscribeRequestV1',
    response_type: 'TerminalOutputBatchV1',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'terminal.user.sensitive.pending': {
    method: 'terminal.user.sensitive.pending',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ManagedProcessRequestV1',
    response_type: 'TerminalSensitivePendingV1',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'terminal.user.sensitive.submit': {
    method: 'terminal.user.sensitive.submit',
    owner: 'agent',
    surface: 'core',
    auth: 'secret_input',
    request_type: 'TerminalSensitiveReplyV1',
    response_type: 'TerminalSensitiveResultV1',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'terminal.user.sensitive.cancel': {
    method: 'terminal.user.sensitive.cancel',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'TerminalSensitiveReplyV1',
    response_type: 'TerminalSensitiveResultV1',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'agent.process.list': {
    method: 'agent.process.list',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ManagedProcessRequestV1',
    response_type: 'ManagedProcessListV1',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.process.stop': {
    method: 'agent.process.stop',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ManagedProcessStopRequestV1',
    response_type: 'ManagedProcessStopResultV1',
    idempotency: 'command_invocation',
    cancellation: 'domain_operation',
    test_only: false,
  },
  'ui.settings.get': {
    method: 'ui.settings.get',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'EmptyToolInput',
    response_type: 'UiSettingsSnapshot',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'ui.settings.update': {
    method: 'ui.settings.update',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'UiSettingsUpdate',
    response_type: 'UiSettingsSnapshot',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'browser.settings.get': {
    method: 'browser.settings.get',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'EmptyToolInput',
    response_type: 'BrowserSettingsSnapshot',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'browser.settings.update': {
    method: 'browser.settings.update',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'BrowserSettingsUpdate',
    response_type: 'BrowserSettingsSnapshot',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'browser.screenshot.read': {
    method: 'browser.screenshot.read',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'BrowserScreenshotReadRequest',
    response_type: 'BrowserScreenshotImageV1',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'browser.host.result': {
    method: 'browser.host.result',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'BrowserHostResultV1',
    response_type: 'BrowserHostResultAck',
    idempotency: 'transient_input',
    cancellation: 'none',
    test_only: false,
  },
  'chrome.status': {
    method: 'chrome.status',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'EmptyToolInput',
    response_type: 'ChromeControlStatus',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'chrome.set_enabled': {
    method: 'chrome.set_enabled',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ChromeSetEnabledRequest',
    response_type: 'ChromeControlStatus',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'chrome.pair.begin': {
    method: 'chrome.pair.begin',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'EmptyToolInput',
    response_type: 'ChromePairingCode',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'chrome.revoke': {
    method: 'chrome.revoke',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'EmptyToolInput',
    response_type: 'ChromeControlStatus',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'chrome.stop': {
    method: 'chrome.stop',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ChromeSessionRequest',
    response_type: 'ChromeSessionShares',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'chrome.shares': {
    method: 'chrome.shares',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ChromeSessionRequest',
    response_type: 'ChromeSessionShares',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'chrome.assist.finish': {
    method: 'chrome.assist.finish',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'ChromeSessionRequest',
    response_type: 'ChromeSessionShares',
    idempotency: 'command_invocation',
    cancellation: 'none',
    test_only: false,
  },
  'media.host.read': {
    method: 'media.host.read',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'MediaHostReadRequestV1',
    response_type: 'MediaHostReadResultV1',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'media.host.result': {
    method: 'media.host.result',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'MediaHostResultV1',
    response_type: 'MediaHostResultAck',
    idempotency: 'transient_input',
    cancellation: 'none',
    test_only: false,
  },
  'agent.attachments.text': {
    method: 'agent.attachments.text',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'AttachmentTextRequest',
    response_type: 'AttachmentText',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.attachments.locate': {
    method: 'agent.attachments.locate',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'AttachmentLocateRequest',
    response_type: 'AttachmentLocation',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.attachments.entry': {
    method: 'agent.attachments.entry',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'AttachmentEntryRequest',
    response_type: 'AttachmentLocation',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
  'agent.files.mentioned': {
    method: 'agent.files.mentioned',
    owner: 'agent',
    surface: 'core',
    auth: 'authenticated_main',
    request_type: 'MentionedFileRequest',
    response_type: 'AttachmentLocation',
    idempotency: 'read_only',
    cancellation: 'none',
    test_only: false,
  },
} as const;
export function coreRoute(method: string) {
  return Object.hasOwn(CORE_ROUTES, method)
    ? CORE_ROUTES[method as keyof typeof CORE_ROUTES]
    : undefined;
}
export const ROUTE_COUNTS = { main: 264, core: 263 } as const;
