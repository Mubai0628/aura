//! The pairing secret of Chrome control, in the Windows Credential Manager. Only Core reads it;
//! the extension receives it once, when pairing succeeds.
use aura_domain_common::CredentialId;
#[cfg(not(test))]
use aura_domain_common::CredentialRef;
use aura_windows_platform::SecretBytes;
#[cfg(not(test))]
use aura_windows_platform::{
    CredentialApplicationAttribute, CredentialStoreError, WindowsCredentialStore,
};

#[cfg(not(test))]
const PURPOSE: &str = "chrome_pairing_v1";

pub(crate) trait ChromeSecrets: Send + Sync {
    fn write(
        &self,
        id: CredentialId,
        extension_id: &str,
        value: &SecretBytes,
    ) -> Result<(), String>;
    /// The secret and the extension it was written for; `None` when there is no value.
    fn read(&self, id: CredentialId) -> Result<Option<(String, SecretBytes)>, String>;
    fn delete(&self, id: CredentialId) -> Result<(), String>;
}

/// `Aura/<namespace hash>/chrome/pairing/<id>`, among the installation's own credentials.
#[cfg(not(test))]
pub(crate) struct WindowsChromeSecrets {
    namespace_hash: String,
}

#[cfg(not(test))]
impl WindowsChromeSecrets {
    pub(crate) fn for_installation(namespace_hash: String) -> Self {
        Self { namespace_hash }
    }

    fn reference(&self, id: CredentialId) -> Result<CredentialRef, String> {
        aura_chrome_domain::protocol::pairing_credential(&self.namespace_hash, id)
            .map_err(|error| format!("配对密钥的存储位置不合法：{error}"))
    }
}

#[cfg(not(test))]
fn describe(error: &CredentialStoreError) -> String {
    match error {
        CredentialStoreError::Windows(error) if error.raw_os_error() == Some(5) => {
            "Windows 拒绝访问凭据管理器".into()
        }
        _ => "凭据管理器读写失败".into(),
    }
}

#[cfg(not(test))]
impl ChromeSecrets for WindowsChromeSecrets {
    fn write(
        &self,
        id: CredentialId,
        extension_id: &str,
        value: &SecretBytes,
    ) -> Result<(), String> {
        let attributes = [("aura.purpose", PURPOSE), ("aura.extension", extension_id)]
            .into_iter()
            .map(|(key, value)| CredentialApplicationAttribute::new(key, value.to_owned()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| describe(&error))?;
        WindowsCredentialStore
            .write_with_attributes(&self.reference(id)?, value, &attributes)
            .map_err(|error| describe(&error))
    }

    fn read(&self, id: CredentialId) -> Result<Option<(String, SecretBytes)>, String> {
        let stored = match crate::credential_io::read(&self.reference(id)?) {
            Ok(stored) => stored,
            Err(CredentialStoreError::NotFound { .. }) => return Ok(None),
            Err(error) => return Err(describe(&error)),
        };
        let attribute = |key: &str| {
            stored.attributes().iter().find(|a| a.keyword() == key).map(|a| a.value().to_owned())
        };
        if attribute("aura.purpose").as_deref() != Some(PURPOSE) {
            return Err("凭据管理器里的这个记录不是 Aura 的 Chrome 配对密钥".into());
        }
        let extension = attribute("aura.extension").ok_or("配对密钥缺少所属扩展")?;
        Ok(Some((extension, stored.into_secret())))
    }

    fn delete(&self, id: CredentialId) -> Result<(), String> {
        match WindowsCredentialStore.delete(&self.reference(id)?) {
            Ok(_) | Err(CredentialStoreError::NotFound { .. }) => Ok(()),
            Err(error) => Err(describe(&error)),
        }
    }
}

/// Core's tests keep the secret in memory; nothing reaches the Credential Manager.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct MemoryChromeSecrets(
    std::sync::Mutex<std::collections::BTreeMap<CredentialId, (String, Vec<u8>)>>,
);

#[cfg(test)]
impl MemoryChromeSecrets {
    pub(crate) fn len(&self) -> usize {
        self.0.lock().map(|values| values.len()).unwrap_or_default()
    }
}

#[cfg(test)]
impl ChromeSecrets for MemoryChromeSecrets {
    fn write(
        &self,
        id: CredentialId,
        extension_id: &str,
        value: &SecretBytes,
    ) -> Result<(), String> {
        let mut values = self.0.lock().map_err(|_| "内存存储损坏".to_owned())?;
        values.insert(id, (extension_id.to_owned(), value.expose().to_vec()));
        Ok(())
    }

    fn read(&self, id: CredentialId) -> Result<Option<(String, SecretBytes)>, String> {
        let values = self.0.lock().map_err(|_| "内存存储损坏".to_owned())?;
        Ok(values
            .get(&id)
            .map(|(extension, value)| (extension.clone(), SecretBytes::new(value.clone()))))
    }

    fn delete(&self, id: CredentialId) -> Result<(), String> {
        self.0.lock().map_err(|_| "内存存储损坏".to_owned())?.remove(&id);
        Ok(())
    }
}
