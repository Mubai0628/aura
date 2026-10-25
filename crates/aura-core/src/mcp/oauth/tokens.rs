//! A server's sign-in, kept for the current Windows user: the authorization server and client
//! it used (not secret) and its tokens, encrypted with DPAPI bound to the server and the
//! trusted configuration. A sign-in made for another configuration is never used.
use super::discovery::AuthorizationServer;
use aura_domain_common::{CredentialId, UtcTimestamp};
use aura_persistence_v2::mcp::{OAuthSession, forget_oauth, oauth, oauth_servers, save_oauth};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

/// Where the tokens come from and what they are for (nothing secret).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Authorization {
    pub server: AuthorizationServer,
    pub client_id: String,
    pub redirect_uri: String,
    pub resource: String,
    pub scopes: Vec<String>,
}

#[derive(Deserialize, Serialize)]
pub(super) struct Tokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
}

impl Drop for Tokens {
    fn drop(&mut self) {
        self.access_token.zeroize();
        if let Some(refresh) = self.refresh_token.as_mut() {
            refresh.zeroize();
        }
    }
}

/// One stored sign-in, decrypted.
pub(super) struct Signed {
    pub session: OAuthSession,
    pub authorization: Authorization,
    pub tokens: Tokens,
}

fn entropy(server_id: &str, fingerprint: &str) -> Vec<u8> {
    format!("aura.mcp.oauth.v1\0{server_id}\0{fingerprint}").into_bytes()
}

/// Stores a sign-in. `expires_in` (seconds) is kept a little short, so a token is refreshed
/// before the server would refuse it.
pub(super) fn save(
    journal: &crate::broker::BrokerJournal,
    server_id: &str,
    fingerprint: &str,
    (credential_id, revision): (CredentialId, u64),
    authorization: &Authorization,
    tokens: &Tokens,
    expires_in: Option<u64>,
) -> Result<(), String> {
    let plain = Zeroizing::new(serde_json::to_vec(tokens).map_err(|e| e.to_string())?);
    let protected =
        aura_windows_platform::protect_for_current_user(&plain, &entropy(server_id, fingerprint))
            .map_err(|error| format!("令牌无法加密保存：{error}"))?;
    let now = UtcTimestamp::now();
    let expires_at = expires_in.map(|seconds| {
        let seconds = i64::try_from(seconds).unwrap_or(i64::MAX / 2).saturating_sub(30).max(1);
        UtcTimestamp::from_value(now.value() + time::Duration::seconds(seconds))
    });
    let session = OAuthSession {
        server_id: server_id.to_owned(),
        config_fingerprint: fingerprint.to_owned(),
        credential_id,
        revision,
        authorization: serde_json::to_value(authorization).map_err(|e| e.to_string())?,
        protected_tokens: protected,
        expires_at,
        updated_at: now,
    };
    let mut journal = journal.lock().map_err(|error| error.to_string())?;
    journal.database.unit_of_work(|tx| save_oauth(tx, &session)).map_err(|e| e.to_string())
}

/// Whether a sign-in for this configuration is stored (nothing is decrypted).
pub(super) fn exists(
    journal: &crate::broker::BrokerJournal,
    server_id: &str,
    fingerprint: &str,
) -> bool {
    journal
        .lock()
        .is_ok_and(|journal| exists_in(journal.database.connection(), server_id, fingerprint))
}

/// [`exists`] in the caller's own store access.
pub(super) fn exists_in(
    db: &aura_persistence_core::Connection,
    server_id: &str,
    fingerprint: &str,
) -> bool {
    oauth(db, server_id)
        .ok()
        .flatten()
        .is_some_and(|session| session.config_fingerprint == fingerprint)
}

/// The stored sign-in for this configuration, decrypted.
pub(super) fn load(
    journal: &crate::broker::BrokerJournal,
    server_id: &str,
    fingerprint: &str,
) -> Result<Option<Signed>, String> {
    let session = {
        let journal = journal.lock().map_err(|error| error.to_string())?;
        oauth(journal.database.connection(), server_id).map_err(|e| e.to_string())?
    };
    let Some(session) = session.filter(|session| session.config_fingerprint == fingerprint) else {
        return Ok(None);
    };
    let plain = aura_windows_platform::unprotect_for_current_user(
        &session.protected_tokens,
        &entropy(server_id, fingerprint),
    )
    .map_err(|_| "保存的登录无法解密（可能来自另一个 Windows 用户），请重新登录".to_owned())?;
    let tokens: Tokens =
        serde_json::from_slice(plain.expose()).map_err(|_| "保存的登录已损坏，请重新登录")?;
    let authorization: Authorization = serde_json::from_value(session.authorization.clone())
        .map_err(|_| "保存的登录已损坏，请重新登录")?;
    Ok(Some(Signed { session, authorization, tokens }))
}

pub(super) fn forget(
    journal: &crate::broker::BrokerJournal,
    server_id: &str,
) -> Result<(), String> {
    let mut journal = journal.lock().map_err(|error| error.to_string())?;
    journal.database.unit_of_work(|tx| forget_oauth(tx, server_id)).map_err(|e| e.to_string())
}

pub(super) fn servers(journal: &crate::broker::BrokerJournal) -> Vec<String> {
    journal
        .lock()
        .ok()
        .and_then(|journal| oauth_servers(journal.database.connection()).ok())
        .unwrap_or_default()
}
