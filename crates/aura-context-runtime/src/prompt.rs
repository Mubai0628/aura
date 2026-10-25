use std::collections::BTreeMap;

use aura_config::PromptSettingsSnapshot;
use aura_context_domain::{
    ContextCompressibility, ContextPrivacyClass, ContextSectionKind, ContextSourceRef,
    ModelEgressAuthorizationBasis, ModelEgressSourceKind, PromptSectionClass,
};
use aura_prompt_runtime::{
    ActivatedPromptSet, ActivatedPromptSourceIdentity, PromptActivationInput, PromptCompilerFault,
    ValidatedPromptBundle, activate_prompt_sources,
};
use aura_security_contract::NetworkDataClass;
use thiserror::Error;

use crate::{ContextFragment, ContextSectionInput};

#[derive(Clone, Debug)]
pub struct PromptContextSources {
    pub active_prompt_module_ids: Vec<String>,
    pub section_inputs: Vec<ContextSectionInput>,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum PromptContextSourceError {
    #[error("context.prompt.activation_failed")]
    Activation,
    #[error("context.prompt.duplicate_dynamic_section")]
    DuplicateDynamicSection,
}

pub fn assemble_prompt_context_sources(
    bundle: &ValidatedPromptBundle,
    settings: &PromptSettingsSnapshot,
    activation: &PromptActivationInput,
    dynamic_sections: Vec<ContextSectionInput>,
) -> Result<PromptContextSources, PromptContextSourceError> {
    let activated =
        activate_prompt_sources(bundle, settings, activation).map_err(map_prompt_error)?;
    merge_prompt_sources(activated, dynamic_sections)
}

fn merge_prompt_sources(
    activated: ActivatedPromptSet,
    dynamic_sections: Vec<ContextSectionInput>,
) -> Result<PromptContextSources, PromptContextSourceError> {
    let dynamic_count = dynamic_sections.len();
    let mut sections = dynamic_sections
        .into_iter()
        .map(|section| (section.kind, section))
        .collect::<BTreeMap<_, _>>();
    if sections.len() != dynamic_count {
        return Err(PromptContextSourceError::DuplicateDynamicSection);
    }
    let mut prompt_fragments = BTreeMap::<ContextSectionKind, Vec<ContextFragment>>::new();
    for source in activated.sources {
        let kind = context_section(source.section);
        let (source_ref, source_kind) = match source.identity {
            ActivatedPromptSourceIdentity::Module { module_id, content_hash } => (
                ContextSourceRef::PromptModule { module_id, content_hash },
                ModelEgressSourceKind::AuraCore,
            ),
            ActivatedPromptSourceIdentity::CustomSystemAddendum { settings_revision, .. } => (
                ContextSourceRef::RuntimeState {
                    state_kind: "prompt_settings".to_owned(),
                    state_revision: settings_revision,
                },
                ModelEgressSourceKind::CustomSystemAddendum,
            ),
        };
        prompt_fragments.entry(kind).or_default().push(ContextFragment {
            source_ref,
            source_kind,
            data_class: NetworkDataClass::PublicContent,
            authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
            content: source.content,
        });
    }
    for (kind, mut fragments) in prompt_fragments {
        match sections.get_mut(&kind) {
            Some(section) => {
                fragments.append(&mut section.fragments);
                section.fragments = fragments;
            }
            None => {
                sections.insert(
                    kind,
                    ContextSectionInput {
                        kind,
                        fragments,
                        compressibility: ContextCompressibility::NeverCompress,
                        privacy_class: ContextPrivacyClass::ModelVisible,
                    },
                );
            }
        }
    }
    Ok(PromptContextSources {
        active_prompt_module_ids: activated.active_module_ids,
        section_inputs: sections.into_values().collect(),
    })
}

const fn context_section(section: PromptSectionClass) -> ContextSectionKind {
    match section {
        PromptSectionClass::Core
        | PromptSectionClass::Compaction
        | PromptSectionClass::Maintenance => ContextSectionKind::CoreSystemPrompt,
        PromptSectionClass::Permission => ContextSectionKind::PermissionAndSecurity,
        PromptSectionClass::ToolSummary => ContextSectionKind::LoadedToolSchemas,
        PromptSectionClass::WorkspaceAgents => ContextSectionKind::WorkspaceAgentsInstructions,
        PromptSectionClass::HistoricalSnapshot => ContextSectionKind::HistoricalSnapshotSummary,
        PromptSectionClass::AgentMode => ContextSectionKind::AgentModePrompt,
        PromptSectionClass::Hook => ContextSectionKind::HookInjections,
        PromptSectionClass::TaskState => ContextSectionKind::CurrentTaskState,
    }
}

fn map_prompt_error(_error: PromptCompilerFault) -> PromptContextSourceError {
    PromptContextSourceError::Activation
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use aura_context_domain::{ContextPrivacyClass, ModelEgressSourceKind, PromptAgentMode};

    use super::*;

    #[test]
    fn embedded_prompt_and_addendum_are_separate_model_visible_core_sources() {
        let bundle = ValidatedPromptBundle::embedded().expect("prompt bundle");
        let result = assemble_prompt_context_sources(
            &bundle,
            &PromptSettingsSnapshot {
                revision: 3,
                custom_system_addendum: Some("extra rule".to_owned()),
            },
            &PromptActivationInput {
                agent_mode: PromptAgentMode::Normal,
                loaded_capabilities: BTreeSet::new(),
                child_agent: false,
                recovery_kinds: BTreeSet::new(),
                compaction: false,
                maintenance_kind: None,
            },
            Vec::new(),
        )
        .expect("prompt sources");
        assert!(!result.active_prompt_module_ids.is_empty());
        let core = result
            .section_inputs
            .iter()
            .find(|section| section.kind == ContextSectionKind::CoreSystemPrompt)
            .expect("core section");
        assert_eq!(core.privacy_class, ContextPrivacyClass::ModelVisible);
        assert_eq!(
            core.fragments.last().map(|fragment| fragment.source_kind),
            Some(ModelEgressSourceKind::CustomSystemAddendum)
        );
    }
}
