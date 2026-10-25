/* eslint-disable */
/** Generated from resources/schemas/aura-contracts.schema.json. Do not edit. */

/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContentHashAlgorithm".
 */
export type ContentHashAlgorithm = 'blake3' | 'sha256';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BuildComponentKind".
 */
export type BuildComponentKind =
  | 'launcher'
  | 'setup'
  | 'desktop'
  | 'main_pipe_bridge'
  | 'core'
  | 'sandbox_host'
  | 'egress_proxy'
  | 'elevated_helper'
  | 'web_runtime'
  | 'chromium'
  | 'plugin_host'
  | 'chrome_native_host'
  | 'prompt_bundle'
  | 'database_schema'
  | 'ipc_protocol';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ReleaseBundleId".
 */
export type ReleaseBundleId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ReleaseChannel".
 */
export type ReleaseChannel = 'Stable' | 'Preview' | 'Nightly';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TokenEstimateExactness".
 */
export type TokenEstimateExactness = ('exact' | 'upper_bound') | 'calibrated';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextRetentionException".
 */
export type ContextRetentionException =
  'oversized_atomic_segment' | 'insufficient_eligible_recent_history';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextProjectionSnapshotId".
 */
export type ContextProjectionSnapshotId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SessionSequence".
 */
export type SessionSequence = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextSourceRef".
 */
export type ContextSourceRef =
  | {
      event_id: EventId;
      type: 'event';
    }
  | {
      artifact_id: ArtifactId;
      type: 'artifact';
    }
  | {
      context_snapshot_id: ContextProjectionSnapshotId;
      type: 'compaction';
    }
  | {
      tool_id: ToolId;
      tool_version: ToolVersion;
      type: 'tool_schema';
    }
  | {
      content_hash: ContentHash;
      module_id: string;
      type: 'prompt_module';
    }
  | {
      content_hash: ContentHash;
      relative_path: string;
      type: 'workspace_file';
      workspace_id: WorkspaceId;
    }
  | {
      lineage_id: TaskLineageId;
      revision: Revision;
      type: 'task_lineage';
    }
  | {
      state_kind: string;
      state_revision: Revision;
      type: 'runtime_state';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "EventId".
 */
export type EventId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ArtifactId".
 */
export type ArtifactId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolId".
 */
export type ToolId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "Revision".
 */
export type Revision = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CanonicalSemVer".
 */
export type CanonicalSemVer = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceId".
 */
export type WorkspaceId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TaskLineageId".
 */
export type TaskLineageId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CompactionAnchorKind".
 */
export type CompactionAnchorKind =
  'event' | 'artifact' | 'tool_call' | 'workspace_file' | 'task_lineage';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelExecutionSnapshotId".
 */
export type ModelExecutionSnapshotId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelStepId".
 */
export type ModelStepId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "UtcTimestamp".
 */
export type UtcTimestamp = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CompactionModelAttemptState".
 */
export type CompactionModelAttemptState =
  'running' | 'completed' | 'failed' | 'cancelled' | 'unknown_side_effect';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TransportAttemptId".
 */
export type TransportAttemptId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentRunId".
 */
export type AgentRunId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FileReferenceId".
 */
export type FileReferenceId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextUsageKind".
 */
export type ContextUsageKind =
  'system_and_rules' | 'tools_and_results' | 'conversation' | 'background_and_compaction';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "LogicalConversationItem".
 */
export type LogicalConversationItem =
  | {
      content: string;
      content_hash: ContentHash;
      source_event_id: EventId;
      type: 'user';
    }
  | {
      content?: string | null;
      content_hash?: ContentHash | null;
      provider_history?: LogicalProviderHistory | null;
      source_event_id: EventId;
      tool_calls: LogicalHistoricalToolCall[];
      type: 'assistant';
    }
  | {
      content: string;
      content_hash: ContentHash;
      is_error: boolean;
      source_event_id: EventId;
      tool_call_id: ToolCallId;
      tool_name: string;
      type: 'tool_result';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderHistoryData".
 */
export type ProviderHistoryData =
  | {
      reasoning_content?: string | null;
      /**
       * Exact provider call ID -> Google thought signature, when supplied.
       */
      thought_signatures: {
        [k: string]: string | undefined;
      };
      type: 'chat';
    }
  | {
      reasoning_items: any[];
      type: 'responses';
    }
  | {
      thinking_blocks: any[];
      type: 'anthropic';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelApiFormat".
 */
export type ModelApiFormat =
  'open_ai_responses' | 'open_ai_chat_completions' | 'anthropic_messages';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "EndpointId".
 */
export type EndpointId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCallId".
 */
export type ToolCallId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolExposureV1".
 */
export type ToolExposureV1 =
  'core' | 'session_pin' | 'workspace_pin' | 'ephemeral_load' | 'unified_entry';
/**
 * Exact, source-specific contract identity frozen into a Registry binding.
 *
 * Package-authored tools retain the complete W2 fingerprint (including the
 * authoritative runtime-closure hash). Built-ins and external adapters use
 * their own normalized/signed contract fingerprints so a generic digest can
 * never be silently reinterpreted as a different source class.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolSourceContractFingerprintV1".
 */
export type ToolSourceContractFingerprintV1 =
  | {
      signed_builtin_tool_contract_fingerprint: CanonicalSha256HashV1;
      source_kind: 'built_in';
    }
  | {
      source_kind: 'user_created';
      tool_contract_fingerprint: ToolContractFingerprintV1;
    }
  | {
      source_kind: 'model_created';
      tool_contract_fingerprint: ToolContractFingerprintV1;
    }
  | {
      source_kind: 'imported';
      tool_contract_fingerprint: ToolContractFingerprintV1;
    }
  | {
      normalized_mcp_tool_contract_fingerprint: CanonicalSha256HashV1;
      source_kind: 'mcp';
    }
  | {
      normalized_plugin_tool_contract_fingerprint: CanonicalSha256HashV1;
      source_kind: 'plugin';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolSourceKindV1".
 */
export type ToolSourceKindV1 =
  'built_in' | 'user_created' | 'model_created' | 'imported' | 'mcp' | 'plugin';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolInstanceId".
 */
export type ToolInstanceId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelProfileId".
 */
export type ModelProfileId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelOutputFormat".
 */
export type ModelOutputFormat =
  | {
      type: 'text';
    }
  | {
      type: 'json_object';
    }
  | {
      name: string;
      schema: any;
      schema_hash: ContentHash;
      strict: boolean;
      type: 'json_schema';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PromptBundleSnapshotId".
 */
export type PromptBundleSnapshotId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextCompressibility".
 */
export type ContextCompressibility =
  | 'never_compress'
  | 'compressible_history'
  | 'compressible_sealed_active_segment'
  | 'user_reducible_only';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextSectionKind".
 */
export type ContextSectionKind =
  | 'core_system_prompt'
  | 'permission_and_security'
  | 'loaded_tool_schemas'
  | 'workspace_agents_instructions'
  | 'historical_snapshot_summary'
  | 'agent_mode_prompt'
  | 'hook_injections'
  | 'current_task_state'
  | 'recent_history'
  | 'file_references_and_pending_attachments'
  | 'current_turn_working_set'
  | 'steered_user_additions';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextPrivacyClass".
 */
export type ContextPrivacyClass = 'model_visible' | 'user_only' | 'internal_only';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SessionId".
 */
export type SessionId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolRegistrySnapshotId".
 */
export type ToolRegistrySnapshotId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCanonicalName".
 */
export type ToolCanonicalName = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolSchemaDialect".
 */
export type ToolSchemaDialect = 'json_schema_draft_2020_12';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TurnTaskId".
 */
export type TurnTaskId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextEventSelector".
 */
export type ContextEventSelector =
  | {
      event_id: EventId;
      type: 'event';
    }
  | {
      end: SessionSequence;
      start: SessionSequence;
      type: 'sequence_range';
    }
  | {
      turn_task_id: TurnTaskId;
      type: 'turn_task';
    }
  | {
      group_ref: HistoryAtomicGroupRefV1;
      type: 'history_atomic_group';
    }
  | {
      atomic_group_id: string;
      type: 'atomic_group';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextSearchMode".
 */
export type ContextSearchMode = 'natural' | 'exact_phrase';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HistoryAtomicGroupKindV1".
 */
export type HistoryAtomicGroupKindV1 = 'aura.p06.history_atomic_group';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolWireVersionV1".
 */
export type ToolWireVersionV1 = 1;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolBatchId".
 */
export type ToolBatchId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CommandInvocationId".
 */
export type CommandInvocationId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ManualCompactionRequestState".
 */
export type ManualCompactionRequestState =
  'requested' | 'running' | 'completed' | 'failed' | 'cancelled';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelEgressAuthorizationBasis".
 */
export type ModelEgressAuthorizationBasis =
  | {
      type: 'model_channel_policy';
    }
  | {
      type: 'user_submission';
    }
  | {
      bindings: SourceAuthorizationBinding[];
      type: 'source_grants';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SourceAuthorizationRef".
 */
export type SourceAuthorizationRef =
  | {
      external_grant_id: ExternalGrantId;
      type: 'external_grant';
    }
  | {
      browser_share_id: BrowserShareId;
      type: 'browser_share';
    }
  | {
      attachment_grant_id: AttachmentGrantId;
      type: 'attachment_grant';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExternalGrantId".
 */
export type ExternalGrantId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserShareId".
 */
export type BrowserShareId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AttachmentGrantId".
 */
export type AttachmentGrantId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NetworkDataClass".
 */
export type NetworkDataClass =
  | 'public_query'
  | 'public_content'
  | 'workspace_metadata'
  | 'workspace_source'
  | 'user_file_content'
  | 'personal_data'
  | 'credential_material'
  | 'security_input'
  | 'password'
  | 'otp'
  | 'cookie'
  | 'private_key'
  | 'suspected_secret'
  | 'diagnostic_data'
  | 'unknown';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelEgressSourceKind".
 */
export type ModelEgressSourceKind =
  | (
      | 'aura_core'
      | 'custom_system_addendum'
      | 'user_event'
      | 'assistant_event'
      | 'tool_result'
      | 'tool_schema'
      | 'workspace_file'
      | 'external_file_text'
      | 'external_file_attachment'
      | 'artifact_attachment'
      | 'browser_text'
      | 'browser_attachment'
      | 'diagnostic_attachment'
      | 'skill'
      | 'hook'
      | 'task_lineage'
      | 'compaction'
    )
  | 'memory_maintenance'
  | 'activity_index';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextPreparationOutcome".
 */
export type ContextPreparationOutcome =
  | {
      context: PreparedLogicalContext;
      type: 'prepared';
    }
  | {
      plan: ContextRemediationPlan;
      type: 'awaiting_remediation';
    }
  | {
      failure: ContextPreparationFailure;
      type: 'interrupted';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "LogicalInstructionKind".
 */
export type LogicalInstructionKind =
  | 'core'
  | 'permission'
  | 'tool_summary'
  | 'workspace_agents'
  | 'historical_snapshot'
  | 'agent_mode'
  | 'hook'
  | 'task_state';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderContinuationState".
 */
export type ProviderContinuationState =
  | {
      state_ref: ArtifactId;
      type: 'open_ai_responses';
    }
  | {
      state_ref: ArtifactId;
      type: 'anthropic';
    }
  | {
      provider_kind: string;
      state_ref: ArtifactId;
      type: 'opaque';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextPreparationTriggerV1".
 */
export type ContextPreparationTriggerV1 =
  | {
      type: 'user_submission';
      user_event_id: EventId;
    }
  | {
      type: 'user_batch';
      user_event_ids: EventId[];
    }
  | {
      assistant_response_event_id: EventId;
      checkpoint_hash: CanonicalSha256HashV1;
      originating_user_event_id: EventId;
      previous_model_step_id: ModelStepId;
      type: 'core_continuation';
    }
  | {
      assistant_response_event_id: EventId;
      history_atomic_group_ref: HistoryAtomicGroupRefV1;
      originating_user_event_id: EventId;
      previous_model_step_id: ModelStepId;
      provider_continuation?: ProviderContinuationBindingV1 | null;
      tool_batch_id: ToolBatchId;
      type: 'tool_continuation';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderId".
 */
export type ProviderId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PromptActivationCondition".
 */
export type PromptActivationCondition =
  | {
      type: 'always';
    }
  | {
      mode: PromptAgentMode;
      type: 'agent_mode';
    }
  | {
      capability: string;
      type: 'capability';
    }
  | {
      type: 'child_agent';
    }
  | {
      recovery_kind: string;
      type: 'recovery';
    }
  | {
      type: 'compaction';
    }
  | {
      maintenance_kind: string;
      type: 'maintenance';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PromptAgentMode".
 */
export type PromptAgentMode = 'normal' | 'plan' | 'goal';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PromptSectionClass".
 */
export type PromptSectionClass =
  | 'core'
  | 'permission'
  | 'tool_summary'
  | 'workspace_agents'
  | 'historical_snapshot'
  | 'agent_mode'
  | 'hook'
  | 'task_state'
  | 'compaction'
  | 'maintenance';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextCompactionStateView".
 */
export type ContextCompactionStateView =
  'not_required' | 'required' | 'running' | 'completed' | 'failed' | 'cancelled';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelChannelDataDisposition".
 */
export type ModelChannelDataDisposition =
  'allowed_by_model_channel' | 'requires_source_egress_grant' | 'always_denied';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PromptInspectorSectionPlacement".
 */
export type PromptInspectorSectionPlacement =
  'instruction_envelope' | 'conversation' | 'current_user_part' | 'attachment';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SkillInstanceId".
 */
export type SkillInstanceId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ErrorCategory".
 */
export type ErrorCategory =
  | 'validation'
  | 'compatibility'
  | 'not_found'
  | 'conflict'
  | 'permission'
  | 'unavailable'
  | 'cancelled'
  | 'transport'
  | 'protocol'
  | 'internal';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CausationId".
 */
export type CausationId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ErrorCode".
 */
export type ErrorCode = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CorrelationId".
 */
export type CorrelationId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExpectedRevision".
 */
export type ExpectedRevision = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CommandId".
 */
export type CommandId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CredentialId".
 */
export type CredentialId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "DeferredMutationId".
 */
export type DeferredMutationId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "DeleteConfirmationId".
 */
export type DeleteConfirmationId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "EditorReservationId".
 */
export type EditorReservationId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "IpcChannelId".
 */
export type IpcChannelId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryId".
 */
export type MemoryId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelAvailabilityTestId".
 */
export type ModelAvailabilityTestId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelContentBlockId".
 */
export type ModelContentBlockId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelDiscoveryRunId".
 */
export type ModelDiscoveryRunId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelFailureId".
 */
export type ModelFailureId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MutationJournalId".
 */
export type MutationJournalId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RendererInstanceId".
 */
export type RendererInstanceId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RuntimeInstanceId".
 */
export type RuntimeInstanceId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "StreamId".
 */
export type StreamId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolConformanceRunId".
 */
export type ToolConformanceRunId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDraftId".
 */
export type ToolDraftId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolExecutionAttemptId".
 */
export type ToolExecutionAttemptId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageId".
 */
export type ToolPackageId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceLifecycleOperationId".
 */
export type WorkspaceLifecycleOperationId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelAvailabilityPrerequisiteFailure".
 */
export type ModelAvailabilityPrerequisiteFailure =
  | 'model_profile_not_found'
  | 'model_profile_not_persisted'
  | 'endpoint_not_found'
  | 'endpoint_configuration_invalid'
  | 'base_url_missing'
  | 'api_format_missing'
  | 'model_name_missing'
  | 'credential_not_configured'
  | 'request_contract_unresolved'
  | 'configuration_revision_changed'
  | 'model_config_mutation_in_progress'
  | 'test_already_running';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelAvailabilityTestState".
 */
export type ModelAvailabilityTestState = 'queued' | 'running' | 'completed' | 'cancelled';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CatalogDecimal".
 */
export type CatalogDecimal = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CatalogPriceCurrency".
 */
export type CatalogPriceCurrency = 'usd';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CatalogPriceUnit".
 */
export type CatalogPriceUnit = 'per_million_tokens';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderCatalogConfigurationClass".
 */
export type ProviderCatalogConfigurationClass =
  'trusted_preset' | 'manual_configuration' | 'catalog_information_only';
/**
 * Renderer-safe initial values for a catalog Provider connection form.
 *
 * Aura presets are compiled into the signed application and may define the
 * complete execution contract. A models.dev API suggestion is intentionally
 * limited to an HTTPS Base URL and must never be treated as trusted execution
 * configuration.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderCatalogConnectionDefaultsView".
 */
export type ProviderCatalogConnectionDefaultsView =
  | {
      source: 'aura_preset';
      variants: ProviderPresetVariantView[];
    }
  | {
      base_url: string;
      source: 'models_dev_api_suggestion';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "EndpointAuthScheme".
 */
export type EndpointAuthScheme = 'none' | 'open_ai_bearer_api_key' | 'anthropic_api_key';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ChatInstructionRole".
 */
export type ChatInstructionRole = 'system' | 'developer';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ChatOutputTokenField".
 */
export type ChatOutputTokenField = 'max_completion_tokens' | 'max_tokens';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelDiscoveryMode".
 */
export type ModelDiscoveryMode =
  | {
      type: 'automatic';
    }
  | {
      path_and_query: string;
      type: 'custom';
    }
  | {
      type: 'disabled';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelCredentialSlot".
 */
export type ModelCredentialSlot = 'api-key';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "EndpointCredentialState".
 */
export type EndpointCredentialState =
  'unconfigured' | 'configured' | 'cleared' | 'unavailable' | 'reconciliation_required';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelDiscoveryState".
 */
export type ModelDiscoveryState = 'queued' | 'running' | 'completed' | 'cancelled';
/**
 * Renderer-safe classification of the Endpoint's currently selected
 * authentication mode. The concrete wire/header scheme remains on
 * `ModelEndpointConfig`; this discriminant only controls active versus
 * dormant credential projection.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "EndpointAuthenticationKind".
 */
export type EndpointAuthenticationKind = 'none' | 'secret';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "EndpointSlug".
 */
export type EndpointSlug = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelOperationCancellationReason".
 */
export type ModelOperationCancellationReason =
  'user_requested' | 'application_restarted' | 'configuration_changed' | 'target_deleted';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelDiscoveryOutcome".
 */
export type ModelDiscoveryOutcome = 'succeeded' | 'partial' | 'failed' | 'unsupported';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelDiscoveryStage".
 */
export type ModelDiscoveryStage = 'requesting' | 'parsing' | 'enriching_catalog' | 'completed';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CatalogMatchState".
 */
export type CatalogMatchState = 'matched' | 'unmatched' | 'ambiguous';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelProfileOrigin".
 */
export type ModelProfileOrigin = 'discovered' | 'manual';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ReasoningEffort".
 */
export type ReasoningEffort = 'off' | 'minimal' | 'low' | 'medium' | 'high' | 'xhigh' | 'max';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderSlug".
 */
export type ProviderSlug = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelProviderSource".
 */
export type ModelProviderSource =
  | {
      catalog_provider_id: string;
      type: 'models_dev';
    }
  | {
      catalog_provider_id?: string | null;
      type: 'custom';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderToolCallWireIdentity".
 */
export type ProviderToolCallWireIdentity =
  | {
      provider_tool_call_id: ProviderToolCallId;
      reuse_domain: ProviderToolCallReuseDomain;
      type: 'provider_opaque';
    }
  | {
      provider_wire_tool_call_id: ProviderWireToolCallId;
      type: 'aura_generated';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderToolCallId".
 */
export type ProviderToolCallId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderWireToolCallId".
 */
export type ProviderWireToolCallId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelFailureCategory".
 */
export type ModelFailureCategory =
  | 'authentication'
  | 'authorization'
  | 'rate_limit'
  | 'quota_or_balance'
  | 'invalid_request'
  | 'unsupported_parameter'
  | 'unsupported_model'
  | 'context_window_exceeded'
  | 'transport'
  | 'stream_interrupted'
  | 'protocol'
  | 'content_filter'
  | 'refusal'
  | 'empty_output'
  | 'invalid_tool_call'
  | 'persistence'
  | 'cancelled'
  | 'unknown';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelRetryClass".
 */
export type ModelRetryClass =
  | {
      type: 'not_retryable';
    }
  | {
      retry_after_ms?: number | null;
      type: 'transparent_retry';
    }
  | {
      parameter: string;
      type: 'compatibility_adjustment';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NormalizedStopReason".
 */
export type NormalizedStopReason =
  | 'end_turn'
  | 'tool_calls'
  | 'max_output_tokens'
  | 'stop_sequence'
  | 'refusal'
  | 'content_filter'
  | 'cancelled'
  | 'provider_incomplete'
  | 'unknown';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "UsageSource".
 */
export type UsageSource = 'provider_reported' | 'local_exact_tokenizer' | 'local_estimated';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderRequestHeaderClass".
 */
export type ProviderRequestHeaderClass =
  | 'protocol_fixed'
  | 'semantic_routing'
  | 'credential_injected'
  | 'content_metadata'
  | 'other_redacted';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TransportOperationContext".
 */
export type TransportOperationContext =
  | {
      context_snapshot_id: ContextProjectionSnapshotId;
      model_execution_snapshot_id: ModelExecutionSnapshotId;
      model_step_id: ModelStepId;
      prompt_bundle_snapshot_id: PromptBundleSnapshotId;
      type: 'agent_model_attempt';
    }
  | {
      discovery_run_id: ModelDiscoveryRunId;
      type: 'model_discovery';
    }
  | {
      model_availability_test_id: ModelAvailabilityTestId;
      model_profile_id: ModelProfileId;
      type: 'model_availability_test';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelStreamEvent".
 */
export type ModelStreamEvent =
  | {
      provider_request_id?: string | null;
      type: 'request_accepted';
    }
  | {
      type: 'response_started';
    }
  | {
      block_id: ModelContentBlockId;
      type: 'assistant_text_started';
    }
  | {
      block_id: ModelContentBlockId;
      text: string;
      type: 'assistant_text_delta';
    }
  | {
      block_id: ModelContentBlockId;
      type: 'reasoning_started';
    }
  | {
      block_id: ModelContentBlockId;
      text: string;
      type: 'reasoning_delta';
    }
  | {
      block_id: ModelContentBlockId;
      text: string;
      type: 'reasoning_summary_delta';
    }
  | {
      text: string;
      type: 'refusal_delta';
    }
  | {
      position: number;
      provider_call_id: ProviderToolCallId;
      tool_name: string;
      type: 'tool_call_started';
    }
  | {
      delta: string;
      provider_call_id: ProviderToolCallId;
      type: 'tool_call_arguments_delta';
    }
  | {
      provider_call_id: ProviderToolCallId;
      type: 'tool_call_completed';
    }
  | {
      capability_name: string;
      payload_ref: ArtifactId;
      type: 'native_capability_event';
    }
  | {
      type: 'usage_updated';
      usage: PartialModelUsage;
    }
  | {
      code?: string | null;
      message: string;
      type: 'provider_warning';
    }
  | {
      stop_reason: NormalizedStopReason;
      type: 'response_completed';
    }
  | {
      reason: ModelIncompleteReason;
      type: 'response_incomplete';
    }
  | {
      failure: NormalizedModelFailure;
      type: 'stream_error';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelIncompleteReason".
 */
export type ModelIncompleteReason =
  | {
      type: 'max_output_tokens';
    }
  | {
      type: 'content_filter';
    }
  | {
      type: 'stream_interrupted';
    }
  | {
      code: string;
      type: 'provider_reported';
    }
  | {
      type: 'unknown';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TransportAttemptKind".
 */
export type TransportAttemptKind = 'initial' | 'transparent_retry' | 'compatibility_adjusted_retry';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TransportAttemptState".
 */
export type TransportAttemptState =
  | 'queued_for_endpoint'
  | 'acquiring_permit'
  | 'preparing_request'
  | 'authorizing_network'
  | 'authorized'
  | 'dispatch_prepared'
  | 'connecting'
  | 'response_started'
  | 'response_completed'
  | 'awaiting_first_event'
  | 'streaming'
  | 'completed'
  | 'cancelled'
  | 'failed';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ArtifactOwnerRef".
 */
export type ArtifactOwnerRef =
  | {
      session_id: SessionId;
      type: 'session';
    }
  | {
      operation: ArtifactOperationOwnerRef;
      type: 'operation';
    }
  | {
      owner: ArtifactSystemOwner;
      type: 'system';
    }
  | {
      owner: ToolArtifactOwnerRef;
      type: 'tool';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ArtifactOperationOwnerRef".
 */
export type ArtifactOperationOwnerRef =
  | {
      model_discovery_run_id: ModelDiscoveryRunId;
      type: 'model_discovery';
    }
  | {
      model_availability_test_id: ModelAvailabilityTestId;
      type: 'model_availability_test';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ArtifactSystemOwner".
 */
export type ArtifactSystemOwner = 'provider_catalog_cache';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolArtifactOwnerRef".
 */
export type ToolArtifactOwnerRef =
  | {
      package_fingerprint: CanonicalSha256HashV1;
      package_version: CanonicalSemVer;
      tool_package_id: ToolPackageId;
      type: 'package_version';
    }
  | {
      tool_draft_id: ToolDraftId;
      type: 'draft';
    }
  | {
      tool_conformance_run_id: ToolConformanceRunId;
      type: 'conformance_run';
    }
  | {
      tool_batch_id: ToolBatchId;
      type: 'batch';
    }
  | {
      tool_call_id: ToolCallId;
      type: 'tool_call';
    }
  | {
      tool_execution_attempt_id: ToolExecutionAttemptId;
      type: 'execution_attempt';
    }
  | {
      tool_registry_snapshot_id: ToolRegistrySnapshotId;
      type: 'registry_snapshot';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ArtifactScope".
 */
export type ArtifactScope =
  | 'session'
  | 'runtime_ephemeral'
  | 'workspace_cache'
  | 'system_cache'
  | 'quarantine'
  | 'package_store'
  | 'tool_execution';
/**
 * An opaque reference to a secret stored in Windows Credential Manager.
 *
 * The reference is safe to persist and log. The secret value it names is not.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CredentialRef".
 */
export type CredentialRef = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProjectionChange".
 */
export type ProjectionChange =
  | {
      payload: CoreHealthProjection;
      projection_kind: 'core_health';
      projection_scope: {
        type: 'global';
      };
      revision: Revision;
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      payload: WorkspaceSessionProjection;
      projection_kind: 'workspace_session';
      projection_scope: {
        type: 'global';
      };
      revision: Revision;
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      payload: WorkspaceFilesProjection;
      projection_kind: 'workspace_files';
      projection_scope: {
        type: 'global';
      };
      revision: Revision;
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      payload: ModelSettingsProjection;
      projection_kind: 'model_settings';
      projection_scope: {
        type: 'global';
      };
      revision: Revision;
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      payload: ProviderCatalogProjection;
      projection_kind: 'provider_catalog';
      projection_scope: {
        type: 'global';
      };
      revision: Revision;
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      payload: EndpointDetailProjection;
      projection_kind: 'endpoint_detail';
      projection_scope: {
        endpoint_id: EndpointId;
        type: 'endpoint';
      };
      revision: Revision;
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      payload: ModelProfileProjection;
      projection_kind: 'model_profile';
      projection_scope: {
        model_profile_id: ModelProfileId;
        type: 'model_profile';
      };
      revision: Revision;
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      payload: SessionModelSelectorProjection;
      projection_kind: 'session_model_selector';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
      revision: Revision;
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      payload: ModelAvailabilityTestProjection;
      projection_kind: 'model_availability_test';
      projection_scope: {
        model_availability_test_id: ModelAvailabilityTestId;
        type: 'model_availability_test';
      };
      revision: Revision;
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      payload: ModelDiscoveryProjection;
      projection_kind: 'model_discovery';
      projection_scope: {
        model_discovery_run_id: ModelDiscoveryRunId;
        type: 'model_discovery';
      };
      revision: Revision;
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      payload: ContextWindowCorrectionProjection;
      projection_kind: 'context_window_correction';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
      revision: Revision;
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      payload: PromptInspectorProjection;
      projection_kind: 'prompt_inspector';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
      revision: Revision;
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      payload: ToolSettingsProjection;
      projection_kind: 'tool_settings';
      projection_scope: {
        type: 'global';
      };
      revision: Revision;
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      payload: ToolPackageListProjection;
      projection_kind: 'tool_package_list';
      projection_scope: {
        type: 'global';
      };
      revision: Revision;
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      payload: ToolDraftListProjection;
      projection_kind: 'tool_draft_list';
      projection_scope: {
        type: 'global';
      };
      revision: Revision;
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      payload: WorkspaceToolPinsProjection;
      projection_kind: 'workspace_tool_pins';
      projection_scope: {
        type: 'workspace';
        workspace_id: WorkspaceId;
      };
      revision: Revision;
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      payload: WorkspaceToolRegistrySummaryProjection;
      projection_kind: 'workspace_tool_registry_summary';
      projection_scope: {
        type: 'workspace';
        workspace_id: WorkspaceId;
      };
      revision: Revision;
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      payload: SessionEffectiveToolRegistryProjection;
      projection_kind: 'session_effective_tool_registry';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
      revision: Revision;
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      payload: SessionLoadedToolsProjection;
      projection_kind: 'session_loaded_tools';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
      revision: Revision;
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      payload: SessionToolActivityProjection;
      projection_kind: 'session_tool_activity';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
      revision: Revision;
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      payload: SessionToolPinsProjectionV1;
      projection_kind: 'session_tool_pins';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
      revision: Revision;
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      payload: ToolPackageProjection;
      projection_kind: 'tool_package';
      projection_scope: {
        tool_package_id: ToolPackageId;
        type: 'tool_package';
      };
      revision: Revision;
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      payload: ToolDefinitionProjection;
      projection_kind: 'tool_definition';
      projection_scope: {
        tool_id: ToolId;
        type: 'tool_definition';
      };
      revision: Revision;
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      payload: ToolDraftProjection;
      projection_kind: 'tool_draft';
      projection_scope: {
        tool_draft_id: ToolDraftId;
        type: 'tool_draft';
      };
      revision: Revision;
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      payload: ToolConformanceRunProjection;
      projection_kind: 'tool_conformance_run';
      projection_scope: {
        tool_conformance_run_id: ToolConformanceRunId;
        type: 'tool_conformance_run';
      };
      revision: Revision;
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      payload: ToolBatchProjection;
      projection_kind: 'tool_batch';
      projection_scope: {
        tool_batch_id: ToolBatchId;
        type: 'tool_batch';
      };
      revision: Revision;
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      payload: ToolCallProjection;
      projection_kind: 'tool_call';
      projection_scope: {
        tool_call_id: ToolCallId;
        type: 'tool_call';
      };
      revision: Revision;
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      payload: ToolExecutionAttemptProjection;
      projection_kind: 'tool_execution_attempt';
      projection_scope: {
        tool_execution_attempt_id: ToolExecutionAttemptId;
        type: 'tool_execution_attempt';
      };
      revision: Revision;
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      payload: ToolLongOperationProjectionV1;
      projection_kind: 'tool_long_operation';
      projection_scope: {
        journal_id: MutationJournalId;
        type: 'tool_long_operation';
      };
      revision: Revision;
      runtime_instance_id: RuntimeInstanceId;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CoreBusyActivityV1".
 */
export type CoreBusyActivityV1 = 'deleting_session' | 'removing_workspace' | 'finishing_deletion';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CoreLifecycleState".
 */
export type CoreLifecycleState =
  | 'not_started'
  | 'starting'
  | 'ready'
  | 'restarting'
  | 'incompatible'
  | 'unavailable'
  | 'app_crash_escalated'
  | 'recovery_required';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CatalogFreshness".
 */
export type CatalogFreshness = 'unavailable' | 'rebuilding' | 'fresh' | 'stale';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceState".
 */
export type WorkspaceState = 'online' | 'offline' | 'archived' | 'reconciliation_required';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceLifecycleOperationKind".
 */
export type WorkspaceLifecycleOperationKind = 'archive' | 'relocate' | 'permanent_delete';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceLifecycleBlocker".
 */
export type WorkspaceLifecycleBlocker =
  | {
      blocker_kind: 'dirty_buffer';
      buffer_revision: Revision;
      path: string;
      session_id: SessionId;
    }
  | {
      blocker_kind: 'active_editing';
      ime_composition_active: boolean;
      path: string;
      session_id: SessionId;
    }
  | {
      blocker_kind: 'pending_buffer_synchronization';
      core_revision: Revision;
      path: string;
      renderer_revision: Revision;
      session_id: SessionId;
    }
  | {
      blocker_kind: 'save_in_progress';
      path: string;
      session_id: SessionId;
    }
  | {
      blocker_kind: 'save_failed';
      error_code: string;
      path: string;
      session_id: SessionId;
    }
  | {
      blocker_kind: 'file_version_conflict';
      path: string;
      session_id: SessionId;
    }
  | {
      blocker_kind: 'editor_state_unconfirmed';
      path?: string | null;
      session_id?: SessionId | null;
    }
  | {
      blocker_kind: 'deferred_mutation_exists';
      path: string;
      session_id: SessionId;
    }
  | {
      blocker_kind: 'runtime_objects_active';
      count: number;
    }
  | {
      blocker_kind: 'runtime_leases_active';
      count: number;
    }
  | {
      blocker_kind: 'mutation_ownership_active';
      count: number;
    }
  | {
      blocker_kind: 'unfinished_journals';
      count: number;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceLifecycleState".
 */
export type WorkspaceLifecycleState =
  | 'preparing'
  | 'blocked'
  | 'saving'
  | 'discarding'
  | 'committing'
  | 'completed'
  | 'cancelled'
  | 'failed';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CatalogIndexMode".
 */
export type CatalogIndexMode = 'full_text' | 'metadata_only';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceWatcherState".
 */
export type WorkspaceWatcherState =
  | 'initializing'
  | 'watching'
  | 'coalescing'
  | 'rescan_required'
  | 'rescanning'
  | 'polling_fallback'
  | 'failed';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "EditorSaveState".
 */
export type EditorSaveState =
  | {
      save_state: 'idle';
    }
  | {
      save_state: 'saving';
    }
  | {
      error_code: string;
      save_state: 'failed';
    }
  | {
      save_state: 'conflict';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderCatalogState".
 */
export type ProviderCatalogState =
  | 'unavailable'
  | 'loading_without_cache'
  | 'fresh'
  | 'stale'
  | 'refreshing_with_lkg'
  | 'refresh_failed_with_lkg';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelAvailabilityCheckResult".
 */
export type ModelAvailabilityCheckResult =
  | {
      latency_ms?: number | null;
      status: 'passed';
    }
  | {
      /**
       * A Renderer-safe, stable issue code from Aura's v1 normalized registry.
       *
       * This is deliberately a distinct domain type. Raw provider, Win32, DNS, or
       * TLS diagnostics must not be converted into it or embedded in its suffix.
       */
      error_code: string;
      provider_status?: number | null;
      status: 'failed';
    }
  | {
      reason_code: ModelAvailabilityCheckNotRunReason;
      status: 'not_run';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelAvailabilityCheckNotRunReason".
 */
export type ModelAvailabilityCheckNotRunReason =
  | 'blocked_by_previous_stage'
  | 'not_applicable_for_scheme'
  | 'no_authentication_required'
  | 'authentication_not_confirmed';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelAvailabilityTestOutcome".
 */
export type ModelAvailabilityTestOutcome = 'available' | 'unavailable';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelAvailabilityTestStage".
 */
export type ModelAvailabilityTestStage =
  'config' | 'network' | 'tls' | 'http' | 'authentication' | 'model_generation' | 'completed';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextWindowResolution".
 */
export type ContextWindowResolution =
  | {
      current_effective_window_tokens: number;
      failed_execution_snapshot_id: ModelExecutionSnapshotId;
      local_estimated_input_tokens: number;
      model_profile_id: ModelProfileId;
      provider_error_ref: ArtifactId;
      type: 'p06_rebuild_required';
    }
  | {
      current_effective_window_tokens: number;
      failed_execution_snapshot_id: ModelExecutionSnapshotId;
      input_contract: ContextWindowTokenInputContract;
      local_estimated_input_tokens: number;
      model_profile: ModelProfileConfig;
      provider_error_ref: ArtifactId;
      type: 'user_correction_required';
    }
  | {
      candidate_window_tokens: number;
      failed_execution_snapshot_id: ModelExecutionSnapshotId;
      model_profile_id: ModelProfileId;
      provider_error_ref: ArtifactId;
      remaining_candidates: number[];
      type: 'automatic_candidate_selected';
    }
  | {
      corrected_window_tokens: number;
      correction_basis: RuntimeWindowCorrectionBasis;
      failed_execution_snapshot_id: ModelExecutionSnapshotId;
      model_profile_id: ModelProfileId;
      models_revision: Revision;
      provider_error_ref: ArtifactId;
      type: 'runtime_correction_applied';
    }
  | {
      current_effective_window_tokens: number;
      failed_execution_snapshot_id: ModelExecutionSnapshotId;
      input_contract: ContextWindowTokenInputContract;
      local_estimated_input_tokens: number;
      model_profile: ModelProfileConfig;
      provider_error_ref: ArtifactId;
      type: 'automatic_candidates_exhausted';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextWindowUnit".
 */
export type ContextWindowUnit = 'tokens';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolNetworkProxyProtocolV1".
 */
export type ToolNetworkProxyProtocolV1 = 'http' | 'socks5';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageLifecycleV1".
 */
export type ToolPackageLifecycleV1 = 'installed' | 'corrupted' | 'deleted';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageKey".
 */
export type PackageKey = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDraftKindV1".
 */
export type ToolDraftKindV1 = 'content_mutation' | 'package_deletion';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDraftStatusV1".
 */
export type ToolDraftStatusV1 =
  | 'editing'
  | 'validating'
  | 'validation_failed'
  | 'ready_for_test'
  | 'testing'
  | 'test_failed'
  | 'ready_to_apply'
  | 'applying'
  | 'applied'
  | 'discarded'
  | 'recovery_required';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPinResolutionStateV1".
 */
export type ToolPinResolutionStateV1 = 'resolved' | 'unresolved' | 'degraded';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolAudienceV1".
 */
export type ToolAudienceV1 =
  'child_runtime_required' | 'subagent_configurable' | 'parent_agent_only';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolRuntimeHealthV1".
 */
export type ToolRuntimeHealthV1 = 'healthy' | 'unavailable' | 'corrupted';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolBatchStatusV1".
 */
export type ToolBatchStatusV1 =
  'pending_admission' | 'admitted' | 'running' | 'cancelling' | 'completed' | 'recovery_required';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolActivityCursorV1".
 */
export type ToolActivityCursorV1 = string;
/**
 * Why a terminal call ended the way it did, in the terms the session shows.
 *
 * It is the stored error's own category — never a reading of the message text — so the
 * row, the group header and the chip can name the same reason the Store recorded, and a
 * `not_found` never reads as a permission problem. `Other` is the honest answer for a
 * category this vocabulary does not distinguish; a legacy row with no error carries
 * `None` instead, which is a different fact from "something else went wrong".
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCallErrorClassV1".
 */
export type ToolCallErrorClassV1 =
  'not_found' | 'permission' | 'conflict' | 'invalid_request' | 'unavailable' | 'other';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolResultOutcomeSummaryView".
 */
export type ToolResultOutcomeSummaryView =
  | {
      type: 'succeeded';
    }
  | {
      type: 'operation_rejected';
    }
  | {
      type: 'failed';
    }
  | {
      reason: ToolCancellationReasonV1;
      type: 'cancelled';
    }
  | {
      reason: ToolNotExecutedReasonV1;
      type: 'not_executed';
    }
  | {
      reason: ToolIndeterminateReasonV1;
      type: 'indeterminate';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCancellationReasonV1".
 */
export type ToolCancellationReasonV1 =
  | 'user_requested'
  | 'turn_interrupted'
  | 'permission_revoked'
  | 'scope_closed'
  | 'core_shutdown'
  | 'timeout'
  | 'parent_agent_cancelled'
  | 'batch_cancelled';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolNotExecutedReasonV1".
 */
export type ToolNotExecutedReasonV1 =
  | 'schema_validation_failed'
  | 'tool_not_found'
  | 'version_mismatch'
  | 'not_loaded'
  | 'disabled'
  | 'trust_required'
  | 'audience_denied'
  | 'purpose_required'
  | 'permission_denied'
  | 'approval_denied'
  | 'runtime_unavailable'
  | 'dependency_not_executed'
  | 'atomic_group_not_executed'
  | 'lineage_reconciliation'
  | 'completion_review'
  | 'cancelled_before_dispatch'
  | 'batch_rejected'
  | 'resource_claim_resolution_failed';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolIndeterminateReasonV1".
 */
export type ToolIndeterminateReasonV1 =
  | 'dispatch_outcome_unknown'
  | 'runtime_terminal_missing'
  | 'runtime_termination_unknown'
  | 'external_effect_unknown'
  | 'result_evidence_invalid'
  | 'persistence_outcome_unknown'
  | 'reconciliation_inconclusive';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SideEffectStateV1".
 */
export type SideEffectStateV1 =
  | (
      | 'not_started'
      | 'no_effect'
      | 'fully_applied'
      | 'partially_applied'
      | 'compensated'
      | 'unknown'
    )
  | 'unassessed';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCallStatusV1".
 */
export type ToolCallStatusV1 =
  | 'pending_admission'
  | 'waiting_dependency'
  | 'waiting_permission'
  | 'waiting_approval'
  | 'waiting_resource'
  | 'queued'
  | 'running'
  | 'cancelling'
  | 'finalized'
  | 'recovery_required';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCredentialPlacementV1".
 */
export type ToolCredentialPlacementV1 =
  | {
      environment_variable: EnvironmentVariableNameV1;
      placement: 'command_environment';
    }
  | {
      placement: 'http_bearer_authorization';
    }
  | {
      header_name: HttpHeaderNameV1;
      placement: 'http_fixed_header';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "EnvironmentVariableNameV1".
 */
export type EnvironmentVariableNameV1 = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HttpHeaderNameV1".
 */
export type HttpHeaderNameV1 = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CredentialSlotKey".
 */
export type CredentialSlotKey = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolConformanceStatusV1".
 */
export type ToolConformanceStatusV1 =
  'queued' | 'running' | 'passed' | 'failed' | 'cancelled' | 'recovery_required';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDefinitionCurrentSelectionV1".
 */
export type ToolDefinitionCurrentSelectionV1 =
  | {
      selection_basis: ToolDefinitionSelectionBasisV1;
      source_selection_ref: ToolDefinitionSourceSelectionRefV1;
      type: 'resolved';
    }
  | {
      reason: ToolDefinitionSelectionIssueV1;
      type: 'unavailable';
    }
  | {
      candidates: ToolDefinitionSourceSelectionRefV1[];
      type: 'ambiguous';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDefinitionSelectionBasisV1".
 */
export type ToolDefinitionSelectionBasisV1 =
  | 'signed_builtin_release'
  | 'active_package_version'
  | 'external_source_authority'
  | 'explicit_instance_selection';
/**
 * Source-owned identity used only by the global Tool Definition projection.
 *
 * This reference deliberately contains no `trs_` binding. Callers must
 * resolve it again through the target Workspace, Session, or child Registry
 * before Load or Execute can reach admission.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDefinitionSourceSelectionRefV1".
 */
export type ToolDefinitionSourceSelectionRefV1 =
  | {
      definition_fingerprint: CanonicalSha256HashV1;
      signed_builtin_release_hash: CanonicalSha256HashV1;
      source: 'signed_builtin_release';
      source_contract_fingerprint: CanonicalSha256HashV1;
      tool_id: ToolId;
      tool_instance_id: ToolInstanceId;
      tool_instance_revision: Revision;
      tool_version: ToolVersion;
    }
  | {
      definition_fingerprint: CanonicalSha256HashV1;
      package_fingerprint: CanonicalSha256HashV1;
      package_revision: Revision;
      package_security_fingerprint: CanonicalBlake3HashV1;
      package_version: CanonicalSemVer;
      source: 'active_package_version';
      tool_id: ToolId;
      tool_instance_id: ToolInstanceId;
      tool_instance_revision: Revision;
      tool_package_id: ToolPackageId;
      tool_version: ToolVersion;
    }
  | {
      authority: ToolDefinitionExternalSourceAuthorityRefV1;
      definition_fingerprint: CanonicalSha256HashV1;
      source: 'external_source_authority';
      tool_id: ToolId;
      tool_instance_id: ToolInstanceId;
      tool_instance_revision: Revision;
      tool_version: ToolVersion;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDefinitionExternalSourceAuthorityRefV1".
 */
export type ToolDefinitionExternalSourceAuthorityRefV1 =
  | {
      authority: McpToolSourceAuthorityRefV1;
      source_kind: 'mcp';
    }
  | {
      authority: PluginToolSourceAuthorityRefV1;
      source_kind: 'plugin';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDefinitionSelectionIssueV1".
 */
export type ToolDefinitionSelectionIssueV1 =
  | 'release_definition_unavailable'
  | 'active_package_version_unavailable'
  | 'external_source_authority_unavailable'
  | 'no_ready_instance'
  | 'explicit_selection_unavailable';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CanonicalToolPackageInstanceConfigurationSchemaV1".
 */
export type CanonicalToolPackageInstanceConfigurationSchemaV1 =
  CanonicalJsonSchemaDocumentV1 | null;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolResultContractV1".
 */
export type ToolResultContractV1 =
  | {
      type: 'unit';
    }
  | {
      type: 'utf8_text';
    }
  | {
      result_schema: CanonicalJsonSchemaDocumentV1;
      type: 'structured_json';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDraftActorV1".
 */
export type ToolDraftActorV1 = 'user' | 'parent_agent' | 'child_agent' | 'system_runtime';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDraftApprovalDecisionV1".
 */
export type ToolDraftApprovalDecisionV1 = 'install' | 'install_and_trust' | 'reject';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageDeleteApprovalDecisionV1".
 */
export type ToolPackageDeleteApprovalDecisionV1 = 'approve_delete' | 'reject';
export type PackageRelativePath = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageFileRoleV1".
 */
export type ToolPackageFileRoleV1 =
  | 'runtime_executable'
  | 'runtime_script'
  | 'runtime_library'
  | 'runtime_resource'
  | 'conformance_fixture'
  | 'documentation'
  | 'license';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageManifestSchemaVersionV1".
 */
export type ToolPackageManifestSchemaVersionV1 = 1;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageToolAudienceV1".
 */
export type PackageToolAudienceV1 = 'subagent_configurable' | 'parent_agent_only';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageCapabilityRequirementV1".
 */
export type PackageCapabilityRequirementV1 =
  | 'workspace_file_read'
  | 'workspace_file_write'
  | 'workspace_file_create'
  | 'workspace_file_delete'
  | 'external_file_read'
  | 'external_file_write'
  | 'external_file_create'
  | 'external_file_delete'
  | 'process_execute'
  | 'git_read'
  | 'git_write'
  | 'git_remote_read'
  | 'git_remote_write'
  | 'git_destructive'
  | 'public_network_read'
  | 'private_network_access'
  | 'data_egress'
  | 'download_artifact'
  | 'download_windows_executable'
  | 'external_service_read'
  | 'external_service_mutation'
  | 'external_service_destructive_mutation';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageConformanceCaseNameV1".
 */
export type ToolPackageConformanceCaseNameV1 = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageModelDescriptionV1".
 */
export type ToolPackageModelDescriptionV1 = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageModelSummaryV1".
 */
export type ToolPackageModelSummaryV1 = string;
/**
 * Package-authored resource requirements. Every variant exposes only the
 * selectors and invocation contexts frozen for that resource class.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageDeclaredResourceClaimV1".
 */
export type PackageDeclaredResourceClaimV1 =
  | {
      access: FileResourceAccessV1;
      claim_key: PackageResourceClaimKeyV1;
      selector: PackageWorkspacePathSelectorV1;
      type: 'workspace_path';
    }
  | {
      access: FileResourceAccessV1;
      claim_key: PackageResourceClaimKeyV1;
      selector: PackageExternalFileSelectorV1;
      type: 'external_file';
    }
  | {
      access: NetworkResourceAccessV1;
      claim_key: PackageResourceClaimKeyV1;
      selector: PackageNetworkTargetSelectorV1;
      type: 'network_target';
    }
  | {
      access: ProcessResourceAccessV1;
      claim_key: PackageResourceClaimKeyV1;
      selector: PackageProcessSelectorV1;
      type: 'process';
    }
  | {
      access: TerminalResourceAccessV1;
      claim_key: PackageResourceClaimKeyV1;
      selector: PackageTerminalSessionSelectorV1;
      type: 'terminal_session';
    }
  | {
      access: RepositoryResourceAccessV1;
      claim_key: PackageResourceClaimKeyV1;
      repository_selector: PackageGitRepositorySelectorV1;
      type: 'git_repository';
    }
  | {
      access: ReadWriteResourceAccessV1;
      claim_key: PackageResourceClaimKeyV1;
      repository_selector: PackageGitRepositorySelectorV1;
      type: 'git_index';
    }
  | {
      access: GitRefResourceAccessV1;
      claim_key: PackageResourceClaimKeyV1;
      ref_selector: PackageGitRefSelectorV1;
      repository_selector: PackageGitRepositorySelectorV1;
      type: 'git_ref';
    }
  | {
      access: GitSequencerResourceAccessV1;
      claim_key: PackageResourceClaimKeyV1;
      repository_selector: PackageGitRepositorySelectorV1;
      type: 'git_sequencer';
    }
  | {
      claim_key: PackageResourceClaimKeyV1;
      selector: PackageArtifactSelectorV1;
      type: 'artifact';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FileResourceAccessV1".
 */
export type FileResourceAccessV1 = 'read' | 'write' | 'create' | 'delete' | 'execute' | 'observe';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageResourceClaimKeyV1".
 */
export type PackageResourceClaimKeyV1 = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageWorkspacePathSelectorV1".
 */
export type PackageWorkspacePathSelectorV1 =
  | {
      path_scope: PackagePathScopeV1;
      pointer: ArgumentJsonPointer;
      type: 'argument_json_pointer';
    }
  | {
      path_scope: PackageWorkspaceRootPathScopeV1;
      type: 'workspace_root';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackagePathScopeV1".
 */
export type PackagePathScopeV1 = 'exact' | 'subtree';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ArgumentJsonPointer".
 */
export type ArgumentJsonPointer = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageWorkspaceRootPathScopeV1".
 */
export type PackageWorkspaceRootPathScopeV1 = 'subtree';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageExternalFileSelectorV1".
 */
export type PackageExternalFileSelectorV1 = {
  path_scope: PackagePathScopeV1;
  pointer: ArgumentJsonPointer;
  type: 'argument_json_pointer';
};
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NetworkResourceAccessV1".
 */
export type NetworkResourceAccessV1 = 'read' | 'write' | 'observe';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageNetworkTargetSelectorV1".
 */
export type PackageNetworkTargetSelectorV1 =
  | {
      type: 'runtime_http_target';
    }
  | {
      pointer: ArgumentJsonPointer;
      type: 'argument_json_pointer';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProcessResourceAccessV1".
 */
export type ProcessResourceAccessV1 = 'execute' | 'observe' | 'control';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageProcessSelectorV1".
 */
export type PackageProcessSelectorV1 = {
  pointer: ArgumentJsonPointer;
  type: 'argument_json_pointer';
};
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TerminalResourceAccessV1".
 */
export type TerminalResourceAccessV1 = 'observe' | 'control';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageTerminalSessionSelectorV1".
 */
export type PackageTerminalSessionSelectorV1 = {
  pointer: ArgumentJsonPointer;
  type: 'argument_json_pointer';
};
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RepositoryResourceAccessV1".
 */
export type RepositoryResourceAccessV1 = 'read' | 'write' | 'observe';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageGitRepositorySelectorV1".
 */
export type PackageGitRepositorySelectorV1 =
  | {
      pointer: ArgumentJsonPointer;
      type: 'argument_json_pointer';
    }
  | {
      type: 'workspace_root';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ReadWriteResourceAccessV1".
 */
export type ReadWriteResourceAccessV1 = 'read' | 'write';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GitRefResourceAccessV1".
 */
export type GitRefResourceAccessV1 = 'read' | 'write' | 'create' | 'delete';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageGitRefSelectorV1".
 */
export type PackageGitRefSelectorV1 = {
  pointer: ArgumentJsonPointer;
  type: 'argument_json_pointer';
};
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GitSequencerResourceAccessV1".
 */
export type GitSequencerResourceAccessV1 = 'observe' | 'control';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageArtifactSelectorV1".
 */
export type PackageArtifactSelectorV1 =
  | {
      access: ArtifactResourceAccessV1;
      pointer: ArgumentJsonPointer;
      type: 'argument_json_pointer';
    }
  | {
      access: PackageAttemptStagingArtifactAccessV1;
      type: 'attempt_staging';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ArtifactResourceAccessV1".
 */
export type ArtifactResourceAccessV1 = 'read' | 'create' | 'delete';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageAttemptStagingArtifactAccessV1".
 */
export type PackageAttemptStagingArtifactAccessV1 = 'create';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CanonicalToolPackageRuntimeV1".
 */
export type CanonicalToolPackageRuntimeV1 =
  | {
      command: CanonicalToolPackageDeclarativeCommandV1;
      type: 'declarative_command';
    }
  | {
      arguments: ToolPackageCommandArgumentV1[];
      credential_slot: CommandCredentialEnvironmentSlotV1 | null;
      environment: CommandEnvironmentEntryV1[];
      /**
       * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
       * via the `definition` "PackageRelativePath".
       */
      executable: PackageRelativePath;
      protocol_version: ToolPackageStdioProtocolVersionV1;
      type: 'aura_stdio';
      working_directory: CommandWorkingDirectoryV1;
    }
  | {
      request: CanonicalToolPackageDeclarativeHttpV1;
      type: 'declarative_http';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CanonicalToolPackageDeclarativeCommandV1".
 */
export type CanonicalToolPackageDeclarativeCommandV1 =
  | {
      arguments: ToolPackageCommandArgumentV1[];
      credential_slot: CommandCredentialEnvironmentSlotV1 | null;
      environment: CommandEnvironmentEntryV1[];
      /**
       * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
       * via the `definition` "PackageRelativePath".
       */
      executable: PackageRelativePath;
      success_exit_codes: ToolPackageCommandSuccessExitCodesV1;
      type: 'argv';
      working_directory: CommandWorkingDirectoryV1;
    }
  | {
      arguments: ToolPackageCommandArgumentV1[];
      credential_slot: CommandCredentialEnvironmentSlotV1 | null;
      environment: CommandEnvironmentEntryV1[];
      /**
       * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
       * via the `definition` "PackageRelativePath".
       */
      script: PackageRelativePath;
      success_exit_codes: ToolPackageCommandSuccessExitCodesV1;
      type: 'powershell_script';
      working_directory: CommandWorkingDirectoryV1;
    };
/**
 * Declarative argv mapping authored by a Package. `NamedOption` carries the
 * frozen omission rule explicitly instead of relying on missing-value
 * coercion in a Runtime implementation.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageCommandArgumentV1".
 */
export type ToolPackageCommandArgumentV1 =
  | {
      type: 'literal';
      value: CommandLiteralV1;
    }
  | {
      source: ArgumentJsonPointer;
      type: 'scalar';
    }
  | {
      source: ArgumentJsonPointer;
      type: 'repeated';
    }
  | {
      flag: CommandFlagV1;
      source: ArgumentJsonPointer;
      type: 'conditional_flag';
    }
  | {
      name: CommandOptionNameV1;
      omit_when_missing_or_null: boolean;
      source: ArgumentJsonPointer;
      type: 'named_option';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CommandLiteralV1".
 */
export type CommandLiteralV1 = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CommandFlagV1".
 */
export type CommandFlagV1 = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CommandOptionNameV1".
 */
export type CommandOptionNameV1 = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CommandEnvironmentValueV1".
 */
export type CommandEnvironmentValueV1 =
  | {
      type: 'literal';
      value: EnvironmentLiteralValueV1;
    }
  | {
      source: ArgumentJsonPointer;
      type: 'scalar';
    }
  | {
      key: InstanceConfigurationKeyV1;
      type: 'instance_configuration';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "EnvironmentLiteralValueV1".
 */
export type EnvironmentLiteralValueV1 = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "InstanceConfigurationKeyV1".
 */
export type InstanceConfigurationKeyV1 = string;
/**
 * @minItems 1
 * @maxItems 32
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageCommandSuccessExitCodesV1".
 */
export type ToolPackageCommandSuccessExitCodesV1 = [number, ...number[]];
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CommandWorkingDirectoryV1".
 */
export type CommandWorkingDirectoryV1 =
  | {
      type: 'package_root';
    }
  | {
      type: 'workspace_root';
    }
  | {
      claim_key: PackageResourceClaimKeyV1;
      type: 'resolved_directory_claim';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageStdioProtocolVersionV1".
 */
export type ToolPackageStdioProtocolVersionV1 = 1;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "DeclarativeHttpBodyV1".
 */
export type DeclarativeHttpBodyV1 =
  | {
      type: 'none';
    }
  | {
      template: HttpJsonTemplateV1;
      type: 'application_json';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HttpJsonTemplateV1".
 */
export type HttpJsonTemplateV1 =
  | {
      type: 'literal';
      value: any;
    }
  | {
      source: ArgumentJsonPointer;
      type: 'argument';
    }
  | {
      items: HttpJsonTemplateV1[];
      type: 'array';
    }
  | {
      properties: HttpJsonPropertyV1[];
      type: 'object';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HttpJsonPropertyNameV1".
 */
export type HttpJsonPropertyNameV1 = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HttpCredentialSlotV1".
 */
export type HttpCredentialSlotV1 =
  | {
      slot_key: CredentialSlotKey;
      type: 'bearer_authorization';
    }
  | {
      header_name: HttpHeaderNameV1;
      slot_key: CredentialSlotKey;
      type: 'fixed_header';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HttpHeaderValueSourceV1".
 */
export type HttpHeaderValueSourceV1 =
  | {
      type: 'literal';
      value: HttpHeaderLiteralValueV1;
    }
  | {
      source: ArgumentJsonPointer;
      type: 'scalar';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HttpHeaderLiteralValueV1".
 */
export type HttpHeaderLiteralValueV1 = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "DeclarativeHttpMethodV1".
 */
export type DeclarativeHttpMethodV1 = 'GET' | 'HEAD' | 'POST' | 'PUT' | 'PATCH' | 'DELETE';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HttpPathSegmentV1".
 */
export type HttpPathSegmentV1 =
  | {
      type: 'literal';
      value: HttpPathLiteralSegmentV1;
    }
  | {
      source: ArgumentJsonPointer;
      type: 'argument';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HttpPathLiteralSegmentV1".
 */
export type HttpPathLiteralSegmentV1 = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HttpQueryNameV1".
 */
export type HttpQueryNameV1 = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HttpValueSourceV1".
 */
export type HttpValueSourceV1 =
  | {
      type: 'literal';
      value: HttpQueryLiteralValueV1;
    }
  | {
      source: ArgumentJsonPointer;
      type: 'scalar';
    }
  | {
      source: ArgumentJsonPointer;
      type: 'repeated';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HttpQueryLiteralValueV1".
 */
export type HttpQueryLiteralValueV1 = string;
/**
 * @minItems 1
 * @maxItems 16
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageHttpSuccessStatusRangesV1".
 */
export type ToolPackageHttpSuccessStatusRangesV1 =
  | [
      {
        end: number;
        start: number;
      },
    ]
  | [
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
    ]
  | [
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
    ]
  | [
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
    ]
  | [
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
    ]
  | [
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
    ]
  | [
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
    ]
  | [
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
    ]
  | [
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
    ]
  | [
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
    ]
  | [
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
    ]
  | [
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
    ]
  | [
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
    ]
  | [
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
    ]
  | [
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
    ]
  | [
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
      {
        end: number;
        start: number;
      },
    ];
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "DeclarativeHttpTargetV1".
 */
export type DeclarativeHttpTargetV1 =
  | {
      /**
       * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
       * via the `definition` "HttpBasePathV1".
       */
      base_path: HttpBasePathV1;
      origin: FixedHttpsOriginV1;
      type: 'fixed_https';
    }
  | {
      endpoint_slot_key: HttpEndpointSlotKeyV1;
      type: 'user_bound_endpoint';
    };
export type HttpBasePathV1 = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FixedHttpsOriginV1".
 */
export type FixedHttpsOriginV1 = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HttpEndpointSlotKeyV1".
 */
export type HttpEndpointSlotKeyV1 = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CanonicalToolSemanticVersionV1".
 */
export type CanonicalToolSemanticVersionV1 = CanonicalSemVer | null;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolKey".
 */
export type ToolKey = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageToolDisplayNameV1".
 */
export type ToolPackageToolDisplayNameV1 = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageUiDescriptionV1".
 */
export type ToolPackageUiDescriptionV1 = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageUiDisplayNameV1".
 */
export type ToolPackageUiDisplayNameV1 = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDraftFileChangeKindV1".
 */
export type ToolDraftFileChangeKindV1 = 'added' | 'modified' | 'removed' | 'unchanged';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NormalizedToolCallInputIssueV1".
 */
export type NormalizedToolCallInputIssueV1 =
  | (
      | 'envelope_root_invalid'
      | 'envelope_shape_invalid'
      | 'aura_metadata_missing'
      | 'purpose_invalid'
      | 'arguments_missing'
    )
  | 'arguments_json_invalid'
  | 'arguments_json_duplicate_key'
  | 'envelope_unexpected_field'
  | 'aura_metadata_shape_invalid'
  | 'aura_metadata_unexpected_field'
  | 'aura_metadata_invalid'
  | 'purpose_missing'
  | 'purpose_too_long'
  | 'arguments_root_invalid';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolExecutionAttemptStatusV1".
 */
export type ToolExecutionAttemptStatusV1 =
  | 'started'
  | 'dispatch_prepared'
  | 'running'
  | 'waiting_external'
  | 'cancelling'
  | 'succeeded'
  | 'failed'
  | 'cancelled'
  | 'indeterminate';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolLongOperationRefV1".
 */
export type ToolLongOperationRefV1 =
  | {
      journal_id: MutationJournalId;
      operation_kind: 'draft_apply';
      tool_draft_id: ToolDraftId;
    }
  | {
      journal_id: MutationJournalId;
      operation_kind: 'package_delete';
      tool_package_id: ToolPackageId;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolLongOperationStateV1".
 */
export type ToolLongOperationStateV1 =
  'queued' | 'running' | 'cancelling' | 'cancelled' | 'succeeded' | 'failed' | 'recovery_required';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProjectionCompatibilityKey".
 */
export type ProjectionCompatibilityKey = ProjectionKey | AuxiliaryProjectionKey;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProjectionKey".
 */
export type ProjectionKey =
  | {
      projection_kind: 'core_health';
      projection_scope: {
        type: 'global';
      };
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      projection_kind: 'workspace_session';
      projection_scope: {
        type: 'global';
      };
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      projection_kind: 'workspace_files';
      projection_scope: {
        type: 'global';
      };
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      projection_kind: 'model_settings';
      projection_scope: {
        type: 'global';
      };
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      projection_kind: 'provider_catalog';
      projection_scope: {
        type: 'global';
      };
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      projection_kind: 'endpoint_detail';
      projection_scope: {
        endpoint_id: EndpointId;
        type: 'endpoint';
      };
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      projection_kind: 'model_profile';
      projection_scope: {
        model_profile_id: ModelProfileId;
        type: 'model_profile';
      };
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      projection_kind: 'session_model_selector';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      projection_kind: 'model_availability_test';
      projection_scope: {
        model_availability_test_id: ModelAvailabilityTestId;
        type: 'model_availability_test';
      };
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      projection_kind: 'model_discovery';
      projection_scope: {
        model_discovery_run_id: ModelDiscoveryRunId;
        type: 'model_discovery';
      };
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      projection_kind: 'context_window_correction';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      projection_kind: 'prompt_inspector';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      projection_kind: 'tool_settings';
      projection_scope: {
        type: 'global';
      };
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      projection_kind: 'tool_package_list';
      projection_scope: {
        type: 'global';
      };
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      projection_kind: 'tool_draft_list';
      projection_scope: {
        type: 'global';
      };
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      projection_kind: 'workspace_tool_pins';
      projection_scope: {
        type: 'workspace';
        workspace_id: WorkspaceId;
      };
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      projection_kind: 'workspace_tool_registry_summary';
      projection_scope: {
        type: 'workspace';
        workspace_id: WorkspaceId;
      };
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      projection_kind: 'session_effective_tool_registry';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      projection_kind: 'session_loaded_tools';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      projection_kind: 'session_tool_activity';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      projection_kind: 'session_tool_pins';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      projection_kind: 'tool_package';
      projection_scope: {
        tool_package_id: ToolPackageId;
        type: 'tool_package';
      };
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      projection_kind: 'tool_definition';
      projection_scope: {
        tool_id: ToolId;
        type: 'tool_definition';
      };
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      projection_kind: 'tool_draft';
      projection_scope: {
        tool_draft_id: ToolDraftId;
        type: 'tool_draft';
      };
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      projection_kind: 'tool_conformance_run';
      projection_scope: {
        tool_conformance_run_id: ToolConformanceRunId;
        type: 'tool_conformance_run';
      };
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      projection_kind: 'tool_batch';
      projection_scope: {
        tool_batch_id: ToolBatchId;
        type: 'tool_batch';
      };
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      projection_kind: 'tool_call';
      projection_scope: {
        tool_call_id: ToolCallId;
        type: 'tool_call';
      };
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      projection_kind: 'tool_execution_attempt';
      projection_scope: {
        tool_execution_attempt_id: ToolExecutionAttemptId;
        type: 'tool_execution_attempt';
      };
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      projection_kind: 'tool_long_operation';
      projection_scope: {
        journal_id: MutationJournalId;
        type: 'tool_long_operation';
      };
      runtime_instance_id: RuntimeInstanceId;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AuxiliaryProjectionKey".
 */
export type AuxiliaryProjectionKey =
  | {
      projection_kind: 'session_permission';
      projection_scope: CompatibilitySessionScope;
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      projection_kind: 'pending_approval';
      projection_scope: CompatibilityGlobalScope;
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      projection_kind: 'security_runtime';
      projection_scope: CompatibilityGlobalScope;
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      projection_kind: 'terminal_state';
      projection_scope: CompatibilityTerminalScope;
      runtime_instance_id: RuntimeInstanceId;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CompatibilitySessionScope".
 */
export type CompatibilitySessionScope = {
  session_id: SessionId;
  type: 'session';
};
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CompatibilityGlobalScope".
 */
export type CompatibilityGlobalScope = {
  type: 'global';
};
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CompatibilityTerminalScope".
 */
export type CompatibilityTerminalScope = {
  session_id: SessionId;
  terminal_id: string;
  type: 'terminal';
};
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProjectionSnapshotRequest".
 */
export type ProjectionSnapshotRequest =
  | {
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'core_health';
      projection_scope: {
        type: 'global';
      };
    }
  | {
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'workspace_session';
      projection_scope: {
        type: 'global';
      };
    }
  | {
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'workspace_files';
      projection_scope: {
        type: 'global';
      };
    }
  | {
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'model_settings';
      projection_scope: {
        type: 'global';
      };
    }
  | {
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'provider_catalog';
      projection_scope: {
        type: 'global';
      };
    }
  | {
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'endpoint_detail';
      projection_scope: {
        endpoint_id: EndpointId;
        type: 'endpoint';
      };
    }
  | {
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'model_profile';
      projection_scope: {
        model_profile_id: ModelProfileId;
        type: 'model_profile';
      };
    }
  | {
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'session_model_selector';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
    }
  | {
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'model_availability_test';
      projection_scope: {
        model_availability_test_id: ModelAvailabilityTestId;
        type: 'model_availability_test';
      };
    }
  | {
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'model_discovery';
      projection_scope: {
        model_discovery_run_id: ModelDiscoveryRunId;
        type: 'model_discovery';
      };
    }
  | {
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'context_window_correction';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
    }
  | {
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'prompt_inspector';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
    }
  | {
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'tool_settings';
      projection_scope: {
        type: 'global';
      };
    }
  | {
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'tool_package_list';
      projection_scope: {
        type: 'global';
      };
    }
  | {
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'tool_draft_list';
      projection_scope: {
        type: 'global';
      };
    }
  | {
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'workspace_tool_pins';
      projection_scope: {
        type: 'workspace';
        workspace_id: WorkspaceId;
      };
    }
  | {
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'workspace_tool_registry_summary';
      projection_scope: {
        type: 'workspace';
        workspace_id: WorkspaceId;
      };
    }
  | {
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'session_effective_tool_registry';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
    }
  | {
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'session_loaded_tools';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
    }
  | {
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'session_tool_activity';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
    }
  | {
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'session_tool_pins';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
    }
  | {
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'tool_package';
      projection_scope: {
        tool_package_id: ToolPackageId;
        type: 'tool_package';
      };
    }
  | {
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'tool_definition';
      projection_scope: {
        tool_id: ToolId;
        type: 'tool_definition';
      };
    }
  | {
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'tool_draft';
      projection_scope: {
        tool_draft_id: ToolDraftId;
        type: 'tool_draft';
      };
    }
  | {
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'tool_conformance_run';
      projection_scope: {
        tool_conformance_run_id: ToolConformanceRunId;
        type: 'tool_conformance_run';
      };
    }
  | {
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'tool_batch';
      projection_scope: {
        tool_batch_id: ToolBatchId;
        type: 'tool_batch';
      };
    }
  | {
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'tool_call';
      projection_scope: {
        tool_call_id: ToolCallId;
        type: 'tool_call';
      };
    }
  | {
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'tool_execution_attempt';
      projection_scope: {
        tool_execution_attempt_id: ToolExecutionAttemptId;
        type: 'tool_execution_attempt';
      };
    }
  | {
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'tool_long_operation';
      projection_scope: {
        journal_id: MutationJournalId;
        type: 'tool_long_operation';
      };
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProjectionSubscription".
 */
export type ProjectionSubscription =
  | {
      after_revision?: Revision | null;
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'core_health';
      projection_scope: {
        type: 'global';
      };
    }
  | {
      after_revision?: Revision | null;
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'workspace_session';
      projection_scope: {
        type: 'global';
      };
    }
  | {
      after_revision?: Revision | null;
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'workspace_files';
      projection_scope: {
        type: 'global';
      };
    }
  | {
      after_revision?: Revision | null;
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'model_settings';
      projection_scope: {
        type: 'global';
      };
    }
  | {
      after_revision?: Revision | null;
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'provider_catalog';
      projection_scope: {
        type: 'global';
      };
    }
  | {
      after_revision?: Revision | null;
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'endpoint_detail';
      projection_scope: {
        endpoint_id: EndpointId;
        type: 'endpoint';
      };
    }
  | {
      after_revision?: Revision | null;
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'model_profile';
      projection_scope: {
        model_profile_id: ModelProfileId;
        type: 'model_profile';
      };
    }
  | {
      after_revision?: Revision | null;
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'session_model_selector';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
    }
  | {
      after_revision?: Revision | null;
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'model_availability_test';
      projection_scope: {
        model_availability_test_id: ModelAvailabilityTestId;
        type: 'model_availability_test';
      };
    }
  | {
      after_revision?: Revision | null;
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'model_discovery';
      projection_scope: {
        model_discovery_run_id: ModelDiscoveryRunId;
        type: 'model_discovery';
      };
    }
  | {
      after_revision?: Revision | null;
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'context_window_correction';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
    }
  | {
      after_revision?: Revision | null;
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'prompt_inspector';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
    }
  | {
      after_revision?: Revision | null;
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'tool_settings';
      projection_scope: {
        type: 'global';
      };
    }
  | {
      after_revision?: Revision | null;
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'tool_package_list';
      projection_scope: {
        type: 'global';
      };
    }
  | {
      after_revision?: Revision | null;
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'tool_draft_list';
      projection_scope: {
        type: 'global';
      };
    }
  | {
      after_revision?: Revision | null;
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'workspace_tool_pins';
      projection_scope: {
        type: 'workspace';
        workspace_id: WorkspaceId;
      };
    }
  | {
      after_revision?: Revision | null;
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'workspace_tool_registry_summary';
      projection_scope: {
        type: 'workspace';
        workspace_id: WorkspaceId;
      };
    }
  | {
      after_revision?: Revision | null;
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'session_effective_tool_registry';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
    }
  | {
      after_revision?: Revision | null;
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'session_loaded_tools';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
    }
  | {
      after_revision?: Revision | null;
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'session_tool_activity';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
    }
  | {
      after_revision?: Revision | null;
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'session_tool_pins';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
    }
  | {
      after_revision?: Revision | null;
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'tool_package';
      projection_scope: {
        tool_package_id: ToolPackageId;
        type: 'tool_package';
      };
    }
  | {
      after_revision?: Revision | null;
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'tool_definition';
      projection_scope: {
        tool_id: ToolId;
        type: 'tool_definition';
      };
    }
  | {
      after_revision?: Revision | null;
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'tool_draft';
      projection_scope: {
        tool_draft_id: ToolDraftId;
        type: 'tool_draft';
      };
    }
  | {
      after_revision?: Revision | null;
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'tool_conformance_run';
      projection_scope: {
        tool_conformance_run_id: ToolConformanceRunId;
        type: 'tool_conformance_run';
      };
    }
  | {
      after_revision?: Revision | null;
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'tool_batch';
      projection_scope: {
        tool_batch_id: ToolBatchId;
        type: 'tool_batch';
      };
    }
  | {
      after_revision?: Revision | null;
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'tool_call';
      projection_scope: {
        tool_call_id: ToolCallId;
        type: 'tool_call';
      };
    }
  | {
      after_revision?: Revision | null;
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'tool_execution_attempt';
      projection_scope: {
        tool_execution_attempt_id: ToolExecutionAttemptId;
        type: 'tool_execution_attempt';
      };
    }
  | {
      after_revision?: Revision | null;
      known_runtime_instance_id?: RuntimeInstanceId | null;
      projection_kind: 'tool_long_operation';
      projection_scope: {
        journal_id: MutationJournalId;
        type: 'tool_long_operation';
      };
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProjectionUnsubscribeRequest".
 */
export type ProjectionUnsubscribeRequest =
  | {
      projection_kind: 'core_health';
      projection_scope: {
        type: 'global';
      };
    }
  | {
      projection_kind: 'workspace_session';
      projection_scope: {
        type: 'global';
      };
    }
  | {
      projection_kind: 'workspace_files';
      projection_scope: {
        type: 'global';
      };
    }
  | {
      projection_kind: 'model_settings';
      projection_scope: {
        type: 'global';
      };
    }
  | {
      projection_kind: 'provider_catalog';
      projection_scope: {
        type: 'global';
      };
    }
  | {
      projection_kind: 'endpoint_detail';
      projection_scope: {
        endpoint_id: EndpointId;
        type: 'endpoint';
      };
    }
  | {
      projection_kind: 'model_profile';
      projection_scope: {
        model_profile_id: ModelProfileId;
        type: 'model_profile';
      };
    }
  | {
      projection_kind: 'session_model_selector';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
    }
  | {
      projection_kind: 'model_availability_test';
      projection_scope: {
        model_availability_test_id: ModelAvailabilityTestId;
        type: 'model_availability_test';
      };
    }
  | {
      projection_kind: 'model_discovery';
      projection_scope: {
        model_discovery_run_id: ModelDiscoveryRunId;
        type: 'model_discovery';
      };
    }
  | {
      projection_kind: 'context_window_correction';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
    }
  | {
      projection_kind: 'prompt_inspector';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
    }
  | {
      projection_kind: 'tool_settings';
      projection_scope: {
        type: 'global';
      };
    }
  | {
      projection_kind: 'tool_package_list';
      projection_scope: {
        type: 'global';
      };
    }
  | {
      projection_kind: 'tool_draft_list';
      projection_scope: {
        type: 'global';
      };
    }
  | {
      projection_kind: 'workspace_tool_pins';
      projection_scope: {
        type: 'workspace';
        workspace_id: WorkspaceId;
      };
    }
  | {
      projection_kind: 'workspace_tool_registry_summary';
      projection_scope: {
        type: 'workspace';
        workspace_id: WorkspaceId;
      };
    }
  | {
      projection_kind: 'session_effective_tool_registry';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
    }
  | {
      projection_kind: 'session_loaded_tools';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
    }
  | {
      projection_kind: 'session_tool_activity';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
    }
  | {
      projection_kind: 'session_tool_pins';
      projection_scope: {
        session_id: SessionId;
        type: 'session';
      };
    }
  | {
      projection_kind: 'tool_package';
      projection_scope: {
        tool_package_id: ToolPackageId;
        type: 'tool_package';
      };
    }
  | {
      projection_kind: 'tool_definition';
      projection_scope: {
        tool_id: ToolId;
        type: 'tool_definition';
      };
    }
  | {
      projection_kind: 'tool_draft';
      projection_scope: {
        tool_draft_id: ToolDraftId;
        type: 'tool_draft';
      };
    }
  | {
      projection_kind: 'tool_conformance_run';
      projection_scope: {
        tool_conformance_run_id: ToolConformanceRunId;
        type: 'tool_conformance_run';
      };
    }
  | {
      projection_kind: 'tool_batch';
      projection_scope: {
        tool_batch_id: ToolBatchId;
        type: 'tool_batch';
      };
    }
  | {
      projection_kind: 'tool_call';
      projection_scope: {
        tool_call_id: ToolCallId;
        type: 'tool_call';
      };
    }
  | {
      projection_kind: 'tool_execution_attempt';
      projection_scope: {
        tool_execution_attempt_id: ToolExecutionAttemptId;
        type: 'tool_execution_attempt';
      };
    }
  | {
      projection_kind: 'tool_long_operation';
      projection_scope: {
        journal_id: MutationJournalId;
        type: 'tool_long_operation';
      };
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CommandOutcome".
 */
export type CommandOutcome =
  | {
      command_invocation_id: CommandInvocationId;
      outcome: 'confirmed';
      response: any;
      terminal: IpcRequestTerminal;
    }
  | {
      command_invocation_id: CommandInvocationId;
      outcome: 'unknown';
      reason: CommandOutcomeUnknown;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "IpcRequestTerminal".
 */
export type IpcRequestTerminal =
  'succeeded' | 'failed' | 'cancelled' | 'transport_lost' | 'protocol_violation' | 'core_restarted';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CommandOutcomeUnknown".
 */
export type CommandOutcomeUnknown = 'not_recorded_or_incomplete';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RuntimeHandshakeKind".
 */
export type RuntimeHandshakeKind = 'initial' | 'reconnect';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SecretNonce".
 */
export type SecretNonce = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RuntimeProcessKind".
 */
export type RuntimeProcessKind =
  | 'launcher'
  | 'electron_main'
  | 'renderer'
  | 'core'
  | 'sandbox_host'
  | 'egress_proxy'
  | 'elevated_helper';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CoreStartupPhase".
 */
export type CoreStartupPhase =
  | 'transport_ready'
  | 'recovering_runtime'
  | 'rebuilding_projection'
  | 'ready'
  | 'recovery_required';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ApprovalRequestId".
 */
export type ApprovalRequestId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ApprovalDecisionKind".
 */
export type ApprovalDecisionKind =
  'allow_once' | 'allow_session' | 'allow_workspace' | 'allow_for_current_turn' | 'deny';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ResolvedCapability".
 */
export type ResolvedCapability =
  | (
      | 'workspace_file_read'
      | 'workspace_file_write'
      | 'workspace_file_create'
      | 'workspace_file_delete'
      | 'workspace_file_move'
      | 'external_file_read'
      | 'external_file_write'
      | 'external_file_create'
      | 'external_file_delete'
      | 'process_execute'
      | 'interactive_terminal'
      | 'background_process'
      | 'git_read'
      | 'git_write'
      | 'git_remote_read'
      | 'git_remote_write'
      | 'git_destructive'
      | 'public_network_read'
      | 'private_network_access'
      | 'data_egress'
      | 'download_artifact'
      | 'download_windows_executable'
      | 'external_service_read'
      | 'external_service_mutation'
      | 'external_service_destructive_mutation'
      | 'browser_read'
      | 'browser_interaction'
      | 'browser_sensitive_action'
      | 'configuration_read'
      | 'configuration_write'
      | 'registry_write'
      | 'system_environment_write'
      | 'system_file_write'
      | 'service_management'
      | 'scheduled_task_management'
      | 'driver_management'
      | 'windows_software_install'
      | 'elevated_operation'
      | 'tool_registry_read'
      | 'tool_ephemeral_exposure_write'
      | 'agent_task_write'
      | 'memory_read'
      | 'memory_write'
      | 'memory_delete'
    )
  | 'ordinary_network'
  | 'agent_task_read'
  | 'external_extension_call';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "InvocationActor".
 */
export type InvocationActor = 'user' | 'parent_agent' | 'child_agent' | 'system_runtime';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PermissionSnapshotId".
 */
export type PermissionSnapshotId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PrincipalId".
 */
export type PrincipalId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SecurityOperationId".
 */
export type SecurityOperationId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "OperationRiskFlag".
 */
export type OperationRiskFlag =
  | (
      | 'unknown_side_effect'
      | 'outside_workspace_write'
      | 'outside_workspace_delete'
      | 'sensitive_path_access'
      | 'data_leaves_device'
      | 'private_or_loopback_network'
      | 'executable_artifact'
      | 'software_installation'
      | 'system_configuration_mutation'
      | 'elevation'
      | 'driver_or_service_mutation'
      | 'destructive_remote_mutation'
      | 'sensitive_browser_action'
      | 'git_bare_force_push'
      | 'git_sequencer_skip_or_quit'
      | 'inline_script'
      | 'untrusted_source'
    )
  | 'permanent_file_deletion';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ResolvedScope".
 */
export type ResolvedScope =
  | {
      scope_kind: 'workspace_root';
      workspace_id: WorkspaceId;
    }
  | {
      canonical_relative_path: string;
      scope_kind: 'workspace_path';
      workspace_id: WorkspaceId;
    }
  | {
      file_reference_id: FileReferenceId;
      scope_kind: 'session_authorized_file';
    }
  | {
      canonical_path: CanonicalPath;
      identity?: FileIdentity | null;
      recursive: boolean;
      scope_kind: 'external_path';
    }
  | {
      grant_id: ExternalGrantId;
      scope_kind: 'external_grant';
    }
  | {
      host: string;
      port: number;
      scheme: string;
      scope_kind: 'network_origin';
    }
  | {
      class: PrivateNetworkClass;
      scope_kind: 'private_network';
    }
  | {
      object_ref?: string | null;
      scope_kind: 'external_service';
      service_id: string;
    }
  | {
      canonical_path: CanonicalPath;
      scope_kind: 'system_path';
    }
  | {
      canonical_path: CanonicalPath;
      scope_kind: 'sensitive_user_path';
    }
  | {
      hive: string;
      path: string;
      scope_kind: 'registry_key';
    }
  | {
      scope: EnvironmentVariableScope;
      scope_kind: 'environment_scope';
    }
  | {
      scope_kind: 'git_repository';
      workspace_id: WorkspaceId;
    }
  | {
      remote: string;
      scope_kind: 'git_remote';
    }
  | {
      page_ref: string;
      scope_kind: 'browser_page';
    }
  | {
      package: PackageRuntimeScopeV1;
      scope_kind: 'package_runtime';
    }
  | {
      conformance: ToolConformanceRuntimeScopeV1;
      scope_kind: 'tool_conformance_runtime';
    }
  | {
      registry_snapshot_ref: ToolRegistrySnapshotRef;
      scope_kind: 'tool_registry_snapshot';
    }
  | {
      agent_run_id: AgentRunId;
      scope_kind: 'turn_task_tool_exposure';
      session_id: SessionId;
      turn_task_id: TurnTaskId;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PrivateNetworkClass".
 */
export type PrivateNetworkClass = 'loopback' | 'private' | 'link_local' | 'metadata' | 'reserved';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "EnvironmentVariableScope".
 */
export type EnvironmentVariableScope = 'process' | 'user' | 'machine';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SecuritySourceV2".
 */
export type SecuritySourceV2 =
  | {
      source_kind: 'user_interface';
    }
  | {
      source_kind: 'tool';
      tool: ToolSecuritySourceRefV1;
    }
  | {
      conformance: ToolConformanceSecuritySourceRefV1;
      source_kind: 'tool_conformance';
    }
  | {
      hook_id: string;
      source_kind: 'hook';
    }
  | {
      server_id: string;
      source_kind: 'mcp_server';
    }
  | {
      skill_id: string;
      source_kind: 'skill';
    }
  | {
      plugin_id: string;
      source_kind: 'plugin';
    }
  | {
      component_id: string;
      source_kind: 'system_component';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ApprovalState".
 */
export type ApprovalState =
  'pending' | 'approved' | 'denied' | 'expired' | 'cancelled' | 'invalidated';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ApprovalDecisionId".
 */
export type ApprovalDecisionId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PermissionGrantId".
 */
export type PermissionGrantId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NetworkDestinationClass".
 */
export type NetworkDestinationClass =
  'public' | 'loopback' | 'private' | 'link_local' | 'metadata' | 'reserved';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NetworkGrantId".
 */
export type NetworkGrantId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NetworkCallerKind".
 */
export type NetworkCallerKind =
  | 'model_transport'
  | 'models_catalog'
  | 'aura_tool'
  | 'web_runtime'
  | 'aura_browser'
  | 'sandbox_process'
  | 'mcp_http'
  | 'external_extension_protocol'
  | 'update_service';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RedirectPolicy".
 */
export type RedirectPolicy =
  | 'deny'
  | {
      same_origin: {
        max_hops: number;
      };
    }
  | {
      reauthorize_each_hop: {
        max_hops: number;
      };
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NetworkDataReference".
 */
export type NetworkDataReference =
  | {
      label: string;
      reference_kind: 'inline_text';
      value: string;
    }
  | {
      name: string;
      reference_kind: 'header';
      value: string;
    }
  | {
      reference_kind: 'json_body';
      value: any;
    }
  | {
      byte_length: number;
      reference_id: string;
      reference_kind: 'file_reference';
    }
  | {
      artifact_id: string;
      byte_length: number;
      reference_kind: 'artifact';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NetworkRequestId".
 */
export type NetworkRequestId = string;
/**
 * Stable evidence describing why P04 permitted one operation. This is kept
 * separate from `PermissionDecision` so the existing Renderer-facing
 * decision wire remains unchanged while Core can bind the exact authority to
 * a one-shot Runtime dispatch.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PermissionAllowAuthorizationBasisV1".
 */
export type PermissionAllowAuthorizationBasisV1 =
  | {
      basis_kind: 'permission_mode';
      mode: PermissionMode;
    }
  | {
      basis_kind: 'permission_grant';
      lifetime: PermissionGrantLifetime;
      permission_grant_id: PermissionGrantId;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PermissionMode".
 */
export type PermissionMode = 'read_only' | 'workspace_read_write' | 'full_access';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PermissionGrantLifetime".
 */
export type PermissionGrantLifetime = 'once' | 'turn_task' | 'session' | 'workspace';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NetworkAccessRequirement".
 */
export type NetworkAccessRequirement = 'not_required' | 'required';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NetworkEnforcementKind".
 */
export type NetworkEnforcementKind =
  'core_control_plane' | 'app_container_network_denied' | 'observed_native_not_proxy_enforced';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SandboxExecutionId".
 */
export type SandboxExecutionId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NativeSandboxMode".
 */
export type NativeSandboxMode =
  | 'compatibility_app_container'
  | 'strict_app_container'
  | 'observed_native'
  | 'observed_native_networked'
  | 'elevated_brokered';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "W7ToolOperationFingerprintDomainV1".
 */
export type W7ToolOperationFingerprintDomainV1 = 'aura.p07.w7_tool_operation';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPurposeTextV1".
 */
export type ToolPurposeTextV1 = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NonSecretToolRuntimeFactsV1".
 */
export type NonSecretToolRuntimeFactsV1 =
  | {
      execution_path: 'sandbox';
      facts: NonSecretSandboxRuntimeFactsV1;
    }
  | {
      execution_path: 'network_gateway';
      facts: NonSecretHttpRuntimeFactsV1;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCredentialPlacementV12".
 */
export type ToolCredentialPlacementV12 =
  | {
      environment_variable: EnvironmentVariableNameV1;
      placement_kind: 'environment';
    }
  | {
      placement_kind: 'bearer_authorization';
    }
  | {
      header_name: HttpHeaderNameV1;
      placement_kind: 'fixed_header';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SandboxFilesystemAccess".
 */
export type SandboxFilesystemAccess = 'read' | 'read_write' | 'create';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPreparedRuntimeKindV1".
 */
export type ToolPreparedRuntimeKindV1 =
  | 'declarative_command_argv'
  | 'declarative_command_powershell_script'
  | 'aura_stdio'
  | 'declarative_http';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CancellationId".
 */
export type CancellationId = string;
/**
 * Frozen requirement profile for an uninstalled Draft Conformance process.
 * The selected P04 implementation must satisfy this profile exactly; there
 * is no ObservedNative or networked fallback.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ConformanceSandboxProfileV1".
 */
export type ConformanceSandboxProfileV1 = 'hermetic_no_network_v1';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CredentialReflectionEncodingV1".
 */
export type CredentialReflectionEncodingV1 =
  | 'raw_utf8'
  | 'json_string_escaped'
  | 'percent_encoded_uppercase'
  | 'percent_encoded_lowercase'
  | 'base64_standard_padded'
  | 'base64_standard_unpadded'
  | 'base64_url_safe_padded'
  | 'base64_url_safe_unpadded';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CredentialReflectionSurfaceV1".
 */
export type CredentialReflectionSurfaceV1 =
  | 'redirect_header_value'
  | 'final_header_value'
  | 'error_header_value'
  | 'final_response_body'
  | 'error_response_body';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CredentialReflectionScanStatusV1".
 */
export type CredentialReflectionScanStatusV1 =
  'not_applicable' | 'passed' | 'detected' | 'incomplete';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExternalIsolationMode".
 */
export type ExternalIsolationMode =
  | 'compatibility_managed_user'
  | 'strict_app_container'
  | 'native_managed_user'
  | 'managed_user_networked';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExternalExtensionKind".
 */
export type ExternalExtensionKind = 'custom_tool' | 'hook' | 'mcp_server' | 'skill' | 'plugin';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExternalGrantCapability".
 */
export type ExternalGrantCapability = 'read' | 'write' | 'create' | 'delete' | 'model_egress';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExternalGrantLifecycle".
 */
export type ExternalGrantLifecycle = 'active' | 'suspended' | 'revoked';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExternalGrantTargetKind".
 */
export type ExternalGrantTargetKind = 'file' | 'directory';
/**
 * How long a directory grant remains reusable after the user approves it.
 *
 * This is the grant's own term, not the approval request's confirmation window.
 * Older records omit the field and stay session-scoped, which is the term they had.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExternalGrantTerm".
 */
export type ExternalGrantTerm = 'session' | 'workspace' | 'turn';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FileSessionConsentAvailability".
 */
export type FileSessionConsentAvailability =
  'in_force' | 'revoked' | 'expired' | 'target_changed' | 'unavailable' | 'not_yet_valid';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HostPreparationAction".
 */
export type HostPreparationAction = 'prepare' | 'repair' | 'remove';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HostPreparationOperationId".
 */
export type HostPreparationOperationId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HostPreparationOperationState".
 */
export type HostPreparationOperationState =
  | 'prepared'
  | 'awaiting_elevation'
  | 'applying'
  | 'completed'
  | 'rollback_pending'
  | 'rolled_back'
  | 'denied'
  | 'failed';
/**
 * The built-in Shell tools a Native session consent may cover.
 *
 * The variant is the conservative identity: a consent is approved for one
 * concrete shell that is part of the signed-in product, never for "some
 * process on this machine". It is the *tool that was approved*, which is an audit
 * fact; the *range the approval covers* is [`NativeSessionConsentScope::shells`].
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NativeShell".
 */
export type NativeShell = 'cmd' | 'powershell';
/**
 * Whether a consent still authorizes new work right now.
 *
 * "Revoked" and "expired" are different things to the person reading the list:
 * one was their own action, the other is a window that closed. A `consumed`
 * one-time authorization is neither, and is never a consent.
 *
 * `Superseded` identifies a legacy answer from a different Core runtime. New
 * session answers survive Core restart and permission mode changes.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NativeSessionConsentAvailability".
 */
export type NativeSessionConsentAvailability =
  ('in_force' | 'revoked' | 'consumed' | 'expired' | 'not_yet_valid') | 'superseded';
/**
 * What the enforcing component could actually inspect for this network
 * activity. This is deliberately independent from the sandbox enforcement
 * mode so audit consumers cannot infer structured HTTP visibility from a
 * blind CONNECT tunnel or an observed native process.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NetworkInspectionLevel".
 */
export type NetworkInspectionLevel = 'structured_http' | 'opaque_tunnel' | 'observed_native_only';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NetworkRequestAuditState".
 */
export type NetworkRequestAuditState =
  'started' | 'authorized' | 'blocked' | 'finished' | 'interrupted';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NetworkGrantIdentityScope".
 */
export type NetworkGrantIdentityScope =
  'session' | 'turn_task' | 'sandbox_execution' | 'system_component';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PermissionDecision".
 */
export type PermissionDecision =
  | {
      decision: 'allow';
      snapshot: PermissionSnapshot;
    }
  | {
      decision: 'ask';
      request: ApprovalRequestDraft;
    }
  | {
      decision: 'deny';
      reason: PermissionDenyReason;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PermissionDenyReason".
 */
export type PermissionDenyReason =
  | 'mode_denied'
  | 'identity_invalid'
  | 'purpose_required'
  | 'source_disabled'
  | 'source_untrusted'
  | 'capability_unresolved'
  | 'scope_unresolved'
  | 'hard_policy'
  | 'denied_for_current_turn'
  | 'sandbox_unavailable'
  | 'network_plan_unavailable'
  | 'network_access_declaration_required';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SecurityProjectionChange".
 */
export type SecurityProjectionChange =
  | {
      payload: SessionPermissionProjection;
      projection_kind: 'session_permission';
      revision: Revision;
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      payload: PendingApprovalProjection;
      projection_kind: 'pending_approval';
      revision: Revision;
      runtime_instance_id: RuntimeInstanceId;
    }
  | {
      payload: SecurityRuntimeProjection;
      projection_kind: 'security_runtime';
      revision: Revision;
      runtime_instance_id: RuntimeInstanceId;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RuntimeComponentState".
 */
export type RuntimeComponentState =
  'not_started' | 'starting' | 'ready' | 'degraded' | 'unavailable' | 'crashed';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "QuarantineRecordId".
 */
export type QuarantineRecordId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "QuarantineReleaseTarget".
 */
export type QuarantineReleaseTarget =
  | {
      path: string;
      target_kind: 'workspace';
    }
  | {
      external_grant_id: ExternalGrantId;
      relative_path: string;
      target_kind: 'external_grant';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "QuarantineState".
 */
export type QuarantineState = 'quarantined' | 'released' | 'discarded';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SandboxHostEvent".
 */
export type SandboxHostEvent =
  | {
      event_kind: 'sandbox.started';
      execution_id: SandboxExecutionId;
    }
  | {
      event_kind: 'sandbox.process_started';
      execution_id: SandboxExecutionId;
      process_id: number;
    }
  | {
      bytes: number[];
      event_kind: 'sandbox.stdout_chunk';
      execution_id: SandboxExecutionId;
      sequence: number;
    }
  | {
      bytes: number[];
      event_kind: 'sandbox.stderr_chunk';
      execution_id: SandboxExecutionId;
      sequence: number;
    }
  | {
      event_kind: 'sandbox.exit';
      execution_id: SandboxExecutionId;
      exit_code: number;
    }
  | {
      event_kind: 'sandbox.cancelled';
      execution_id: SandboxExecutionId;
    }
  | {
      event_kind: 'sandbox.timeout';
      execution_id: SandboxExecutionId;
    }
  | {
      code: string;
      event_kind: 'sandbox.policy_violation';
      execution_id: SandboxExecutionId;
    }
  | {
      event_kind: 'sandbox.cleanup_completed';
      execution_id: SandboxExecutionId;
    }
  | {
      code: string;
      event_kind: 'sandbox.cleanup_failed';
      execution_id: SandboxExecutionId;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NetworkExecutionMode".
 */
export type NetworkExecutionMode =
  | {
      mode: NativeSandboxMode;
      mode_kind: 'native';
    }
  | {
      mode: ExternalIsolationMode;
      mode_kind: 'external';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SandboxNetworkPolicyErrorCode".
 */
export type SandboxNetworkPolicyErrorCode =
  | 'network_access_declaration_required'
  | 'app_container_network_denied'
  | 'network_grant_unavailable_in_selected_mode';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TrustRecordId".
 */
export type TrustRecordId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TrustState".
 */
export type TrustState =
  | {
      trust_state: 'system_trusted';
    }
  | {
      fingerprint: ContentHash;
      trust_state: 'trusted';
      trusted_at: UtcTimestamp;
    }
  | {
      trust_state: 'untrusted';
    }
  | {
      trust_state: 'revoked';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolActivityCursorKindV1".
 */
export type ToolActivityCursorKindV1 = 'aura.p07.tool_activity_cursor';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolBatchErrorCode".
 */
export type ToolBatchErrorCode =
  | 'tool.batch.tool_call_identity_conflict'
  | 'tool.batch.identity_mismatch'
  | 'tool.batch.content_hash_mismatch'
  | 'tool.batch.invocation_boundary_invalid'
  | 'tool.batch.registry_snapshot_mismatch'
  | 'tool.batch.control_state_mismatch'
  | 'tool.batch.dependency_cycle'
  | 'tool.batch.atomic_group_invalid'
  | 'tool.batch.provider_mapping_ambiguous'
  | 'tool.batch.completion_batch_invalid'
  | 'tool.batch.plan_invalidated'
  | 'tool.batch.rejected';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NormalizedToolCallInputStateV1".
 */
export type NormalizedToolCallInputStateV1 =
  | {
      arguments: any;
      arguments_hash: CanonicalSha256HashV1;
      purpose: ToolPurposeTextV1;
      type: 'ready';
    }
  | {
      /**
       * Absent for a value written before the diagnostic existed, and for an issue
       * whose location is the whole envelope.
       */
      detail?: NormalizedToolCallInputDiagnosticV1 | null;
      issue: NormalizedToolCallInputIssueV1;
      type: 'invalid_input';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolResultOutcomeV1".
 */
export type ToolResultOutcomeV1 =
  | {
      type: 'succeeded';
    }
  | {
      error: ErrorEnvelope;
      type: 'failed';
    }
  | {
      error: ErrorEnvelope;
      type: 'operation_rejected';
    }
  | {
      reason: ToolCancellationReasonV1;
      type: 'cancelled';
    }
  | {
      error?: ErrorEnvelope | null;
      reason: ToolNotExecutedReasonV1;
      type: 'not_executed';
    }
  | {
      error: ErrorEnvelope;
      reason: ToolIndeterminateReasonV1;
      type: 'indeterminate';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BuiltinToolCapabilityV1".
 */
export type BuiltinToolCapabilityV1 = 'tool_registry_read' | 'tool_ephemeral_exposure_write';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BuiltinToolScopeRequirementV1".
 */
export type BuiltinToolScopeRequirementV1 =
  | {
      type: 'tool_registry_snapshot';
    }
  | {
      type: 'turn_task_tool_exposure';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolConformanceErrorCode".
 */
export type ToolConformanceErrorCode =
  | 'tool.conformance.case_missing'
  | 'tool.conformance.case_invalid'
  | 'tool.conformance.isolation_unavailable'
  | 'tool.conformance.run_failed'
  | 'tool.conformance.evidence_invalid';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCredentialCandidateCompatibilityV1".
 */
export type ToolCredentialCandidateCompatibilityV1 = 'compatible';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCredentialKindV1".
 */
export type ToolCredentialKindV1 = 'api_key';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCredentialScopeSummaryV1".
 */
export type ToolCredentialScopeSummaryV1 =
  | {
      scope: 'user';
    }
  | {
      scope: 'workspace';
    }
  | {
      scope: 'session';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCredentialStateV1".
 */
export type ToolCredentialStateV1 = 'configured' | 'suspended' | 'revoked' | 'recovery_required';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolLoadStateV1".
 */
export type ToolLoadStateV1 = 'core' | 'pinned' | 'ephemeral' | 'discoverable';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDraftErrorCode".
 */
export type ToolDraftErrorCode =
  | 'tool.draft.not_found'
  | 'tool.draft.revision_conflict'
  | 'tool.draft.validation_failed'
  | 'tool.draft.test_required'
  | 'tool.draft.apply_failed'
  | 'tool.draft.journal_failed'
  | 'tool.draft.authentication_required'
  | 'tool.draft.identity_mismatch'
  | 'tool.draft.snapshot_conflict'
  | 'tool.draft.preview_stale'
  | 'tool.draft.idempotency_conflict'
  | 'tool.draft.invalid_transition'
  | 'tool.draft.approval_required'
  | 'tool.draft.approval_mismatch';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolErrorRetryClassV1".
 */
export type ToolErrorRetryClassV1 = 'never' | 'conditional' | 'retryable';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolErrorVisibilityV1".
 */
export type ToolErrorVisibilityV1 =
  'internal_only' | 'renderer_safe' | 'model_safe' | 'renderer_and_model_safe';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ResolvedResourceAccessV1".
 */
export type ResolvedResourceAccessV1 =
  'read' | 'write' | 'create' | 'delete' | 'execute' | 'observe' | 'control';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ResolvedResourceKindV1".
 */
export type ResolvedResourceKindV1 =
  | 'memory_vault'
  | 'workspace_path'
  | 'external_file'
  | 'network_target'
  | 'process'
  | 'terminal_session'
  | 'git_repository'
  | 'git_index'
  | 'git_ref'
  | 'git_sequencer'
  | 'artifact'
  | 'workspace_mutation_ownership'
  | 'external_source_instance'
  | 'agent_task_state';
/**
 * Closed catalog of P07 lifecycle events. Event Store metadata (EventId, actor,
 * causation and correlation) remains in the shared persistence envelope.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolLifecycleEventV1".
 */
export type ToolLifecycleEventV1 =
  | {
      event_type: 'ToolPackageInstalled';
      payload: ToolPackageInstalledPayloadV1;
    }
  | {
      event_type: 'ToolPackageUpdated';
      payload: ToolPackageUpdatedPayloadV1;
    }
  | {
      event_type: 'ToolPackageDeleted';
      payload: ToolPackageDeletedPayloadV1;
    }
  | {
      event_type: 'ToolTrustChanged';
      payload: ToolTrustChangedPayloadV1;
    }
  | {
      event_type: 'ToolEnabledChanged';
      payload: ToolEnabledChangedPayloadV1;
    }
  | {
      event_type: 'ToolInstanceConfigurationChanged';
      payload: ToolInstanceConfigurationChangedPayloadV1;
    }
  | {
      event_type: 'ToolInstanceCredentialBindingChanged';
      payload: ToolInstanceCredentialBindingChangedPayloadV1;
    }
  | {
      event_type: 'WorkspaceToolPinChanged';
      payload: WorkspaceToolPinChangedPayloadV1;
    }
  | {
      event_type: 'SessionToolPinChanged';
      payload: SessionToolPinChangedPayloadV1;
    }
  | {
      event_type: 'ToolRegistrySnapshotCreated';
      payload: ToolRegistrySnapshotCreatedPayloadV1;
    }
  | {
      event_type: 'ToolBatchAdmitted';
      payload: ToolBatchAdmittedPayloadV1;
    }
  | {
      event_type: 'ToolBatchAdmissionRejected';
      payload: ToolBatchAdmissionRejectedPayloadV1;
    }
  | {
      event_type: 'ToolExecutionAttemptStarted';
      payload: ToolExecutionAttemptStartedPayloadV1;
    }
  | {
      event_type: 'ToolExecutionAttemptDispatchPrepared';
      payload: ToolExecutionAttemptDispatchPreparedPayloadV1;
    }
  | {
      event_type: 'ToolExecutionAttemptCancellationRequested';
      payload: ToolExecutionAttemptCancellationRequestedPayloadV1;
    }
  | {
      event_type: 'ToolExecutionAttemptSucceeded';
      payload: ToolExecutionAttemptSucceededPayloadV1;
    }
  | {
      event_type: 'ToolExecutionAttemptFailed';
      payload: ToolExecutionAttemptFailedPayloadV1;
    }
  | {
      event_type: 'ToolExecutionAttemptCancelled';
      payload: ToolExecutionAttemptCancelledPayloadV1;
    }
  | {
      event_type: 'ToolExecutionAttemptIndeterminate';
      payload: ToolExecutionAttemptIndeterminatePayloadV1;
    }
  | {
      event_type: 'ToolResultFinalized';
      payload: ToolResultFinalizedPayloadV1;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolRegistryScopeV1".
 */
export type ToolRegistryScopeV1 =
  | {
      type: 'workspace';
      workspace_id: WorkspaceId;
    }
  | {
      agent_run_id: AgentRunId;
      session_id: SessionId;
      turn_task_id: TurnTaskId;
      type: 'session';
      workspace_id: WorkspaceId;
    }
  | {
      child_agent_run_id: AgentRunId;
      session_id: SessionId;
      turn_task_id: TurnTaskId;
      type: 'child_agent';
      workspace_id: WorkspaceId;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolOutputSlotKeyV1".
 */
export type ToolOutputSlotKeyV1 = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelVisibleToolResultOutcomeV1".
 */
export type ModelVisibleToolResultOutcomeV1 =
  | {
      payload: ModelVisibleToolSuccessPayloadV1;
      type: 'succeeded';
    }
  | {
      error: ModelSafeToolErrorV1;
      type: 'failed';
    }
  | {
      error: ModelSafeToolErrorV1;
      type: 'operation_rejected';
    }
  | {
      reason: ToolCancellationReasonV1;
      type: 'cancelled';
    }
  | {
      error?: ModelSafeToolErrorV1 | null;
      reason: ToolNotExecutedReasonV1;
      type: 'not_executed';
    }
  | {
      error: ModelSafeToolErrorV1;
      reason: ToolIndeterminateReasonV1;
      type: 'indeterminate';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelVisibleToolSuccessPayloadV1".
 */
export type ModelVisibleToolSuccessPayloadV1 =
  | {
      type: 'unit';
    }
  | {
      text: string;
      type: 'utf8_text';
    }
  | {
      type: 'structured_json';
      value: any;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDraftArchiveSourceV1".
 */
export type ToolDraftArchiveSourceV1 =
  | {
      archive: FileTargetRef;
      source_kind: 'file_target';
    }
  | {
      artifact_id: ArtifactId;
      source_kind: 'controlled_artifact';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FileTargetRef".
 */
export type FileTargetRef =
  | {
      path: string;
      target_kind: 'workspace';
    }
  | {
      file_reference_id: FileReferenceId;
      target_kind: 'session_file';
    }
  | {
      path: string;
      target_kind: 'external_path';
    }
  | {
      external_grant_id: ExternalGrantId;
      relative_path: string;
      target_kind: 'external_grant';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolMutationResultV1".
 */
export type ToolMutationResultV1 =
  | {
      draft_revision: Revision;
      entity_revision: Revision;
      result_kind: 'draft_committed';
      snapshot_hash: CanonicalSha256HashV1;
      tool_draft_id: ToolDraftId;
    }
  | {
      operational_revision: Revision;
      result_kind: 'conformance_accepted';
      tool_conformance_run_id: ToolConformanceRunId;
    }
  | {
      accepted: ToolLongOperationAcceptedV1;
      result_kind: 'long_operation_accepted';
    }
  | {
      entity_revision: Revision;
      result_kind: 'package_committed';
      tool_package_id: ToolPackageId;
    }
  | {
      entity_revision: Revision;
      result_kind: 'tool_committed';
      tool_id: ToolId;
    }
  | {
      entity_revision: Revision;
      result_kind: 'instance_committed';
      tool_instance_id: ToolInstanceId;
    }
  | {
      entity_revision: Revision;
      result_kind: 'workspace_pin_committed';
      workspace_id: WorkspaceId;
    }
  | {
      entity_revision: Revision;
      result_kind: 'session_pin_committed';
      session_id: SessionId;
    }
  | {
      operation: ToolOperationRefV1;
      operational_revision: Revision;
      result_kind: 'operation_cancelled';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolOperationRefV1".
 */
export type ToolOperationRefV1 =
  | {
      operation_kind: 'conformance';
      tool_conformance_run_id: ToolConformanceRunId;
    }
  | {
      journal_id: MutationJournalId;
      operation_kind: 'draft_apply';
      tool_draft_id: ToolDraftId;
    }
  | {
      journal_id: MutationJournalId;
      operation_kind: 'package_delete';
      tool_package_id: ToolPackageId;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageErrorCode".
 */
export type ToolPackageErrorCode =
  | 'tool.package.manifest_missing'
  | 'tool.package.manifest_invalid'
  | 'tool.package.manifest_too_large'
  | 'tool.package.package_key_mismatch'
  | 'tool.package.version_content_conflict'
  | 'tool.package.archive_too_large'
  | 'tool.package.expanded_size_exceeded'
  | 'tool.package.entry_count_exceeded'
  | 'tool.package.file_size_exceeded'
  | 'tool.package.compression_ratio_exceeded'
  | 'tool.package.path_depth_exceeded'
  | 'tool.package.invalid_path'
  | 'tool.package.case_collision'
  | 'tool.package.special_file_denied'
  | 'tool.package.encrypted_archive_denied'
  | 'tool.package.unsupported_compression'
  | 'tool.package.closed_file_set_mismatch'
  | 'tool.package.file_hash_mismatch'
  | 'tool.package.runtime_closure_invalid'
  | 'tool.package.fingerprint_mismatch'
  | 'tool.package.corrupted'
  | 'tool.package.payload_unavailable'
  | 'tool.package.import_journal_failed'
  | 'tool.package.deletion_approval_invalid'
  | 'tool.package.deletion_impact_changed'
  | 'tool.package.deletion_pending'
  | 'tool.package.deletion_journal_failed'
  | 'tool.package.source_unavailable';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageFingerprintKindV1".
 */
export type PackageFingerprintKindV1 = 'aura.p07.package_fingerprint';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FingerprintPreimageVersionV1".
 */
export type FingerprintPreimageVersionV1 = 1;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageRuntimeClosurePolicyVersionV1".
 */
export type PackageRuntimeClosurePolicyVersionV1 = 1;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageImportDiagnosticCodeV1".
 */
export type PackageImportDiagnosticCodeV1 =
  | 'invalid_manifest'
  | 'invalid_source'
  | 'path_denied'
  | 'limit_exceeded'
  | 'integrity_mismatch'
  | 'storage_unavailable';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageImportDiagnosticLocationV1".
 */
export type PackageImportDiagnosticLocationV1 =
  | {
      column?: number | null;
      field?: string | null;
      line?: number | null;
      type: 'manifest';
    }
  | {
      type: 'package_files';
    }
  | {
      type: 'import_source';
    }
  | {
      type: 'staging';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageConformanceStateV1".
 */
export type PackageConformanceStateV1 =
  'not_run' | 'running' | 'succeeded' | 'failed' | 'cancelled' | 'interrupted';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageInstallEffectV1".
 */
export type PackageInstallEffectV1 = 'trust_current_fingerprint_and_enable';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageExecutionLocationV1".
 */
export type PackageExecutionLocationV1 = 'controlled_http' | 'user_job';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageRuntimeSupportV1".
 */
export type PackageRuntimeSupportV1 = 'ready_for_conformance' | 'program_credential_unsupported';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageWorkingDirectoryPreviewV1".
 */
export type PackageWorkingDirectoryPreviewV1 =
  | {
      type: 'not_applicable';
    }
  | {
      declared: CommandWorkingDirectoryV1;
      /**
       * Absolute OS directory for display; it grants no resource authority.
       */
      path: string;
      type: 'resolved';
    }
  | {
      declared: CommandWorkingDirectoryV1;
      type: 'unresolved';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageToolCanonicalSuffixV1".
 */
export type PackageToolCanonicalSuffixV1 = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RawToolPackageModelDescriptionV1".
 */
export type RawToolPackageModelDescriptionV1 = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RawToolPackageResultContractV1".
 */
export type RawToolPackageResultContractV1 =
  | {
      type: 'unit';
    }
  | {
      type: 'utf8_text';
    }
  | {
      result_schema: ToolPackageJsonObjectV1;
      type: 'structured_json';
    };
/**
 * Raw Package Runtime union. External MCP/Plugin sources cannot be declared
 * by an imported Package and therefore have no variant here.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RawToolPackageRuntimeV1".
 */
export type RawToolPackageRuntimeV1 =
  | {
      command: RawToolPackageDeclarativeCommandV1;
      type: 'declarative_command';
    }
  | {
      arguments: ToolPackageCommandArgumentV1[];
      credential_slot?: CommandCredentialEnvironmentSlotV1 | null;
      environment: CommandEnvironmentEntryV1[];
      /**
       * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
       * via the `definition` "PackageRelativePath".
       */
      executable: PackageRelativePath;
      protocol_version: ToolPackageStdioProtocolVersionV1;
      type: 'aura_stdio';
      working_directory: CommandWorkingDirectoryV1;
    }
  | {
      request: RawToolPackageDeclarativeHttpV1;
      type: 'declarative_http';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RawToolPackageDeclarativeCommandV1".
 */
export type RawToolPackageDeclarativeCommandV1 =
  | {
      arguments: ToolPackageCommandArgumentV1[];
      credential_slot?: CommandCredentialEnvironmentSlotV1 | null;
      environment: CommandEnvironmentEntryV1[];
      /**
       * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
       * via the `definition` "PackageRelativePath".
       */
      executable: PackageRelativePath;
      success_exit_codes?: ToolPackageCommandSuccessExitCodesV1 | null;
      type: 'argv';
      working_directory: CommandWorkingDirectoryV1;
    }
  | {
      arguments: ToolPackageCommandArgumentV1[];
      credential_slot?: CommandCredentialEnvironmentSlotV1 | null;
      environment: CommandEnvironmentEntryV1[];
      /**
       * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
       * via the `definition` "PackageRelativePath".
       */
      script: PackageRelativePath;
      success_exit_codes?: ToolPackageCommandSuccessExitCodesV1 | null;
      type: 'powershell_script';
      working_directory: CommandWorkingDirectoryV1;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RawToolPackageDeclarativeHttpBodyV1".
 */
export type RawToolPackageDeclarativeHttpBodyV1 =
  | {
      type: 'none';
    }
  | {
      template: RawToolPackageHttpJsonTemplateV1;
      type: 'application_json';
    };
/**
 * Raw HTTP JSON templates preserve author order for arrays and object
 * property lists. Object properties reject duplicate names immediately, but
 * ordering is applied only by the explicit Raw-to-Canonical contract
 * conversion below.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RawToolPackageHttpJsonTemplateV1".
 */
export type RawToolPackageHttpJsonTemplateV1 =
  | {
      type: 'literal';
      value: any;
    }
  | {
      source: ArgumentJsonPointer;
      type: 'argument';
    }
  | {
      items: RawToolPackageHttpJsonTemplateV1[];
      type: 'array';
    }
  | {
      properties: RawToolPackageHttpJsonPropertyV1[];
      type: 'object';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RawToolPackageUiDescriptionV1".
 */
export type RawToolPackageUiDescriptionV1 = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageRuntimeVisibilityPolicyV1".
 */
export type PackageRuntimeVisibilityPolicyV1 = 'full_package';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageSecurityFingerprintKindV1".
 */
export type PackageSecurityFingerprintKindV1 = 'aura.p07.package_security_fingerprint';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolRegistryErrorCode".
 */
export type ToolRegistryErrorCode =
  | 'tool.registry.settings_invalid'
  | 'tool.registry.snapshot_not_found'
  | 'tool.registry.snapshot_unavailable'
  | 'tool.registry.snapshot_corrupted'
  | 'tool.registry.snapshot_version_mismatch'
  | 'tool.registry.pin_unresolved'
  | 'tool.registry.pin_degraded'
  | 'tool.registry.dynamic_schema_limit_below_pinned_count'
  | 'tool.registry.cursor_expired'
  | 'tool.registry.cursor_unavailable'
  | 'tool.registry.cursor_mismatch'
  | 'tool.registry.search_request_invalid'
  | 'tool.registry.tool_not_found'
  | 'tool.registry.version_mismatch'
  | 'tool.registry.trust_required'
  | 'tool.registry.disabled'
  | 'tool.registry.audience_denied'
  | 'tool.registry.capacity_exceeded';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ResourceClaimV1".
 */
export type ResourceClaimV1 =
  | {
      access: FileResourceAccessV1;
      selector: ResourceClaimSelectorV1;
      type: 'workspace_path';
    }
  | {
      access: FileResourceAccessV1;
      selector: ResourceClaimSelectorV1;
      type: 'external_file';
    }
  | {
      access: NetworkResourceAccessV1;
      selector: ResourceClaimSelectorV1;
      type: 'network_target';
    }
  | {
      access: ProcessResourceAccessV1;
      selector: ResourceClaimSelectorV1;
      type: 'process';
    }
  | {
      access: TerminalResourceAccessV1;
      selector: ResourceClaimSelectorV1;
      type: 'terminal_session';
    }
  | {
      access: RepositoryResourceAccessV1;
      selector: ResourceClaimSelectorV1;
      type: 'git_repository';
    }
  | {
      access: ReadWriteResourceAccessV1;
      selector: ResourceClaimSelectorV1;
      type: 'git_index';
    }
  | {
      access: GitRefResourceAccessV1;
      selector: ResourceClaimSelectorV1;
      type: 'git_ref';
    }
  | {
      access: GitSequencerResourceAccessV1;
      selector: ResourceClaimSelectorV1;
      type: 'git_sequencer';
    }
  | {
      access: ArtifactResourceAccessV1;
      selector: ResourceClaimSelectorV1;
      type: 'artifact';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ResourceClaimSelectorV1".
 */
export type ResourceClaimSelectorV1 =
  | {
      pointer: ArgumentJsonPointer;
      type: 'argument_json_pointer';
    }
  | {
      context: ResourceInvocationContextV1;
      type: 'invocation_context';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ResourceInvocationContextV1".
 */
export type ResourceInvocationContextV1 =
  | 'workspace_root'
  | 'package_root'
  | 'session'
  | 'turn_task'
  | 'agent_run'
  | 'tool_call'
  | 'attempt_staging';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolResourceErrorCode".
 */
export type ToolResourceErrorCode =
  | 'tool.resource.http_model_authentication_forbidden'
  | 'tool.resource.http_request_invalid'
  | 'tool.resource.http_credential_unavailable'
  | 'tool.resource.http_credential_rebind_required'
  | 'tool.resource.http_credential_origin_denied'
  | 'tool.resource.selector_missing'
  | 'tool.resource.selector_type_mismatch'
  | 'tool.resource.canonicalization_failed'
  | 'tool.resource.identity_resolution_failed'
  | 'tool.resource.scope_resolution_failed'
  | 'tool.resource.lock_key_invalid'
  | 'tool.resource.unsupported_claim'
  | 'tool.resource.resolver_unavailable';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolResultErrorCode".
 */
export type ToolResultErrorCode =
  | 'tool.result.utf8_invalid'
  | 'tool.result.json_invalid'
  | 'tool.result.schema_validation_failed'
  | 'tool.result.artifact_missing'
  | 'tool.result.evidence_invalid'
  | 'tool.result.side_effect_evidence_insufficient'
  | 'tool.result.outcome_state_invalid'
  | 'tool.result.reconciliation_inconclusive';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolRuntimeDescriptorV1".
 */
export type ToolRuntimeDescriptorV1 =
  | {
      command: DeclarativeCommandV1;
      type: 'declarative_command';
    }
  | {
      arguments: CommandArgumentV1[];
      credential_slot?: CommandCredentialEnvironmentSlotV1 | null;
      environment: CommandEnvironmentEntryV1[];
      /**
       * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
       * via the `definition` "PackageRelativePath".
       */
      executable: PackageRelativePath;
      type: 'aura_stdio';
      working_directory: CommandWorkingDirectoryV1;
    }
  | {
      request: DeclarativeHttpV1;
      type: 'declarative_http';
    }
  | {
      source_contract_key: string;
      type: 'external_source';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "DeclarativeCommandV1".
 */
export type DeclarativeCommandV1 =
  | {
      arguments: CommandArgumentV1[];
      credential_slot?: CommandCredentialEnvironmentSlotV1 | null;
      environment: CommandEnvironmentEntryV1[];
      /**
       * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
       * via the `definition` "PackageRelativePath".
       */
      executable: PackageRelativePath;
      success_exit_codes: number[];
      type: 'argv';
      working_directory: CommandWorkingDirectoryV1;
    }
  | {
      arguments: CommandArgumentV1[];
      credential_slot?: CommandCredentialEnvironmentSlotV1 | null;
      environment: CommandEnvironmentEntryV1[];
      /**
       * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
       * via the `definition` "PackageRelativePath".
       */
      script: PackageRelativePath;
      success_exit_codes: number[];
      type: 'powershell_script';
      working_directory: CommandWorkingDirectoryV1;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CommandArgumentV1".
 */
export type CommandArgumentV1 =
  | {
      type: 'literal';
      value: CommandLiteralV1;
    }
  | {
      source: ArgumentJsonPointer;
      type: 'scalar';
    }
  | {
      source: ArgumentJsonPointer;
      type: 'repeated';
    }
  | {
      flag: CommandFlagV1;
      source: ArgumentJsonPointer;
      type: 'conditional_flag';
    }
  | {
      name: CommandOptionNameV1;
      omit_when_missing_or_null: boolean;
      source: ArgumentJsonPointer;
      type: 'named_option';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolRuntimeErrorCode".
 */
export type ToolRuntimeErrorCode =
  | 'tool.runtime.http_model_authentication_forbidden'
  | 'tool.runtime.http_control_endpoint_denied'
  | 'tool.runtime.http_credential_reflection_blocked'
  | 'tool.runtime.unavailable'
  | 'tool.runtime.dispatch_outcome_unknown'
  | 'tool.runtime.dispatch_failed'
  | 'tool.runtime.timeout'
  | 'tool.runtime.io_limit_exceeded'
  | 'tool.runtime.process_launch_failed'
  | 'tool.runtime.process_tree_termination_unknown'
  | 'tool.runtime.protocol_frame_invalid'
  | 'tool.runtime.protocol_sequence_invalid'
  | 'tool.runtime.protocol_handshake_invalid'
  | 'tool.runtime.protocol_terminal_missing'
  | 'tool.runtime.protocol_terminal_conflict'
  | 'tool.runtime.artifact_invalid'
  | 'tool.runtime.http_request_invalid'
  | 'tool.runtime.http_redirect_denied'
  | 'tool.runtime.http_gateway_failed'
  | 'tool.runtime.credential_binding_invalid'
  | 'tool.runtime.persistence_outcome_unknown';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolSchedulerErrorCode".
 */
export type ToolSchedulerErrorCode =
  | 'tool.scheduler.concurrency_limit_invalid'
  | 'tool.scheduler.timeout_invalid'
  | 'tool.scheduler.retry_limit_invalid'
  | 'tool.scheduler.resource_wait_cancelled'
  | 'tool.scheduler.execution_plan_invalid'
  | 'tool.scheduler.execution_plan_hash_mismatch'
  | 'tool.scheduler.dispatch_state_invalid'
  | 'tool.scheduler.operation_already_terminal'
  | 'tool.scheduler.operation_not_cancellable';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolSchemaErrorCode".
 */
export type ToolSchemaErrorCode =
  | 'tool.schema.unsupported_dialect'
  | 'tool.schema.unsupported_keyword'
  | 'tool.schema.external_reference_denied'
  | 'tool.schema.unsupported_format'
  | 'tool.schema.invalid_pattern'
  | 'tool.schema.complexity_limit_exceeded'
  | 'tool.schema.meta_schema_invalid'
  | 'tool.schema.validator_compilation_failed'
  | 'tool.schema.input_root_not_object'
  | 'tool.schema.reserved_provider_metadata_property'
  | 'tool.schema.result_root_type_invalid'
  | 'tool.schema.hash_mismatch'
  | 'tool.schema.arguments_too_large'
  | 'tool.schema.arguments_invalid';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolSearchCursorV1".
 */
export type ToolSearchCursorV1 = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolSearchQueryV1".
 */
export type ToolSearchQueryV1 = string;
/**
 * The V1 wire keeps `message_type` and its typed `payload` at the frame root.
 * Common identity fields are deliberately repeated in every tagged variant so
 * Serde and JSON Schema cannot admit an invalid message/payload combination.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "StdioProtocolFrameV1".
 */
export type StdioProtocolFrameV1 =
  | {
      message_type: 'hello';
      payload: StdioHelloPayloadV1;
      protocol_version: number;
      sequence: number;
      tool_call_id: ToolCallId;
      tool_execution_attempt_id: ToolExecutionAttemptId;
    }
  | {
      message_type: 'execute';
      payload: StdioExecutePayloadV1;
      protocol_version: number;
      sequence: number;
      tool_call_id: ToolCallId;
      tool_execution_attempt_id: ToolExecutionAttemptId;
    }
  | {
      message_type: 'cancel';
      payload: StdioEmptyPayloadV1;
      protocol_version: number;
      sequence: number;
      tool_call_id: ToolCallId;
      tool_execution_attempt_id: ToolExecutionAttemptId;
    }
  | {
      message_type: 'progress';
      payload: StdioProgressPayloadV1;
      protocol_version: number;
      sequence: number;
      tool_call_id: ToolCallId;
      tool_execution_attempt_id: ToolExecutionAttemptId;
    }
  | {
      message_type: 'artifact_ready';
      payload: StdioArtifactReadyPayloadV1;
      protocol_version: number;
      sequence: number;
      tool_call_id: ToolCallId;
      tool_execution_attempt_id: ToolExecutionAttemptId;
    }
  | {
      message_type: 'cancel_acknowledged';
      payload: StdioEmptyPayloadV1;
      protocol_version: number;
      sequence: number;
      tool_call_id: ToolCallId;
      tool_execution_attempt_id: ToolExecutionAttemptId;
    }
  | {
      message_type: 'final_result';
      payload: StdioFinalResultPayloadV1;
      protocol_version: number;
      sequence: number;
      tool_call_id: ToolCallId;
      tool_execution_attempt_id: ToolExecutionAttemptId;
    }
  | {
      message_type: 'final_failure';
      payload: StdioFinalFailurePayloadV1;
      protocol_version: number;
      sequence: number;
      tool_call_id: ToolCallId;
      tool_execution_attempt_id: ToolExecutionAttemptId;
    };
export type ToolOutputRelativePathV1 = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolContractFingerprintKindV1".
 */
export type ToolContractFingerprintKindV1 = 'aura.p07.tool_contract_fingerprint';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolResultOutcomeSummaryV1".
 */
export type ToolResultOutcomeSummaryV1 =
  | {
      type: 'succeeded';
    }
  | {
      type: 'failed';
    }
  | {
      reason: ToolCancellationReasonV1;
      type: 'cancelled';
    }
  | {
      reason: ToolNotExecutedReasonV1;
      type: 'not_executed';
    }
  | {
      reason: ToolIndeterminateReasonV1;
      type: 'indeterminate';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SideEffectEvidenceProducerV1".
 */
export type SideEffectEvidenceProducerV1 =
  | 'core_transaction'
  | 'strict_sandbox'
  | 'workspace_journal'
  | 'trusted_network_adapter'
  | 'reconciliation';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AttemptSideEffectProofV1".
 */
export type AttemptSideEffectProofV1 =
  | {
      evidence_kind: 'no_effect';
      evidence_ref: ArtifactId;
    }
  | {
      evidence_kind: 'fully_applied';
      postcondition_ref: ArtifactId;
      receipt_ref: ArtifactId;
    }
  | {
      evidence_kind: 'partially_applied';
      receipt_ref: ArtifactId;
    }
  | {
      baseline_verification_ref: ArtifactId;
      compensation_ref: ArtifactId;
      evidence_kind: 'compensated';
      original_effect_ref: ArtifactId;
    }
  | {
      evidence_kind: 'unknown';
      evidence_ref: ArtifactId;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FileStorageSource".
 */
export type FileStorageSource = 'editor_buffer' | 'disk';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsExpectedState".
 */
export type FsExpectedState =
  | {
      expected_kind: 'absent';
    }
  | {
      expected_kind: 'file_hash';
      hash: ContentHash;
    }
  | {
      expected_kind: 'directory';
    };
/**
 * Renderer/model-safe export target. Arbitrary absolute paths are
 * intentionally absent: Core resolves either a Workspace path or a
 * Session-bound ExternalGrantId.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ArtifactExportTarget".
 */
export type ArtifactExportTarget =
  | {
      path: string;
      target_kind: 'workspace';
    }
  | {
      external_grant_id: ExternalGrantId;
      relative_path: string;
      target_kind: 'external_grant';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsMutationDisposition".
 */
export type FsMutationDisposition = 'applied' | 'deferred_by_editor_activity';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsChangesetOperation".
 */
export type FsChangesetOperation =
  | {
      content: string;
      encoding?: TextEncoding | null;
      expected: FsExpectedState;
      line_ending?: LineEnding | null;
      operation: 'write_file';
      target: WorkspaceFileTarget;
    }
  | {
      expected_hash: ContentHash;
      operation: 'apply_patch';
      target: WorkspaceFileTarget;
      unified_diff: string;
    }
  | {
      operation: 'create_directory';
      target: WorkspaceFileTarget;
    }
  | {
      destination: WorkspaceFileTarget;
      destination_expected: FsExpectedState;
      operation: 'copy';
      source: WorkspaceFileTarget;
      source_expected: FsExpectedState;
    }
  | {
      destination: WorkspaceFileTarget;
      destination_expected: FsExpectedState;
      operation: 'move';
      source: WorkspaceFileTarget;
      source_expected: FsExpectedState;
    }
  | {
      expected: FsExpectedState;
      operation: 'delete';
      recursive: boolean;
      target: WorkspaceFileTarget;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TextEncoding".
 */
export type TextEncoding = 'utf8' | 'utf8_bom' | 'utf16_le' | 'utf16_be' | 'windows1252';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "LineEnding".
 */
export type LineEnding = 'lf' | 'cr_lf' | 'cr' | 'mixed' | 'none';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsNodeKind".
 */
export type FsNodeKind = 'file' | 'directory' | 'reparse_point';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BinaryDisplayFormat".
 */
export type BinaryDisplayFormat = 'hex' | 'base64' | 'hex_with_ascii';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TextReadRange".
 */
export type TextReadRange =
  | {
      range_kind: 'full';
    }
  | {
      count: number;
      range_kind: 'lines';
      start_line_index: number;
    }
  | {
      count: number;
      range_kind: 'characters';
      start_character_offset: number;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsSearchPattern".
 */
export type FsSearchPattern =
  | {
      /**
       * Default false.
       */
      case_sensitive?: boolean;
      kind: 'literal';
      text: string;
    }
  | {
      /**
       * Default false.
       */
      case_sensitive?: boolean;
      kind: 'regex';
      pattern: string;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsSearchCompletion".
 */
export type FsSearchCompletion =
  | {
      status: 'complete';
    }
  | {
      reasons: FsSearchIncompleteReason[];
      status: 'partial';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsSearchIncompleteReason".
 */
export type FsSearchIncompleteReason =
  | 'scope_excluded'
  | 'result_limit'
  | 'scan_byte_budget'
  | 'time_budget'
  | 'file_too_large'
  | 'unsupported_encoding'
  | 'file_changed_during_search'
  | 'file_temporarily_unavailable';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsSkippedScopeReason".
 */
export type FsSkippedScopeReason = 'protected_scope' | 'control_authority' | 'target_unavailable';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceLifecycleOperation".
 */
export type WorkspaceLifecycleOperation =
  | {
      operation_kind: 'archive';
    }
  | {
      confirmed_same_logical_project: boolean;
      operation_kind: 'relocate';
      selected_target_root: string;
    }
  | {
      delete_confirmation_id: DeleteConfirmationId;
      operation_kind: 'permanent_delete';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceLifecycleResolution".
 */
export type WorkspaceLifecycleResolution =
  'save_all_and_continue' | 'return_to_editing' | 'discard_dirty_buffers_and_continue';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CatalogEntryKind".
 */
export type CatalogEntryKind = 'file' | 'directory' | 'reparse_point';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceCreateDisposition".
 */
export type WorkspaceCreateDisposition = 'created' | 'restored_archived' | 'existing';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RelocateConfirmation".
 */
export type RelocateConfirmation = 'require_same_object' | 'user_confirmed_same_logical_project';
/**
 * How a snapshot got shorter than what exists.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ActivityOmissionReason".
 */
export type ActivityOmissionReason =
  | 'summary_row_limit'
  | 'summary_byte_limit'
  | 'capacity_dropped'
  | 'index_unavailable'
  | 'rejected_fact'
  | 'visibility_filtered';
/**
 * Why the index is reporting less than everything.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ActivityCoverageState".
 */
export type ActivityCoverageState = 'complete' | 'partial' | 'unavailable';
/**
 * The real execution reference an activity is derived from.
 *
 * Every variant names identities the publishing owner really holds. The Agent
 * variants carry the real `ToolCall`/`Operation`/`Attempt`; the non-Agent
 * variants carry the identity that owner actually has, so no Agent identity is
 * fabricated for Main, a buffer commit or a managed background process.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ActivityExecutionRef".
 */
export type ActivityExecutionRef =
  | {
      attempt_ordinal: number;
      operation_id: SecurityOperationId;
      owner: 'agent_tool_call';
      tool_call_id: ToolCallId;
    }
  | {
      command_invocation_id?: CommandInvocationId | null;
      operation_id: SecurityOperationId;
      owner: 'main_file_mutation';
    }
  | {
      buffer_revision: Revision;
      owner: 'buffer_commit';
      reservation_id: EditorReservationId;
    }
  | {
      lifecycle_operation_id: WorkspaceLifecycleOperationId;
      owner: 'p04_publication';
      publication_id: DeferredMutationId;
    }
  | {
      correlation_id: CorrelationId;
      operation_id?: SecurityOperationId | null;
      owner: 'p04_file_publication';
    }
  | {
      attempt_ordinal: number;
      operation_id: SecurityOperationId;
      owner: 'shell_execution';
    }
  | {
      command_id: CommandId;
      invocation_id: CommandInvocationId;
      owner: 'shell_invocation';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ActivityId".
 */
export type ActivityId = string;
/**
 * Which real executor published this activity.
 *
 * The set is closed on purpose. A new publisher is a real new owner of a real
 * side effect, and it has to be named here before it can publish.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ActivityKind".
 */
export type ActivityKind =
  | 'agent_tool'
  | 'main_fs'
  | 'buffer_commit'
  | 'p04_publication'
  | 'shell_foreground'
  | 'shell_managed_background';
/**
 * One row of a query or a summary.
 * Informational source classification, independent of resource and permission descriptors.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ActivityPrivateSource".
 */
export type ActivityPrivateSource = 'workspace_metadata' | 'personal_data';
/**
 * What a caller is looking for.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ActivityQuery".
 */
export type ActivityQuery =
  | {
      by: 'session';
    }
  | {
      by: 'resource';
      target: ActivityResourceTarget;
    }
  | {
      by: 'group';
      group: ActivityToolGroup;
    }
  | {
      by: 'exact';
      id: ActivityId;
    }
  | {
      by: 'unknown_write_set';
    };
/**
 * The physical resource a relation applies to.
 *
 * These are A's already-verified facts, republished by the real owner. The
 * publication path copies them; it does not re-open the path, re-resolve the
 * identity or consult the journal to learn them.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ActivityResourceTarget".
 */
export type ActivityResourceTarget =
  | {
      ancestors?: FileIdentity[];
      identity: FileIdentity;
      kind: 'verified_object';
      subtree: boolean;
    }
  | {
      ancestors?: FileIdentity[];
      case_insensitive: boolean;
      /**
       * One-to-one name keys from the held-directory boundary. Raw names stay in `pending`.
       * Missing facts on an insensitive target are unproven, never language-folded guesses.
       */
      comparison_keys?: string[] | null;
      kind: 'verified_entry';
      parent: FileIdentity;
      pending: string[];
    }
  | {
      /**
       * The 128-bit file id, when the volume reports one.
       *
       * @minItems 16
       * @maxItems 16
       */
      file_id128?:
        | [
            number,
            number,
            number,
            number,
            number,
            number,
            number,
            number,
            number,
            number,
            number,
            number,
            number,
            number,
            number,
            number,
          ]
        | null;
      file_index: number;
      kind: 'existing_object';
      volume_serial: number;
    }
  | {
      case_insensitive: boolean;
      comparison_key?: string | null;
      kind: 'directory_entry';
      leaf: string;
      parent_file_index: number;
      parent_volume_serial: number;
    }
  | {
      file_index: number;
      inclusive: boolean;
      kind: 'subtree';
      volume_serial: number;
    }
  | {
      /**
       * A bounded, human-readable hint. Never a credential or a payload.
       */
      hint: string;
      kind: 'unproven';
    };
/**
 * The tool group. Groups fail independently: one broken group cannot take
 * another group's rows out of the index.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ActivityToolGroup".
 */
export type ActivityToolGroup = 'file_read' | 'file_write' | 'shell' | 'web' | 'other';
/**
 * What kind of access the executor has against one real target.
 *
 * `UnknownWriteSet` is not a placeholder for "we did not look": it is the honest
 * answer for a Shell whose write set cannot be proven. It never serialises
 * globally and it never disables Shell.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ActivityResourceRelation".
 */
export type ActivityResourceRelation = 'read' | 'write' | 'unknown_write_set';
/**
 * One picture or file the composer sends with a message.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentAttachmentDraft".
 */
export type AgentAttachmentDraft =
  | {
      data_base64: string;
      kind: 'image';
      media_type: string;
      name: string;
    }
  | {
      kind: 'text';
      name: string;
      text: string;
    }
  | {
      kind: 'file';
      path: string;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentContextRemediationState".
 */
export type AgentContextRemediationState = 'awaiting' | 'rechecking' | 'cleared' | 'cancelled';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentContextWindowState".
 */
export type AgentContextWindowState =
  'awaiting' | 'preparing' | 'request_running' | 'applied' | 'cleared' | 'cancelled';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentContextWindowAction".
 */
export type AgentContextWindowAction = 'recheck' | 'try_smaller_windows';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentContinuation".
 */
export type AgentContinuation =
  | {
      trigger: 'initial';
      user_event_id: EventId;
    }
  | {
      history_group_hash: CanonicalSha256HashV1;
      tool_batch_id: ToolBatchId;
      trigger: 'tool_batch';
    }
  | {
      previous_model_step_id: ModelStepId;
      trigger: 'completion_reminder';
    }
  | {
      query_id: UserQueryId;
      trigger: 'query_answered';
      user_event_id: EventId;
    }
  | {
      recovery_episode: number;
      trigger: 'recovered';
    }
  | {
      context_snapshot_id: ContextProjectionSnapshotId;
      trigger: 'compacted';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "UserQueryId".
 */
export type UserQueryId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentFileDeleteOutcome".
 */
export type AgentFileDeleteOutcome = 'recycled' | 'deleted';
/**
 * A path relative to the workspace root ("." is the root). An absolute path inside the workspace names the same file.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentWorkspacePath".
 */
export type AgentWorkspacePath = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentFileWriteExpectation".
 */
export type AgentFileWriteExpectation = FsExpectedState | ContentHash;
/**
 * Where the final answer of a completed turn really comes from. The desktop
 * projection carries this so one source is rendered once instead of comparing
 * report strings, which never matched a model-rewritten final answer.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentFinalTextSource".
 */
export type AgentFinalTextSource =
  | {
      kind: 'assistant_step';
      model_step_id: ModelStepId;
    }
  | {
      kind: 'completion_report';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentInboxAction".
 */
export type AgentInboxAction =
  | {
      action: 'edit';
      message_id: EventId;
      text: string;
    }
  | {
      action: 'remove';
      message_id: EventId;
    }
  | {
      action: 'append';
      message_id: EventId;
      turn_task_id: TurnTaskId;
    }
  | {
      action: 'revoke';
      message_id: EventId;
    }
  | {
      action: 'pause';
    }
  | {
      action: 'resume';
    };
/**
 * One picture or file a message carries, as Aura keeps it with the message.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentMessageAttachment".
 */
export type AgentMessageAttachment =
  | {
      artifact_id: ArtifactId;
      byte_length: number;
      kind: 'image';
      media_type: string;
      name: string;
    }
  | {
      artifact_id: ArtifactId;
      byte_length: number;
      kind: 'text';
      name: string;
    }
  | {
      byte_length: number;
      kind: 'file';
      listing_artifact_id?: ArtifactId | null;
      name: string;
      path: string;
    }
  | {
      byte_length: number;
      external_grant_id: ExternalGrantId;
      kind: 'external_file';
      listing_artifact_id?: ArtifactId | null;
      name: string;
      path: string;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentInputLane".
 */
export type AgentInputLane =
  | {
      lane: 'next_turn';
    }
  | {
      lane: 'addition';
      turn_task_id: TurnTaskId;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TurnReasoningEffort".
 */
export type TurnReasoningEffort = 'off' | 'minimal' | 'low' | 'medium' | 'high' | 'xhigh' | 'max';
/**
 * Where a Skill lives: `aura_store` (imported into Aura), `agent_global` (the user's
 * `.agents\skills`) or `workspace` (this workspace's `.agents\skills`).
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SkillOrigin".
 */
export type SkillOrigin = 'aura_store' | 'agent_global' | 'workspace';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentMessageTextSource".
 */
export type AgentMessageTextSource =
  | {
      event_id: EventId;
      kind: 'user';
    }
  | {
      kind: 'assistant';
      model_step_id: ModelStepId;
    }
  | {
      kind: 'reasoning';
      model_step_id: ModelStepId;
    }
  | {
      kind: 'completion';
      turn_task_id: TurnTaskId;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentModeControlAction".
 */
export type AgentModeControlAction =
  | {
      action: 'terminate';
    }
  | {
      action: 'pause_goal';
    }
  | {
      action: 'edit_goal_budget';
      goal_budget: GoalBudget;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModeEpisodeEndReason".
 */
export type ModeEpisodeEndReason =
  'plan_completed' | 'goal_completed' | 'user_terminated' | 'skipped';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentTaskMode".
 */
export type AgentTaskMode = 'normal' | 'plan' | 'goal';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "QueryAnswerValue".
 */
export type QueryAnswerValue =
  | {
      answer: 'text';
      text: string;
    }
  | {
      answer: 'choices';
      ids: string[];
    }
  | {
      accepted: boolean;
      answer: 'confirm';
    }
  | {
      answer: 'file_or_directory';
      file_reference_id: FileReferenceId;
    }
  | {
      answer: 'custom';
      text: string;
    }
  | {
      answer: 'declined';
    }
  | {
      answer: 'no_response';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentRelatedTurnKind".
 */
export type AgentRelatedTurnKind = 'implement_plan' | 'recover';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentRunState".
 */
export type AgentRunState =
  | {
      state: 'preparing_context';
    }
  | {
      model_step_id: ModelStepId;
      state: 'awaiting_context_remediation';
    }
  | {
      model_step_id: ModelStepId;
      state: 'model_running';
    }
  | {
      state: 'tools_running';
      tool_batch_id: ToolBatchId;
    }
  | {
      state: 'awaiting_approval';
      tool_batch_id: ToolBatchId;
    }
  | {
      query_id: UserQueryId;
      state: 'awaiting_query';
    }
  | {
      state: 'completed';
    }
  | {
      state: 'cancelled';
    }
  | {
      episode: number;
      state: 'recovery_required';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelStepState".
 */
export type ModelStepState =
  | {
      state: 'preparing';
    }
  | {
      execution_snapshot_id: ModelExecutionSnapshotId;
      state: 'running';
    }
  | {
      assistant_event_id: EventId;
      output_artifact_id: ArtifactId;
      state: 'completed';
      tool_batch_id?: ToolBatchId | null;
    }
  | {
      error_artifact_id: ArtifactId;
      state: 'failed';
    }
  | {
      state: 'cancelled';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "QueryAnswerType".
 */
export type QueryAnswerType =
  | {
      type: 'text';
    }
  | {
      options: QueryChoice[];
      type: 'single_choice';
    }
  | {
      options: QueryChoice[];
      type: 'multi_choice';
    }
  | {
      type: 'confirm';
    }
  | {
      type: 'file_or_directory';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "UserQueryState".
 */
export type UserQueryState =
  | {
      state: 'pending';
    }
  | {
      answers: QueryAnswer[];
      state: 'answered';
      user_event_id: EventId;
    }
  | {
      state: 'cancelled';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TurnTaskState".
 */
export type TurnTaskState =
  | {
      state: 'pending';
    }
  | {
      state: 'running';
    }
  | {
      state: 'awaiting_approval';
    }
  | {
      query_id: UserQueryId;
      state: 'awaiting_query';
    }
  | {
      final_report: string;
      state: 'completed';
    }
  | {
      reason: string;
      state: 'failed';
    }
  | {
      state: 'cancelled';
    }
  | {
      reason: string;
      state: 'recovery_required';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentSessionActivityState".
 */
export type AgentSessionActivityState = 'running' | 'waiting';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TodoItemId".
 */
export type TodoItemId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TodoStatus".
 */
export type TodoStatus = 'pending' | 'in_progress' | 'completed' | 'blocked' | 'cancelled';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "UserMessageOrigin".
 */
export type UserMessageOrigin =
  | {
      origin: 'user_input';
    }
  | {
      origin: 'steer';
    }
  | {
      origin: 'query_answer';
      query_id: UserQueryId;
    };
/**
 * What a turn is for. Only `/init` differs from an ordinary turn: it is a one-off
 * task that runs as Normal and leaves the session's Plan or Goal mode untouched.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentTaskPurpose".
 */
export type AgentTaskPurpose = 'conversation' | 'workspace_init' | 'compaction';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TerminalEndReason".
 */
export type TerminalEndReason =
  | 'exited'
  | 'user_closed'
  | 'agent_closed'
  | 'core_stopped'
  | 'session_deleted'
  | 'owner_dropped'
  | 'output_failed';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentTerminalShell".
 */
export type AgentTerminalShell = 'powershell' | 'cmd';
/**
 * A choice only the
 * user can make, offered on the row of a call it stopped.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentToolOffer".
 */
export type AgentToolOffer = 'enable_browser_debugging';
/**
 * The service `web.search` and `web.fetch` use.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WebServiceSettingV1".
 */
export type WebServiceSettingV1 =
  | {
      kind: 'automatic';
    }
  | {
      kind: 'local';
    }
  | {
      endpoint_id: EndpointId;
      kind: 'deepseek';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AttemptState".
 */
export type AttemptState =
  | {
      state: 'prepared';
    }
  | {
      state: 'running';
    }
  | {
      state: 'waiting_external';
    }
  | {
      state: 'cancel_requested';
    }
  | {
      result: ExecutionResult;
      state: 'terminal';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExecutionOutcome".
 */
export type ExecutionOutcome = 'succeeded' | 'failed' | 'cancelled' | 'timed_out' | 'unknown';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RollbackEvidence".
 */
export type RollbackEvidence =
  | {
      rollback: 'not_applicable';
    }
  | {
      evidence_ref: ArtifactId;
      rollback: 'available';
    }
  | {
      evidence_ref: ArtifactId;
      rollback: 'completed';
    }
  | {
      rollback: 'unavailable';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TerminationEvidence".
 */
export type TerminationEvidence =
  | {
      evidence_ref: ArtifactId;
      termination: 'confirmed';
    }
  | {
      evidence_ref: ArtifactId;
      termination: 'runtime_managed';
    }
  | {
      termination: 'unknown';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BackendClass".
 */
export type BackendClass =
  | 'core_broker'
  | 'restricted_user_offline'
  | 'restricted_user_online'
  | 'native'
  | 'app_container'
  | 'container';
/**
 * Independent enforcement requirements, not a total ordering of backends.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "EnforcementGuarantee".
 */
export type EnforcementGuarantee =
  | (
      | 'authorized_reads_only'
      | 'credential_isolation'
      | 'identity_isolation'
      | 'authorized_writes_only'
      | 'offline_network'
      | 'exact_network_origins'
      | 'child_process_control'
      | 'bounded_output'
    )
  | 'read_only_user_files'
  | 'direct_network_blocked_v1';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AuthorizationLifetime".
 */
export type AuthorizationLifetime =
  | {
      lifetime: 'once';
      operation_id: SecurityOperationId;
    }
  | {
      lifetime: 'turn_task';
      session_id: SessionId;
      turn_task_id: TurnTaskId;
    }
  | {
      lifetime: 'session';
      session_id: SessionId;
    }
  | {
      lifetime: 'workspace';
      workspace_id: WorkspaceId;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryVaultScope".
 */
export type MemoryVaultScope =
  | {
      kind: 'global';
    }
  | {
      kind: 'project';
      workspace_id: WorkspaceId;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NetworkConsentCaller".
 */
export type NetworkConsentCaller =
  'ordinary_tools' | 'built_in_shells' | 'git' | 'package_tools' | 'aura_browser';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NetworkConsentDestinations".
 */
export type NetworkConsentDestinations =
  | 'all_ordinary_destinations'
  | {
      git: {
        target: GitRemoteTargetV1;
      };
    }
  | {
      package: {
        package_fingerprint: CanonicalSha256HashV1;
        package_id: ToolPackageId;
      };
    }
  | {
      browser_site: {
        host: string;
      };
    };
/**
 * Policy profile, independent of the UI's file-access shorthand.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SecurityProfile".
 */
export type SecurityProfile = 'balanced' | 'strict' | 'guided' | 'full_access';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AuthorizationBasis".
 */
export type AuthorizationBasis =
  | {
      basis: 'authenticated_main';
      channel_id: IpcChannelId;
      core_runtime_instance_id: RuntimeInstanceId;
      main_runtime_instance_id: RuntimeInstanceId;
      principal_id: PrincipalId;
    }
  | {
      basis: 'permission_mode';
      mode: PermissionMode;
      session_permission_revision: number;
      snapshot_id: PermissionSnapshotId;
    }
  | {
      basis: 'grant';
      grant_id: PermissionGrantId;
      revision: Revision;
    }
  | {
      basis: 'composite';
      slices: CapabilityAuthorization[];
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CapabilityAuthority".
 */
export type CapabilityAuthority =
  | {
      basis: 'permission_mode';
      mode: PermissionMode;
      session_permission_revision: number;
      snapshot_id: PermissionSnapshotId;
    }
  | {
      basis: 'grant';
      grant_id: PermissionGrantId;
      revision: Revision;
      scope: AuthorizationScope;
    }
  | {
      basis: 'external_grant';
      grant_id: ExternalGrantId;
      revalidation_revision: number;
    };
/**
 * Resolved policy resource. Opaque domain resources identify exact immutable
 * content; they grant no filesystem access without the owning domain resolver.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ResourceDescriptor".
 */
export type ResourceDescriptor =
  | {
      scope_kind: 'workspace_root';
      workspace_id: WorkspaceId;
    }
  | {
      canonical_relative_path: string;
      file_target?: FileConsentTarget | null;
      scope_kind: 'workspace_path';
      workspace_id: WorkspaceId;
    }
  | {
      file_reference_id: FileReferenceId;
      scope_kind: 'session_authorized_file';
    }
  | {
      canonical_path: CanonicalPath;
      /**
       * True when this object is a directory whose contents are part of the frozen scope.
       * Absent on older operations and on single-file operations, so their fingerprints
       * stay unchanged. A directory with `recursive: false` covers that directory and its
       * direct children; `recursive: true` covers the approved subtree. The model's
       * recursive argument is only a request until this fact is what the user approves.
       */
      directory?: boolean;
      file_target?: FileConsentTarget | null;
      identity?: FileIdentity | null;
      parent_identity?: FileIdentity | null;
      recursive: boolean;
      scope_kind: 'external_path';
    }
  | {
      grant_id: ExternalGrantId;
      scope_kind: 'external_grant';
    }
  | {
      host: string;
      port: number;
      scheme: string;
      scope_kind: 'network_origin';
    }
  | {
      class: PrivateNetworkClass;
      scope_kind: 'private_network';
    }
  | {
      object_ref?: string | null;
      scope_kind: 'external_service';
      service_id: string;
    }
  | {
      canonical_path: CanonicalPath;
      file_target?: FileConsentTarget | null;
      /**
       * Unbound when classification did not verify an object, including every operation
       * stored before identity binding existed. Absent and explicit JSON null both mean
       * unbound and are omitted from canonical JSON, so those fingerprints still verify.
       * A verified identity is serialized and is part of the fingerprint.
       */
      identity?: FileIdentity | null;
      parent_identity?: FileIdentity | null;
      scope_kind: 'system_path';
    }
  | {
      canonical_path: CanonicalPath;
      file_target?: FileConsentTarget | null;
      /**
       * Same canonical rule as [`ResourceDescriptor::SystemPath`]: an unbound identity is
       * omitted, and a verified identity is bound into the operation fingerprint.
       */
      identity?: FileIdentity | null;
      parent_identity?: FileIdentity | null;
      scope_kind: 'sensitive_user_path';
    }
  | {
      hive: string;
      path: string;
      scope_kind: 'registry_key';
    }
  | {
      scope: EnvironmentVariableScope;
      scope_kind: 'environment_scope';
    }
  | {
      scope_kind: 'git_repository';
      workspace_id: WorkspaceId;
    }
  | {
      scope: MemoryVaultScope;
      scope_kind: 'memory_vault';
      vault_identity: ContentHash;
    }
  | {
      remote: string;
      scope_kind: 'git_remote';
    }
  | {
      facts: GitRemoteFactsV1;
      scope_kind: 'git_remote_destination';
    }
  | {
      page_ref: string;
      scope_kind: 'browser_page';
    }
  | {
      allowed_by_rule?: string | null;
      host: string;
      scope_kind: 'browser_site';
    }
  | {
      resource_fingerprint: CanonicalSha256HashV1;
      scope_kind: 'package_runtime';
    }
  | {
      package_fingerprint: CanonicalSha256HashV1;
      package_id: ToolPackageId;
      runtime: PackageRuntimeClass;
      scope_kind: 'tool_package';
    }
  | {
      resource_fingerprint: CanonicalSha256HashV1;
      scope_kind: 'tool_conformance_runtime';
    }
  | {
      config_fingerprint: ContentHash;
      scope_kind: 'mcp_server';
      server_id: string;
    }
  | {
      resource_fingerprint: CanonicalSha256HashV1;
      scope_kind: 'tool_registry_snapshot';
    }
  | {
      agent_run_id: AgentRunId;
      scope_kind: 'turn_task_tool_exposure';
      session_id: SessionId;
      turn_task_id: TurnTaskId;
    }
  | {
      agent_run_id: AgentRunId;
      scope_kind: 'agent_task';
      session_id: SessionId;
      turn_task_id: TurnTaskId;
    };
/**
 * How a package tool reaches the outside: through Core's controlled HTTP, or as a
 * program running with the user's identity in a Job.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageRuntimeClass".
 */
export type PackageRuntimeClass = 'http' | 'program';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExecutionCompatibility".
 */
export type ExecutionCompatibility =
  | 'native_windows_toolchain'
  | 'long_running_process'
  | 'interactive_terminal'
  | 'process_tree_cancellation';
/**
 * Registration key, not an allocated persistent entity ID.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BackendId".
 */
export type BackendId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BackendKnownLimitation".
 */
export type BackendKnownLimitation = 'dnscache_service_mediated_dns';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BackendReadiness".
 */
export type BackendReadiness =
  | {
      readiness: 'ready';
    }
  | {
      readiness: 'unavailable';
      reason: string;
    }
  | {
      readiness: 'setup_required';
      reason: string;
    }
  | {
      readiness: 'repair_required';
      reason: string;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BackendError".
 */
export type BackendError =
  | {
      error: 'unavailable';
      reason: string;
    }
  | {
      error: 'preparation_rejected';
      reason: string;
    }
  | {
      error: 'dispatch_rejected';
      reason: string;
    }
  | {
      attempt: AttemptRef;
      error: 'dispatch_unknown';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BackendLifecycle".
 */
export type BackendLifecycle = 'dormant' | 'starting' | 'ready' | 'degraded';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BackgroundState".
 */
export type BackgroundState = 'running' | 'exited' | 'interrupted' | 'unknown';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserAssistOutcome".
 */
export type BrowserAssistOutcome = 'done' | 'declined' | 'timed_out' | 'closed';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserConsoleLevel".
 */
export type BrowserConsoleLevel = 'error' | 'warning' | 'all';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserHistoryAction".
 */
export type BrowserHistoryAction = 'back' | 'forward' | 'reload';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserHostActionV1".
 */
export type BrowserHostActionV1 =
  | {
      kind: 'open';
      url: string;
    }
  | {
      action: BrowserHistoryAction;
      kind: 'navigate';
    }
  | {
      kind: 'snapshot';
      max_chars: number;
    }
  | {
      element?: string | null;
      full_page: boolean;
      kind: 'screenshot';
    }
  | {
      button: BrowserMouseButton;
      double: boolean;
      element?: string | null;
      kind: 'click';
      x?: number | null;
      y?: number | null;
    }
  | {
      clear: boolean;
      element?: string | null;
      kind: 'type';
      submit: boolean;
      text: string;
    }
  | {
      hold_ms: number;
      key: string;
      kind: 'press';
      repeat: number;
    }
  | {
      amount: number;
      direction: BrowserScrollDirection;
      element?: string | null;
      kind: 'scroll';
    }
  | {
      element: string;
      kind: 'hover';
    }
  | {
      element: string;
      kind: 'select';
      values: string[];
    }
  | {
      kind: 'wait';
      text?: string | null;
      text_gone?: string | null;
      time_ms?: number | null;
      timeout_ms: number;
    }
  | {
      kind: 'console';
      level: BrowserConsoleLevel;
      max_entries: number;
    }
  | {
      kind: 'close';
    }
  | {
      kind: 'request_user';
      reason: string;
      timeout_ms: number;
    }
  | {
      expression: string;
      kind: 'evaluate';
    }
  | {
      failed_only: boolean;
      kind: 'network';
      max_entries: number;
    }
  | {
      element: string;
      kind: 'inspect';
    }
  | {
      kind: 'performance';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserMouseButton".
 */
export type BrowserMouseButton = 'left' | 'right' | 'middle';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserScrollDirection".
 */
export type BrowserScrollDirection = 'up' | 'down' | 'left' | 'right';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserHostOutcomeV1".
 */
export type BrowserHostOutcomeV1 =
  | {
      image?: BrowserImageV1 | null;
      outcome: 'completed';
      result: any;
    }
  | {
      message: string;
      outcome: 'refused';
    }
  | {
      may_have_acted: boolean;
      message: string;
      outcome: 'failed';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ConfigModule".
 */
export type ConfigModule =
  | 'aura'
  | 'models'
  | 'tools'
  | 'hooks'
  | 'mcp'
  | 'skills'
  | 'commands'
  | 'plugins'
  | 'ui'
  | 'prompt'
  | 'security'
  | 'browser'
  | 'memory'
  | 'diagnostics'
  | 'update';
/**
 * Which recovery action the user is asking for.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ConfigRecoveryAction".
 */
export type ConfigRecoveryAction = 'retry' | 'restore_previous_valid' | 'reset_to_default';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CancelDisposition".
 */
export type CancelDisposition = 'accepted' | 'too_late_to_cancel' | 'unknown_request';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CapabilityReadiness".
 */
export type CapabilityReadiness = 'ready' | 'unavailable';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CommandKind".
 */
export type CommandKind = 'immediate' | 'block' | 'prompt';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CommandSource".
 */
export type CommandSource = 'builtin' | 'user' | 'workspace' | 'plugin';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CompletionAssessment".
 */
export type CompletionAssessment =
  | {
      assessment: 'fully_satisfied';
    }
  | {
      assessment: 'best_effort_balanced';
      attempted_alternatives: string[];
      limiting_factors: string[];
      unresolved_intents: string[];
      why_further_work_is_not_reasonable: string;
    }
  | {
      assessment: 'not_ready';
      next_actions: string[];
      remaining_work: string[];
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CompletionGate".
 */
export type CompletionGate =
  | {
      gate: 'unreviewed';
    }
  | {
      gate: 'reviewed_not_ready';
      lineage_revision: Revision;
      review_id: CompletionReviewId;
    }
  | {
      gate: 'reviewed_ready';
      lineage_revision: Revision;
      outcome: NormalCompletionOutcome;
      review_id: CompletionReviewId;
    }
  | {
      gate: 'invalidated';
      reason: CompletionInvalidation;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CompletionReviewId".
 */
export type CompletionReviewId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NormalCompletionOutcome".
 */
export type NormalCompletionOutcome =
  ('fully_satisfied' | 'best_effort_balanced') | 'model_decision';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CompletionInvalidation".
 */
export type CompletionInvalidation =
  | 'new_user_source_attached'
  | 'steer_attached'
  | 'query_answer_attached'
  | 'relation_changed'
  | 'completion_policy_changed'
  | 'lineage_switched'
  | 'source_reclassified'
  | 'context_compacted';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CompletionPolicy".
 */
export type CompletionPolicy =
  | {
      policy: 'best_effort_balanced';
    }
  | {
      policy: 'strict_completion_required';
      source_event_ids: EventId[];
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ReviewedIntentResult".
 */
export type ReviewedIntentResult =
  | 'satisfied'
  | 'satisfied_within_user_requested_scope'
  | 'deferred_by_user'
  | 'cancelled_by_user'
  | 'unresolved';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SourceReadReceiptId".
 */
export type SourceReadReceiptId = string;
/**
 * Where a domain's currently-served snapshot came from.
 *
 * Serializable because a recovery outcome reports it to the user, and the
 * outcome is part of a projection. The wire form is the stable snake_case key, so
 * a renderer can name the source without a second mapping table.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ConfigSource".
 */
export type ConfigSource = 'default' | 'file' | 'last_known_good';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextBrokerReadRequest".
 */
export type ContextBrokerReadRequest =
  | {
      kind: 'status';
      request: {
        agent_run_id: AgentRunId;
        session_id: SessionId;
      };
    }
  | {
      kind: 'search';
      request: ContextSearchInput;
    }
  | {
      kind: 'read_events';
      request: ContextReadEventsInput;
    }
  | {
      kind: 'read_tool_result';
      request: ContextReadToolResultInput;
    }
  | {
      kind: 'read_compaction';
      request: ContextReadCompactionInput;
    }
  | {
      kind: 'memory_settings';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CoreHealth".
 */
export type CoreHealth = 'starting' | 'ready' | 'recovery_required';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CoreRunnerMessage".
 */
export type CoreRunnerMessage =
  | {
      envelope: RunnerDispatchEnvelope;
      message: 'execute';
    }
  | {
      attempt: AttemptRef;
      message: 'cancel';
    };
/**
 * Core-derived authority reference. A reference identifies a source; it does
 * not prove admission, trust, or permission. Tool adapters hash the complete
 * validated source, including its exact version, Registry and fingerprints.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SecuritySourceRef".
 */
export type SecuritySourceRef =
  | {
      source_kind: 'user_interface';
    }
  | {
      identity_hash: CanonicalSha256HashV1;
      source_kind: 'built_in_tool';
    }
  | {
      identity_hash: CanonicalSha256HashV1;
      source_kind: 'tool';
    }
  | {
      identity_hash: CanonicalSha256HashV1;
      source_kind: 'tool_conformance';
    }
  | {
      hook_id: string;
      source_kind: 'hook';
    }
  | {
      server_id: string;
      source_kind: 'mcp_server';
    }
  | {
      skill_id: string;
      source_kind: 'skill';
    }
  | {
      plugin_id: string;
      source_kind: 'plugin';
    }
  | {
      component_id: string;
      source_kind: 'system_component';
    };
/**
 * Broad process networking is never represented as exact-origin HTTP access.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "OperationNetwork".
 */
export type OperationNetwork = 'none' | 'exact_origins' | 'broad_process' | 'browser_site_policy';
/**
 * Secret-generation revision for one stable CredentialId.
 *
 * This is intentionally distinct from a row revision. Clear and metadata-only
 * mutations may change row concurrency without creating a new secret
 * generation.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CredentialRevision".
 */
export type CredentialRevision = string;
/**
 * The terminal a new terminal tab opens.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "DefaultTerminal".
 */
export type DefaultTerminal = 'power_shell' | 'cmd';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExecutionAcceptanceCase".
 */
export type ExecutionAcceptanceCase =
  | 'source_build_install_binding'
  | 'managed_power_shell'
  | 'system_cmd'
  | 'fixed_environment'
  | 'stdin_stdout_stderr'
  | 'file_read'
  | 'workspace_write'
  | 'read_only_write_denied'
  | 'path_escape_denied'
  | 'credential_canary_denied'
  | 'restricted_identity'
  | 'child_job_inheritance'
  | 'cancel_tree_stopped'
  | 'output_limit'
  | 'timeout'
  | 'repeated_dispatch_denied'
  | 'stale_authority_denied'
  | 'result_unique_terminal'
  | 'post_dispatch_crash_unknown'
  | 'core_restart_recovery'
  | 'system_reboot_recovery'
  | 'maintenance_configuration_protected'
  | 'owner_session_isolation'
  | 'unprivileged_after_reboot'
  | 'direct_tcp_v4_blocked'
  | 'direct_udp_v4_blocked'
  | 'direct_tcp_v6_blocked'
  | 'direct_udp_v6_blocked'
  | 'dnscache_known_limitation'
  | 'main_user_network_unaffected'
  | 'installation_repair'
  | 'installation_rollback'
  | 'installation_remove'
  | 'online_tcp_v4_allowed'
  | 'online_udp_v4_allowed'
  | 'online_tcp_v6_allowed'
  | 'online_udp_v6_allowed'
  | 'online_synthetic_upload'
  | 'offline_network_positive_control'
  | 'offline_network_negative_control'
  | 'offline_online_identity_isolation'
  | 'native_explicit_approval'
  | 'native_isolation_reduced'
  | 'old_dispatch_binding_denied'
  | 'controlled_web_exact_origin'
  | 'controlled_web_redirect'
  | 'controlled_web_limits'
  | 'controlled_web_reflection'
  | 'built_in_registry'
  | 'stable_registry_snapshot'
  | 'stable_execution_history'
  | 'batch_admission'
  | 'dependency_scheduling'
  | 'lock_scheduling'
  | 'tool_attempt_lifecycle'
  | 'tool_result_lifecycle'
  | 'tool_side_effect_classification'
  | 'history_atomic_group'
  | 'agent_without_package_editor'
  | 'core_route_broker_tool_loop'
  | 'core_route_offline_tool_loop'
  | 'text_only_completion'
  | 'sequential_tool_execution'
  | 'parallel_tool_execution'
  | 'tool_rejection'
  | 'non_zero_exit'
  | 'tool_start_failure'
  | 'cancellation'
  | 'unknown_result'
  | 'recovery'
  | 'without_provider_continuation'
  | 'compaction'
  | 'todo_lifecycle'
  | 'plan_lifecycle'
  | 'goal_lifecycle'
  | 'task_lineage_lifecycle'
  | 'p06_history_reconstruction'
  | 'background_query'
  | 'background_stop'
  | 'background_normal_completion'
  | 'background_crash_recovery'
  | 'immutable_pagination'
  | 'electron_main_core_production_chain';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExecutionCheckResult".
 */
export type ExecutionCheckResult = 'passed' | 'failed' | 'not_run' | 'accepted_known_limitation';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExecutionHostEdition".
 */
export type ExecutionHostEdition = 'home' | 'pro';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExtensionReadiness".
 */
export type ExtensionReadiness = 'disabled' | 'ready' | 'degraded';
/**
 * The projected force of a directory or file grant. Turn end is decided by the caller
 * that knows the session's current turn; this value is what the management UI shows.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExternalGrantAvailability".
 */
export type ExternalGrantAvailability =
  ('in_force' | 'expired' | 'suspended' | 'revoked') | 'turn_ended';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FileConsentProtection".
 */
export type FileConsentProtection = 'ordinary' | 'system' | 'sensitive_user' | 'control_authority';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GitBranchInput".
 */
export type GitBranchInput =
  | {
      kind: 'create';
      name: string;
      start?: string | null;
      switch?: boolean;
    }
  | {
      kind: 'switch';
      name: string;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GitDiffTargetV1".
 */
export type GitDiffTargetV1 =
  | {
      kind: 'unstaged';
    }
  | {
      kind: 'staged';
    }
  | {
      commit: string;
      kind: 'commit';
    }
  | {
      from: string;
      kind: 'range';
      to: string;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GitReadInputV1".
 */
export type GitReadInputV1 =
  | {
      input: GitStatusInput;
      kind: 'status';
    }
  | {
      input: GitDiffInput;
      kind: 'diff';
    }
  | {
      input: GitLogInput;
      kind: 'log';
    }
  | {
      input: GitUntrackedDiffInputV1;
      kind: 'untracked_diff';
    }
  | {
      input: GitReviewDiffInputV1;
      kind: 'review_diff';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GitReadResultV1".
 */
export type GitReadResultV1 =
  | {
      kind: 'status';
      result: GitStatusResultV1;
    }
  | {
      kind: 'diff';
      result: GitDiffResultV1;
    }
  | {
      kind: 'log';
      result: GitLogResultV1;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GitRuntimeStateV1".
 */
export type GitRuntimeStateV1 = 'available' | 'missing' | 'too_old' | 'invalid';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GitSyncInput".
 */
export type GitSyncInput =
  | {
      kind: 'fetch';
      remote?: string | null;
    }
  | {
      branch?: string | null;
      kind: 'pull';
      remote?: string | null;
    };
/**
 * The program, always inside the package: its executable, or a script run by the
 * PowerShell that ships with Aura (the same two forms tool packages use).
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HookCommand".
 */
export type HookCommand =
  | {
      arguments: string[];
      path: string;
      type: 'executable';
    }
  | {
      arguments: string[];
      path: string;
      type: 'powershell_script';
    };
/**
 * The lifecycle points a Hook can subscribe to (detailed design §24.4).
 *
 * `session.ended` waits for permanent session deletion, which V2 does not have yet, and
 * `model.stream_batch` is for declarative Hooks, which the first version does not have; a
 * manifest naming either is refused with that reason rather than silently never called.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HookEvent".
 */
export type HookEvent =
  | 'session.started'
  | 'session.resumed'
  | 'message.before_accept'
  | 'turn.before_start'
  | 'context.before_build'
  | 'model.before_call'
  | 'model.after_call'
  | 'tool.before_call'
  | 'tool.after_call'
  | 'tool.on_error'
  | 'context.before_compact'
  | 'context.after_compact'
  | 'turn.before_complete'
  | 'turn.completed';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HookFailurePolicy".
 */
export type HookFailurePolicy = 'fail_open' | 'fail_closed';
/**
 * `user` Hooks fail open, `security` Hooks fail closed, unless the manifest says otherwise.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HookKind".
 */
export type HookKind = 'user' | 'security';
/**
 * What the decision did: the action went on, was stopped, or only later Hooks were.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HookEffect".
 */
export type HookEffect = 'none' | 'applied' | 'action_blocked' | 'later_hooks_stopped';
/**
 * Why a registered Hook does or does not run now.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HookEntryState".
 */
export type HookEntryState = ('disabled' | 'not_trusted') | 'ready' | 'content_changed' | 'invalid';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HookFailureKind".
 */
export type HookFailureKind =
  | ('not_started' | 'timed_out' | 'output_limit' | 'exit_code' | 'invalid_result' | 'cancelled')
  | 'content_changed';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HookInvocationId".
 */
export type HookInvocationId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HookOutcome".
 */
export type HookOutcome = 'continued' | 'transformed' | 'blocked' | 'failed';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HookMoveDirection".
 */
export type HookMoveDirection = 'up' | 'down';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HttpBodyKind".
 */
export type HttpBodyKind = 'text' | 'json' | 'binary';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HttpRequestBody".
 */
export type HttpRequestBody =
  | {
      content_type?: string | null;
      kind: 'text';
      text: string;
    }
  | {
      kind: 'json';
      value: any;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ImportDisposition".
 */
export type ImportDisposition =
  'imported' | 'skipped' | 'invalid' | 'requires_user_action' | 'conflict';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MaintenanceResourceState".
 */
export type MaintenanceResourceState = 'pending' | 'applied' | 'removing' | 'removed' | 'retired';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MaintenanceTarget".
 */
export type MaintenanceTarget =
  | {
      kind: 'null';
    }
  | {
      kind: 'ksec';
    }
  | {
      kind: 'owner_namespace';
      logon_id: string;
      session_id: number;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ManagedProcessKindV1".
 */
export type ManagedProcessKindV1 = 'shell' | 'terminal';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ManagedShell".
 */
export type ManagedShell = 'power_shell' | 'cmd';
/**
 * How Aura signs in to an HTTP server. The secret or the tokens are never in this file.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "McpHttpAuth".
 */
export type McpHttpAuth =
  | {
      header?: string | null;
      secret_id: CredentialId;
      type: 'secret';
    }
  | {
      client_id?: string | null;
      scopes?: string[];
      type: 'oauth';
    };
/**
 * How Aura reaches the server.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "McpTransport".
 */
export type McpTransport =
  | {
      arguments?: string[];
      command: string;
      environment?: McpEnvironmentEntry[];
      /**
       * Variables whose values are secrets Aura keeps; their names may look like secrets.
       */
      secret_environment?: McpSecretEnvironment[];
      type: 'stdio';
      working_directory?: string | null;
    }
  | {
      auth?: McpHttpAuth | null;
      headers?: McpHeader[];
      type: 'http';
      url: string;
    };
/**
 * Why a configured server does or does not offer tools now.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "McpServerState".
 */
export type McpServerState =
  | ('not_trusted' | 'ready')
  | 'disabled'
  | 'config_changed'
  | 'invalid'
  | 'starting'
  | 'failed'
  | 'sign_in_required';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MediaHostActionV1".
 */
export type MediaHostActionV1 =
  | {
      kind: 'probe';
    }
  | {
      count?: number | null;
      kind: 'frames';
      max_side: number;
      sheet: boolean;
      times?: number[] | null;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MediaHostCompletedV1".
 */
export type MediaHostCompletedV1 =
  | {
      byte_length: number;
      codec?: string | null;
      container?: string | null;
      duration: number;
      frame_rate?: number | null;
      has_audio?: boolean | null;
      height: number;
      kind: 'probe';
      width: number;
    }
  | {
      frames: MediaHostFrameV1[];
      kind: 'frames';
      sheet?: MediaImageV1 | null;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MediaHostOutcomeV1".
 */
export type MediaHostOutcomeV1 =
  | {
      outcome: 'completed';
      result: MediaHostCompletedV1;
    }
  | {
      message: string;
      outcome: 'refused';
    }
  | {
      message: string;
      outcome: 'failed';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryCandidate".
 */
export type MemoryCandidate =
  | {
      action: 'create';
      content: MemoryCandidateContent;
      path: string;
      scope: MemoryScope;
    }
  | {
      action: 'update';
      content: MemoryCandidateContent;
      expected_hash: string;
      scope: MemoryScope;
      target: MemoryId;
    }
  | {
      action: 'supersede';
      content: MemoryCandidateContent;
      expected_hash: string;
      path: string;
      scope: MemoryScope;
      target: MemoryId;
    }
  | {
      action: 'conflict';
      content: MemoryCandidateContent;
      expected_hash: string;
      path: string;
      scope: MemoryScope;
      target: MemoryId;
    }
  | {
      action: 'ignore';
      basis: string;
      scope: MemoryScope;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryScope".
 */
export type MemoryScope = 'global' | 'project';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryTarget".
 */
export type MemoryTarget =
  | {
      id: MemoryId;
      kind: 'id';
    }
  | {
      kind: 'path';
      path: string;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryMaintenanceState".
 */
export type MemoryMaintenanceState =
  'queued' | 'running' | 'completed' | 'skipped' | 'failed' | 'interrupted';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryStatus".
 */
export type MemoryStatus = 'active' | 'superseded' | 'forgotten' | 'conflicted' | 'archived';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryRelation".
 */
export type MemoryRelation = 'related' | 'supersedes' | 'conflicts';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemorySelector".
 */
export type MemorySelector =
  | {
      id: MemoryId;
      kind: 'id';
    }
  | {
      kind: 'path';
      path: string;
      scope: MemoryScope;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryRepairPhase".
 */
export type MemoryRepairPhase = 'preview' | 'apply';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NetworkSessionConsentAvailability".
 */
export type NetworkSessionConsentAvailability =
  'in_force' | 'revoked' | 'expired' | 'not_yet_valid';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NormalCompletionRecordId".
 */
export type NormalCompletionRecordId = string;
/**
 * A Renderer-safe, stable issue code from Aura's v1 normalized registry.
 *
 * This is deliberately a distinct domain type. Raw provider, Win32, DNS, or
 * TLS diagnostics must not be converted into it or embedded in its suffix.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NormalizedIssueCode".
 */
export type NormalizedIssueCode = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TaskLineageState".
 */
export type TaskLineageState =
  | (
      | 'active'
      | 'suspended_by_topic_switch'
      | 'interrupted_resumable'
      | 'completed'
      | 'cancelled_by_user'
    )
  | 'expired';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "OperationState".
 */
export type OperationState =
  | 'prepared'
  | 'running'
  | 'waiting_external'
  | 'cancel_requested'
  | 'succeeded'
  | 'failed'
  | 'cancelled'
  | 'unknown';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageConformanceReasonV1".
 */
export type PackageConformanceReasonV1 =
  | (
      | 'completed'
      | 'invalid_result'
      | 'invalid_protocol'
      | 'invalid_artifact'
      | 'cancelled'
      | 'timed_out'
      | 'output_limit'
      | 'process_failed'
      | 'transport_unconfirmed'
      | 'cleanup_unconfirmed'
      | 'credential_reflected'
    )
  | 'program_credential_unsupported'
  | 'completed_result_unchecked';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageImportOutcomeV1".
 */
export type PackageImportOutcomeV1 =
  | {
      type: 'cancelled';
    }
  | {
      type: 'imported';
      view: PackageVersionViewV1;
    }
  | {
      diagnostics: PackageImportDiagnosticV1[];
      type: 'rejected';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PermissionModeTransition".
 */
export type PermissionModeTransition = 'settled' | 'stopping' | 'stop_unconfirmed';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PlanStepExecution".
 */
export type PlanStepExecution =
  | {
      execution: 'serial';
    }
  | {
      execution: 'parallel_group';
      group: string;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PluginLayout".
 */
export type PluginLayout = 'aura' | 'claude_code';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PluginState".
 */
export type PluginState =
  'not_trusted' | 'ready' | 'disabled' | 'content_changed' | 'invalid' | 'unavailable';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PluginServerKind".
 */
export type PluginServerKind = 'stdio' | 'http';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PluginSignIn".
 */
export type PluginSignIn = 'secret' | 'oauth';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PolicyAuthorization".
 */
export type PolicyAuthorization =
  | {
      authorization: AuthorizationDecision;
      decision: 'allow';
    }
  | {
      decision: 'ask';
      proposal: AuthorizationProposal;
    }
  | {
      decision: 'deny';
      reason: PolicyDenyReason;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PolicyDenyReason".
 */
export type PolicyDenyReason =
  | 'mode_denied'
  | 'approval_required'
  | 'invalid_operation'
  | 'identity_mismatch'
  | 'source_disabled'
  | 'source_untrusted'
  | 'hard_boundary'
  | 'denied_for_turn'
  | 'network_declaration'
  | 'profile_prohibited'
  | 'backend_unavailable'
  | 'dispatch_already_started';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ReconcileResult".
 */
export type ReconcileResult =
  | {
      handle: ExecutionHandle;
      reconciliation: 'running';
    }
  | {
      reconciliation: 'completed';
      result: ExecutionResult;
    }
  | {
      attempt: AttemptRef;
      reconciliation: 'unknown';
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RunnerCoreMessage".
 */
export type RunnerCoreMessage =
  | {
      message: 'ready';
    }
  | {
      attempt: AttemptRef;
      message: 'started';
      process_id: number;
    }
  | {
      message: 'progress';
      update: RunnerProgress;
    }
  | {
      message: 'completed';
      observation: RunnerObservation;
    }
  | {
      message: 'background';
      update: RunnerBackgroundUpdate;
    }
  | {
      message: 'rejected';
      reason: string;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RunnerStopReason".
 */
export type RunnerStopReason =
  'exited' | 'cancelled' | 'timed_out' | 'output_limit' | 'core_disconnected' | 'unknown';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ShellCallState".
 */
export type ShellCallState =
  'waiting' | 'running' | 'completed' | 'failed' | 'interrupted' | 'indeterminate';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ShellConsentLifetime".
 */
export type ShellConsentLifetime = 'legacy_runtime' | 'session';
/**
 * Where a Skill folder lives.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SkillSource".
 */
export type SkillSource = 'aura_store' | 'agent_global' | 'workspace' | 'plugin';
/**
 * Whether the model may see and load a Skill now, and why not.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SkillState".
 */
export type SkillState =
  ('not_trusted' | 'ready') | 'disabled' | 'content_changed' | 'invalid' | 'unavailable';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SourceMembership".
 */
export type SourceMembership =
  | {
      membership: 'active';
    }
  | {
      decision_event_id: EventId;
      membership: 'reclassified_to_lineage';
      target_lineage_id: TaskLineageId;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TaskLineageRelationKind".
 */
export type TaskLineageRelationKind =
  | 'root'
  | 'continues'
  | 'adds_requirement'
  | 'adds_constraint'
  | 'clarifies'
  | 'corrects'
  | 'narrows'
  | 'defers'
  | 'expands'
  | 'cancels_part'
  | 'cancels_task'
  | 'mixed'
  | 'reclassifies_unrelated_part';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TaskLineageRelationId".
 */
export type TaskLineageRelationId = string;
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TaskLineageReconcileDecision".
 */
export type TaskLineageReconcileDecision =
  | {
      decision: 'continue_existing';
    }
  | {
      decision: 'clarify_existing';
    }
  | {
      decision: 'modify_existing';
    }
  | {
      decision: 'expand_existing';
    }
  | {
      decision: 'narrow_existing';
    }
  | {
      decision: 'defer_part';
    }
  | {
      decision: 'cancel_part';
    }
  | {
      decision: 'mixed';
    }
  | {
      decision: 'suspend_and_start_new';
    }
  | {
      decision: 'resume_suspended';
      lineage_id: TaskLineageId;
    }
  | {
      decision: 'cancel_existing';
    }
  | {
      decision: 'expire_prior';
      lineage_id: TaskLineageId;
    }
  | {
      decision: 'reference_prior';
      lineage_id: TaskLineageId;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TaskLineageReconcileToolDecision".
 */
export type TaskLineageReconcileToolDecision =
  | {
      decision: 'continue_existing';
    }
  | {
      decision: 'clarify_existing';
    }
  | {
      decision: 'modify_existing';
    }
  | {
      decision: 'expand_existing';
    }
  | {
      decision: 'narrow_existing';
    }
  | {
      decision: 'defer_part';
    }
  | {
      decision: 'cancel_part';
    }
  | {
      decision: 'mixed';
    }
  | {
      decision: 'cancel_existing';
    }
  | {
      decision: 'expire_prior';
      lineage_id: TaskLineageId;
    }
  | {
      decision: 'reference_prior';
      lineage_id: TaskLineageId;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TerminalControllerV1".
 */
export type TerminalControllerV1 = 'user' | 'agent';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TerminalOwnerV1".
 */
export type TerminalOwnerV1 = 'user' | 'agent';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TerminalOpeningPhaseV1".
 */
export type TerminalOpeningPhaseV1 =
  'waiting_runtime' | 'waiting_launch' | 'launching' | 'cancelled';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TerminalSensitiveStatusV1".
 */
export type TerminalSensitiveStatusV1 = 'completed' | 'cancelled' | 'failed';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolBrokerAction".
 */
export type ToolBrokerAction =
  | {
      kind: 'search';
      request: ToolSearchRequestV1;
    }
  | {
      kind: 'describe';
      request: ToolDescribeRequestV1;
    }
  | {
      kind: 'list_loaded';
    }
  | {
      kind: 'load';
      request: ToolLoadRequestV1;
    }
  | {
      kind: 'unload';
      request: ToolLoadRequestV1;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCredentialScopeV1".
 */
export type ToolCredentialScopeV1 =
  | {
      scope: 'user';
    }
  | {
      scope: 'workspace';
      workspace_id: WorkspaceId;
    }
  | {
      scope: 'session';
      session_id: SessionId;
      workspace_id: WorkspaceId;
    };
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCredentialUsagePolicyV1".
 */
export type ToolCredentialUsagePolicyV1 = {
  policy: 'explicit';
  tool_ids: ToolId[];
  tool_package_ids: ToolPackageId[];
};
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCredentialMutationDispositionV1".
 */
export type ToolCredentialMutationDispositionV1 = 'committed' | 'aborted';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCredentialOutcomeStateV1".
 */
export type ToolCredentialOutcomeStateV1 = 'pending' | 'committed' | 'aborted' | 'not_obtained';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WebFetchMode".
 */
export type WebFetchMode = 'auto' | 'http_only' | 'headless_render_required';
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceDirectorySelection".
 */
export type WorkspaceDirectorySelection =
  | {
      status: 'cancelled';
    }
  | {
      selected_root: string;
      status: 'selected';
      suggested_display_name: string;
    };
/**
 * Where a workspace file opens outside Aura.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceFileOpener".
 */
export type WorkspaceFileOpener = 'default_application' | 'file_manager';

export interface AuraContractCatalog {
  component_manifest: BuildComponentManifest;
  content_hash: ContentHash;
  context: ContextContractFixture;
  contract_version: ContractVersion;
  error: ErrorEnvelope;
  expected_revision: ExpectedRevision;
  ids: CommonIdFixture;
  models: ModelContractFixture;
  persistence: PersistenceContractFixture;
  projection_contract: ProjectionContractSchemaRoot;
  revision: Revision;
  runtime: RuntimeContractFixture;
  security: SecurityContractFixture;
  timestamp: UtcTimestamp;
  tools: ToolContractFixture;
  windows_path: string;
  workspace_files: WorkspaceFilesContractFixture;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BuildComponentManifest".
 */
export interface BuildComponentManifest {
  build_commit: string;
  bundle_namespace: string;
  components: BuildComponent[];
  manifest_version: ContractVersion;
  product_version: string;
  release_bundle_id: ReleaseBundleId;
  release_channel: ReleaseChannel;
  target_triple: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BuildComponent".
 */
export interface BuildComponent {
  component_id: string;
  content_hash: ContentHash;
  kind: BuildComponentKind;
  protocol_version?: ContractVersion | null;
  relative_path: string;
  version: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContentHash".
 */
export interface ContentHash {
  algorithm: ContentHashAlgorithm;
  hex: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContractVersion".
 */
export interface ContractVersion {
  major: number;
  minor: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextContractFixture".
 */
export interface ContextContractFixture {
  budget_snapshot?: ContextBudgetSnapshot | null;
  compaction?: StructuredCompactionRecord | null;
  compaction_anchor_manifest?: CompactionAnchorManifest | null;
  compaction_model_attempt?: CompactionModelAttemptRecord | null;
  context_projection_snapshot?: ContextProjectionSnapshot | null;
  context_read_events_input?: ContextReadEventsInput | null;
  context_read_tool_result_input?: ContextReadToolResultInput | null;
  context_search_input?: ContextSearchInput | null;
  history_atomic_group_preimage?: HistoryAtomicGroupPreimageV1 | null;
  history_atomic_group_ref?: HistoryAtomicGroupRefV1 | null;
  manual_compaction_request?: ManualCompactionRequestRecord | null;
  model_egress_manifest?: ModelEgressManifestV1 | null;
  p07_context_effective_tool_bindings: EffectiveToolBindingSnapshot[];
  p07_context_tool_schema_set: ToolSchemaSetSnapshotV1;
  p08_active_task_lineage?: PromptInspectorTaskLineageView | null;
  preparation_outcome?: ContextPreparationOutcome | null;
  preparation_trigger?: ContextPreparationTriggerV1 | null;
  prompt_bundle_snapshot?: PromptBundleSnapshot | null;
  prompt_inspector_projection?: PromptInspectorProjection | null;
  prompt_settings_snapshot: PromptSettingsSnapshot;
  provider_continuation_binding?: ProviderContinuationBindingV1 | null;
  tool_result_context_projection?: ToolResultContextProjectionV1 | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextBudgetSnapshot".
 */
export interface ContextBudgetSnapshot {
  effective_max_output_tokens: number;
  estimated_input_tokens: number;
  item_estimates: ContextItemEstimate[];
  output_reserve_tokens: number;
  policy: string;
  request_input_budget_tokens: number;
  runtime_effective_window_tokens: number;
  safety_reserve_tokens: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextItemEstimate".
 */
export interface ContextItemEstimate {
  byte_length: number;
  content_hash: ContentHash;
  estimated_tokens: number;
  estimator: TokenEstimatorIdentity;
  ordinal: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TokenEstimatorIdentity".
 */
export interface TokenEstimatorIdentity {
  estimator_id: string;
  estimator_version: number;
  exactness: TokenEstimateExactness;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "StructuredCompactionRecord".
 */
export interface StructuredCompactionRecord {
  active_turn_summary_hash: ContentHash;
  anchor_manifest_hash: ContentHash;
  historical_summary_hash: ContentHash;
  retained_recent_tokens: number;
  retention_exceptions: ContextRetentionException[];
  sequence: number;
  source_context_snapshot_id: ContextProjectionSnapshotId;
  source_span: CompactionSourceSpan;
  structured_markdown_hash: ContentHash;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CompactionSourceSpan".
 */
export interface CompactionSourceSpan {
  end_sequence: SessionSequence;
  source_refs: ContextSourceRef[];
  start_sequence: SessionSequence;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolVersion".
 */
export interface ToolVersion {
  definition_revision: Revision;
  semantic_version?: CanonicalSemVer | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CompactionAnchorManifest".
 */
export interface CompactionAnchorManifest {
  anchors: CompactionAnchor[];
  manifest_hash: ContentHash;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CompactionAnchor".
 */
export interface CompactionAnchor {
  anchor: string;
  kind: CompactionAnchorKind;
  preview: string;
  source_hash: ContentHash;
  source_ref: ContextSourceRef;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CompactionModelAttemptRecord".
 */
export interface CompactionModelAttemptRecord {
  attempt_context_snapshot_id: ContextProjectionSnapshotId;
  attempt_ordinal: number;
  compaction_sequence: number;
  error_code?: string | null;
  model_execution_snapshot_id: ModelExecutionSnapshotId;
  model_step_id: ModelStepId;
  source_context_snapshot_id: ContextProjectionSnapshotId;
  started_at: UtcTimestamp;
  state: CompactionModelAttemptState;
  terminal_at?: UtcTimestamp | null;
  transport_attempt_id?: TransportAttemptId | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextProjectionSnapshot".
 */
export interface ContextProjectionSnapshot {
  active_prompt_module_ids: string[];
  active_task_lineage_id?: TaskLineageId | null;
  agent_run_id: AgentRunId;
  attachments: ModelAttachmentSnapshot[];
  budget: ContextBudgetSnapshot;
  context_usage: ContextUsageSummaryView;
  conversation_items: LogicalConversationItem[];
  created_at: UtcTimestamp;
  effective_tool_bindings: EffectiveToolBindingSnapshot[];
  history_cutoff: SessionSequence;
  id: ContextProjectionSnapshotId;
  instruction_envelope_artifact_id: ArtifactId;
  instruction_envelope_hash: ContentHash;
  model_egress_manifest_artifact_id: ArtifactId;
  model_egress_manifest_hash: ContentHash;
  model_egress_policy_version: number;
  model_profile_id: ModelProfileId;
  model_step_id: ModelStepId;
  output_request: ModelOutputRequest;
  prompt_bundle_snapshot_id: PromptBundleSnapshotId;
  sections: ContextSectionSnapshot[];
  session_id: SessionId;
  snapshot_hash: ContentHash;
  snapshot_version: number;
  tool_schema_set: ToolSchemaSetSnapshotV1;
  turn_task_id: TurnTaskId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelAttachmentSnapshot".
 */
export interface ModelAttachmentSnapshot {
  artifact_id: ArtifactId;
  byte_length: number;
  content_hash: ContentHash;
  display_name: string;
  file_reference_id?: FileReferenceId | null;
  media_type: string;
  source_event_id: EventId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextUsageSummaryView".
 */
export interface ContextUsageSummaryView {
  estimated: boolean;
  runtime_effective_window_tokens: number;
  segments: ContextUsageSegmentView[];
  used_input_tokens: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextUsageSegmentView".
 */
export interface ContextUsageSegmentView {
  estimated_tokens: number;
  kind: ContextUsageKind;
  label: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "LogicalProviderHistory".
 */
export interface LogicalProviderHistory {
  data: ProviderHistoryData;
  model_name: string;
  reuse_domain: ProviderToolCallReuseDomain;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderToolCallReuseDomain".
 */
export interface ProviderToolCallReuseDomain {
  api_format: ModelApiFormat;
  endpoint_id: EndpointId;
  endpoint_security_fingerprint: ContentHash;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "LogicalHistoricalToolCall".
 */
export interface LogicalHistoricalToolCall {
  arguments_hash: ContentHash;
  arguments_json: string;
  canonical_name: string;
  tool_call_id: ToolCallId;
  /**
   * Null only for a call whose Provider name was absent from the frozen request.
   */
  tool_id: ToolId | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "EffectiveToolBindingSnapshot".
 */
export interface EffectiveToolBindingSnapshot {
  definition_fingerprint: CanonicalSha256HashV1;
  exposure: ToolExposureV1;
  source_contract_fingerprint: ToolSourceContractFingerprintV1;
  source_kind: ToolSourceKindV1;
  tool_id: ToolId;
  tool_instance_id: ToolInstanceId;
  tool_version: ToolVersion;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CanonicalSha256HashV1".
 */
export interface CanonicalSha256HashV1 {
  algorithm: 'sha256';
  canonicalization_version: 'rfc8785_jcs_v1';
  hex: string;
}
/**
 * Installation-independent hash of one Package-authored Tool contract. Under
 * V1 `full_package`, every Tool fingerprint references the same authoritative
 * `PackageRuntimeClosureHashV1` produced by the W2 staging evidence chain.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolContractFingerprintV1".
 */
export interface ToolContractFingerprintV1 {
  fingerprint: CanonicalSha256HashV1;
  package_runtime_closure_hash: CanonicalSha256HashV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelOutputRequest".
 */
export interface ModelOutputRequest {
  effective_max_output_tokens: number;
  format: ModelOutputFormat;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextSectionSnapshot".
 */
export interface ContextSectionSnapshot {
  byte_count: number;
  character_count: number;
  compressibility: ContextCompressibility;
  content_artifact_id: ArtifactId;
  content_hash: ContentHash;
  estimated_tokens: number;
  is_empty_marker: boolean;
  kind: ContextSectionKind;
  physical_ordinal: number;
  privacy_class: ContextPrivacyClass;
  source_refs: ContextSourceRef[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolSchemaSetSnapshotV1".
 */
export interface ToolSchemaSetSnapshotV1 {
  registry_snapshot_ref: ToolRegistrySnapshotRef;
  tool_set_hash: CanonicalSha256HashV1;
  tools: ToolSchemaSnapshotV1[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolRegistrySnapshotRef".
 */
export interface ToolRegistrySnapshotRef {
  content_hash: CanonicalSha256HashV1;
  registry_revision: Revision;
  tool_registry_snapshot_id: ToolRegistrySnapshotId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolSchemaSnapshotV1".
 */
export interface ToolSchemaSnapshotV1 {
  canonical_name: ToolCanonicalName;
  description: string;
  input_schema: any;
  schema_dialect: ToolSchemaDialect;
  schema_hash: CanonicalSha256HashV1;
  schema_policy_version: number;
  strict: boolean;
  tool_id: ToolId;
  tool_schema_snapshot_hash: CanonicalSha256HashV1;
  tool_version: ToolVersion;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextReadEventsInput".
 */
export interface ContextReadEventsInput {
  agent_run_id: AgentRunId;
  character_limit: number;
  cursor?: string | null;
  selectors: ContextEventSelector[];
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HistoryAtomicGroupRefV1".
 */
export interface HistoryAtomicGroupRefV1 {
  assistant_response_event_id: EventId;
  group_hash: CanonicalSha256HashV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextReadToolResultInput".
 */
export interface ContextReadToolResultInput {
  agent_run_id: AgentRunId;
  cursor?: string | null;
  include_input: boolean;
  session_id: SessionId;
  tool_call_id: ToolCallId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextSearchInput".
 */
export interface ContextSearchInput {
  agent_run_id: AgentRunId;
  cursor?: string | null;
  filters: ContextSearchFilters;
  limit: number;
  mode: ContextSearchMode;
  query: string;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextSearchFilters".
 */
export interface ContextSearchFilters {
  agent_run_id?: AgentRunId | null;
  include_reasoning: boolean;
  source_types: string[];
  turn_task_id?: TurnTaskId | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HistoryAtomicGroupPreimageV1".
 */
export interface HistoryAtomicGroupPreimageV1 {
  agent_run_id: AgentRunId;
  assistant_response_content_hash: CanonicalSha256HashV1;
  assistant_response_event_id: EventId;
  batch_summary_hash: CanonicalSha256HashV1;
  calls: HistoryToolCallResultEntryV1[];
  kind: HistoryAtomicGroupKindV1;
  model_step_id: ModelStepId;
  preimage_version: ToolWireVersionV1;
  session_id: SessionId;
  tool_batch_id: ToolBatchId;
  turn_task_id: TurnTaskId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HistoryToolCallResultEntryV1".
 */
export interface HistoryToolCallResultEntryV1 {
  ordinal: number;
  tool_call_fact_hash: CanonicalSha256HashV1;
  tool_call_id: ToolCallId;
  tool_result_fact_hash: CanonicalSha256HashV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ManualCompactionRequestRecord".
 */
export interface ManualCompactionRequestRecord {
  agent_run_id: AgentRunId;
  command_event_id: EventId;
  command_invocation_id: CommandInvocationId;
  compaction_sequence?: number | null;
  created_at: UtcTimestamp;
  error_code?: string | null;
  focus_artifact_id?: ArtifactId | null;
  focus_hash?: ContentHash | null;
  session_id: SessionId;
  source_context_snapshot_id?: ContextProjectionSnapshotId | null;
  state: ManualCompactionRequestState;
  terminal_at?: UtcTimestamp | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelEgressManifestV1".
 */
export interface ModelEgressManifestV1 {
  context_snapshot_id: ContextProjectionSnapshotId;
  items: ModelEgressItem[];
  manifest_hash: ContentHash;
  policy_version: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelEgressItem".
 */
export interface ModelEgressItem {
  authorization_basis: ModelEgressAuthorizationBasis;
  byte_length: number;
  content_hash: ContentHash;
  data_class: NetworkDataClass;
  ordinal: number;
  source_kind: ModelEgressSourceKind;
  source_ref: ContextSourceRef;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SourceAuthorizationBinding".
 */
export interface SourceAuthorizationBinding {
  authorization: SourceAuthorizationRef;
  authorization_revision: Revision;
  binding_fingerprint: ContentHash;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PromptInspectorTaskLineageView".
 */
export interface PromptInspectorTaskLineageView {
  completion_gate: string;
  completion_policy: string;
  lineage_id: TaskLineageId;
  preview_characters: number;
  preview_estimated_tokens: number;
  revision: Revision;
  source_count: number;
  source_recheck_required: boolean;
  state: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PreparedLogicalContext".
 */
export interface PreparedLogicalContext {
  attachments: ModelAttachmentSnapshot[];
  context_snapshot_id: ContextProjectionSnapshotId;
  conversation_items: LogicalConversationItem[];
  instruction_blocks: LogicalInstructionBlock[];
  instruction_envelope: CompiledInstructionEnvelope;
  model_egress_manifest: ModelEgressManifestV1;
  model_step_id: ModelStepId;
  output_request: ModelOutputRequest;
  prompt_bundle_snapshot_id: PromptBundleSnapshotId;
  provider_continuation?: ProviderContinuationState | null;
  snapshot: ContextProjectionSnapshot;
  tool_schema_set: ToolSchemaSetSnapshotV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "LogicalInstructionBlock".
 */
export interface LogicalInstructionBlock {
  content: string;
  content_hash: ContentHash;
  kind: LogicalInstructionKind;
  source_event_ids: EventId[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CompiledInstructionEnvelope".
 */
export interface CompiledInstructionEnvelope {
  canonical_text: string;
  instruction_hash: ContentHash;
  ordered_blocks: LogicalInstructionBlock[];
  serializer_version: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextRemediationPlan".
 */
export interface ContextRemediationPlan {
  actions: string[];
  overflow: ContextOverflowDiagnosticView;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextOverflowDiagnosticView".
 */
export interface ContextOverflowDiagnosticView {
  compressible_history_tokens: number;
  error_code: string;
  estimated_input_tokens: number;
  non_compressible_tokens: number;
  overflow_tokens: number;
  remediation: string[];
  request_input_budget_tokens: number;
  user_reducible_tokens: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextPreparationFailure".
 */
export interface ContextPreparationFailure {
  code: string;
  retryable: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderContinuationBindingV1".
 */
export interface ProviderContinuationBindingV1 {
  api_format: ModelApiFormat;
  assistant_response_event_id: EventId;
  endpoint_fingerprint: ContentHash;
  endpoint_id: EndpointId;
  model_profile_id: ModelProfileId;
  previous_model_execution_id: ModelExecutionSnapshotId;
  provider_id: ProviderId;
  provider_response_ref: ArtifactId;
  request_evidence_hash: CanonicalSha256HashV1;
  response_evidence_hash: CanonicalSha256HashV1;
  state: ProviderContinuationState;
  tool_batch_id: ToolBatchId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PromptBundleSnapshot".
 */
export interface PromptBundleSnapshot {
  bundle_manifest_hash: ContentHash;
  bundle_version: CanonicalSemVer;
  created_at: UtcTimestamp;
  custom_addendum_artifact_id?: ArtifactId | null;
  custom_addendum_hash?: ContentHash | null;
  id: PromptBundleSnapshotId;
  module_snapshots: PromptModuleSnapshot[];
  release_identity: string;
  serializer_version: number;
  settings_revision: Revision;
  turn_task_id: TurnTaskId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PromptModuleSnapshot".
 */
export interface PromptModuleSnapshot {
  activation: PromptActivationCondition;
  content_artifact_id: ArtifactId;
  content_hash: ContentHash;
  module_id: string;
  module_version: CanonicalSemVer;
  order: number;
  section: PromptSectionClass;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PromptInspectorProjection".
 */
export interface PromptInspectorProjection {
  active_agent_run_id?: AgentRunId | null;
  active_model_step_id?: ModelStepId | null;
  active_task_lineage?: PromptInspectorTaskLineageView | null;
  active_tool_call_ids: ToolCallId[];
  /**
   * The frozen run-information activity projection for this step.
   *
   * It is reported from the **same** frozen snapshot the activity fragment was built
   * from, so the Inspector can never describe a picture different from the one that was
   * actually sent. It is `None` when this step had no activity fragment at all.
   */
  activity?: PromptInspectorActivityView | null;
  attachments: PromptInspectorAttachmentView[];
  budget?: PromptInspectorBudgetView | null;
  cache_usage: PromptInspectorCacheUsageView;
  compaction: PromptInspectorCompactionView;
  context_snapshot_id?: ContextProjectionSnapshotId | null;
  context_usage?: ContextUsageSummaryView | null;
  model_egress?: PromptInspectorModelEgressView | null;
  model_profile_id?: ModelProfileId | null;
  overflow?: ContextOverflowDiagnosticView | null;
  prompt_bundle_snapshot_id?: PromptBundleSnapshotId | null;
  /**
   * Set on the step whose preparation cleared older tool results or
   * compacted history before its request went out.
   */
  reduction?: PromptInspectorReductionView | null;
  revision: Revision;
  sections: PromptInspectorSectionView[];
  session_id: SessionId;
  skills: PromptInspectorSkillView[];
  tools: PromptInspectorToolView[];
}
/**
 * The Inspector's real projection of the frozen activity snapshot.
 *
 * Every field is a fact the fragment was built from; nothing here is recomputed from a
 * second read, so "the Inspector and the request body agree" is structural rather than a
 * property someone has to remember to maintain.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PromptInspectorActivityView".
 */
export interface PromptInspectorActivityView {
  body_hash: ContentHash1;
  /**
   * Why the index is reporting less than everything.
   */
  coverage: 'complete' | 'partial' | 'unavailable';
  /**
   * Whether an activity fragment was placed in the request at all.
   */
  fragment_present: boolean;
  /**
   * Rows omitted for any reason, summed across the coverage counters.
   */
  omitted: number;
  /**
   * The index revision at that instant.
   */
  revision: string;
  /**
   * Rows in the reader's scope before any cap.
   */
  rows_in_scope: number;
  /**
   * Rows the model actually saw.
   */
  rows_shown: number;
  /**
   * The runtime the snapshot came from.
   */
  runtime_instance_id: string;
  /**
   * The instant the index was sampled.
   */
  sampled_at: string;
  /**
   * True when a body was cut to fit its byte bound.
   */
  truncated: boolean;
}
/**
 * The content hash of the exact body that became the fragment. It is what makes "the
 * Inspector shows the bytes that were sent" checkable rather than asserted.
 */
export interface ContentHash1 {
  algorithm: ContentHashAlgorithm;
  hex: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PromptInspectorAttachmentView".
 */
export interface PromptInspectorAttachmentView {
  byte_length: number;
  content_hash: ContentHash;
  display_name: string;
  estimated_tokens?: number | null;
  media_type: string;
  resolved: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PromptInspectorBudgetView".
 */
export interface PromptInspectorBudgetView {
  estimated_input_tokens: number;
  estimator_id: string;
  estimator_version: number;
  exact: boolean;
  output_reserve_tokens: number;
  policy: string;
  request_input_budget_tokens: number;
  runtime_effective_window_tokens: number;
  safety_reserve_tokens: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PromptInspectorCacheUsageView".
 */
export interface PromptInspectorCacheUsageView {
  actual_cached_input_tokens?: number | null;
  actual_input_tokens?: number | null;
  stable_prefix_estimate_tokens: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PromptInspectorCompactionView".
 */
export interface PromptInspectorCompactionView {
  retained_recent_tokens: number;
  retention_exceptions: ContextRetentionException[];
  source_context_snapshot_id?: ContextProjectionSnapshotId | null;
  state: ContextCompactionStateView;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PromptInspectorModelEgressView".
 */
export interface PromptInspectorModelEgressView {
  attachments_resolved: boolean;
  classes: PromptInspectorEgressClassView[];
  manifest_hash: ContentHash;
  policy_version: number;
  unresolved_source_kinds: string[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PromptInspectorEgressClassView".
 */
export interface PromptInspectorEgressClassView {
  byte_length: number;
  data_class: NetworkDataClass;
  disposition: ModelChannelDataDisposition;
  item_count: number;
}
/**
 * What one
 * step's preparation gave back - the request's estimated input when it first did not fit (or
 * when the user asked for a compaction), how many older tool results were cleared, and whether
 * history was compacted. The size after is the step's own context meter.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PromptInspectorReductionView".
 */
export interface PromptInspectorReductionView {
  cleared_tool_results: number;
  compacted: boolean;
  input_tokens_before: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PromptInspectorSectionView".
 */
export interface PromptInspectorSectionView {
  byte_count: number;
  changed_from_previous_step: boolean;
  character_count: number;
  compressibility: ContextCompressibility;
  content_hash: ContentHash;
  estimated_tokens: number;
  is_empty_marker: boolean;
  kind: ContextSectionKind;
  physical_ordinal: number;
  placement: PromptInspectorSectionPlacement;
  privacy_class: ContextPrivacyClass;
  source_kinds: string[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PromptInspectorSkillView".
 */
export interface PromptInspectorSkillView {
  content_hash: ContentHash;
  estimated_tokens: number;
  skill_instance_id: SkillInstanceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PromptInspectorToolView".
 */
export interface PromptInspectorToolView {
  canonical_name: string;
  estimated_tokens: number;
  schema_hash: CanonicalSha256HashV1;
  tool_id: ToolId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PromptSettingsSnapshot".
 */
export interface PromptSettingsSnapshot {
  custom_system_addendum?: string | null;
  revision: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolResultContextProjectionV1".
 */
export interface ToolResultContextProjectionV1 {
  full_byte_length: number;
  full_content_hash: ContentHash;
  full_result_ref: ArtifactId;
  preview: string;
  preview_truncated: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ErrorEnvelope".
 */
export interface ErrorEnvelope {
  category: ErrorCategory;
  causation_id?: CausationId | null;
  code: ErrorCode;
  contract_version: ContractVersion;
  correlation_id: CorrelationId;
  details: {
    [k: string]: any | undefined;
  };
  message: string;
  retryable: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CommonIdFixture".
 */
export interface CommonIdFixture {
  agent_run_id: AgentRunId;
  artifact_id: ArtifactId;
  attachment_grant_id: AttachmentGrantId;
  browser_share_id: BrowserShareId;
  causation_id: CausationId;
  command_id: CommandId;
  command_invocation_id: CommandInvocationId;
  context_projection_snapshot_id: ContextProjectionSnapshotId;
  correlation_id: CorrelationId;
  credential_id: CredentialId;
  deferred_mutation_id: DeferredMutationId;
  delete_confirmation_id: DeleteConfirmationId;
  editor_reservation_id: EditorReservationId;
  endpoint_id: EndpointId;
  event_id: EventId;
  external_grant_id: ExternalGrantId;
  file_reference_id: FileReferenceId;
  ipc_channel_id: IpcChannelId;
  memory_id: MemoryId;
  model_availability_test_id: ModelAvailabilityTestId;
  model_content_block_id: ModelContentBlockId;
  model_discovery_run_id: ModelDiscoveryRunId;
  model_execution_snapshot_id: ModelExecutionSnapshotId;
  model_failure_id: ModelFailureId;
  model_profile_id: ModelProfileId;
  model_step_id: ModelStepId;
  mutation_journal_id: MutationJournalId;
  prompt_bundle_snapshot_id: PromptBundleSnapshotId;
  provider_id: ProviderId;
  release_bundle_id: ReleaseBundleId;
  renderer_instance_id: RendererInstanceId;
  runtime_instance_id: RuntimeInstanceId;
  session_id: SessionId;
  skill_instance_id: SkillInstanceId;
  stream_id: StreamId;
  task_lineage_id: TaskLineageId;
  tool_batch_id: ToolBatchId;
  tool_call_id: ToolCallId;
  tool_conformance_run_id: ToolConformanceRunId;
  tool_draft_id: ToolDraftId;
  tool_execution_attempt_id: ToolExecutionAttemptId;
  tool_id: ToolId;
  tool_instance_id: ToolInstanceId;
  tool_package_id: ToolPackageId;
  tool_registry_snapshot_id: ToolRegistrySnapshotId;
  transport_attempt_id: TransportAttemptId;
  turn_task_id: TurnTaskId;
  workspace_id: WorkspaceId;
  workspace_lifecycle_operation_id: WorkspaceLifecycleOperationId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelContractFixture".
 */
export interface ModelContractFixture {
  availability_prerequisite_failure?: ModelAvailabilityPrerequisiteFailure | null;
  availability_test_cancel_request?: ModelAvailabilityTestCancelRequest | null;
  availability_test_cancel_result?: ModelAvailabilityTestCancelResult | null;
  availability_test_start_accepted?: ModelAvailabilityTestStartAccepted | null;
  availability_test_start_request?: ModelAvailabilityTestStartRequest | null;
  catalog_ensure_fresh_request?: ModelCatalogEnsureFreshRequest | null;
  catalog_get_request?: ModelCatalogGetRequest | null;
  catalog_provider_get_request?: ModelCatalogProviderGetRequest | null;
  catalog_provider_get_result?: ProviderCatalogModelPage | null;
  catalog_refresh_request?: ModelCatalogRefreshRequest | null;
  clear_credential_request?: ModelEndpointClearCredentialRequest | null;
  compatibility_get_request?: ModelCompatibilityGetRequest | null;
  compatibility_reset_request?: ModelCompatibilityResetRequest | null;
  compatibility_reset_result?: ModelCompatibilityResetResult | null;
  compatibility_snapshot?: EndpointCompatibilitySnapshot | null;
  configure_credential_request?: ModelEndpointConfigureCredentialRequest | null;
  context_window_clear_user_override_request?: ContextWindowClearUserOverrideRequest | null;
  context_window_correct_request?: ContextWindowCorrectRequest | null;
  context_window_mutation_result?: ContextWindowMutationResult | null;
  context_window_reset_runtime_correction_request?: ContextWindowResetRuntimeCorrectionRequest | null;
  credential_cleared_event?: ModelEndpointCredentialClearedPayload | null;
  credential_configured_event?: ModelEndpointCredentialConfiguredPayload | null;
  credential_mutation_result?: EndpointCredentialMutationResult | null;
  /**
   * Secret-generation revision for one stable CredentialId.
   *
   * This is intentionally distinct from a row revision. Clear and metadata-only
   * mutations may change row concurrency without creating a new secret
   * generation.
   */
  credential_revision: string;
  default_mutation_result?: DefaultModelMutationResult | null;
  default_set_request?: ModelDefaultSetRequest | null;
  deletion_mutation_result?: ModelDeletionMutationResult | null;
  discovery_cancel_request?: ModelDiscoveryCancelRequest | null;
  discovery_cancel_result?: ModelDiscoveryCancelResult | null;
  discovery_start_accepted?: ModelDiscoveryStartAccepted | null;
  discovery_start_request?: ModelDiscoveryStartRequest | null;
  dormant_credential_summary?: DormantEndpointCredentialSummary | null;
  endpoint_create_request?: ModelEndpointCreateRequest | null;
  endpoint_create_with_credential_request?: ModelEndpointCreateWithCredentialRequest | null;
  endpoint_create_with_credential_result?: EndpointCreatedWithCredentialResult | null;
  endpoint_delete_request?: ModelEndpointDeleteRequest | null;
  endpoint_mutation_result?: ModelEndpointMutationResult | null;
  endpoint_replace_credential_request?: ModelEndpointReplaceCredentialRequest | null;
  endpoint_update_request?: ModelEndpointUpdateRequest | null;
  execution_snapshot?: ModelExecutionSnapshot | null;
  failure?: NormalizedModelFailure | null;
  logical_request?: LogicalModelRequest | null;
  output?: NormalizedModelOutput | null;
  persisted_failure?: PersistedModelFailure | null;
  profile_apply_catalog_update_request?: ModelProfileApplyCatalogUpdateRequest | null;
  profile_create_from_catalog_request?: ModelProfileCreateFromCatalogRequest | null;
  profile_create_manual_request?: ModelProfileCreateManualRequest | null;
  profile_delete_request?: ModelProfileDeleteRequest | null;
  profile_mutation_result?: ModelProfileMutationResult | null;
  profile_set_subagent_availability_request?: ModelProfileSetSubagentAvailabilityRequest | null;
  profile_update_request?: ModelProfileUpdateRequest | null;
  provider_bulk_availability_result?: ProviderBulkAvailabilityResult | null;
  provider_create_request?: ModelProviderCreateRequest | null;
  provider_delete_request?: ModelProviderDeleteRequest | null;
  provider_icon_read_request?: ProviderIconReadRequest | null;
  provider_icon_read_result?: ProviderIconReadResult | null;
  provider_mutation_result?: ModelProviderMutationResult | null;
  provider_rename_request?: ModelProviderRenameRequest | null;
  provider_request_evidence?: ProviderRequestEvidence | null;
  provider_set_all_current_models_subagent_availability_request?: ModelProviderSetAllCurrentModelsSubagentAvailabilityRequest | null;
  provider_tool_call_id?: ProviderToolCallId | null;
  provider_tool_call_protocol_binding?: ProviderToolCallProtocolBindingRecord | null;
  registry?: ModelRegistryConfig | null;
  runtime_capabilities?: ModelRuntimeCapabilityFacts | null;
  session_binding_mutation_result?: SessionModelBindingMutationResult | null;
  session_set_binding_request?: SessionModelBindingSetRequest | null;
  settings_get_snapshot_request?: ModelSettingsGetSnapshotRequest | null;
  stream_event?: ModelStreamEventEnvelope | null;
  tool_versions: ToolVersion[];
  transport_attempt?: TransportAttemptRecord | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelAvailabilityTestCancelRequest".
 */
export interface ModelAvailabilityTestCancelRequest {
  expected_operational_revision: ExpectedRevision;
  model_availability_test_id: ModelAvailabilityTestId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelAvailabilityTestCancelResult".
 */
export interface ModelAvailabilityTestCancelResult {
  model_availability_test_id: ModelAvailabilityTestId;
  operational_revision: Revision;
  state: ModelAvailabilityTestState;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelAvailabilityTestStartAccepted".
 */
export interface ModelAvailabilityTestStartAccepted {
  model_availability_test_id: ModelAvailabilityTestId;
  operational_revision: Revision;
  state: ModelAvailabilityTestState;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelAvailabilityTestStartRequest".
 */
export interface ModelAvailabilityTestStartRequest {
  expected_endpoint_revision: ExpectedRevision;
  expected_model_profile_revision: ExpectedRevision;
  model_profile_id: ModelProfileId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelCatalogEnsureFreshRequest".
 */
export interface ModelCatalogEnsureFreshRequest {}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelCatalogGetRequest".
 */
export interface ModelCatalogGetRequest {}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelCatalogProviderGetRequest".
 */
export interface ModelCatalogProviderGetRequest {
  after_model_id?: string | null;
  catalog_provider_id: string;
  expected_catalog_revision?: Revision | null;
  limit?: number | null;
  query?: string | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderCatalogModelPage".
 */
export interface ProviderCatalogModelPage {
  catalog_content_hash: ContentHash;
  catalog_revision: Revision;
  models: ProviderCatalogModelView[];
  next_model_id?: string | null;
  provider: ProviderCatalogSummaryView;
}
/**
 * Renderer-safe catalog item paired with the immutable object hash required
 * by catalog-backed Profile create/update CAS commands.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderCatalogModelView".
 */
export interface ProviderCatalogModelView {
  catalog_object_hash: ContentHash;
  model: ProviderCatalogModelEntry;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderCatalogModelEntry".
 */
export interface ProviderCatalogModelEntry {
  aliases: string[];
  capabilities: ProviderCatalogModelCapabilities;
  catalog_model_id: string;
  context_window_tokens?: number | null;
  description?: string | null;
  display_name: string;
  family?: string | null;
  input_limit_tokens?: number | null;
  input_modalities: string[];
  knowledge_cutoff?: string | null;
  last_updated?: string | null;
  open_weights?: boolean | null;
  output_limit_tokens?: number | null;
  output_modalities: string[];
  price?: ProviderCatalogPrice | null;
  reasoning_options: string[];
  release_date?: string | null;
  status?: string | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderCatalogModelCapabilities".
 */
export interface ProviderCatalogModelCapabilities {
  attachment: boolean;
  interleaved?: boolean | null;
  reasoning: boolean;
  structured_output: boolean;
  temperature: boolean;
  tool_call: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderCatalogPrice".
 */
export interface ProviderCatalogPrice {
  cache_read?: CatalogDecimal | null;
  cache_write?: CatalogDecimal | null;
  currency: CatalogPriceCurrency;
  input?: CatalogDecimal | null;
  output?: CatalogDecimal | null;
  unit: CatalogPriceUnit;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderCatalogSummaryView".
 */
export interface ProviderCatalogSummaryView {
  catalog_model_count: number;
  catalog_provider_id: string;
  configuration_class: ProviderCatalogConfigurationClass;
  connection_defaults?: ProviderCatalogConnectionDefaultsView | null;
  display_name: string;
  fallback_initial: string;
  icon_ref?: ProviderIconRef | null;
  local_provider_id?: ProviderId | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderPresetVariantView".
 */
export interface ProviderPresetVariantView {
  api_format: ModelApiFormat;
  auth_scheme: EndpointAuthScheme;
  base_url: string;
  chat_instruction_role?: ChatInstructionRole | null;
  chat_output_token_field?: ChatOutputTokenField | null;
  discovery_mode: ModelDiscoveryMode;
  display_name: string;
  recommended: boolean;
  variant_id: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderIconRef".
 */
export interface ProviderIconRef {
  artifact_id: ArtifactId;
  byte_length: number;
  content_hash: ContentHash;
  pixel_size: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelCatalogRefreshRequest".
 */
export interface ModelCatalogRefreshRequest {}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelEndpointClearCredentialRequest".
 */
export interface ModelEndpointClearCredentialRequest {
  credential_id: CredentialId;
  credential_slot: ModelCredentialSlot;
  endpoint_id: EndpointId;
  expected_models_revision: ExpectedRevision;
  expected_row_revision: ExpectedRevision;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelCompatibilityGetRequest".
 */
export interface ModelCompatibilityGetRequest {
  endpoint_id: EndpointId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelCompatibilityResetRequest".
 */
export interface ModelCompatibilityResetRequest {
  endpoint_id: EndpointId;
  expected_revision: ExpectedRevision;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelCompatibilityResetResult".
 */
export interface ModelCompatibilityResetResult {
  changed: boolean;
  compatibility?: EndpointCompatibilityOverrides | null;
  endpoint_id: EndpointId;
  revision: Revision;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "EndpointCompatibilityOverrides".
 */
export interface EndpointCompatibilityOverrides {
  endpoint_fingerprint: ContentHash;
  learned_at?: UtcTimestamp | null;
  output_token_field_aliases: ChatOutputTokenFieldAlias[];
  unsupported_features: string[];
  unsupported_parameters: string[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ChatOutputTokenFieldAlias".
 */
export interface ChatOutputTokenFieldAlias {
  configured_field: ChatOutputTokenField;
  effective_field: ChatOutputTokenField;
  learned_at: UtcTimestamp;
  learned_from_transport_attempt_id: TransportAttemptId;
  provider_model_name: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "EndpointCompatibilitySnapshot".
 */
export interface EndpointCompatibilitySnapshot {
  compatibility?: EndpointCompatibilityOverrides | null;
  endpoint_id: EndpointId;
  revision: Revision;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelEndpointConfigureCredentialRequest".
 */
export interface ModelEndpointConfigureCredentialRequest {
  credential_id: CredentialId;
  credential_slot: ModelCredentialSlot;
  endpoint_id: EndpointId;
  expected_models_revision: ExpectedRevision;
  expected_row_revision: ExpectedRevision;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextWindowClearUserOverrideRequest".
 */
export interface ContextWindowClearUserOverrideRequest {
  expected_revision: ExpectedRevision;
  model_profile_id: ModelProfileId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextWindowCorrectRequest".
 */
export interface ContextWindowCorrectRequest {
  expected_revision: ExpectedRevision;
  model_profile_id: ModelProfileId;
  window_tokens: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextWindowMutationResult".
 */
export interface ContextWindowMutationResult {
  changed: boolean;
  revision: Revision;
  value: ContextWindowUpdateResult;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextWindowUpdateResult".
 */
export interface ContextWindowUpdateResult {
  experience_warning: boolean;
  model_profile_id: ModelProfileId;
  runtime_effective_window_tokens: number;
  user_override_revision: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextWindowResetRuntimeCorrectionRequest".
 */
export interface ContextWindowResetRuntimeCorrectionRequest {
  expected_revision: ExpectedRevision;
  model_profile_id: ModelProfileId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelEndpointCredentialClearedPayload".
 */
export interface ModelEndpointCredentialClearedPayload {
  credential_id: CredentialId;
  endpoint_id: EndpointId;
  /**
   * Secret-generation revision for one stable CredentialId.
   *
   * This is intentionally distinct from a row revision. Clear and metadata-only
   * mutations may change row concurrency without creating a new secret
   * generation.
   */
  last_committed_credential_revision: string;
  models_revision: Revision;
  previous_credential_binding_fingerprint: ContentHash;
  row_revision: Revision;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelEndpointCredentialConfiguredPayload".
 */
export interface ModelEndpointCredentialConfiguredPayload {
  /**
   * Secret-generation revision for one stable CredentialId.
   *
   * This is intentionally distinct from a row revision. Clear and metadata-only
   * mutations may change row concurrency without creating a new secret
   * generation.
   */
  active_credential_revision: string;
  credential_binding_fingerprint: ContentHash;
  credential_id: CredentialId;
  endpoint_id: EndpointId;
  models_revision: Revision;
  row_revision: Revision;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "EndpointCredentialMutationResult".
 */
export interface EndpointCredentialMutationResult {
  changed: boolean;
  credential_status: EndpointCredentialStatus;
  endpoint_id: EndpointId;
  models_revision: Revision;
}
/**
 * Renderer-safe state for one stable Endpoint credential slot.
 *
 * `CredentialId` is a non-secret logical identity, not an authority-bearing
 * capability. Storage locators, targets, binding fingerprints, Journal facts,
 * and Secret material are intentionally absent.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "EndpointCredentialStatus".
 */
export interface EndpointCredentialStatus {
  active_credential_revision?: string | null;
  credential_id: CredentialId;
  endpoint_id: EndpointId;
  error_code?: string | null;
  last_committed_credential_revision?: string | null;
  row_revision: Revision;
  slot_name: ModelCredentialSlot;
  state: EndpointCredentialState;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "DefaultModelMutationResult".
 */
export interface DefaultModelMutationResult {
  changed: boolean;
  revision: Revision;
  value: DefaultModelSelection;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "DefaultModelSelection".
 */
export interface DefaultModelSelection {
  aura_config_revision: number;
  model_profile_id?: ModelProfileId | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelDefaultSetRequest".
 */
export interface ModelDefaultSetRequest {
  expected_revision: ExpectedRevision;
  model_profile_id?: ModelProfileId | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelDeletionMutationResult".
 */
export interface ModelDeletionMutationResult {
  changed: boolean;
  revision: Revision;
  value: DeleteModelConfigCommitResult;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "DeleteModelConfigCommitResult".
 */
export interface DeleteModelConfigCommitResult {
  /**
   * The configuration delete committed successfully, but one or more
   * internal CredentialDelete children still require secure cleanup.
   */
  credential_cleanup_pending: boolean;
  default_model_cleared: boolean;
  deleted: DeleteModelConfigResult;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "DeleteModelConfigResult".
 */
export interface DeleteModelConfigResult {
  deleted_endpoint_ids: EndpointId[];
  deleted_model_profile_ids: ModelProfileId[];
  deleted_provider_ids: ProviderId[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelDiscoveryCancelRequest".
 */
export interface ModelDiscoveryCancelRequest {
  expected_operational_revision: ExpectedRevision;
  model_discovery_run_id: ModelDiscoveryRunId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelDiscoveryCancelResult".
 */
export interface ModelDiscoveryCancelResult {
  model_discovery_run_id: ModelDiscoveryRunId;
  operational_revision: Revision;
  state: ModelDiscoveryState;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelDiscoveryStartAccepted".
 */
export interface ModelDiscoveryStartAccepted {
  model_discovery_run_id: ModelDiscoveryRunId;
  operational_revision: Revision;
  state: ModelDiscoveryState;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelDiscoveryStartRequest".
 */
export interface ModelDiscoveryStartRequest {
  endpoint_id: EndpointId;
  expected_endpoint_revision: ExpectedRevision;
  expected_models_revision: ExpectedRevision;
}
/**
 * Renderer-safe summary for a retained credential slot whose Endpoint is
 * currently configured with `auth_scheme = none`.
 *
 * The summary preserves the distinction between a never-created slot and a
 * retained dormant slot without exposing an active revision, storage
 * locator, binding fingerprint, Journal state, or native platform error.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "DormantEndpointCredentialSummary".
 */
export interface DormantEndpointCredentialSummary {
  credential_id: CredentialId;
  endpoint_id: EndpointId;
  error_code?: string | null;
  last_committed_credential_revision?: string | null;
  retained_state: EndpointCredentialState;
  row_revision: Revision;
  slot_name: ModelCredentialSlot;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelEndpointCreateRequest".
 */
export interface ModelEndpointCreateRequest {
  anthropic_workspace_id?: string | null;
  api_format: ModelApiFormat;
  auth_scheme: EndpointAuthScheme;
  base_url: string;
  chat_instruction_role?: ChatInstructionRole | null;
  chat_output_token_field?: ChatOutputTokenField | null;
  discovery_mode: ModelDiscoveryMode;
  display_name: string;
  expected_revision: ExpectedRevision;
  max_parallel_model_requests: number;
  openai_organization_id?: string | null;
  openai_project_id?: string | null;
  provider_id: ProviderId;
  timeouts: ModelEndpointTimeouts;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelEndpointTimeouts".
 */
export interface ModelEndpointTimeouts {
  connect_ms: number;
  first_event_ms: number;
  stream_idle_ms: number;
  total_ms?: number | null;
}
/**
 * Non-secret Control Command half of `models.endpoint.create_with_credential`.
 *
 * The matching API key is carried only by the negotiated SecretInput frame.
 * Endpoint, credential, Journal, Event, and reserved Revision identities are
 * generated by Core and therefore intentionally absent here.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelEndpointCreateWithCredentialRequest".
 */
export interface ModelEndpointCreateWithCredentialRequest {
  anthropic_workspace_id?: string | null;
  api_format: ModelApiFormat;
  auth_scheme: EndpointAuthScheme;
  base_url: string;
  chat_instruction_role?: ChatInstructionRole | null;
  chat_output_token_field?: ChatOutputTokenField | null;
  discovery_mode: ModelDiscoveryMode;
  display_name: string;
  expected_revision: ExpectedRevision;
  max_parallel_model_requests: number;
  openai_organization_id?: string | null;
  openai_project_id?: string | null;
  provider_id: ProviderId;
  timeouts: ModelEndpointTimeouts;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "EndpointCreatedWithCredentialResult".
 */
export interface EndpointCreatedWithCredentialResult {
  endpoint_projection: EndpointDetailProjection;
  models_revision: Revision;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "EndpointDetailProjection".
 */
export interface EndpointDetailProjection {
  authentication: EndpointAuthenticationProjection;
  endpoint: ModelEndpointConfig;
  endpoint_id: EndpointId;
  latest_discovery_run?: ModelDiscoveryRunView | null;
  models: ModelProfileConfig[];
  models_revision: Revision;
  provider: ModelProviderConfig;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "EndpointAuthenticationProjection".
 */
export interface EndpointAuthenticationProjection {
  active_credential?: EndpointCredentialStatus | null;
  auth_kind: EndpointAuthenticationKind;
  dormant_credential?: DormantEndpointCredentialSummary | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelEndpointConfig".
 */
export interface ModelEndpointConfig {
  anthropic_workspace_id?: string | null;
  api_format: ModelApiFormat;
  auth_scheme: EndpointAuthScheme;
  base_url: string;
  chat_instruction_role?: ChatInstructionRole | null;
  chat_output_token_field?: ChatOutputTokenField | null;
  compatibility?: EndpointCompatibilityOverrides | null;
  created_at: UtcTimestamp;
  discovery_mode: ModelDiscoveryMode;
  display_name: string;
  id: EndpointId;
  max_parallel_model_requests: number;
  model_order: ModelProfileId[];
  openai_organization_id?: string | null;
  openai_project_id?: string | null;
  provider_id: ProviderId;
  revision: Revision;
  slug: EndpointSlug;
  timeouts: ModelEndpointTimeouts;
  updated_at: UtcTimestamp;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelDiscoveryRunView".
 */
export interface ModelDiscoveryRunView {
  cancellation_reason?: ModelOperationCancellationReason | null;
  created_count?: number | null;
  finished_at?: UtcTimestamp | null;
  id: ModelDiscoveryRunId;
  not_returned_count?: number | null;
  outcome?: ModelDiscoveryOutcome | null;
  provider_error?: ProviderErrorSummaryView | null;
  returned_count?: number | null;
  revision: Revision;
  stage?: ModelDiscoveryStage | null;
  started_at: UtcTimestamp;
  state: ModelDiscoveryState;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderErrorSummaryView".
 */
export interface ProviderErrorSummaryView {
  /**
   * A Renderer-safe, stable issue code from Aura's v1 normalized registry.
   *
   * This is deliberately a distinct domain type. Raw provider, Win32, DNS, or
   * TLS diagnostics must not be converted into it or embedded in its suffix.
   */
  issue_code: string;
  message_was_redacted: boolean;
  message_was_truncated: boolean;
  provider_message?: string | null;
  provider_request_id?: string | null;
  provider_status?: number | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelProfileConfig".
 */
export interface ModelProfileConfig {
  available_for_subagents: boolean;
  catalog?: ModelCatalogMetadata | null;
  context: ModelContextWindowConfig;
  created_at: UtcTimestamp;
  display_name: string;
  enabled: boolean;
  endpoint_id: EndpointId;
  id: ModelProfileId;
  notes?: string | null;
  origin: ModelProfileOrigin;
  provider_model_name: string;
  request_defaults: ModelRequestDefaults;
  revision: Revision;
  updated_at: UtcTimestamp;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelCatalogMetadata".
 */
export interface ModelCatalogMetadata {
  catalog_content_hash: ContentHash;
  catalog_model_id: string;
  catalog_object_hash: ContentHash;
  catalog_provider_id: string;
  catalog_revision: Revision;
  match_state: CatalogMatchState;
  matched_at?: UtcTimestamp | null;
  snapshot: ProviderCatalogModelEntry;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelContextWindowConfig".
 */
export interface ModelContextWindowConfig {
  catalog_window_tokens?: number | null;
  historical_snapshot_max_tokens: number;
  runtime_corrected_window_tokens?: number | null;
  runtime_correction_basis?: RuntimeWindowCorrectionBasis | null;
  runtime_effective_window_tokens: number;
  user_override_revision: number;
  user_override_window_tokens?: number | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RuntimeWindowCorrectionBasis".
 */
export interface RuntimeWindowCorrectionBasis {
  corrected_at: UtcTimestamp;
  endpoint_fingerprint: ContentHash;
  model_name_hash: ContentHash;
  provider_error_ref: ArtifactId;
  user_override_revision: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelRequestDefaults".
 */
export interface ModelRequestDefaults {
  chat_instruction_role?: ChatInstructionRole | null;
  chat_output_token_field?: ChatOutputTokenField | null;
  /**
   * Whether the model is sent images (the browser's screenshots), when its user said so;
   * none takes it from the matched catalog entry.
   */
  image_input?: boolean | null;
  max_output_tokens?: number | null;
  /**
   * The thinking level the profile asks for; none is the provider's own default and
   * sends nothing.
   */
  reasoning_effort?: ReasoningEffort | null;
  /**
   * The levels the model offers, when its user listed them (a model the catalog does
   * not know, or knows wrongly); none takes them from the catalog and the API format.
   */
  reasoning_efforts?: ReasoningEffort[] | null;
  temperature?: number | null;
  top_p?: number | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelProviderConfig".
 */
export interface ModelProviderConfig {
  /**
   * Connections tried in this order when the one used fails before it answers. Every other connection is a backup in the order they were created; a list saved before is
   * still read and decides nothing.
   */
  backup_endpoint_ids?: EndpointId[];
  created_at: UtcTimestamp;
  /**
   * The
   * connection this provider's models are sent through. None makes the first created the
   * default.
   */
  default_endpoint_id?: EndpointId | null;
  display_name: string;
  endpoint_order: EndpointId[];
  id: ProviderId;
  slug: ProviderSlug;
  source: ModelProviderSource;
  updated_at: UtcTimestamp;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelEndpointDeleteRequest".
 */
export interface ModelEndpointDeleteRequest {
  endpoint_id: EndpointId;
  expected_revision: ExpectedRevision;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelEndpointMutationResult".
 */
export interface ModelEndpointMutationResult {
  changed: boolean;
  revision: Revision;
  value: ModelEndpointConfig;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelEndpointReplaceCredentialRequest".
 */
export interface ModelEndpointReplaceCredentialRequest {
  credential_id: CredentialId;
  credential_slot: ModelCredentialSlot;
  endpoint_id: EndpointId;
  expected_models_revision: ExpectedRevision;
  expected_row_revision: ExpectedRevision;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelEndpointUpdateRequest".
 */
export interface ModelEndpointUpdateRequest {
  anthropic_workspace_id?: string | null;
  api_format: ModelApiFormat;
  auth_scheme: EndpointAuthScheme;
  base_url: string;
  chat_instruction_role?: ChatInstructionRole | null;
  chat_output_token_field?: ChatOutputTokenField | null;
  discovery_mode: ModelDiscoveryMode;
  display_name: string;
  endpoint_id: EndpointId;
  expected_endpoint_revision: ExpectedRevision;
  expected_models_revision: ExpectedRevision;
  max_parallel_model_requests: number;
  openai_organization_id?: string | null;
  openai_project_id?: string | null;
  timeouts: ModelEndpointTimeouts;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelExecutionSnapshot".
 */
export interface ModelExecutionSnapshot {
  anthropic_workspace_id?: string | null;
  api_format: ModelApiFormat;
  auth_scheme: EndpointAuthScheme;
  base_url: string;
  chat_instruction_role?: ChatInstructionRole | null;
  chat_output_token_field?: ChatOutputTokenField | null;
  context_snapshot_id: ContextProjectionSnapshotId;
  created_at: UtcTimestamp;
  credential_binding_fingerprint?: ContentHash | null;
  credential_id?: CredentialId | null;
  credential_revision?: string | null;
  endpoint_config_revision: number;
  endpoint_display_name: string;
  endpoint_fingerprint: ContentHash;
  endpoint_id: EndpointId;
  historical_snapshot_max_tokens: number;
  historical_tool_call_wire_bindings: ProviderToolCallWireBinding[];
  id: ModelExecutionSnapshotId;
  model_config_revision: number;
  model_name: string;
  model_profile_id: ModelProfileId;
  model_step_id: ModelStepId;
  openai_organization_id?: string | null;
  openai_project_id?: string | null;
  prompt_bundle_snapshot_id: PromptBundleSnapshotId;
  provider_display_name: string;
  provider_id: ProviderId;
  readable_ref: string;
  request_options: ResolvedModelRequestOptions;
  resolved_request_url: string;
  runtime_effective_window_tokens: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderToolCallWireBinding".
 */
export interface ProviderToolCallWireBinding {
  provider_wire_tool_name: string;
  tool_call_id: ToolCallId;
  /**
   * Null only for a call whose Provider name was absent from the frozen request.
   */
  tool_id: ToolId | null;
  wire_identity: ProviderToolCallWireIdentity;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ResolvedModelRequestOptions".
 */
export interface ResolvedModelRequestOptions {
  effective_max_output_tokens: number;
  /**
   * The model is sent images, such as the browser's screenshots.
   */
  image_input?: boolean;
  /**
   * The model takes its level as an effort (its catalog lists effort levels), not as
   * a thinking budget; how the Anthropic format sends the level.
   */
  reasoning_by_effort?: boolean;
  /**
   * The thinking level sent with the request, when the profile names one the model
   * offers.
   */
  reasoning_effort?: ReasoningEffort | null;
  temperature?: number | null;
  top_p?: number | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NormalizedModelFailure".
 */
export interface NormalizedModelFailure {
  category: ModelFailureCategory;
  code: string;
  message: string;
  partial_output_ref?: ArtifactId | null;
  provider_error_ref?: ArtifactId | null;
  provider_request_id?: string | null;
  provider_status?: number | null;
  retry_class: ModelRetryClass;
  transport_attempt_id?: TransportAttemptId | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "LogicalModelRequest".
 */
export interface LogicalModelRequest {
  attachments: ModelAttachmentSnapshot[];
  context_snapshot_id: ContextProjectionSnapshotId;
  conversation_items: LogicalConversationItem[];
  execution_snapshot_id: ModelExecutionSnapshotId;
  /**
   * Wire order only. The immutable schema set retains canonical ToolId order.
   */
  fixed_tool_ids?: ToolId[];
  instruction_envelope: CompiledInstructionEnvelope;
  model_step_id: ModelStepId;
  output_request: ModelOutputRequest;
  provider_continuation?: ProviderContinuationState | null;
  tool_schema_set: ToolSchemaSetSnapshotV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NormalizedModelOutput".
 */
export interface NormalizedModelOutput {
  assistant_blocks: AssistantContentBlock[];
  model_step_id: ModelStepId;
  provider_continuation?: ProviderContinuationState | null;
  provider_request_id?: string | null;
  raw_response_ref?: ArtifactId | null;
  reasoning_blocks: ReasoningContentBlock[];
  refusal?: ModelRefusal | null;
  stop_reason: NormalizedStopReason;
  tool_calls: NormalizedToolCall[];
  usage: ModelUsage;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AssistantContentBlock".
 */
export interface AssistantContentBlock {
  block_id: ModelContentBlockId;
  provider_reference?: ProviderProtocolReference | null;
  text: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderProtocolReference".
 */
export interface ProviderProtocolReference {
  block_id?: string | null;
  index?: number | null;
  item_id?: string | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ReasoningContentBlock".
 */
export interface ReasoningContentBlock {
  block_id: ModelContentBlockId;
  provider_reference?: ProviderProtocolReference | null;
  summary?: string | null;
  text: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelRefusal".
 */
export interface ModelRefusal {
  message: string;
  provider_code?: string | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NormalizedToolCall".
 */
export interface NormalizedToolCall {
  arguments_json: string;
  position: number;
  provider_call_id: ProviderToolCallId;
  tool_call_id: ToolCallId;
  tool_name: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelUsage".
 */
export interface ModelUsage {
  cache_write_tokens?: number | null;
  cached_input_tokens?: number | null;
  input_tokens: number;
  output_tokens: number;
  reasoning_tokens?: number | null;
  source: UsageSource;
  total_tokens: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PersistedModelFailure".
 */
export interface PersistedModelFailure {
  created_at: UtcTimestamp;
  failure: NormalizedModelFailure;
  fingerprint: ContentHash;
  id: ModelFailureId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelProfileApplyCatalogUpdateRequest".
 */
export interface ModelProfileApplyCatalogUpdateRequest {
  expected_catalog_object_hash: ContentHash;
  expected_catalog_revision: Revision;
  expected_model_profile_revision: ExpectedRevision;
  expected_models_revision: ExpectedRevision;
  model_profile_id: ModelProfileId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelProfileCreateFromCatalogRequest".
 */
export interface ModelProfileCreateFromCatalogRequest {
  catalog_model_id: string;
  catalog_provider_id: string;
  endpoint_id: EndpointId;
  expected_catalog_object_hash: ContentHash;
  expected_catalog_revision: Revision;
  expected_endpoint_revision: ExpectedRevision;
  expected_models_revision: ExpectedRevision;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelProfileCreateManualRequest".
 */
export interface ModelProfileCreateManualRequest {
  expected_revision: ExpectedRevision;
  input: AddManualModelInput;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AddManualModelInput".
 */
export interface AddManualModelInput {
  context_window_tokens?: number | null;
  display_name?: string | null;
  endpoint_id: EndpointId;
  notes?: string | null;
  provider_model_name: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelProfileDeleteRequest".
 */
export interface ModelProfileDeleteRequest {
  expected_revision: ExpectedRevision;
  model_profile_id: ModelProfileId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelProfileMutationResult".
 */
export interface ModelProfileMutationResult {
  changed: boolean;
  revision: Revision;
  value: ModelProfileConfig;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelProfileSetSubagentAvailabilityRequest".
 */
export interface ModelProfileSetSubagentAvailabilityRequest {
  available_for_subagents: boolean;
  expected_revision: ExpectedRevision;
  model_profile_id: ModelProfileId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelProfileUpdateRequest".
 */
export interface ModelProfileUpdateRequest {
  display_name: string;
  enabled: boolean;
  expected_revision: ExpectedRevision;
  historical_snapshot_max_tokens: number;
  model_profile_id: ModelProfileId;
  notes?: string | null;
  request_defaults: ModelRequestDefaults;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderBulkAvailabilityResult".
 */
export interface ProviderBulkAvailabilityResult {
  changed_model_profile_ids: ModelProfileId[];
  provider_id: ProviderId;
  revision: Revision;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelProviderCreateRequest".
 */
export interface ModelProviderCreateRequest {
  expected_revision: ExpectedRevision;
  input: CreateModelProviderInput;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CreateModelProviderInput".
 */
export interface CreateModelProviderInput {
  display_name: string;
  source: ModelProviderSource;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelProviderDeleteRequest".
 */
export interface ModelProviderDeleteRequest {
  expected_revision: ExpectedRevision;
  provider_id: ProviderId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderIconReadRequest".
 */
export interface ProviderIconReadRequest {
  artifact_id: ArtifactId;
  content_hash: ContentHash;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderIconReadResult".
 */
export interface ProviderIconReadResult {
  byte_length: number;
  content_hash: ContentHash;
  png_base64: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelProviderMutationResult".
 */
export interface ModelProviderMutationResult {
  changed: boolean;
  revision: Revision;
  value: ModelProviderConfig;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelProviderRenameRequest".
 */
export interface ModelProviderRenameRequest {
  display_name: string;
  expected_revision: ExpectedRevision;
  provider_id: ProviderId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderRequestEvidence".
 */
export interface ProviderRequestEvidence {
  adapter: ModelAdapterIdentity;
  applied_output_token_field_alias?: ChatOutputTokenFieldAlias | null;
  body_byte_length: number;
  body_sha256: ContentHash;
  canonical_body_artifact_id?: ArtifactId | null;
  compatibility_removals: string[];
  headers: ProviderRequestHeaderMetadata[];
  historical_tool_call_wire_bindings: ProviderToolCallWireBinding[];
  operation_context: TransportOperationContext;
  target: ProviderRequestTargetMetadata;
  tool_name_bindings: ProviderToolNameBinding[];
  tool_registry_snapshot_ref?: ToolRegistrySnapshotRef | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelAdapterIdentity".
 */
export interface ModelAdapterIdentity {
  api_format: ModelApiFormat;
  contract_version: ContractVersion;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderRequestHeaderMetadata".
 */
export interface ProviderRequestHeaderMetadata {
  class: ProviderRequestHeaderClass;
  name: string;
  non_secret_value?: string | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderRequestTargetMetadata".
 */
export interface ProviderRequestTargetMetadata {
  method: string;
  origin: NormalizedOrigin;
  path: string;
  query_parameter_names: string[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NormalizedOrigin".
 */
export interface NormalizedOrigin {
  host: string;
  port: number;
  scheme: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderToolNameBinding".
 */
export interface ProviderToolNameBinding {
  canonical_name: string;
  package_schema_hash: CanonicalSha256HashV1;
  provider_arguments_envelope_version: number;
  provider_wire_name: string;
  provider_wrapped_schema_hash: CanonicalSha256HashV1;
  tool_definition: ToolDefinitionSnapshotRef;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDefinitionSnapshotRef".
 */
export interface ToolDefinitionSnapshotRef {
  registry_snapshot_ref: ToolRegistrySnapshotRef;
  tool_id: ToolId;
  tool_version: ToolVersion;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelProviderSetAllCurrentModelsSubagentAvailabilityRequest".
 */
export interface ModelProviderSetAllCurrentModelsSubagentAvailabilityRequest {
  available_for_subagents: boolean;
  expected_revision: ExpectedRevision;
  provider_id: ProviderId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderToolCallProtocolBindingRecord".
 */
export interface ProviderToolCallProtocolBindingRecord {
  model_execution_snapshot_id: ModelExecutionSnapshotId;
  provider_tool_call_id: ProviderToolCallId;
  provider_wire_tool_name: string;
  tool_call_id: ToolCallId;
  /**
   * Null only for a call whose Provider name was absent from the frozen request.
   */
  tool_definition: ToolDefinitionSnapshotRef | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelRegistryConfig".
 */
export interface ModelRegistryConfig {
  endpoints: {
    [k: string]: ModelEndpointConfig | undefined;
  };
  models: {
    [k: string]: ModelProfileConfig | undefined;
  };
  provider_order: ProviderId[];
  providers: {
    [k: string]: ModelProviderConfig | undefined;
  };
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelRuntimeCapabilityFacts".
 */
export interface ModelRuntimeCapabilityFacts {
  api_format: ModelApiFormat;
  native_attachment_parts_present: boolean;
  native_capability_names: string[];
  native_tool_channel_exposed: boolean;
  reasoning_channel_observed: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SessionModelBindingMutationResult".
 */
export interface SessionModelBindingMutationResult {
  changed: boolean;
  revision: Revision;
  value?: SessionModelBinding | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SessionModelBinding".
 */
export interface SessionModelBinding {
  binding_revision: Revision;
  bound_at: UtcTimestamp;
  model_profile_id: ModelProfileId;
  readable_ref_at_binding: string;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SessionModelBindingSetRequest".
 */
export interface SessionModelBindingSetRequest {
  expected_revision: ExpectedRevision;
  model_profile_id?: ModelProfileId | null;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelSettingsGetSnapshotRequest".
 */
export interface ModelSettingsGetSnapshotRequest {}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelStreamEventEnvelope".
 */
export interface ModelStreamEventEnvelope {
  event: ModelStreamEvent;
  model_step_id: ModelStepId;
  received_at: UtcTimestamp;
  sequence: number;
  transport_attempt_id: TransportAttemptId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PartialModelUsage".
 */
export interface PartialModelUsage {
  cache_write_tokens?: number | null;
  cached_input_tokens?: number | null;
  input_tokens?: number | null;
  output_tokens?: number | null;
  reasoning_tokens?: number | null;
  total_tokens?: number | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TransportAttemptRecord".
 */
export interface TransportAttemptRecord {
  attempt_kind: TransportAttemptKind;
  attempt_number: number;
  client_request_id: string;
  endpoint_fingerprint: ContentHash;
  endpoint_id: EndpointId;
  failure_id?: ModelFailureId | null;
  finished_at?: UtcTimestamp | null;
  first_byte_at?: UtcTimestamp | null;
  first_model_event_at?: UtcTimestamp | null;
  id: TransportAttemptId;
  provider_request_id?: string | null;
  raw_stream_artifact_id?: ArtifactId | null;
  request_evidence: ProviderRequestEvidence;
  started_at: UtcTimestamp;
  state: TransportAttemptState;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PersistenceContractFixture".
 */
export interface PersistenceContractFixture {
  artifact_record: ArtifactRecord;
  credential_ref: CredentialRef;
  workspace_session_projection_change: ProjectionChange;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ArtifactRecord".
 */
export interface ArtifactRecord {
  access: number;
  artifact_id: ArtifactId;
  byte_length: number;
  content_hash: ContentHash;
  created_at: UtcTimestamp;
  inline: boolean;
  media_type: string;
  owner: ArtifactOwnerRef;
  scope: ArtifactScope;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CoreHealthProjection".
 */
export interface CoreHealthProjection {
  /**
   * Long work that holds Core's stores, while it has run for a while ("Core 正忙").
   */
  busy?: CoreBusyV1 | null;
  read_only: boolean;
  restart_attempt: number;
  safe_mode: boolean;
  state: CoreLifecycleState;
  status_code: string;
  status_message: string;
}
/**
 * The long work Core is doing that holds its stores — other requests wait
 * behind it — and since when.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CoreBusyV1".
 */
export interface CoreBusyV1 {
  activity: CoreBusyActivityV1;
  since: UtcTimestamp;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceSessionProjection".
 */
export interface WorkspaceSessionProjection {
  sessions: SessionSummary[];
  workspaces: WorkspaceSummary[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SessionSummary".
 */
export interface SessionSummary {
  session_id: SessionId;
  state: string;
  title: string;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceSummary".
 */
export interface WorkspaceSummary {
  authorization_epoch: Revision;
  canonical_root: string;
  catalog_freshness: CatalogFreshness;
  display_name: string;
  revision: Revision;
  root_identity?: FileIdentity | null;
  selected_root: string;
  state: WorkspaceState;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FileIdentity".
 */
export interface FileIdentity {
  file_id: string;
  volume_serial_number: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceFilesProjection".
 */
export interface WorkspaceFilesProjection {
  lifecycle_operations: WorkspaceLifecycleSummary[];
  workspaces: WorkspaceFileStatus[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceLifecycleSummary".
 */
export interface WorkspaceLifecycleSummary {
  ack_deadline_at?: UtcTimestamp | null;
  approval_follow_up?: EditorApprovalFollowUp[];
  error_code?: string | null;
  lifecycle_revision: Revision;
  operation: WorkspaceLifecycleOperationKind;
  operation_id: WorkspaceLifecycleOperationId;
  preflight?: WorkspaceLifecyclePreflight | null;
  renderer_instance_id: RendererInstanceId;
  state: WorkspaceLifecycleState;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "EditorApprovalFollowUp".
 */
export interface EditorApprovalFollowUp {
  message: string;
  path: string;
  requires_reapproval: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceLifecyclePreflight".
 */
export interface WorkspaceLifecyclePreflight {
  blockers: WorkspaceLifecycleBlocker[];
  lifecycle_revision: Revision;
  operation: WorkspaceLifecycleOperationKind;
  operation_id: WorkspaceLifecycleOperationId;
  releasable_editor_states: EditorStateRef[];
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "EditorStateRef".
 */
export interface EditorStateRef {
  buffer_revision: Revision;
  canonical_relative_path: string;
  reservation_epoch: Revision;
  reservation_id?: EditorReservationId | null;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceFileStatus".
 */
export interface WorkspaceFileStatus {
  catalog: WorkspaceCatalogStatus;
  deferred_mutations: DeferredMutationSummary[];
  editor_reservations: EditorReservationSummary[];
  logical_files: LogicalFileStatus[];
  workspace_id: WorkspaceId;
  workspace_revision: Revision;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceCatalogStatus".
 */
export interface WorkspaceCatalogStatus {
  catalog_revision: Revision;
  freshness: CatalogFreshness;
  indexed_at?: string | null;
  partitions: CatalogPartitionSummary[];
  watcher_state: WorkspaceWatcherState;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CatalogPartitionSummary".
 */
export interface CatalogPartitionSummary {
  file_count: number;
  index_mode: CatalogIndexMode;
  indexable_text_bytes: number;
  partition_path: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "DeferredMutationSummary".
 */
export interface DeferredMutationSummary {
  created_at: string;
  deferred_mutation_id: DeferredMutationId;
  path: string;
  reason: string;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "EditorReservationSummary".
 */
export interface EditorReservationSummary {
  path: string;
  reservation_epoch: Revision;
  reservation_id: EditorReservationId;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "LogicalFileStatus".
 */
export interface LogicalFileStatus {
  active_editing: boolean;
  approval_follow_up?: EditorApprovalFollowUp[];
  buffer_revision: Revision;
  core_acknowledged_revision: Revision;
  dirty: boolean;
  disk_hash?: ContentHash | null;
  ime_composition_active: boolean;
  logical_hash: ContentHash;
  path: string;
  reservation_epoch: Revision;
  reservation_id?: EditorReservationId | null;
  save_state: EditorSaveState;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelSettingsProjection".
 */
export interface ModelSettingsProjection {
  active_credentials: EndpointCredentialStatus[];
  aura_revision: Revision;
  default_model_profile_id?: ModelProfileId | null;
  dormant_credentials: DormantEndpointCredentialSummary[];
  models_revision: Revision;
  /**
   * The thinking levels each model offers; a model offering none is absent.
   */
  reasoning_efforts?: ModelReasoningEffortsView[];
  registry: ModelRegistryConfig;
}
/**
 * The thinking levels one model offers, lowest first.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelReasoningEffortsView".
 */
export interface ModelReasoningEffortsView {
  model_profile_id: ModelProfileId;
  reasoning_efforts: ReasoningEffort[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderCatalogProjection".
 */
export interface ProviderCatalogProjection {
  cards: ProviderCatalogCardView[];
  catalog_content_hash?: ContentHash | null;
  catalog_revision?: Revision | null;
  checked_at?: UtcTimestamp | null;
  last_error_code?: string | null;
  last_successful_refresh_at?: UtcTimestamp | null;
  projection_revision: Revision;
  state: ProviderCatalogState;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProviderCatalogCardView".
 */
export interface ProviderCatalogCardView {
  catalog_model_count: number;
  catalog_provider_id: string;
  configuration_class: ProviderCatalogConfigurationClass;
  connection_defaults?: ProviderCatalogConnectionDefaultsView | null;
  display_name: string;
  fallback_initial: string;
  icon_ref?: ProviderIconRef | null;
  local_provider_id?: ProviderId | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelProfileProjection".
 */
export interface ModelProfileProjection {
  active_availability_test?: ModelAvailabilityTestView | null;
  endpoint: ModelEndpointConfig;
  execution_readiness: ModelExecutionReadinessView;
  latest_availability_test?: ModelAvailabilityTestView | null;
  model_profile_id: ModelProfileId;
  models_revision: Revision;
  profile: ModelProfileConfig;
  provider: ModelProviderConfig;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelAvailabilityTestView".
 */
export interface ModelAvailabilityTestView {
  cancellation_reason?: ModelOperationCancellationReason | null;
  completed_at?: UtcTimestamp | null;
  endpoint_id: EndpointId;
  endpoint_revision_at_start: Revision;
  endpoint_security_fingerprint: ContentHash;
  id: ModelAvailabilityTestId;
  model_profile_id: ModelProfileId;
  model_profile_revision_at_start: Revision;
  models_revision_at_start: Revision;
  result?: ModelAvailabilityTestResult | null;
  revision: Revision;
  stage?: ModelAvailabilityTestStage | null;
  started_at: UtcTimestamp;
  state: ModelAvailabilityTestState;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelAvailabilityTestResult".
 */
export interface ModelAvailabilityTestResult {
  authentication: ModelAvailabilityCheckResult;
  completed_at: UtcTimestamp;
  config: ModelAvailabilityCheckResult;
  http: ModelAvailabilityCheckResult;
  model_generation: ModelAvailabilityCheckResult;
  network: ModelAvailabilityCheckResult;
  outcome: ModelAvailabilityTestOutcome;
  provider_error?: ProviderErrorSummaryView | null;
  started_at: UtcTimestamp;
  tested_endpoint_id: EndpointId;
  tested_model_name: string;
  tested_model_profile_id: ModelProfileId;
  tls: ModelAvailabilityCheckResult;
  /**
   * Items: A Renderer-safe, stable issue code from Aura's v1 normalized registry.
   *
   * This is deliberately a distinct domain type. Raw provider, Win32, DNS, or
   * TLS diagnostics must not be converted into it or embedded in its suffix.
   */
  warnings: string[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelExecutionReadinessView".
 */
export interface ModelExecutionReadinessView {
  is_executable: boolean;
  issue_code?: string | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SessionModelSelectorProjection".
 */
export interface SessionModelSelectorProjection {
  binding_revision: Revision;
  default_model_profile_id?: ModelProfileId | null;
  endpoints: SessionModelEndpointView[];
  models: SessionModelOptionView[];
  projection_revision: Revision;
  providers: SessionModelProviderView[];
  selected_model_execution_readiness: ModelExecutionReadinessView;
  selected_model_profile_id?: ModelProfileId | null;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SessionModelEndpointView".
 */
export interface SessionModelEndpointView {
  api_format: ModelApiFormat;
  display_name: string;
  endpoint_id: EndpointId;
  order: number;
  provider_id: ProviderId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SessionModelOptionView".
 */
export interface SessionModelOptionView {
  display_name: string;
  effective_context_window_tokens: number;
  effective_max_output_tokens?: number | null;
  endpoint_id: EndpointId;
  execution_readiness: ModelExecutionReadinessView;
  model_profile_id: ModelProfileId;
  order: number;
  provider_model_name: string;
  /**
   * The level its profile asks for; none is the provider's default.
   */
  reasoning_effort?: ReasoningEffort | null;
  /**
   * The thinking levels the model offers, lowest first; empty when it offers none.
   */
  reasoning_efforts?: ReasoningEffort[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SessionModelProviderView".
 */
export interface SessionModelProviderView {
  display_name: string;
  order: number;
  provider_id: ProviderId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelAvailabilityTestProjection".
 */
export interface ModelAvailabilityTestProjection {
  model_availability_test_id: ModelAvailabilityTestId;
  test: ModelAvailabilityTestView;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelDiscoveryProjection".
 */
export interface ModelDiscoveryProjection {
  model_discovery_run_id: ModelDiscoveryRunId;
  run: ModelDiscoveryRunView;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextWindowCorrectionProjection".
 */
export interface ContextWindowCorrectionProjection {
  active_corrections: ContextWindowCorrectionView[];
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextWindowCorrectionView".
 */
export interface ContextWindowCorrectionView {
  agent_run_id: AgentRunId;
  failed_execution_snapshot_id: ModelExecutionSnapshotId;
  model_profile_id: ModelProfileId;
  model_step_id: ModelStepId;
  provider_error?: ProviderErrorSummaryView | null;
  resolution: ContextWindowResolution;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextWindowTokenInputContract".
 */
export interface ContextWindowTokenInputContract {
  maximum_tokens?: number | null;
  minimum_tokens: number;
  unit: ContextWindowUnit;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolSettingsProjection".
 */
export interface ToolSettingsProjection {
  configuration_issue?: ToolProjectionErrorView | null;
  settings: ToolSettingsSnapshotV1;
}
/**
 * Renderer-safe error facts. Internal diagnostic details and raw failures are
 * intentionally absent from every P07 projection.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolProjectionErrorView".
 */
export interface ToolProjectionErrorView {
  category: ErrorCategory;
  code: ErrorCode;
  message: string;
  retryable: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolSettingsSnapshotV1".
 */
export interface ToolSettingsSnapshotV1 {
  content_hash: CanonicalSha256HashV1;
  default_timeout_ms: number;
  max_concurrent_tool_calls: number;
  max_dynamic_tool_schemas: number;
  max_retries: number;
  /**
   * Absent in pre-L01 snapshots. Absence preserves their original serialization and hash.
   */
  network_proxy?: ToolNetworkProxyV1 | null;
  revision: Revision;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolNetworkProxyV1".
 */
export interface ToolNetworkProxyV1 {
  enabled: boolean;
  host: string;
  port: number;
  protocol: ToolNetworkProxyProtocolV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageListProjection".
 */
export interface ToolPackageListProjection {
  packages: ToolPackageSummaryView[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageSummaryView".
 */
export interface ToolPackageSummaryView {
  active_version?: CanonicalSemVer | null;
  deletion_pending: boolean;
  eligible_tool_count: number;
  enabled_tool_count: number;
  entity_revision: Revision;
  lifecycle: ToolPackageLifecycleV1;
  package_key: PackageKey;
  tool_count: number;
  tool_package_id: ToolPackageId;
  trusted: boolean;
  updated_at: UtcTimestamp;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDraftListProjection".
 */
export interface ToolDraftListProjection {
  drafts: ToolDraftSummaryView[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDraftSummaryView".
 */
export interface ToolDraftSummaryView {
  entity_revision: Revision;
  kind: ToolDraftKindV1;
  safe_error?: ToolProjectionErrorView | null;
  status: ToolDraftStatusV1;
  tool_draft_id: ToolDraftId;
  updated_at: UtcTimestamp;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceToolPinsProjection".
 */
export interface WorkspaceToolPinsProjection {
  pin_set_revision: Revision;
  pins: ToolPinProjectionEntryV1[];
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPinProjectionEntryV1".
 */
export interface ToolPinProjectionEntryV1 {
  pin_revision: Revision;
  resolved_binding?: EffectiveToolBindingSnapshot | null;
  safe_issue?: ToolProjectionErrorView | null;
  state: ToolPinResolutionStateV1;
  tool_id: ToolId;
  tool_version: ToolVersion;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceToolRegistrySummaryProjection".
 */
export interface WorkspaceToolRegistrySummaryProjection {
  capacity_remediation_required: boolean;
  discoverable_tool_count: number;
  exposed_tool_count: number;
  registry_snapshot_ref?: ToolRegistrySnapshotRef | null;
  safe_issue?: ToolProjectionErrorView | null;
  unresolved_pin_count: number;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SessionEffectiveToolRegistryProjection".
 */
export interface SessionEffectiveToolRegistryProjection {
  registry_snapshot_ref: ToolRegistrySnapshotRef;
  session_id: SessionId;
  tools: EffectiveToolView[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "EffectiveToolView".
 */
export interface EffectiveToolView {
  audience: ToolAudienceV1;
  canonical_name: ToolCanonicalName;
  display_name: string;
  eligible: boolean;
  enabled: boolean;
  exposure: ToolExposureV1;
  pin_state?: ToolPinResolutionStateV1 | null;
  runtime_health: ToolRuntimeHealthV1;
  safe_ineligibility_reason?: ToolProjectionErrorView | null;
  source: ToolSourceKindV1;
  summary: string;
  tool_id: ToolId;
  tool_instance_id: ToolInstanceId;
  tool_version: ToolVersion;
  trusted: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SessionLoadedToolsProjection".
 */
export interface SessionLoadedToolsProjection {
  registry_snapshot_ref: ToolRegistrySnapshotRef;
  session_id: SessionId;
  tools: EffectiveToolView[];
  turn_task_id: TurnTaskId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SessionToolActivityProjection".
 */
export interface SessionToolActivityProjection {
  active_batches: ToolBatchActivitySummaryView[];
  older_activity_cursor?: ToolActivityCursorV1 | null;
  recent_calls: ToolCallActivitySummaryView[];
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolBatchActivitySummaryView".
 */
export interface ToolBatchActivitySummaryView {
  call_count: number;
  can_cancel: boolean;
  completed_at?: UtcTimestamp | null;
  completed_call_count: number;
  started_at?: UtcTimestamp | null;
  status: ToolBatchStatusV1;
  tool_batch_id: ToolBatchId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCallActivitySummaryView".
 */
export interface ToolCallActivitySummaryView {
  can_cancel: boolean;
  completed_at?: UtcTimestamp | null;
  display_name: string;
  /**
   * The classified reason for a terminal outcome, when the call recorded one.
   */
  error_class?: ToolCallErrorClassV1 | null;
  ordinal: number;
  outcome?: ToolResultOutcomeSummaryView | null;
  side_effect_state?: SideEffectStateV1 | null;
  started_at?: UtcTimestamp | null;
  status: ToolCallStatusV1;
  tool_batch_id: ToolBatchId;
  tool_call_id: ToolCallId;
  /**
   * Null only for a ToolNotFound call whose Provider name was absent from the frozen request.
   */
  tool_id: ToolId | null;
  tool_version: ToolVersion | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SessionToolPinsProjectionV1".
 */
export interface SessionToolPinsProjectionV1 {
  pin_set_revision: Revision;
  pins: ToolPinProjectionEntryV1[];
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageProjection".
 */
export interface ToolPackageProjection {
  active_security_fingerprint?: CanonicalBlake3HashV1 | null;
  active_version?: CanonicalSemVer | null;
  created_at: UtcTimestamp;
  deletion_pending: boolean;
  entity_revision: Revision;
  lifecycle: ToolPackageLifecycleV1;
  package_key: PackageKey;
  safe_error?: ToolProjectionErrorView | null;
  tool_package_id: ToolPackageId;
  tools: ToolDefinitionSummaryView[];
  trusted_security_fingerprint?: CanonicalBlake3HashV1 | null;
  updated_at: UtcTimestamp;
  versions: ToolPackageVersionSummaryView[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CanonicalBlake3HashV1".
 */
export interface CanonicalBlake3HashV1 {
  algorithm: 'blake3';
  canonicalization_version: 'rfc8785_jcs_v1';
  hex: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDefinitionSummaryView".
 */
export interface ToolDefinitionSummaryView {
  audience: ToolAudienceV1;
  canonical_name: ToolCanonicalName;
  display_name: string;
  entity_revision: Revision;
  instances: ToolDefinitionInstanceView[];
  model_summary: string;
  tool_id: ToolId;
  tool_version: ToolVersion;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDefinitionInstanceView".
 */
export interface ToolDefinitionInstanceView {
  configuration?: ToolInstanceConfigurationProjectionV1 | null;
  credential_slot_binding?: ToolCredentialSlotBindingSummaryV1 | null;
  enabled: boolean;
  entity_revision: Revision;
  ready: boolean;
  runtime_health: ToolRuntimeHealthV1;
  tool_instance_id: ToolInstanceId;
  tool_version: ToolVersion;
  trusted: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolInstanceConfigurationProjectionV1".
 */
export interface ToolInstanceConfigurationProjectionV1 {
  configuration_hash: CanonicalSha256HashV1;
  configuration_revision: Revision;
  safe_non_secret_value: any;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCredentialSlotBindingSummaryV1".
 */
export interface ToolCredentialSlotBindingSummaryV1 {
  bound_credential?: ToolBoundCredentialSummaryV1 | null;
  placement: ToolCredentialPlacementV1;
  rebind_required_reason?: string | null;
  slot_key: CredentialSlotKey;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolBoundCredentialSummaryV1".
 */
export interface ToolBoundCredentialSummaryV1 {
  binding_revision: Revision;
  credential_id: CredentialId;
  /**
   * Secret-generation revision for one stable CredentialId.
   *
   * This is intentionally distinct from a row revision. Clear and metadata-only
   * mutations may change row concurrency without creating a new secret
   * generation.
   */
  credential_revision: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageVersionSummaryView".
 */
export interface ToolPackageVersionSummaryView {
  conformance_run_ids: ToolConformanceRunId[];
  conformance_status: ToolConformanceStatusV1;
  installed_at: UtcTimestamp;
  package_fingerprint: CanonicalSha256HashV1;
  package_security_fingerprint: CanonicalBlake3HashV1;
  package_version: CanonicalSemVer;
  payload_available: boolean;
  payload_gc_eligible_at?: UtcTimestamp | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDefinitionProjection".
 */
export interface ToolDefinitionProjection {
  canonical_name: ToolCanonicalName;
  current_selection: ToolDefinitionCurrentSelectionV1;
  display_name: string;
  entity_revision: Revision;
  safe_error?: ToolProjectionErrorView | null;
  tool_id: ToolId;
  versions: ToolDefinitionVersionView[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "McpToolSourceAuthorityRefV1".
 */
export interface McpToolSourceAuthorityRefV1 {
  authority_revision: Revision;
  normalized_mcp_tool_contract_fingerprint: CanonicalSha256HashV1;
  source_contract_key: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PluginToolSourceAuthorityRefV1".
 */
export interface PluginToolSourceAuthorityRefV1 {
  authority_revision: Revision;
  normalized_plugin_tool_contract_fingerprint: CanonicalSha256HashV1;
  source_contract_key: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDefinitionVersionView".
 */
export interface ToolDefinitionVersionView {
  audience: ToolAudienceV1;
  capability_count: number;
  execution_semantics: ToolExecutionSemanticsV1;
  instance_configuration_schema: CanonicalToolPackageInstanceConfigurationSchemaV1;
  instances: ToolDefinitionInstanceView[];
  model_description: string;
  model_summary: string;
  payload_available: boolean;
  resource_claim_count: number;
  result_contract: ToolResultContractV1;
  schema: ToolSchemaSnapshotV1;
  source: ToolSourceKindV1;
  tool_version: ToolVersion;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolExecutionSemanticsV1".
 */
export interface ToolExecutionSemanticsV1 {
  /**
   * The definition explicitly permits an automatic retry after all Core
   * safety gates have also passed.
   */
  automatic_retry_allowed: boolean;
  /**
   * The runtime can create a persistent effect outside its attempt-local
   * staging area. Actual SideEffectState remains Core-derived evidence.
   */
  may_create_persistent_effects: boolean;
  /**
   * Repeating the same normalized invocation is declared idempotent.
   */
  repeat_is_idempotent: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CanonicalJsonSchemaDocumentV1".
 */
export interface CanonicalJsonSchemaDocumentV1 {
  document: {};
  schema_dialect: 'json_schema_draft_2020_12';
  schema_hash: {
    algorithm: 'sha256';
    canonicalization_version: 'rfc8785_jcs_v1';
    hex: string;
  };
  schema_policy_version: 1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDraftProjection".
 */
export interface ToolDraftProjection {
  approval?: ToolDraftApprovalEvidenceV1 | null;
  can_cancel: boolean;
  conformance_run_id?: ToolConformanceRunId | null;
  created_at: UtcTimestamp;
  current_draft_revision: Revision;
  draft_hash: CanonicalSha256HashV1;
  entity_revision: Revision;
  kind: ToolDraftKindV1;
  package_delete_approval?: ToolPackageDeleteApprovalEvidenceV1 | null;
  package_delete_snapshot?: ToolPackageDeleteDraftSnapshotV1 | null;
  preview?: ToolDraftPreviewV1 | null;
  safe_error?: ToolProjectionErrorView | null;
  status: ToolDraftStatusV1;
  tool_draft_id: ToolDraftId;
  updated_at: UtcTimestamp;
  user_readable_artifacts: UserReadableToolArtifactView[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDraftApprovalEvidenceV1".
 */
export interface ToolDraftApprovalEvidenceV1 {
  actor: ToolDraftActorV1;
  canonical_manifest_hash: CanonicalSha256HashV1;
  conformance_run_ids: ToolConformanceRunId[];
  decided_at: UtcTimestamp;
  decision: ToolDraftApprovalDecisionV1;
  draft: ConformanceDraftRevisionRefV1;
  evidence_hash: CanonicalSha256HashV1;
  package_fingerprint: CanonicalSha256HashV1;
  package_security_fingerprint: CanonicalBlake3HashV1;
  preview_hash: CanonicalSha256HashV1;
  runtime_closure_hash: CanonicalSha256HashV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ConformanceDraftRevisionRefV1".
 */
export interface ConformanceDraftRevisionRefV1 {
  draft_revision: Revision;
  draft_snapshot_hash: CanonicalSha256HashV1;
  tool_draft_id: ToolDraftId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageDeleteApprovalEvidenceV1".
 */
export interface ToolPackageDeleteApprovalEvidenceV1 {
  decided_at: UtcTimestamp;
  decision: ToolPackageDeleteApprovalDecisionV1;
  draft: ConformanceDraftRevisionRefV1;
  evidence_hash: CanonicalSha256HashV1;
  preview_hash: CanonicalSha256HashV1;
  target_snapshot_hash: CanonicalSha256HashV1;
  user_configuration_operation_identity_binding_hash: ContentHash2;
}
/**
 * Hash of the non-deserializable Core-owned
 * `UserConfigurationOperationIdentity` used at the authenticated Main
 * configuration boundary.
 */
export interface ContentHash2 {
  algorithm: ContentHashAlgorithm;
  hex: string;
}
/**
 * Immutable package-deletion Draft revision. The four collection hashes are
 * produced from Core-owned SQLite facts; this contract intentionally does not
 * accept Renderer-supplied impact or runtime-reference collections.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageDeleteDraftSnapshotV1".
 */
export interface ToolPackageDeleteDraftSnapshotV1 {
  active_package_version: CanonicalSemVer;
  created_at: UtcTimestamp;
  deletion_policy_version: number;
  draft: ConformanceDraftRevisionRefV1;
  initiator: ToolDraftActorV1;
  installed_package_versions_hash: CanonicalSha256HashV1;
  package_fingerprint: CanonicalSha256HashV1;
  package_revision: Revision;
  package_security_fingerprint: CanonicalBlake3HashV1;
  parent_revision?: Revision | null;
  persistent_impact_hash: CanonicalSha256HashV1;
  preview_hash: CanonicalSha256HashV1;
  runtime_strong_reference_observed_summary_hash: CanonicalSha256HashV1;
  tool_identity_set_hash: CanonicalSha256HashV1;
  tool_package_id: ToolPackageId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDraftPreviewV1".
 */
export interface ToolDraftPreviewV1 {
  all_conformance_passed: boolean;
  canonical_manifest: CanonicalToolPackageManifestV1;
  canonical_manifest_hash: CanonicalSha256HashV1;
  conformance: ToolDraftConformanceSummaryV1[];
  draft: ConformanceDraftRevisionRefV1;
  file_changes: ToolDraftFileChangeV1[];
  generated_at: UtcTimestamp;
  package_fingerprint: CanonicalSha256HashV1;
  package_security_fingerprint: CanonicalBlake3HashV1;
  preview_hash: CanonicalSha256HashV1;
  runtime_closure_hash: CanonicalSha256HashV1;
}
/**
 * Normalized contract used as the future fingerprint input. Defaults are
 * explicit and schemas are already represented by their canonical P07 forms.
 * The pure conversion method is Contract-only and is not wired to a file
 * parser, Package Store, Trust, Conformance, Journal, or Eligible Registry.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CanonicalToolPackageManifestV1".
 */
export interface CanonicalToolPackageManifestV1 {
  /**
   * @maxItems 4095
   */
  files: ToolPackageFileEntryV1[];
  minimum_aura_version: CanonicalSemVer;
  package_key: PackageKey;
  package_version: CanonicalSemVer;
  schema_version: ToolPackageManifestSchemaVersionV1;
  /**
   * @minItems 1
   */
  tools: [CanonicalToolPackageToolV1, ...CanonicalToolPackageToolV1[]];
  ui: CanonicalToolPackageUiV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageFileEntryV1".
 */
export interface ToolPackageFileEntryV1 {
  byte_length: number;
  /**
   * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
   * via the `definition` "PackageRelativePath".
   */
  path: PackageRelativePath;
  role: ToolPackageFileRoleV1;
  sha256: ToolPackageFileSha256V1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageFileSha256V1".
 */
export interface ToolPackageFileSha256V1 {
  algorithm: 'sha256';
  hex: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CanonicalToolPackageToolV1".
 */
export interface CanonicalToolPackageToolV1 {
  audience: PackageToolAudienceV1;
  capability_requirements: PackageCapabilityRequirementV1[];
  /**
   * @minItems 1
   */
  conformance_cases: [ToolPackageConformanceCaseV1, ...ToolPackageConformanceCaseV1[]];
  execution_semantics: ToolExecutionSemanticsV1;
  instance_configuration: CanonicalJsonSchemaDocumentV1 | null;
  model: CanonicalToolPackageModelContractV1;
  resource_claims: PackageDeclaredResourceClaimV1[];
  runtime: CanonicalToolPackageRuntimeV1;
  semantic_version: CanonicalToolSemanticVersionV1;
  tool_key: ToolKey;
  ui: ToolPackageToolUiV1;
}
/**
 * A V1 Package conformance case is deliberately narrow: it supplies one
 * Package-argument object to the owning Tool. Result interpretation remains
 * governed by that Tool's declared Result Contract; this draft does not add a
 * second assertion language.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageConformanceCaseV1".
 */
export interface ToolPackageConformanceCaseV1 {
  arguments: ToolPackageJsonObjectV1;
  name: ToolPackageConformanceCaseNameV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageJsonObjectV1".
 */
export interface ToolPackageJsonObjectV1 {
  [k: string]: any | undefined;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CanonicalToolPackageModelContractV1".
 */
export interface CanonicalToolPackageModelContractV1 {
  canonical_name: ToolCanonicalName;
  description: ToolPackageModelDescriptionV1;
  input_schema: CanonicalJsonSchemaDocumentV1;
  result_contract: ToolResultContractV1;
  strict: boolean;
  summary: ToolPackageModelSummaryV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CommandCredentialEnvironmentSlotV1".
 */
export interface CommandCredentialEnvironmentSlotV1 {
  environment_variable: EnvironmentVariableNameV1;
  slot_key: CredentialSlotKey;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CommandEnvironmentEntryV1".
 */
export interface CommandEnvironmentEntryV1 {
  name: EnvironmentVariableNameV1;
  value: CommandEnvironmentValueV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CanonicalToolPackageDeclarativeHttpV1".
 */
export interface CanonicalToolPackageDeclarativeHttpV1 {
  body: DeclarativeHttpBodyV1;
  credential_slot: HttpCredentialSlotV1 | null;
  headers: HttpHeaderEntryV1[];
  method: DeclarativeHttpMethodV1;
  path: HttpPathSegmentV1[];
  query: HttpQueryEntryV1[];
  success_status_ranges: ToolPackageHttpSuccessStatusRangesV1;
  target: DeclarativeHttpTargetV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HttpJsonPropertyV1".
 */
export interface HttpJsonPropertyV1 {
  name: HttpJsonPropertyNameV1;
  value: HttpJsonTemplateV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HttpHeaderEntryV1".
 */
export interface HttpHeaderEntryV1 {
  name: HttpHeaderNameV1;
  value: HttpHeaderValueSourceV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HttpQueryEntryV1".
 */
export interface HttpQueryEntryV1 {
  name: HttpQueryNameV1;
  value: HttpValueSourceV1;
}
/**
 * UI-only metadata. It is deliberately separate from model-visible text and
 * therefore is not reused as a Provider description.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageToolUiV1".
 */
export interface ToolPackageToolUiV1 {
  display_name: ToolPackageToolDisplayNameV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CanonicalToolPackageUiV1".
 */
export interface CanonicalToolPackageUiV1 {
  description: ToolPackageUiDescriptionV1 | null;
  display_name: ToolPackageUiDisplayNameV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDraftConformanceSummaryV1".
 */
export interface ToolDraftConformanceSummaryV1 {
  passed: boolean;
  tool_conformance_run_id: ToolConformanceRunId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDraftFileChangeV1".
 */
export interface ToolDraftFileChangeV1 {
  byte_length?: number | null;
  change: ToolDraftFileChangeKindV1;
  /**
   * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
   * via the `definition` "PackageRelativePath".
   */
  path: PackageRelativePath;
  raw_file_sha256?: ToolPackageFileSha256V1 | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "UserReadableToolArtifactView".
 */
export interface UserReadableToolArtifactView {
  artifact_id: ArtifactId;
  byte_length: number;
  media_type: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolConformanceRunProjection".
 */
export interface ToolConformanceRunProjection {
  can_cancel: boolean;
  completed_at?: UtcTimestamp | null;
  entity_revision: Revision;
  package_fingerprint: CanonicalSha256HashV1;
  safe_error?: ToolProjectionErrorView | null;
  started_at?: UtcTimestamp | null;
  status: ToolConformanceStatusV1;
  tool_conformance_run_id: ToolConformanceRunId;
  tool_draft_id?: ToolDraftId | null;
  user_readable_artifacts: UserReadableToolArtifactView[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolBatchProjection".
 */
export interface ToolBatchProjection {
  agent_run_id: AgentRunId;
  batch_content_hash: CanonicalSha256HashV1;
  calls: ToolCallActivitySummaryView[];
  can_cancel: boolean;
  completed_at?: UtcTimestamp | null;
  control_state_hash: CanonicalSha256HashV1;
  control_state_revision: Revision;
  model_step_id: ModelStepId;
  registry_snapshot_ref: ToolRegistrySnapshotRef;
  safe_error?: ToolProjectionErrorView | null;
  session_id: SessionId;
  started_at?: UtcTimestamp | null;
  status: ToolBatchStatusV1;
  tool_batch_id: ToolBatchId;
  turn_task_id: TurnTaskId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCallProjection".
 */
export interface ToolCallProjection {
  attempt_ids: ToolExecutionAttemptId[];
  can_cancel: boolean;
  completed_at?: UtcTimestamp | null;
  /**
   * Null only for a ToolNotFound call whose Provider name was absent from the frozen request.
   */
  definition_ref: ToolDefinitionSnapshotRef | null;
  invalid_input_issue?: NormalizedToolCallInputIssueV1 | null;
  model_visible_result?: UserReadableToolArtifactView | null;
  ordinal: number;
  outcome?: ToolResultOutcomeSummaryView | null;
  purpose?: string | null;
  safe_error?: ToolProjectionErrorView | null;
  side_effect_state?: SideEffectStateV1 | null;
  started_at?: UtcTimestamp | null;
  status: ToolCallStatusV1;
  tool_batch_id: ToolBatchId;
  tool_call_id: ToolCallId;
  user_audit_result?: UserReadableToolArtifactView | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolExecutionAttemptProjection".
 */
export interface ToolExecutionAttemptProjection {
  attempt_number: number;
  can_cancel: boolean;
  completed_at?: UtcTimestamp | null;
  dispatch_prepared_at?: UtcTimestamp | null;
  progress?: ToolAttemptProgressView | null;
  safe_error?: ToolProjectionErrorView | null;
  side_effect_state: SideEffectStateV1;
  started_at: UtcTimestamp;
  status: ToolExecutionAttemptStatusV1;
  tool_call_id: ToolCallId;
  tool_execution_attempt_id: ToolExecutionAttemptId;
  user_readable_artifacts: UserReadableToolArtifactView[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolAttemptProgressView".
 */
export interface ToolAttemptProgressView {
  message?: string | null;
  percentage?: number | null;
  sequence: number;
  updated_at: UtcTimestamp;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolLongOperationProjectionV1".
 */
export interface ToolLongOperationProjectionV1 {
  can_cancel: boolean;
  completed_at?: UtcTimestamp | null;
  operation: ToolLongOperationRefV1;
  operational_revision: Revision;
  safe_error?: ToolProjectionErrorView | null;
  started_at?: UtcTimestamp | null;
  state: ToolLongOperationStateV1;
}
/**
 * Schema-only aggregation root owned by the projection contract crate.
 *
 * Runtime messages use the individual contracts below; this type prevents
 * the repository-wide schema generator from recreating their ownership.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProjectionContractSchemaRoot".
 */
export interface ProjectionContractSchemaRoot {
  compatibility_snapshot: ProjectionCompatibilitySnapshotV1;
  projection_change: ProjectionChange;
  projection_key: ProjectionKey;
  projection_snapshot_request: ProjectionSnapshotRequest;
  projection_subscription: ProjectionSubscription;
  projection_unsubscribe_request: ProjectionUnsubscribeRequest;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProjectionCompatibilitySnapshotV1".
 */
export interface ProjectionCompatibilitySnapshotV1 {
  contract_version: number;
  paused_projections: ProjectionCompatibilityKey[];
  revision: Revision;
  runtime_instance_id: RuntimeInstanceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RuntimeContractFixture".
 */
export interface RuntimeContractFixture {
  active_install: ActiveInstallState;
  authenticated_peer_identity: AuthenticatedPeerIdentity;
  binary_stream_frame: BinaryStreamFrame;
  cancellation: CancellationRequest;
  command_outcome: CommandOutcome;
  frame_header: IpcFrameHeader;
  handshake_complete: RuntimeHandshakeComplete;
  handshake_finish: RuntimeHandshakeFinish;
  handshake_request: RuntimeHandshakeRequest;
  handshake_response: RuntimeHandshakeResponse;
  handshake_transcript: RuntimeHandshakeTranscript;
  heartbeat: RuntimeHeartbeat;
  ipc_error: IpcErrorEnvelope;
  projection_change: ProjectionChange;
  projection_keys: ProjectionKey[];
  projection_snapshot_request: ProjectionSnapshotRequest;
  projection_subscription: ProjectionSubscription;
  projection_unsubscribe_request: ProjectionUnsubscribeRequest;
  startup_progress: CoreStartupProgress;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ActiveInstallState".
 */
export interface ActiveInstallState {
  active: string;
  last_known_good?: string | null;
  pending?: string | null;
  previous?: string | null;
  release_descriptor: ReleaseDescriptor;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ReleaseDescriptor".
 */
export interface ReleaseDescriptor {
  bundle_namespace: string;
  identity: ReleaseIdentity;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ReleaseIdentity".
 */
export interface ReleaseIdentity {
  component_manifest_hash: ContentHash;
  release_bundle_id: ReleaseBundleId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AuthenticatedPeerIdentity".
 */
export interface AuthenticatedPeerIdentity {
  process_creation_time_100ns: number;
  process_id: number;
  release_descriptor: ReleaseDescriptor;
  runtime_instance_id: RuntimeInstanceId;
  windows_session_id: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BinaryStreamFrame".
 */
export interface BinaryStreamFrame {
  chunk_sequence: number;
  data_length: number;
  end_of_message: boolean;
  stream_id: StreamId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CancellationRequest".
 */
export interface CancellationRequest {
  request_id: CorrelationId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "IpcFrameHeader".
 */
export interface IpcFrameHeader {
  channel_id: IpcChannelId;
  connection_epoch: number;
  sequence: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RuntimeHandshakeComplete".
 */
export interface RuntimeHandshakeComplete {
  channel_id: IpcChannelId;
  connection_epoch: number;
  handshake_kind: RuntimeHandshakeKind;
  transcript_hash: ContentHash;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RuntimeHandshakeFinish".
 */
export interface RuntimeHandshakeFinish {
  bootstrap_proof?: SecretNonce | null;
  channel_id: IpcChannelId;
  connection_epoch: number;
  handshake_kind: RuntimeHandshakeKind;
  transcript_hash: ContentHash;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RuntimeHandshakeRequest".
 */
export interface RuntimeHandshakeRequest {
  channel_id: IpcChannelId;
  client_nonce: SecretNonce;
  connection_epoch: number;
  handshake: RuntimeHandshake;
  handshake_kind: RuntimeHandshakeKind;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RuntimeHandshake".
 */
export interface RuntimeHandshake {
  build_commit: string;
  component_version: string;
  parent_runtime_instance_id?: RuntimeInstanceId | null;
  process_kind: RuntimeProcessKind;
  protocol_version: ContractVersion;
  release_descriptor: ReleaseDescriptor;
  runtime_instance_id: RuntimeInstanceId;
  supported_features: string[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RuntimeHandshakeResponse".
 */
export interface RuntimeHandshakeResponse {
  channel_id: IpcChannelId;
  client_nonce: SecretNonce;
  connection_epoch: number;
  handshake: RuntimeHandshake;
  handshake_kind: RuntimeHandshakeKind;
  negotiated_protocol_version: ContractVersion;
  server_nonce: SecretNonce;
  transcript_hash: ContentHash;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RuntimeHandshakeTranscript".
 */
export interface RuntimeHandshakeTranscript {
  channel_id: IpcChannelId;
  client_handshake: RuntimeHandshake;
  client_nonce: SecretNonce;
  connection_epoch: number;
  handshake_kind: RuntimeHandshakeKind;
  negotiated_protocol_version: ContractVersion;
  server_handshake: RuntimeHandshake;
  server_nonce: SecretNonce;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RuntimeHeartbeat".
 */
export interface RuntimeHeartbeat {
  runtime_instance_id: RuntimeInstanceId;
  sent_at: UtcTimestamp;
  sequence: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "IpcErrorEnvelope".
 */
export interface IpcErrorEnvelope {
  category: ErrorCategory;
  code: ErrorCode;
  correlation_id: CorrelationId;
  details: {
    [k: string]: any | undefined;
  };
  message: string;
  retryable: boolean;
  terminal: IpcRequestTerminal;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CoreStartupProgress".
 */
export interface CoreStartupProgress {
  message: string;
  occurred_at: UtcTimestamp;
  phase: CoreStartupPhase;
  runtime_instance_id: RuntimeInstanceId;
  sequence: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SecurityContractFixture".
 */
export interface SecurityContractFixture {
  approval_details?: GetApprovalDetailsInput | null;
  approval_request?: ApprovalRequest | null;
  approval_resolution?: ApprovalResolution | null;
  approval_resolve?: ResolveApprovalInput | null;
  authorized_network_request?: AuthorizedNetworkRequest | null;
  authorized_tool_sandbox_execution?: AuthorizedToolSandboxExecutionV1 | null;
  conformance_sandbox_profile?: ConformanceSandboxProfileV1 | null;
  credential_reflection_scan?: CredentialReflectionScanEvidenceV1 | null;
  external_extension_risk?: ExternalExtensionRiskView | null;
  external_grant_attach_read_only?: AttachExternalReadOnlyInput | null;
  external_grant_derive_workspace?: DeriveWorkspaceExternalGrantInput | null;
  external_grant_list?: ListSessionExternalGrantsInput | null;
  external_grant_record?: ExternalGrantRecord | null;
  external_grant_revalidate?: RevalidateExternalGrantInput | null;
  external_grant_revoke?: RevokeExternalGrantInput | null;
  external_grant_view?: ExternalGrantView | null;
  external_process_launch_plan?: ExternalProcessLaunchPlan | null;
  file_session_consent_list?: ListFileSessionConsentsInput | null;
  file_session_consent_revoke?: RevokeFileSessionConsentInput | null;
  file_session_consent_view?: FileSessionConsentView | null;
  get_session_state?: GetSessionPermissionStateInput | null;
  host_preparation_begin?: HostPreparationBeginResult | null;
  host_preparation_mutation?: HostPreparationMutationInput | null;
  host_preparation_resolve?: ResolveHostPreparationInput | null;
  host_preparation_status?: HostPreparationStatus | null;
  list_grants?: ListPermissionGrantsInput | null;
  model_channel_data_disposition: ModelChannelDataDisposition;
  model_egress_policy_version: number;
  native_session_consent_list?: ListNativeSessionConsentsInput | null;
  native_session_consent_revoke?: RevokeNativeSessionConsentInput | null;
  native_session_consent_scope?: NativeSessionConsentScopeView | null;
  native_session_consent_view?: NativeSessionConsentView | null;
  network_audit?: NetworkRequestAuditRecord | null;
  network_grant?: NetworkGrant | null;
  network_request_plan?: NetworkRequestPlan | null;
  operation_security_plan?: OperationSecurityPlan | null;
  permission_decision?: PermissionDecision | null;
  permission_defaults?: PermissionDefaults | null;
  permission_grant?: PermissionGrant | null;
  permission_snapshot?: PermissionSnapshot | null;
  projection_change?: SecurityProjectionChange | null;
  quarantine_discard?: DiscardQuarantineInput | null;
  quarantine_discard_confirmation?: QuarantineDiscardConfirmation | null;
  quarantine_issue_discard_confirmation?: IssueQuarantineDiscardConfirmationInput | null;
  quarantine_prepare_release?: PrepareQuarantineReleaseInput | null;
  quarantine_prepare_release_result?: PrepareQuarantineReleaseResult | null;
  quarantine_record?: QuarantineRecordDetails | null;
  quarantine_release?: ReleaseQuarantineInput | null;
  revoke_grant?: RevokePermissionGrantInput | null;
  sandbox_execution_plan?: SandboxExecutionPlan | null;
  sandbox_host_event?: SandboxHostEvent | null;
  sandbox_network_policy_error?: SandboxNetworkPolicyError | null;
  set_permission_defaults?: SetPermissionDefaultsInput | null;
  set_session_mode?: SetSessionPermissionModeInput | null;
  tool_conformance_runtime_scope?: ToolConformanceRuntimeScopeV1 | null;
  tool_conformance_source?: ToolConformanceSecuritySourceRefV1 | null;
  tool_sandbox_dispatch_binding?: ToolSandboxDispatchBindingV1 | null;
  trust_fingerprint?: TrustFingerprintInput | null;
  trust_record?: ExternalTrustRecord | null;
  w7_tool_operation?: W7ToolOperationFingerprintPreimageV1 | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GetApprovalDetailsInput".
 */
export interface GetApprovalDetailsInput {
  approval_id: ApprovalRequestId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ApprovalRequest".
 */
export interface ApprovalRequest {
  actual_operation_summary: string;
  allowed_decisions: ApprovalDecisionKind[];
  capabilities: ResolvedCapability[];
  created_at: UtcTimestamp;
  id: ApprovalRequestId;
  identity: InvocationIdentity;
  invalidation_reason?: string | null;
  model_purpose?: string | null;
  operation_fingerprints: ContentHash[];
  operation_ids: SecurityOperationId[];
  permission_snapshot_id: PermissionSnapshotId;
  release_descriptor?: ReleaseDescriptor | null;
  request_revision: number;
  resolved_at?: UtcTimestamp | null;
  risk_flags: OperationRiskFlag[];
  scopes: ResolvedScope[];
  source: SecuritySourceV2;
  source_fingerprint?: ContentHash | null;
  source_summary: string;
  state: ApprovalState;
  title: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "InvocationIdentity".
 */
export interface InvocationIdentity {
  actor: InvocationActor;
  agent_run_id?: AgentRunId | null;
  correlation_id: CorrelationId;
  parent_agent_run_id?: AgentRunId | null;
  permission_snapshot_id?: PermissionSnapshotId | null;
  principal_id: PrincipalId;
  session_id?: SessionId | null;
  turn_task_id?: TurnTaskId | null;
  workspace_id?: WorkspaceId | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CanonicalPath".
 */
export interface CanonicalPath {
  display: string;
  utf16: number[];
}
/**
 * Exact immutable Package Runtime resource authorized for one P07 Tool
 * operation. This scope grants access only to the verified full-package
 * Runtime Root identified by these three fields; it does not imply access to
 * a Workspace, external path, system path, network, or Credential.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageRuntimeScopeV1".
 */
export interface PackageRuntimeScopeV1 {
  package_version: CanonicalSemVer;
  runtime_closure_hash: CanonicalSha256HashV1;
  tool_package_id: ToolPackageId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolConformanceRuntimeScopeV1".
 */
export interface ToolConformanceRuntimeScopeV1 {
  conformance: ToolConformanceSecuritySourceRefV1;
}
/**
 * Exact authority identity for one uninstalled Draft/Quarantine Conformance
 * run. It cannot be interpreted as an installed Tool or Registry binding.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolConformanceSecuritySourceRefV1".
 */
export interface ToolConformanceSecuritySourceRefV1 {
  canonical_manifest_hash: CanonicalSha256HashV1;
  conformance_binding_hash: CanonicalSha256HashV1;
  conformance_case_name: ToolPackageConformanceCaseNameV1;
  draft_revision: Revision;
  draft_snapshot_hash: CanonicalSha256HashV1;
  runtime_closure_hash: CanonicalSha256HashV1;
  tool_conformance_run_id: ToolConformanceRunId;
  tool_contract_fingerprint: ToolContractFingerprintV1;
  tool_draft_id: ToolDraftId;
  tool_key: ToolKey;
}
/**
 * Exact Tool identity accepted by P04 for one concrete ToolCall. Production
 * construction is additionally gated by P07 Batch Admission against the
 * unique EffectiveToolBindingSnapshot in the same immutable `trs_`.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolSecuritySourceRefV1".
 */
export interface ToolSecuritySourceRefV1 {
  definition_fingerprint: CanonicalSha256HashV1;
  definition_ref: ToolDefinitionSnapshotRef;
  source_contract_fingerprint: ToolSourceContractFingerprintV1;
  source_kind: ToolSourceKindV1;
  tool_instance_id: ToolInstanceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ApprovalResolution".
 */
export interface ApprovalResolution {
  approval_id: ApprovalRequestId;
  decision: ApprovalDecisionKind;
  decision_id: ApprovalDecisionId;
  grant_ids: PermissionGrantId[];
  resolved_at: UtcTimestamp;
  state: ApprovalState;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ResolveApprovalInput".
 */
export interface ResolveApprovalInput {
  approval_id: ApprovalRequestId;
  decision: ApprovalDecisionKind;
  expected_request_revision: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AuthorizedNetworkRequest".
 */
export interface AuthorizedNetworkRequest {
  destination: ResolvedNetworkDestination;
  matched_grant_ids: NetworkGrantId[];
  plan: NetworkRequestPlan;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ResolvedNetworkDestination".
 */
export interface ResolvedNetworkDestination {
  addresses: string[];
  classes: NetworkDestinationClass[];
  host: string;
  port: number;
  scheme: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NetworkRequestPlan".
 */
export interface NetworkRequestPlan {
  byte_limits: NetworkByteLimits;
  caller: NetworkCallerKind;
  credential_refs: CredentialRef[];
  data_classes: NetworkDataClass[];
  destination?: ResolvedNetworkDestination | null;
  identity: InvocationIdentity;
  method: string;
  network_grant_ids: NetworkGrantId[];
  redirect_policy: RedirectPolicy;
  request_data: NetworkDataReference[];
  request_id: NetworkRequestId;
  requested_url: string;
  timeout_ms: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NetworkByteLimits".
 */
export interface NetworkByteLimits {
  max_decompressed_bytes: number;
  max_download_bytes: number;
  max_upload_bytes: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AuthorizedToolSandboxExecutionV1".
 */
export interface AuthorizedToolSandboxExecutionV1 {
  binding: ToolSandboxDispatchBindingV1;
  operation: W7ToolOperationFingerprintPreimageV1;
  plan: SandboxExecutionPlan;
}
/**
 * Durable, one-shot authorization/dispatch relation consumed by Sandbox
 * Host before it may spawn a Tool process. It contains no secret bytes.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolSandboxDispatchBindingV1".
 */
export interface ToolSandboxDispatchBindingV1 {
  attempt_output_root?: CanonicalPath | null;
  authorization_basis: PermissionAllowAuthorizationBasisV1;
  binding_hash: CanonicalSha256HashV1;
  binding_version: number;
  execution_plan_hash: CanonicalSha256HashV1;
  network_access: NetworkAccessRequirement;
  network_enforcement: NetworkEnforcementKind;
  operation_fingerprint: CanonicalBlake3HashV1;
  operation_id: SecurityOperationId;
  operation_identity: InvocationIdentity;
  permission_snapshot: PermissionSnapshot;
  prepared_at: UtcTimestamp;
  runtime_binding_hash: CanonicalSha256HashV1;
  runtime_output_authorization?: ToolRuntimeOutputAuthorizationV1 | null;
  sandbox_execution_id: SandboxExecutionId;
  sandbox_plan_hash: ContentHash;
  selected_mode: NativeSandboxMode;
  source: ToolSecuritySourceRefV1;
  source_security_fingerprint: ContentHash;
  tool_batch_id: ToolBatchId;
  tool_call_id: ToolCallId;
  tool_execution_attempt_id: ToolExecutionAttemptId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PermissionSnapshot".
 */
export interface PermissionSnapshot {
  active_grant_ids: PermissionGrantId[];
  created_at: UtcTimestamp;
  id: PermissionSnapshotId;
  mode: PermissionMode;
  session_id: SessionId;
  session_permission_revision: number;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolRuntimeOutputAuthorizationV1".
 */
export interface ToolRuntimeOutputAuthorizationV1 {
  authorization_hash: CanonicalSha256HashV1;
  authorization_version: ToolWireVersionV1;
  max_artifact_count: 256;
  max_single_artifact_bytes: 67108864;
  max_total_artifact_bytes: 268435456;
  media_policy_version: ToolWireVersionV1;
  path_policy_version: ToolWireVersionV1;
  tool_call_id: ToolCallId;
  tool_execution_attempt_id: ToolExecutionAttemptId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "W7ToolOperationFingerprintPreimageV1".
 */
export interface W7ToolOperationFingerprintPreimageV1 {
  arguments: any;
  arguments_hash: CanonicalSha256HashV1;
  capabilities: ResolvedCapability[];
  execution_plan_hash: CanonicalSha256HashV1;
  invocation_identity: CoreInvocationIdentityV1;
  kind: W7ToolOperationFingerprintDomainV1;
  network_access: NetworkAccessRequirement;
  preimage_version: number;
  purpose: ToolPurposeTextV1;
  risks: OperationRiskFlag[];
  runtime: NonSecretToolRuntimeFactsV1;
  runtime_binding_hash: CanonicalSha256HashV1;
  sandbox_mode?: NativeSandboxMode | null;
  scopes: ResolvedScope[];
  source: ToolSecuritySourceRefV1;
  source_security_fingerprint: ContentHash;
  tool_batch_id: ToolBatchId;
  tool_call_id: ToolCallId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CoreInvocationIdentityV1".
 */
export interface CoreInvocationIdentityV1 {
  agent_run_id: AgentRunId;
  command_invocation_id: CommandInvocationId;
  model_step_id: ModelStepId;
  session_id: SessionId;
  turn_task_id: TurnTaskId;
}
/**
 * Complete non-secret facts that P04 approves for a short-lived command or
 * Aura Stdio launch. Attempt-local TEMP/TMP and the Credential plaintext are
 * deliberately absent and may only be materialized after the pre-dispatch
 * transaction has committed.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NonSecretSandboxRuntimeFactsV1".
 */
export interface NonSecretSandboxRuntimeFactsV1 {
  arguments: WindowsOsString[];
  aura_stdio_bootstrap?: AuraStdioBootstrapPolicyV1 | null;
  aura_stdio_output?: AuraStdioOutputProvisionPolicyV1 | null;
  credential?: ToolCredentialUseV1 | null;
  environment: SanitizedEnvironmentEntry[];
  executable: CanonicalExecutableRef;
  /**
   * Exact non-secret filesystem launch facts approved by P04. These are
   * part of the operation fingerprint and cannot be widened when the
   * Sandbox plan is materialized.
   */
  filesystem_grants: SandboxFilesystemGrant[];
  output_limits: ProcessOutputLimits;
  runtime_kind: ToolPreparedRuntimeKindV1;
  stdio_protocol_version?: number | null;
  success_exit_codes: number[];
  timeout_ms: number;
  working_directory: CanonicalPath;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WindowsOsString".
 */
export interface WindowsOsString {
  utf16: number[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AuraStdioBootstrapPolicyV1".
 */
export interface AuraStdioBootstrapPolicyV1 {
  policy_version: 1;
  tool_call_id_environment_variable: 'AURA_TOOL_CALL_ID';
  tool_call_id_source: 'admitted_tool_call';
  tool_execution_attempt_id_environment_variable: 'AURA_TOOL_EXECUTION_ATTEMPT_ID';
  tool_execution_attempt_id_source: 'dispatch_binding';
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AuraStdioOutputProvisionPolicyV1".
 */
export interface AuraStdioOutputProvisionPolicyV1 {
  output_root_environment_variable: 'AURA_TOOL_OUTPUT_ROOT';
  output_root_source: 'dispatch_binding';
  policy_version: 1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCredentialUseV1".
 */
export interface ToolCredentialUseV1 {
  credential_id: CredentialId;
  /**
   * Secret-generation revision for one stable CredentialId.
   *
   * This is intentionally distinct from a row revision. Clear and metadata-only
   * mutations may change row concurrency without creating a new secret
   * generation.
   */
  credential_revision: string;
  placement: ToolCredentialPlacementV12;
  slot_key: CredentialSlotKey;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SanitizedEnvironmentEntry".
 */
export interface SanitizedEnvironmentEntry {
  name: string;
  value: WindowsOsString;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CanonicalExecutableRef".
 */
export interface CanonicalExecutableRef {
  content_hash: ContentHash;
  identity: FileIdentity;
  path: CanonicalPath;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SandboxFilesystemGrant".
 */
export interface SandboxFilesystemGrant {
  access: SandboxFilesystemAccess;
  identity?: FileIdentity | null;
  path: CanonicalPath;
  recursive: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ProcessOutputLimits".
 */
export interface ProcessOutputLimits {
  combined_bytes: number;
  stderr_bytes: number;
  stdout_bytes: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NonSecretHttpRuntimeFactsV1".
 */
export interface NonSecretHttpRuntimeFactsV1 {
  body_byte_length: number;
  body_hash: CanonicalSha256HashV1;
  canonical_path: string;
  credential?: ToolCredentialUseV1 | null;
  exact_url: string;
  grant_manifest_hash: CanonicalSha256HashV1;
  headers_hash: CanonicalSha256HashV1;
  method: string;
  prepared_request_hash: CanonicalSha256HashV1;
  response_decompressed_limit: number;
  response_wire_limit: number;
  runtime_kind: ToolPreparedRuntimeKindV1;
  success_status_ranges: [any, any][];
  timeout_ms: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SandboxExecutionPlan".
 */
export interface SandboxExecutionPlan {
  args: WindowsOsString[];
  cancellation_id: CancellationId;
  environment: SanitizedEnvironmentEntry[];
  executable: CanonicalExecutableRef;
  execution_id: SandboxExecutionId;
  filesystem_grants: SandboxFilesystemGrant[];
  identity: InvocationIdentity;
  mode: NativeSandboxMode;
  network_access: NetworkAccessRequirement;
  network_enforcement: NetworkEnforcementKind;
  network_grant_ids: NetworkGrantId[];
  output_limits: ProcessOutputLimits;
  permission_snapshot_id: PermissionSnapshotId;
  plan_hash: ContentHash;
  timeout_ms: number;
  working_directory: CanonicalPath;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CredentialReflectionScanEvidenceV1".
 */
export interface CredentialReflectionScanEvidenceV1 {
  credential_id: CredentialId;
  /**
   * Secret-generation revision for one stable CredentialId.
   *
   * This is intentionally distinct from a row revision. Clear and metadata-only
   * mutations may change row concurrency without creating a new secret
   * generation.
   */
  credential_revision: string;
  detected_encoding?: CredentialReflectionEncodingV1 | null;
  detected_surface?: CredentialReflectionSurfaceV1 | null;
  encoding_variants_scanned: CredentialReflectionEncodingSetV1;
  evidence_hash: CanonicalSha256HashV1;
  policy_version: ToolWireVersionV1;
  scanned_body_bytes: number;
  scanned_header_count: number;
  status: CredentialReflectionScanStatusV1;
  surfaces_scanned: CredentialReflectionSurfaceSetV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CredentialReflectionEncodingSetV1".
 */
export interface CredentialReflectionEncodingSetV1 {
  values: CredentialReflectionEncodingV1[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CredentialReflectionSurfaceSetV1".
 */
export interface CredentialReflectionSurfaceSetV1 {
  values: CredentialReflectionSurfaceV1[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExternalExtensionRiskView".
 */
export interface ExternalExtensionRiskView {
  display_name: string;
  path_or_origin: string;
  runtime_isolation?: ExternalIsolationMode | null;
  source_kind: ExternalExtensionKind;
  summary: string;
  warnings: string[];
}
/**
 * Main-only input produced from a native file picker. This command is never
 * exposed through `window.aura.v1` as a generic path-taking method.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AttachExternalReadOnlyInput".
 */
export interface AttachExternalReadOnlyInput {
  selected_path: string;
  session_id: SessionId;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "DeriveWorkspaceExternalGrantInput".
 */
export interface DeriveWorkspaceExternalGrantInput {
  permission_grant_id: PermissionGrantId;
  session_id: SessionId;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ListSessionExternalGrantsInput".
 */
export interface ListSessionExternalGrantsInput {
  session_id: SessionId;
}
/**
 * Core-owned capability record. Paths and file identities are verification
 * facts; only `ExternalGrantId` is an authority-bearing handle.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExternalGrantRecord".
 */
export interface ExternalGrantRecord {
  canonical_root: CanonicalPath;
  capabilities: ExternalGrantCapability[];
  created_at: UtcTimestamp;
  /**
   * A clock bound on top of [`Self::term`]. Absent means the term itself is the bound:
   * the session, the workspace, or the turn, plus revocation. It is not the approval
   * request's confirmation deadline.
   */
  expires_at?: UtcTimestamp | null;
  id: ExternalGrantId;
  lifecycle: ExternalGrantLifecycle;
  permission_grant_id?: PermissionGrantId | null;
  recursive_scope: boolean;
  revalidation_revision: number;
  revoked_at?: UtcTimestamp | null;
  root_identity: FileIdentity;
  selected_root: CanonicalPath;
  session_id: SessionId;
  suspended_at?: UtcTimestamp | null;
  target_kind: ExternalGrantTargetKind;
  /**
   * How long a directory grant remains reusable after the user approves it.
   *
   * This is the grant's own term, not the approval request's confirmation window.
   * Older records omit the field and stay session-scoped, which is the term they had.
   */
  term?: 'session' | 'workspace' | 'turn';
  /**
   * Set only when [`Self::term`] is [`ExternalGrantTerm::Turn`].
   */
  turn_task_id?: TurnTaskId | null;
  updated_at: UtcTimestamp;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RevalidateExternalGrantInput".
 */
export interface RevalidateExternalGrantInput {
  external_grant_id: ExternalGrantId;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RevokeExternalGrantInput".
 */
export interface RevokeExternalGrantInput {
  external_grant_id: ExternalGrantId;
  session_id: SessionId;
}
/**
 * Renderer/model-safe view. It intentionally omits the canonical path and
 * file identity so callers cannot reflect either value back as authority.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExternalGrantView".
 */
export interface ExternalGrantView {
  /**
   * Whether this grant can authorize a call right now. Lifecycle alone is not enough:
   * an active row can still be past its clock.
   */
  availability?: ('in_force' | 'expired' | 'suspended' | 'revoked') | 'turn_ended';
  capabilities: ExternalGrantCapability[];
  display_path: string;
  expires_at?: UtcTimestamp | null;
  id: ExternalGrantId;
  lifecycle: ExternalGrantLifecycle;
  recursive_scope: boolean;
  revalidation_revision: number;
  session_id: SessionId;
  target_kind: ExternalGrantTargetKind;
  term: ExternalGrantTerm;
  workspace_id: WorkspaceId;
}
/**
 * Frozen launch contract for a trusted external process. It deliberately uses
 * `ExternalIsolationMode` instead of the native Tool sandbox enum: external
 * compatibility means managed standard-user execution, never Compatibility
 * AppContainer.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExternalProcessLaunchPlan".
 */
export interface ExternalProcessLaunchPlan {
  args: WindowsOsString[];
  cancellation_id: CancellationId;
  environment: SanitizedEnvironmentEntry[];
  executable: CanonicalExecutableRef;
  execution_id: SandboxExecutionId;
  identity: InvocationIdentity;
  isolation: ExternalIsolationMode;
  network_access: NetworkAccessRequirement;
  network_enforcement: NetworkEnforcementKind;
  output_limits: ProcessOutputLimits;
  permission_snapshot_id: PermissionSnapshotId;
  plan_hash: ContentHash;
  source: SecuritySourceV2;
  source_fingerprint: ContentHash;
  timeout_ms: number;
  working_directory: CanonicalPath;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ListFileSessionConsentsInput".
 */
export interface ListFileSessionConsentsInput {
  include_inactive?: boolean;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RevokeFileSessionConsentInput".
 */
export interface RevokeFileSessionConsentInput {
  expected_revision: Revision;
  grant_id: PermissionGrantId;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FileSessionConsentView".
 */
export interface FileSessionConsentView {
  availability: FileSessionConsentAvailability;
  expires_at?: UtcTimestamp | null;
  grant_id: PermissionGrantId;
  grant_revision: Revision;
  issued_at: UtcTimestamp;
  revoked_at?: UtcTimestamp | null;
  scope: FileSessionConsentRef;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FileSessionConsentRef".
 */
export interface FileSessionConsentRef {
  approval_id: ApprovalRequestId;
  approved_operation_fingerprint: CanonicalSha256HashV11;
  capabilities: ResolvedCapability[];
  grant_revision: Revision;
  principal_id: PrincipalId;
  revoked_grant_revision?: Revision | null;
  session_id: SessionId;
  targets: FileConsentTarget[];
}
/**
 * Original approval audit identity, never rewritten on publication.
 */
export interface CanonicalSha256HashV11 {
  algorithm: 'sha256';
  canonicalization_version: 'rfc8785_jcs_v1';
  hex: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FileConsentTarget".
 */
export interface FileConsentTarget {
  /**
   * Verified directory identities, outermost first, including the immediate parent.
   */
  ancestors?: string[];
  /**
   * Display only. Equality and containment never fold this string.
   */
  canonical_path: string;
  /**
   * The directory entry, used only when consuming its own publication receipt.
   */
  entry_key?: string;
  /**
   * Verified object identity or verified parent + native entry key for a create.
   */
  identity_key?: string;
  /**
   * Recursive ordinary approval cannot inherit a protected descendant.
   */
  protection?: 'ordinary' | 'system' | 'sensitive_user' | 'control_authority';
  recursive: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GetSessionPermissionStateInput".
 */
export interface GetSessionPermissionStateInput {
  session_id: SessionId;
}
/**
 * Main-only result for a trusted Host Preparation request. Renderer receives
 * only `status`; Electron Main uses the read-only ApprovalView to render the
 * native confirmation dialog.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HostPreparationBeginResult".
 */
export interface HostPreparationBeginResult {
  approval?: ApprovalView | null;
  status: HostPreparationStatus;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ApprovalView".
 */
export interface ApprovalView {
  actual_operation_summary: string;
  allowed_decisions: ApprovalDecisionKind[];
  created_at: UtcTimestamp;
  id: ApprovalRequestId;
  model_purpose?: string | null;
  release_descriptor?: ReleaseDescriptor | null;
  request_revision: number;
  risk_flags: OperationRiskFlag[];
  /**
   * Originating session; None denotes a non-conversation approval.
   */
  session_id?: SessionId | null;
  source_summary: string;
  title: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HostPreparationStatus".
 */
export interface HostPreparationStatus {
  active_operation?: HostPreparationOperationView | null;
  revision: number;
  summary: HostPreparationSummary;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HostPreparationOperationView".
 */
export interface HostPreparationOperationView {
  action: HostPreparationAction;
  add_all_application_packages_ace: boolean;
  add_all_restricted_application_packages_ace: boolean;
  created_at: UtcTimestamp;
  error_code?: string | null;
  expires_at: UtcTimestamp;
  host_preparation_revision: number;
  operation_id: HostPreparationOperationId;
  operation_revision: number;
  plan_hash: ContentHash;
  release_descriptor: ReleaseDescriptor;
  requires_elevation: boolean;
  runtime_instance_id: RuntimeInstanceId;
  state: HostPreparationOperationState;
  update_null_device_security: boolean;
  updated_at: UtcTimestamp;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HostPreparationSummary".
 */
export interface HostPreparationSummary {
  appcontainer_available: boolean;
  last_error_code?: string | null;
  nested_jobs_available: boolean;
  nul_compatibility_ready: boolean;
  residual_acl_count: number;
  residual_appcontainer_profile_count: number;
  residual_firewall_rule_count: number;
  restart_required: boolean;
  standard_user_token_available: boolean;
  system_drive_compatibility_ready: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HostPreparationMutationInput".
 */
export interface HostPreparationMutationInput {
  expected_revision: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ResolveHostPreparationInput".
 */
export interface ResolveHostPreparationInput {
  approval_id: ApprovalRequestId;
  decision: ApprovalDecisionKind;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ListPermissionGrantsInput".
 */
export interface ListPermissionGrantsInput {
  session_id?: SessionId | null;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ListNativeSessionConsentsInput".
 */
export interface ListNativeSessionConsentsInput {
  /**
   * Include revoked, consumed and expired consents. The default is in-force
   * only, because that is the list a person acts on.
   */
  include_inactive?: boolean;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RevokeNativeSessionConsentInput".
 */
export interface RevokeNativeSessionConsentInput {
  correlation_id: CorrelationId;
  /**
   * The revision the caller last saw. Withdrawal is a compare-and-swap, so a
   * stale list cannot withdraw an authorization that has since changed.
   */
  expected_revision: string;
  grant_id: PermissionGrantId;
  session_id: SessionId;
}
/**
 * What one Native session consent covers.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NativeSessionConsentScopeView".
 */
export interface NativeSessionConsentScopeView {
  approved_operation_fingerprint: CanonicalSha256HashV12;
  /**
   * Revision when the user answered, retained for audit. Only legacy answers
   * expire when this revision changes; new session answers persist.
   */
  approved_permission_revision: number;
  /**
   * The built-in Shell tools a Native session consent may cover.
   *
   * The variant is the conservative identity: a consent is approved for one
   * concrete shell that is part of the signed-in product, never for "some
   * process on this machine". It is the *tool that was approved*, which is an audit
   * fact; the *range the approval covers* is [`NativeSessionConsentScope::shells`].
   */
  approved_shell: 'cmd' | 'powershell';
  /**
   * Issuing Core for audit; only legacy records expire on a Core restart.
   */
  core_runtime_instance_id: string;
  lifetime?: 'legacy_runtime' | 'session';
  /**
   * The actor the consent was approved for. An Agent-initiated call in the same
   * session is a different principal and is not covered.
   */
  principal_id: string;
  session_id: SessionId;
  /**
   * The trusted built-in Shells this consent authorizes. It is stored rather than
   * derived from `approved_shell`, because "which tool was approved" and "what
   * the approval covers" are different facts.
   */
  shells: NativeShell[];
  workspace_id: WorkspaceId;
}
/**
 * The Operation the user actually confirmed. It is audit evidence, never an
 * equality condition on later work.
 */
export interface CanonicalSha256HashV12 {
  algorithm: 'sha256';
  canonicalization_version: 'rfc8785_jcs_v1';
  hex: string;
}
/**
 * One consent as the management surface shows it.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NativeSessionConsentView".
 */
export interface NativeSessionConsentView {
  approval_id: ApprovalRequestId;
  /**
   * The confirmation window that produced it. It bounds the *approval*, not the
   * consent: an in-force consent does not become invalid when this passes.
   */
  approved_through: string;
  availability: NativeSessionConsentAvailability;
  /**
   * The Native backend this consent authorizes. A display fact, not an
   * authorization input.
   */
  backend_id: string;
  /**
   * The authorization that carries the consent. This is the withdrawal handle.
   */
  grant_id: string;
  grant_revision: Revision;
  issued_at: UtcTimestamp;
  revoked_at?: UtcTimestamp | null;
  scope: NativeSessionConsentScopeView;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NetworkRequestAuditRecord".
 */
export interface NetworkRequestAuditRecord {
  caller: NetworkCallerKind;
  data_classes: NetworkDataClass[];
  destination_classes: NetworkDestinationClass[];
  download_bytes: number;
  error_code?: string | null;
  finished_at?: UtcTimestamp | null;
  identity: InvocationIdentity;
  inspection_level: NetworkInspectionLevel;
  method: string;
  network_enforcement: NetworkEnforcementKind;
  origin?: NormalizedOrigin | null;
  request_id: NetworkRequestId;
  started_at: UtcTimestamp;
  state: NetworkRequestAuditState;
  upload_bytes: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NetworkGrant".
 */
export interface NetworkGrant {
  allowed_data_classes: NetworkDataClass[];
  destination_classes: NetworkDestinationClass[];
  expires_at?: UtcTimestamp | null;
  id: NetworkGrantId;
  identity_scope: NetworkGrantIdentityScope;
  identity_scope_id: string;
  lifetime: PermissionGrantLifetime;
  max_download_bytes?: number | null;
  max_upload_bytes?: number | null;
  methods: string[];
  origins: NormalizedOrigin[];
  revoked_at?: UtcTimestamp | null;
  source_fingerprint?: ContentHash | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "OperationSecurityPlan".
 */
export interface OperationSecurityPlan {
  capabilities: ResolvedCapability[];
  identity: InvocationIdentity;
  network_access: NetworkAccessRequirement;
  network_plan?: NetworkRequestPlan | null;
  operation_fingerprint: ContentHash;
  operation_id: SecurityOperationId;
  purpose?: string | null;
  requested_sandbox?: NativeSandboxMode | null;
  risk_flags: OperationRiskFlag[];
  scopes: ResolvedScope[];
  source: SecuritySourceV2;
  source_fingerprint?: ContentHash | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ApprovalRequestDraft".
 */
export interface ApprovalRequestDraft {
  actual_operation_summary: string;
  allowed_decisions: ApprovalDecisionKind[];
  capabilities: ResolvedCapability[];
  identity: InvocationIdentity;
  model_purpose?: string | null;
  operation_fingerprints: ContentHash[];
  operation_ids: SecurityOperationId[];
  permission_snapshot_id: PermissionSnapshotId;
  release_descriptor?: ReleaseDescriptor | null;
  risk_flags: OperationRiskFlag[];
  scopes: ResolvedScope[];
  source: SecuritySourceV2;
  source_fingerprint?: ContentHash | null;
  source_summary: string;
  title: string;
}
/**
 * Defaults persisted in aura.toml. Existing session permissions are independent.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PermissionDefaults".
 */
export interface PermissionDefaults {
  mode: PermissionMode;
  revision: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PermissionGrant".
 */
export interface PermissionGrant {
  approved_risk_flags: OperationRiskFlag[];
  capabilities: ResolvedCapability[];
  consumed_at?: UtcTimestamp | null;
  created_at: UtcTimestamp;
  expires_at?: UtcTimestamp | null;
  id: PermissionGrantId;
  lifetime: PermissionGrantLifetime;
  operation_fingerprint?: ContentHash | null;
  revoked_at?: UtcTimestamp | null;
  scopes: ResolvedScope[];
  session_id?: SessionId | null;
  source: SecuritySourceV2;
  source_fingerprint?: ContentHash | null;
  turn_task_id?: TurnTaskId | null;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SessionPermissionProjection".
 */
export interface SessionPermissionProjection {
  active_grant_count: number;
  mode: PermissionMode;
  revision: number;
  session_id: SessionId;
  /**
   * Older projection records predate coordinated active execution. Current
   * responses always serialize the actual persisted transition phase.
   */
  transition?: 'settled' | 'stopping' | 'stop_unconfirmed';
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PendingApprovalProjection".
 */
export interface PendingApprovalProjection {
  approvals: ApprovalView[];
  revision: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SecurityRuntimeProjection".
 */
export interface SecurityRuntimeProjection {
  egress_proxy_state: RuntimeComponentState;
  elevated_helper_state: RuntimeComponentState;
  host_preparation: HostPreparationSummary;
  quarantine_count: number;
  sandbox_available: boolean;
  sandbox_host_state: RuntimeComponentState;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "DiscardQuarantineInput".
 */
export interface DiscardQuarantineInput {
  confirmation_nonce: SecretNonce;
  quarantine_id: QuarantineRecordId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "QuarantineDiscardConfirmation".
 */
export interface QuarantineDiscardConfirmation {
  confirmation_nonce: SecretNonce;
  content_hash: ContentHash;
  expires_at: UtcTimestamp;
  quarantine_id: QuarantineRecordId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "IssueQuarantineDiscardConfirmationInput".
 */
export interface IssueQuarantineDiscardConfirmationInput {
  quarantine_id: QuarantineRecordId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PrepareQuarantineReleaseInput".
 */
export interface PrepareQuarantineReleaseInput {
  quarantine_id: QuarantineRecordId;
  session_id: SessionId;
  target: QuarantineReleaseTarget;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PrepareQuarantineReleaseResult".
 */
export interface PrepareQuarantineReleaseResult {
  approval_id: ApprovalRequestId;
  approval_request_revision: number;
  content_hash: ContentHash;
  quarantine_id: QuarantineRecordId;
  target: QuarantineReleaseTarget;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "QuarantineRecordDetails".
 */
export interface QuarantineRecordDetails {
  artifact?: ArtifactRecord | null;
  record: QuarantineRecord;
  storage_path?: string | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "QuarantineRecord".
 */
export interface QuarantineRecord {
  byte_length: number;
  content_disposition?: string | null;
  content_hash: ContentHash;
  created_at: UtcTimestamp;
  detected_kinds: string[];
  id: QuarantineRecordId;
  media_type?: string | null;
  original_name: string;
  resolved_at?: UtcTimestamp | null;
  source_url: string;
  state: QuarantineState;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ReleaseQuarantineInput".
 */
export interface ReleaseQuarantineInput {
  approval_id: ApprovalRequestId;
  quarantine_id: QuarantineRecordId;
  session_id: SessionId;
  target: QuarantineReleaseTarget;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RevokePermissionGrantInput".
 */
export interface RevokePermissionGrantInput {
  grant_id: PermissionGrantId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SandboxNetworkPolicyError".
 */
export interface SandboxNetworkPolicyError {
  allowed_networked_modes: NetworkExecutionMode[];
  code: SandboxNetworkPolicyErrorCode;
  declared_access: NetworkAccessRequirement;
  required_declaration: NetworkAccessRequirement;
  selected_mode: NetworkExecutionMode;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SetPermissionDefaultsInput".
 */
export interface SetPermissionDefaultsInput {
  expected_revision: number;
  mode: PermissionMode;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SetSessionPermissionModeInput".
 */
export interface SetSessionPermissionModeInput {
  expected_revision: number;
  mode: PermissionMode;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TrustFingerprintInput".
 */
export interface TrustFingerprintInput {
  capabilities: string[];
  content_hashes: ContentHash[];
  manifest: any;
  origin?: string | null;
  resource_policies: string[];
  runtime_entry?: string | null;
  schema?: any;
  source: SecuritySourceV2;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExternalTrustRecord".
 */
export interface ExternalTrustRecord {
  effective_fingerprint?: ContentHash | null;
  id: TrustRecordId;
  source: SecuritySourceV2;
  state: TrustState;
  updated_at: UtcTimestamp;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolContractFixture".
 */
export interface ToolContractFixture {
  activity_cursor_binding?: ToolActivityCursorBindingV1 | null;
  activity_page?: ToolActivityPageV1 | null;
  activity_page_request?: ToolActivityPageRequestV1 | null;
  batch_error_codes: ToolBatchErrorCode[];
  batch_request?: ToolBatchRequestV1 | null;
  batch_result?: ToolBatchResultV1 | null;
  builtin_definition?: BuiltinToolDefinitionV1 | null;
  canonical_hash?: CanonicalSha256HashV1 | null;
  canonical_package_manifest?: CanonicalToolPackageManifestV1 | null;
  conformance_error_codes: ToolConformanceErrorCode[];
  credential_candidates?: ToolCredentialCandidatesProjectionV1 | null;
  definition_current_selection?: ToolDefinitionCurrentSelectionV1 | null;
  describe_request?: ToolDescribeRequestV1 | null;
  describe_result?: ToolDescribeResultV1 | null;
  draft_error_codes: ToolDraftErrorCode[];
  draft_kind?: ToolDraftKindV1 | null;
  draft_snapshot_transfer?: ToolDraftSnapshotTransferV1 | null;
  effective_binding?: EffectiveToolBindingSnapshot | null;
  error_catalog: ToolErrorCatalogV1;
  execution_attempt?: ToolExecutionAttemptRecordV1 | null;
  execution_plan?: ToolExecutionPlanV1 | null;
  input_schema?: CanonicalJsonSchemaDocumentV1 | null;
  lifecycle_event?: ToolLifecycleEventV1 | null;
  list_loaded_request?: ToolListLoadedRequestV1 | null;
  list_loaded_result?: ToolListLoadedResultV1 | null;
  load_request?: ToolLoadRequestV1 | null;
  load_result?: ToolLoadResultV1 | null;
  model_visible_result?: ModelVisibleToolResultEnvelopeV1 | null;
  mutation_contract?: ToolMutationContractSchemaRootV1 | null;
  package_delete_approval?: ToolPackageDeleteApprovalEvidenceV1 | null;
  package_delete_draft_snapshot?: ToolPackageDeleteDraftSnapshotV1 | null;
  package_error_codes: ToolPackageErrorCode[];
  package_file?: ToolPackageFileEntryV1 | null;
  package_fingerprint?: CanonicalSha256HashV1 | null;
  package_fingerprint_preimage?: PackageFingerprintPreimageV1 | null;
  package_import_diagnostic?: PackageImportDiagnosticV1 | null;
  package_import_preview?: PackageImportPreviewV1 | null;
  package_manifest?: RawToolPackageManifestV1 | null;
  package_runtime_closure?: PackageRuntimeClosureV1 | null;
  package_runtime_closure_hash?: CanonicalSha256HashV1 | null;
  package_runtime_visibility_policy?: PackageRuntimeVisibilityPolicyV1 | null;
  package_security_fingerprint?: CanonicalBlake3HashV1 | null;
  package_security_fingerprint_preimage?: PackageSecurityFingerprintPreimageV1 | null;
  package_version?: ToolPackageVersionRefV1 | null;
  read_contract?: ToolReadContractSchemaRootV1 | null;
  registry_error_codes: ToolRegistryErrorCode[];
  registry_snapshot?: ToolRegistrySnapshotV1 | null;
  resolved_resource_claim?: ResolvedResourceClaimV1 | null;
  resource_claim?: ResourceClaimV1 | null;
  resource_error_codes: ToolResourceErrorCode[];
  result?: ToolResultV1 | null;
  result_error_codes: ToolResultErrorCode[];
  runtime?: ToolRuntimeDescriptorV1 | null;
  runtime_error_codes: ToolRuntimeErrorCode[];
  runtime_output_authorization?: ToolRuntimeOutputAuthorizationV1 | null;
  scheduler_error_codes: ToolSchedulerErrorCode[];
  schema_error_codes: ToolSchemaErrorCode[];
  schema_set_snapshot?: ToolSchemaSetSnapshotV1 | null;
  schema_snapshot?: ToolSchemaSnapshotV1 | null;
  search_page?: ToolSearchPageV1 | null;
  search_request?: ToolSearchRequestV1 | null;
  settings_snapshot?: ToolSettingsSnapshotV1 | null;
  stdio_frame?: StdioProtocolFrameV1 | null;
  structured_result_schema?: CanonicalJsonSchemaDocumentV1 | null;
  tool_contract_fingerprint?: ToolContractFingerprintV1 | null;
  tool_contract_fingerprint_preimage?: ToolContractFingerprintPreimageV1 | null;
  user_audit_result?: UserAuditToolResultEnvelopeV1 | null;
  verified_attempt_side_effect?: VerifiedAttemptSideEffectEvidenceV1 | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolActivityCursorBindingV1".
 */
export interface ToolActivityCursorBindingV1 {
  cursor_version: ToolWireVersionV1;
  cutoff_session_sequence: SessionSequence;
  kind: ToolActivityCursorKindV1;
  last_session_sequence: SessionSequence;
  last_tool_call_id: ToolCallId;
  ordering_algorithm_version: ToolWireVersionV1;
  page_size: number;
  session_id: SessionId;
  visibility_context_hash: CanonicalSha256HashV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolActivityPageV1".
 */
export interface ToolActivityPageV1 {
  cutoff_session_sequence: SessionSequence;
  items: ToolCallActivityHistoryViewV1[];
  next_cursor?: ToolActivityCursorV1 | null;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCallActivityHistoryViewV1".
 */
export interface ToolCallActivityHistoryViewV1 {
  artifact_unavailable: boolean;
  call: ToolCallActivitySummaryView;
  definition_ref: ToolDefinitionSnapshotRef;
  session_sequence: SessionSequence;
  source: ToolSourceKindV1;
  tool_instance_id: ToolInstanceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolActivityPageRequestV1".
 */
export interface ToolActivityPageRequestV1 {
  cursor: ToolActivityCursorV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolBatchRequestV1".
 */
export interface ToolBatchRequestV1 {
  batch_content_hash: CanonicalSha256HashV1;
  calls: NormalizedToolCallV1[];
  invocation_identity: CoreInvocationIdentityV1;
  p08_state_hash: CanonicalSha256HashV1;
  p08_state_revision: Revision;
  registry_snapshot_ref: ToolRegistrySnapshotRef;
  tool_batch_id: ToolBatchId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NormalizedToolCallV1".
 */
export interface NormalizedToolCallV1 {
  /**
   * None only for a Provider name absent from the frozen request. Such a call
   * can only become ToolNotFound, never an executable operation.
   */
  definition_ref: ToolDefinitionSnapshotRef | null;
  input: NormalizedToolCallInputStateV1;
  ordinal: number;
  provider_arguments_envelope_hash: CanonicalSha256HashV1;
  tool_call_id: ToolCallId;
}
/**
 * Where a normalization issue was found, in a form that is safe to show the model.
 *
 * It names structure, never content: a pointer inside the argument envelope and the
 * offending *field names*. The argument text itself is never copied here — the
 * corrective message is stored with the Tool result and rendered in the session, and
 * neither may echo a script, a credential or the whole envelope. Both members are
 * bounded by the producer.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NormalizedToolCallInputDiagnosticV1".
 */
export interface NormalizedToolCallInputDiagnosticV1 {
  /**
   * The offending field names, in the stable order the strict parser keeps object
   * keys in — the diagnostic names them, it does not reproduce the envelope.
   */
  fields?: string[];
  /**
   * Where the issue is. `$` is the envelope root; `/_aura` and `/_aura/<name>` locate
   * the metadata of envelopes 1-2, and `/aura_purpose` the purpose of envelope 3.
   */
  pointer: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolBatchResultV1".
 */
export interface ToolBatchResultV1 {
  completed_at: UtcTimestamp;
  results: ToolResultV1[];
  status: ToolBatchStatusV1;
  tool_batch_id: ToolBatchId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolResultV1".
 */
export interface ToolResultV1 {
  aggregate_side_effect_evidence_ref?: ArtifactId | null;
  attempts: ToolExecutionAttemptOutcomeRefV1[];
  completed_at: UtcTimestamp;
  internal_diagnostic_ref?: ArtifactId | null;
  model_visible_result_ref?: ArtifactId | null;
  outcome: ToolResultOutcomeV1;
  side_effect_state: SideEffectStateV1;
  started_at?: UtcTimestamp | null;
  tool_call_id: ToolCallId;
  user_audit_result_ref?: ArtifactId | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolExecutionAttemptOutcomeRefV1".
 */
export interface ToolExecutionAttemptOutcomeRefV1 {
  attempt_number: number;
  dispatched: boolean;
  side_effect_evidence_ref?: ArtifactId | null;
  side_effect_state: SideEffectStateV1;
  tool_execution_attempt_id: ToolExecutionAttemptId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BuiltinToolDefinitionV1".
 */
export interface BuiltinToolDefinitionV1 {
  audience: ToolAudienceV1;
  builtin_key: ToolCanonicalName;
  capabilities: BuiltinToolCapabilityV1[];
  input_schema: CanonicalJsonSchemaDocumentV1;
  model_description: string;
  model_summary: string;
  result_contract: ToolResultContractV1;
  scope_requirements: BuiltinToolScopeRequirementV1[];
  source: ToolSourceKindV1;
  strict: boolean;
  tool_id: ToolId;
  tool_version: ToolVersion;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCredentialCandidatesProjectionV1".
 */
export interface ToolCredentialCandidatesProjectionV1 {
  candidate_set_revision: Revision;
  candidates: ToolCredentialCandidateViewV1[];
  credential_slot_key: CredentialSlotKey;
  instance_revision: Revision;
  placement: ToolCredentialPlacementV1;
  session_id?: SessionId | null;
  tool_instance_id: ToolInstanceId;
  workspace_id?: WorkspaceId | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCredentialCandidateViewV1".
 */
export interface ToolCredentialCandidateViewV1 {
  compatibility: ToolCredentialCandidateCompatibilityV1;
  credential_id: CredentialId;
  credential_kind: ToolCredentialKindV1;
  /**
   * Secret-generation revision for one stable CredentialId.
   *
   * This is intentionally distinct from a row revision. Clear and metadata-only
   * mutations may change row concurrency without creating a new secret
   * generation.
   */
  credential_revision: string;
  display_name: string;
  scope_summary: ToolCredentialScopeSummaryV1;
  state: ToolCredentialStateV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDescribeRequestV1".
 */
export interface ToolDescribeRequestV1 {
  definition_ref: ToolDefinitionSnapshotRef;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDescribeResultV1".
 */
export interface ToolDescribeResultV1 {
  audience: ToolAudienceV1;
  canonical_name: ToolCanonicalName;
  capability_summary: ToolCapabilitySummaryV1[];
  currently_loadable: boolean;
  definition_ref: ToolDefinitionSnapshotRef;
  display_name: string;
  load_state: ToolLoadStateV1;
  model_description: string;
  model_summary: string;
  resource_claim_summary: ToolResourceClaimSummaryV1[];
  result_contract: ToolResultContractV1;
  safe_unavailability_reason?: ToolSafeIssueViewV1 | null;
  schema: ToolSchemaSnapshotV1;
  source: ToolSourceKindV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCapabilitySummaryV1".
 */
export interface ToolCapabilitySummaryV1 {
  capability: string;
  summary: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolResourceClaimSummaryV1".
 */
export interface ToolResourceClaimSummaryV1 {
  access: string[];
  claim_key: PackageResourceClaimKeyV1;
  resource_kind: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolSafeIssueViewV1".
 */
export interface ToolSafeIssueViewV1 {
  category: ErrorCategory;
  code: ErrorCode;
  message: string;
  retryable: boolean;
}
/**
 * Canonical transport envelope for replacing unvalidated Draft content.
 *
 * This is deliberately not a Tool Package Manifest and validation here does
 * not imply that `raw_manifest_utf8` is a valid manifest. Core must authorize
 * and verify the envelope Artifact and every child Artifact before converting
 * this value to `DraftPackageContentV1`.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDraftSnapshotTransferV1".
 */
export interface ToolDraftSnapshotTransferV1 {
  files: ToolDraftSnapshotTransferFileV1[];
  raw_manifest_byte_length: number;
  raw_manifest_sha256: ToolPackageFileSha256V1;
  raw_manifest_utf8: string;
  schema_version: number;
  snapshot_hash: CanonicalSha256HashV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDraftSnapshotTransferFileV1".
 */
export interface ToolDraftSnapshotTransferFileV1 {
  artifact: ToolDraftTransferArtifactRefV1;
  /**
   * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
   * via the `definition` "PackageRelativePath".
   */
  path: PackageRelativePath;
}
/**
 * Exact immutable Artifact identity used by the Draft Snapshot Transfer
 * envelope. The Artifact store still verifies its own content hash; these
 * fields additionally bind the transfer contract to the authored SHA-256
 * bytes and length.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDraftTransferArtifactRefV1".
 */
export interface ToolDraftTransferArtifactRefV1 {
  artifact_id: ArtifactId;
  byte_length: number;
  sha256: ToolPackageFileSha256V1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolErrorCatalogV1".
 */
export interface ToolErrorCatalogV1 {
  catalog_version: number;
  entries: ToolErrorCatalogEntryV1[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolErrorCatalogEntryV1".
 */
export interface ToolErrorCatalogEntryV1 {
  category: ErrorCategory;
  code: ErrorCode;
  retry_class: ToolErrorRetryClassV1;
  safe_message_key: string;
  suggested_not_executed_reason?: ToolNotExecutedReasonV1 | null;
  visibility: ToolErrorVisibilityV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolExecutionAttemptRecordV1".
 */
export interface ToolExecutionAttemptRecordV1 {
  attempt_number: number;
  completed_at?: UtcTimestamp | null;
  dispatch_prepared_at?: UtcTimestamp | null;
  evidence_ref?: ArtifactId | null;
  last_progress_sequence: number;
  side_effect_state: SideEffectStateV1;
  started_at: UtcTimestamp;
  status: ToolExecutionAttemptStatusV1;
  tool_call_id: ToolCallId;
  tool_execution_attempt_id: ToolExecutionAttemptId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolExecutionPlanV1".
 */
export interface ToolExecutionPlanV1 {
  calls: ToolExecutionPlanCallV1[];
  execution_plan_hash: CanonicalSha256HashV1;
  registry_snapshot_ref: ToolRegistrySnapshotRef;
  tool_batch_id: ToolBatchId;
  tool_settings_hash: CanonicalSha256HashV1;
  tool_settings_revision: Revision;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolExecutionPlanCallV1".
 */
export interface ToolExecutionPlanCallV1 {
  dependency_tool_call_ids: ToolCallId[];
  entity_revision: Revision;
  execution_atomic_group?: number | null;
  local_concurrency_limit?: number | null;
  max_retries: number;
  ordinal: number;
  resolved_resource_claims: ResolvedResourceClaimV1[];
  timeout_ms: number;
  tool_call_id: ToolCallId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ResolvedResourceClaimV1".
 */
export interface ResolvedResourceClaimV1 {
  access: ResolvedResourceAccessV1;
  canonical_lock_key: string;
  canonical_resource_identity: string;
  core_derived: boolean;
  kind: ResolvedResourceKindV1;
  resolution_evidence_hash: CanonicalSha256HashV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageInstalledPayloadV1".
 */
export interface ToolPackageInstalledPayloadV1 {
  installed_at: UtcTimestamp;
  manifest_hash: CanonicalSha256HashV1;
  package: ToolPackageVersionRefV1;
  tool_identities: ToolPackageToolIdentityV1[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageVersionRefV1".
 */
export interface ToolPackageVersionRefV1 {
  package_fingerprint: CanonicalSha256HashV1;
  package_key: PackageKey;
  package_version: CanonicalSemVer;
  security_fingerprint: CanonicalBlake3HashV1;
  tool_package_id: ToolPackageId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageToolIdentityV1".
 */
export interface ToolPackageToolIdentityV1 {
  tool_id: ToolId;
  tool_key: ToolKey;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageUpdatedPayloadV1".
 */
export interface ToolPackageUpdatedPayloadV1 {
  active_tool_identities: ToolPackageToolIdentityV1[];
  manifest_hash: CanonicalSha256HashV1;
  package: ToolPackageVersionRefV1;
  previous_package: ToolPackageVersionRefV1;
  retired_tools: ToolTombstoneV1[];
  updated_at: UtcTimestamp;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolTombstoneV1".
 */
export interface ToolTombstoneV1 {
  deleted_at: UtcTimestamp;
  tool_id: ToolId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageDeletedPayloadV1".
 */
export interface ToolPackageDeletedPayloadV1 {
  package_tombstone: ToolPackageTombstoneV1;
  tool_tombstones: ToolTombstoneV1[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageTombstoneV1".
 */
export interface ToolPackageTombstoneV1 {
  deleted_at: UtcTimestamp;
  package_key: PackageKey;
  tool_package_id: ToolPackageId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolTrustChangedPayloadV1".
 */
export interface ToolTrustChangedPayloadV1 {
  changed_at: UtcTimestamp;
  entity_revision: Revision;
  package_security_fingerprint: CanonicalBlake3HashV1;
  tool_package_id: ToolPackageId;
  trusted: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolEnabledChangedPayloadV1".
 */
export interface ToolEnabledChangedPayloadV1 {
  changed_at: UtcTimestamp;
  enabled: boolean;
  entity_revision: Revision;
  tool_instance_id: ToolInstanceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolInstanceConfigurationChangedPayloadV1".
 */
export interface ToolInstanceConfigurationChangedPayloadV1 {
  changed_at: UtcTimestamp;
  configuration_hash: CanonicalSha256HashV1;
  configuration_revision: Revision;
  entity_revision: Revision;
  schema_hash: CanonicalSha256HashV1;
  tool_instance_id: ToolInstanceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolInstanceCredentialBindingChangedPayloadV1".
 */
export interface ToolInstanceCredentialBindingChangedPayloadV1 {
  binding_revision?: Revision | null;
  changed_at: UtcTimestamp;
  credential_id?: CredentialId | null;
  credential_revision?: string | null;
  entity_revision: Revision;
  tool_instance_id: ToolInstanceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceToolPinChangedPayloadV1".
 */
export interface WorkspaceToolPinChangedPayloadV1 {
  changed_at: UtcTimestamp;
  current_pin?: ToolPinV1 | null;
  entity_revision: Revision;
  previous_pin?: ToolPinV1 | null;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPinV1".
 */
export interface ToolPinV1 {
  revision: Revision;
  safe_reason?: string | null;
  state: ToolPinResolutionStateV1;
  tool_id: ToolId;
  tool_version: ToolVersion;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SessionToolPinChangedPayloadV1".
 */
export interface SessionToolPinChangedPayloadV1 {
  changed_at: UtcTimestamp;
  current_pin?: ToolPinV1 | null;
  entity_revision: Revision;
  previous_pin?: ToolPinV1 | null;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolRegistrySnapshotCreatedPayloadV1".
 */
export interface ToolRegistrySnapshotCreatedPayloadV1 {
  binding_set_hash: CanonicalSha256HashV1;
  created_at: UtcTimestamp;
  registry_snapshot_ref: ToolRegistrySnapshotRef;
  scope: ToolRegistryScopeV1;
  tool_set_hash: CanonicalSha256HashV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolBatchAdmittedPayloadV1".
 */
export interface ToolBatchAdmittedPayloadV1 {
  admitted_at: UtcTimestamp;
  batch_content_hash: CanonicalSha256HashV1;
  execution_plan_hash: CanonicalSha256HashV1;
  tool_batch_id: ToolBatchId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolBatchAdmissionRejectedPayloadV1".
 */
export interface ToolBatchAdmissionRejectedPayloadV1 {
  batch_content_hash: CanonicalSha256HashV1;
  error: ErrorEnvelope;
  internal_diagnostic_ref?: ArtifactId | null;
  rejected_at: UtcTimestamp;
  tool_batch_id: ToolBatchId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolExecutionAttemptStartedPayloadV1".
 */
export interface ToolExecutionAttemptStartedPayloadV1 {
  attempt_number: number;
  execution_plan_hash: CanonicalSha256HashV1;
  started_at: UtcTimestamp;
  tool_batch_id: ToolBatchId;
  tool_call_id: ToolCallId;
  tool_execution_attempt_id: ToolExecutionAttemptId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolExecutionAttemptDispatchPreparedPayloadV1".
 */
export interface ToolExecutionAttemptDispatchPreparedPayloadV1 {
  dispatch_evidence_hash: CanonicalSha256HashV1;
  prepared_at: UtcTimestamp;
  tool_execution_attempt_id: ToolExecutionAttemptId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolExecutionAttemptCancellationRequestedPayloadV1".
 */
export interface ToolExecutionAttemptCancellationRequestedPayloadV1 {
  reason: ToolCancellationReasonV1;
  requested_at: UtcTimestamp;
  tool_execution_attempt_id: ToolExecutionAttemptId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolExecutionAttemptSucceededPayloadV1".
 */
export interface ToolExecutionAttemptSucceededPayloadV1 {
  completed_at: UtcTimestamp;
  evidence_ref: ArtifactId;
  side_effect_state: SideEffectStateV1;
  tool_execution_attempt_id: ToolExecutionAttemptId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolExecutionAttemptFailedPayloadV1".
 */
export interface ToolExecutionAttemptFailedPayloadV1 {
  completed_at: UtcTimestamp;
  error: ErrorEnvelope;
  evidence_ref: ArtifactId;
  side_effect_state: SideEffectStateV1;
  tool_execution_attempt_id: ToolExecutionAttemptId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolExecutionAttemptCancelledPayloadV1".
 */
export interface ToolExecutionAttemptCancelledPayloadV1 {
  completed_at: UtcTimestamp;
  evidence_ref: ArtifactId;
  reason: ToolCancellationReasonV1;
  side_effect_state: SideEffectStateV1;
  tool_execution_attempt_id: ToolExecutionAttemptId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolExecutionAttemptIndeterminatePayloadV1".
 */
export interface ToolExecutionAttemptIndeterminatePayloadV1 {
  completed_at: UtcTimestamp;
  error: ErrorEnvelope;
  evidence_ref: ArtifactId;
  reason: ToolIndeterminateReasonV1;
  side_effect_state: SideEffectStateV1;
  tool_execution_attempt_id: ToolExecutionAttemptId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolResultFinalizedPayloadV1".
 */
export interface ToolResultFinalizedPayloadV1 {
  result: ToolResultV1;
  tool_batch_id: ToolBatchId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolListLoadedRequestV1".
 */
export interface ToolListLoadedRequestV1 {
  registry_snapshot_ref: ToolRegistrySnapshotRef;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolListLoadedResultV1".
 */
export interface ToolListLoadedResultV1 {
  registry_snapshot_ref: ToolRegistrySnapshotRef;
  tools: ToolLoadedEntryV1[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolLoadedEntryV1".
 */
export interface ToolLoadedEntryV1 {
  audience: ToolAudienceV1;
  canonical_name: ToolCanonicalName;
  definition_ref: ToolDefinitionSnapshotRef;
  display_name: string;
  load_state: ToolLoadStateV1;
  model_summary: string;
  source: ToolSourceKindV1;
  tool_instance_id: ToolInstanceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolLoadRequestV1".
 */
export interface ToolLoadRequestV1 {
  /**
   * @minItems 1
   * @maxItems 50
   */
  definitions: [ToolDefinitionSnapshotRef, ...ToolDefinitionSnapshotRef[]];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolLoadResultV1".
 */
export interface ToolLoadResultV1 {
  changed: boolean;
  resulting_registry_snapshot_ref: ToolRegistrySnapshotRef;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelVisibleToolResultEnvelopeV1".
 */
export interface ModelVisibleToolResultEnvelopeV1 {
  artifacts: ModelVisibleToolArtifactRefV1[];
  completed_at: UtcTimestamp;
  /**
   * Null only for a ToolNotFound call whose Provider name was absent from the frozen request.
   */
  definition_ref: ToolDefinitionSnapshotRef | null;
  outcome: ModelVisibleToolResultOutcomeV1;
  schema_version: ToolWireVersionV1;
  side_effect_state: SideEffectStateV1;
  tool_call_id: ToolCallId;
  /**
   * Items: A Renderer-safe, stable issue code from Aura's v1 normalized registry.
   *
   * This is deliberately a distinct domain type. Raw provider, Win32, DNS, or
   * TLS diagnostics must not be converted into it or embedded in its suffix.
   */
  warnings: string[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelVisibleToolArtifactRefV1".
 */
export interface ModelVisibleToolArtifactRefV1 {
  artifact_id: ArtifactId;
  byte_length: number;
  content_hash: ContentHash;
  media_type: string;
  slot: ToolOutputSlotKeyV1;
}
/**
 * Renderer/model-safe error surface. It deliberately excludes `details`,
 * causation chains, raw Runtime diagnostics, and Provider payloads.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelSafeToolErrorV1".
 */
export interface ModelSafeToolErrorV1 {
  category: ErrorCategory;
  code: ErrorCode;
  message: string;
  retryable: boolean;
}
/**
 * Schema aggregation only. Runtime IPC uses the individual method-specific
 * DTOs so no all-nullable mutation request can enter the authenticated path.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolMutationContractSchemaRootV1".
 */
export interface ToolMutationContractSchemaRootV1 {
  draft_apply: ToolDraftApplyRequestV1;
  draft_approve: ToolDraftApproveRequestV1;
  draft_cancel_conformance: ToolDraftCancelConformanceRequestV1;
  draft_create: ToolDraftCreateRequestV1;
  draft_import_archive: ToolDraftImportArchiveRequestV1;
  draft_import_directory: ToolDraftImportDirectoryRequestV1;
  draft_preview: ToolDraftValidateRequestV1;
  draft_reject: ToolDraftRejectRequestV1;
  draft_replace_snapshot: ToolDraftReplaceSnapshotRequestV1;
  draft_start_conformance: ToolDraftStartConformanceRequestV1;
  draft_validate: ToolDraftValidateRequestV1;
  instance_bind_credential: ToolInstanceBindCredentialRequestV1;
  instance_clear_credential: ToolInstanceClearCredentialRequestV1;
  instance_update_configuration: ToolInstanceUpdateConfigurationRequestV1;
  mutation_result: ToolMutationResultV1;
  operation_cancel: ToolOperationCancelRequestV1;
  package_approve_delete: ToolPackageApproveDeleteRequestV1;
  package_reject_delete: ToolPackageApproveDeleteRequestV1;
  package_request_delete: ToolPackageRequestDeleteRequestV1;
  package_set_enabled: ToolPackageSetEnabledRequestV1;
  package_set_trust: ToolPackageSetTrustRequestV1;
  session_pin_clear: SessionToolPinClearRequestV1;
  session_pin_set: SessionToolPinSetRequestV1;
  tool_set_enabled: ToolSetEnabledRequestV1;
  workspace_pin_clear: WorkspaceToolPinClearRequestV1;
  workspace_pin_set: WorkspaceToolPinSetRequestV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDraftApplyRequestV1".
 */
export interface ToolDraftApplyRequestV1 {
  expected_entity_revision: Revision;
  expected_fingerprint: CanonicalSha256HashV1;
  expected_preview_hash: CanonicalSha256HashV1;
  expected_security_fingerprint: CanonicalBlake3HashV1;
  expected_snapshot_hash: CanonicalSha256HashV1;
  tool_draft_id: ToolDraftId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDraftApproveRequestV1".
 */
export interface ToolDraftApproveRequestV1 {
  expected_entity_revision: Revision;
  expected_fingerprint: CanonicalSha256HashV1;
  expected_preview_hash: CanonicalSha256HashV1;
  expected_security_fingerprint: CanonicalBlake3HashV1;
  expected_snapshot_hash: CanonicalSha256HashV1;
  tool_draft_id: ToolDraftId;
  trust_after_install: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDraftCancelConformanceRequestV1".
 */
export interface ToolDraftCancelConformanceRequestV1 {
  expected_operational_revision: Revision;
  tool_conformance_run_id: ToolConformanceRunId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDraftCreateRequestV1".
 */
export interface ToolDraftCreateRequestV1 {}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDraftImportArchiveRequestV1".
 */
export interface ToolDraftImportArchiveRequestV1 {
  source: ToolDraftArchiveSourceV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDraftImportDirectoryRequestV1".
 */
export interface ToolDraftImportDirectoryRequestV1 {
  directory: FileTargetRef;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDraftValidateRequestV1".
 */
export interface ToolDraftValidateRequestV1 {
  expected_entity_revision: Revision;
  expected_snapshot_hash: CanonicalSha256HashV1;
  tool_draft_id: ToolDraftId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDraftRejectRequestV1".
 */
export interface ToolDraftRejectRequestV1 {
  expected_entity_revision: Revision;
  expected_preview_hash: CanonicalSha256HashV1;
  expected_snapshot_hash: CanonicalSha256HashV1;
  tool_draft_id: ToolDraftId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDraftReplaceSnapshotRequestV1".
 */
export interface ToolDraftReplaceSnapshotRequestV1 {
  expected_draft_revision: Revision;
  expected_snapshot_hash: CanonicalSha256HashV1;
  snapshot_artifact: ToolDraftTransferArtifactRefV1;
  tool_draft_id: ToolDraftId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolDraftStartConformanceRequestV1".
 */
export interface ToolDraftStartConformanceRequestV1 {
  conformance_case: ToolPackageConformanceCaseNameV1;
  expected_entity_revision: Revision;
  expected_snapshot_hash: CanonicalSha256HashV1;
  instance_configuration?: any;
  tool_draft_id: ToolDraftId;
  tool_key: ToolKey;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolInstanceBindCredentialRequestV1".
 */
export interface ToolInstanceBindCredentialRequestV1 {
  credential_id: CredentialId;
  /**
   * Secret-generation revision for one stable CredentialId.
   *
   * This is intentionally distinct from a row revision. Clear and metadata-only
   * mutations may change row concurrency without creating a new secret
   * generation.
   */
  credential_revision: string;
  credential_slot_key: CredentialSlotKey;
  expected_binding_revision?: Revision | null;
  expected_candidate_set_revision: Revision;
  expected_entity_revision: Revision;
  placement: ToolCredentialPlacementV1;
  session_id?: SessionId | null;
  tool_instance_id: ToolInstanceId;
  workspace_id?: WorkspaceId | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolInstanceClearCredentialRequestV1".
 */
export interface ToolInstanceClearCredentialRequestV1 {
  expected_binding_revision: Revision;
  expected_entity_revision: Revision;
  tool_instance_id: ToolInstanceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolInstanceUpdateConfigurationRequestV1".
 */
export interface ToolInstanceUpdateConfigurationRequestV1 {
  configuration: any;
  expected_configuration_revision?: Revision | null;
  expected_entity_revision: Revision;
  expected_schema_hash: CanonicalSha256HashV1;
  tool_instance_id: ToolInstanceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolLongOperationAcceptedV1".
 */
export interface ToolLongOperationAcceptedV1 {
  operation: ToolLongOperationRefV1;
  operational_revision: Revision;
  state: ToolLongOperationStateV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolOperationCancelRequestV1".
 */
export interface ToolOperationCancelRequestV1 {
  expected_operational_revision: Revision;
  operation: ToolOperationRefV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageApproveDeleteRequestV1".
 */
export interface ToolPackageApproveDeleteRequestV1 {
  expected_entity_revision: Revision;
  expected_persistent_impact_hash: CanonicalSha256HashV1;
  expected_preview_hash: CanonicalSha256HashV1;
  expected_snapshot_hash: CanonicalSha256HashV1;
  tool_draft_id: ToolDraftId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageRequestDeleteRequestV1".
 */
export interface ToolPackageRequestDeleteRequestV1 {
  expected_entity_revision: Revision;
  expected_fingerprint: CanonicalSha256HashV1;
  expected_security_fingerprint: CanonicalBlake3HashV1;
  tool_package_id: ToolPackageId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageSetEnabledRequestV1".
 */
export interface ToolPackageSetEnabledRequestV1 {
  enabled: boolean;
  expected_entity_revision: Revision;
  tool_package_id: ToolPackageId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolPackageSetTrustRequestV1".
 */
export interface ToolPackageSetTrustRequestV1 {
  expected_entity_revision: Revision;
  expected_fingerprint: CanonicalBlake3HashV1;
  tool_package_id: ToolPackageId;
  trusted: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SessionToolPinClearRequestV1".
 */
export interface SessionToolPinClearRequestV1 {
  expected_entity_revision: Revision;
  session_id: SessionId;
  tool_id: ToolId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SessionToolPinSetRequestV1".
 */
export interface SessionToolPinSetRequestV1 {
  expected_entity_revision?: Revision | null;
  session_id: SessionId;
  tool_id: ToolId;
  tool_version: ToolVersion;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolSetEnabledRequestV1".
 */
export interface ToolSetEnabledRequestV1 {
  enabled: boolean;
  expected_entity_revision: Revision;
  tool_id: ToolId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceToolPinClearRequestV1".
 */
export interface WorkspaceToolPinClearRequestV1 {
  expected_entity_revision: Revision;
  tool_id: ToolId;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceToolPinSetRequestV1".
 */
export interface WorkspaceToolPinSetRequestV1 {
  expected_entity_revision?: Revision | null;
  tool_id: ToolId;
  tool_version: ToolVersion;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageFingerprintPreimageV1".
 */
export interface PackageFingerprintPreimageV1 {
  canonical_manifest: CanonicalToolPackageManifestV1;
  kind: PackageFingerprintKindV1;
  package_key: PackageKey;
  package_version: CanonicalSemVer;
  preimage_version: FingerprintPreimageVersionV1;
  runtime_closure: PackageRuntimeClosureV1;
  /**
   * @minItems 1
   */
  tool_contract_fingerprints: [ToolContractFingerprintEntryV1, ...ToolContractFingerprintEntryV1[]];
}
/**
 * An authoritative V1 full-package Runtime Closure. Its fields are private
 * and it has no general `Deserialize` implementation: callers can only obtain
 * it from a staged runtime root whose complete file set was read and hashed,
 * or reconcile stored bytes against that same evidence.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageRuntimeClosureV1".
 */
export interface PackageRuntimeClosureV1 {
  canonical_manifest_hash: CanonicalSha256HashV1;
  /**
   * @maxItems 4095
   */
  files: PackageRuntimeClosureFileV1[];
  policy_version: PackageRuntimeClosurePolicyVersionV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageRuntimeClosureFileV1".
 */
export interface PackageRuntimeClosureFileV1 {
  byte_length: number;
  /**
   * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
   * via the `definition` "PackageRelativePath".
   */
  path: PackageRelativePath;
  raw_file_sha256: ToolPackageFileSha256V1;
  role: ToolPackageFileRoleV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolContractFingerprintEntryV1".
 */
export interface ToolContractFingerprintEntryV1 {
  fingerprint: ToolContractFingerprintV1;
  tool_key: ToolKey;
}
/**
 * Fixed, user-facing reasons. Never includes Rust type names, raw JSON values,
 * credentials or a chained operating-system error.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageImportDiagnosticV1".
 */
export interface PackageImportDiagnosticV1 {
  code: PackageImportDiagnosticCodeV1;
  location: PackageImportDiagnosticLocationV1;
  message: string;
}
/**
 * The full canonical manifest retains tool descriptions, capabilities, resource
 * claims, HTTP target/path mappings and credential slot declarations. UI must
 * display these rather than infer an endpoint or omit a declared requirement.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageImportPreviewV1".
 */
export interface PackageImportPreviewV1 {
  conformance: PackageConformanceStateV1;
  content: PackageContentDescriptorV1;
  contract_version: ToolPackageManifestSchemaVersionV1;
  install_effect: PackageInstallEffectV1;
  runtimes: PackageRuntimePreviewV1[];
  version: PackageVersionBindingV1;
}
/**
 * Immutable content facts; directory bindings and Conformance are deliberately
 * outside this object so identical content can share one payload.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageContentDescriptorV1".
 */
export interface PackageContentDescriptorV1 {
  manifest: CanonicalToolPackageManifestV1;
  package_fingerprint: CanonicalSha256HashV1;
  runtime_closure_hash: CanonicalSha256HashV1;
  security_fingerprint: CanonicalBlake3HashV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageRuntimePreviewV1".
 */
export interface PackageRuntimePreviewV1 {
  execution_location: PackageExecutionLocationV1;
  support: PackageRuntimeSupportV1;
  tool_key: ToolKey;
  working_directory: PackageWorkingDirectoryPreviewV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageVersionBindingV1".
 */
export interface PackageVersionBindingV1 {
  package_fingerprint: CanonicalSha256HashV1;
  package_id: ToolPackageId;
  package_version: CanonicalSemVer;
  revision: Revision;
}
/**
 * The file representation authored by a Package producer. This type is a
 * contract only; P07-DG-001 forbids wiring it to file parsing or installation
 * before the project owner reviews the generated draft.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RawToolPackageManifestV1".
 */
export interface RawToolPackageManifestV1 {
  /**
   * @maxItems 4095
   */
  files: ToolPackageFileEntryV1[];
  minimum_aura_version: CanonicalSemVer;
  package_key: PackageKey;
  package_version: CanonicalSemVer;
  schema_version: ToolPackageManifestSchemaVersionV1;
  /**
   * @minItems 1
   */
  tools: [RawToolPackageToolV1, ...RawToolPackageToolV1[]];
  ui: RawToolPackageUiV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RawToolPackageToolV1".
 */
export interface RawToolPackageToolV1 {
  audience?: PackageToolAudienceV1 | null;
  capability_requirements: PackageCapabilityRequirementV1[];
  /**
   * @minItems 1
   */
  conformance_cases: [ToolPackageConformanceCaseV1, ...ToolPackageConformanceCaseV1[]];
  execution_semantics: ToolExecutionSemanticsV1;
  instance_configuration: ToolPackageJsonObjectV1 | null;
  model: RawToolPackageModelContractV1;
  resource_claims: PackageDeclaredResourceClaimV1[];
  runtime: RawToolPackageRuntimeV1;
  semantic_version?: CanonicalSemVer | null;
  tool_key: ToolKey;
  ui: ToolPackageToolUiV1;
}
/**
 * Package-author supplied model contract before schema normalization and
 * hashing. The JSON Schema documents are the only intentionally open JSON
 * objects in this structure.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RawToolPackageModelContractV1".
 */
export interface RawToolPackageModelContractV1 {
  canonical_suffix: PackageToolCanonicalSuffixV1;
  description: RawToolPackageModelDescriptionV1;
  input_schema: ToolPackageJsonObjectV1;
  result_contract: RawToolPackageResultContractV1;
  strict: boolean;
  summary: ToolPackageModelSummaryV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RawToolPackageDeclarativeHttpV1".
 */
export interface RawToolPackageDeclarativeHttpV1 {
  body: RawToolPackageDeclarativeHttpBodyV1;
  credential_slot?: HttpCredentialSlotV1 | null;
  headers: HttpHeaderEntryV1[];
  method: DeclarativeHttpMethodV1;
  path: HttpPathSegmentV1[];
  query: HttpQueryEntryV1[];
  success_status_ranges?: ToolPackageHttpSuccessStatusRangesV1 | null;
  target: DeclarativeHttpTargetV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RawToolPackageHttpJsonPropertyV1".
 */
export interface RawToolPackageHttpJsonPropertyV1 {
  name: HttpJsonPropertyNameV1;
  value: RawToolPackageHttpJsonTemplateV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RawToolPackageUiV1".
 */
export interface RawToolPackageUiV1 {
  description?: RawToolPackageUiDescriptionV1 | null;
  display_name: ToolPackageUiDisplayNameV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageSecurityFingerprintPreimageV1".
 */
export interface PackageSecurityFingerprintPreimageV1 {
  canonical_manifest: CanonicalToolPackageManifestV1;
  kind: PackageSecurityFingerprintKindV1;
  package_key: PackageKey;
  package_version: CanonicalSemVer;
  preimage_version: FingerprintPreimageVersionV1;
  runtime_closure: PackageRuntimeClosureV1;
  /**
   * @minItems 1
   */
  tool_contract_fingerprints: [ToolContractFingerprintEntryV1, ...ToolContractFingerprintEntryV1[]];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolReadContractSchemaRootV1".
 */
export interface ToolReadContractSchemaRootV1 {
  credential_candidates: ToolCredentialCandidatesRequestV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCredentialCandidatesRequestV1".
 */
export interface ToolCredentialCandidatesRequestV1 {
  credential_slot_key: CredentialSlotKey;
  expected_instance_revision: Revision;
  placement: ToolCredentialPlacementV1;
  session_id?: SessionId | null;
  tool_instance_id: ToolInstanceId;
  workspace_id?: WorkspaceId | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolRegistrySnapshotV1".
 */
export interface ToolRegistrySnapshotV1 {
  /**
   * Every discoverable exact Tool version frozen in this scoped snapshot.
   */
  catalog_entries: ToolRegistryCatalogEntryV1[];
  created_at: UtcTimestamp;
  derivation_hash?: CanonicalSha256HashV1 | null;
  /**
   * Only the Core/Pinned/Ephemeral tools exposed to the current ModelStep.
   */
  effective_bindings: EffectiveToolBindingSnapshot[];
  eligibility_context_hash: CanonicalSha256HashV1;
  /**
   * The discoverable tools this step can call through `tools.call` without their
   * definitions being sent; each is `ToolExposureV1::UnifiedEntry`. Absent in snapshots made
   * before the unified entry, which therefore offer it nothing.
   */
  entry_bindings?: EffectiveToolBindingSnapshot[];
  parent_registry_snapshot_ref?: ToolRegistrySnapshotRef | null;
  schema_set: ToolSchemaSetSnapshotV1;
  scope: ToolRegistryScopeV1;
  snapshot_ref: ToolRegistrySnapshotRef;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolRegistryCatalogEntryV1".
 */
export interface ToolRegistryCatalogEntryV1 {
  canonical_name: ToolCanonicalName;
  definition_fingerprint: CanonicalSha256HashV1;
  display_name: string;
  load_state: ToolLoadStateV1;
  source: ToolSourceKindV1;
  source_contract_fingerprint: ToolSourceContractFingerprintV1;
  summary: string;
  tool_id: ToolId;
  tool_instance_id: ToolInstanceId;
  tool_version: ToolVersion;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "DeclarativeHttpV1".
 */
export interface DeclarativeHttpV1 {
  body: DeclarativeHttpBodyV1;
  credential_slot?: HttpCredentialSlotV1 | null;
  headers: HttpHeaderEntryV1[];
  method: DeclarativeHttpMethodV1;
  path: HttpPathSegmentV1[];
  query: HttpQueryEntryV1[];
  success_status_ranges: HttpStatusRangeV1[];
  target: DeclarativeHttpTargetV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HttpStatusRangeV1".
 */
export interface HttpStatusRangeV1 {
  end: number;
  start: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolSearchPageV1".
 */
export interface ToolSearchPageV1 {
  next_cursor?: ToolSearchCursorV1 | null;
  registry_snapshot: ToolRegistrySnapshotRef;
  results: ToolSearchHitV1[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolSearchHitV1".
 */
export interface ToolSearchHitV1 {
  canonical_name: string;
  display_name: string;
  load_state: ToolLoadStateV1;
  source: ToolSourceKindV1;
  summary: string;
  tool_id: ToolId;
  tool_version: ToolVersion;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolSearchRequestV1".
 */
export interface ToolSearchRequestV1 {
  cursor?: ToolSearchCursorV1 | null;
  limit: number;
  query: ToolSearchQueryV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "StdioHelloPayloadV1".
 */
export interface StdioHelloPayloadV1 {
  runtime_name: string;
  runtime_version: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "StdioExecutePayloadV1".
 */
export interface StdioExecutePayloadV1 {
  arguments: any;
  arguments_hash: ContentHash;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "StdioEmptyPayloadV1".
 */
export interface StdioEmptyPayloadV1 {}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "StdioProgressPayloadV1".
 */
export interface StdioProgressPayloadV1 {
  message?: string | null;
  percentage?: number | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "StdioArtifactReadyPayloadV1".
 */
export interface StdioArtifactReadyPayloadV1 {
  byte_length: number;
  content_hash: ContentHash;
  media_type: string;
  /**
   * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
   * via the `definition` "ToolOutputRelativePathV1".
   */
  relative_path: ToolOutputRelativePathV1;
  slot: ToolOutputSlotKeyV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "StdioFinalResultPayloadV1".
 */
export interface StdioFinalResultPayloadV1 {
  result: any;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "StdioFinalFailurePayloadV1".
 */
export interface StdioFinalFailurePayloadV1 {
  code: string;
  safe_message: string;
}
/**
 * Exact V1 preimage for one Package-authored Tool contract. Fields are
 * private and the type cannot be deserialized independently of authoritative
 * staging evidence.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolContractFingerprintPreimageV1".
 */
export interface ToolContractFingerprintPreimageV1 {
  kind: ToolContractFingerprintKindV1;
  package_key: PackageKey;
  preimage_version: FingerprintPreimageVersionV1;
  runtime_closure_hash: CanonicalSha256HashV1;
  tool_contract: CanonicalToolPackageToolV1;
  tool_key: ToolKey;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "UserAuditToolResultEnvelopeV1".
 */
export interface UserAuditToolResultEnvelopeV1 {
  arguments_hash?: CanonicalSha256HashV1 | null;
  artifacts: UserReadableToolArtifactView[];
  attempts: UserAuditAttemptSummaryV1[];
  completed_at: UtcTimestamp;
  definition_ref: ToolDefinitionSnapshotRef;
  ordinal: number;
  outcome: ToolResultOutcomeSummaryV1;
  purpose?: ToolPurposeTextV1 | null;
  schema_version: ToolWireVersionV1;
  side_effect_state: SideEffectStateV1;
  source: ToolSourceKindV1;
  started_at?: UtcTimestamp | null;
  tool_batch_id: ToolBatchId;
  tool_call_id: ToolCallId;
  tool_instance_id: ToolInstanceId;
  /**
   * Items: A Renderer-safe, stable issue code from Aura's v1 normalized registry.
   *
   * This is deliberately a distinct domain type. Raw provider, Win32, DNS, or
   * TLS diagnostics must not be converted into it or embedded in its suffix.
   */
  warnings: string[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "UserAuditAttemptSummaryV1".
 */
export interface UserAuditAttemptSummaryV1 {
  attempt_number: number;
  completed_at?: UtcTimestamp | null;
  safe_error?: ModelSafeToolErrorV1 | null;
  side_effect_state: SideEffectStateV1;
  started_at: UtcTimestamp;
  status: ToolExecutionAttemptStatusV1;
  tool_execution_attempt_id: ToolExecutionAttemptId;
}
/**
 * Core-verified evidence for one exact Runtime attempt. The Runtime-facing
 * candidate contract never carries this type; only trusted Core adapters may
 * create it after checking the declared effect scope and receipt facts.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "VerifiedAttemptSideEffectEvidenceV1".
 */
export interface VerifiedAttemptSideEffectEvidenceV1 {
  effect_scope_hash: CanonicalSha256HashV1;
  evidence_hash: CanonicalSha256HashV1;
  evidence_policy_version: ToolWireVersionV1;
  producer: SideEffectEvidenceProducerV1;
  proof: AttemptSideEffectProofV1;
  tool_execution_attempt_id: ToolExecutionAttemptId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceFilesContractFixture".
 */
export interface WorkspaceFilesContractFixture {
  artifact_capture_workspace_file?: CaptureWorkspaceFileRequest | null;
  artifact_capture_workspace_file_result?: CaptureWorkspaceFileResult | null;
  artifact_export?: ArtifactExportRequest | null;
  artifact_export_result?: ArtifactExportResult | null;
  buffer_edit?: BufferEditCommand | null;
  buffer_edit_result?: BufferEditResult | null;
  buffer_open?: BufferOpenCommand | null;
  buffer_save?: BufferSaveCommand | null;
  fs_apply_changeset?: FsApplyChangesetRequest | null;
  fs_apply_changeset_result?: FsApplyChangesetResult | null;
  fs_apply_patch?: FsApplyPatchRequest | null;
  fs_copy?: FsCopyRequest | null;
  fs_create_directory?: FsCreateDirectoryRequest | null;
  fs_delete?: FsDeleteRequest | null;
  fs_find_paths?: FsFindPathsRequest | null;
  fs_glob?: FsGlobRequest | null;
  fs_glob_result?: FsGlobResult | null;
  fs_hash?: FsHashRequest | null;
  fs_hash_result?: FsHashResult | null;
  fs_inspect?: FsInspectRequest | null;
  fs_inspect_result?: FsInspectResult | null;
  fs_list?: FsListRequest | null;
  fs_list_result?: FsListResult | null;
  fs_move?: FsMoveRequest | null;
  fs_mutation_result?: FsMutationResult | null;
  fs_read_binary_range?: FsReadBinaryRangeRequest | null;
  fs_read_binary_range_result?: FsReadBinaryRangeResult | null;
  fs_read_text?: FsReadTextRequest | null;
  fs_read_text_result?: FsReadTextResult | null;
  fs_search_text?: FsSearchTextRequest | null;
  fs_search_text_result?: FsSearchTextResult | null;
  fs_stat?: FsStatRequest | null;
  fs_stat_result?: FsStatResult | null;
  fs_write_file?: FsWriteFileRequest | null;
  internal_file_target: FileTargetRef;
  lifecycle_ack?: EditorLifecycleTransitionAck | null;
  lifecycle_begin?: BeginWorkspaceLifecycleInput | null;
  lifecycle_begin_result?: BeginWorkspaceLifecycleResult | null;
  lifecycle_discard_confirmation?: DiscardConfirmation | null;
  lifecycle_issue_discard_confirmation?: IssueDiscardConfirmationInput | null;
  lifecycle_prepare?: PrepareEditorLifecycleTransition | null;
  lifecycle_prepare_delivered?: WorkspaceLifecyclePrepareDeliveredInput | null;
  lifecycle_query?: WorkspaceLifecycleOperationQuery | null;
  lifecycle_renderer_unavailable?: RendererLifecycleUnavailableInput | null;
  lifecycle_resolve?: ResolveWorkspaceLifecycleInput | null;
  logical_file_snapshot?: LogicalFileSnapshot | null;
  model_visible_workspace_target: WorkspaceFileTarget;
  path_search?: PathSearchCommand | null;
  path_search_result?: PathSearchResult | null;
  workspace_archive?: WorkspaceArchiveCommand | null;
  workspace_create?: WorkspaceCreateCommand | null;
  workspace_create_result?: WorkspaceCreateResult | null;
  workspace_delete?: WorkspaceDeleteCommand | null;
  workspace_delete_result?: WorkspaceDeleteResult | null;
  workspace_files_projection_change: ProjectionChange;
  workspace_relocate?: WorkspaceRelocateCommand | null;
  workspace_restore?: WorkspaceRestoreCommand | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CaptureWorkspaceFileRequest".
 */
export interface CaptureWorkspaceFileRequest {
  session_id: SessionId;
  target: WorkspaceFileTarget;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceFileTarget".
 */
export interface WorkspaceFileTarget {
  path: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CaptureWorkspaceFileResult".
 */
export interface CaptureWorkspaceFileResult {
  artifact: ArtifactRecord;
  logical_hash: ContentHash;
  path: string;
  source: FileStorageSource;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ArtifactExportRequest".
 */
export interface ArtifactExportRequest {
  artifact_id: ArtifactId;
  expected: FsExpectedState;
  session_id: SessionId;
  target: ArtifactExportTarget;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ArtifactExportResult".
 */
export interface ArtifactExportResult {
  artifact: ArtifactRecord;
  content_hash: ContentHash;
  deferred_mutation_id?: DeferredMutationId | null;
  disposition: FsMutationDisposition;
  requires_reread: boolean;
  target: ArtifactExportTarget;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BufferEditCommand".
 */
export interface BufferEditCommand {
  active_editing: boolean;
  base_core_revision: Revision;
  buffer_revision: Revision;
  content: string;
  ime_composition_active: boolean;
  path: string;
  reservation_epoch: Revision;
  reservation_id: EditorReservationId;
  session_id: SessionId;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BufferEditResult".
 */
export interface BufferEditResult {
  deferred_intents_superseded: number;
  requires_model_reread: boolean;
  status: LogicalFileStatus;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BufferOpenCommand".
 */
export interface BufferOpenCommand {
  path: string;
  session_id: SessionId;
  workspace_id: WorkspaceId;
  writable: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BufferSaveCommand".
 */
export interface BufferSaveCommand {
  expected_buffer_revision: Revision;
  path: string;
  reservation_epoch: Revision;
  reservation_id: EditorReservationId;
  session_id: SessionId;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsApplyChangesetRequest".
 */
export interface FsApplyChangesetRequest {
  operations: FsChangesetOperation[];
  session_id: SessionId;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsApplyChangesetResult".
 */
export interface FsApplyChangesetResult {
  mutation: FsMutationResult;
  steps: FsChangesetStepResult[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsMutationResult".
 */
export interface FsMutationResult {
  affected_paths: string[];
  catalog_refresh_pending: boolean;
  deferred_mutation_id?: DeferredMutationId | null;
  disposition: FsMutationDisposition;
  /**
   * Present when this mutation wrote through an authorized external target, so a
   * caller can tell an external file from a workspace file without guessing from
   * the path. `affected_paths` then holds the path relative to the grant root.
   */
  external?: FsExternalWriteTarget | null;
  requires_reread: boolean;
  /**
   * The workspace revision at the time of the mutation. A mutation of an
   * authorized external file does not change it: that file is not indexed and
   * has no workspace revision of its own.
   */
  workspace_revision: string;
}
/**
 * The authorized external file one mutation wrote.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsExternalWriteTarget".
 */
export interface FsExternalWriteTarget {
  bytes_written: number;
  /**
   * True when this call created the file instead of replacing one.
   */
  created: boolean;
  /**
   * The absolute path as the user sees it.
   */
  display_path: string;
  encoding: TextEncoding;
  grant_id?: ExternalGrantId | null;
  line_ending: LineEnding;
  logical_hash: ContentHash3;
  /**
   * Hash the file had before the write. Absent for a create.
   */
  previous_hash?: ContentHash | null;
}
/**
 * Hash of the bytes this mutation wrote.
 */
export interface ContentHash3 {
  algorithm: ContentHashAlgorithm;
  hex: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsChangesetStepResult".
 */
export interface FsChangesetStepResult {
  /**
   * The hash of the bytes a write or patch step wrote, the file's logical_hash after it.
   */
  after_hash?: ContentHash | null;
  index: number;
  operation: string;
  paths: string[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsApplyPatchRequest".
 */
export interface FsApplyPatchRequest {
  expected_hash: ContentHash;
  session_id: SessionId;
  target: WorkspaceFileTarget;
  unified_diff: string;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsCopyRequest".
 */
export interface FsCopyRequest {
  destination: WorkspaceFileTarget;
  destination_expected: FsExpectedState;
  session_id: SessionId;
  source: WorkspaceFileTarget;
  source_expected: FsExpectedState;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsCreateDirectoryRequest".
 */
export interface FsCreateDirectoryRequest {
  session_id: SessionId;
  target: WorkspaceFileTarget;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsDeleteRequest".
 */
export interface FsDeleteRequest {
  expected: FsExpectedState;
  recursive: boolean;
  session_id: SessionId;
  target: WorkspaceFileTarget;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsFindPathsRequest".
 */
export interface FsFindPathsRequest {
  limit: number;
  query: string;
  session_id: SessionId;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsGlobRequest".
 */
export interface FsGlobRequest {
  limit: number;
  pattern: string;
  session_id: SessionId;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsGlobResult".
 */
export interface FsGlobResult {
  catalog_revision: Revision;
  paths: string[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsHashRequest".
 */
export interface FsHashRequest {
  session_id: SessionId;
  target: WorkspaceFileTarget;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsHashResult".
 */
export interface FsHashResult {
  active_editor_session?: SessionId | null;
  disk_hash?: ContentHash | null;
  disk_may_be_stale: boolean;
  hash: ContentHash;
  is_dirty: boolean;
  path: string;
  source: FileStorageSource;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsInspectRequest".
 */
export interface FsInspectRequest {
  session_id: SessionId;
  target: WorkspaceFileTarget;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsInspectResult".
 */
export interface FsInspectResult {
  byte_length: number;
  content_hash: ContentHash;
  extension?: string | null;
  is_dirty: boolean;
  is_executable: boolean;
  is_text: boolean;
  magic_description: string;
  media_type: string;
  path: string;
  source: FileStorageSource;
  suitable_for_native_attachment: boolean;
  text_encoding?: TextEncoding | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsListRequest".
 */
export interface FsListRequest {
  session_id: SessionId;
  target: WorkspaceFileTarget;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsListResult".
 */
export interface FsListResult {
  catalog_revision: Revision;
  entries: FsListEntry[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsListEntry".
 */
export interface FsListEntry {
  byte_length?: number | null;
  generated: boolean;
  ignored: boolean;
  kind: FsNodeKind;
  name: string;
  path: string;
  reparse_tag?: number | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsMoveRequest".
 */
export interface FsMoveRequest {
  destination: WorkspaceFileTarget;
  destination_expected: FsExpectedState;
  session_id: SessionId;
  source: WorkspaceFileTarget;
  source_expected: FsExpectedState;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsReadBinaryRangeRequest".
 */
export interface FsReadBinaryRangeRequest {
  format: BinaryDisplayFormat;
  length: number;
  offset: number;
  session_id: SessionId;
  target: WorkspaceFileTarget;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsReadBinaryRangeResult".
 */
export interface FsReadBinaryRangeResult {
  content_hash: ContentHash;
  data: string;
  format: BinaryDisplayFormat;
  offset: number;
  path: string;
  returned_bytes: number;
  source: FileStorageSource;
  total_bytes: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsReadTextRequest".
 */
export interface FsReadTextRequest {
  explicit_encoding?: TextEncoding | null;
  range: TextReadRange;
  session_id: SessionId;
  target: WorkspaceFileTarget;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsReadTextResult".
 */
export interface FsReadTextResult {
  active_editor_session?: SessionId | null;
  buffer_revision?: Revision | null;
  disk_hash?: ContentHash | null;
  disk_may_be_stale: boolean;
  encoding: TextEncoding;
  /**
   * Present when this read used an authorized external target. It names the
   * grant that authorized it and the absolute path the caller referred to, so
   * the model and the audit trail can tell an external file from a workspace
   * file without guessing from the path.
   */
  external?: FsExternalReadTarget | null;
  is_dirty: boolean;
  line_ending: LineEnding;
  logical_hash: ContentHash;
  /**
   * The path the read used, relative to its root: the workspace root for an
   * ordinary read, or the authorized external root when `external` is present.
   */
  path: string;
  returned_range: TextReadRange;
  source: FileStorageSource;
  text: string;
  total_characters: number;
  total_lines: number;
}
/**
 * The authorized external target one read used.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsExternalReadTarget".
 */
export interface FsExternalReadTarget {
  /**
   * The absolute path as the user sees it. `path` above is the same file
   * relative to this grant's root.
   */
  display_path: string;
  /**
   * The session grant that authorized the target, when a grant did. A path the
   * existing policy allowed directly — an ordinary external file in FullAccess —
   * has no grant, and the empty value is how the model and the audit trail can
   * tell the two sources apart.
   */
  grant_id?: ExternalGrantId | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsSearchTextRequest".
 */
export interface FsSearchTextRequest {
  include_generated: boolean;
  include_ignored: boolean;
  limit: number;
  path_glob?: string | null;
  pattern: FsSearchPattern;
  root: string;
  session_id: SessionId;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsSearchTextResult".
 */
export interface FsSearchTextResult {
  candidate_file_count: number;
  catalog_revision: Revision;
  completion: FsSearchCompletion;
  /**
   * Present when this search ran outside the workspace. There is no external index to
   * read, so the match paths are relative to `external.display_path` and
   * `catalog_revision` stays the workspace's own revision, which an external search
   * does not change.
   */
  external?: FsExternalSearchTarget | null;
  matches: FsSearchTextMatch[];
  scanned_bytes: number;
  /**
   * Files whose content was opened and examined. A file that is silently
   * excluded (binary content) is counted here *and* in
   * [`Self::skipped_file_count`], so the two counters are independent
   * observations and their sum may exceed the candidate count.
   */
  scanned_file_count: number;
  /**
   * Files not contributing matches: silently excluded content plus files
   * reported as skipped with a reason.
   */
  skipped_file_count: number;
  skipped_scopes?: FsSkippedScope[];
}
/**
 * The authorized external directory one text search used.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsExternalSearchTarget".
 */
export interface FsExternalSearchTarget {
  /**
   * The absolute directory path as the user sees it. Each match's `path` is relative
   * to this directory.
   */
  display_path: string;
  /**
   * The session grant that authorized the search, when a grant did. A search the
   * existing policy allowed directly has none.
   */
  grant_id?: ExternalGrantId | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsSearchTextMatch".
 */
export interface FsSearchTextMatch {
  buffer_revision?: Revision | null;
  column_character_offset: number;
  excerpt: string;
  excerpt_start_character_offset: number;
  excerpt_truncated_after: boolean;
  excerpt_truncated_before: boolean;
  generated: boolean;
  ignored: boolean;
  line_index: number;
  logical_hash: ContentHash;
  match_character_length: number;
  path: string;
  source: FileStorageSource;
}
/**
 * A verified range excluded from an Agent's traversal. It is not an empty match.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsSkippedScope".
 */
export interface FsSkippedScope {
  canonical_path: string;
  reason: FsSkippedScopeReason;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsStatRequest".
 */
export interface FsStatRequest {
  session_id: SessionId;
  target: WorkspaceFileTarget;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsStatResult".
 */
export interface FsStatResult {
  active_editor_session?: SessionId | null;
  byte_length?: number | null;
  content_hash?: ContentHash | null;
  generated: boolean;
  ignored: boolean;
  kind: FsNodeKind;
  modified_at?: string | null;
  path: string;
  reparse_tag?: number | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsWriteFileRequest".
 */
export interface FsWriteFileRequest {
  content: string;
  encoding?: TextEncoding | null;
  expected: FsExpectedState;
  line_ending?: LineEnding | null;
  session_id: SessionId;
  target: WorkspaceFileTarget;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "EditorLifecycleTransitionAck".
 */
export interface EditorLifecycleTransitionAck {
  buffers: EditorBufferLifecycleState[];
  lifecycle_revision: Revision;
  operation_id: WorkspaceLifecycleOperationId;
  renderer_instance_id: RendererInstanceId;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "EditorBufferLifecycleState".
 */
export interface EditorBufferLifecycleState {
  buffer_revision: Revision;
  canonical_relative_path: string;
  core_acknowledged_revision: Revision;
  disk_hash?: ContentHash | null;
  ime_composition_active: boolean;
  is_active_editing: boolean;
  is_dirty: boolean;
  logical_hash: ContentHash;
  reservation_epoch: Revision;
  reservation_id?: EditorReservationId | null;
  save_state: EditorSaveState;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BeginWorkspaceLifecycleInput".
 */
export interface BeginWorkspaceLifecycleInput {
  expected_workspace_revision: Revision;
  operation: WorkspaceLifecycleOperation;
  renderer_instance_id: RendererInstanceId;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BeginWorkspaceLifecycleResult".
 */
export interface BeginWorkspaceLifecycleResult {
  ack_deadline_at: UtcTimestamp;
  lifecycle_revision: Revision;
  operation_id: WorkspaceLifecycleOperationId;
  state: WorkspaceLifecycleState;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "DiscardConfirmation".
 */
export interface DiscardConfirmation {
  affected_buffers: DiscardBufferBinding[];
  confirmation_nonce: SecretNonce;
  expires_at: UtcTimestamp;
  lifecycle_revision: Revision;
  operation_id: WorkspaceLifecycleOperationId;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "DiscardBufferBinding".
 */
export interface DiscardBufferBinding {
  buffer_revision: Revision;
  canonical_relative_path: string;
  disk_hash?: ContentHash | null;
  logical_hash: ContentHash;
  reservation_epoch: Revision;
  reservation_id: EditorReservationId;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "IssueDiscardConfirmationInput".
 */
export interface IssueDiscardConfirmationInput {
  expected_lifecycle_revision: Revision;
  operation_id: WorkspaceLifecycleOperationId;
  renderer_instance_id: RendererInstanceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PrepareEditorLifecycleTransition".
 */
export interface PrepareEditorLifecycleTransition {
  deadline_at: UtcTimestamp;
  lifecycle_revision: Revision;
  operation: WorkspaceLifecycleOperationKind;
  operation_id: WorkspaceLifecycleOperationId;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceLifecyclePrepareDeliveredInput".
 */
export interface WorkspaceLifecyclePrepareDeliveredInput {
  expected_lifecycle_revision: Revision;
  operation_id: WorkspaceLifecycleOperationId;
  renderer_instance_id: RendererInstanceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceLifecycleOperationQuery".
 */
export interface WorkspaceLifecycleOperationQuery {
  operation_id: WorkspaceLifecycleOperationId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RendererLifecycleUnavailableInput".
 */
export interface RendererLifecycleUnavailableInput {
  renderer_instance_id: RendererInstanceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ResolveWorkspaceLifecycleInput".
 */
export interface ResolveWorkspaceLifecycleInput {
  action: WorkspaceLifecycleResolution;
  discard_confirmation_nonce?: SecretNonce | null;
  expected_lifecycle_revision: Revision;
  operation_id: WorkspaceLifecycleOperationId;
  renderer_instance_id: RendererInstanceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "LogicalFileSnapshot".
 */
export interface LogicalFileSnapshot {
  active_editor_session?: SessionId | null;
  buffer_revision?: Revision | null;
  bytes: number[];
  disk_hash?: ContentHash | null;
  /**
   * Encoding the producing layer already established for `bytes` (an editor
   * buffer's decoded encoding, or the byte order mark the disk read
   * detected). Consumers reuse it instead of re-detecting or guessing.
   * `None` means no reliable declaration is available.
   */
  encoding?: TextEncoding | null;
  is_dirty: boolean;
  logical_hash: ContentHash;
  path: string;
  source: FileStorageSource;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PathSearchCommand".
 */
export interface PathSearchCommand {
  limit: number;
  query: string;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PathSearchResult".
 */
export interface PathSearchResult {
  catalog_revision: Revision;
  matches: PathSearchMatch[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PathSearchMatch".
 */
export interface PathSearchMatch {
  generated: boolean;
  ignored: boolean;
  kind: CatalogEntryKind;
  path: string;
  score: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceArchiveCommand".
 */
export interface WorkspaceArchiveCommand {
  expected_workspace_revision: Revision;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceCreateCommand".
 */
export interface WorkspaceCreateCommand {
  display_name: string;
  selected_root: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceCreateResult".
 */
export interface WorkspaceCreateResult {
  disposition: WorkspaceCreateDisposition;
  workspace: WorkspaceSummary;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceDeleteCommand".
 */
export interface WorkspaceDeleteCommand {
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceDeleteResult".
 */
export interface WorkspaceDeleteResult {
  artifacts_queued_for_gc: number;
  ended_sessions: SessionId[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceRelocateCommand".
 */
export interface WorkspaceRelocateCommand {
  confirmation: RelocateConfirmation;
  expected_workspace_revision: Revision;
  selected_root: string;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceRestoreCommand".
 */
export interface WorkspaceRestoreCommand {
  workspace_id: WorkspaceId;
}
/**
 * The coverage block every snapshot and query result carries.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ActivityCoverage".
 */
export interface ActivityCoverage {
  /**
   * Rows dropped at publication because the index was full.
   */
  capacity_dropped: number;
  /**
   * Why rows were omitted, in a deterministic order.
   */
  omission_reasons: ActivityOmissionReason[];
  /**
   * Rows omitted from this particular rendering by a cap.
   */
  omitted_by_limit: number;
  /**
   * Facts refused as malformed.
   */
  rejected: number;
  state: ActivityCoverageState;
  /**
   * Total rows the index held for the reader's scope before any cap.
   */
  total_in_scope: number;
  /**
   * True when a body was cut to fit its byte bound.
   */
  truncated: boolean;
  /**
   * Facts dropped because the index was unavailable.
   */
  unavailable_dropped: number;
  /**
   * Rows the reader was not permitted to see. Present, not silently zero.
   */
  visibility_filtered: number;
}
/**
 * One allow-listed key parameter. The list is closed per tool group.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ActivityParameter".
 */
export interface ActivityParameter {
  key: string;
  truncated: boolean;
  value: string;
}
/**
 * The versioned wire request for the run-information query route.
 *
 * It is a Rust DTO with serde, and the TypeScript side is generated from it. The
 * request carries only what a caller really knows: which session it is asking as,
 * what it is looking for, and how much of one page it wants. It carries no authority:
 * the route resolves the caller's real workspace from its own projection, so a request
 * cannot widen its own visibility by naming one.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ActivityQueryRequestV1".
 */
export interface ActivityQueryRequestV1 {
  /**
   * Not used by the query.
   */
  expected_revision?: Revision | null;
  /**
   * Rows requested. Zero means "the default page".
   */
  limit?: number;
  /**
   * Zero-based row offset within the queried set.
   */
  offset?: number;
  /**
   * The query to answer.
   */
  query:
    | {
        by: 'session';
      }
    | {
        by: 'resource';
        target: ActivityResourceTarget;
      }
    | {
        by: 'group';
        group: ActivityToolGroup;
      }
    | {
        by: 'exact';
        id: ActivityId;
      }
    | {
        by: 'unknown_write_set';
      };
  /**
   * Physical targets the caller already named, so a row touching one of them is
   * reported even when it belongs to another session or workspace.
   *
   * These are **lookup hints, not grants**. They cannot widen what the caller may
   * see: a target is only honoured when the caller's own scope already reaches it,
   * and the source markers on every returned row stay exactly as the publishing
   * owner recorded them.
   */
  relevant_targets?: ActivityResourceTarget[];
  session_id: SessionId;
}
/**
 * The versioned wire response for the run-information query route.
 *
 * The coverage block is part of the response, not an optional extra: a caller that
 * receives zero rows must be able to tell "nothing is running" from "this index could
 * not answer", and a caller that receives some rows must be able to see what was
 * dropped, filtered or truncated.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ActivityQueryResponseV1".
 */
export interface ActivityQueryResponseV1 {
  /**
   * The versioned contract this response is written under.
   */
  contract_version: number;
  result: ActivityQueryResult;
}
/**
 * One page of a precise query, bound to the snapshot it was read from.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ActivityQueryResult".
 */
export interface ActivityQueryResult {
  /**
   * True when `offset + rows.len()` is still short of the matching total.
   */
  has_more: boolean;
  header: ActivitySnapshotHeader;
  /**
   * How many rows this page requested.
   */
  limit: number;
  /**
   * Zero-based offset of the first row in this page.
   */
  offset: number;
  /**
   * The query this page answers, echoed so a page is self-describing.
   */
  query:
    | {
        by: 'session';
      }
    | {
        by: 'resource';
        target: ActivityResourceTarget;
      }
    | {
        by: 'group';
        group: ActivityToolGroup;
      }
    | {
        by: 'exact';
        id: ActivityId;
      }
    | {
        by: 'unknown_write_set';
      };
  rows: ActivityRow[];
}
/**
 * The exact picture every reader shares for one sampling instant.
 *
 * A precise query, a page of one and the model summary are all projections of
 * *one* of these. They are never mixed, which is what makes "the page is
 * consistent" checkable rather than hopeful.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ActivitySnapshotHeader".
 */
export interface ActivitySnapshotHeader {
  coverage: ActivityCoverage1;
  /**
   * The index revision at that instant.
   */
  revision: string;
  /**
   * The runtime this snapshot was taken from.
   */
  runtime_instance_id: string;
  /**
   * The instant the index was actually sampled.
   */
  sampled_at: string;
  /**
   * True when the sampled revision is not the current one, so a caller knows
   * the world moved on after the picture it is holding.
   */
  stale: boolean;
}
/**
 * Coverage of this snapshot against the reader's scope.
 */
export interface ActivityCoverage1 {
  /**
   * Rows dropped at publication because the index was full.
   */
  capacity_dropped: number;
  /**
   * Why rows were omitted, in a deterministic order.
   */
  omission_reasons: ActivityOmissionReason[];
  /**
   * Rows omitted from this particular rendering by a cap.
   */
  omitted_by_limit: number;
  /**
   * Facts refused as malformed.
   */
  rejected: number;
  state: ActivityCoverageState;
  /**
   * Total rows the index held for the reader's scope before any cap.
   */
  total_in_scope: number;
  /**
   * True when a body was cut to fit its byte bound.
   */
  truncated: boolean;
  /**
   * Facts dropped because the index was unavailable.
   */
  unavailable_dropped: number;
  /**
   * Rows the reader was not permitted to see. Present, not silently zero.
   */
  visibility_filtered: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ActivityRow".
 */
export interface ActivityRow {
  /**
   * Present only when the publisher really has one.
   */
  agent_run_id?: AgentRunId | null;
  /**
   * The real execution reference, echoed so a reader can name the execution.
   */
  execution:
    | {
        attempt_ordinal: number;
        operation_id: SecurityOperationId;
        owner: 'agent_tool_call';
        tool_call_id: ToolCallId;
      }
    | {
        command_invocation_id?: CommandInvocationId | null;
        operation_id: SecurityOperationId;
        owner: 'main_file_mutation';
      }
    | {
        buffer_revision: Revision;
        owner: 'buffer_commit';
        reservation_id: EditorReservationId;
      }
    | {
        lifecycle_operation_id: WorkspaceLifecycleOperationId;
        owner: 'p04_publication';
        publication_id: DeferredMutationId;
      }
    | {
        correlation_id: CorrelationId;
        operation_id?: SecurityOperationId | null;
        owner: 'p04_file_publication';
      }
    | {
        attempt_ordinal: number;
        operation_id: SecurityOperationId;
        owner: 'shell_execution';
      }
    | {
        command_id: CommandId;
        invocation_id: CommandInvocationId;
        owner: 'shell_invocation';
      };
  id: ActivityId;
  kind: ActivityKind;
  parameters: ActivityParameter[];
  /**
   * Original source class of private paths and allowlisted parameters. Metadata stays separate.
   */
  private_source?: 'workspace_metadata' | 'personal_data';
  resources: ActivityResource[];
  session_id: SessionId;
  started_at: UtcTimestamp;
  tool_group: ActivityToolGroup;
  tool_id: ToolId;
  tool_name?: string;
  workspace_id: WorkspaceId;
}
/**
 * One resource relation inside a published activity.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ActivityResource".
 */
export interface ActivityResource {
  /**
   * The display name the owner already had. It is display only: two different
   * strings may name the same physical target, and the identity above decides.
   */
  display_name: string;
  /**
   * True when the display name was cut to fit the bound.
   */
  display_name_truncated: boolean;
  relation: ActivityResourceRelation;
  target: ActivityResourceTarget;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentAssistantView".
 */
export interface AgentAssistantView {
  at: UtcTimestamp;
  /**
   * The provider's backup connection this step
   * went through, because the one before it failed before answering.
   */
  backup_connection?: string | null;
  /**
   * This step's request first gave older tool results' room back, with
   * nothing summarized.
   */
  context_cleared?: AgentContextClearedView | null;
  /**
   * This
   * step's context was compacted first — older history became a summary.
   */
  context_compacted?: AgentContextCompactedView | null;
  /**
   * `compaction.state == running` from that same inspection, when recorded.
   */
  context_compaction_running?: boolean;
  context_snapshot_id?: ContextProjectionSnapshotId | null;
  /**
   * Frozen context meter copied from the prompt inspection. The ring uses this
   * and does not load the full inspector.
   */
  context_usage?: ContextUsageSummaryView | null;
  model_profile_id?: ModelProfileId | null;
  model_step_id: ModelStepId;
  partial: boolean;
  reasoning: string;
  reasoning_total_bytes: number;
  /**
   * Stored output digest. Both visible sides belong to this sealed revision;
   * the message source distinguishes assistant text from reasoning.
   */
  sealed_revision?: string | null;
  /**
   * Opaque identity of the live visible body of this step. Text and reasoning
   * come from one stream, so one identity describes both: equal values mean
   * the body only grew, a different value means a retry restarted it and any
   * bytes a client already holds belong to another body. `None` once the
   * step's output is sealed and the sealed text is authoritative.
   */
  stream_body_id?: string | null;
  stream_sequence?: number | null;
  text: string;
  text_total_bytes: number;
  tool_call_ids: ToolCallId[];
  /**
   * While the step runs, what it is writing into tool calls. Core keeps
   * it in memory only; it is empty once the step ends and is never stored.
   */
  tool_input?: AgentToolInputView[];
}
/**
 * Older tool results cleared just before a step, with nothing summarized - how
 * many, and the request's estimated input before and after (the step's own meter).
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentContextClearedView".
 */
export interface AgentContextClearedView {
  cleared_tool_results: number;
  input_tokens_after: number;
  input_tokens_before: number;
}
/**
 * A compaction just before a step: who asked for it and how much recent history stayed as it was.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentContextCompactedView".
 */
export interface AgentContextCompactedView {
  /**
   * Older tool results cleared first, in the same preparation.
   */
  cleared_tool_results?: number;
  input_tokens_after?: number | null;
  /**
   * The request's estimated input before it was made smaller, and after (the
   * step's own meter); absent for steps prepared by older versions.
   */
  input_tokens_before?: number | null;
  /**
   * The user asked for it (`/compact`); otherwise Aura did when the context neared its limit.
   */
  manual: boolean;
  retained_recent_tokens: number;
}
/**
 * A tool call a running step is writing: the tool's name as the conversation shows it, the file
 * it names once that part of its input has arrived, and how much of its input has arrived.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentToolInputView".
 */
export interface AgentToolInputView {
  complete: boolean;
  received_bytes: number;
  target?: string | null;
  title: string;
}
/**
 * How far a branch's copy is while its older turns are still being
 * carried after it opened with its newest ones.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentBranchCopyingView".
 */
export interface AgentBranchCopyingView {
  copied_turns: number;
  /**
   * The copy stopped before every turn was carried.
   */
  failed: boolean;
  total_turns: number;
}
/**
 * Where a branch came from: shown above its first own turn.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentBranchOrigin".
 */
export interface AgentBranchOrigin {
  created_at: UtcTimestamp;
  source_session_id: SessionId;
  /**
   * The conversation's title when the branch was made.
   */
  source_title: string;
  source_turn_task_id: TurnTaskId;
  /**
   * The turn's number in that conversation, counting from 1.
   */
  turn_number: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentCompactionRequest".
 */
export interface AgentCompactionRequest {
  agent_run_id: AgentRunId;
  focus?: string | null;
  session_id: SessionId;
}
/**
 * `/compact` in an idle session. `focus` is the user's optional note after the command:
 * what the summary should keep. Core compacts before the turn's first step.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentCompactionStartRequest".
 */
export interface AgentCompactionStartRequest {
  focus?: string | null;
  model_profile_id: ModelProfileId;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentCompactionStatus".
 */
export interface AgentCompactionStatus {
  agent_run_id: AgentRunId;
  request?: ManualCompactionRequestRecord | null;
  session_id: SessionId;
}
/**
 * A history poll's view of one completion report: where it lives, how long it is,
 * and a short prefix. It is not the model's final answer.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentCompletionSummary".
 */
export interface AgentCompletionSummary {
  /**
   * Blake3 of the report bytes, recorded when the turn completed. Older rows omit it.
   */
  digest?: string | null;
  preview: string;
  total_bytes: number;
}
/**
 * Session/run authority is supplied by the Agent's admitted invocation.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentContextReadCompactionInput".
 */
export interface AgentContextReadCompactionInput {
  sequence: number;
  source_context_snapshot_id: ContextProjectionSnapshotId;
}
/**
 * Core injects session/run; a model may select only already readable events.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentContextReadEventsInput".
 */
export interface AgentContextReadEventsInput {
  /**
   * Characters per page. Default 8192.
   */
  character_limit?: number;
  cursor?: string | null;
  selectors: ContextEventSelector[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentContextReadToolResultInput".
 */
export interface AgentContextReadToolResultInput {
  cursor?: string | null;
  /**
   * Also return the call's input. Default false.
   */
  include_input?: boolean;
  tool_call_id: ToolCallId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentContextRecheckRequest".
 */
export interface AgentContextRecheckRequest {
  agent_run_id: AgentRunId;
  expected_revision: Revision;
  session_id: SessionId;
  source_model_step_id: ModelStepId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentContextRemediation".
 */
export interface AgentContextRemediation {
  budget: ContextBudgetSnapshot;
  created_at: UtcTimestamp;
  model_step_id: ModelStepId;
  plan: ContextRemediationPlan;
  revision: Revision;
  state: AgentContextRemediationState;
  updated_at: UtcTimestamp;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentContextRemediationView".
 */
export interface AgentContextRemediationView {
  agent_run_id: AgentRunId;
  provider_correction?: AgentContextWindowCorrection | null;
  remediation?: AgentContextRemediation | null;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentContextWindowCorrection".
 */
export interface AgentContextWindowCorrection {
  correction: ContextWindowCorrectionView;
  created_at: UtcTimestamp;
  revision: Revision;
  state: AgentContextWindowState;
  updated_at: UtcTimestamp;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentContextWindowResolveRequest".
 */
export interface AgentContextWindowResolveRequest {
  action: AgentContextWindowAction;
  agent_run_id: AgentRunId;
  expected_revision: Revision;
  session_id: SessionId;
  source_model_step_id: ModelStepId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentFileDeleteInput".
 */
export interface AgentFileDeleteInput {
  /**
   * The file or folder to delete; a folder goes with everything in it.
   */
  path: string;
  /**
   * Default false: the Recycle Bin. True deletes for good and asks the user every time; use it
   * only when the user asked for that, or the Recycle Bin cannot take the item.
   */
  permanent?: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentFileDeleteResult".
 */
export interface AgentFileDeleteResult {
  outcome: AgentFileDeleteOutcome;
  /**
   * Where it was, as Core found it.
   */
  path: string;
  was_folder: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentFileListCursor".
 */
export interface AgentFileListCursor {
  after_path: AgentWorkspacePath;
  /**
   * The version of what was listed: the workspace index's directory revision, or
   * the digest of an authorized external directory's live contents. A change to
   * that directory invalidates the cursor instead of silently skipping entries.
   */
  catalog_revision: string;
  directory: AgentWorkspacePath;
  /**
   * Present when this page listed an authorized external directory: the grant it
   * was listed through. A cursor from one grant never continues another.
   */
  external_grant_id?: ExternalGrantId | null;
}
/**
 * Lists immediate children, including empty directories. Use `.` for the root.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentFileListInput".
 */
export interface AgentFileListInput {
  cursor?: OpaquePageCursor | null;
  /**
   * Entries per page, 1..=100 (default 100).
   */
  limit?: number;
  path: AgentWorkspacePath;
}
/**
 * Pass next_cursor from the previous page unchanged to continue. Leave it out to start.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "OpaquePageCursor".
 */
export interface OpaquePageCursor {}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentFileListResult".
 */
export interface AgentFileListResult {
  catalog_revision: Revision;
  entries: FsListEntry[];
  /**
   * Present when this listing used an authorized external directory.
   */
  external?: FsExternalListTarget | null;
  next_cursor?: AgentFileListCursor | null;
  skipped_scopes?: FsSkippedScope[];
  truncated: boolean;
}
/**
 * The authorized external directory one listing used.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "FsExternalListTarget".
 */
export interface FsExternalListTarget {
  /**
   * The absolute directory path as the user sees it. Each entry's `path` is
   * relative to this grant's root.
   */
  display_path: string;
  grant_id?: ExternalGrantId | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentFilePatchInput".
 */
export interface AgentFilePatchInput {
  expected_hash: ContentHash;
  path: AgentWorkspacePath;
  unified_diff: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentFileReadInput".
 */
export interface AgentFileReadInput {
  /**
   * Leave out to detect the encoding.
   */
  explicit_encoding?: TextEncoding | null;
  path: AgentWorkspacePath;
  /**
   * Which part to read. Default: the whole file.
   */
  range?:
    | {
        range_kind: 'full';
      }
    | {
        count: number;
        range_kind: 'lines';
        start_line_index: number;
      }
    | {
        count: number;
        range_kind: 'characters';
        start_character_offset: number;
      };
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentFileSearchInput".
 */
export interface AgentFileSearchInput {
  /**
   * Default false.
   */
  include_generated?: boolean;
  /**
   * Default false.
   */
  include_ignored?: boolean;
  /**
   * Maximum matches returned, 1..=100 (default 100). This searches text, not filenames.
   */
  limit?: number;
  path_glob?: string | null;
  pattern: FsSearchPattern;
  /**
   * A path relative to the workspace root ("." is the root). An absolute path inside the workspace names the same file.
   */
  root?: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentFileWriteInput".
 */
export interface AgentFileWriteInput {
  /**
   * Use {expected_kind:"absent"} for a new file. For an existing file, use the
   * latest logical_hash object ({algorithm,hex}) or {expected_kind:"file_hash",hash}.
   */
  expected: FsExpectedState | ContentHash;
  path: AgentWorkspacePath;
  text: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentHistoryRequest".
 */
export interface AgentHistoryRequest {
  before_turn_task_id?: TurnTaskId | null;
  limit: number;
  session_id: SessionId;
  within_turn?: AgentTurnHistoryCursor | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentTurnHistoryCursor".
 */
export interface AgentTurnHistoryCursor {
  before_model_step_id?: ModelStepId | null;
  before_user_event_id?: EventId | null;
  turn_task_id: TurnTaskId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentInboxMutation".
 */
export interface AgentInboxMutation {
  action: AgentInboxAction;
  expected_revision: Revision;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentInboxProjection".
 */
export interface AgentInboxProjection {
  messages: AgentPendingMessage[];
  pause_reason?: string | null;
  paused: boolean;
  revision: Revision;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentPendingMessage".
 */
export interface AgentPendingMessage {
  /**
   * The pictures and files added to this message, kept with it.
   */
  attachments?: AgentMessageAttachment[];
  created_at: UtcTimestamp;
  destination: AgentInputLane;
  message_id: EventId;
  /**
   * The thinking level chosen with this message for the turn it starts.
   */
  reasoning_effort?: TurnReasoningEffort | null;
  revision?: string;
  /**
   * The Skills chosen with this message, as checked when it was sent.
   */
  skills?: LoadedSkillEntry[];
  text: string;
}
/**
 * A Skill loaded for this turn, at the content it was loaded with.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "LoadedSkillEntry".
 */
export interface LoadedSkillEntry {
  /**
   * The content fingerprint the user trusted and this load used.
   */
  fingerprint: string;
  name: string;
  origin: SkillOrigin;
  skill_id: SkillInstanceId;
}
/**
 * `/init`: start a workspace initialization task in an idle session. `text` is the
 * user's optional note after the command; the task uses the session's permission mode.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentInitStartRequest".
 */
export interface AgentInitStartRequest {
  model_profile_id: ModelProfileId;
  session_id: SessionId;
  text?: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentInspectorRequest".
 */
export interface AgentInspectorRequest {
  model_step_id: ModelStepId;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentMessageTextPage".
 */
export interface AgentMessageTextPage {
  /**
   * The body this page was read from, `None` for sealed text. A client that
   * asked from a byte of another body must discard the page instead of
   * appending it.
   */
  body_id?: string | null;
  /**
   * The byte the returned text really starts at. It is the requested offset
   * for sealed text, and the nearest character boundary at or before it for a
   * live body that moved while the request was in flight.
   */
  from_offset: number;
  next_offset?: number | null;
  /**
   * Same immutable revision as the preview. Missing on live and legacy data
   * without a recorded digest; clients must not cache those as sealed text.
   */
  sealed_revision?: string | null;
  source: AgentMessageTextSource;
  text: string;
  total_bytes: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentMessageTextRequest".
 */
export interface AgentMessageTextRequest {
  limit: number;
  offset: number;
  session_id: SessionId;
  source: AgentMessageTextSource;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GoalBudget".
 */
export interface GoalBudget {
  max_tokens?: number | null;
  max_turns?: number | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentModeControlRequest".
 */
export interface AgentModeControlRequest {
  action: AgentModeControlAction;
  expected_revision: Revision;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentModeEpisodeView".
 */
export interface AgentModeEpisodeView {
  disposition_required?: boolean;
  end_reason?: ModeEpisodeEndReason | null;
  goal_budget?: GoalBudget | null;
  inherited_goal?: GoalState | null;
  mode: AgentTaskMode;
  paused: boolean;
  revision: Revision;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GoalState".
 */
export interface GoalState {
  budget: GoalBudget;
  completed: boolean;
  consumed_tokens: number;
  consumed_turns: number;
  latest_check_model_step?: ModelStepId | null;
  revision: Revision;
  turn_task_id: TurnTaskId;
}
/**
 * What the settings form shows for the proxy field. It is not a validated proxy.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentNetworkProxyDraftV1".
 */
export interface AgentNetworkProxyDraftV1 {
  enabled: boolean;
  host: string;
  port: number;
  protocol: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentNetworkProxyRecoveryPreviewV1".
 */
export interface AgentNetworkProxyRecoveryPreviewV1 {
  expected_revision: Revision;
  /**
   * Null restores the absence of a proxy; no other setting is restored.
   */
  proxy?: ToolNetworkProxyV1 | null;
  source_revision: Revision;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentNetworkProxyTestRequest".
 */
export interface AgentNetworkProxyTestRequest {
  host: string;
  port: number;
  protocol: ToolNetworkProxyProtocolV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentNetworkProxyTestResult".
 */
export interface AgentNetworkProxyTestResult {
  message: string;
  ok: boolean;
}
/**
 * A historical inspector is bound to the exact execution and context that
 * produced that step, independent of the session's current configuration.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentPromptInspection".
 */
export interface AgentPromptInspection {
  context: ContextProjectionSnapshot;
  execution: ModelExecutionSnapshot;
  projection: PromptInspectorProjection;
  /**
   * The whole conversation's cache use when the step was read.
   */
  session_cache?: SessionCacheUsageView | null;
}
/**
 * What the conversation's
 * finished requests sent, and what their Providers said they read from the cache.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SessionCacheUsageView".
 */
export interface SessionCacheUsageView {
  cached_input_tokens: number;
  /**
   * The input of the reported requests, so the share is of what was reported.
   */
  input_tokens: number;
  /**
   * Those whose Provider reported its cache use.
   */
  reported_steps: number;
  /**
   * Finished requests of the conversation.
   */
  steps: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentQueryAnswerRequest".
 */
export interface AgentQueryAnswerRequest {
  agent_run_id: AgentRunId;
  answers: QueryAnswer[];
  expected_revision: Revision;
  query_id: UserQueryId;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "QueryAnswer".
 */
export interface QueryAnswer {
  question_id: string;
  value: QueryAnswerValue;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentQueryFileRequest".
 */
export interface AgentQueryFileRequest {
  agent_run_id: AgentRunId;
  expected_revision: Revision;
  query_id: UserQueryId;
  question_id: string;
  relative_path: string;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentQueryFileSelection".
 */
export interface AgentQueryFileSelection {
  file_reference_id: FileReferenceId;
  relative_path: string;
}
/**
 * The user touched the waiting question (clicked it, chose, typed), so
 * its countdown starts again.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentQueryTouchRequest".
 */
export interface AgentQueryTouchRequest {
  agent_run_id: AgentRunId;
  query_id: UserQueryId;
  session_id: SessionId;
}
/**
 * When the touched question now ends as "no response".
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentQueryTouchView".
 */
export interface AgentQueryTouchView {
  deadline_at: UtcTimestamp;
  query_id: UserQueryId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentRelatedTurnRequest".
 */
export interface AgentRelatedTurnRequest {
  expected_revision: Revision;
  /**
   * Explicit user adjustment of the recovered Goal's total limit. Omission
   * carries the original limit; cumulative usage is never reset.
   */
  goal_budget?: GoalBudget | null;
  kind: AgentRelatedTurnKind;
  session_id: SessionId;
  source_turn_task_id: TurnTaskId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentResultRequest".
 */
export interface AgentResultRequest {
  limit: number;
  offset: number;
  session_id: SessionId;
  tool_call_id: ToolCallId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentResultView".
 */
export interface AgentResultView {
  artifact_id: ArtifactId;
  next_offset?: number | null;
  text: string;
  tool_call_id: ToolCallId;
  total_bytes: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentRun".
 */
export interface AgentRun {
  agent_run_id: AgentRunId;
  created_at: UtcTimestamp;
  next_model_ordinal: number;
  parent_agent_run_id?: AgentRunId | null;
  prompt_bundle_snapshot_id: PromptBundleSnapshotId;
  revision: Revision;
  state: AgentRunState;
  turn_task_id: TurnTaskId;
  updated_at: UtcTimestamp;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentRunProjection".
 */
export interface AgentRunProjection {
  cancellation_requested: boolean;
  model_steps: ModelStep[];
  pending_approval_ids: ApprovalRequestId[];
  pending_query?: UserQuery | null;
  /**
   * When the waiting question ends as "no response" unless the user touches it.
   */
  pending_query_deadline_at?: UtcTimestamp | null;
  recovery_episodes: RecoveryEpisode[];
  run: AgentRun;
  turn: TurnTask;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelStep".
 */
export interface ModelStep {
  agent_run_id: AgentRunId;
  continuation: AgentContinuation;
  created_at: UtcTimestamp;
  finished_at?: UtcTimestamp | null;
  model_step_id: ModelStepId;
  ordinal: number;
  state: ModelStepState;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "UserQuery".
 */
export interface UserQuery {
  agent_run_id: AgentRunId;
  created_at: UtcTimestamp;
  input: QueryUserQuestionInput;
  query_id: UserQueryId;
  revision: Revision;
  state: UserQueryState;
  tool_call_id: ToolCallId;
  turn_task_id: TurnTaskId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "QueryUserQuestionInput".
 */
export interface QueryUserQuestionInput {
  questions: QueryQuestion[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "QueryQuestion".
 */
export interface QueryQuestion {
  answer_type: QueryAnswerType;
  id: string;
  text: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "QueryChoice".
 */
export interface QueryChoice {
  id: string;
  label: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RecoveryEpisode".
 */
export interface RecoveryEpisode {
  agent_run_id: AgentRunId;
  created_at: UtcTimestamp;
  evidence_refs: ArtifactId[];
  not_executed_tool_call_ids: ToolCallId[];
  ordinal: number;
  reason: string;
  resolved_at?: UtcTimestamp | null;
  unresolved_operation_ids: SecurityOperationId[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TurnTask".
 */
export interface TurnTask {
  agent_run_id: AgentRunId;
  created_at: UtcTimestamp;
  lineage_id: TaskLineageId;
  lineage_resolution_required: boolean;
  mode: AgentTaskMode;
  revision: Revision;
  root_user_event_id: EventId;
  session_id: SessionId;
  state: TurnTaskState;
  turn_task_id: TurnTaskId;
  updated_at: UtcTimestamp;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentRunRequest".
 */
export interface AgentRunRequest {
  agent_run_id: AgentRunId;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentSessionActivity".
 */
export interface AgentSessionActivity {
  session_id: SessionId;
  state: AgentSessionActivityState;
}
/**
 * Branch this conversation at one of its finished turns. An authenticated user command; a model
 * tool cannot call it.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentSessionBranchRequest".
 */
export interface AgentSessionBranchRequest {
  session_id: SessionId;
  /**
   * The finished turn the branch continues from; it and every turn before it are carried.
   */
  turn_task_id: string;
}
/**
 * The new conversation.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentSessionBranchResult".
 */
export interface AgentSessionBranchResult {
  origin: AgentBranchOrigin;
  session_id: SessionId;
  title: string;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentSessionProjection".
 */
export interface AgentSessionProjection {
  active?: AgentRunProjection | null;
  /**
   * Set while a branch's older turns are still being carried.
   */
  branch_copying?: AgentBranchCopyingView | null;
  /**
   * Set on a branch: the conversation and turn it was made from.
   */
  branch_origin?: AgentBranchOrigin | null;
  inbox: AgentInboxProjection;
  mode_episode?: AgentModeEpisodeView | null;
  next_before_turn_task_id?: TurnTaskId | null;
  revision: Revision;
  runtime_instance_id: RuntimeInstanceId;
  session_id: SessionId;
  turns: AgentTurnView[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentTurnView".
 */
export interface AgentTurnView {
  assistant_messages: AgentAssistantView[];
  /**
   * Bounded facts of a completion report. The authoritative report stays in the
   * turn record; this is only what a history poll needs before the body is paged.
   */
  completion?: AgentCompletionSummary | null;
  diagnostic?: string | null;
  /**
   * Where this turn's final answer comes from. Absent on historical turns
   * whose completion source predates this field; they render as a report.
   */
  final_text_source?: AgentFinalTextSource | null;
  goal?: GoalState | null;
  /**
   * A turn this branch carried from the conversation it was made from. It is shown and
   * can be copied or branched from again; it cannot be continued, retried or implemented here.
   */
  imported?: boolean;
  next_messages?: AgentTurnHistoryCursor | null;
  plan?: PlanRecord | null;
  source?: AgentRelatedTurnRequest | null;
  todo: TodoState;
  turn: TurnTask;
  user_messages: AgentUserMessageView[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PlanRecord".
 */
export interface PlanRecord {
  at: UtcTimestamp;
  /**
   * Loaded verbatim from User Events by Core, never accepted in Tool input.
   */
  original_user_goal: string;
  plan: PlanCompleteInput;
  revision: Revision;
  turn_task_id: TurnTaskId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PlanCompleteInput".
 */
export interface PlanCompleteInput {
  current_project_state?: string;
  expected_turn_revision: Revision;
  requirements?: string[];
  steps?: PlanStep[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PlanStep".
 */
export interface PlanStep {
  /**
   * Default empty.
   */
  depends_on?: string[];
  description: string;
  /**
   * Default serial.
   */
  execution?:
    | {
        execution: 'serial';
      }
    | {
        execution: 'parallel_group';
        group: string;
      };
  id: string;
  validation?: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TodoState".
 */
export interface TodoState {
  items: TodoItem[];
  revision: Revision;
  turn_task_id: TurnTaskId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TodoItem".
 */
export interface TodoItem {
  id: TodoItemId;
  status: TodoStatus;
  title: string;
}
/**
 * A bounded view, not an OriginalUserMessage whose content hash would imply
 * the complete text. The immutable source remains available via message.text.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentUserMessageView".
 */
export interface AgentUserMessageView {
  at: UtcTimestamp;
  /**
   * The pictures and files added to this message. `text` and `total_bytes` are then
   * what the user typed; the model read them with the message ([`message_with_attachments`]).
   */
  attachments?: AgentMessageAttachment[];
  event_id: EventId;
  origin: UserMessageOrigin;
  /**
   * Identity of the complete immutable text, not of the bounded preview.
   */
  sealed_revision?: string | null;
  /**
   * The Skills the user chose with this message.
   */
  skills?: LoadedSkillEntry[];
  text: string;
  total_bytes: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentSessionRequest".
 */
export interface AgentSessionRequest {
  session_id: SessionId;
}
/**
 * Only the conversations with a turn under way; every other one is idle.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentSessionsActivity".
 */
export interface AgentSessionsActivity {
  sessions: AgentSessionActivity[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentSessionsActivityRequest".
 */
export interface AgentSessionsActivityRequest {}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentStartRequest".
 */
export interface AgentStartRequest {
  /**
   * Pictures and files added to the message in the composer. The run's configuration
   * keeps their digests ([`AgentStartRequest::receipt`]); the message keeps them.
   */
  attachments?: AgentAttachmentDraft[];
  /**
   * User preview of NativeApproved. This is never an approval or a silent
   * fallback; policy still Asks AllowOnce/AllowSession before dispatch.
   */
  explicit_backend_id?: string | null;
  goal_budget?: GoalBudget | null;
  mode: AgentTaskMode;
  model_profile_id: ModelProfileId;
  /**
   * The thinking level the conversation uses for this turn; none is the model's
   * default. Kept with the run's configuration, so every step of the turn uses it.
   */
  reasoning_effort?: TurnReasoningEffort | null;
  session_id: SessionId;
  /**
   * Skills the user chose for this turn in the composer.
   */
  skill_ids?: SkillInstanceId[];
  text: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentSubmitRequest".
 */
export interface AgentSubmitRequest {
  /**
   * Pictures and files added to the message in the composer.
   */
  attachments?: AgentAttachmentDraft[];
  destination: AgentInputLane;
  /**
   * The thinking level the conversation uses with its model, for the turn this message
   * starts; none is the model's default.
   */
  reasoning_effort?: TurnReasoningEffort | null;
  session_id: SessionId;
  /**
   * Skills the user chose for the turn this message starts. Only a message that starts
   * a new turn may carry them.
   */
  skill_ids?: SkillInstanceId[];
  text: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentTerminalCloseInput".
 */
export interface AgentTerminalCloseInput {
  terminal_id: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentTerminalOpenInput".
 */
export interface AgentTerminalOpenInput {
  requires_network?: boolean;
  shell?: 'powershell' | 'cmd';
  title?: string;
  working_directory?: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentTerminalReadInput".
 */
export interface AgentTerminalReadInput {
  max_bytes?: number;
  offset?: number;
  terminal_id: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentTerminalResultV1".
 */
export interface AgentTerminalResultV1 {
  end?: TerminalEnd | null;
  next_offset: number;
  output: string;
  running: boolean;
  start_offset: number;
  terminal_id: string;
  truncated: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TerminalEnd".
 */
export interface TerminalEnd {
  error?: string | null;
  exit_code?: number | null;
  reason: TerminalEndReason;
  stop_confirmed: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentTerminalSensitiveInput".
 */
export interface AgentTerminalSensitiveInput {
  prompt: string;
  terminal_id: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentTerminalWriteInput".
 */
export interface AgentTerminalWriteInput {
  input?: string;
  interrupt?: boolean;
  terminal_id: string;
  until_text?: string | null;
  wait_ms?: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentToolActivityEntry".
 */
export interface AgentToolActivityEntry {
  display: AgentToolDisplay;
  model_step_id: ModelStepId;
  permission_mode?: PermissionMode | null;
  permission_revision?: number | null;
  shell_query_available: boolean;
  summary: ToolCallActivitySummaryView;
}
/**
 * Bounded, deterministic display derived from execution facts, never model prose.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentToolDisplay".
 */
export interface AgentToolDisplay {
  details: string[];
  /**
   * The screenshot the call took, read with `browser.screenshot.read`.
   */
  image?: ArtifactId | null;
  internal: boolean;
  /**
   * What the user can do from the row about why the call did not run.
   */
  offer?: AgentToolOffer | null;
  summary: string;
  target?: string | null;
  title: string;
  truncated: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentToolActivityPage".
 */
export interface AgentToolActivityPage {
  calls: AgentToolActivityEntry[];
  next_before_call_id?: ToolCallId | null;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentToolActivityRequest".
 */
export interface AgentToolActivityRequest {
  before_call_id?: ToolCallId | null;
  /**
   * Exact visible identities, at most 100. Mutually exclusive with a cursor.
   */
  call_ids?: ToolCallId[] | null;
  limit: number;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentToolCatalogEntry".
 */
export interface AgentToolCatalogEntry {
  available: boolean;
  default_loaded: boolean;
  description: string;
  name: string;
  required: boolean;
  source: string;
  tool_id: ToolId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentToolSettingsRequest".
 */
export interface AgentToolSettingsRequest {}
/**
 * One revision-checked edit of every user-owned execution-strategy value.
 * Rust owns the ranges; the renderer never widens them by editing a generated
 * artifact. `fixed_tool_ids` is carried along so a strategy save cannot silently
 * drop a concurrently updated tool list.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentToolSettingsUpdate".
 */
export interface AgentToolSettingsUpdate {
  default_timeout_ms: number;
  expected_revision: Revision;
  fixed_tool_ids: ToolId[];
  max_concurrent_tool_calls: number;
  max_dynamic_tool_schemas: number;
  max_retries: number;
  /**
   * Absent leaves the stored proxy untouched. JSON `null` clears it. An object replaces it.
   */
  network_proxy?: ToolNetworkProxyV1 | null;
  /**
   * Restore the recommended default tool list instead of `fixed_tool_ids`.
   *
   * A user who once saved their own list keeps it across upgrades, so the
   * recommended list only reaches an untouched configuration. This is the
   * explicit user action that opts back in; it is never applied silently.
   */
  use_recommended_fixed_tool_ids?: boolean;
  /**
   * Absent leaves the stored web service untouched.
   */
  web_service?: WebServiceSettingV1 | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AgentToolSettingsView".
 */
export interface AgentToolSettingsView {
  default_timeout_ms: number;
  fixed_tool_ids: ToolId[];
  max_concurrent_tool_calls: number;
  max_dynamic_tool_schemas: number;
  max_retries: number;
  /**
   * The proxy that tools will actually use. Empty when disabled or the stored field is unusable.
   */
  network_proxy?: ToolNetworkProxyV1 | null;
  /**
   * Why the stored field is not in effect. Other Tools settings stay readable and writable.
   */
  network_proxy_diagnostic?: string | null;
  /**
   * The stored field, including a value that failed validation, so the form can keep it.
   */
  network_proxy_draft?: AgentNetworkProxyDraftV1 | null;
  /**
   * A field-only recovery preview. Applying it must use this preview's Tools revision.
   */
  network_proxy_recovery?: AgentNetworkProxyRecoveryPreviewV1 | null;
  revision: Revision;
  tools: AgentToolCatalogEntry[];
  /**
   * The service `web.search` and `web.fetch` use.
   */
  web_service?:
    | {
        kind: 'automatic';
      }
    | {
        kind: 'local';
      }
    | {
        endpoint_id: EndpointId;
        kind: 'deepseek';
      };
  /**
   * The DeepSeek connections with a key it can use.
   */
  web_service_connections?: WebServiceConnectionV1[];
}
/**
 * A model connection the web service can use: DeepSeek's own API, with a key.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WebServiceConnectionV1".
 */
export interface WebServiceConnectionV1 {
  display_name: string;
  endpoint_id: EndpointId;
}
/**
 * One entry of an
 * attached zip outside the workspace, by its path in the zip's listing. Core copies it into Aura's
 * own folder for the conversation and says where, for Main only. Or of a zip in the session's workspace, by its path there — one of the two.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AttachmentEntryRequest".
 */
export interface AttachmentEntryRequest {
  entry: string;
  external_grant_id?: ExternalGrantId | null;
  session_id: SessionId;
  workspace_path?: string | null;
}
/**
 * An attached file outside the workspace, by the authorization its message gave this
 * conversation. Main asks; the renderer never learns more than the message already shows.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AttachmentLocateRequest".
 */
export interface AttachmentLocateRequest {
  external_grant_id: ExternalGrantId;
  session_id: SessionId;
}
/**
 * Where the file is now, after Core opened it and found it is still the file that was attached.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AttachmentLocation".
 */
export interface AttachmentLocation {
  byte_length: number;
  path: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AttachmentText".
 */
export interface AttachmentText {
  text: string;
}
/**
 * A text file or a zip's listing one of the conversation's messages carries.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AttachmentTextRequest".
 */
export interface AttachmentTextRequest {
  artifact_id: ArtifactId;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AttemptRef".
 */
export interface AttemptRef {
  operation_id: SecurityOperationId;
  ordinal: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExecutionResult".
 */
export interface ExecutionResult {
  completed_at: UtcTimestamp;
  outcome: ExecutionOutcome;
  receipt: ExecutionReceipt;
  side_effect: SideEffectStateV1;
  termination: TerminationEvidence;
  warnings: string[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExecutionReceipt".
 */
export interface ExecutionReceipt {
  attempt: AttemptRef;
  evidence_ref: ArtifactId;
  postcondition_fingerprint?: CanonicalSha256HashV1 | null;
  rollback: RollbackEvidence;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AuthorizationApproval".
 */
export interface AuthorizationApproval {
  allowed_decisions: ApprovalDecisionKind[];
  approval_id: ApprovalRequestId;
  expires_at: UtcTimestamp;
  requested_at: UtcTimestamp;
  revision: Revision;
  scope: AuthorizationScope;
  state: ApprovalState;
}
/**
 * The content fingerprint binds Source, Capability, Resource, Risk and exact
 * credential generations. Backend requirements bound permitted rebinding.
 *
 * `consent` is absent for every historical and exact-operation authorization.
 * It is present only on a reusable Native session consent, which is the single
 * documented exception to "a Grant authorizes exactly the operation it was
 * approved for".
 *
 * `file_consent` is the same exception for file targets, and it is a **separate field on
 * purpose**. The two consents answer different questions — "this conversation may keep running
 * ordinary Shell commands" versus "this conversation may keep using this file capability on
 * this target" — and they carry different facts (a set of Shells versus a set of capabilities
 * and named targets). One shared field would have to be read as whichever shape happened to be
 * present, which is exactly the ambiguity that would make a reuse check unsound.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AuthorizationScope".
 */
export interface AuthorizationScope {
  backend_requirement: BackendRequirement;
  /**
   * The missing capabilities actually shown in a composite approval. Historical exact
   * approvals omit this field; their exact operation fingerprint remains authoritative.
   */
  capability_limit?: ResolvedCapability[] | null;
  consent?: NativeSessionConsentRef | null;
  /**
   * The reusable file consent, present only when an ordinary file approval recorded one.
   */
  file_consent?: FileSessionConsentRef | null;
  lifetime: AuthorizationLifetime;
  /**
   * A ReadOnly answer covering one MCP server for the rest of the turn.
   */
  mcp_consent?: McpTurnConsentRef | null;
  memory_consent?: MemorySessionConsentRef | null;
  network_consent?: NetworkSessionConsentRef | null;
  operation_fingerprint: CanonicalSha256HashV1;
  profile: SecurityProfile;
}
/**
 * Limits are explicit. An empty allowed class set grants no dispatch route.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BackendRequirement".
 */
export interface BackendRequirement {
  allowed_classes: BackendClass[];
  guarantees: EnforcementGuarantee[];
}
/**
 * The immutable identity of the approval that first produced a reusable Native
 * session consent.
 *
 * A native session consent is the one authorization that deliberately outlives
 * the exact operation it was approved for, so the scope carrying it has to say
 * which approval it came from. `operation_fingerprint` then describes the
 * operation a *derived* decision is issued for, and is allowed to differ from
 * [`Self::approved_operation_fingerprint`] only because this reference exists.
 * Nothing else may compare a stored grant's scope with a decision's scope
 * across two different operations.
 *
 * Two revisions are named because the Grant's revision moves when it is revoked
 * and does not move for any other reason. `grant_revision` is the revision the
 * consent was issued at, and `revoked_grant_revision` is the one a withdrawal
 * produced. That is what lets a reader tell "still live", "withdrawn" and "this
 * row does not describe its Grant" apart from the stored facts alone, instead of
 * treating every revision change as corruption and reporting a successful
 * withdrawal as a failure.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NativeSessionConsentRef".
 */
export interface NativeSessionConsentRef {
  /**
   * The approval fact this consent was resolved from. A second approval is a
   * second consent, never a resurrection of this one.
   */
  approval_id: string;
  approved: NativeSessionConsentScope;
  approved_operation_fingerprint: CanonicalSha256HashV13;
  /**
   * Issuing Core for audit. Only LegacyRuntime uses this as an expiry condition.
   */
  core_runtime_instance_id: string;
  /**
   * The consent Grant. Its revisions are checked on every use, so revocation and
   * consumption are visible without a second lookup.
   */
  grant_id: string;
  /**
   * The revision the consent was issued at.
   */
  grant_revision: string;
  lifetime?: 'legacy_runtime' | 'session';
  /**
   * The revision a withdrawal produced, recorded on the Grant itself when the
   * user withdraws. Absent while the consent is live.
   */
  revoked_grant_revision?: Revision | null;
  /**
   * Approved revision for audit. Only LegacyRuntime expires on a later revision.
   */
  session_permission_revision: number;
}
/**
 * The actor the user saw, the Shell they confirmed, and the range that answer
 * covers.
 */
export interface NativeSessionConsentScope {
  /**
   * The Shell the user actually confirmed. Audit only.
   */
  approved_shell: 'cmd' | 'powershell';
  principal_id: PrincipalId;
  /**
   * The trusted built-in Shells this consent authorizes.
   */
  shells: NativeShell[];
}
/**
 * The operation the user actually confirmed. Kept for audit and for the
 * management surface; never used as an equality condition on new work.
 */
export interface CanonicalSha256HashV13 {
  algorithm: 'sha256';
  canonicalization_version: 'rfc8785_jcs_v1';
  hex: string;
}
/**
 * A ReadOnly answer for one server: the server's later calls in the same turn, made with the
 * configuration the user trusted, ask no more.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "McpTurnConsentRef".
 */
export interface McpTurnConsentRef {
  approval_id: ApprovalRequestId;
  approved_operation_fingerprint: CanonicalSha256HashV1;
  config_fingerprint: ContentHash;
  grant_revision: Revision;
  principal_id: PrincipalId;
  server_id: string;
  session_id: SessionId;
  turn_task_id: TurnTaskId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemorySessionConsentRef".
 */
export interface MemorySessionConsentRef {
  approval_id: ApprovalRequestId;
  approved_operation_fingerprint: CanonicalSha256HashV1;
  grant_revision: Revision;
  principal_id: PrincipalId;
  revoked_grant_revision?: Revision | null;
  scope: MemoryVaultScope;
  session_id: SessionId;
  vault_identity: ContentHash;
}
/**
 * A user's session answer. It has no Core-instance or permission-revision lifetime binding.
 * Unknown semantic versions fail deserialization; historical grants have no such reference.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NetworkSessionConsentRef".
 */
export interface NetworkSessionConsentRef {
  approval_id: ApprovalRequestId;
  approved_operation_fingerprint: CanonicalSha256HashV1;
  caller: NetworkConsentCaller;
  destinations: NetworkConsentDestinations;
  grant_id: PermissionGrantId;
  grant_revision: Revision;
  principal_id: PrincipalId;
  revoked_grant_revision?: Revision | null;
  session_id: SessionId;
  version: ToolWireVersionV1;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GitRemoteTargetV1".
 */
export interface GitRemoteTargetV1 {
  endpoint_fingerprint: CanonicalSha256HashV14;
  remote_branch: string;
  remote_url: string;
  repository: CanonicalSha256HashV15;
}
/**
 * Binds non-display URL components too; raw query data is never persisted.
 */
export interface CanonicalSha256HashV14 {
  algorithm: 'sha256';
  canonicalization_version: 'rfc8785_jcs_v1';
  hex: string;
}
/**
 * Hash of the retained common-dir's volume and file identity, not its spelling.
 */
export interface CanonicalSha256HashV15 {
  algorithm: 'sha256';
  canonicalization_version: 'rfc8785_jcs_v1';
  hex: string;
}
/**
 * Each slice binds exactly one capability and the immutable operation's caller and targets.
 * It is dispatch evidence for this operation, never a new reusable grant.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CapabilityAuthorization".
 */
export interface CapabilityAuthorization {
  authority: CapabilityAuthority;
  caller: NetworkCallerKind;
  capability: ResolvedCapability;
  identity: InvocationIdentity;
  resources: ResourceDescriptor[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GitRemoteFactsV1".
 */
export interface GitRemoteFactsV1 {
  force_with_lease?: string | null;
  local_branch?: string | null;
  local_head?: string | null;
  /**
   * Based only on the last fetched tracking ref. None means no such ref exists.
   */
  outgoing_count?: number | null;
  outgoing_subjects: string[];
  remote: string;
  target: GitRemoteTargetV1;
}
/**
 * Core-issued decision facts. Deserialization does not authenticate a decision;
 * applications must revalidate the authority and cancellation revision.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AuthorizationDecision".
 */
export interface AuthorizationDecision {
  basis: AuthorizationBasis;
  decided_at: UtcTimestamp;
  expires_at: UtcTimestamp;
  scope: AuthorizationScope;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AuthorizationGrant".
 */
export interface AuthorizationGrant {
  consumed_at?: UtcTimestamp | null;
  expires_at?: UtcTimestamp | null;
  grant_id: PermissionGrantId;
  issued_at: UtcTimestamp;
  revision: Revision;
  revoked_at?: UtcTimestamp | null;
  scope: AuthorizationScope;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "AuthorizationProposal".
 */
export interface AuthorizationProposal {
  allowed_decisions: ApprovalDecisionKind[];
  /**
   * Present only when a session approval here would create a reusable Native
   * session consent. Absent means the session answer keeps its existing, exact
   * meaning for this Operation's own class of work.
   */
  native_session_consent?: NativeSessionConsentFact | null;
  /**
   * Stable reason codes; actual command/resources come from immutable content.
   */
  reasons: string[];
  /**
   * Only capabilities not already covered by verified authority are requested.
   */
  requested_capabilities?: ResolvedCapability[] | null;
  scope: AuthorizationScope;
}
/**
 * What a "本会话允许" answer would create for a Native Shell call.
 *
 * The confirmation dialog has to describe the choice it is offering, and the
 * choice is not the same for every backend. For an ordinary restricted backend a
 * session approval covers the Operation's own kind of work; for Native it creates
 * one reusable consent that later calls can be derived from. Saying which one is
 * on the table is a fact about the proposal, so it travels with the proposal
 * rather than being guessed in the Renderer.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NativeSessionConsentFact".
 */
export interface NativeSessionConsentFact {
  /**
   * The principal the consent would be bound to.
   */
  principal_id: string;
  /**
   * The session permission revision it would be bound to. Any change to this
   * session's permissions ends it, so the dialog can say so truthfully.
   */
  session_permission_revision: number;
  /**
   * The built-in Shell tools a Native session consent may cover.
   *
   * The variant is the conservative identity: a consent is approved for one
   * concrete shell that is part of the signed-in product, never for "some
   * process on this machine". It is the *tool that was approved*, which is an audit
   * fact; the *range the approval covers* is [`NativeSessionConsentScope::shells`].
   */
  shell: 'cmd' | 'powershell';
  /**
   * The trusted built-in Shells a session approval would cover. This is the range
   * the dialog has to describe, and it is not derived in the Renderer.
   */
  shells: NativeShell[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BackendDescriptor".
 */
export interface BackendDescriptor {
  capabilities: ResolvedCapability[];
  class: BackendClass;
  compatibility: ExecutionCompatibility[];
  guarantees: EnforcementGuarantee[];
  id: BackendId;
  known_limitations: BackendKnownLimitation[];
  protocol_fingerprint: CanonicalSha256HashV16;
  readiness: BackendReadiness;
  revision: Revision;
  runtime_instance_id: RuntimeInstanceId;
}
/**
 * Identifies the backend's exact supported action protocol.
 */
export interface CanonicalSha256HashV16 {
  algorithm: 'sha256';
  canonicalization_version: 'rfc8785_jcs_v1';
  hex: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BackendStatus".
 */
export interface BackendStatus {
  backend: string;
  lifecycle: BackendLifecycle;
  readiness: BackendReadiness;
}
/**
 * Stored evidence from the owning service, not a renderer success flag.
 * Domain facts contain identities/revisions/digests, never credential values.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrokerEvidence".
 */
export interface BrokerEvidence {
  action_kind: string;
  after: any;
  before: any;
  domain_journal: any;
  request_fingerprint: CanonicalSha256HashV1;
  side_effect: SideEffectStateV1;
}
/**
 * Credential-free read. Authority is reloaded from Core's Store; grant IDs
 * identify existing approvals and cannot declare origins or destination classes.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrokerHttpReadRequest".
 */
export interface BrokerHttpReadRequest {
  max_bytes: number;
  network_grant_id: NetworkGrantId;
  session_id: SessionId;
  url: string;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrokerHttpReadResult".
 */
export interface BrokerHttpReadResult {
  body_base64: string;
  content_hash: ContentHash;
  final_origin: NormalizedOrigin;
  status: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserAssistResult".
 */
export interface BrowserAssistResult {
  /**
   * What the user answered. Aura does not check whether what they did worked.
   */
  outcome: 'done' | 'declined' | 'timed_out' | 'closed';
  /**
   * The page afterwards; refs from before are no longer valid.
   */
  page?: BrowserPageResult | null;
}
/**
 * What the page looks like after an action.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserPageResult".
 */
export interface BrowserPageResult {
  /**
   * Console messages, page errors and failed loads since the previous browser result.
   */
  console: BrowserConsoleEntryV1[];
  /**
   * Dialogs the page opened during the action, and how Aura answered them.
   */
  dialogs: string[];
  /**
   * What stopped or changed the action, such as a navigation the settings refused.
   */
  notice?: string | null;
  /**
   * The page's accessibility snapshot. Elements carry refs (e3) for click, type and the rest.
   */
  snapshot: string;
  snapshot_truncated: boolean;
  title: string;
  url: string;
}
/**
 * One thing the page reported: a console message, an uncaught error or a load that failed.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserConsoleEntryV1".
 */
export interface BrowserConsoleEntryV1 {
  /**
   * log, info, warning, error, exception or failed_load.
   */
  level: string;
  /**
   * Where it came from (address and line), when the page said.
   */
  source?: string | null;
  text: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserClickInput".
 */
export interface BrowserClickInput {
  button?: 'left' | 'right' | 'middle';
  double?: boolean;
  /**
   * The element to click: a ref from the latest snapshot, such as e12.
   */
  ref?: string | null;
  /**
   * Or a point on the page, in CSS pixels of its 1280x800 viewport (x with y).
   */
  x?: number | null;
  y?: number | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserCloseInput".
 */
export interface BrowserCloseInput {}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserCloseResult".
 */
export interface BrowserCloseResult {
  closed: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserConsoleInput".
 */
export interface BrowserConsoleInput {
  /**
   * Only errors, errors and warnings, or everything (default all).
   */
  level?: BrowserConsoleLevel | null;
  /**
   * The latest this many entries (default 50, at most 200).
   */
  max_entries?: number | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserConsoleResult".
 */
export interface BrowserConsoleResult {
  entries: BrowserConsoleEntryV1[];
  /**
   * Older entries were dropped to stay within the limit.
   */
  truncated: boolean;
  url: string;
}
/**
 * Where an element is drawn, in CSS pixels of the page's viewport, rounded.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserElementBoxV1".
 */
export interface BrowserElementBoxV1 {
  height: number;
  width: number;
  x: number;
  y: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserEvaluateInput".
 */
export interface BrowserEvaluateInput {
  /**
   * A JavaScript expression, run in the page; a promise is awaited (at most 10 seconds).
   */
  expression: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserEvaluateResult".
 */
export interface BrowserEvaluateResult {
  /**
   * The error it threw, when it threw.
   */
  exception?: string | null;
  url: string;
  /**
   * The value as JSON text (at most 20000 characters), when it ran.
   */
  value?: string | null;
  value_truncated: boolean;
}
/**
 * `browser.cancel.v1`, Core to Main: Core no longer waits for this command (the turn was
 * stopped, or Core's timer ran out). A command not started yet is dropped; a wait for the user
 * ends. An input already under way is not undone.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserHostCancelV1".
 */
export interface BrowserHostCancelV1 {
  command_id: string;
  runtime_instance_id: RuntimeInstanceId;
}
/**
 * `browser.command.v1`, Core to Main.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserHostCommandV1".
 */
export interface BrowserHostCommandV1 {
  access: BrowserPageAccessV1;
  action: BrowserHostActionV1;
  /**
   * Unguessable, one per command; the result names it.
   */
  command_id: string;
  /**
   * The host of the page when Core authorized the action (none: a blank or workspace page).
   * An input action on a page that has since moved elsewhere is refused, not carried out.
   */
  expected_host?: string | null;
  runtime_instance_id: RuntimeInstanceId;
  session_id: SessionId;
  /**
   * How long Core waits for the result.
   */
  timeout_ms: number;
}
/**
 * How the session's model page may navigate while it carries out the command.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserPageAccessV1".
 */
export interface BrowserPageAccessV1 {
  /**
   * Hosts the user approved for this session, and the one approved for this command.
   */
  approved_hosts: string[];
  /**
   * The workspace folder (a `file:` address ending in `/`) a workspace page may load from.
   */
  file_root?: string | null;
  /**
   * A ReadOnly session: the page navigates only to allowed sites and approved hosts.
   */
  read_only: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserImageV1".
 */
export interface BrowserImageV1 {
  data_base64: string;
  height: number;
  media_type: string;
  width: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserHostResultAck".
 */
export interface BrowserHostResultAck {
  /**
   * False when no command was waiting under that id (it had timed out or never existed).
   */
  accepted: boolean;
}
/**
 * `browser.host.result`, Main to Core.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserHostResultV1".
 */
export interface BrowserHostResultV1 {
  command_id: string;
  outcome: BrowserHostOutcomeV1;
  /**
   * The page's address afterwards (about:blank when there is no page).
   */
  page_url: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserHoverInput".
 */
export interface BrowserHoverInput {
  ref: string;
}
/**
 * One element, as DevTools' Elements panel shows it. Read through DevTools, no page script runs.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserInspectInput".
 */
export interface BrowserInspectInput {
  /**
   * The element's ref in the latest snapshot (e3).
   */
  element: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserInspectResult".
 */
export interface BrowserInspectResult {
  /**
   * Its border box; none when it is not drawn (display: none, detached).
   */
  box_model?: BrowserElementBoxV1 | null;
  /**
   * The element's HTML as the page has it now (at most 8000 characters).
   */
  html: string;
  html_truncated: boolean;
  /**
   * Its computed style for layout and look, one property a line ("display: flex").
   */
  styles: string[];
  url: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserNavigateInput".
 */
export interface BrowserNavigateInput {
  action: BrowserHistoryAction;
}
/**
 * One request the page made. Cookie and authorization header values are not shown.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserNetworkEntryV1".
 */
export interface BrowserNetworkEntryV1 {
  failure?: string | null;
  /**
   * document, script, image, fetch, xhr, websocket…
   */
  kind: string;
  method: string;
  mime_type?: string | null;
  request_headers: string[];
  response_headers: string[];
  status?: number | null;
  url: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserNetworkInput".
 */
export interface BrowserNetworkInput {
  /**
   * Only requests that failed or answered 400 and above.
   */
  failed_only?: boolean;
  /**
   * The latest this many requests (default 50, at most 200).
   */
  max_entries?: number | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserNetworkResult".
 */
export interface BrowserNetworkResult {
  entries: BrowserNetworkEntryV1[];
  truncated: boolean;
  url: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserOpenInput".
 */
export interface BrowserOpenInput {
  /**
   * A workspace file to open instead, such as index.html or dist/game.html.
   */
  path?: string | null;
  /**
   * An http(s) address, or a host such as localhost:5173. Give url or path, not both.
   */
  url?: string | null;
}
/**
 * The page's performance counters, as Chromium keeps them.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserPerformanceInput".
 */
export interface BrowserPerformanceInput {}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserPerformanceResult".
 */
export interface BrowserPerformanceResult {
  /**
   * One counter a line, with its unit ("JSHeapUsedSize: 12.4 MB", "LayoutCount: 42").
   */
  metrics: string[];
  url: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserPressInput".
 */
export interface BrowserPressInput {
  /**
   * Keep it held this long, in milliseconds, as a game expects (at most 3000).
   */
  hold_ms?: number | null;
  /**
   * A key or a chord: Enter, Escape, Tab, ArrowLeft, Space, a, Control+a, Shift+Tab.
   */
  key: string;
  /**
   * Press it this many times (default 1, at most 20).
   */
  repeat?: number | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserRequestUserInput".
 */
export interface BrowserRequestUserInput {
  /**
   * What the user should do on the page, in a sentence (sign in, solve a check, choose).
   */
  reason: string;
  /**
   * How long to wait for them, in seconds (default 180, at most 300).
   */
  timeout_seconds?: number | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserScreenshotImageV1".
 */
export interface BrowserScreenshotImageV1 {
  data_base64: string;
  media_type: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserScreenshotInput".
 */
export interface BrowserScreenshotInput {
  /**
   * The whole page instead of what is on screen.
   */
  full_page?: boolean;
  /**
   * Only this element (a ref from the latest snapshot, such as e7).
   */
  ref?: string | null;
}
/**
 * `browser.screenshot.read`: a screenshot a browser call of this conversation took, for the
 * conversation to show.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserScreenshotReadRequest".
 */
export interface BrowserScreenshotReadRequest {
  artifact_id: ArtifactId;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserScreenshotResult".
 */
export interface BrowserScreenshotResult {
  height: number;
  /**
   * The image, kept with this call for the user to see.
   */
  image_artifact_id: string;
  note: string;
  /**
   * Whether the image was attached for the model to look at.
   */
  shown_to_model: boolean;
  title: string;
  url: string;
  width: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserScrollInput".
 */
export interface BrowserScrollInput {
  /**
   * How far, in CSS pixels (default 600).
   */
  amount?: number | null;
  direction: BrowserScrollDirection;
  /**
   * Scroll inside this element (a ref) instead of the page.
   */
  ref?: string | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserSelectInput".
 */
export interface BrowserSelectInput {
  /**
   * The select element (a ref).
   */
  ref: string;
  /**
   * The options to choose, by value or by visible text.
   */
  values: string[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserSettings".
 */
export interface BrowserSettings {
  /**
   * Sites a ReadOnly session opens without asking.
   */
  allowed_sites: string[];
  /**
   * Sites the browser never loads, in every mode, including requests a page makes itself.
   */
  blocked_sites: string[];
  /**
   * Advanced: lets the model run scripts in pages and read full network records (default off).
   */
  debugging: boolean;
  /**
   * Whether the built-in browser can be used at all (default on).
   */
  enabled: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserSettingsSnapshot".
 */
export interface BrowserSettingsSnapshot {
  issue?: string | null;
  revision?: number | null;
  settings: BrowserSettings;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserSettingsUpdate".
 */
export interface BrowserSettingsUpdate {
  expected_revision: number;
  settings: BrowserSettings;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserSnapshotInput".
 */
export interface BrowserSnapshotInput {
  /**
   * Longest snapshot to return, in characters (default 12000, at most 60000).
   */
  max_chars?: number | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserTypeInput".
 */
export interface BrowserTypeInput {
  /**
   * Replace the field's current text instead of adding to it.
   */
  clear?: boolean;
  /**
   * The field to type into (a ref); without it, the focused element.
   */
  ref?: string | null;
  /**
   * Press Enter afterwards.
   */
  submit?: boolean;
  text: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BrowserWaitInput".
 */
export interface BrowserWaitInput {
  /**
   * Wait until this text is on the page.
   */
  text?: string | null;
  /**
   * Wait until this text is gone.
   */
  text_gone?: string | null;
  /**
   * Or simply wait this long, in milliseconds (at most 10000).
   */
  time_ms?: number | null;
  /**
   * Give up after this long, in milliseconds (default 10000, at most 30000).
   */
  timeout_ms?: number | null;
}
/**
 * The plan for a whole cross-domain recovery.
 *
 * A cross-domain restore must reach **one provable consistent version**,
 * so the bundle plan is computed as a unit. It never staples each domain's own
 * last-known-good together and calls the result consistent: either a common
 * revision is provable, or the cross-domain restore is honestly disabled and the
 * user is offered a scoped default reset instead.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "BundleRecoveryPlan".
 */
export interface BundleRecoveryPlan {
  /**
   * The common revision every affected domain can reach, when one exists.
   */
  consistent_revision?: number | null;
  /**
   * The exact set the próven consistent version restores, when one exists.
   *
   * This is the preview's own binding. `apply` consumes
   * **this** list rather than re-selecting each domain's own latest history, so
   * the write cannot silently differ from what the user was shown. Each entry is
   * `(module, revision, content digest of the target version)`.
   *
   * It is empty exactly when no joint record proved a consistent set, which is
   * also when `consistent_revision` is `None`.
   */
  consistent_set: [any, any, any][];
  /**
   * Per-domain plans, so the user sees the whole scope before triggering.
   */
  domains: DomainRecoveryPlan[];
  /**
   * A bounded explanation when a consistent version cannot be proven.
   */
  inconsistency_code?: string | null;
  modules: ConfigModule[];
  options: RecoveryOption[];
  /**
   * Core supplies this opaque binding; clients echo it without reimplementing hashing.
   */
  preview_id: string;
}
/**
 * What one domain's recovery would do, before anything is written.
 *
 * This is the **preview**. It names the affected domain, the options with their
 * real availability, the versions that could be restored, the references that
 * would need rebinding, and the actual loss.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "DomainRecoveryPlan".
 */
export interface DomainRecoveryPlan {
  /**
   * The revision currently served, if the domain serves anything.
   */
  current_revision?: number | null;
  /**
   * The domain's current state, as the health registry reports it.
   */
  degraded: boolean;
  /**
   * The stable classification of the fault that made it degraded, if any.
   */
  fault_code?: string | null;
  /**
   * What the user actually loses by restoring the previous version. Stated so
   * the choice is informed rather than a guess.
   */
  loss: string[];
  module: ConfigModule;
  /**
   * The revision the recovery result would carry: always `current + 1`.
   */
  next_revision: number;
  options: RecoveryOption[];
  /**
   * The identity of the state this preview was computed from.
   *
   * A confirmation must be bound to what
   * the user was actually shown. Measured defect: a preview of a rev1 reset was
   * still applied after a legitimate save to dark/rev2, so the stale
   * confirmation overwrote a newer, valid configuration — `current_revision` was
   * not part of any check at commit time.
   *
   * The caller echoes this back and the write refuses when the domain has moved
   * since. It is derived from the real facts the preview was built from, so a
   * caller cannot forge a preview for a state it was never shown.
   */
  preview_id: string;
  /**
   * The recorded versions, newest first, with their current-validity verdict.
   */
  recoverable_versions: RecoverableVersion[];
  /**
   * Settings keys that reference something outside the domain and would need
   * rebinding after a restore or reset. Names only; never their values.
   */
  references_needing_rebinding: string[];
  /**
   * Whether the runtime could establish this domain's recovery scope at all.
   */
  scope_known: boolean;
}
/**
 * Whether one recovery option is available, and exactly why not when it is not.
 *
 * An unavailable option is **stated**, never silently omitted and never replaced
 * by a different action. That is what "honestly disable the option" means.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RecoveryOption".
 */
export interface RecoveryOption {
  action: ConfigRecoveryAction;
  available: boolean;
  /**
   * A bounded, path-free explanation for the user.
   */
  detail?: string | null;
  /**
   * A stable code when `available` is false or when the option carries a
   * caveat. No path, no document body, nothing the user supplied.
   */
  reason_code?: string | null;
}
/**
 * One recorded version the domain could be restored to.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RecoverableVersion".
 */
export interface RecoverableVersion {
  /**
   * Whether this version still passes the **current** validator and permission.
   */
  passes_current_validation: boolean;
  /**
   * A stable code when it does not.
   */
  rejection_code?: string | null;
  revision: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CancelReceipt".
 */
export interface CancelReceipt {
  handle: ExecutionHandle;
  termination: TerminationEvidence;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExecutionHandle".
 */
export interface ExecutionHandle {
  attempt: AttemptRef;
  dispatch_fingerprint: CanonicalSha256HashV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CancellationState".
 */
export interface CancellationState {
  requested: boolean;
  revision: Revision;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CapabilityStatus".
 */
export interface CapabilityStatus {
  capability: string;
  readiness: CapabilityReadiness;
  reason_code: string;
}
/**
 * Text the user asked to copy.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ClipboardWriteRequest".
 */
export interface ClipboardWriteRequest {
  /**
   * At most 16 Mi characters; Main also refuses more than 16 MiB as UTF-8.
   */
  text: string;
}
/**
 * Whether the clipboard took the text, and what the user is told when it did not.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ClipboardWritten".
 */
export interface ClipboardWritten {
  completed: boolean;
  message: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CommandCatalog".
 */
export interface CommandCatalog {
  commands: CommandEntry[];
  /**
   * Where the user's own commands live (it may not exist yet).
   */
  user_directory: string;
  /**
   * The selected workspace's `.aura/commands` (it may not exist yet).
   */
  workspace_directory?: string | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CommandEntry".
 */
export interface CommandEntry {
  /**
   * What the text after the command means, when the command takes any.
   */
  argument_hint?: string | null;
  description: string;
  /**
   * False for a user command the user turned off. Built-ins are always enabled.
   */
  enabled: boolean;
  /**
   * Why a user command cannot be offered (invalid, or shadowed by another).
   */
  issue?: string | null;
  kind: CommandKind;
  /**
   * Where a user command's files are, for the settings page.
   */
  location?: string | null;
  /**
   * Typed after `/`: lowercase letters, digits and `-`.
   */
  name: string;
  /**
   * The plugin that brings it, for a plugin's command.
   */
  plugin?: string | null;
  /**
   * The version of a user command's files; removal or replacement names it.
   */
  revision?: string | null;
  source: CommandSource;
  /**
   * Prompt commands only. `$ARGUMENTS` marks where the typed text goes.
   */
  template?: string | null;
  title: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CommandCatalogRequest".
 */
export interface CommandCatalogRequest {
  /**
   * The workspace whose own commands are included; built-ins and the user's
   * commands are listed without one.
   */
  workspace_id?: WorkspaceId | null;
}
/**
 * Settings → 命令: remove one of the user's own commands at the revision shown. Workspace
 * commands are project files and are not removed from settings.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CommandDeleteRequest".
 */
export interface CommandDeleteRequest {
  name: string;
  revision: string;
  workspace_id?: WorkspaceId | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CommandDirectoryOpened".
 */
export interface CommandDirectoryOpened {
  completed: boolean;
  message: string;
}
/**
 * Open a command's folder, or the folder commands are added to, in the file manager.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CommandDirectoryRequest".
 */
export interface CommandDirectoryRequest {
  /**
   * None opens the folder commands of this source are added to.
   */
  name?: string | null;
  source: CommandSource;
  workspace_id?: WorkspaceId | null;
}
/**
 * Settings → 命令: turn one of the user's or the workspace's commands on or off.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CommandSetEnabledRequest".
 */
export interface CommandSetEnabledRequest {
  enabled: boolean;
  name: string;
  source: CommandSource;
  workspace_id?: WorkspaceId | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CompletionPolicyUpdate".
 */
export interface CompletionPolicyUpdate {
  /**
   * Required also when relaxing Strict. The referenced original user message
   * and exact excerpt remain audit evidence; the model cannot erase them.
   */
  authority_event_id: string;
  authority_excerpt: UserMessageExcerptSelector;
  policy: CompletionPolicy;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "UserMessageExcerptSelector".
 */
export interface UserMessageExcerptSelector {
  exact_text: string;
  occurrence: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CompletionReview".
 */
export interface CompletionReview {
  agent_run_id: AgentRunId;
  created_at: UtcTimestamp;
  input: TaskLineageReviewCompletionInput;
  lineage_id: TaskLineageId;
  lineage_revision: Revision;
  model_step_id: ModelStepId;
  review_id: CompletionReviewId;
  turn_task_id: TurnTaskId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TaskLineageReviewCompletionInput".
 */
export interface TaskLineageReviewCompletionInput {
  assessment: CompletionAssessment;
  /**
   * Default empty. What shows it, as you would cite it to the user: workspace paths, tests or
   * commands. An artifact id from a tool result must be one of this run's.
   */
  evidence_refs?: string[];
  expected_lineage_revision: Revision;
  lineage_id: TaskLineageId;
  reviewed_user_intents: ReviewedUserIntent[];
  source_read_receipt_id?: SourceReadReceiptId | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ReviewedUserIntent".
 */
export interface ReviewedUserIntent {
  description: string;
  /**
   * Default empty. What shows it, as you would cite it to the user: workspace paths, tests or
   * commands. An artifact id from a tool result must be one of this run's.
   */
  evidence_refs?: string[];
  result: ReviewedIntentResult;
  source_event_ids: EventId[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ConfigRecoveryApplyRequest".
 */
export interface ConfigRecoveryApplyRequest {
  /**
   * Which recovery action the user is asking for.
   */
  action: 'retry' | 'restore_previous_valid' | 'reset_to_default';
  /**
   * A single domain, for a single-domain recovery.
   */
  domain?: ConfigModule | null;
  /**
   * Several domains, for a cross-domain recovery that must reach one version.
   */
  domains?: ConfigModule[];
  /**
   * The identity of the preview the user confirmed.
   *
   * The apply used to **re-plan** from the
   * request's domains and discard the preview entirely, so the plan the user was
   * shown was never the plan applied, and a stale confirmation could overwrite
   * newer valid configuration. Carrying the previewed identity across the wire is
   * what lets the write refuse a confirmation that no longer describes the live
   * state.
   *
   * It is **required**. It used to be an `Option`, and
   * `None` meant "apply the current state", which was a bypass: a caller that omitted the
   * field wrote configuration no user had confirmed. A missing identity is now refused
   * before any write, exactly like a stale one.
   */
  preview_id: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ConfigRecoveryPlanRequest".
 */
export interface ConfigRecoveryPlanRequest {
  domains?: ConfigModule[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextReadCompactionInput".
 */
export interface ContextReadCompactionInput {
  agent_run_id: AgentRunId;
  sequence: number;
  session_id: SessionId;
  source_context_snapshot_id: ContextProjectionSnapshotId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ContextBrokerReadResult".
 */
export interface ContextBrokerReadResult {
  value: any;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RunnerDispatchEnvelope".
 */
export interface RunnerDispatchEnvelope {
  attempt_write_sid: string;
  dispatch: DispatchRequest;
  expires_at: UtcTimestamp;
  schema_version: ToolWireVersionV1;
  scratch_directory: ExecutionFileBinding;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "DispatchRequest".
 */
export interface DispatchRequest {
  binding: DispatchBinding;
  operation: SecuredOperation;
  plan: ExecutionPlan;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "DispatchBinding".
 */
export interface DispatchBinding {
  attempt: AttemptRef;
  authorization_fingerprint: CanonicalSha256HashV1;
  cancellation_revision: Revision;
  operation_fingerprint: CanonicalSha256HashV1;
  plan_fingerprint: CanonicalSha256HashV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SecuredOperation".
 */
export interface SecuredOperation {
  content: OperationContent;
  fingerprint: CanonicalSha256HashV1;
  operation_id: SecurityOperationId;
  schema_version: ToolWireVersionV1;
}
/**
 * Immutable domain action facts. The domain owns the action contract and
 * decoder. Backend selection cannot rewrite this canonical payload.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "OperationContent".
 */
export interface OperationContent {
  action: any;
  action_kind: string;
  action_revision: Revision;
  capabilities: ResolvedCapability[];
  compatibility: ExecutionCompatibility[];
  credentials: CredentialUseRef[];
  identity: InvocationIdentity;
  network: OperationNetwork;
  purpose?: string | null;
  resources: ResourceDescriptor[];
  risks: OperationRiskFlag[];
  source: SecuritySourceRef;
  source_revision_fingerprint?: ContentHash | null;
}
/**
 * References a domain authority and exact secret generation, never a Secret
 * value, WinCred target or a permission to obtain either.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "CredentialUseRef".
 */
export interface CredentialUseRef {
  authority: SecuritySourceRef;
  credential_id: CredentialId;
  /**
   * Secret-generation revision for one stable CredentialId.
   *
   * This is intentionally distinct from a row revision. Clear and metadata-only
   * mutations may change row concurrency without creating a new secret
   * generation.
   */
  credential_revision: string;
  use_fingerprint: CanonicalSha256HashV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExecutionPlan".
 */
export interface ExecutionPlan {
  fingerprint: CanonicalSha256HashV1;
  prepared: PreparedExecution;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PreparedExecution".
 */
export interface PreparedExecution {
  backend: BackendDescriptor;
  operation_fingerprint: CanonicalSha256HashV1;
  preparation: any;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExecutionFileBinding".
 */
export interface ExecutionFileBinding {
  /**
   * @minItems 16
   * @maxItems 16
   */
  file_id: [
    number,
    number,
    number,
    number,
    number,
    number,
    number,
    number,
    number,
    number,
    number,
    number,
    number,
    number,
    number,
    number,
  ];
  path: string;
  sha256?: ContentHash | null;
  /**
   * Decimal string: Windows volume IDs may exceed the exact JSON/JCS integer range.
   */
  volume_serial: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "EmptyToolInput".
 */
export interface EmptyToolInput {}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExecutionAcceptanceObservation".
 */
export interface ExecutionAcceptanceObservation {
  case: ExecutionAcceptanceCase;
  detail: string;
  /**
   * Report-relative evidence path, never a raw command or credential.
   */
  evidence_path: string;
  evidence_sha256: ContentHash;
  result: ExecutionCheckResult;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExecutionAccount".
 */
export interface ExecutionAccount {
  credential_sha256: ContentHash;
  name: string;
  sid: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExecutionApprovalRequest".
 */
export interface ExecutionApprovalRequest {
  approval_id: ApprovalRequestId;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExecutionApprovalResolutionRequest".
 */
export interface ExecutionApprovalResolutionRequest {
  approval_id: ApprovalRequestId;
  decision: ApprovalDecisionKind;
  expected_revision: Revision;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExecutionApprovalView".
 */
export interface ExecutionApprovalView {
  approval: AuthorizationApproval;
  backend: BackendDescriptor;
  /**
   * The permission mode in force, which is the *allowed range*. It is separate
   * from `backend`, which is *how this call will run*: FullAccess with a
   * restricted backend and FullAccess with Native are different executions of
   * the same allowance, and the confirmation has to say which one it is.
   */
  mode: 'read_only' | 'workspace_read_write' | 'full_access';
  /**
   * Present only when a session approval here would create a reusable Native
   * session consent, so the dialog describes the range the button really grants.
   */
  native_session_consent?: NativeSessionConsentFact | null;
  /**
   * Present when this call will use the frozen step proxy. The sentence is the whole fact.
   */
  network_proxy_notice?: string | null;
  operation: SecuredOperation;
  reasons: string[];
  requested_capabilities?: ResolvedCapability[] | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExecutionAttempt".
 */
export interface ExecutionAttempt {
  binding: DispatchBinding;
  dispatched_at?: UtcTimestamp | null;
  prepared_at: UtcTimestamp;
  state: AttemptState;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExecutionHostAcceptanceReport".
 */
export interface ExecutionHostAcceptanceReport {
  acceptance_version: string;
  assurance_version: string;
  candidate_inventory_sha256: ContentHash4;
  cmd_image: ExecutionFileBinding;
  core_sha256: ContentHash;
  edition: ExecutionHostEdition;
  helper_sha256: ContentHash;
  installation_fingerprint: CanonicalSha256HashV1;
  maintainer_sha256: ContentHash;
  observations: ExecutionAcceptanceObservation[];
  observed_at: UtcTimestamp;
  powershell_manifest_sha256: ContentHash;
  runner_sha256: ContentHash;
  schema_version: ToolWireVersionV1;
  source_fingerprint: CanonicalSha256HashV1;
  windows_build: string;
}
/**
 * SHA-256 of the immutable candidate's bundle-inventory.json bytes.
 */
export interface ContentHash4 {
  algorithm: ContentHashAlgorithm;
  hex: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExtensionStatus".
 */
export interface ExtensionStatus {
  extension: string;
  readiness: ExtensionReadiness;
  reason_code: string;
}
/**
 * A web page the conversation links to; `http` and `https` only.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ExternalLinkOpenRequest".
 */
export interface ExternalLinkOpenRequest {
  url: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GitBranchResultV1".
 */
export interface GitBranchResultV1 {
  branch: string;
  created: boolean;
  execution_notice: string;
  head?: string | null;
  previous_branch?: string | null;
  previous_head?: string | null;
  status: GitStatusResultV1;
  switched: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GitStatusResultV1".
 */
export interface GitStatusResultV1 {
  ahead: number;
  behind: number;
  branch?: string | null;
  branches: GitBranchV1[];
  conflicts: string[];
  detached: boolean;
  entries: GitChangeV1[];
  head?: string | null;
  next_offset?: number | null;
  operation?: string | null;
  /**
   * Submodule contents are deliberately not traversed or executed.
   */
  submodule_contents_checked: boolean;
  total_entries: number;
  truncated: boolean;
  upstream?: string | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GitBranchV1".
 */
export interface GitBranchV1 {
  commit: string;
  name: string;
  remote: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GitChangeV1".
 */
export interface GitChangeV1 {
  /**
   * Present only when an attribute selects an external converter suppressed by isolated reads.
   */
  comparison_note?: string | null;
  conflict: boolean;
  index_status: string;
  original_path?: string | null;
  path: string;
  submodule: boolean;
  untracked: boolean;
  worktree_status: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GitCommitInput".
 */
export interface GitCommitInput {
  /**
   * Required non-empty commit message. No amend, editor or hook bypass option.
   */
  message: string;
  /**
   * Literal workspace-relative files to stage and commit; excludes unrelated staged files.
   *
   * @maxItems 200
   */
  paths?: string[];
  /**
   * Commit the existing index only. Mutually exclusive with non-empty paths.
   */
  staged_only?: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GitCommitResultV1".
 */
export interface GitCommitResultV1 {
  branch?: string | null;
  commit: GitCommitV1;
  execution_notice: string;
  status: GitStatusResultV11;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GitCommitV1".
 */
export interface GitCommitV1 {
  author: string;
  authored_at: string;
  body: string;
  commit: string;
  files: GitFileStatV1[];
  parents: string[];
  subject: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GitFileStatV1".
 */
export interface GitFileStatV1 {
  added?: number | null;
  binary: boolean;
  comparison_note?: string | null;
  deleted?: number | null;
  original_path?: string | null;
  path: string;
}
/**
 * Bounded post-operation status, including remaining untracked and staged changes.
 */
export interface GitStatusResultV11 {
  ahead: number;
  behind: number;
  branch?: string | null;
  branches: GitBranchV1[];
  conflicts: string[];
  detached: boolean;
  entries: GitChangeV1[];
  head?: string | null;
  next_offset?: number | null;
  operation?: string | null;
  /**
   * Submodule contents are deliberately not traversed or executed.
   */
  submodule_contents_checked: boolean;
  total_entries: number;
  truncated: boolean;
  upstream?: string | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GitDiffInput".
 */
export interface GitDiffInput {
  max_bytes?: number;
  offset_bytes?: number;
  paths?: string[];
  target?:
    | {
        kind: 'unstaged';
      }
    | {
        kind: 'staged';
      }
    | {
        commit: string;
        kind: 'commit';
      }
    | {
        from: string;
        kind: 'range';
        to: string;
      };
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GitDiffResultV1".
 */
export interface GitDiffResultV1 {
  files: GitFileStatV1[];
  next_offset?: number | null;
  offset_bytes: number;
  patch: string;
  total_bytes: number;
  truncated: boolean;
  warnings: string[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GitInstallationResultV1".
 */
export interface GitInstallationResultV1 {
  opened: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GitLogInput".
 */
export interface GitLogInput {
  commit?: string | null;
  limit?: number;
  paths?: string[];
  start?: string | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GitLogResultV1".
 */
export interface GitLogResultV1 {
  commits: GitCommitV1[];
  truncated: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GitPushInput".
 */
export interface GitPushInput {
  branch?: string | null;
  /**
   * Full expected remote object ID. An exact lease, never a bare force or tracking-ref lease.
   */
  force_with_lease?: string | null;
  remote?: string | null;
  remote_branch?: string | null;
  set_upstream?: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GitStatusInput".
 */
export interface GitStatusInput {
  include_branches?: boolean;
  include_untracked?: boolean;
  limit?: number;
  offset?: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GitUntrackedDiffInputV1".
 */
export interface GitUntrackedDiffInputV1 {
  max_bytes?: number;
  offset_bytes?: number;
  path: string;
}
/**
 * The review page's comparison. Untracked files are not part of it: status lists them and
 * `UntrackedDiff` reads them one by one.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GitReviewDiffInputV1".
 */
export interface GitReviewDiffInputV1 {
  /**
   * None: the uncommitted changes, staged or not, against HEAD (the empty tree before the
   * first commit). A branch: the working tree against the commit where HEAD left it.
   */
  base?: string | null;
  /**
   * Lines that differ only in whitespace are not changes; files with nothing else drop out.
   */
  ignore_whitespace?: boolean;
  max_bytes?: number;
  offset_bytes?: number;
  paths?: string[];
  /**
   * Only the files and their line counts, with an empty patch. The review page lists
   * the files this way and reads each file's lines when it is shown.
   */
  stats_only?: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GitRefUpdateV1".
 */
export interface GitRefUpdateV1 {
  after?: string | null;
  before?: string | null;
  reference: string;
  result: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GitRemoteResultV1".
 */
export interface GitRemoteResultV1 {
  execution_notice: string;
  kind: string;
  local_branch?: string | null;
  refs: GitRefUpdateV1[];
  remote: string;
  remote_branch?: string | null;
  /**
   * Normalized display and consent URL; has no userinfo, query or fragment.
   */
  remote_url: string;
  status: GitStatusResultV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GitRuntimeStatusV1".
 */
export interface GitRuntimeStatusV1 {
  executable?: string | null;
  installation_url: string;
  message: string;
  next_step: string;
  state: GitRuntimeStateV1;
  version?: string | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GitWorkspaceReadRequestV1".
 */
export interface GitWorkspaceReadRequestV1 {
  query: GitReadInputV1;
  session_id: SessionId;
  workspace_id: WorkspaceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GoalCheckInput".
 */
export interface GoalCheckInput {
  assessment: string;
  /**
   * Default empty. What shows it, as you would cite it to the user: workspace paths, tests or
   * commands. An artifact id from a tool result must be one of this run's.
   */
  evidence_refs?: string[];
  expected_goal_revision: Revision;
  /**
   * Default empty.
   */
  remaining_work?: string[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "GoalCompleteInput".
 */
export interface GoalCompleteInput {
  /**
   * Default empty. What shows it, as you would cite it to the user: workspace paths, tests or
   * commands. An artifact id from a tool result must be one of this run's.
   */
  evidence_refs?: string[];
  expected_goal_revision: Revision;
  final_report: string;
  verification?: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HookDefinition".
 */
export interface HookDefinition {
  command: HookCommand;
  description: string;
  events: HookEvent[];
  failure_policy: HookFailurePolicy;
  kind: HookKind;
  name: string;
  timeout_ms: number;
  /**
   * Tool names (`fs.write`) or families (`shell.*`) for `tool.*` events; empty is every tool.
   */
  tools: string[];
  version: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HookEntryView".
 */
export interface HookEntryView {
  definition?: HookDefinition | null;
  enabled: boolean;
  fingerprint?: string | null;
  path: string;
  /**
   * The plugin that brings it; it is trusted, switched and removed with the plugin.
   */
  plugin?: string | null;
  problems: string[];
  state: HookEntryState;
  trusted_fingerprint?: string | null;
}
/**
 * Newest first.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HookInvocationList".
 */
export interface HookInvocationList {
  invocations: HookInvocationView[];
  /**
   * Every call of the turn the request names, however many the page holds.
   */
  statistics?: HookTurnStatistics | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HookInvocationView".
 */
export interface HookInvocationView {
  duration_ms: number;
  effect: HookEffect;
  event: HookEvent;
  failure?: HookFailureKind | null;
  fingerprint: string;
  hook_name: string;
  hook_version: string;
  invocation_id: HookInvocationId;
  outcome: HookOutcome;
  path: string;
  /**
   * The block reason, or what went wrong.
   */
  reason?: string | null;
  session_id?: SessionId | null;
  started_at: UtcTimestamp;
  /**
   * The start of stderr, for the user's own debugging.
   */
  stderr?: string | null;
  /**
   * A short description of the transform.
   */
  summary?: string | null;
  tool_call_id?: ToolCallId | null;
  turn_task_id?: TurnTaskId | null;
}
/**
 * A turn's Hook calls, as the session view shows them when the turn ends.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HookTurnStatistics".
 */
export interface HookTurnStatistics {
  blocked: number;
  calls: number;
  failed: number;
  succeeded: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HookInvocationListRequest".
 */
export interface HookInvocationListRequest {
  limit: number;
  /**
   * One Hook's calls, or every Hook's when absent.
   */
  path?: string | null;
  turn_task_id?: TurnTaskId | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HookListRequest".
 */
export interface HookListRequest {}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HookMoveRequest".
 */
export interface HookMoveRequest {
  direction: HookMoveDirection;
  expected_revision: number;
  path: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HookPackageFile".
 */
export interface HookPackageFile {
  bytes: number;
  /**
   * Relative to the package root, with `/` separators.
   */
  path: string;
}
/**
 * Untrust or remove one Hook. Removing only drops the registry entry; the folder stays.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HookPathRequest".
 */
export interface HookPathRequest {
  expected_revision: number;
  path: string;
}
/**
 * Everything the user sees before trusting a package.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HookPreview".
 */
export interface HookPreview {
  definition?: HookDefinition | null;
  files: HookPackageFile[];
  fingerprint?: string | null;
  path: string;
  problems: string[];
  /**
   * Already in the registry (trusting again replaces its fingerprint).
   */
  registered: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HookPreviewRequest".
 */
export interface HookPreviewRequest {
  path: string;
}
/**
 * The registry in execution order.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HookRegistryView".
 */
export interface HookRegistryView {
  entries: HookEntryView[];
  revision: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HookSelectDirectoryRequest".
 */
export interface HookSelectDirectoryRequest {}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HookSelectedDirectory".
 */
export interface HookSelectedDirectory {
  path?: string | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HookSetEnabledRequest".
 */
export interface HookSetEnabledRequest {
  enabled: boolean;
  expected_revision: number;
  path: string;
}
/**
 * Trust exactly the previewed content; adds the Hook when it is not registered yet.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HookTrustRequest".
 */
export interface HookTrustRequest {
  enabled: boolean;
  expected_fingerprint: string;
  expected_revision: number;
  path: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HttpDownloadInput".
 */
export interface HttpDownloadInput {
  /**
   * For a site that serves a file only to its own pages: User-Agent, Referer, Accept and
   * Accept-Language only.
   */
  headers?: HttpHeader[];
  overwrite?: boolean;
  path: string;
  url: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HttpHeader".
 */
export interface HttpHeader {
  name: string;
  value: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HttpDownloadResult".
 */
export interface HttpDownloadResult {
  bytes_written: number;
  content_type?: string | null;
  final_url: string;
  hash: ContentHash;
  overwritten: boolean;
  path: string;
  published: boolean;
  status: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HttpRequestInput".
 */
export interface HttpRequestInput {
  body?: HttpRequestBody | null;
  /**
   * Selects an existing credential slot configured by the user for this exact tool.
   */
  credential?: string | null;
  headers?: HttpHeader[];
  method?: string;
  url: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "HttpRequestResult".
 */
export interface HttpRequestResult {
  body_kind: HttpBodyKind;
  bytes_received: number;
  elapsed_ms: number;
  /**
   * Safe display URL: credentials and query values are never exposed here.
   */
  final_url: string;
  headers: HttpHeader[];
  hint?: string | null;
  json?: any;
  redirects: string[];
  status: number;
  text?: string | null;
  transfer_complete: boolean;
  truncated: boolean;
}
/**
 * Report keys are opaque digests/typed IDs, never row payloads, secret values,
 * Windows credential targets or diagnostic messages copied from legacy data.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ImportItem".
 */
export interface ImportItem {
  disposition: ImportDisposition;
  kind: string;
  reason_code: string;
  record_count: number;
  source_key: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ImportReport".
 */
export interface ImportReport {
  already_applied: boolean;
  applied: boolean;
  items: ImportItem[];
  schema_version: ToolWireVersionV1;
  source_fingerprint: ContentHash;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "InstallationManifest".
 */
export interface InstallationManifest {
  assurance_version: string;
  cmd_image: ExecutionFileBinding;
  core_image: ExecutionFileBinding;
  installation_id: string;
  installation_version: string;
  maintenance_config_sha256: ContentHash;
  offline: ExecutionAccount;
  online?: ExecutionAccount | null;
  owner_sid: string;
  powershell_image: ExecutionFileBinding;
  resource_record_hash: ContentHash;
  revision: Revision;
  runner_image: ExecutionFileBinding;
  schema_version: ToolWireVersionV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ListMemorySessionConsentsInput".
 */
export interface ListMemorySessionConsentsInput {
  include_inactive?: boolean;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ListNetworkSessionConsentsInput".
 */
export interface ListNetworkSessionConsentsInput {
  include_inactive?: boolean;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MaintenanceConfig".
 */
export interface MaintenanceConfig {
  /**
   * Exact existing ACL ownership handed off during elevated Setup.
   */
  initial_resources: MaintenanceResourceReceipt[];
  installation_id: string;
  offline_sid: string;
  online_sid?: string | null;
  owner_sid: string;
  schema: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MaintenanceResourceReceipt".
 */
export interface MaintenanceResourceReceipt {
  after: string;
  before: string;
  boot_id: string;
  /**
   * Kernel object identity that survives NT BootIdentifier reuse across
   * Restart. Absent on journals written before this field existed.
   */
  object_generation?: string | null;
  parent_dacl?: string | null;
  state: MaintenanceResourceState;
  target: MaintenanceTarget;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MaintenanceJournal".
 */
export interface MaintenanceJournal {
  config_sha256: ContentHash;
  error?: string | null;
  observed_at: UtcTimestamp;
  resources: MaintenanceResourceReceipt[];
  schema: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ManagedProcessListV1".
 */
export interface ManagedProcessListV1 {
  processes: ManagedProcessViewV1[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ManagedProcessViewV1".
 */
export interface ManagedProcessViewV1 {
  command: string;
  kind: ManagedProcessKindV1;
  started_at: UtcTimestamp;
  terminal_id?: string | null;
  tool_call_id: ToolCallId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ManagedProcessRequestV1".
 */
export interface ManagedProcessRequestV1 {
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ManagedProcessStopRequestV1".
 */
export interface ManagedProcessStopRequestV1 {
  session_id: SessionId;
  tool_call_id: ToolCallId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ManagedProcessStopResultV1".
 */
export interface ManagedProcessStopResultV1 {
  stopped: boolean;
}
/**
 * Model-visible command input. Runtime location, PATH, arguments, credentials,
 * account identity and channel are deliberately not request fields.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ManagedShellCommand".
 */
export interface ManagedShellCommand {
  max_output_bytes: number;
  script: string;
  shell: ManagedShell;
  stdin: string;
  timeout_ms: number;
  working_directory: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "McpDescribeRequest".
 */
export interface McpDescribeRequest {
  id: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "McpEnvironmentEntry".
 */
export interface McpEnvironmentEntry {
  name: string;
  value: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "McpHeader".
 */
export interface McpHeader {
  name: string;
  value: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "McpListRequest".
 */
export interface McpListRequest {}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "McpPreview".
 */
export interface McpPreview {
  fingerprint?: string | null;
  problems: string[];
  resolved_command?: ResolvedCommand | null;
}
/**
 * The program a stdio command resolved to when the user previewed or trusted it.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ResolvedCommand".
 */
export interface ResolvedCommand {
  /**
   * Absolute path of the executable or `.cmd`/`.bat` script.
   */
  path: string;
  sha256: string;
}
/**
 * Check a configuration before saving it: problems, the resolved command, the fingerprint.
 * Nothing is started.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "McpPreviewRequest".
 */
export interface McpPreviewRequest {
  server: McpServerConfig;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "McpServerConfig".
 */
export interface McpServerConfig {
  /**
   * Saved for multi-agent; no sub-agent exists yet.
   */
  available_for_subagents?: boolean;
  call_timeout_ms?: number;
  display_name: string;
  enabled?: boolean;
  /**
   * Stable: it names the tools (`mcp.<id>.<tool>`); the display name may change freely.
   */
  id: string;
  startup_timeout_ms?: number;
  transport: McpTransport;
  trusted_fingerprint?: string | null;
}
/**
 * An environment variable whose value is a secret Aura keeps (never in this file).
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "McpSecretEnvironment".
 */
export interface McpSecretEnvironment {
  name: string;
  secret_id: CredentialId;
}
/**
 * Start the server now and list its tools again (the settings page's test).
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "McpRefreshRequest".
 */
export interface McpRefreshRequest {
  id: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "McpRegistryView".
 */
export interface McpRegistryView {
  revision: number;
  servers: McpServerView[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "McpServerView".
 */
export interface McpServerView {
  config: McpServerConfig;
  discovered_at?: UtcTimestamp | null;
  /**
   * What would be trusted now; absent when the command cannot be resolved.
   */
  fingerprint?: string | null;
  last_error?: string | null;
  /**
   * The plugin that brings it; it is changed and removed with the plugin.
   */
  plugin?: string | null;
  problems: string[];
  protocol_version?: string | null;
  resolved_command?: ResolvedCommand | null;
  /**
   * Session instances running now.
   */
  running_instances: number;
  server_name?: string | null;
  server_version?: string | null;
  /**
   * Present for a server that signs in with OAuth.
   */
  sign_in?: McpSignInView | null;
  state: McpServerState;
  tool_count?: number | null;
}
/**
 * A server's OAuth sign-in as the settings page shows it. Tokens never appear here.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "McpSignInView".
 */
export interface McpSignInView {
  /**
   * The host of the authorization server the browser was sent to.
   */
  authorization_server?: string | null;
  /**
   * Why the last sign-in or refresh did not complete.
   */
  last_error?: string | null;
  /**
   * A sign-in waits for the browser.
   */
  pending: boolean;
  signed_in: boolean;
}
/**
 * Add a server (`replaces` absent) or change one. Trust and the on/off switch are never
 * taken from this request: a new server starts untrusted and off, and a changed one keeps
 * its old trust, which no longer matches when anything that runs has changed.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "McpSaveRequest".
 */
export interface McpSaveRequest {
  expected_revision: number;
  replaces?: string | null;
  server: McpServerConfig;
}
/**
 * Write a secret for one server. The value travels only on the dedicated secret input,
 * never in these parameters. Without `secret_id` a new secret is created; with one, its next
 * value replaces the current one (the server's trust is unchanged).
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "McpSecretSetRequest".
 */
export interface McpSecretSetRequest {
  secret_id?: CredentialId | null;
  server_id: string;
}
/**
 * The ID the server's configuration names the secret by; the value is never returned, and
 * an answer with anything more is refused.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "McpSecretSetResult".
 */
export interface McpSecretSetResult {
  secret_id: CredentialId;
}
/**
 * One server with its tools and the end of its standard error output.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "McpServerDetail".
 */
export interface McpServerDetail {
  server: McpServerView;
  skipped_tools: string[];
  stderr: string[];
  tools: McpToolView[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "McpToolView".
 */
export interface McpToolView {
  canonical_name: string;
  description: string;
  title?: string | null;
  wire_name: string;
}
/**
 * Untrust or remove one server.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "McpServerRequest".
 */
export interface McpServerRequest {
  expected_revision: number;
  id: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "McpSetEnabledRequest".
 */
export interface McpSetEnabledRequest {
  enabled: boolean;
  expected_revision: number;
  id: string;
}
/**
 * The address the browser opens and its authorization server's host (shown to the user,
 * and as a link to copy when no browser opened).
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "McpSignInBegun".
 */
export interface McpSignInBegun {
  authorization_server: string;
  authorization_url: string;
}
/**
 * Sign in to a trusted OAuth server. Core prepares the sign-in and answers with the
 * authorization address; Main opens it in the system browser (https, or this computer only);
 * the browser's return completes the sign-in in Core.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "McpSignInRequest".
 */
export interface McpSignInRequest {
  id: string;
}
/**
 * Forget a server's sign-in.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "McpSignOutRequest".
 */
export interface McpSignOutRequest {
  id: string;
}
/**
 * Trust exactly the shown fingerprint and switch the server on.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "McpTrustRequest".
 */
export interface McpTrustRequest {
  expected_fingerprint: string;
  expected_revision: number;
  id: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MediaFrameResult".
 */
export interface MediaFrameResult {
  actual_time: number;
  image_artifact_id?: ArtifactId | null;
  requested_time: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MediaFramesInput".
 */
export interface MediaFramesInput {
  /**
   * Uniform interior samples: duration*(i+0.5)/count, 1..12.
   */
  count?: number | null;
  /**
   * Longest side of each image, or the complete contact sheet: 64..2048, default 1024.
   */
  max_side?: number;
  path: string;
  sheet?: boolean;
  /**
   * Exact requested seconds, in [0,duration); choose this or count, never both.
   */
  times?: number[] | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MediaFramesResult".
 */
export interface MediaFramesResult {
  frames: MediaFrameResult[];
  note: string;
  sheet_artifact_id?: ArtifactId | null;
  shown_to_model: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MediaHostCancelV1".
 */
export interface MediaHostCancelV1 {
  command_id: string;
  runtime_instance_id: RuntimeInstanceId;
  session_id: SessionId;
}
/**
 * Main receives no path, only a bounded command whose bytes remain owned by Core.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MediaHostCommandV1".
 */
export interface MediaHostCommandV1 {
  action: MediaHostActionV1;
  command_id: string;
  file_size: number;
  media_type: string;
  runtime_instance_id: RuntimeInstanceId;
  session_id: SessionId;
  timeout_ms: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MediaHostFrameV1".
 */
export interface MediaHostFrameV1 {
  actual_time: number;
  image?: MediaImageV1 | null;
  requested_time: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MediaImageV1".
 */
export interface MediaImageV1 {
  data_base64: string;
  height: number;
  media_type: string;
  width: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MediaHostReadRequestV1".
 */
export interface MediaHostReadRequestV1 {
  command_id: string;
  length: number;
  offset: number;
  runtime_instance_id: RuntimeInstanceId;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MediaHostReadResultV1".
 */
export interface MediaHostReadResultV1 {
  data_base64: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MediaHostResultAck".
 */
export interface MediaHostResultAck {
  accepted: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MediaHostResultV1".
 */
export interface MediaHostResultV1 {
  command_id: string;
  outcome: MediaHostOutcomeV1;
  runtime_instance_id: RuntimeInstanceId;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MediaProbeInput".
 */
export interface MediaProbeInput {
  /**
   * A workspace file or a file already authorized for reading in this conversation.
   */
  path: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MediaProbeResult".
 */
export interface MediaProbeResult {
  byte_length: number;
  codec?: string | null;
  container?: string | null;
  duration: number;
  frame_rate?: number | null;
  has_audio?: boolean | null;
  height: number;
  width: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryCandidateContent".
 */
export interface MemoryCandidateContent {
  basis: string;
  body: string;
  category: string;
  /**
   * 0 to 1: how sure the note is.
   */
  confidence?: number | null;
  /**
   * 1 to 5: 3 is ordinary, 5 is critical.
   */
  importance?: number | null;
  tags?: string[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryCreateInput".
 */
export interface MemoryCreateInput {
  items: MemoryCreateItem[];
  scope: MemoryScope;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryCreateItem".
 */
export interface MemoryCreateItem {
  body: string;
  category: string;
  /**
   * 0 to 1: how sure the note is.
   */
  confidence?: number | null;
  conflicts_with_ids?: MemoryId[];
  /**
   * 1 to 5: 3 is ordinary, 5 is critical.
   */
  importance?: number | null;
  path: string;
  /**
   * 0 or more: how strongly recall prefers the note (1 is ordinary).
   */
  recall_weight?: number | null;
  related_ids?: MemoryId[];
  supersedes_ids?: MemoryId[];
  tags?: string[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryDeleteInput".
 */
export interface MemoryDeleteInput {
  items: MemoryDeleteItem[];
  scope: MemoryScope;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryDeleteItem".
 */
export interface MemoryDeleteItem {
  expected_hash: string;
  target: MemoryTarget;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryDesktopActionResult".
 */
export interface MemoryDesktopActionResult {
  completed: boolean;
  message: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryDesktopRepairApply".
 */
export interface MemoryDesktopRepairApply {
  preview_id: string;
  vault: MemoryVaultRequest;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryVaultRequest".
 */
export interface MemoryVaultRequest {
  scope: MemoryScope;
  workspace_id?: WorkspaceId | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryDesktopRequest".
 */
export interface MemoryDesktopRequest {
  workspace_id?: WorkspaceId | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryDesktopSnapshot".
 */
export interface MemoryDesktopSnapshot {
  recent_runs: MemoryMaintenanceRun[];
  vaults: MemoryVaultStatus[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryMaintenanceRun".
 */
export interface MemoryMaintenanceRun {
  finished_at?: UtcTimestamp | null;
  input_tokens: number;
  model_profile_id?: ModelProfileId | null;
  output_tokens: number;
  reason?: string | null;
  run_id: string;
  runtime_instance_id: RuntimeInstanceId;
  session_id: SessionId;
  started_at: UtcTimestamp;
  state: MemoryMaintenanceState;
  turn_id?: string | null;
  /**
   * Counts observed so far; false means final Provider usage is unconfirmed.
   */
  usage_complete?: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryVaultStatus".
 */
export interface MemoryVaultStatus {
  directory: string;
  exists: boolean;
  invalid: number;
  /**
   * The plain text log of this Vault's finished maintenance runs, once one was written.
   */
  maintenance_log?: string | null;
  rebuilding: boolean;
  recent_changes: MemoryHistory[];
  scope: MemoryScope;
  valid: number;
  watcher_error?: string | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryHistory".
 */
export interface MemoryHistory {
  action: string;
  actor: string;
  after_hash?: string | null;
  at: UtcTimestamp;
  before_hash?: string | null;
  id?: MemoryId | null;
  path: string;
  session_id?: SessionId | null;
  title: string;
  turn_id?: string | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryDocument".
 */
export interface MemoryDocument {
  body: string;
  metadata: MemoryMetadata;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryMetadata".
 */
export interface MemoryMetadata {
  category: string;
  confidence: number;
  conflicts_with_ids: MemoryId[];
  created_at: UtcTimestamp;
  id: MemoryId;
  importance: number;
  last_confirmed_at?: UtcTimestamp | null;
  recall_weight: number;
  related_ids: MemoryId[];
  schema_version: number;
  scope: MemoryScope;
  source_events: string[];
  source_sessions: SessionId[];
  status: MemoryStatus;
  supersedes_ids: MemoryId[];
  tags: string[];
  updated_at: UtcTimestamp;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryEventCandidate".
 */
export interface MemoryEventCandidate {
  basis: string;
  summary: string;
  title: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryExportChunk".
 */
export interface MemoryExportChunk {
  bytes: number[];
  eof: boolean;
  next_offset: number;
  offset: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryExportChunkRequest".
 */
export interface MemoryExportChunkRequest {
  export_id: string;
  index: number;
  max_bytes: number;
  offset: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryExportCloseRequest".
 */
export interface MemoryExportCloseRequest {
  export_id: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryExportEntry".
 */
export interface MemoryExportEntry {
  directory: boolean;
  length: number;
  path: string;
  sha256: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryExportEntryRequest".
 */
export interface MemoryExportEntryRequest {
  export_id: string;
  index: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryExportHandle".
 */
export interface MemoryExportHandle {
  entries: number;
  export_id: string;
  total_bytes: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryInvalidFile".
 */
export interface MemoryInvalidFile {
  path: string;
  reason: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryLinkInput".
 */
export interface MemoryLinkInput {
  relation: MemoryRelation;
  scope: MemoryScope;
  source: MemoryId;
  target: MemoryId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryMaintenanceOutput".
 */
export interface MemoryMaintenanceOutput {
  candidates: MemoryCandidate[];
  events: MemoryEventCandidate[];
  schema_version: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryMaintenanceResult".
 */
export interface MemoryMaintenanceResult {
  created: number;
  events: number;
  ignored: number;
  updated: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryMutationResult".
 */
export interface MemoryMutationResult {
  entries: MemoryReceipt[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryReceipt".
 */
export interface MemoryReceipt {
  hash: string;
  id: MemoryId;
  path: string;
  scope: MemoryScope;
  title: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryRead".
 */
export interface MemoryRead {
  hash: string;
  history: MemoryHistory[];
  note: MemoryDocument;
  path: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryReadInput".
 */
export interface MemoryReadInput {
  target: MemorySelector;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryRecentInput".
 */
export interface MemoryRecentInput {
  limit?: number | null;
  scope: MemoryScope;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryRepairChange".
 */
export interface MemoryRepairChange {
  changes: string[];
  expected_hash?: string | null;
  path: string;
  proposed_hash: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryRepairInput".
 */
export interface MemoryRepairInput {
  phase: MemoryRepairPhase;
  preview_token?: string | null;
  scope: MemoryScope;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryRepairPreview".
 */
export interface MemoryRepairPreview {
  changes: MemoryRepairChange[];
  manual: MemoryInvalidFile[];
  preview_token: string;
  rebuild_index: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryRepairResult".
 */
export interface MemoryRepairResult {
  applied: boolean;
  preview: MemoryRepairPreview;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryScopeInput".
 */
export interface MemoryScopeInput {
  scope: MemoryScope;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemorySearchHit".
 */
export interface MemorySearchHit {
  excerpt: string;
  hash: string;
  metadata: MemoryMetadata;
  path: string;
  scope: MemoryScope;
  score: number;
  title: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemorySearchInput".
 */
export interface MemorySearchInput {
  category?: string | null;
  include_inactive?: boolean;
  limit?: number | null;
  query: string;
  scope: MemoryScope;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemorySearchResult".
 */
export interface MemorySearchResult {
  hits: MemorySearchHit[];
  incomplete: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemorySessionConsentView".
 */
export interface MemorySessionConsentView {
  grant_id: PermissionGrantId;
  grant_revision: Revision;
  /**
   * False once revoked; a revoked answer is kept for the record and covers nothing.
   */
  in_force: boolean;
  issued_at: UtcTimestamp;
  revoked_at?: UtcTimestamp | null;
  scope: MemorySessionConsentRef;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemorySettings".
 */
export interface MemorySettings {
  automatic_maintenance: boolean;
  event_retention_days: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemorySettingsSnapshot".
 */
export interface MemorySettingsSnapshot {
  issue?: string | null;
  revision?: number | null;
  settings: MemorySettings;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemorySettingsUpdate".
 */
export interface MemorySettingsUpdate {
  expected_revision: number;
  settings: MemorySettings;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryStateInput".
 */
export interface MemoryStateInput {
  scope: MemoryScope;
  targets: MemoryTarget[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryUpdateInput".
 */
export interface MemoryUpdateInput {
  items: MemoryUpdateItem[];
  scope: MemoryScope;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryUpdateItem".
 */
export interface MemoryUpdateItem {
  body?: string | null;
  category?: string | null;
  /**
   * 0 to 1: how sure the note is.
   */
  confidence?: number | null;
  conflicts_with_ids?: MemoryId[] | null;
  expected_hash: string;
  /**
   * 1 to 5: 3 is ordinary, 5 is critical.
   */
  importance?: number | null;
  /**
   * 0 or more: how strongly recall prefers the note (1 is ordinary).
   */
  recall_weight?: number | null;
  related_ids?: MemoryId[] | null;
  status?: MemoryStatus | null;
  supersedes_ids?: MemoryId[] | null;
  tags?: string[] | null;
  target: MemoryTarget;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MemoryValidation".
 */
export interface MemoryValidation {
  invalid: MemoryInvalidFile[];
  rebuilding: boolean;
  valid: number;
}
/**
 * A file outside the workspace that a message of this conversation names by its full path.
 * Core reads that message and finds the path in it before saying where the file is, so only a
 * path the conversation shows can be opened; Aura's own data and the user's credential stores are
 * not. For Main only.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "MentionedFileRequest".
 */
export interface MentionedFileRequest {
  mentioned_in: AgentMessageTextSource;
  path: string;
  session_id: SessionId;
}
/**
 * Which of a provider's connections its models use, and which stand in when it fails before
 * answering. An empty choice returns each model to its own connection with no backup.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ModelProviderConnectionsRequest".
 */
export interface ModelProviderConnectionsRequest {
  backup_endpoint_ids: EndpointId[];
  default_endpoint_id?: EndpointId | null;
  expected_revision: ExpectedRevision;
  provider_id: ProviderId;
}
/**
 * The actor a Native session consent is bound to, and the range it covers.
 *
 * Naming the principal is what keeps "the same conversation" from meaning "the
 * same browser tab": an approval granted while the user was driving the session
 * does not authorize an Agent-initiated call in that session, and no other
 * session or Core runtime instance can borrow it. It is an authorization
 * binding, not an operating-system isolation claim — a Native process still
 * runs as the signed-in Windows user and is not confined to the workspace.
 *
 * `shells` is the covered range. It is stored as a set rather than derived from a
 * single Shell so that two facts stay separate: *which
 * tool was approved* (`approved_shell`, audit) and *what the approval covers*
 * (`shells`, authorization). Nothing in the reuse test reads `approved_shell`.
 *
 * Two stored shapes are read, and they are not interchangeable:
 *
 * * the current shape — `approved_shell` plus the `shells` range,
 * * the predecessor shape — a single `shell`, whose approval covered exactly that
 *   one Shell. It is read as a one-Shell range and **never widened**: covering a
 *   second Shell is a new answer from the user, not something a reader may infer
 *   from the fact that both Shells are built in today.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NativeSessionConsentScope".
 */
export interface NativeSessionConsentScope1 {
  /**
   * The Shell the user actually confirmed. Audit only.
   */
  approved_shell: 'cmd' | 'powershell';
  principal_id: PrincipalId;
  /**
   * The trusted built-in Shells this consent authorizes.
   */
  shells: NativeShell[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NetworkSessionConsentView".
 */
export interface NetworkSessionConsentView {
  availability: NetworkSessionConsentAvailability;
  expires_at?: UtcTimestamp | null;
  grant_id: PermissionGrantId;
  grant_revision: Revision;
  issued_at: UtcTimestamp;
  revoked_at?: UtcTimestamp | null;
  scope: NetworkSessionConsentRef;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NormalCompleteInput".
 */
export interface NormalCompleteInput {
  /**
   * Default empty.
   */
  disclosed_limitations?: string[];
  expected_lineage_revision: Revision;
  final_report: string;
  lineage_id: TaskLineageId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "NormalCompletionRecord".
 */
export interface NormalCompletionRecord {
  agent_run_id: AgentRunId;
  at: UtcTimestamp;
  disclosed_limitations: string[];
  final_report: string;
  lineage_id: TaskLineageId;
  outcome: NormalCompletionOutcome;
  record_id: NormalCompletionRecordId;
  /**
   * Historical review binding; new completions need no review.
   */
  review_id?: CompletionReviewId | null;
  turn_task_id: TurnTaskId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "OpenTaskLineageSummary".
 */
export interface OpenTaskLineageSummary {
  last_active_at: UtcTimestamp;
  lineage_id: TaskLineageId;
  revision: Revision;
  root_preview: string;
  root_user_event_id: EventId;
  source_count: number;
  state: TaskLineageState;
}
/**
 * Whether it opened, and what the user is told either way.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "OpenedOutside".
 */
export interface OpenedOutside {
  completed: boolean;
  message: string;
}
/**
 * Only an authenticated Core user-entry transaction may append this event.
 * Model continuation is a different type and cannot construct a user source.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "OriginalUserMessage".
 */
export interface OriginalUserMessage {
  at: UtcTimestamp;
  content_hash: ContentHash;
  event_id: EventId;
  origin: UserMessageOrigin;
  session_id: SessionId;
  text: string;
}
/**
 * Revoke trust or uninstall. The revision is the one the user saw in the impact view.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageChangeRequestV1".
 */
export interface PackageChangeRequestV1 {
  expected_package_revision: Revision;
  package_id: ToolPackageId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageConformanceCancelRequestV1".
 */
export interface PackageConformanceCancelRequestV1 {
  run_id: ToolConformanceRunId;
  version: PackageVersionRefV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageVersionRefV1".
 */
export interface PackageVersionRefV1 {
  package_fingerprint: CanonicalSha256HashV1;
  package_id: ToolPackageId;
  package_version: CanonicalSemVer;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageConformanceCaseResultV1".
 */
export interface PackageConformanceCaseResultV1 {
  case_name: ToolPackageConformanceCaseNameV1;
  execution_location: PackageExecutionLocationV1;
  reason: PackageConformanceReasonV1;
  runtime_evidence_hash: CanonicalSha256HashV1;
  tool_key: ToolKey;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageConformancePreparationV1".
 */
export interface PackageConformancePreparationV1 {
  preparation_id: string;
  preview: PackageImportPreviewV1;
  preview_hash: CanonicalSha256HashV17;
}
/**
 * Binds the user's approval to the actual run directory and settings.
 */
export interface CanonicalSha256HashV17 {
  algorithm: 'sha256';
  canonicalization_version: 'rfc8785_jcs_v1';
  hex: string;
}
/**
 * Prepares one run: the payload is restored, settings are checked, and the
 * concrete preview the user approves is returned. Nothing runs yet.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageConformancePrepareRequestV1".
 */
export interface PackageConformancePrepareRequestV1 {
  settings?: PackageToolSettingsV1[];
  version: PackageVersionBindingV1;
}
/**
 * One tool's settings. A tool without an instance-configuration schema has no
 * configuration, and every user-bound endpoint slot it declares has exactly one address.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageToolSettingsV1".
 */
export interface PackageToolSettingsV1 {
  configuration?: any;
  endpoints?: PackageEndpointBindingV1[];
  tool_key: ToolKey;
}
/**
 * The service address the user supplied for one `user_bound_endpoint` slot.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageEndpointBindingV1".
 */
export interface PackageEndpointBindingV1 {
  /**
   * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
   * via the `definition` "HttpBasePathV1".
   */
  base_path: HttpBasePathV1;
  endpoint_slot_key: HttpEndpointSlotKeyV1;
  origin: FixedHttpsOriginV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageConformanceReportV1".
 */
export interface PackageConformanceReportV1 {
  cases: PackageConformanceCaseResultV1[];
  cleanup_verified: boolean;
  completed_at: UtcTimestamp;
  run_id: ToolConformanceRunId;
  settings_hash: CanonicalSha256HashV18;
  started_at: UtcTimestamp;
  version: PackageVersionBindingV1;
}
/**
 * The exact settings every case ran with; install binds this through the report hash.
 */
export interface CanonicalSha256HashV18 {
  algorithm: 'sha256';
  canonicalization_version: 'rfc8785_jcs_v1';
  hex: string;
}
/**
 * The user's explicit approval to run the prepared package code once.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageConformanceStartRequestV1".
 */
export interface PackageConformanceStartRequestV1 {
  preparation_id: string;
  preview_hash: CanonicalSha256HashV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageConformanceViewV1".
 */
export interface PackageConformanceViewV1 {
  report?: PackageConformanceReportV1 | null;
  /**
   * The value an install request must repeat. Present only for a finished run.
   */
  report_hash?: CanonicalSha256HashV1 | null;
  run_id?: ToolConformanceRunId | null;
  started_at?: UtcTimestamp | null;
  state: PackageConformanceStateV1;
  version: PackageVersionBindingV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageCredentialBindRequestV1".
 */
export interface PackageCredentialBindRequestV1 {
  credential_id: CredentialId;
  /**
   * Secret-generation revision for one stable CredentialId.
   *
   * This is intentionally distinct from a row revision. Clear and metadata-only
   * mutations may change row concurrency without creating a new secret
   * generation.
   */
  credential_revision: string;
  /**
   * `None` when the slot has no binding yet.
   */
  expected_binding_revision?: Revision | null;
  package_id: ToolPackageId;
  slot_key: CredentialSlotKey;
  tool_key: ToolKey;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageCredentialBindingV1".
 */
export interface PackageCredentialBindingV1 {
  bound_at: UtcTimestamp;
  credential_id: CredentialId;
  /**
   * Secret-generation revision for one stable CredentialId.
   *
   * This is intentionally distinct from a row revision. Clear and metadata-only
   * mutations may change row concurrency without creating a new secret
   * generation.
   */
  credential_revision: string;
  revision: Revision;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageCredentialClearRequestV1".
 */
export interface PackageCredentialClearRequestV1 {
  expected_binding_revision: Revision;
  package_id: ToolPackageId;
  slot_key: CredentialSlotKey;
  tool_key: ToolKey;
}
/**
 * One HTTP tool credential slot. The target is what `tools.credential.*` accepts for
 * this tool; the binding is the user's explicit choice of credential.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageCredentialSlotViewV1".
 */
export interface PackageCredentialSlotViewV1 {
  binding?: PackageCredentialBindingV1 | null;
  /**
   * The exact origin a bound credential's allowed websites must contain.
   */
  origin: string;
  placement: ToolCredentialPlacementV1;
  slot_key: CredentialSlotKey;
  target: ToolCredentialManagementTargetV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCredentialManagementTargetV1".
 */
export interface ToolCredentialManagementTargetV1 {
  credential_slot_key: CredentialSlotKey;
  expected_instance_revision: Revision;
  placement: ToolCredentialPlacementV1;
  session_id?: SessionId | null;
  tool_instance_id: ToolInstanceId;
  workspace_id?: WorkspaceId | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageEnableRequestV1".
 */
export interface PackageEnableRequestV1 {
  enabled: boolean;
  expected_package_revision: Revision;
  package_id: ToolPackageId;
}
/**
 * One imported version as stored, with its last Conformance run.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageVersionViewV1".
 */
export interface PackageVersionViewV1 {
  conformance: PackageConformanceViewV1;
  package_revision: Revision;
  preview: PackageImportPreviewV1;
  /**
   * The settings the last run used, or the installed settings; empty when none.
   */
  settings: PackageToolSettingsV1[];
}
/**
 * Core-surface import request. The path comes from Main's own native dialog and
 * is never accepted from the renderer.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageImportSourceV1".
 */
export interface PackageImportSourceV1 {
  path: string;
}
/**
 * The user's explicit install approval: exact version, exact successful report and
 * the package revision the user saw.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageInstallRequestV1".
 */
export interface PackageInstallRequestV1 {
  expected_package_revision: Revision;
  report_hash: CanonicalSha256HashV1;
  version: PackageVersionBindingV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageInstalledV1".
 */
export interface PackageInstalledV1 {
  enabled: boolean;
  tools: PackageToolSummaryV1[];
  trusted: boolean;
  version: PackageVersionRefV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageToolSummaryV1".
 */
export interface PackageToolSummaryV1 {
  canonical_name: ToolCanonicalName;
  credential?: PackageCredentialSlotViewV1 | null;
  display_name: string;
  execution_location: PackageExecutionLocationV1;
  tool_id: ToolId;
  tool_instance_id: ToolInstanceId;
  tool_key: ToolKey;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageListV1".
 */
export interface PackageListV1 {
  packages: PackageSummaryV1[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageSummaryV1".
 */
export interface PackageSummaryV1 {
  display_name: string;
  installed?: PackageInstalledV1 | null;
  package_id: ToolPackageId;
  package_key: PackageKey;
  revision: Revision;
  /**
   * Newest import first. Content of other versions stays until it is collected.
   */
  versions: PackageVersionSummaryV1[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PackageVersionSummaryV1".
 */
export interface PackageVersionSummaryV1 {
  conformance: PackageConformanceStateV1;
  imported_at: UtcTimestamp;
  version: PackageVersionBindingV1;
}
/**
 * Untrust, uninstall or discard a pending update.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PluginChangeRequest".
 */
export interface PluginChangeRequest {
  expected_revision: number;
  name: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PluginCommandItem".
 */
export interface PluginCommandItem {
  argument_hint?: string | null;
  description?: string | null;
  name: string;
  /**
   * Why it is not offered.
   */
  problem?: string | null;
  title?: string | null;
  warnings: string[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PluginCounts".
 */
export interface PluginCounts {
  commands: number;
  hooks: number;
  servers: number;
  skills: number;
}
/**
 * Main opened an installed plugin's folder in the file manager, or says why it did not.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PluginDirectoryOpened".
 */
export interface PluginDirectoryOpened {
  completed: boolean;
  message: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PluginFile".
 */
export interface PluginFile {
  bytes: number;
  /**
   * Relative to the plugin folder, with `/` separators.
   */
  path: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PluginHookItem".
 */
export interface PluginHookItem {
  definition?: HookDefinition | null;
  /**
   * The Hook package folder, relative to the plugin's `hooks`.
   */
  folder: string;
  problems: string[];
}
/**
 * Copies a plugin folder (or unpacks a ZIP file) into Aura's own. A plugin with a name that is
 * already installed becomes that plugin's pending update.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PluginImportRequest".
 */
export interface PluginImportRequest {
  expected_revision: number;
  path: string;
}
/**
 * What each process tree the plugin starts may use (an MCP server instance, one Hook call).
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PluginLimits".
 */
export interface PluginLimits {
  memory_mb: number;
  /**
   * Processes alive at once.
   */
  processes: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PluginListRequest".
 */
export interface PluginListRequest {}
/**
 * A newer copy of an installed plugin, imported and waiting to be trusted. The installed copy
 * keeps working until then.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PluginPendingView".
 */
export interface PluginPendingView {
  counts: PluginCounts;
  fingerprint?: string | null;
  problems: string[];
  version?: string | null;
}
/**
 * Everything a plugin brings, for review before it is trusted.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PluginPreview".
 */
export interface PluginPreview {
  commands: PluginCommandItem[];
  files: PluginFile[];
  /**
   * The fingerprint of the copy shown; trusting names it.
   */
  fingerprint?: string | null;
  hooks: PluginHookItem[];
  /**
   * The update waiting to be trusted, rather than the installed copy.
   */
  pending: boolean;
  plugin: PluginView;
  servers: PluginServerItem[];
  skills: PluginSkillItem[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PluginView".
 */
export interface PluginView {
  author?: string | null;
  bytes: number;
  counts: PluginCounts;
  description?: string | null;
  enabled: boolean;
  file_count: number;
  fingerprint?: string | null;
  homepage?: string | null;
  layout?: PluginLayout | null;
  limits: PluginLimits;
  name: string;
  /**
   * The installed copy, in Aura's plugin folder.
   */
  path: string;
  pending_update?: PluginPendingView | null;
  problems: string[];
  state: PluginState;
  trusted_fingerprint?: string | null;
  /**
   * Parts of the plugin Aura does not use, each with the reason.
   */
  unsupported: string[];
  version?: string | null;
  warnings: string[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PluginServerItem".
 */
export interface PluginServerItem {
  arguments: string[];
  display_name: string;
  /**
   * The MCP server it runs as; its tools are `mcp.<id>.<tool>`.
   */
  id: string;
  kind: PluginServerKind;
  /**
   * The name inside the plugin.
   */
  name: string;
  problems: string[];
  /**
   * Environment variables whose values you enter in Aura.
   */
  secret_names: string[];
  sign_in?: PluginSignIn | null;
  /**
   * The program (with the plugin folder filled in) or the address.
   */
  target: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PluginSkillItem".
 */
export interface PluginSkillItem {
  description?: string | null;
  name?: string | null;
  problems: string[];
  /**
   * The Skill folder, relative to the plugin's `skills`.
   */
  relative: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PluginRegistryView".
 */
export interface PluginRegistryView {
  /**
   * Why nothing any plugin brings is offered (`plugins.json` cannot be used).
   */
  issue?: string | null;
  /**
   * Aura's plugin folder.
   */
  library: string;
  plugins: PluginView[];
  revision: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PluginRequest".
 */
export interface PluginRequest {
  name: string;
  /**
   * The update waiting to be trusted, rather than the installed copy.
   */
  pending?: boolean;
}
/**
 * Main's own file dialog for importing a plugin packed as a ZIP file.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PluginSelectArchiveRequest".
 */
export interface PluginSelectArchiveRequest {}
/**
 * Main's own folder dialog for importing a plugin.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PluginSelectDirectoryRequest".
 */
export interface PluginSelectDirectoryRequest {}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PluginSelectedArchive".
 */
export interface PluginSelectedArchive {
  /**
   * Absent when the user cancelled.
   */
  path?: string | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PluginSelectedDirectory".
 */
export interface PluginSelectedDirectory {
  /**
   * Absent when the user cancelled.
   */
  path?: string | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PluginSetEnabledRequest".
 */
export interface PluginSetEnabledRequest {
  enabled: boolean;
  expected_revision: number;
  name: string;
}
/**
 * Trusts exactly the content that was shown (its fingerprint); for a pending update, this also
 * replaces the installed copy.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PluginTrustRequest".
 */
export interface PluginTrustRequest {
  expected_fingerprint: string;
  expected_revision: number;
  name: string;
  pending?: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "PrepareRequest".
 */
export interface PrepareRequest {
  operation: SecuredOperation;
  requirement: BackendRequirement;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RecoveryNotice".
 */
export interface RecoveryNotice {
  /**
   * The configuration domains whose fault is being reported, by key.
   */
  affected_domains: string[];
  /**
   * The actions the user may take, by key.
   */
  available_actions: string[];
  /**
   * Stable codes for each fault. What the user's report will quote.
   */
  fault_codes: string[];
  /**
   * A bounded next step for the user.
   */
  next_step: string;
  /**
   * A recovery is still owned by this runtime; a state read is not its receipt.
   */
  recovery_in_progress: boolean;
  /**
   * Whether the runtime can say which domains are affected.
   *
   * `false` is the honest "unknown scope": the user is told the runtime cannot
   * narrow it, rather than being shown a misleadingly precise list.
   */
  scope_known: boolean;
  /**
   * New calls that are suspended, described in bounded terms.
   */
  suspended_calls: string[];
  /**
   * The capabilities that are unavailable because of those faults, by key.
   */
  unavailable_capabilities: string[];
}
/**
 * What one recovery action really did.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RecoveryOutcome".
 */
export interface RecoveryOutcome {
  action: ConfigRecoveryAction;
  applied: boolean;
  module: ConfigModule;
  /**
   * A stable code when the action did not apply.
   */
  reason_code?: string | null;
  /**
   * The new revision the domain now serves, when the action applied.
   */
  revision?: number | null;
  /**
   * The source the recovered snapshot came from.
   */
  source?: ConfigSource | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RecoveryReport".
 */
export interface RecoveryReport {
  action: ConfigRecoveryAction;
  outcomes: RecoveryOutcome[];
  /**
   * A stable code when the attempt did not fully apply.
   */
  reason_code?: string | null;
  /**
   * The capabilities whose service is **really present** after this attempt.
   */
  restored_capabilities: string[];
  /**
   * The capabilities still unavailable afterwards, each with its stable reason.
   *
   * The previous report had nowhere to put this, so a
   * capability whose construction genuinely failed was indistinguishable from one that came
   * back. The user saw a cleared fault marker and concluded the feature worked.
   */
  unavailable_capabilities?: UnavailableCapability[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "UnavailableCapability".
 */
export interface UnavailableCapability {
  /**
   * The capability, by stable key.
   */
  capability: string;
  /**
   * Why it is still unavailable, in bounded, path-free terms.
   */
  reason: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RevokeMemorySessionConsentInput".
 */
export interface RevokeMemorySessionConsentInput {
  expected_revision: Revision;
  grant_id: PermissionGrantId;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RevokeNetworkSessionConsentInput".
 */
export interface RevokeNetworkSessionConsentInput {
  expected_revision: Revision;
  grant_id: PermissionGrantId;
  session_id: SessionId;
}
/**
 * Appended after the immutable direct-execution result. Bytes are deltas, and
 * sequence numbers prevent duplicate or reordered output from replacing facts.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RunnerBackgroundUpdate".
 */
export interface RunnerBackgroundUpdate {
  attempt: AttemptRef;
  observed_at: UtcTimestamp;
  output_truncated: boolean;
  sequence: number;
  state: BackgroundState;
  stderr: number[];
  stdout: number[];
}
/**
 * Ordered direct-execution deltas, retained even if no terminal frame arrives.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RunnerProgress".
 */
export interface RunnerProgress {
  attempt: AttemptRef;
  observed_at: UtcTimestamp;
  output_truncated: boolean;
  sequence: number;
  stderr: number[];
  stdout: number[];
  submitted_script?: string | null;
}
/**
 * Raw execution facts, not an Agent result or an Artifact receipt. Core owns
 * publication. Generic script side effects are unassessed, not execution loss.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RunnerObservation".
 */
export interface RunnerObservation {
  account_sid: string;
  attempt: AttemptRef;
  binding_fingerprint: CanonicalSha256HashV1;
  exit_code?: number | null;
  finished_at: UtcTimestamp;
  output_truncated?: boolean;
  process_id: number;
  process_tree_stopped: boolean;
  shell_image: ExecutionFileBinding;
  started_at: UtcTimestamp;
  stderr: number[];
  stdout: number[];
  stop_reason: RunnerStopReason;
  /**
   * Exact executed contents, including Aura's fixed Shell bootstrap.
   */
  submitted_script?: string;
}
/**
 * Core integrity, service availability and optional execution facilities are
 * separate facts. Backend readiness does not grant permission to dispatch.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "RuntimeReadiness".
 */
export interface RuntimeReadiness {
  backends: BackendStatus[];
  capabilities: CapabilityStatus[];
  core_health: CoreHealth;
  extensions: ExtensionStatus[];
  revision: Revision;
  schema_version: ToolWireVersionV1;
}
/**
 * Authenticated user command. Core allocates identity and uses the configured
 * default permission mode; a model tool cannot call this route.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SessionCreateRequest".
 */
export interface SessionCreateRequest {
  title: string;
  workspace_id: WorkspaceId;
}
/**
 * A deleted conversation. Its conversation, process and audit records are gone; its
 * Workspace's files, `.memories` and global memory are untouched.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SessionDeleteResult".
 */
export interface SessionDeleteResult {
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SessionDynamicTools".
 */
export interface SessionDynamicTools {
  revision: Revision;
  session_id: SessionId;
  tool_ids: ToolId[];
}
/**
 * Archive, restore or delete one conversation. An authenticated user command; a model
 * tool cannot call it. Archiving and deleting are refused while the conversation has a task
 * that has not finished.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SessionLifecycleRequest".
 */
export interface SessionLifecycleRequest {
  session_id: SessionId;
}
/**
 * Rename an existing conversation without overwriting a concurrent rename.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SessionRenameRequest".
 */
export interface SessionRenameRequest {
  expected_title: string;
  session_id: SessionId;
  title: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ShellExecuteInput".
 */
export interface ShellExecuteInput {
  /**
   * Default 65536. Bytes of stdout and stderr kept, up to 1048576.
   */
  max_output_bytes?: number;
  /**
   * Default false. Set true when the script needs the network; read-only and
   * workspace modes then ask for network approval. It declares a need and never
   * grants access by itself.
   */
  requires_network?: boolean;
  /**
   * The script to run.
   */
  script: string;
  /**
   * Text for standard input. Default empty.
   */
  stdin?: string;
  /**
   * Default 120000. After this many milliseconds Aura stops the whole process tree.
   * The tool time limit in Tool settings (default 10 minutes, at most 30 minutes) also
   * ends the call, so a value above that limit does not help. At most 86400000.
   */
  timeout_ms?: number;
  /**
   * Default: the workspace root. A path relative to it.
   */
  working_directory?: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ShellExecutionView".
 */
export interface ShellExecutionView {
  background?: BackgroundState | null;
  command: ManagedShellCommand;
  diagnostic?: string | null;
  environment_version: string;
  exit_code?: number | null;
  /**
   * Names only, from the launcher's actual filtered environment.
   */
  filtered_environment_names?: string[];
  finished_at?: UtcTimestamp | null;
  original_input: ShellExecuteInput;
  output_truncated: boolean;
  process_id?: number | null;
  shell_image?: ExecutionFileBinding | null;
  started_at?: UtcTimestamp | null;
  state: ShellCallState;
  stderr: number[];
  stderr_next?: number | null;
  stderr_total: number;
  stdout: number[];
  stdout_next?: number | null;
  stdout_total: number;
  stop_reason?: RunnerStopReason | null;
  submitted_script?: string | null;
  tool_call_id: ToolCallId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ShellQueryInput".
 */
export interface ShellQueryInput {
  /**
   * Bytes per page. Default 65536.
   */
  max_bytes?: number;
  /**
   * Byte offset into stderr. Default 0.
   */
  stderr_offset?: number;
  /**
   * Byte offset into stdout. Default 0.
   */
  stdout_offset?: number;
  tool_call_id: ToolCallId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ShellQueryRequest".
 */
export interface ShellQueryRequest {
  input: ShellQueryInput;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ShellStopInput".
 */
export interface ShellStopInput {
  tool_call_id: ToolCallId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ShellStopRequest".
 */
export interface ShellStopRequest {
  input: ShellStopInput;
  session_id: SessionId;
}
/**
 * Untrust a Skill, or remove one from Aura's own folder.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SkillChangeRequest".
 */
export interface SkillChangeRequest {
  expected_revision: number;
  id: SkillInstanceId;
  workspace_id?: WorkspaceId | null;
}
/**
 * Main opened a listed Skill's folder in the file manager, or says why it did not.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SkillDirectoryOpened".
 */
export interface SkillDirectoryOpened {
  completed: boolean;
  message: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SkillFile".
 */
export interface SkillFile {
  bytes: number;
  /**
   * Relative to the Skill folder, with `/` separators.
   */
  path: string;
}
/**
 * Copy a Skill folder, or unpack a ZIP file, the user chose into Aura's own folder; the
 * original is not changed.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SkillImportRequest".
 */
export interface SkillImportRequest {
  expected_revision: number;
  path: string;
}
/**
 * List the Skills; a workspace adds its own `.agents\skills`.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SkillListRequest".
 */
export interface SkillListRequest {
  workspace_id?: WorkspaceId | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SkillPreview".
 */
export interface SkillPreview {
  files: SkillFile[];
  skill: SkillView;
  /**
   * `SKILL.md` as written.
   */
  skill_md: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SkillView".
 */
export interface SkillView {
  /**
   * What the Skill says it needs. Shown only; Aura never loads or allows a tool because a
   * Skill names it.
   */
  allowed_tools: string[];
  bytes: number;
  compatibility?: string | null;
  description?: string | null;
  enabled: boolean;
  file_count: number;
  /**
   * What trust would record now; absent when the folder cannot be read.
   */
  fingerprint?: string | null;
  id: SkillInstanceId;
  license?: string | null;
  /**
   * Files `SKILL.md` links to that are not in the folder (the Skill still works).
   */
  missing_references: string[];
  name?: string | null;
  /**
   * The folder on this computer.
   */
  path: string;
  /**
   * The plugin that brings it, for a plugin's Skill.
   */
  plugin?: string | null;
  problems: string[];
  relative: string;
  source: SkillSource;
  state: SkillState;
  trusted_fingerprint?: string | null;
  warnings: string[];
  workspace_id?: WorkspaceId | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SkillRegistryView".
 */
export interface SkillRegistryView {
  /**
   * Why saved choices cannot be used now (then nothing is offered to the model).
   */
  issue?: string | null;
  revision: number;
  roots: SkillRootView[];
  skills: SkillView[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SkillRootView".
 */
export interface SkillRootView {
  exists: boolean;
  path: string;
  source: SkillSource;
}
/**
 * One Skill, for its preview or its folder.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SkillRequest".
 */
export interface SkillRequest {
  id: SkillInstanceId;
  workspace_id?: WorkspaceId | null;
}
/**
 * Main's own file dialog for importing a Skill packed as a ZIP file (`.zip`, `.skill`).
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SkillSelectArchiveRequest".
 */
export interface SkillSelectArchiveRequest {}
/**
 * Main's own folder dialog for importing a Skill.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SkillSelectDirectoryRequest".
 */
export interface SkillSelectDirectoryRequest {}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SkillSelectedArchive".
 */
export interface SkillSelectedArchive {
  /**
   * Absent when the user cancelled.
   */
  path?: string | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SkillSelectedDirectory".
 */
export interface SkillSelectedDirectory {
  /**
   * Absent when the user cancelled.
   */
  path?: string | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SkillSetEnabledRequest".
 */
export interface SkillSetEnabledRequest {
  enabled: boolean;
  expected_revision: number;
  id: SkillInstanceId;
  workspace_id?: WorkspaceId | null;
}
/**
 * The saved choices for one Skill in `skills.json`. Nothing else about a Skill is stored.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SkillSetting".
 */
export interface SkillSetting {
  /**
   * Saved for multi-agent; no sub-agent exists yet.
   */
  available_for_subagents?: boolean;
  enabled?: boolean;
  id: SkillInstanceId;
  /**
   * The folder relative to its source's root, `/`-separated.
   */
  relative: string;
  source: SkillSource;
  trusted_fingerprint?: string | null;
  /**
   * The workspace a workspace Skill belongs to.
   */
  workspace_id?: WorkspaceId | null;
}
/**
 * Trust exactly the shown content and switch the Skill on.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SkillTrustRequest".
 */
export interface SkillTrustRequest {
  expected_fingerprint: string;
  expected_revision: number;
  id: SkillInstanceId;
  workspace_id?: WorkspaceId | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SourceReadReceipt".
 */
export interface SourceReadReceipt {
  agent_run_id: AgentRunId;
  created_at: UtcTimestamp;
  lineage_id: TaskLineageId;
  lineage_revision: Revision;
  model_step_id: ModelStepId;
  receipt_id: SourceReadReceiptId;
  session_id: SessionId;
  source_event_hashes: [any, any][];
  turn_task_id: TurnTaskId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "SourceRecheckFacts".
 */
export interface SourceRecheckFacts {
  compacted: boolean;
  crossed_turn: boolean;
  resumed: boolean;
  source_not_in_context: boolean;
  used_child_agent: boolean;
  used_query: boolean;
  used_steer: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TaskLineage".
 */
export interface TaskLineage {
  completed_at?: UtcTimestamp | null;
  completion_gate: CompletionGate;
  completion_policy: CompletionPolicy;
  created_at: UtcTimestamp;
  created_by_turn_task_id: TurnTaskId;
  derived_from_lineage_id?: TaskLineageId | null;
  last_active_turn_task_id: TurnTaskId;
  lineage_id: TaskLineageId;
  relations: TaskLineageRelation[];
  revision: Revision;
  root_user_event_id: EventId;
  session_id: SessionId;
  source_recheck: SourceRecheckFacts;
  sources: TaskLineageSource[];
  state: TaskLineageState;
  updated_at: UtcTimestamp;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TaskLineageRelation".
 */
export interface TaskLineageRelation {
  created_at: UtcTimestamp;
  created_by_turn_task_id: TurnTaskId;
  earlier_excerpt?: UserMessageTextRange | null;
  earlier_user_event_id?: EventId | null;
  kind: TaskLineageRelationKind;
  later_excerpt?: UserMessageTextRange | null;
  later_user_event_id: EventId;
  relation_id: TaskLineageRelationId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "UserMessageTextRange".
 */
export interface UserMessageTextRange {
  end_utf8_byte: number;
  start_utf8_byte: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TaskLineageSource".
 */
export interface TaskLineageSource {
  attached_at: UtcTimestamp;
  attached_by_turn_task_id: TurnTaskId;
  content_hash: ContentHash;
  immutable_preview: string;
  membership: SourceMembership;
  related_ranges: UserMessageTextRange[];
  source_sequence: number;
  user_event_id: EventId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TaskLineageListOpenResult".
 */
export interface TaskLineageListOpenResult {
  active_lineage_id?: TaskLineageId | null;
  lineages: OpenTaskLineageSummary[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TaskLineageReadSourcesInput".
 */
export interface TaskLineageReadSourcesInput {
  expected_lineage_revision: Revision;
  lineage_id: TaskLineageId;
  source_event_ids: EventId[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TaskLineageReadSourcesResult".
 */
export interface TaskLineageReadSourcesResult {
  completion_source_recheck_required: boolean;
  lineage_id: TaskLineageId;
  lineage_revision: Revision;
  messages: OriginalUserMessage[];
  source_read_receipt_id: SourceReadReceiptId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TaskLineageReconcileInput".
 */
export interface TaskLineageReconcileInput {
  active_lineage_id?: TaskLineageId | null;
  completion_policy_update?: CompletionPolicyUpdate | null;
  current_user_event_id: EventId;
  decision: TaskLineageReconcileDecision;
  expected_lineage_revision?: Revision | null;
  reason: string;
  relations: TaskLineageRelationInput[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TaskLineageRelationInput".
 */
export interface TaskLineageRelationInput {
  earlier_excerpt?: UserMessageExcerptSelector | null;
  earlier_user_event_id?: EventId | null;
  kind: TaskLineageRelationKind;
  later_excerpt?: UserMessageExcerptSelector | null;
  later_user_event_id: EventId;
}
/**
 * Model-facing reconcile arguments. Session, current user event, and lineage
 * revision are bound by Core from the live call.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TaskLineageReconcileToolInput".
 */
export interface TaskLineageReconcileToolInput {
  completion_policy_update?: CompletionPolicyUpdate | null;
  decision: TaskLineageReconcileToolDecision;
  reason: string;
  relations?: TaskLineageRelationInput[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TerminalControlRequestV1".
 */
export interface TerminalControlRequestV1 {
  controller: TerminalControllerV1;
  session_id: SessionId;
  terminal_id: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TerminalListV1".
 */
export interface TerminalListV1 {
  next_cursor?: string | null;
  terminals: TerminalViewV1[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TerminalViewV1".
 */
export interface TerminalViewV1 {
  controller: TerminalControllerV1;
  end?: TerminalEnd | null;
  owner: TerminalOwnerV1;
  process_id: number;
  revision: Revision;
  session_id: SessionId;
  shell: ManagedShell;
  started_at: UtcTimestamp;
  terminal_id: string;
  title: string;
  tool_call_id?: ToolCallId | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TerminalOpeningChangeV1".
 */
export interface TerminalOpeningChangeV1 {
  contract_version: ToolWireVersionV1;
  opening_id: string;
  phase: TerminalOpeningPhaseV1;
  runtime_instance_id: RuntimeInstanceId;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TerminalOpeningProgressV1".
 */
export interface TerminalOpeningProgressV1 {
  contract_version: ToolWireVersionV1;
  phase: TerminalOpeningPhaseV1;
  request_id: CorrelationId;
  runtime_instance_id: RuntimeInstanceId;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TerminalOpeningRefV1".
 */
export interface TerminalOpeningRefV1 {
  contract_version: ToolWireVersionV1;
  opening_id: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TerminalOpeningStartV1".
 */
export interface TerminalOpeningStartV1 {
  contract_version: ToolWireVersionV1;
  opening_id: string;
  request: UserTerminalOpenRequestV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "UserTerminalOpenRequestV1".
 */
export interface UserTerminalOpenRequestV1 {
  columns: number;
  rows: number;
  session_id: SessionId;
  shell: ManagedShell;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TerminalOutputBatchV1".
 */
export interface TerminalOutputBatchV1 {
  data_base64: string;
  end?: TerminalEnd | null;
  next_offset: number;
  runtime_instance_id: RuntimeInstanceId;
  session_id: SessionId;
  start_offset: number;
  terminal_id: string;
  truncated: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TerminalRequestV1".
 */
export interface TerminalRequestV1 {
  session_id: SessionId;
  terminal_id: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TerminalResizeRequestV1".
 */
export interface TerminalResizeRequestV1 {
  columns: number;
  rows: number;
  session_id: SessionId;
  terminal_id: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TerminalSensitiveChangedV1".
 */
export interface TerminalSensitiveChangedV1 {
  pending: TerminalSensitivePendingV1;
  runtime_instance_id: RuntimeInstanceId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TerminalSensitivePendingV1".
 */
export interface TerminalSensitivePendingV1 {
  requests: TerminalSensitiveRequestV1[];
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TerminalSensitiveRequestV1".
 */
export interface TerminalSensitiveRequestV1 {
  expires_at: UtcTimestamp;
  prompt: string;
  session_id: SessionId;
  terminal_id: string;
  tool_call_id: ToolCallId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TerminalSensitiveReplyV1".
 */
export interface TerminalSensitiveReplyV1 {
  session_id: SessionId;
  tool_call_id: ToolCallId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TerminalSensitiveResultV1".
 */
export interface TerminalSensitiveResultV1 {
  status: TerminalSensitiveStatusV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TerminalSessionRequestV1".
 */
export interface TerminalSessionRequestV1 {
  cursor?: string | null;
  session_id: SessionId;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TerminalStateChangedV1".
 */
export interface TerminalStateChangedV1 {
  runtime_instance_id: RuntimeInstanceId;
  view: TerminalViewV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TerminalSubscribeRequestV1".
 */
export interface TerminalSubscribeRequestV1 {
  offset: number;
  session_id: SessionId;
  terminal_id: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TodoItemDraft".
 */
export interface TodoItemDraft {
  /**
   * Default pending.
   */
  status?: 'pending' | 'in_progress' | 'completed' | 'blocked' | 'cancelled';
  title: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TodoReplaceInput".
 */
export interface TodoReplaceInput {
  expected_revision: Revision;
  items: TodoItemDraft[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "TodoUpdateInput".
 */
export interface TodoUpdateInput {
  expected_revision: Revision;
  item_id: TodoItemId;
  status: TodoStatus;
  title?: string | null;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolBrokerInvokeRequest".
 */
export interface ToolBrokerInvokeRequest {
  action: ToolBrokerAction;
  registry_snapshot_ref: ToolRegistrySnapshotRef;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolBrokerInvokeResult".
 */
export interface ToolBrokerInvokeResult {
  changed?: boolean | null;
  resulting_registry_snapshot_ref?: ToolRegistrySnapshotRef | null;
  value: any;
}
/**
 * Authority metadata for a Tool Credential. Secret bytes and WinCred
 * locators are deliberately held by the P02 storage binding, never by this
 * renderer-safe domain record.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCredentialAuthorityRecordV1".
 */
export interface ToolCredentialAuthorityRecordV1 {
  /**
   * User-configured HTTP origins. Empty historical records authorize no model-selected URL.
   */
  allowed_origins?: string[];
  authenticated_owner: PrincipalId;
  created_at: UtcTimestamp;
  credential_id: CredentialId;
  credential_kind: ToolCredentialKindV1;
  /**
   * Secret-generation revision for one stable CredentialId.
   *
   * This is intentionally distinct from a row revision. Clear and metadata-only
   * mutations may change row concurrency without creating a new secret
   * generation.
   */
  credential_revision: string;
  display_name: string;
  placement_policy: ToolCredentialPlacementPolicyV1;
  row_revision: Revision;
  scope: ToolCredentialScopeV1;
  state: ToolCredentialStateV1;
  updated_at: UtcTimestamp;
  usage_policy: ToolCredentialUsagePolicyV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCredentialPlacementPolicyV1".
 */
export interface ToolCredentialPlacementPolicyV1 {
  allowed_placements: ToolCredentialPlacementV1[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCredentialChangeRequestV1".
 */
export interface ToolCredentialChangeRequestV1 {
  credential_id: CredentialId;
  expected_row_revision: Revision;
  target: ToolCredentialManagementTargetV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCredentialCreateRequestV1".
 */
export interface ToolCredentialCreateRequestV1 {
  allowed_origins?: string[];
  display_name: string;
  scope: ToolCredentialScopeV1;
  target: ToolCredentialManagementTargetV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCredentialListResultV1".
 */
export interface ToolCredentialListResultV1 {
  credentials: ToolCredentialAuthorityRecordV1[];
  pending_credential_ids: CredentialId[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCredentialMutationResultV1".
 */
export interface ToolCredentialMutationResultV1 {
  /**
   * The authoritative change committed; deletion of an old WinCred value may still need retry.
   */
  cleanup_pending: boolean;
  credential?: ToolCredentialAuthorityRecordV1 | null;
  disposition: ToolCredentialMutationDispositionV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCredentialOriginsRequestV1".
 */
export interface ToolCredentialOriginsRequestV1 {
  allowed_origins: string[];
  credential_id: CredentialId;
  expected_row_revision: Revision;
  target: ToolCredentialManagementTargetV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCredentialOutcomeRequestV1".
 */
export interface ToolCredentialOutcomeRequestV1 {
  command_invocation_id: CommandInvocationId;
  target: ToolCredentialManagementTargetV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "ToolCredentialOutcomeV1".
 */
export interface ToolCredentialOutcomeV1 {
  command_invocation_id: CommandInvocationId;
  result?: ToolCredentialMutationResultV1 | null;
  state: ToolCredentialOutcomeStateV1;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "UiSettings".
 */
export interface UiSettings {
  default_terminal: DefaultTerminal;
  /**
   * The session's runtime readiness and tool execution record, for debugging (default off).
   */
  show_debug_information: boolean;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "UiSettingsSnapshot".
 */
export interface UiSettingsSnapshot {
  issue?: string | null;
  revision?: number | null;
  settings: UiSettings;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "UiSettingsUpdate".
 */
export interface UiSettingsUpdate {
  expected_revision: number;
  settings: UiSettings;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "UserTerminalWriteRequestV1".
 */
export interface UserTerminalWriteRequestV1 {
  input: string;
  interrupt: boolean;
  session_id: SessionId;
  terminal_id: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "V2KernelFixture".
 */
export interface V2KernelFixture {
  authority: AuthorizationDecision;
  cancellation: CancellationState;
  dispatch: DispatchRequest;
  grant: AuthorizationGrant;
  result: ExecutionResult;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WebDocument".
 */
export interface WebDocument {
  links: WebSearchHit[];
  media_type: string;
  rendered: boolean;
  response_hash: ContentHash;
  retrieved_at: UtcTimestamp;
  source_url: string;
  text: string;
  title: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WebSearchHit".
 */
export interface WebSearchHit {
  /**
   * Search pages without a reliable publication date keep this absent.
   */
  published_at?: string | null;
  snippet: string;
  source: string;
  title: string;
  url: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WebFetchInput".
 */
export interface WebFetchInput {
  max_bytes?: number;
  mode?: 'auto' | 'http_only' | 'headless_render_required';
  url: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WebFetchResult".
 */
export interface WebFetchResult {
  document_artifact_id: ArtifactId;
  excerpt: string;
  rendered: boolean;
  source_url: string;
  title: string;
  total_chars: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WebReadInput".
 */
export interface WebReadInput {
  document_artifact_id: ArtifactId;
  max_chars?: number;
  offset_chars?: number;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WebReadResult".
 */
export interface WebReadResult {
  document_artifact_id: ArtifactId;
  next_offset_chars?: number | null;
  source_url: string;
  text: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WebSearchInput".
 */
export interface WebSearchInput {
  max_results?: number;
  query: string;
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WebSearchResult".
 */
export interface WebSearchResult {
  evidence_artifact_id: ArtifactId;
  hits: WebSearchHit[];
  live: boolean;
  query: string;
  retrieved_at: UtcTimestamp;
  surface: string;
}
/**
 * Prepared by the Core-side backend, included in the immutable Plan hash.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WindowsUserPreparation".
 */
export interface WindowsUserPreparation {
  account_sid: string;
  assurance_version: string;
  installation_fingerprint: CanonicalSha256HashV1;
  mode: PermissionMode;
  permission_revision: number;
  read_roots: ExecutionFileBinding[];
  schema_version: ToolWireVersionV1;
  shell_environment_version: string;
  shell_image: ExecutionFileBinding;
  valid_until: UtcTimestamp;
  working_directory: ExecutionFileBinding;
  write_roots: ExecutionFileBinding[];
}
/**
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceFileOpenRequest".
 */
export interface WorkspaceFileOpenRequest {
  opener: WorkspaceFileOpener;
  /**
   * Workspace-relative, `/` between its parts.
   */
  path: string;
  session_id: SessionId;
  workspace_id: WorkspaceId;
}
/**
 * Main owns the native picker. Renderer cannot supply paths or dialog options.
 *
 * This interface was referenced by `AuraContractCatalog`'s JSON-Schema
 * via the `definition` "WorkspaceSelectDirectoryRequest".
 */
export interface WorkspaceSelectDirectoryRequest {}
