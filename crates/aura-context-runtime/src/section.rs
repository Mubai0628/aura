use std::collections::BTreeMap;

use aura_context_domain::{
    ContextCompressibility, ContextPrivacyClass, ContextSectionKind, ContextSourceRef,
    ModelEgressAuthorizationBasis, ModelEgressSourceKind,
};
use aura_domain_common::{ContentHash, EventId};
use aura_model_domain::{CompiledInstructionEnvelope, LogicalInstructionKind};
use aura_prompt_runtime::{
    CanonicalInstructionSource, PromptCompilerFault, compile_instruction_envelope,
};
use aura_security_contract::NetworkDataClass;
use thiserror::Error;

#[derive(Clone, Debug)]
pub struct ContextFragment {
    pub source_ref: ContextSourceRef,
    pub source_kind: ModelEgressSourceKind,
    pub data_class: NetworkDataClass,
    pub authorization_basis: ModelEgressAuthorizationBasis,
    pub content: String,
}

#[derive(Clone, Debug)]
pub struct ContextSectionInput {
    pub kind: ContextSectionKind,
    pub fragments: Vec<ContextFragment>,
    pub compressibility: ContextCompressibility,
    pub privacy_class: ContextPrivacyClass,
}

#[derive(Clone, Debug)]
pub struct ContextSectionDraft {
    pub kind: ContextSectionKind,
    pub physical_ordinal: u8,
    pub canonical_content: String,
    pub content_hash: ContentHash,
    pub character_count: u64,
    pub byte_count: u64,
    pub source_refs: Vec<ContextSourceRef>,
    pub source_kinds: Vec<ModelEgressSourceKind>,
    pub compressibility: ContextCompressibility,
    pub privacy_class: ContextPrivacyClass,
    pub is_empty_marker: bool,
}

