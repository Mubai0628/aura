//! What an HTTP session sends to sign in, read again for every request so a new secret (or a
//! refreshed token) takes effect at once. The gateway adds the value after its secret scan
//! and scans the answer for it; the value never passes through the session's own headers.
use aura_domain_common::{CredentialId, CredentialRevision, Revision};
use aura_network_control::{NetworkControlError, ResolvedToolCredential, ToolCredentialBinding};
use aura_permission_domain::{NormalizedOrigin, ToolCredentialPlacementV1, ToolCredentialUseV1};
use aura_tool_contract::{CredentialSlotKey, HttpHeaderNameV1};
use std::sync::Arc;
use zeroize::Zeroizing;

/// One request's credential: what the grant names and the value only the gateway reads.
pub(crate) struct RequestCredential {
    pub binding: ToolCredentialUseV1,
    pub resolver: OneRequest,
}

/// Resolves exactly the credential it was built for, once per request.
pub(crate) struct OneRequest {
    binding: ToolCredentialBinding,
    header_value: Zeroizing<Vec<u8>>,
}

impl aura_network_control::ToolCredentialResolver for OneRequest {
    fn resolve(
        &self,
        requested: &ToolCredentialUseV1,
    ) -> Result<ResolvedToolCredential, NetworkControlError> {
        if requested.credential_id != self.binding.credential_id
            || requested.credential_revision != self.binding.credential_revision
        {
            return Err(NetworkControlError::CredentialBindingMismatch);
        }
        Ok(ResolvedToolCredential {
            binding: self.binding.clone(),
            secret_header_value: self.header_value.to_vec(),
        })
    }
}

/// Where a server's credential goes in its requests.
#[derive(Clone, Debug)]
pub(crate) enum Placement {
    /// `Authorization: Bearer <value>`.
    Bearer,
    /// `Authorization: <value>` (for example `Token …`).
    Authorization,
    /// `<header>: <value>`.
    Header(String),
}

impl Placement {
    pub(crate) fn of(header: Option<&str>) -> Self {
        match header {
            None => Self::Bearer,
            Some(name) if name.eq_ignore_ascii_case("authorization") => Self::Authorization,
            Some(name) => Self::Header(name.to_ascii_lowercase()),
        }
    }
}

/// The scheme, host and port the gateway compares a credential's binding with.
pub(crate) fn origin(endpoint: &str) -> Result<NormalizedOrigin, String> {
    let url = url::Url::parse(endpoint).map_err(|_| "地址不合法".to_owned())?;
    let host = match url.host().ok_or("地址没有主机")? {
        url::Host::Domain(name) => name.trim_end_matches('.').to_ascii_lowercase(),
        url::Host::Ipv4(address) => address.to_string(),
        url::Host::Ipv6(address) => address.to_string(),
    };
    let port = url.port_or_known_default().ok_or("地址没有端口")?;
    Ok(NormalizedOrigin { scheme: url.scheme().to_owned(), host, port })
}

/// The credential for one request: `value` placed as `placement`, bound to `origin`.
pub(crate) fn request_credential(
    credential_id: CredentialId,
    revision: u64,
    placement: &Placement,
    origin: NormalizedOrigin,
    value: &[u8],
) -> Result<RequestCredential, String> {
    let revision = CredentialRevision::new(Revision::new(revision))
        .map_err(|_| "凭据的修订号不合法".to_owned())?;
    let (placed, header_name, header_value) = match placement {
        Placement::Bearer => {
            let mut header = b"Bearer ".to_vec();
            header.extend_from_slice(value);
            (ToolCredentialPlacementV1::BearerAuthorization, "authorization".to_owned(), header)
        }
        Placement::Authorization => (
            ToolCredentialPlacementV1::BearerAuthorization,
            "authorization".to_owned(),
            value.to_vec(),
        ),
        Placement::Header(name) => (
            ToolCredentialPlacementV1::FixedHeader {
                header_name: HttpHeaderNameV1::new(name.clone())
                    .map_err(|_| format!("请求头 {name} 不能用来放密钥"))?,
            },
            name.clone(),
            value.to_vec(),
        ),
    };
    let slot_key = CredentialSlotKey::new("mcp").map_err(|_| "凭据槽位不合法".to_owned())?;
    Ok(RequestCredential {
        binding: ToolCredentialUseV1 {
            credential_id,
            credential_revision: revision,
            slot_key,
            placement: placed,
        },
        resolver: OneRequest {
            binding: ToolCredentialBinding {
                credential_id,
                credential_revision: revision,
                origin,
                header_name,
            },
            header_value: Zeroizing::new(header_value),
        },
    })
}

/// Supplies a session's credential for each request.
pub(crate) trait CredentialSource: Send + Sync {
    fn current(&self) -> Result<RequestCredential, String>;
    /// The server refused the credential (HTTP 401). Whether asking again may help: a
    /// sign-in refreshes its token first; a fixed secret would only be refused again.
    fn rejected(&self) -> bool {
        false
    }
}

/// A server's static secret, read from the secret store for every request.
pub(crate) struct StaticSecret {
    pub secrets: Arc<super::McpSecrets>,
    pub server_id: String,
    pub secret_id: CredentialId,
    pub placement: Placement,
    pub origin: NormalizedOrigin,
}

impl CredentialSource for StaticSecret {
    fn current(&self) -> Result<RequestCredential, String> {
        let (revision, value) = self.secrets.value(&self.server_id, self.secret_id)?;
        request_credential(
            self.secret_id,
            revision,
            &self.placement,
            self.origin.clone(),
            value.expose(),
        )
    }
}
