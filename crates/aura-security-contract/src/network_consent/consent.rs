use crate::{
    AuthorizationGrant, AuthorizationLifetime, InvocationIdentity, OperationFingerprint,
    ResolvedCapability, SecuritySourceRef,
};
use aura_domain_common::{
    ApprovalRequestId, PermissionGrantId, PrincipalId, Revision, SessionId, ToolWireVersionV1,
    WorkspaceId,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkConsentCaller {
    OrdinaryTools,
    BuiltInShells,
    Git,
    // Installed package tools. Never shares an answer with built-in tools.
    PackageTools,
    // The built-in browser. Its answers name one site and cover nothing else.
    AuraBrowser,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkConsentDestinations {
    AllOrdinaryDestinations,
    Git {
        target: crate::GitRemoteTargetV1,
    },
    /// One installed package version in one session: the tools of that package may
    /// run (and, when the dialog asked for it, reach the network) without asking again.
    /// A new fingerprint is a new question.
    Package {
        package_id: aura_domain_common::ToolPackageId,
        package_fingerprint: aura_domain_common::jcs::CanonicalSha256HashV1,
    },
    /// One site in the built-in browser, for this session: opening it, or acting on it,
    /// as the answer's capabilities say.
    BrowserSite {
        host: String,
    },
}

/// A user's session answer. It has no Core-instance or permission-revision lifetime binding.
/// Unknown semantic versions fail deserialization; historical grants have no such reference.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkSessionConsentRef {
    pub version: ToolWireVersionV1,
    pub approval_id: ApprovalRequestId,
    pub grant_id: PermissionGrantId,
    pub grant_revision: Revision,
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub principal_id: PrincipalId,
    pub caller: NetworkConsentCaller,
    pub destinations: NetworkConsentDestinations,
    pub approved_operation_fingerprint: OperationFingerprint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_grant_revision: Option<Revision>,
}

/// The source is the Core-verified registration, never an argument supplied by the model.
#[must_use]
pub fn ordinary_tool_network_caller(
    source: &SecuritySourceRef,
    action: &str,
) -> Option<NetworkConsentCaller> {
    if crate::built_in_shell(source, action) {
        return Some(NetworkConsentCaller::BuiltInShells);
    }
    if matches!(source, SecuritySourceRef::Tool { .. }) && action.starts_with("user.") {
        return Some(NetworkConsentCaller::PackageTools);
    }
    (matches!(source, SecuritySourceRef::BuiltInTool { .. })
        && matches!(action, "http.request" | "http.download" | "web.search" | "web.fetch"))
    .then_some(NetworkConsentCaller::OrdinaryTools)
}

impl NetworkSessionConsentRef {
    #[must_use]
    pub fn covers(
        &self,
        grant: &AuthorizationGrant,
        identity: &InvocationIdentity,
        source: &SecuritySourceRef,
        action: &str,
        capability: ResolvedCapability,
        resources: &[crate::ResourceDescriptor],
    ) -> bool {
        let destination = match (&self.destinations, self.caller) {
            (NetworkConsentDestinations::AllOrdinaryDestinations, caller) => {
                capability == ResolvedCapability::OrdinaryNetwork
                    && ordinary_tool_network_caller(source, action) == Some(caller)
            }
            (NetworkConsentDestinations::Git { target }, NetworkConsentCaller::Git) => {
                capability == ResolvedCapability::GitRemoteWrite
                    && crate::git_push_consent_target(source, action, &[capability], resources)
                        == Some(target)
            }
            (
                NetworkConsentDestinations::Package { package_id, package_fingerprint },
                NetworkConsentCaller::PackageTools,
            ) => {
                matches!(
                    capability,
                    ResolvedCapability::ProcessExecute | ResolvedCapability::OrdinaryNetwork
                ) && grant
                    .scope
                    .capability_limit
                    .as_ref()
                    .is_some_and(|caps| caps.contains(&capability))
                    && crate::package_tool_target(source, action, resources).is_some_and(
                        |(id, fingerprint, _)| {
                            id == package_id && fingerprint == package_fingerprint
                        },
                    )
            }
            (
                NetworkConsentDestinations::BrowserSite { host },
                NetworkConsentCaller::AuraBrowser,
            ) => {
                matches!(
                    capability,
                    ResolvedCapability::BrowserRead | ResolvedCapability::BrowserInteraction
                ) && grant
                    .scope
                    .capability_limit
                    .as_ref()
                    .is_some_and(|caps| caps.contains(&capability))
                    && crate::browser_tool(source, action)
                    && crate::browser_site(resources).is_some_and(|(site, _)| site == host)
            }
            _ => false,
        };
        destination
            && self.grant_id == grant.grant_id
            && self.grant_revision == grant.revision
            && self.revoked_grant_revision.is_none()
            && identity.principal_id == self.principal_id
            && identity.workspace_id == Some(self.workspace_id)
            && identity.session_id == Some(self.session_id)
            && matches!(grant.scope.lifetime, AuthorizationLifetime::Session { session_id } if session_id == self.session_id)
    }
}