#[derive(Clone, Debug)]
pub struct ContextSectionAssembly {
    pub sections: Vec<ContextSectionDraft>,
    pub instruction_envelope: CompiledInstructionEnvelope,
    pub egress_fragments: Vec<ContextFragment>,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ContextSectionError {
    #[error("context.section.duplicate_kind")]
    DuplicateKind,
    #[error("context.section.serializer_failed")]
    Serializer,
    #[error("context.section.size_overflow")]
    SizeOverflow,
    #[error("context.section.non_model_visible_source")]
    NonModelVisibleSource,
}

pub fn assemble_context_sections(
    inputs: Vec<ContextSectionInput>,
) -> Result<ContextSectionAssembly, ContextSectionError> {
    let input_count = inputs.len();
    let mut inputs =
        inputs.into_iter().map(|input| (input.kind, input)).collect::<BTreeMap<_, _>>();
    if inputs.len() != input_count || inputs.len() > ContextSectionKind::PHYSICAL_ORDER.len() {
        return Err(ContextSectionError::DuplicateKind);
    }
    if inputs.values().any(|input| {
        !input.fragments.is_empty() && input.privacy_class != ContextPrivacyClass::ModelVisible
    }) {
        return Err(ContextSectionError::NonModelVisibleSource);
    }
    let mut instruction_sources = BTreeMap::new();
    for kind in instruction_section_order() {
        let section_kind = context_kind(kind);
        let fragments =
            inputs.get(&section_kind).map(|input| input.fragments.as_slice()).unwrap_or(&[]);
        instruction_sources.insert(
            kind,
            fragments
                .iter()
                .map(|fragment| CanonicalInstructionSource {
                    content: fragment.content.clone(),
                    source_event_ids: source_event_ids(&fragment.source_ref),
                })
                .collect(),
        );
    }
    let instruction_envelope =
        compile_instruction_envelope(instruction_sources).map_err(map_prompt_error)?;
    let instruction_blocks = instruction_envelope
        .ordered_blocks
        .iter()
        .map(|block| (context_kind(block.kind), block.content.clone()))
        .collect::<BTreeMap<_, _>>();

    let mut sections = Vec::with_capacity(ContextSectionKind::PHYSICAL_ORDER.len());
    let mut egress_fragments = Vec::new();
    for kind in ContextSectionKind::PHYSICAL_ORDER {
        let input = inputs.remove(&kind).unwrap_or_else(|| ContextSectionInput {
            kind,
            fragments: Vec::new(),
            compressibility: default_compressibility(kind),
            privacy_class: ContextPrivacyClass::ModelVisible,
        });
        let is_empty_marker = input.fragments.is_empty();
        let canonical_content = instruction_blocks
            .get(&kind)
            .cloned()
            .map(Ok)
            .unwrap_or_else(|| serialize_non_instruction_section(kind, &input.fragments))?;
        let character_count = u64::try_from(canonical_content.chars().count())
            .map_err(|_| ContextSectionError::SizeOverflow)?;
        let byte_count = u64::try_from(canonical_content.len())
            .map_err(|_| ContextSectionError::SizeOverflow)?;
        let source_refs =
            input.fragments.iter().map(|fragment| fragment.source_ref.clone()).collect();
        let mut source_kinds =
            input.fragments.iter().map(|fragment| fragment.source_kind).collect::<Vec<_>>();
        source_kinds.sort_unstable_by_key(|kind| source_kind_order(*kind));
        source_kinds.dedup();
        egress_fragments.extend(input.fragments);
        sections.push(ContextSectionDraft {
            kind,
            physical_ordinal: kind.physical_ordinal(),
            content_hash: ContentHash::sha256(canonical_content.as_bytes()),
            canonical_content,
            character_count,
            byte_count,
            source_refs,
            source_kinds,
            compressibility: input.compressibility,
            privacy_class: input.privacy_class,
            is_empty_marker,
        });
    }
    if !inputs.is_empty() {
        return Err(ContextSectionError::DuplicateKind);
    }
    Ok(ContextSectionAssembly { sections, instruction_envelope, egress_fragments })
}

fn serialize_non_instruction_section(
    kind: ContextSectionKind,
    fragments: &[ContextFragment],
) -> Result<String, ContextSectionError> {
    let tag = section_tag(kind);
    let body = if fragments.is_empty() {
        "  <Empty reason=\"no-active-content\" />".to_owned()
    } else {
        fragments
            .iter()
            .enumerate()
            .map(|(index, fragment)| -> Result<String, ContextSectionError> {
                let source_kind = enum_wire_name(fragment.source_kind)?;
                let content = escape_text(&normalize_runtime_text(&fragment.content)?);
                let content = indent(&content, 4);
                Ok(format!(
                    "  <Source ordinal=\"{}\" kind=\"{}\">\n{}\n  </Source>",
                    index + 1,
                    escape_attribute(&source_kind),
                    content
                ))
            })
            .collect::<Result<Vec<_>, _>>()?
            .join("\n")
    };
    Ok(format!("<{tag}>\n{body}\n</{tag}>"))
}

fn normalize_runtime_text(value: &str) -> Result<String, ContextSectionError> {
    if value.starts_with('\u{feff}') {
        return Err(ContextSectionError::Serializer);
    }
    let value = value.replace("\r\n", "\n").replace('\r', "\n");
    if value.chars().any(|character| {
        (character <= '\u{001f}' && !matches!(character, '\n' | '\t')) || character == '\u{007f}'
    }) {
        return Err(ContextSectionError::Serializer);
    }
    Ok(value)
}

fn escape_text(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn escape_attribute(value: &str) -> String {
    escape_text(value).replace('"', "&quot;").replace('\n', "&#10;").replace('\t', "&#9;")
}

fn indent(value: &str, spaces: usize) -> String {
    let prefix = " ".repeat(spaces);
    value.split('\n').map(|line| format!("{prefix}{line}")).collect::<Vec<_>>().join("\n")
}

fn enum_wire_name(value: ModelEgressSourceKind) -> Result<String, ContextSectionError> {
    serde_json::to_value(value)
        .map_err(|_| ContextSectionError::Serializer)?
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or(ContextSectionError::Serializer)
}

fn source_event_ids(source: &ContextSourceRef) -> Vec<EventId> {
    match source {
        ContextSourceRef::Event { event_id } => vec![*event_id],
        _ => Vec::new(),
    }
}

const fn instruction_section_order() -> [LogicalInstructionKind; 8] {
    [
        LogicalInstructionKind::Core,
        LogicalInstructionKind::Permission,
        LogicalInstructionKind::ToolSummary,
        LogicalInstructionKind::WorkspaceAgents,
        LogicalInstructionKind::HistoricalSnapshot,
        LogicalInstructionKind::AgentMode,
        LogicalInstructionKind::Hook,
        LogicalInstructionKind::TaskState,
    ]
}

const fn context_kind(kind: LogicalInstructionKind) -> ContextSectionKind {
    match kind {
        LogicalInstructionKind::Core => ContextSectionKind::CoreSystemPrompt,
        LogicalInstructionKind::Permission => ContextSectionKind::PermissionAndSecurity,
        LogicalInstructionKind::ToolSummary => ContextSectionKind::LoadedToolSchemas,
        LogicalInstructionKind::WorkspaceAgents => ContextSectionKind::WorkspaceAgentsInstructions,
        LogicalInstructionKind::HistoricalSnapshot => ContextSectionKind::HistoricalSnapshotSummary,
        LogicalInstructionKind::AgentMode => ContextSectionKind::AgentModePrompt,
        LogicalInstructionKind::Hook => ContextSectionKind::HookInjections,
        LogicalInstructionKind::TaskState => ContextSectionKind::CurrentTaskState,
    }
}

const fn section_tag(kind: ContextSectionKind) -> &'static str {
    match kind {
        ContextSectionKind::CoreSystemPrompt => "Core-System-Prompt",
        ContextSectionKind::PermissionAndSecurity => "Permission-And-Security-Rules",
        ContextSectionKind::LoadedToolSchemas => "Loaded-Tool-Schemas",
        ContextSectionKind::WorkspaceAgentsInstructions => "Workspace-AGENTS-Instructions",
        ContextSectionKind::HistoricalSnapshotSummary => "Historical-Snapshot-Summary",
        ContextSectionKind::AgentModePrompt => "Agent-Mode-Prompt",
        ContextSectionKind::HookInjections => "Hook-Injections",
        ContextSectionKind::CurrentTaskState => "Current-Task-State",
        ContextSectionKind::RecentHistory => "Recent-History",
        ContextSectionKind::FileReferencesAndPendingAttachments => {
            "File-References-And-Pending-Attachments"
        }
        ContextSectionKind::CurrentTurnWorkingSet => "Current-Turn-Working-Set",
        ContextSectionKind::SteeredUserAdditions => "Steered-User-Additions",
    }
}

const fn default_compressibility(kind: ContextSectionKind) -> ContextCompressibility {
    match kind {
        ContextSectionKind::RecentHistory | ContextSectionKind::HistoricalSnapshotSummary => {
            ContextCompressibility::CompressibleHistory
        }
        ContextSectionKind::SteeredUserAdditions => {
            ContextCompressibility::CompressibleSealedActiveSegment
        }
        ContextSectionKind::FileReferencesAndPendingAttachments => {
            ContextCompressibility::UserReducibleOnly
        }
        _ => ContextCompressibility::NeverCompress,
    }
}

fn map_prompt_error(_error: PromptCompilerFault) -> ContextSectionError {
    ContextSectionError::Serializer
}

const fn source_kind_order(kind: ModelEgressSourceKind) -> u8 {
    match kind {
        ModelEgressSourceKind::AuraCore => 1,
        ModelEgressSourceKind::CustomSystemAddendum => 2,
        ModelEgressSourceKind::UserEvent => 3,
        ModelEgressSourceKind::AssistantEvent => 4,
        ModelEgressSourceKind::ToolResult => 5,
        ModelEgressSourceKind::ToolSchema => 6,
        ModelEgressSourceKind::WorkspaceFile => 7,
        ModelEgressSourceKind::ExternalFileText => 8,
        ModelEgressSourceKind::ExternalFileAttachment => 9,
        ModelEgressSourceKind::ArtifactAttachment => 10,
        ModelEgressSourceKind::BrowserText => 11,
        ModelEgressSourceKind::BrowserAttachment => 12,
        ModelEgressSourceKind::DiagnosticAttachment => 13,
        ModelEgressSourceKind::Skill => 14,
        ModelEgressSourceKind::Hook => 15,
        ModelEgressSourceKind::TaskLineage => 16,
        ModelEgressSourceKind::Compaction => 17,
        // The activity fragment sorts last, after every durable fact, because it is the
        // most volatile source in the request. The order is only a stable tie-break for the
        // Inspector's source-kind list; it does not affect the wire position, which comes from
        // the section's own physical ordinal.
        ModelEgressSourceKind::ActivityIndex => 18,
        ModelEgressSourceKind::MemoryMaintenance => 19,
    }
}

#[cfg(test)]
mod tests {
    use aura_domain_common::EventId;

