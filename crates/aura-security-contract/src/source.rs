use aura_domain_common::ContentHash;
use aura_domain_common::jcs::CanonicalSha256HashV1;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::TrustState;

/// Core-derived authority reference. A reference identifies a source; it does
/// not prove admission, trust, or permission. Tool adapters hash the complete
/// validated source, including its exact version, Registry and fingerprints.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "source_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SecuritySourceRef {
    UserInterface,
    BuiltInTool { identity_hash: CanonicalSha256HashV1 },
    Tool { identity_hash: CanonicalSha256HashV1 },
    ToolConformance { identity_hash: CanonicalSha256HashV1 },
    Hook { hook_id: String },
    McpServer { server_id: String },
    Skill { skill_id: String },
    Plugin { plugin_id: String },
    SystemComponent { component_id: String },
}

impl SecuritySourceRef {
    #[must_use]
    pub const fn requires_trust(&self) -> bool {
        matches!(
            self,
            Self::Tool { .. }
                | Self::Hook { .. }
                | Self::McpServer { .. }
                | Self::Skill { .. }
                | Self::Plugin { .. }
        )
    }

    #[must_use]
    pub fn trust_is_effective(
        &self,
        fingerprint: Option<&ContentHash>,
        state: &TrustState,
    ) -> bool {
        if !self.requires_trust() {
            return !matches!(state, TrustState::Revoked);
        }
        match state {
            TrustState::SystemTrusted => true,
            TrustState::Trusted { fingerprint: trusted, .. } => fingerprint == Some(trusted),
            TrustState::Untrusted | TrustState::Revoked => false,
        }
    }

    /// Preserves the existing persistence and approval-summary source keys.
    #[must_use]
    pub fn stable_key(&self) -> String {
        match self {
            Self::UserInterface => "user_interface".to_owned(),
            Self::BuiltInTool { identity_hash } | Self::Tool { identity_hash } => {
                format!("tool:{}", identity_hash.hex())
            }
            Self::ToolConformance { identity_hash } => {
                format!("tool_conformance:{}", identity_hash.hex())
            }
            Self::Hook { hook_id } => format!("hook:{hook_id}"),
            Self::McpServer { server_id } => format!("mcp:{server_id}"),
            Self::Skill { skill_id } => format!("skill:{skill_id}"),
            Self::Plugin { plugin_id } => format!("plugin:{plugin_id}"),
            Self::SystemComponent { component_id } => format!("system:{component_id}"),
        }
    }
}
