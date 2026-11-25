//! Admit the previous state representation only while its conversation and authority still hold.
use aura_context_domain::{
    ContextSectionKind, ContextSourceRef, ModelEgressAuthorizationBasis, ModelEgressSourceKind,
};
use aura_domain_common::{ContentHash, Revision};
use aura_model_domain::{
    LogicalConversationItem, ModelExecutionSnapshot, TaskStateReplay, TaskStateUpdate,
};
use aura_security_contract::NetworkDataClass;
use serde::Serialize;
use serde_json::Value;

use crate::{
    ContextFragment, ContextPreparationError, ContextPreparationInput, ContextSectionAssembly,
    ModelEgressSourceDraft,
};

pub const ADMITTED_STATE: &str = "agent.admitted_task_state.v1";

pub(crate) fn place(
    input: &ContextPreparationInput,
    assembly: &ContextSectionAssembly,
    items: &[LogicalConversationItem],
    route: &ModelExecutionSnapshot,
) -> Result<Option<TaskStateReplay>, ContextPreparationError> {
    let envelope = &assembly.instruction_envelope;
    if !aura_model_transport::supports_task_state_replay(route, envelope, &input.tool_schema_set) {
        return Ok(None);
    }
    let layout = aura_model_transport::instruction_envelope_layout(envelope);
    let state_sources = assembly
        .sections
        .iter()
        .find(|section| section.kind == ContextSectionKind::CurrentTaskState)
        .ok_or(ContextPreparationError::SectionsInvalid)?;
    let fragments = assembly
        .egress_fragments
        .iter()
        .filter(|f| state_sources.source_refs.contains(&f.source_ref))
        .collect::<Vec<_>>();
    let control = fragments
        .iter()
        .find(|f| core_state(f, "agent.control_state.v1"))
        .map(|f| serde_json::from_str::<Value>(&f.content))
        .transpose()
        .map_err(|_| ContextPreparationError::CanonicalSerialization)?
        .unwrap_or(Value::Null);
    let authority = hash(&(
        "aura.task_state_replay.v1",
        input.request.session_id,
        input.request.invocation_identity.workspace_id,
        route.endpoint_id,
        &route.endpoint_fingerprint,
        &route.resolved_request_url,
        &route.credential_binding_fingerprint,
        &route.model_name,
        &layout.stable,
        &layout.background,
        control_scope(&control),
    ))?;
    let mut active_sources = lineage_grants(&control)?;
    for fragment in fragments {
        if ["agent.control_state.v1", "agent.multi_agent_controls.v1", "agent.web_search_limits.v1"]
            .into_iter()
            .any(|kind| core_state(fragment, kind))
        {
            continue;
        }
        // Runtime revisions can advance even when a file, rule or observation retains the
        // exact content and original authority. Match both bytes and the source identity.
        let source = match &fragment.source_ref {
            ContextSourceRef::RuntimeState { state_kind, .. } => ContextSourceRef::RuntimeState {
                state_kind: state_kind.clone(),
                state_revision: Revision::INITIAL,
            },
            other => other.clone(),
        };
        active_sources.push(hash(&(
            source,
            fragment.source_kind,
            fragment.data_class,
            &fragment.authorization_basis,
            &fragment.content,
        ))?);
    }
    active_sources.sort_unstable_by(|a, b| a.hex.cmp(&b.hex));
    active_sources.dedup();
    let mut current =
        TaskStateReplay { authority, active_sources, leading: layout.current, updates: vec![] };
    let Some(previous) = input.previous_snapshot.as_ref().filter(|snapshot| {
        snapshot.session_id == input.request.session_id
            && snapshot.model_profile_id == input.request.model_profile_id
            && snapshot.tool_schema_set.tools == input.tool_schema_set.tools
            && prefix_matches(&snapshot.conversation_items, items)
            && snapshot.attachments.iter().all(|a| input.attachments.contains(a))
    }) else {
        return Ok(Some(current));
    };
    let Some(admitted) =
        previous.task_state_replay.as_ref().filter(|replay| replay.is_authorized_by(&current))
    else {
        return Ok(Some(current));
    };
    let mut continued = admitted.clone();
    if continued.current() != current.leading {
        let Some(last) = items.last() else { return Ok(Some(current)) };
        continued.updates.push(TaskStateUpdate {
            after: last.source_event_id(),
            content: current.leading.clone(),
        });
    }
    if !continued.fits(items) {
        return Ok(Some(current));
    }
    // Keep all grants involved in anything replayed. Seeing a new source can only extend them.
    current.leading = continued.leading;
    current.updates = continued.updates;
    Ok(Some(current))
}