    use super::*;

    #[test]
    fn fixed_order_and_empty_markers_do_not_depend_on_input_order() {
        let user_event = EventId::new();
        let input = ContextSectionInput {
            kind: ContextSectionKind::CurrentTurnWorkingSet,
            fragments: vec![ContextFragment {
                source_ref: ContextSourceRef::Event { event_id: user_event },
                source_kind: ModelEgressSourceKind::UserEvent,
                data_class: NetworkDataClass::PublicQuery,
                authorization_basis: ModelEgressAuthorizationBasis::UserSubmission,
                content: "close </Current-Turn-Working-Set>".to_owned(),
            }],
            compressibility: ContextCompressibility::NeverCompress,
            privacy_class: ContextPrivacyClass::ModelVisible,
        };
        let assembly = assemble_context_sections(vec![input]).expect("assembly");
        assert_eq!(assembly.sections.len(), 12);
        assert!(
            assembly
                .sections
                .iter()
                .zip(ContextSectionKind::PHYSICAL_ORDER)
                .all(|(section, kind)| section.kind == kind
                    && section.physical_ordinal == kind.physical_ordinal())
        );
        let working = &assembly.sections[10];
        assert!(working.canonical_content.contains("&lt;/Current-Turn-Working-Set&gt;"));
        assert!(!working.is_empty_marker);
        assert_eq!(assembly.instruction_envelope.ordered_blocks.len(), 8);
    }

