//! The current user message as a turn's step sends it.
use aura_context_domain::{ContextSectionKind, ContextSourceRef};
use aura_domain_common::EventId;

use crate::{ContextPreparationError, ContextSectionAssembly};

/// The turn's first step sends its message the way every later
/// step and turn sends it - the user's own text - so the provider's cache of the conversation
/// reaches past it from the second step on. It used to wrap the text in the working-set section,
/// between the file-reference and steered-addition sections, which no producer fills. Should
/// either hold content, the message keeps them, in their sections.
pub(crate) fn current_user_content(
    assembly: &ContextSectionAssembly,
    user_event_id: EventId,
) -> Result<String, ContextPreparationError> {
    match current_user_sections(assembly)? {
        Some(sections) => {
            Ok(sections.into_iter().map(|(_, content)| content).collect::<Vec<_>>().join("\n"))
        }
        None => input_batch_content(assembly, user_event_id),
    }
}

pub(crate) fn current_user_sections(
    assembly: &ContextSectionAssembly,
) -> Result<Option<[(ContextSectionKind, &str); 3]>, ContextPreparationError> {
    let extras = [
        ContextSectionKind::FileReferencesAndPendingAttachments,
        ContextSectionKind::SteeredUserAdditions,
    ];
    if assembly
        .sections
        .iter()
        .filter(|section| extras.contains(&section.kind))
        .all(|section| section.is_empty_marker)
    {
        return Ok(None);
    }
    let kinds = [
        ContextSectionKind::FileReferencesAndPendingAttachments,
        ContextSectionKind::CurrentTurnWorkingSet,
        ContextSectionKind::SteeredUserAdditions,
    ];
    let parts = kinds.map(|kind| {
        assembly
            .sections
            .iter()
            .find(|section| section.kind == kind)
            .map(|section| (kind, section.canonical_content.as_str()))
            .ok_or(ContextPreparationError::SectionsInvalid)
    });
    let [files, working, additions] = parts;
    Ok(Some([files?, working?, additions?]))
}

/// One user event's own text, as the conversation shows it.
pub(crate) fn input_batch_content(
    assembly: &ContextSectionAssembly,
    id: EventId,
) -> Result<String, ContextPreparationError> {
    assembly
        .egress_fragments
        .iter()
        .find(|f| matches!(f.source_ref, ContextSourceRef::Event { event_id } if event_id == id))
        .map(|f| f.content.clone())
        .ok_or(ContextPreparationError::CurrentUserSourceMissing)
}
