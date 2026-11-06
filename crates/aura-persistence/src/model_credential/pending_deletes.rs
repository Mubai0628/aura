//! The credential deletes a completed model-config change left open, because Windows Credential
//! Manager had not deleted those credentials yet; the cleanup retries them.
use std::str::FromStr;

use aura_domain_common::MutationJournalId;
use rusqlite::Connection;

use super::{CredentialMutationJournalRecord, load_credential_journal};
use crate::{ModelConfigMutationJournalState, PersistenceCoordinator, PersistenceError};

impl PersistenceCoordinator {
    /// The credential deletes still open whose model-config change completed without them: each
    /// is resumed under its own credential lease, never the model-config one.
    pub fn pending_credential_deletes(
        &self,
    ) -> Result<Vec<CredentialMutationJournalRecord>, PersistenceError> {
        let deletes = self.with_reader(load_incomplete_credential_deletes)?;
        let mut pending = Vec::with_capacity(deletes.len());
        for delete in deletes {
            let Some(parent) = delete.parent_journal_id else { continue };
            if self
                .model_config_mutation_journal(parent)?
                .is_some_and(|parent| parent.state == ModelConfigMutationJournalState::Completed)
            {
                pending.push(delete);
            }
        }
        Ok(pending)
    }
}

fn load_incomplete_credential_deletes(
    connection: &Connection,
) -> Result<Vec<CredentialMutationJournalRecord>, PersistenceError> {
    let mut statement = connection.prepare(
        "SELECT journal_id
         FROM mutation_journals
         WHERE journal_kind='credential_delete'
           AND state NOT IN ('completed', 'aborted', 'reconciliation_required')
         ORDER BY created_at, journal_id",
    )?;
    let ids =
        statement.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    ids.into_iter()
        .map(|value| {
            load_credential_journal(connection, MutationJournalId::from_str(&value)?)?
                .ok_or(PersistenceError::CredentialJournalNotFound)
        })
        .collect()
}

#[cfg(test)]
mod tests;