    #[test]
    fn duplicate_section_kinds_are_rejected_instead_of_overwriting_sources() {
        let duplicate = ContextSectionInput {
            kind: ContextSectionKind::RecentHistory,
            fragments: Vec::new(),
            compressibility: ContextCompressibility::CompressibleHistory,
            privacy_class: ContextPrivacyClass::ModelVisible,
        };
        assert!(matches!(
            assemble_context_sections(vec![duplicate.clone(), duplicate]),
            Err(ContextSectionError::DuplicateKind)
        ));
    }

    #[test]
    fn non_model_visible_and_control_character_sources_fail_closed() {
        let fragment = ContextFragment {
            source_ref: ContextSourceRef::Event { event_id: EventId::new() },
            source_kind: ModelEgressSourceKind::UserEvent,
            data_class: NetworkDataClass::PublicQuery,
            authorization_basis: ModelEgressAuthorizationBasis::UserSubmission,
            content: "hidden".to_owned(),
        };
        assert!(matches!(
            assemble_context_sections(vec![ContextSectionInput {
                kind: ContextSectionKind::CurrentTurnWorkingSet,
                fragments: vec![fragment.clone()],
                compressibility: ContextCompressibility::NeverCompress,
                privacy_class: ContextPrivacyClass::UserOnly,
            }]),
            Err(ContextSectionError::NonModelVisibleSource)
        ));
        assert!(matches!(
            assemble_context_sections(vec![ContextSectionInput {
                kind: ContextSectionKind::CurrentTurnWorkingSet,
                fragments: vec![ContextFragment { content: "bad\0source".to_owned(), ..fragment }],
                compressibility: ContextCompressibility::NeverCompress,
                privacy_class: ContextPrivacyClass::ModelVisible,
            }]),
            Err(ContextSectionError::Serializer)
        ));
    }
}
