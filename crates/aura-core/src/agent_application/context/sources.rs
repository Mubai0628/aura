//! Frozen prompt material and model-visible history source classification.
use super::*;
mod prompt_state;
pub(super) use prompt_state::prompt_state;

pub(in crate::agent_application) fn core_tool(
    name: &str,
    mode: AgentTaskMode,
    purpose: AgentTaskPurpose,
) -> bool {
    (purpose == AgentTaskPurpose::WorkspaceInit && name == "workspace_init.complete")
        || name.starts_with("tools.")
        // The model can always find, load and read the user's trusted Skills.
        || matches!(name, "skills.search" | "skills.load" | "skills.read_resource")
        || matches!(
            name,
            "task_lineage.list_open"
                | "task_lineage.reconcile"
                | "task_lineage.review_completion"
                | "task_lineage.read_sources"
                | "normal.complete"
                | "query.user_question"
                | "mode.disposition"
        )
        || match mode {
            AgentTaskMode::Normal => false,
            AgentTaskMode::Plan => name == "plan.complete",
            AgentTaskMode::Goal => matches!(name, "goal.check" | "goal.complete"),
        }
}

pub(in crate::agent_application) fn section(
    kind: ContextSectionKind,
    fragments: Vec<ContextFragment>,
    compressibility: ContextCompressibility,
) -> ContextSectionInput {
    ContextSectionInput {
        kind,
        fragments,
        compressibility,
        privacy_class: ContextPrivacyClass::ModelVisible,
    }
}
pub(in crate::agent_application) fn history_fragment(
    item: &LogicalConversationItem,
) -> anyhow::Result<ContextFragment> {
    let (event, kind, data, basis, content) = match item {
        LogicalConversationItem::User { source_event_id, content, .. } => (
            *source_event_id,
            ModelEgressSourceKind::UserEvent,
            NetworkDataClass::UserFileContent,
            ModelEgressAuthorizationBasis::UserSubmission,
            content.clone(),
        ),
        LogicalConversationItem::Assistant { source_event_id, .. } => (
            *source_event_id,
            ModelEgressSourceKind::AssistantEvent,
            NetworkDataClass::PublicContent,
            ModelEgressAuthorizationBasis::ModelChannelPolicy,
            serde_json::to_string(item)?,
        ),
        LogicalConversationItem::ToolResult { source_event_id, tool_name, content, .. } => (
            *source_event_id,
            ModelEgressSourceKind::ToolResult,
            if tool_name == "agent.activity.query" {
                super::super::activity::query_history_class(content)?
            } else {
                NetworkDataClass::WorkspaceSource
            },
            ModelEgressAuthorizationBasis::ModelChannelPolicy,
            content.clone(),
        ),
    };
    Ok(ContextFragment {
        source_ref: ContextSourceRef::Event { event_id: event },
        source_kind: kind,
        data_class: data,
        authorization_basis: basis,
        content,
    })
}
pub(in crate::agent_application) fn frozen_prompt(
    bundle: &PromptBundleSnapshot,
    load: impl Fn(ArtifactId) -> anyhow::Result<Vec<u8>>,
) -> anyhow::Result<(ValidatedPromptBundle, aura_config::PromptSettingsSnapshot)> {
    let read = |id, hash: &ContentHash| -> anyhow::Result<String> {
        let bytes = load(id)?;
        anyhow::ensure!(ContentHash::sha256(&bytes) == *hash, "agent.prompt_hash");
        Ok(String::from_utf8(bytes)?)
    };
    let modules = bundle
        .module_snapshots
        .iter()
        .map(|m| {
            Ok(ValidatedPromptModule {
                module_id: m.module_id.clone(),
                module_version: m.module_version.clone(),
                path: format!("artifact:{}", m.content_artifact_id),
                required: true,
                section: m.section,
                activation: m.activation.clone(),
                order: m.order,
                content_hash: m.content_hash.clone(),
                normalized_content: read(m.content_artifact_id, &m.content_hash)?,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let custom_system_addendum = bundle
        .custom_addendum_artifact_id
        .zip(bundle.custom_addendum_hash.as_ref())
        .map(|(id, h)| read(id, h))
        .transpose()?;
    Ok((
        ValidatedPromptBundle {
            schema_version: 1,
            bundle_version: bundle.bundle_version.clone(),
            serializer_version: bundle.serializer_version,
            manifest_hash: bundle.bundle_manifest_hash.clone(),
            modules,
        },
        aura_config::PromptSettingsSnapshot {
            revision: bundle.settings_revision.value(),
            custom_system_addendum,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_mode_exposes_lineage_resolution_before_ordinary_tools() {
        let ordinary = AgentTaskPurpose::Conversation;
        for mode in [AgentTaskMode::Normal, AgentTaskMode::Plan, AgentTaskMode::Goal] {
            assert!(core_tool("task_lineage.list_open", mode, ordinary));
            assert!(core_tool("task_lineage.read_sources", mode, ordinary));
            assert!(core_tool("task_lineage.reconcile", mode, ordinary));
            assert!(core_tool("normal.complete", mode, ordinary));
            assert!(core_tool("query.user_question", mode, ordinary));
            assert!(core_tool("mode.disposition", mode, ordinary));
            assert_eq!(core_tool("plan.complete", mode, ordinary), mode == AgentTaskMode::Plan);
            assert_eq!(core_tool("goal.complete", mode, ordinary), mode == AgentTaskMode::Goal);
            assert!(!core_tool("workspace_init.complete", mode, ordinary));
        }
    }

    #[test]
    fn only_an_init_task_exposes_its_write_and_it_keeps_the_ordinary_tools() {
        let init = AgentTaskPurpose::WorkspaceInit;
        assert!(core_tool("workspace_init.complete", AgentTaskMode::Normal, init));
        assert!(core_tool("normal.complete", AgentTaskMode::Normal, init));
        assert!(core_tool("query.user_question", AgentTaskMode::Normal, init));
        assert!(!core_tool("plan.complete", AgentTaskMode::Normal, init));
    }
}
