use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;

pub const MODEL_EGRESS_POLICY_VERSION_UNSUPPORTED_CODE: &str =
    "model_egress.policy_version_unsupported";

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(transparent)]
pub struct ModelEgressPolicyVersion(NonZeroU32);

impl ModelEgressPolicyVersion {
    pub const V1: Self = Self(NonZeroU32::MIN);
    /// Configured Provider communication uses source access authorization, not
    /// an additional content-origin egress approval. V1 remains interpretable.
    pub const V2: Self = Self(NonZeroU32::new(2).expect("nonzero version"));
    pub const CURRENT: Self = Self::V2;

    #[must_use]
    pub const fn new(value: NonZeroU32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0.get()
    }

    #[must_use]
    pub const fn is_supported(self) -> bool {
        self.0.get() == Self::V1.0.get() || self.0.get() == Self::V2.0.get()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkCallerKind {
    ModelTransport,
    ModelsCatalog,
    AuraTool,
    WebRuntime,
    AuraBrowser,
    SandboxProcess,
    McpHttp,
    ExternalExtensionProtocol,
    UpdateService,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum NetworkDestinationClass {
    Public,
    Loopback,
    Private,
    LinkLocal,
    Metadata,
    Reserved,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedNetworkDestination {
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub addresses: Vec<String>,
    pub classes: Vec<NetworkDestinationClass>,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum NetworkDataClass {
    PublicQuery,
    PublicContent,
    WorkspaceMetadata,
    WorkspaceSource,
    UserFileContent,
    PersonalData,
    CredentialMaterial,
    SecurityInput,
    Password,
    Otp,
    Cookie,
    PrivateKey,
    SuspectedSecret,
    DiagnosticData,
    Unknown,
}

impl NetworkDataClass {
    /// Historical V1 predicate. New model preparation must use
    /// `model_channel_disposition(ModelEgressPolicyVersion::CURRENT)`.
    #[must_use]
    pub const fn is_model_channel_always_denied(self) -> bool {
        matches!(
            self,
            Self::CredentialMaterial
                | Self::SecurityInput
                | Self::Password
                | Self::Otp
                | Self::Cookie
                | Self::PrivateKey
                | Self::SuspectedSecret
                | Self::Unknown
        )
    }

    pub fn model_channel_disposition(
        self,
        policy_version: ModelEgressPolicyVersion,
    ) -> Result<ModelChannelDataDisposition, ModelEgressPolicyVersionUnsupported> {
        if !policy_version.is_supported() {
            return Err(ModelEgressPolicyVersionUnsupported { policy_version });
        }
        if policy_version == ModelEgressPolicyVersion::V2 {
            return Ok(match self {
                // These designate protected security inputs/credential material,
                // not heuristics over ordinary user-authorized source text.
                Self::CredentialMaterial
                | Self::SecurityInput
                | Self::Password
                | Self::Otp
                | Self::Cookie
                | Self::PrivateKey => ModelChannelDataDisposition::AlwaysDenied,
                _ => ModelChannelDataDisposition::AllowedByModelChannel,
            });
        }
        Ok(match self {
            Self::PublicQuery
            | Self::PublicContent
            | Self::WorkspaceMetadata
            | Self::WorkspaceSource => ModelChannelDataDisposition::AllowedByModelChannel,
            Self::UserFileContent | Self::PersonalData | Self::DiagnosticData => {
                ModelChannelDataDisposition::RequiresSourceEgressGrant
            }
            Self::CredentialMaterial
            | Self::SecurityInput
            | Self::Password
            | Self::Otp
            | Self::Cookie
            | Self::PrivateKey
            | Self::SuspectedSecret
            | Self::Unknown => ModelChannelDataDisposition::AlwaysDenied,
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelChannelDataDisposition {
    AllowedByModelChannel,
    RequiresSourceEgressGrant,
    AlwaysDenied,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error(
    "{MODEL_EGRESS_POLICY_VERSION_UNSUPPORTED_CODE}: unsupported model egress policy version {policy_version:?}"
)]
pub struct ModelEgressPolicyVersionUnsupported {
    pub policy_version: ModelEgressPolicyVersion,
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::{
        MODEL_EGRESS_POLICY_VERSION_UNSUPPORTED_CODE, ModelChannelDataDisposition,
        ModelEgressPolicyVersion, NetworkDataClass,
    };

    #[test]
    fn model_channel_permanently_denies_unknown_and_secret_bearing_classes() {
        for class in [
            NetworkDataClass::Unknown,
            NetworkDataClass::CredentialMaterial,
            NetworkDataClass::SecurityInput,
            NetworkDataClass::Password,
            NetworkDataClass::Otp,
            NetworkDataClass::Cookie,
            NetworkDataClass::PrivateKey,
            NetworkDataClass::SuspectedSecret,
        ] {
            assert!(class.is_model_channel_always_denied());
        }
        for class in [
            NetworkDataClass::PublicQuery,
            NetworkDataClass::PublicContent,
            NetworkDataClass::WorkspaceMetadata,
            NetworkDataClass::WorkspaceSource,
            NetworkDataClass::UserFileContent,
            NetworkDataClass::PersonalData,
            NetworkDataClass::DiagnosticData,
        ] {
            assert!(!class.is_model_channel_always_denied());
        }
    }

    #[test]
    fn model_channel_disposition_wire_values_are_stable() {
        assert_eq!(
            serde_json::to_string(&ModelChannelDataDisposition::AllowedByModelChannel)
                .expect("serialize disposition"),
            "\"allowed_by_model_channel\""
        );
        assert_eq!(
            serde_json::to_string(&ModelChannelDataDisposition::RequiresSourceEgressGrant)
                .expect("serialize disposition"),
            "\"requires_source_egress_grant\""
        );
        assert_eq!(
            serde_json::to_string(&ModelChannelDataDisposition::AlwaysDenied)
                .expect("serialize disposition"),
            "\"always_denied\""
        );
    }

    #[test]
    fn policy_v1_matrix_is_exhaustive_and_always_denied_has_priority() {
        for class in [
            NetworkDataClass::PublicQuery,
            NetworkDataClass::PublicContent,
            NetworkDataClass::WorkspaceMetadata,
            NetworkDataClass::WorkspaceSource,
        ] {
            assert_eq!(
                class
                    .model_channel_disposition(ModelEgressPolicyVersion::V1)
                    .expect("supported policy"),
                ModelChannelDataDisposition::AllowedByModelChannel
            );
        }
        for class in [
            NetworkDataClass::UserFileContent,
            NetworkDataClass::PersonalData,
            NetworkDataClass::DiagnosticData,
        ] {
            assert_eq!(
                class
                    .model_channel_disposition(ModelEgressPolicyVersion::V1)
                    .expect("supported policy"),
                ModelChannelDataDisposition::RequiresSourceEgressGrant
            );
        }
        for class in [
            NetworkDataClass::Unknown,
            NetworkDataClass::CredentialMaterial,
            NetworkDataClass::SecurityInput,
            NetworkDataClass::Password,
            NetworkDataClass::Otp,
            NetworkDataClass::Cookie,
            NetworkDataClass::PrivateKey,
            NetworkDataClass::SuspectedSecret,
        ] {
            assert_eq!(
                class
                    .model_channel_disposition(ModelEgressPolicyVersion::V1)
                    .expect("supported policy"),
                ModelChannelDataDisposition::AlwaysDenied
            );
        }
    }

    #[test]
    fn policy_version_is_a_nonzero_integer_and_unknown_versions_fail_closed() {
        assert_eq!(
            serde_json::to_value(ModelEgressPolicyVersion::V1).expect("serialize"),
            serde_json::json!(1)
        );
        assert!(serde_json::from_value::<ModelEgressPolicyVersion>(serde_json::json!(0)).is_err());

        let unsupported =
            ModelEgressPolicyVersion::new(NonZeroU32::new(3).expect("non-zero version"));
        let error = NetworkDataClass::PublicQuery
            .model_channel_disposition(unsupported)
            .expect_err("unknown policy must fail closed");
        assert_eq!(error.policy_version, unsupported);
        assert!(error.to_string().starts_with(MODEL_EGRESS_POLICY_VERSION_UNSUPPORTED_CODE));
    }
}
