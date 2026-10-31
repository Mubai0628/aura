//! Where an MCP secret's value lives: the Windows Credential Manager in the product, memory
//! in Core's own tests. Each record carries its purpose, server and revision, so a value is
//! never used for another server or after a newer one was written.
use aura_core_base::credential_io;
use aura_domain_common::{CredentialId, CredentialRef};
use aura_windows_platform::{
    CredentialApplicationAttribute, CredentialStoreError, SecretBytes, WindowsCredentialStore,
};
#[cfg(any(test, feature = "test-hooks"))]
use std::{collections::BTreeMap, sync::Mutex};

const PURPOSE: &str = "mcp_secret_v1";

/// One stored value with the facts it was written with.
pub struct StoredSecret {
    pub server_id: String,
    pub revision: u64,
    pub value: SecretBytes,
}

pub trait SecretStore: Send + Sync {
    fn write(
        &self,
        id: CredentialId,
        server_id: &str,
        revision: u64,
        value: &SecretBytes,
    ) -> Result<(), String>;
    /// `None` when there is no value.
    fn read(&self, id: CredentialId) -> Result<Option<StoredSecret>, String>;
    fn delete(&self, id: CredentialId) -> Result<(), String>;
}

/// The Windows Credential Manager under `Aura/mcp/secret/<id>`, or under an isolated test
/// namespace (`Aura/test/<run>/<id>`).
pub struct WindowsSecretStore {
    test_run: Option<String>,
}

impl WindowsSecretStore {
    pub fn product() -> Self {
        Self { test_run: None }
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub fn isolated(run: &str) -> Self {
        Self { test_run: Some(run.to_owned()) }
    }

    fn reference(&self, id: CredentialId) -> Result<CredentialRef, String> {
        match &self.test_run {
            None => format!("wincred://aura/mcp/secret/{id}").parse(),
            Some(run) => CredentialRef::for_test(run, &id.to_string()),
        }
        .map_err(|error| format!("密钥的存储位置不合法：{error}"))
    }
}

fn describe(error: &CredentialStoreError) -> String {
    match error {
        CredentialStoreError::Windows(error) if error.raw_os_error() == Some(5) => {
            "Windows 拒绝访问凭据管理器".into()
        }
        CredentialStoreError::NotFound { .. } => "凭据管理器里没有这个密钥".into(),
        _ => "凭据管理器读写失败".into(),
    }
}

impl SecretStore for WindowsSecretStore {
    fn write(
        &self,
        id: CredentialId,
        server_id: &str,
        revision: u64,
        value: &SecretBytes,
    ) -> Result<(), String> {
        let attributes = [
            ("aura.purpose", PURPOSE.to_owned()),
            ("aura.server", server_id.to_owned()),
            ("aura.revision", revision.to_string()),
        ]
        .into_iter()
        .map(|(key, value)| CredentialApplicationAttribute::new(key, value))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| describe(&error))?;
        WindowsCredentialStore
            .write_with_attributes(&self.reference(id)?, value, &attributes)
            .map_err(|error| describe(&error))
    }

    fn read(&self, id: CredentialId) -> Result<Option<StoredSecret>, String> {
        let stored = match credential_io::read(&self.reference(id)?) {
            Ok(stored) => stored,
            Err(CredentialStoreError::NotFound { .. }) => return Ok(None),
            Err(error) => return Err(describe(&error)),
        };
        let attribute = |key: &str| {
            stored.attributes().iter().find(|a| a.keyword() == key).map(|a| a.value().to_owned())
        };
        if attribute("aura.purpose").as_deref() != Some(PURPOSE) {
            return Err("凭据管理器里的这个记录不是 Aura 的 MCP 密钥".into());
        }
        let server_id = attribute("aura.server").ok_or("MCP 密钥缺少所属服务器")?;
        let revision = attribute("aura.revision")
            .and_then(|revision| revision.parse().ok())
            .ok_or("MCP 密钥缺少修订号")?;
        Ok(Some(StoredSecret { server_id, revision, value: stored.into_secret() }))
    }

    fn delete(&self, id: CredentialId) -> Result<(), String> {
        WindowsCredentialStore.delete(&self.reference(id)?).map(|_| ()).map_err(|e| describe(&e))
    }
}

/// A value kept in memory with the server and revision it was written for.
#[cfg(any(test, feature = "test-hooks"))]
type MemoryEntry = (String, u64, Vec<u8>);

/// Core's tests keep values in memory; nothing reaches the Credential Manager.
#[cfg(any(test, feature = "test-hooks"))]
#[derive(Default)]
pub struct MemorySecretStore(Mutex<BTreeMap<CredentialId, MemoryEntry>>);

#[cfg(any(test, feature = "test-hooks"))]
impl SecretStore for MemorySecretStore {
    fn write(
        &self,
        id: CredentialId,
        server_id: &str,
        revision: u64,
        value: &SecretBytes,
    ) -> Result<(), String> {
        let mut values = self.0.lock().map_err(|_| "内存存储损坏".to_owned())?;
        values.insert(id, (server_id.to_owned(), revision, value.expose().to_vec()));
        Ok(())
    }

    fn read(&self, id: CredentialId) -> Result<Option<StoredSecret>, String> {
        let values = self.0.lock().map_err(|_| "内存存储损坏".to_owned())?;
        Ok(values.get(&id).map(|(server_id, revision, value)| StoredSecret {
            server_id: server_id.clone(),
            revision: *revision,
            value: SecretBytes::new(value.clone()),
        }))
    }

    fn delete(&self, id: CredentialId) -> Result<(), String> {
        self.0.lock().map_err(|_| "内存存储损坏".to_owned())?.remove(&id);
        Ok(())
    }
}

#[cfg(any(test, feature = "test-hooks"))]
impl MemorySecretStore {
    #[allow(
        clippy::len_without_is_empty,
        reason = "a store for Core's tests, which count its values and never ask if it is empty"
    )]
    pub fn len(&self) -> usize {
        self.0.lock().map(|values| values.len()).unwrap_or_default()
    }

    /// Loses one value behind Aura's back (as if the Credential Manager entry were deleted).
    pub fn forget_value(&self, id: CredentialId) {
        if let Ok(mut values) = self.0.lock() {
            values.remove(&id);
        }
    }
}
