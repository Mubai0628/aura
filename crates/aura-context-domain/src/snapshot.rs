use aura_domain_common::CanonicalSemVer;
use aura_domain_common::{
    AgentRunId, ArtifactId, ContentHash, ContextProjectionSnapshotId, ModelProfileId, ModelStepId,
    PromptBundleSnapshotId, Revision, SessionId, SessionSequence, TaskLineageId, TurnTaskId,
    UtcTimestamp, canonical_json_v1_sha256,
};
use aura_model_domain::{LogicalConversationItem, ModelAttachmentSnapshot, ModelOutputRequest};
use aura_security_contract::ModelEgressPolicyVersion;
use aura_tool_contract::{EffectiveToolBindingSnapshot, ToolSchemaSetSnapshotV1};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    ContextBudgetSnapshot, ContextCompressibility, ContextPrivacyClass, ContextSourceRef,
    ContextUsageSummaryView,
};

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum PromptSectionClass {
    Core,
    Permission,
    ToolSummary,
    WorkspaceAgents,
    HistoricalSnapshot,
    AgentMode,
    Hook,
    TaskState,
    Compaction,
    Maintenance,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PromptActivationCondition {
    Always,
    AgentMode { mode: PromptAgentMode },
    Capability { capability: String },
    ChildAgent,
    Recovery { recovery_kind: String },
    Compaction,
    Maintenance { maintenance_kind: String },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptAgentMode {
    Normal,
    Plan,
    Goal,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromptModuleSnapshot {
    pub module_id: String,
    pub module_version: CanonicalSemVer,
    pub section: PromptSectionClass,
    pub activation: PromptActivationCondition,
    pub order: u32,
    pub content_hash: ContentHash,
    pub content_artifact_id: ArtifactId,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromptBundleSnapshot {
    pub id: PromptBundleSnapshotId,
    pub turn_task_id: TurnTaskId,
    pub bundle_version: CanonicalSemVer,
    pub serializer_version: u32,
    pub module_snapshots: Vec<PromptModuleSnapshot>,
    pub custom_addendum_artifact_id: Option<ArtifactId>,
    pub custom_addendum_hash: Option<ContentHash>,
    pub settings_revision: Revision,
    pub bundle_manifest_hash: ContentHash,
    pub release_identity: String,
    pub created_at: UtcTimestamp,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum PromptBundleSnapshotError {
    #[error("prompt_bundle.snapshot.metadata_invalid")]
    MetadataInvalid,
    #[error("prompt_bundle.snapshot.module_order_invalid")]
    ModuleOrderInvalid,
    #[error("prompt_bundle.snapshot.addendum_pair_invalid")]
    AddendumPairInvalid,
}

impl PromptBundleSnapshot {
    pub fn validate_contract_shape(&self) -> Result<(), PromptBundleSnapshotError> {
        if self.serializer_version == 0
            || self.release_identity.trim().is_empty()
            || self.module_snapshots.is_empty()
        {
            return Err(PromptBundleSnapshotError::MetadataInvalid);
        }
        if self.custom_addendum_artifact_id.is_some() != self.custom_addendum_hash.is_some() {
            return Err(PromptBundleSnapshotError::AddendumPairInvalid);
        }
        if !self.module_snapshots.windows(2).all(|pair| {
            prompt_section_order(pair[0].section)
                .cmp(&prompt_section_order(pair[1].section))
                .then_with(|| pair[0].order.cmp(&pair[1].order))
                .then_with(|| pair[0].module_id.cmp(&pair[1].module_id))
                .is_lt()
        }) || self.module_snapshots.iter().any(|module| {
            module.module_id.trim().is_empty() || module.module_version.as_str().trim().is_empty()
        }) {
            return Err(PromptBundleSnapshotError::ModuleOrderInvalid);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ContextSnapshotError {
    #[error("context.snapshot.section_order_invalid")]
    SectionOrderInvalid,
    #[error("context.snapshot.budget_invalid")]
    BudgetInvalid,
    #[error("context.snapshot.tool_order_invalid")]
    ToolOrderInvalid,
    #[error("context.snapshot.snapshot_hash_mismatch")]
    SnapshotHashMismatch,
    #[error("context.snapshot.canonical_serialization_failed")]
    CanonicalSerialization,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ContextSectionKind {
    CoreSystemPrompt,
    PermissionAndSecurity,
    LoadedToolSchemas,
    WorkspaceAgentsInstructions,
    HistoricalSnapshotSummary,
    AgentModePrompt,
    HookInjections,
    CurrentTaskState,
    RecentHistory,
    FileReferencesAndPendingAttachments,
    CurrentTurnWorkingSet,
    SteeredUserAdditions,
}

impl ContextSectionKind {
    pub const PHYSICAL_ORDER: [Self; 12] = [
        Self::CoreSystemPrompt,
        Self::PermissionAndSecurity,
        Self::LoadedToolSchemas,
        Self::WorkspaceAgentsInstructions,
        Self::HistoricalSnapshotSummary,
        Self::AgentModePrompt,
        Self::HookInjections,
        Self::CurrentTaskState,
        Self::RecentHistory,
        Self::FileReferencesAndPendingAttachments,
        Self::CurrentTurnWorkingSet,
        Self::SteeredUserAdditions,
    ];

    #[must_use]
    pub const fn physical_ordinal(self) -> u8 {
        match self {
            Self::CoreSystemPrompt => 1,
            Self::PermissionAndSecurity => 2,
            Self::LoadedToolSchemas => 3,
            Self::WorkspaceAgentsInstructions => 4,
            Self::HistoricalSnapshotSummary => 5,
            Self::AgentModePrompt => 6,
            Self::HookInjections => 7,
            Self::CurrentTaskState => 8,
            Self::RecentHistory => 9,
            Self::FileReferencesAndPendingAttachments => 10,
            Self::CurrentTurnWorkingSet => 11,
            Self::SteeredUserAdditions => 12,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextSectionSnapshot {
    pub kind: ContextSectionKind,
    pub physical_ordinal: u8,
    pub content_artifact_id: ArtifactId,
    pub content_hash: ContentHash,
    pub character_count: u64,
    pub byte_count: u64,
    pub estimated_tokens: u64,
    pub source_refs: Vec<ContextSourceRef>,
    pub compressibility: ContextCompressibility,
    pub privacy_class: ContextPrivacyClass,
    pub is_empty_marker: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextProjectionSnapshot {
    pub snapshot_version: u32,
    pub id: ContextProjectionSnapshotId,
    pub session_id: SessionId,
    pub turn_task_id: TurnTaskId,
    pub agent_run_id: AgentRunId,
    pub model_step_id: ModelStepId,
    pub prompt_bundle_snapshot_id: PromptBundleSnapshotId,
    pub active_prompt_module_ids: Vec<String>,
    pub model_profile_id: ModelProfileId,
    pub budget: ContextBudgetSnapshot,
    pub sections: Vec<ContextSectionSnapshot>,
    pub tool_schema_set: ToolSchemaSetSnapshotV1,
    pub effective_tool_bindings: Vec<EffectiveToolBindingSnapshot>,
    pub conversation_items: Vec<LogicalConversationItem>,
    pub attachments: Vec<ModelAttachmentSnapshot>,
    pub active_task_lineage_id: Option<TaskLineageId>,
    pub instruction_envelope_artifact_id: ArtifactId,
    pub instruction_envelope_hash: ContentHash,
    pub model_egress_policy_version: ModelEgressPolicyVersion,
    pub model_egress_manifest_artifact_id: ArtifactId,
    pub model_egress_manifest_hash: ContentHash,
    pub output_request: ModelOutputRequest,
    pub history_cutoff: SessionSequence,
    pub context_usage: ContextUsageSummaryView,
    pub snapshot_hash: ContentHash,
    pub created_at: UtcTimestamp,
}

impl ContextProjectionSnapshot {
    #[must_use]
    pub fn has_canonical_section_order(&self) -> bool {
        self.sections.len() == ContextSectionKind::PHYSICAL_ORDER.len()
            && self.sections.iter().zip(ContextSectionKind::PHYSICAL_ORDER).all(
                |(section, expected)| {
                    section.kind == expected
                        && section.physical_ordinal == expected.physical_ordinal()
                },
            )
    }

    pub fn seal(mut self) -> Result<Self, ContextSnapshotError> {
        self.validate_shape_without_hash()?;
        self.snapshot_hash = self.canonical_hash()?;
        Ok(self)
    }

    pub fn validate_contract_shape(&self) -> Result<(), ContextSnapshotError> {
        self.validate_shape_without_hash()?;
        if self.snapshot_hash != self.canonical_hash()? {
            return Err(ContextSnapshotError::SnapshotHashMismatch);
        }
        Ok(())
    }

    fn validate_shape_without_hash(&self) -> Result<(), ContextSnapshotError> {
        if self.snapshot_version != 2 {
            return Err(ContextSnapshotError::SnapshotHashMismatch);
        }
        if !self.has_canonical_section_order() {
            return Err(ContextSnapshotError::SectionOrderInvalid);
        }
        if self.active_prompt_module_ids.is_empty()
            || !self.active_prompt_module_ids.windows(2).all(|pair| pair[0] < pair[1])
            || self.active_prompt_module_ids.iter().any(|module_id| module_id.trim().is_empty())
        {
            return Err(ContextSnapshotError::SectionOrderInvalid);
        }
        if self.output_request.effective_max_output_tokens == 0
            || self.output_request.effective_max_output_tokens != self.budget.output_reserve_tokens
            || self.budget.effective_max_output_tokens != self.budget.output_reserve_tokens
            || self.budget.estimated_input_tokens != self.context_usage.used_input_tokens
            || !self.context_usage.is_consistent()
            || !self.budget.item_estimates.iter().enumerate().all(|(index, item)| {
                u32::try_from(index + 1).is_ok_and(|ordinal| item.ordinal == ordinal)
            })
        {
            return Err(ContextSnapshotError::BudgetInvalid);
        }
        if self.tool_schema_set.validate().is_err()
            || !self
                .effective_tool_bindings
                .windows(2)
                .all(|pair| pair[0].tool_id < pair[1].tool_id)
            || self.effective_tool_bindings.len() != self.tool_schema_set.tools.len()
            || self.effective_tool_bindings.iter().zip(&self.tool_schema_set.tools).any(
                |(binding, schema)| {
                    binding.tool_id != schema.tool_id || binding.tool_version != schema.tool_version
                },
            )
        {
            return Err(ContextSnapshotError::ToolOrderInvalid);
        }
        Ok(())
    }

    fn canonical_hash(&self) -> Result<ContentHash, ContextSnapshotError> {
        #[derive(Serialize)]
        struct HashInput<'a> {
            snapshot_version: u32,
            id: ContextProjectionSnapshotId,
            session_id: SessionId,
            turn_task_id: TurnTaskId,
            agent_run_id: AgentRunId,
            model_step_id: ModelStepId,
            prompt_bundle_snapshot_id: PromptBundleSnapshotId,
            active_prompt_module_ids: &'a [String],
            model_profile_id: ModelProfileId,
            budget: &'a ContextBudgetSnapshot,
            sections: &'a [ContextSectionSnapshot],
            tool_schema_set: &'a ToolSchemaSetSnapshotV1,
            effective_tool_bindings: &'a [EffectiveToolBindingSnapshot],
            conversation_items: &'a [LogicalConversationItem],
            attachments: &'a [ModelAttachmentSnapshot],
            active_task_lineage_id: Option<TaskLineageId>,
            instruction_envelope_artifact_id: ArtifactId,
            instruction_envelope_hash: &'a ContentHash,
            model_egress_policy_version: ModelEgressPolicyVersion,
            model_egress_manifest_artifact_id: ArtifactId,
            model_egress_manifest_hash: &'a ContentHash,
            output_request: &'a ModelOutputRequest,
            history_cutoff: SessionSequence,
            context_usage: &'a ContextUsageSummaryView,
            created_at: UtcTimestamp,
        }
        canonical_json_v1_sha256(&HashInput {
            snapshot_version: self.snapshot_version,
            id: self.id,
            session_id: self.session_id,
            turn_task_id: self.turn_task_id,
            agent_run_id: self.agent_run_id,
            model_step_id: self.model_step_id,
            prompt_bundle_snapshot_id: self.prompt_bundle_snapshot_id,
            active_prompt_module_ids: &self.active_prompt_module_ids,
            model_profile_id: self.model_profile_id,
            budget: &self.budget,
            sections: &self.sections,
            tool_schema_set: &self.tool_schema_set,
            effective_tool_bindings: &self.effective_tool_bindings,
            conversation_items: &self.conversation_items,
            attachments: &self.attachments,
            active_task_lineage_id: self.active_task_lineage_id,
            instruction_envelope_artifact_id: self.instruction_envelope_artifact_id,
            instruction_envelope_hash: &self.instruction_envelope_hash,
            model_egress_policy_version: self.model_egress_policy_version,
            model_egress_manifest_artifact_id: self.model_egress_manifest_artifact_id,
            model_egress_manifest_hash: &self.model_egress_manifest_hash,
            output_request: &self.output_request,
            history_cutoff: self.history_cutoff,
            context_usage: &self.context_usage,
            created_at: self.created_at,
        })
        .map_err(|_| ContextSnapshotError::CanonicalSerialization)
    }
}

const fn prompt_section_order(section: PromptSectionClass) -> u8 {
    match section {
        PromptSectionClass::Core => 1,
        PromptSectionClass::Permission => 2,
        PromptSectionClass::ToolSummary => 3,
        PromptSectionClass::WorkspaceAgents => 4,
        PromptSectionClass::HistoricalSnapshot => 5,
        PromptSectionClass::AgentMode => 6,
        PromptSectionClass::Hook => 7,
        PromptSectionClass::TaskState => 8,
        PromptSectionClass::Compaction => 9,
        PromptSectionClass::Maintenance => 10,
    }
}
