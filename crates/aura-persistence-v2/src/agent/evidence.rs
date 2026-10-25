//! Completion evidence as a model gives it -
//! workspace paths, tests, commands, or an artifact id a tool result showed it. An artifact id must
//! be one of this run's; anything else is the model's own reference, kept as text.
use aura_domain_common::ArtifactId;
use aura_persistence_core::{PersistenceCoreError, StoreResult, artifact};
use rusqlite::Connection;

pub(crate) fn verify<'a>(
    db: &Connection,
    references: impl IntoIterator<Item = &'a String>,
    run: &str,
) -> StoreResult<()> {
    for reference in references {
        if let Ok(id) = reference.trim().parse::<ArtifactId>() {
            // One that is not this run's is the model's to correct, not a
            // damaged store.
            artifact::read(db, id, run).map_err(|error| match error {
                PersistenceCoreError::Invalid(_) => PersistenceCoreError::Invalid(
                    "tool.arguments_invalid: an artifact id in evidence_refs is not one of this run's results; cite the workspace files, tests or commands that show it instead",
                ),
                other => other,
            })?;
        }
    }
    Ok(())
}
