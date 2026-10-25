//! The credentials a server's configuration names (secrets and OAuth sign-ins): whether they
//! are set, what a connection sends, and when they go.
use super::McpRuntime;
use crate::mcp::credentials::{CredentialSource, McpSecrets, Placement, StaticSecret, origin};
use aura_mcp::{McpHttpAuth, McpServerConfig, McpSignInView, McpTransport};
use std::{collections::BTreeSet, sync::Arc};
use zeroize::Zeroizing;

impl McpRuntime {
    /// Removes the secrets and sign-ins no configured server names, counting every plugin's
    /// whether or not it is ready now. Skipped while `mcp.json` or `plugins.json` cannot
    /// be read, so a damaged file never costs the user their secrets.
    pub(crate) fn sweep_credentials(&self) {
        let registry = self.settings.mcp_registry();
        if registry.issue.is_some() {
            return;
        }
        let Some((plugin_secrets, plugin_servers)) = self.plugin_credentials() else { return };
        let mut named: BTreeSet<_> =
            registry.servers.iter().flat_map(|server| server.transport.secret_ids()).collect();
        named.extend(plugin_secrets);
        self.secrets.sweep(&named);
        let mut servers: Vec<String> = registry.servers.iter().map(|s| s.id.clone()).collect();
        servers.extend(plugin_servers);
        self.oauth.sweep(&servers);
    }

    /// A secret of `id` was saved: a trusted, switched-on server lists its tools again with it
    /// (a plugin's server whose first listing waited for the secret, or a changed secret).
    /// Running instances keep going; the ones started next get the new value.
    pub(crate) fn secret_written(self: &Arc<Self>, id: &str) {
        let server = self.configured().servers.into_iter().find(|server| {
            server.id == id && server.enabled && server.trusted_fingerprint.is_some()
        });
        if let Some(server) = server {
            self.discover_later(server);
        }
    }

    fn uses_oauth(server: &McpServerConfig) -> bool {
        matches!(
            &server.transport,
            McpTransport::Http { auth: Some(McpHttpAuth::OAuth { .. }), .. }
        )
    }

    /// Whether `server` signs in with OAuth and has no sign-in for `fingerprint`.
    pub(crate) fn needs_sign_in(
        &self,
        server: &McpServerConfig,
        fingerprint: Option<&str>,
    ) -> bool {
        Self::uses_oauth(server)
            && !fingerprint.is_some_and(|fingerprint| self.oauth.signed_in(&server.id, fingerprint))
    }

    pub(crate) fn sign_in_view(
        &self,
        server: &McpServerConfig,
        fingerprint: Option<&str>,
    ) -> Option<McpSignInView> {
        Self::uses_oauth(server).then(|| self.oauth.view(&server.id, fingerprint))
    }

    /// The fingerprint a sign-in is made for: the server must be trusted, on and unchanged.
    pub(crate) fn ready_for_sign_in(&self, server: &McpServerConfig) -> Result<String, String> {
        let checked = crate::mcp::launch::check(server, &self.hashes);
        if !checked.problems.is_empty() {
            return Err(checked.problems.join("；"));
        }
        match (&server.trusted_fingerprint, checked.fingerprint) {
            (None, _) => Err("请先信任这个服务器，再登录".into()),
            (Some(trusted), Some(current)) if *trusted == current && server.enabled => Ok(current),
            (Some(_), Some(_)) if !server.enabled => Err("服务器已停用，请先启用".into()),
            _ => Err("服务器的配置在信任之后变了，请先重新信任".into()),
        }
    }

    /// Forgets a server's sign-in and stops its running instances.
    pub(crate) fn sign_out(&self, server_id: &str) {
        if let Err(reason) = self.oauth.sign_out(server_id) {
            crate::append_core_log(
                "mcp.sign_out_failed",
                serde_json::json!({"server": server_id, "reason": reason}),
            );
        }
        self.stop_server(server_id);
    }

    pub(crate) fn oauth(&self) -> &std::sync::Arc<crate::mcp::oauth::McpOAuth> {
        &self.oauth
    }

