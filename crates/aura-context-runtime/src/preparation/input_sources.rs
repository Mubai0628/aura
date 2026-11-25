use super::*;

fn peer_fragment(fragment: &crate::ContextFragment, event_id: EventId) -> bool {
    fragment.source_ref == ContextSourceRef::Event { event_id }
        && fragment.source_kind == ModelEgressSourceKind::PeerTask
        && fragment.authorization_basis == ModelEgressAuthorizationBasis::ModelChannelPolicy
}

pub(super) fn peer_item(
    assembly: &ContextSectionAssembly,
    event_id: EventId,
    input: &ContextPreparationInput,
    conversation_empty: bool,
) -> Result<LogicalConversationItem, ContextPreparationError> {
    if input.tool_continuation_facts.is_some()
        || input.core_continuation_facts.is_some()
        || !conversation_empty
        || !input.attachments.is_empty()
    {
        return Err(ContextPreparationError::ConversationInvalid);
    }
    let fragment = assembly
        .egress_fragments
        .iter()
        .find(|fragment| peer_fragment(fragment, event_id))
        .ok_or(ContextPreparationError::ConversationInvalid)?;
    // The data message goes into the Provider's user slot, but its sealed attribution and
    // authorizations are those of the peer envelope, never of authenticated user input.
    Ok(LogicalConversationItem::User {
        source_event_id: event_id,
        content_hash: ContentHash::sha256(fragment.content.as_bytes()),
        content: fragment.content.clone(),
    })
}

pub(super) fn validate_current_user_source(
    assembly: &ContextSectionAssembly,
    event_id: EventId,
) -> Result<(), ContextPreparationError> {
    let present = assembly.egress_fragments.iter().any(|fragment| {
        fragment.source_ref == ContextSourceRef::Event { event_id }
            && fragment.source_kind == ModelEgressSourceKind::UserEvent
            && matches!(fragment.authorization_basis, ModelEgressAuthorizationBasis::UserSubmission)
    });
    present.then_some(()).ok_or(ContextPreparationError::CurrentUserSourceMissing)
}

pub(super) fn validate_recent_history_sources(
    assembly: &ContextSectionAssembly,
    items: &[LogicalConversationItem],
) -> Result<(), ContextPreparationError> {
    for item in items {
        let event_id = conversation_source_event_id(item);
        let expected = match item {
            LogicalConversationItem::User { .. } => ModelEgressSourceKind::UserEvent,
            LogicalConversationItem::Assistant { .. } => ModelEgressSourceKind::AssistantEvent,
            LogicalConversationItem::ToolResult { .. } => ModelEgressSourceKind::ToolResult,
        };
        if !assembly.egress_fragments.iter().any(|fragment| {
            (fragment.source_ref == ContextSourceRef::Event { event_id }
                && fragment.source_kind == expected)
                || (expected == ModelEgressSourceKind::UserEvent
                    && peer_fragment(fragment, event_id))
        }) {
            return Err(ContextPreparationError::ConversationInvalid);
        }
    }
    Ok(())
}
