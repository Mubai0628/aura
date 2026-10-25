//! The MCP secrets of this process. A value arrives only through the dedicated secret input,
//! is registered before it is written and confirmed after it is read back, is read only by
//! Core when it reaches the server, and goes when the server stops naming it.
use super::store::SecretStore;
use aura_domain_common::{CredentialId, UtcTimestamp};
use aura_persistence_v2::mcp::{
    SecretRecord, SecretState, forget_secret, register_secret, secret, secret_stored, secrets,
};
use aura_windows_platform::SecretBytes;
use std::{collections::BTreeSet, sync::Arc};

pub(crate) struct McpSecrets {
    store: Arc<dyn SecretStore>,
    journal: Arc<crate::broker::BrokerJournal>,
}

impl McpSecrets {
    pub(crate) fn new(
        store: Arc<dyn SecretStore>,
        journal: Arc<crate::broker::BrokerJournal>,
    ) -> Self {
        Self { store, journal }
    }

    fn write<T>(
        &self,
        change: impl FnOnce(
            &aura_persistence_core::Transaction<'_>,
        ) -> aura_persistence_core::StoreResult<T>,
    ) -> Result<T, String> {
        let mut journal = self.journal.lock().map_err(|error| error.to_string())?;
        journal.database.unit_of_work(change).map_err(|error| error.to_string())
    }

    fn record(&self, id: CredentialId) -> Result<Option<SecretRecord>, String> {
        let journal = self.journal.lock().map_err(|error| error.to_string())?;
        secret(journal.database.connection(), id).map_err(|error| error.to_string())
    }

    /// Writes a value for `secret_id` (a new secret when absent) of `server_id`; returns the ID
    /// the server's configuration names it by.
    pub(crate) fn set(
        &self,
        server_id: &str,
        secret_id: Option<CredentialId>,
        value: &SecretBytes,
    ) -> Result<CredentialId, String> {
        // No ID yet: a new one (the default ID is a fresh random one).
        let id = secret_id.unwrap_or_default();
        let registered = self
            .write(|tx| register_secret(tx, id, server_id, UtcTimestamp::now()))
            .map_err(|_| "这个密钥不属于这个服务器，或者无法登记".to_owned())?;
        self.store.write(id, server_id, registered.revision, value)?;
        let stored = self.store.read(id)?.ok_or("写入后读不到这个密钥")?;
        if stored.server_id != server_id || stored.revision != registered.revision {
            return Err("写入的密钥与登记不一致，请重试".into());
        }
        self.write(|tx| secret_stored(tx, id, registered.revision, UtcTimestamp::now()))?;
        Ok(id)
    }

    /// The current value of a server's secret and its revision. Only Core's connection code
    /// calls this, for one request.
    pub(crate) fn value(
        &self,
        server_id: &str,
        secret_id: CredentialId,
    ) -> Result<(u64, SecretBytes), String> {
        let record = self.record(secret_id)?.ok_or("密钥还没有设置")?;
        if record.server_id != server_id {
            return Err("这个密钥不属于这个服务器".into());
        }
        if record.state != SecretState::Stored {
            return Err("密钥没有写完，请在设置里重新设置".into());
        }
        let stored = self.store.read(secret_id)?.ok_or("凭据管理器里没有这个密钥，请重新设置")?;
        if stored.server_id != server_id || stored.revision != record.revision {
            return Err("凭据管理器里的密钥与登记不一致，请重新设置".into());
        }
        Ok((record.revision, stored.value))
    }

    /// Whether the server's secret was written, read in `db` (the caller's own store access;
    /// nothing is read from the Credential Manager).
    pub(crate) fn is_set_in(
        db: &aura_persistence_core::Connection,
        server_id: &str,
        secret_id: CredentialId,
    ) -> bool {
        matches!(
            secret(db, secret_id),
            Ok(Some(record)) if record.server_id == server_id && record.state == SecretState::Stored
        )
    }

    /// Whether `secret_id` was registered for `server_id` (a configuration may name it).
    pub(crate) fn belongs_to(&self, server_id: &str, secret_id: CredentialId) -> bool {
        matches!(self.record(secret_id), Ok(Some(record)) if record.server_id == server_id)
    }

    pub(crate) fn forget(&self, secret_id: CredentialId) -> Result<(), String> {
        self.store.delete(secret_id)?;
        self.write(|tx| forget_secret(tx, secret_id))
    }

    /// Removes every secret no configured server names: a value set in a form that was never
    /// saved, or one whose server was removed while its cleanup failed.
    pub(crate) fn sweep(&self, named: &BTreeSet<CredentialId>) {
        let known = match self.journal.lock() {
            Ok(journal) => secrets(journal.database.connection()).unwrap_or_default(),
            Err(_) => return,
        };
        for record in known.into_iter().filter(|record| !named.contains(&record.secret_id)) {
            if let Err(reason) = self.forget(record.secret_id) {
                crate::append_core_log(
                    "mcp.secret_cleanup_failed",
                    serde_json::json!({"secret_id": record.secret_id, "reason": reason}),
                );
            }
        }
    }
}