fn core_state(fragment: &ContextFragment, kind: &str) -> bool {
    matches!(&fragment.source_ref, ContextSourceRef::RuntimeState { state_kind, .. } if state_kind == kind)
        && fragment.source_kind == ModelEgressSourceKind::AuraCore
        && fragment.data_class == NetworkDataClass::PublicContent
        && fragment.authorization_basis == ModelEgressAuthorizationBasis::ModelChannelPolicy
}

fn control_scope(state: &Value) -> impl Serialize + '_ {
    let peers = &state["peer_collaboration"];
    let preview = &peers["directory"]["preview"];
    let visible_peers = preview["peers"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|peer| {
            (&peer["session_id"], &peer["name"], &peer["description"], &peer["can_dispatch"])
        })
        .collect::<Vec<_>>();
    (
        &state["permission_mode"],
        &state["related_source"],
        &peers["scope_update"],
        &preview["graph_id"],
        visible_peers,
    )
}

fn lineage_grants(state: &Value) -> Result<Vec<ContentHash>, ContextPreparationError> {
    state["lineages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|lineage| {
            !matches!(lineage["state"].as_str(), Some("cancelled_by_user" | "expired"))
        })
        .flat_map(|lineage| {
            lineage["sources"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|source| source["membership"] == "active")
                .map(|source| hash(&(&lineage["lineage_id"], source)))
        })
        .collect()
}

fn prefix_matches(
    admitted: &[LogicalConversationItem],
    current: &[LogicalConversationItem],
) -> bool {
    let Some(prefix) = current.get(..admitted.len()) else { return false };
    admitted.iter().zip(prefix).all(|(old, new)| match (old, new) {
        // Revalidate native payloads separately at their producing request boundaries.
        (
            LogicalConversationItem::Assistant {
                source_event_id: old_id,
                content: old_content,
                content_hash: old_hash,
                tool_calls: old_calls,
                ..
            },
            LogicalConversationItem::Assistant {
                source_event_id: new_id,
                content: new_content,
                content_hash: new_hash,
                tool_calls: new_calls,
                ..
            },
        ) => {
            (old_id, old_content, old_hash, old_calls) == (new_id, new_content, new_hash, new_calls)
        }
        _ => old == new,
    })
}

pub(crate) fn admitted_egress(
    replay: &TaskStateReplay,
) -> impl Iterator<Item = ModelEgressSourceDraft> + '_ {
    // The live checkpoint already has one item per contributing source. Non-Core
    // fragments remain admitted only while those original classified items still apply.
    replay.messages().take(replay.updates.len()).enumerate().map(|(index, content)| {
        ModelEgressSourceDraft {
            source_ref: ContextSourceRef::RuntimeState {
                state_kind: ADMITTED_STATE.into(),
                state_revision: Revision::new(index as u64 + 1),
            },
            source_kind: ModelEgressSourceKind::AuraCore,
            data_class: NetworkDataClass::PublicContent,
            authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
            content_hash: ContentHash::sha256(content.as_bytes()),
            byte_length: content.len() as u64,
        }
    })
}

fn hash(value: &impl Serialize) -> Result<ContentHash, ContextPreparationError> {
    let bytes =
        serde_json::to_vec(value).map_err(|_| ContextPreparationError::CanonicalSerialization)?;
    Ok(ContentHash::sha256(&bytes))
}
