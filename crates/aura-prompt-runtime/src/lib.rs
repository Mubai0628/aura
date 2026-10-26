#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use aura_config::PromptSettingsSnapshot;
use aura_context_domain::{
    PromptActivationCondition, PromptAgentMode, PromptBundleSnapshot, PromptModuleSnapshot,
    PromptSectionClass,
};
use aura_domain_common::CanonicalSemVer;
use aura_domain_common::{
    ArtifactId, ContentHash, ContentHashAlgorithm, EventId, PromptBundleSnapshotId, Revision,
    TurnTaskId, UtcTimestamp,
};
use aura_model_domain::{
    CompiledInstructionEnvelope, LogicalInstructionBlock, LogicalInstructionKind,
    LogicalModelRequestError,
};
use aura_network_control::SecretScanner;
use thiserror::Error;

struct EmbeddedPromptModule {
    id: &'static str,
    version: &'static str,
    path: &'static str,
    required: bool,
    section: &'static str,
    activation: &'static str,
    order: u32,
    content_hash_hex: &'static str,
    content: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/embedded_prompt_bundle.rs"));

pub const PROMPT_MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const PROMPT_SERIALIZER_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedPromptModule {
    pub module_id: String,
    pub module_version: CanonicalSemVer,
    pub path: String,
    pub required: bool,
    pub section: PromptSectionClass,
    pub activation: PromptActivationCondition,
    pub order: u32,
    pub content_hash: ContentHash,
    pub normalized_content: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedPromptBundle {
    pub schema_version: u32,
    pub bundle_version: CanonicalSemVer,
    pub serializer_version: u32,
    pub manifest_hash: ContentHash,
    pub modules: Vec<ValidatedPromptModule>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PromptActivationInput {
    pub agent_mode: PromptAgentMode,
    pub loaded_capabilities: BTreeSet<String>,
    // UNWIRED: production always passes false, an empty set and None for these three, so the
    // child-agent, recovery and failure-summary / environment-update maintenance modules are never
    // selected (memory maintenance and compaction take their modules by id).
    pub child_agent: bool,
    pub recovery_kinds: BTreeSet<String>,
    pub compaction: bool,
    pub maintenance_kind: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalInstructionSource {
    pub content: String,
    pub source_event_ids: Vec<EventId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PromptBundleArtifactDraft {
    pub artifact_id: ArtifactId,
    pub media_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrozenPromptBundleDraft {
    pub snapshot: PromptBundleSnapshot,
    pub artifacts: Vec<PromptBundleArtifactDraft>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ActivatedPromptSourceIdentity {
    Module { module_id: String, content_hash: ContentHash },
    CustomSystemAddendum { settings_revision: Revision, content_hash: ContentHash },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivatedPromptSource {
    pub identity: ActivatedPromptSourceIdentity,
    pub section: PromptSectionClass,
    pub content: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivatedPromptSet {
    pub active_module_ids: Vec<String>,
    pub sources: Vec<ActivatedPromptSource>,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum PromptCompilerFault {
    #[error("prompt.compiler_fault: embedded prompt manifest is invalid")]
    Manifest,
    #[error("prompt.compiler_fault: embedded prompt module is invalid")]
    Module,
    #[error("prompt.compiler_fault: required prompt module is missing")]
    RequiredModuleMissing,
    #[error("prompt.compiler_fault: prompt activation is invalid")]
    Activation,
    #[error("prompt.compiler_fault: prompt serializer rejected an input")]
    Serializer,
    #[error("prompt.compiler_fault: custom system addendum contains suspected secret material")]
    AddendumSecret,
    #[error("prompt.compiler_fault: prompt bundle snapshot metadata is invalid")]
    Snapshot,
}

impl ValidatedPromptBundle {
    pub fn embedded() -> Result<Self, PromptCompilerFault> {
        if EMBEDDED_SCHEMA_VERSION != PROMPT_MANIFEST_SCHEMA_VERSION
            || EMBEDDED_SERIALIZER_VERSION != PROMPT_SERIALIZER_VERSION
        {
            return Err(PromptCompilerFault::Manifest);
        }
        let bundle_version = CanonicalSemVer::new(EMBEDDED_BUNDLE_VERSION)
            .map_err(|_| PromptCompilerFault::Manifest)?;
        let manifest_hash =
            ContentHash::new(ContentHashAlgorithm::Sha256, EMBEDDED_MANIFEST_HASH_HEX.to_owned())
                .map_err(|_| PromptCompilerFault::Manifest)?;
        let mut modules = Vec::with_capacity(EMBEDDED_MODULES.len());
        let mut ids = BTreeSet::new();
        let mut order_keys = BTreeSet::new();
        for module in EMBEDDED_MODULES {
            if !ids.insert(module.id) || !order_keys.insert((module.section, module.order)) {
                return Err(PromptCompilerFault::Module);
            }
            let content_hash =
                ContentHash::new(ContentHashAlgorithm::Sha256, module.content_hash_hex.to_owned())
                    .map_err(|_| PromptCompilerFault::Module)?;
            if ContentHash::sha256(module.content.as_bytes()) != content_hash
                || !module.content.ends_with('\n')
                || module.content.contains('\r')
            {
                return Err(PromptCompilerFault::Module);
            }
            modules.push(ValidatedPromptModule {
                module_id: module.id.to_owned(),
                module_version: CanonicalSemVer::new(module.version)
                    .map_err(|_| PromptCompilerFault::Module)?,
                path: module.path.to_owned(),
                required: module.required,
                section: parse_section(module.section)?,
                activation: parse_activation(module.activation)?,
                order: module.order,
                content_hash,
                normalized_content: module.content.to_owned(),
            });
        }
        for required in EMBEDDED_MODULES.iter().filter(|module| module.required) {
            if !modules.iter().any(|module| module.module_id == required.id) {
                return Err(PromptCompilerFault::RequiredModuleMissing);
            }
        }
        modules.sort_by(|left, right| {
            section_key(left.section)
                .cmp(&section_key(right.section))
                .then_with(|| left.order.cmp(&right.order))
                .then_with(|| left.module_id.cmp(&right.module_id))
        });
        Ok(Self {
            schema_version: EMBEDDED_SCHEMA_VERSION,
            bundle_version,
            serializer_version: EMBEDDED_SERIALIZER_VERSION,
            manifest_hash,
            modules,
        })
    }

    pub fn active_modules<'a>(
        &'a self,
        input: &PromptActivationInput,
    ) -> Result<Vec<&'a ValidatedPromptModule>, PromptCompilerFault> {
        if input.compaction && input.maintenance_kind.is_some() {
            return Err(PromptCompilerFault::Activation);
        }
        let mut active = self
            .modules
            .iter()
            .filter(|module| activation_matches(&module.activation, input))
            .collect::<Vec<_>>();
        active.sort_by(|left, right| {
            section_key(left.section)
                .cmp(&section_key(right.section))
                .then_with(|| left.order.cmp(&right.order))
                .then_with(|| left.module_id.cmp(&right.module_id))
        });
        Ok(active)
    }
}

pub fn validated_custom_addendum(
    settings: &PromptSettingsSnapshot,
) -> Result<Option<String>, PromptCompilerFault> {
    let Some(addendum) = &settings.custom_system_addendum else {
        return Ok(None);
    };
    SecretScanner
        .scan_text("prompt.custom_system_addendum", addendum)
        .map_err(|_| PromptCompilerFault::AddendumSecret)?;
    Ok(Some(addendum.clone()))
}

pub fn freeze_prompt_bundle(
    bundle: &ValidatedPromptBundle,
    settings: &PromptSettingsSnapshot,
    turn_task_id: TurnTaskId,
    release_identity: &str,
    created_at: UtcTimestamp,
) -> Result<FrozenPromptBundleDraft, PromptCompilerFault> {
    if release_identity.trim().is_empty()
        || bundle.serializer_version == 0
        || bundle.modules.is_empty()
    {
        return Err(PromptCompilerFault::Snapshot);
    }

    let mut artifacts = Vec::with_capacity(bundle.modules.len() + 1);
    let mut module_snapshots = Vec::with_capacity(bundle.modules.len());
    for module in &bundle.modules {
        let artifact_id = ArtifactId::new();
        let bytes = module.normalized_content.as_bytes().to_vec();
        if ContentHash::sha256(&bytes) != module.content_hash {
            return Err(PromptCompilerFault::Module);
        }
        artifacts.push(PromptBundleArtifactDraft {
            artifact_id,
            media_type: "text/markdown; charset=utf-8".to_owned(),
            bytes,
        });
        module_snapshots.push(PromptModuleSnapshot {
            module_id: module.module_id.clone(),
            module_version: module.module_version.clone(),
            section: module.section,
            activation: module.activation.clone(),
            order: module.order,
            content_hash: module.content_hash.clone(),
            content_artifact_id: artifact_id,
        });
    }

    let addendum = validated_custom_addendum(settings)?;
    let (custom_addendum_artifact_id, custom_addendum_hash) = match addendum {
        Some(addendum) => {
            let artifact_id = ArtifactId::new();
            let bytes = addendum.into_bytes();
            let content_hash = ContentHash::sha256(&bytes);
            artifacts.push(PromptBundleArtifactDraft {
                artifact_id,
                media_type: "text/plain; charset=utf-8".to_owned(),
                bytes,
            });
            (Some(artifact_id), Some(content_hash))
        }
        None => (None, None),
    };

    Ok(FrozenPromptBundleDraft {
        snapshot: PromptBundleSnapshot {
            id: PromptBundleSnapshotId::new(),
            turn_task_id,
            bundle_version: bundle.bundle_version.clone(),
            serializer_version: bundle.serializer_version,
            module_snapshots,
            custom_addendum_artifact_id,
            custom_addendum_hash,
            settings_revision: Revision::new(settings.revision),
            bundle_manifest_hash: bundle.manifest_hash.clone(),
            release_identity: release_identity.to_owned(),
            created_at,
        },
        artifacts,
    })
}

pub fn activate_prompt_sources(
    bundle: &ValidatedPromptBundle,
    settings: &PromptSettingsSnapshot,
    activation: &PromptActivationInput,
) -> Result<ActivatedPromptSet, PromptCompilerFault> {
    let active_modules = bundle.active_modules(activation)?;
    if active_modules.is_empty() {
        return Err(PromptCompilerFault::RequiredModuleMissing);
    }
    let mut active_module_ids =
        active_modules.iter().map(|module| module.module_id.clone()).collect::<Vec<_>>();
    active_module_ids.sort();
    if !active_module_ids.windows(2).all(|pair| pair[0] < pair[1]) {
        return Err(PromptCompilerFault::Activation);
    }
    let mut sources = active_modules
        .into_iter()
        .map(|module| ActivatedPromptSource {
            identity: ActivatedPromptSourceIdentity::Module {
                module_id: module.module_id.clone(),
                content_hash: module.content_hash.clone(),
            },
            section: module.section,
            content: module.normalized_content.clone(),
        })
        .collect::<Vec<_>>();
    if let Some(addendum) = validated_custom_addendum(settings)? {
        let content_hash = ContentHash::sha256(addendum.as_bytes());
        let source = ActivatedPromptSource {
            identity: ActivatedPromptSourceIdentity::CustomSystemAddendum {
                settings_revision: Revision::new(settings.revision),
                content_hash,
            },
            section: PromptSectionClass::Core,
            content: addendum,
        };
        let insert_at = sources
            .iter()
            .rposition(|source| source.section == PromptSectionClass::Core)
            .map_or(0, |index| index + 1);
        sources.insert(insert_at, source);
    }
    Ok(ActivatedPromptSet { active_module_ids, sources })
}

pub fn compile_instruction_envelope(
    mut sources: BTreeMap<LogicalInstructionKind, Vec<CanonicalInstructionSource>>,
) -> Result<CompiledInstructionEnvelope, PromptCompilerFault> {
    let order = [
        LogicalInstructionKind::Core,
        LogicalInstructionKind::Permission,
        LogicalInstructionKind::ToolSummary,
        LogicalInstructionKind::WorkspaceAgents,
        LogicalInstructionKind::HistoricalSnapshot,
        LogicalInstructionKind::AgentMode,
        LogicalInstructionKind::Hook,
        LogicalInstructionKind::TaskState,
    ];
    let mut blocks = Vec::with_capacity(order.len());
    for kind in order {
        let tag = instruction_tag(kind);
        let inputs = sources.remove(&kind).unwrap_or_default();
        let mut source_event_ids = inputs
            .iter()
            .flat_map(|input| input.source_event_ids.iter().copied())
            .collect::<Vec<_>>();
        source_event_ids.sort_unstable();
        source_event_ids.dedup();
        let body = if inputs.is_empty() {
            "  <Empty reason=\"no-active-content\" />".to_owned()
        } else {
            inputs
                .iter()
                .map(|input| {
                    normalize_runtime_text(&input.content)
                        .map(|content| indent(&escape_text(&content), 2))
                })
                .collect::<Result<Vec<_>, PromptCompilerFault>>()?
                .join("\n")
        };
        let content = format!("<{tag}>\n{body}\n</{tag}>");
        blocks.push(LogicalInstructionBlock {
            kind,
            content_hash: ContentHash::sha256(content.as_bytes()),
            content,
            source_event_ids,
        });
    }
    if !sources.is_empty() {
        return Err(PromptCompilerFault::Serializer);
    }
    let canonical_text =
        blocks.iter().map(|block| block.content.as_str()).collect::<Vec<_>>().join("\n");
    CompiledInstructionEnvelope::new(PROMPT_SERIALIZER_VERSION, blocks, canonical_text)
        .map_err(map_model_error)
}

fn normalize_runtime_text(value: &str) -> Result<String, PromptCompilerFault> {
    if value.starts_with('\u{feff}') {
        return Err(PromptCompilerFault::Serializer);
    }
    let value = value.replace("\r\n", "\n").replace('\r', "\n");
    if value.chars().any(|character| {
        (character <= '\u{001f}' && !matches!(character, '\n' | '\t')) || character == '\u{007f}'
    }) {
        return Err(PromptCompilerFault::Serializer);
    }
    Ok(value.trim_end_matches('\n').to_owned())
}

fn escape_text(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn indent(value: &str, spaces: usize) -> String {
    let prefix = " ".repeat(spaces);
    value.lines().map(|line| format!("{prefix}{line}")).collect::<Vec<_>>().join("\n")
}

const fn instruction_tag(kind: LogicalInstructionKind) -> &'static str {
    match kind {
        LogicalInstructionKind::Core => "Core-System-Prompt",
        LogicalInstructionKind::Permission => "Permission-And-Security-Rules",
        LogicalInstructionKind::ToolSummary => "Loaded-Tool-Schemas",
        LogicalInstructionKind::WorkspaceAgents => "Workspace-AGENTS-Instructions",
        LogicalInstructionKind::HistoricalSnapshot => "Historical-Snapshot-Summary",
        LogicalInstructionKind::AgentMode => "Agent-Mode-Prompt",
        LogicalInstructionKind::Hook => "Hook-Injections",
        LogicalInstructionKind::TaskState => "Current-Task-State",
    }
}

fn parse_section(value: &str) -> Result<PromptSectionClass, PromptCompilerFault> {
    match value {
        "core" => Ok(PromptSectionClass::Core),
        "permission" => Ok(PromptSectionClass::Permission),
        "tool_summary" => Ok(PromptSectionClass::ToolSummary),
        "workspace_agents" => Ok(PromptSectionClass::WorkspaceAgents),
        "historical_snapshot" => Ok(PromptSectionClass::HistoricalSnapshot),
        "agent_mode" => Ok(PromptSectionClass::AgentMode),
        "hook" => Ok(PromptSectionClass::Hook),
        "task_state" => Ok(PromptSectionClass::TaskState),
        "compaction" => Ok(PromptSectionClass::Compaction),
        "maintenance" => Ok(PromptSectionClass::Maintenance),
        _ => Err(PromptCompilerFault::Module),
    }
}

fn parse_activation(value: &str) -> Result<PromptActivationCondition, PromptCompilerFault> {
    if value == "always" {
        return Ok(PromptActivationCondition::Always);
    }
    if value == "child_agent" {
        return Ok(PromptActivationCondition::ChildAgent);
    }
    if value == "compaction" {
        return Ok(PromptActivationCondition::Compaction);
    }
    if let Some(value) = value.strip_prefix("agent_mode:") {
        return Ok(PromptActivationCondition::AgentMode {
            mode: match value {
                "normal" => PromptAgentMode::Normal,
                "plan" => PromptAgentMode::Plan,
                "goal" => PromptAgentMode::Goal,
                _ => return Err(PromptCompilerFault::Activation),
            },
        });
    }
    if let Some(value) = value.strip_prefix("capability:") {
        return Ok(PromptActivationCondition::Capability { capability: value.to_owned() });
    }
    if let Some(value) = value.strip_prefix("recovery:") {
        return Ok(PromptActivationCondition::Recovery { recovery_kind: value.to_owned() });
    }
    if let Some(value) = value.strip_prefix("maintenance:") {
        return Ok(PromptActivationCondition::Maintenance { maintenance_kind: value.to_owned() });
    }
    Err(PromptCompilerFault::Activation)
}

fn activation_matches(
    condition: &PromptActivationCondition,
    input: &PromptActivationInput,
) -> bool {
    match condition {
        PromptActivationCondition::Always => !input.compaction && input.maintenance_kind.is_none(),
        PromptActivationCondition::AgentMode { mode } => {
            !input.compaction && input.maintenance_kind.is_none() && *mode == input.agent_mode
        }
        PromptActivationCondition::Capability { capability } => {
            !input.compaction
                && input.maintenance_kind.is_none()
                && input.loaded_capabilities.contains(capability)
        }
        PromptActivationCondition::ChildAgent => {
            !input.compaction && input.maintenance_kind.is_none() && input.child_agent
        }
        PromptActivationCondition::Recovery { recovery_kind } => {
            !input.compaction
                && input.maintenance_kind.is_none()
                && input.recovery_kinds.contains(recovery_kind)
        }
        PromptActivationCondition::Compaction => input.compaction,
        PromptActivationCondition::Maintenance { maintenance_kind } => {
            input.maintenance_kind.as_deref() == Some(maintenance_kind)
        }
    }
}

const fn section_key(section: PromptSectionClass) -> u8 {
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

fn map_model_error(_error: LogicalModelRequestError) -> PromptCompilerFault {
    PromptCompilerFault::Serializer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_bundle_passes_runtime_integrity_check() {
        let bundle = ValidatedPromptBundle::embedded().expect("embedded prompt bundle");
        assert_eq!(bundle.schema_version, 1);
        assert_eq!(bundle.serializer_version, 1);
        assert!(!bundle.modules.is_empty());
    }

    /// The model receives one activated set per effective mode. No mode may still
    /// carry the superseded completion mandate, and each mode must state the route
    /// that really ends its turns.
    #[test]
    fn activated_prompts_state_one_completion_route_per_effective_mode() {
        let bundle = ValidatedPromptBundle::embedded().expect("embedded prompt bundle");
        let settings = PromptSettingsSnapshot { revision: 1, custom_system_addendum: None };
        let activated = |mode: PromptAgentMode| {
            let sources = activate_prompt_sources(
                &bundle,
                &settings,
                &PromptActivationInput {
                    agent_mode: mode,
                    loaded_capabilities: BTreeSet::new(),
                    child_agent: false,
                    recovery_kinds: BTreeSet::new(),
                    compaction: false,
                    maintenance_kind: None,
                },
            )
            .expect("activated prompts");
            sources
                .sources
                .iter()
                .map(|source| source.content.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        };
        let normal = activated(PromptAgentMode::Normal);
        let plan = activated(PromptAgentMode::Plan);
        let goal = activated(PromptAgentMode::Goal);
        for (mode, text) in [("normal", &normal), ("plan", &plan), ("goal", &goal)] {
            assert!(
                !text.contains("submit your final answer through normal.complete"),
                "{mode} still requires the completion Tool"
            );
            assert!(
                !text.contains("the next user message continues Goal"),
                "{mode} still promises the next message resumes Goal"
            );
        }
        assert!(
            normal.contains("a complete end of your response that carries the user-visible answer finishes the turn"),
            "Normal must state that its own answer ends the turn"
        );
        assert!(
            normal.contains("`normal.complete` stays available")
                && normal.contains("never required"),
            "Normal keeps the explicit report optional"
        );
        for (mode, text) in [("plan", &plan), ("goal", &goal)] {
            assert!(
                text.contains("finish a single turn through normal.complete"),
                "{mode} keeps the explicit single-turn completion"
            );
            assert!(
                text.contains("mode.disposition"),
                "{mode} states the intent disposition instead of silent inheritance"
            );
            assert!(
                text.contains("a plain answer does not end"),
                "{mode} must not promise that prose alone ends its turns"
            );
        }
        assert!(
            !normal.contains("Each new user message starts its own current task"),
            "a consumed addition is not a new task root"
        );
        assert!(
            normal.contains("Each new turn root input starts its own current task")
                && normal.contains("Core's persisted source binding decides"),
            "task lineage must keep additions and query answers on the current task"
        );
        assert!(
            goal.contains("does not by itself resume the Goal"),
            "Goal no longer promises that a later message resumes it"
        );
    }

    #[test]
    fn canonical_serializer_escapes_sources_and_emits_exact_order() {
        let event_id = EventId::new();
        let envelope = compile_instruction_envelope(BTreeMap::from([(
            LogicalInstructionKind::Core,
            vec![CanonicalInstructionSource {
                content: "user </Core-System-Prompt> & data".to_owned(),
                source_event_ids: vec![event_id],
            }],
        )]))
        .expect("canonical envelope");
        assert_eq!(envelope.serializer_version, 1);
        assert_eq!(envelope.ordered_blocks.len(), 8);
        assert!(envelope.canonical_text.starts_with("<Core-System-Prompt>\n"));
        assert!(envelope.canonical_text.contains("&lt;/Core-System-Prompt&gt; &amp; data"));
        assert_eq!(envelope.canonical_text.matches("\n<").count(), 15);
        envelope.validate_contract_shape().expect("valid model contract");
    }

    #[test]
    fn prompt_bundle_freeze_captures_all_modules_and_validated_addendum() {
        let bundle = ValidatedPromptBundle::embedded().expect("embedded prompt bundle");
        let settings = PromptSettingsSnapshot {
            revision: 7,
            custom_system_addendum: Some("keep the boundary explicit\n".to_owned()),
        };
        let frozen = freeze_prompt_bundle(
            &bundle,
            &settings,
            TurnTaskId::new(),
            "aura-test-release",
            UtcTimestamp::now(),
        )
        .expect("frozen prompt bundle");
        assert_eq!(frozen.snapshot.module_snapshots.len(), bundle.modules.len());
        assert_eq!(frozen.artifacts.len(), bundle.modules.len() + 1);
        assert_eq!(frozen.snapshot.settings_revision, Revision::new(7));
        assert!(frozen.snapshot.custom_addendum_artifact_id.is_some());
        for module in &frozen.snapshot.module_snapshots {
            let artifact = frozen
                .artifacts
                .iter()
                .find(|artifact| artifact.artifact_id == module.content_artifact_id)
                .expect("module artifact");
            assert_eq!(ContentHash::sha256(&artifact.bytes), module.content_hash);
        }
    }

    #[test]
    fn activation_is_canonical_and_keeps_addendum_as_an_independent_core_source() {
        let bundle = ValidatedPromptBundle::embedded().expect("embedded prompt bundle");
        let settings = PromptSettingsSnapshot {
            revision: 11,
            custom_system_addendum: Some("User addendum".to_owned()),
        };
        let activated = activate_prompt_sources(
            &bundle,
            &settings,
            &PromptActivationInput {
                agent_mode: PromptAgentMode::Normal,
                loaded_capabilities: BTreeSet::new(),
                child_agent: false,
                recovery_kinds: BTreeSet::new(),
                compaction: false,
                maintenance_kind: None,
            },
        )
        .expect("activation");
        assert!(!activated.active_module_ids.is_empty());
        assert!(activated.active_module_ids.windows(2).all(|pair| pair[0] < pair[1]));
        let addendum = activated
            .sources
            .iter()
            .find(|source| {
                matches!(
                    source.identity,
                    ActivatedPromptSourceIdentity::CustomSystemAddendum { .. }
                )
            })
            .expect("addendum source");
        assert_eq!(addendum.section, PromptSectionClass::Core);
        assert_eq!(addendum.content, "User addendum");
    }
}