    /// Why `server` cannot be reached yet for its credentials, read in `db` (the caller's own
    /// store access): a secret it names was never set, or it signs in with OAuth and has no
    /// sign-in for `fingerprint`.
    pub(super) fn credential_gap(
        &self,
        server: &McpServerConfig,
        fingerprint: Option<&str>,
        db: &aura_persistence_core::Connection,
    ) -> Option<String> {
        if let Some(missing) = Self::missing_secret_in(server, db) {
            return Some(format!("服务器 {} 的{missing}", server.id));
        }
        let signed_in = fingerprint.is_some_and(|fingerprint| {
            crate::mcp::oauth::signed_in_in(db, &server.id, fingerprint)
        });
        (Self::uses_oauth(server) && !signed_in)
            .then(|| format!("服务器 {} 需要先在设置里登录", server.id))
    }

    /// Why the server cannot be reached yet, when a secret it names was never set.
    pub(crate) fn missing_secret(&self, server: &McpServerConfig) -> Option<String> {
        match self.journal.lock() {
            Ok(journal) => Self::missing_secret_in(server, journal.database.connection()),
            Err(_) => Some("密钥的状态无法读取".to_owned()),
        }
    }

    fn missing_secret_in(
        server: &McpServerConfig,
        db: &aura_persistence_core::Connection,
    ) -> Option<String> {
        let set = |id| McpSecrets::is_set_in(db, &server.id, id);
        let missing: Vec<String> = match &server.transport {
            McpTransport::Stdio { secret_environment, .. } => secret_environment
                .iter()
                .filter(|entry| !set(entry.secret_id))
                .map(|entry| entry.name.clone())
                .collect(),
            McpTransport::Http { auth: Some(McpHttpAuth::Secret { secret_id, .. }), .. } => {
                if set(*secret_id) { Vec::new() } else { vec!["认证密钥".to_owned()] }
            }
            McpTransport::Http { .. } => Vec::new(),
        };
        (!missing.is_empty()).then(|| format!("密钥还没有设置：{}", missing.join("、")))
    }

    /// What an HTTP connection to `server` signs in with, read again for every request.
    pub(super) fn http_credential(
        &self,
        server: &McpServerConfig,
    ) -> Result<Option<Arc<dyn CredentialSource>>, String> {
        let McpTransport::Http { url, auth, .. } = &server.transport else {
            return Ok(None);
        };
        match auth {
            None => Ok(None),
            Some(McpHttpAuth::Secret { secret_id, header }) => Ok(Some(Arc::new(StaticSecret {
                secrets: Arc::clone(&self.secrets),
                server_id: server.id.clone(),
                secret_id: *secret_id,
                placement: Placement::of(header.as_deref()),
                origin: origin(url)?,
            }))),
            Some(McpHttpAuth::OAuth { .. }) => {
                let fingerprint = server.trusted_fingerprint.as_deref().ok_or("服务器尚未信任")?;
                Ok(Some(self.oauth.source(server, fingerprint)?))
            }
        }
    }

    /// The secret environment variables a program server starts with.
    pub(super) fn secret_environment(
        &self,
        server: &McpServerConfig,
    ) -> Result<Vec<(String, Zeroizing<String>)>, String> {
        let McpTransport::Stdio { secret_environment, .. } = &server.transport else {
            return Ok(Vec::new());
        };
        secret_environment
            .iter()
            .map(|entry| {
                let (_, value) = self.secrets.value(&server.id, entry.secret_id)?;
                let text = std::str::from_utf8(value.expose())
                    .map_err(|_| format!("环境变量 {} 的密钥不是文字", entry.name))?;
                Ok((entry.name.clone(), Zeroizing::new(text.to_owned())))
            })
            .collect()
    }

    /// The secrets `before` named and `after` no longer does go (a changed or removed server).
    pub(crate) fn release_secrets(
        &self,
        before: Option<&McpServerConfig>,
        after: Option<&McpServerConfig>,
    ) {
        let kept: BTreeSet<_> = after
            .map(|server| server.transport.secret_ids())
            .unwrap_or_default()
            .into_iter()
            .collect();
        for id in before.map(|server| server.transport.secret_ids()).unwrap_or_default() {
            if kept.contains(&id) {
                continue;
            }
            if let Err(reason) = self.secrets.forget(id) {
                crate::append_core_log(
                    "mcp.secret_cleanup_failed",
                    serde_json::json!({"secret_id": id, "reason": reason}),
                );
            }
        }
    }

    pub(crate) fn secrets(&self) -> &crate::mcp::credentials::McpSecrets {
        &self.secrets
    }
}
