use std::collections::BTreeSet;

use aura_domain_common::{
    CommandInvocationId, ContentHash, ContentHashAlgorithm, CorrelationId, CredentialId,
    CredentialRevision, EndpointId, IpcChannelId, ModelAvailabilityTestId, ModelDiscoveryRunId,
    ModelProfileId, NetworkRequestId, Revision, RuntimeInstanceId, TransportAttemptId,
    UtcTimestamp, canonical_json_v1, canonical_json_v1_sha256,
};
use aura_model_domain::{CredentialBindingFingerprint, TransportOperationContext};
use aura_permission_domain::{
    CoreRequestGrant, NetworkByteLimits, NetworkDestinationPolicy, NetworkProtocolClass,
    NormalizedOrigin, RedirectPolicy,
};
use aura_security_contract::{InvocationIdentity, NetworkCallerKind, NetworkDestinationClass};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const CONFIGURATION_MANIFEST_SCHEMA_VERSION: u16 = 1;
pub const CONFIGURATION_REDIRECT_MAX_HOPS: u8 = 3;
pub const PROVIDER_TRANSPORT_GRANT_TTL_SECONDS: i64 = 30;

#[derive(Serialize)]
struct CanonicalHeaderEntry<'a> {
    name: String,
    value: &'a str,
}

pub fn canonical_non_secret_headers_hash(
    headers: &[(String, String)],
) -> Result<ContentHash, ProviderGrantContractError> {
    let mut entries = Vec::with_capacity(headers.len());
    for (name, value) in headers {
        let name = name.to_ascii_lowercase();
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            || matches!(
                name.as_str(),
                "authorization" | "proxy-authorization" | "x-api-key" | "cookie" | "set-cookie"
            )
            || value.bytes().any(|byte| matches!(byte, b'\r' | b'\n'))
        {
            return Err(ProviderGrantContractError::Headers);
        }
        entries.push(CanonicalHeaderEntry { name, value });
    }
    entries.sort_unstable_by(|left, right| {
        left.name.cmp(&right.name).then_with(|| left.value.cmp(right.value))
    });
    canonical_json_v1_sha256(&entries).map_err(|_| ProviderGrantContractError::CanonicalJson)
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigurationOperationKind {
    ModelDiscovery,
    ModelAvailabilityTest,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UserConfigurationActor {
    User,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UserConfigurationSource {
    UserInterface,
}

/// Trusted identity constructed by the Core settings command handler from an
/// authenticated Main channel. It intentionally cannot be deserialized.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UserConfigurationOperationIdentity {
    command_invocation_id: CommandInvocationId,
    correlation_id: CorrelationId,
    core_runtime_instance_id: RuntimeInstanceId,
    authenticated_main_runtime_instance_id: RuntimeInstanceId,
    authenticated_channel_id: IpcChannelId,
    actor: UserConfigurationActor,
    source: UserConfigurationSource,
    operation_kind: ConfigurationOperationKind,
    target_endpoint_id: EndpointId,
    config_revision: Revision,
    created_at: UtcTimestamp,
}

/// The trusted Core-side authority for one Provider transport request.
///
/// This type is intentionally Rust-only and cannot cross IPC or be synthesized
/// from Renderer-controlled JSON. Agent requests carry the P08-owned
/// `InvocationIdentity`; settings requests carry the identity that Core built
/// from the authenticated Main channel.
#[derive(Clone, Debug, PartialEq)]
pub enum ProviderTransportRequestAuthority {
    AgentModel(InvocationIdentity),
    UserConfiguration(UserConfigurationOperationIdentity),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthenticatedConfigurationChannelIdentity {
    pub core_runtime_instance_id: RuntimeInstanceId,
    pub authenticated_main_runtime_instance_id: RuntimeInstanceId,
    pub authenticated_channel_id: IpcChannelId,
}

impl UserConfigurationOperationIdentity {
    #[must_use]
    pub const fn from_authenticated_settings_command(
        command_invocation_id: CommandInvocationId,
        correlation_id: CorrelationId,
        channel: AuthenticatedConfigurationChannelIdentity,
        operation_kind: ConfigurationOperationKind,
        target_endpoint_id: EndpointId,
        config_revision: Revision,
        created_at: UtcTimestamp,
    ) -> Self {
        Self {
            command_invocation_id,
            correlation_id,
            core_runtime_instance_id: channel.core_runtime_instance_id,
            authenticated_main_runtime_instance_id: channel.authenticated_main_runtime_instance_id,
            authenticated_channel_id: channel.authenticated_channel_id,
            actor: UserConfigurationActor::User,
            source: UserConfigurationSource::UserInterface,
            operation_kind,
            target_endpoint_id,
            config_revision,
            created_at,
        }
    }

    #[must_use]
    pub const fn command_invocation_id(&self) -> CommandInvocationId {
        self.command_invocation_id
    }

    #[must_use]
    pub const fn correlation_id(&self) -> CorrelationId {
        self.correlation_id
    }

    #[must_use]
    pub const fn operation_kind(&self) -> ConfigurationOperationKind {
        self.operation_kind
    }

    #[must_use]
    pub const fn target_endpoint_id(&self) -> EndpointId {
        self.target_endpoint_id
    }

    #[must_use]
    pub const fn config_revision(&self) -> Revision {
        self.config_revision
    }

    pub fn binding_hash(&self) -> Result<ContentHash, ProviderGrantContractError> {
        canonical_json_v1_sha256(self).map_err(|_| ProviderGrantContractError::CanonicalJson)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConfigurationRequestPurpose {
    ModelDiscovery {
        model_discovery_run_id: ModelDiscoveryRunId,
    },
    ModelAvailabilityProbe {
        model_availability_test_id: ModelAvailabilityTestId,
        model_profile_id: ModelProfileId,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfigurationTransportLimitsV1 {
    pub max_request_body_bytes: u64,
    pub max_response_wire_bytes: u64,
    pub max_response_decompressed_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModelDiscoveryOperationLimitsV1 {
    pub max_pages: u16,
    pub max_models: u32,
    pub max_aggregate_decompressed_bytes: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ConfigurationTransportPolicyV1;

impl ConfigurationTransportPolicyV1 {
    const KIB: u64 = 1_024;
    const MIB: u64 = 1_024 * Self::KIB;

    pub const MODEL_DISCOVERY: ConfigurationTransportLimitsV1 = ConfigurationTransportLimitsV1 {
        max_request_body_bytes: 4 * Self::KIB,
        max_response_wire_bytes: 8 * Self::MIB,
        max_response_decompressed_bytes: 16 * Self::MIB,
    };
    pub const MODEL_AVAILABILITY_PROBE: ConfigurationTransportLimitsV1 =
        ConfigurationTransportLimitsV1 {
            max_request_body_bytes: 64 * Self::KIB,
            max_response_wire_bytes: Self::MIB,
            max_response_decompressed_bytes: 2 * Self::MIB,
        };
    pub const MODEL_DISCOVERY_OPERATION: ModelDiscoveryOperationLimitsV1 =
        ModelDiscoveryOperationLimitsV1 {
            max_pages: 16,
            max_models: 10_000,
            max_aggregate_decompressed_bytes: 32 * Self::MIB,
        };

    #[must_use]
    pub const fn limits_for(
        purpose: &ConfigurationRequestPurpose,
    ) -> ConfigurationTransportLimitsV1 {
        match purpose {
            ConfigurationRequestPurpose::ModelDiscovery { .. } => Self::MODEL_DISCOVERY,
            ConfigurationRequestPurpose::ModelAvailabilityProbe { .. } => {
                Self::MODEL_AVAILABILITY_PROBE
            }
        }
    }

    #[must_use]
    pub const fn agent_model_response_limit_bytes(effective_max_output_tokens: u64) -> Option<u64> {
        if effective_max_output_tokens == 0 {
            return None;
        }
        let calculated =
            (8 * Self::MIB).saturating_add(effective_max_output_tokens.saturating_mul(256));
        let minimum = 32 * Self::MIB;
        let maximum = 128 * Self::MIB;
        Some(if calculated < minimum {
            minimum
        } else if calculated > maximum {
            maximum
        } else {
            calculated
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigurationRequestTargetV1 {
    pub method: String,
    pub origin: NormalizedOrigin,
    pub path_and_query: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigurationRequestCredentialBindingV1 {
    pub credential_id: CredentialId,
    pub credential_revision: CredentialRevision,
    pub credential_binding_fingerprint: CredentialBindingFingerprint,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigurationResponseMediaPolicyV1 {
    pub accepted_media_types: BTreeSet<String>,
    pub require_content_type: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigurationRequestDataClass {
    ProtocolMetadata,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigurationOperationRequestManifestV1 {
    pub schema_version: u16,
    pub command_invocation_id: CommandInvocationId,
    pub correlation_id: CorrelationId,
    pub user_configuration_operation_identity_binding_hash: ContentHash,
    pub purpose: ConfigurationRequestPurpose,
    pub transport_attempt_id: TransportAttemptId,
    pub endpoint_id: EndpointId,
    pub models_config_revision: Revision,
    pub endpoint_security_fingerprint: ContentHash,
    pub target: ConfigurationRequestTargetV1,
    pub non_secret_headers_hash: ContentHash,
    pub request_body_hash: ContentHash,
    pub request_body_length: u64,
    pub credential_binding: Option<ConfigurationRequestCredentialBindingV1>,
    pub max_request_bytes: u64,
    pub max_response_bytes: u64,
    pub max_decompressed_response_bytes: u64,
    pub redirect_policy: RedirectPolicy,
    pub response_media_policy: ConfigurationResponseMediaPolicyV1,
    pub data_class: ConfigurationRequestDataClass,
}

impl ConfigurationOperationRequestManifestV1 {
    pub fn validate(
        &self,
        identity: &UserConfigurationOperationIdentity,
        operation_context: &TransportOperationContext,
    ) -> Result<(), ProviderGrantContractError> {
        if self.schema_version != CONFIGURATION_MANIFEST_SCHEMA_VERSION {
            return Err(ProviderGrantContractError::ManifestVersion);
        }
        if self.command_invocation_id != identity.command_invocation_id()
            || self.correlation_id != identity.correlation_id()
            || self.endpoint_id != identity.target_endpoint_id()
            || self.models_config_revision != identity.config_revision()
            || self.user_configuration_operation_identity_binding_hash != identity.binding_hash()?
        {
            return Err(ProviderGrantContractError::IdentityBinding);
        }
        validate_purpose(identity.operation_kind(), &self.purpose, operation_context)?;
        validate_sha256(&self.user_configuration_operation_identity_binding_hash)?;
        validate_sha256(&self.non_secret_headers_hash)?;
        validate_sha256(&self.request_body_hash)?;
        if let Some(binding) = &self.credential_binding {
            validate_sha256(&binding.credential_binding_fingerprint.0)?;
        }
        self.target.validate()?;
        self.response_media_policy.validate()?;
        let required_limits = ConfigurationTransportPolicyV1::limits_for(&self.purpose);
        if self.max_request_bytes == 0
            || self.max_response_bytes == 0
            || self.max_decompressed_response_bytes == 0
            || self.request_body_length > self.max_request_bytes
            || self.max_request_bytes != required_limits.max_request_body_bytes
            || self.max_response_bytes != required_limits.max_response_wire_bytes
            || self.max_decompressed_response_bytes
                != required_limits.max_response_decompressed_bytes
        {
            return Err(ProviderGrantContractError::ByteLimits);
        }
        if self.redirect_policy
            != (RedirectPolicy::SameOrigin { max_hops: CONFIGURATION_REDIRECT_MAX_HOPS })
        {
            return Err(ProviderGrantContractError::RedirectPolicy);
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ProviderGrantContractError> {
        canonical_json_v1(self).map_err(|_| ProviderGrantContractError::CanonicalJson)
    }

    pub fn manifest_hash(&self) -> Result<ContentHash, ProviderGrantContractError> {
        canonical_json_v1_sha256(self).map_err(|_| ProviderGrantContractError::CanonicalJson)
    }
}

impl ConfigurationRequestTargetV1 {
    fn validate(&self) -> Result<(), ProviderGrantContractError> {
        if self.method.is_empty()
            || self.method.len() > 16
            || !self.method.bytes().all(|byte| byte.is_ascii_uppercase() || byte == b'-')
        {
            return Err(ProviderGrantContractError::Target);
        }
        if !matches!(self.origin.scheme.as_str(), "http" | "https")
            || self.origin.host.trim().is_empty()
            || self.origin.host.contains(['/', '@', '#', '?'])
            || self.path_and_query.is_empty()
            || !self.path_and_query.starts_with('/')
            || self.path_and_query.contains('#')
            || self.path_and_query.bytes().any(|byte| matches!(byte, b'\r' | b'\n'))
        {
            return Err(ProviderGrantContractError::Target);
        }
        Ok(())
    }
}

impl ConfigurationResponseMediaPolicyV1 {
    fn validate(&self) -> Result<(), ProviderGrantContractError> {
        if self.accepted_media_types.iter().any(|media_type| {
            let Some((kind, subtype)) = media_type.split_once('/') else {
                return true;
            };
            kind.is_empty()
                || subtype.is_empty()
                || media_type.contains(';')
                || !media_type.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'/' | b'.' | b'+' | b'-')
                })
        }) {
            return Err(ProviderGrantContractError::ResponseMediaPolicy);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderTransportAuthorizationBinding {
    AgentModel {
        invocation_identity_binding_hash: ContentHash,
        model_egress_manifest_hash: ContentHash,
    },
    UserConfiguration {
        operation_kind: ConfigurationOperationKind,
        identity_binding_hash: ContentHash,
        models_config_revision: Revision,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderTransportAttemptGrant {
    pub transport_attempt_id: TransportAttemptId,
    pub operation_context: TransportOperationContext,
    pub endpoint_id: EndpointId,
    pub endpoint_security_fingerprint: ContentHash,
    pub request_manifest_hash: ContentHash,
    pub non_secret_headers_hash: ContentHash,
    pub request_body_hash: ContentHash,
    pub request_body_length: u64,
    pub target: ConfigurationRequestTargetV1,
    pub credential_binding: Option<ConfigurationRequestCredentialBindingV1>,
    pub byte_limits: NetworkByteLimits,
    pub redirect_policy: RedirectPolicy,
    pub response_media_policy: ConfigurationResponseMediaPolicyV1,
    pub authorization: ProviderTransportAuthorizationBinding,
}

impl ProviderTransportAttemptGrant {
    pub fn from_configuration_manifest(
        identity: &UserConfigurationOperationIdentity,
        operation_context: TransportOperationContext,
        manifest: &ConfigurationOperationRequestManifestV1,
    ) -> Result<Self, ProviderGrantContractError> {
        manifest.validate(identity, &operation_context)?;
        let grant = Self {
            transport_attempt_id: manifest.transport_attempt_id,
            operation_context,
            endpoint_id: manifest.endpoint_id,
            endpoint_security_fingerprint: manifest.endpoint_security_fingerprint.clone(),
            request_manifest_hash: manifest.manifest_hash()?,
            non_secret_headers_hash: manifest.non_secret_headers_hash.clone(),
            request_body_hash: manifest.request_body_hash.clone(),
            request_body_length: manifest.request_body_length,
            target: manifest.target.clone(),
            credential_binding: manifest.credential_binding.clone(),
            byte_limits: NetworkByteLimits {
                max_upload_bytes: manifest.max_request_bytes,
                max_download_bytes: manifest.max_response_bytes,
                max_decompressed_bytes: manifest.max_decompressed_response_bytes,
            },
            redirect_policy: manifest.redirect_policy,
            response_media_policy: manifest.response_media_policy.clone(),
            authorization: ProviderTransportAuthorizationBinding::UserConfiguration {
                operation_kind: identity.operation_kind(),
                identity_binding_hash: identity.binding_hash()?,
                models_config_revision: identity.config_revision(),
            },
        };
        grant.validate_intrinsic()?;
        Ok(grant)
    }

    pub fn validate_intrinsic(&self) -> Result<(), ProviderGrantContractError> {
        validate_sha256(&self.request_manifest_hash)?;
        validate_sha256(&self.non_secret_headers_hash)?;
        validate_sha256(&self.request_body_hash)?;
        self.target.validate()?;
        self.response_media_policy.validate()?;
        if self.byte_limits.max_upload_bytes == 0
            || self.byte_limits.max_download_bytes == 0
            || self.byte_limits.max_decompressed_bytes == 0
            || self.request_body_length > self.byte_limits.max_upload_bytes
        {
            return Err(ProviderGrantContractError::ByteLimits);
        }
        if let Some(binding) = &self.credential_binding {
            validate_sha256(&binding.credential_binding_fingerprint.0)?;
        }
        match (&self.operation_context, &self.authorization) {
            (
                TransportOperationContext::AgentModelAttempt { .. },
                ProviderTransportAuthorizationBinding::AgentModel {
                    invocation_identity_binding_hash,
                    model_egress_manifest_hash,
                },
            ) => {
                validate_sha256(invocation_identity_binding_hash)?;
                validate_sha256(model_egress_manifest_hash)?;
            }
            (
                TransportOperationContext::ModelDiscovery { .. }
                | TransportOperationContext::ModelAvailabilityTest { .. },
                ProviderTransportAuthorizationBinding::UserConfiguration {
                    identity_binding_hash,
                    ..
                },
            ) => {
                validate_sha256(identity_binding_hash)?;
                if self.redirect_policy
                    != (RedirectPolicy::SameOrigin { max_hops: CONFIGURATION_REDIRECT_MAX_HOPS })
                {
                    return Err(ProviderGrantContractError::RedirectPolicy);
                }
            }
            _ => return Err(ProviderGrantContractError::AuthorizationContext),
        }
        Ok(())
    }

    pub fn validate_configuration_copy(
        &self,
        identity: &UserConfigurationOperationIdentity,
        manifest: &ConfigurationOperationRequestManifestV1,
    ) -> Result<(), ProviderGrantContractError> {
        let expected =
            Self::from_configuration_manifest(identity, self.operation_context.clone(), manifest)?;
        if *self != expected {
            return Err(ProviderGrantContractError::ManifestCopyMismatch);
        }
        Ok(())
    }
}

pub fn issue_configuration_provider_transport_grant(
    identity: &UserConfigurationOperationIdentity,
    operation_context: TransportOperationContext,
    manifest: &ConfigurationOperationRequestManifestV1,
    logical_request_id: NetworkRequestId,
    issued_at: UtcTimestamp,
) -> Result<CoreRequestGrant<ProviderTransportAttemptGrant>, ProviderGrantContractError> {
    let purpose = ProviderTransportAttemptGrant::from_configuration_manifest(
        identity,
        operation_context,
        manifest,
    )?;
    Ok(CoreRequestGrant {
        logical_request_id,
        caller: NetworkCallerKind::ModelTransport,
        destination_policy: NetworkDestinationPolicy {
            protocol: NetworkProtocolClass::Http,
            host: purpose.target.origin.host.clone(),
            port: purpose.target.origin.port,
            allowed_destination_classes: vec![
                NetworkDestinationClass::Public,
                NetworkDestinationClass::Private,
                NetworkDestinationClass::Loopback,
            ],
        },
        methods: vec![purpose.target.method.clone()],
        redirect_policy: purpose.redirect_policy,
        data_classes: Vec::new(),
        byte_limits: purpose.byte_limits.clone(),
        expires_at: UtcTimestamp::from_value(
            issued_at.value() + time::Duration::seconds(PROVIDER_TRANSPORT_GRANT_TTL_SECONDS),
        ),
        purpose,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StandardNetworkRequestGrant;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ProviderGrantContractError {
    #[error("configuration manifest schema version must be 1")]
    ManifestVersion,
    #[error("configuration operation identity binding mismatch")]
    IdentityBinding,
    #[error("configuration purpose and transport operation context mismatch")]
    PurposeContext,
    #[error("provider grant authorization and transport operation context mismatch")]
    AuthorizationContext,
    #[error("configuration request target is invalid")]
    Target,
    #[error("configuration request headers are invalid or contain credential material")]
    Headers,
    #[error("configuration request hash must use SHA-256")]
    HashAlgorithm,
    #[error("configuration request byte limits are invalid")]
    ByteLimits,
    #[error("configuration request redirect policy must be same-origin with three hops")]
    RedirectPolicy,
    #[error("configuration response media policy is invalid")]
    ResponseMediaPolicy,
    #[error("canonical JSON v1 serialization failed")]
    CanonicalJson,
    #[error("provider grant copied fields do not match the configuration manifest")]
    ManifestCopyMismatch,
}

fn validate_sha256(hash: &ContentHash) -> Result<(), ProviderGrantContractError> {
    if hash.algorithm == ContentHashAlgorithm::Sha256 {
        Ok(())
    } else {
        Err(ProviderGrantContractError::HashAlgorithm)
    }
}

fn validate_purpose(
    operation_kind: ConfigurationOperationKind,
    purpose: &ConfigurationRequestPurpose,
    operation_context: &TransportOperationContext,
) -> Result<(), ProviderGrantContractError> {
    let matches_kind = matches!(
        (operation_kind, purpose),
        (
            ConfigurationOperationKind::ModelDiscovery,
            ConfigurationRequestPurpose::ModelDiscovery { .. },
        ) | (
            ConfigurationOperationKind::ModelAvailabilityTest,
            ConfigurationRequestPurpose::ModelAvailabilityProbe { .. },
        )
    );
    let matches_context = match (purpose, operation_context) {
        (
            ConfigurationRequestPurpose::ModelDiscovery { model_discovery_run_id },
            TransportOperationContext::ModelDiscovery { discovery_run_id },
        ) => model_discovery_run_id == discovery_run_id,
        (
            ConfigurationRequestPurpose::ModelAvailabilityProbe {
                model_availability_test_id,
                model_profile_id,
            },
            TransportOperationContext::ModelAvailabilityTest {
                model_availability_test_id: context_test_id,
                model_profile_id: context_profile_id,
            },
        ) => {
            model_availability_test_id == context_test_id && model_profile_id == context_profile_id
        }
        _ => false,
    };
    if matches_kind && matches_context {
        Ok(())
    } else {
        Err(ProviderGrantContractError::PurposeContext)
    }
}

pub fn invocation_identity_binding_hash(
    identity: &InvocationIdentity,
) -> Result<ContentHash, ProviderGrantContractError> {
    canonical_json_v1_sha256(identity).map_err(|_| ProviderGrantContractError::CanonicalJson)
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, str::FromStr};

    use aura_domain_common::{
        ContentHash, CredentialRevision, EndpointId, ModelDiscoveryRunId, Revision, UtcTimestamp,
    };
    use aura_model_domain::{CredentialBindingFingerprint, TransportOperationContext};
    use aura_permission_domain::{NormalizedOrigin, RedirectPolicy};

    use super::{
        AuthenticatedConfigurationChannelIdentity, CONFIGURATION_MANIFEST_SCHEMA_VERSION,
        CONFIGURATION_REDIRECT_MAX_HOPS, ConfigurationOperationKind,
        ConfigurationOperationRequestManifestV1, ConfigurationRequestCredentialBindingV1,
        ConfigurationRequestDataClass, ConfigurationRequestPurpose, ConfigurationRequestTargetV1,
        ConfigurationResponseMediaPolicyV1, ConfigurationTransportPolicyV1,
        ProviderGrantContractError, ProviderTransportAttemptGrant,
        UserConfigurationOperationIdentity, issue_configuration_provider_transport_grant,
    };

    fn fixed<T>(value: &str) -> T
    where
        T: FromStr,
        T::Err: std::fmt::Debug,
    {
        value.parse().expect("fixed typed ID")
    }

    fn identity(endpoint_id: EndpointId) -> UserConfigurationOperationIdentity {
        UserConfigurationOperationIdentity::from_authenticated_settings_command(
            fixed("civ_01ARZ3NDEKTSV4RRFFQ69G5FAV"),
            fixed("cor_01ARZ3NDEKTSV4RRFFQ69G5FAW"),
            AuthenticatedConfigurationChannelIdentity {
                core_runtime_instance_id: fixed("run_01ARZ3NDEKTSV4RRFFQ69G5FAX"),
                authenticated_main_runtime_instance_id: fixed("run_01ARZ3NDEKTSV4RRFFQ69G5FAY"),
                authenticated_channel_id: fixed("chn_01ARZ3NDEKTSV4RRFFQ69G5FAZ"),
            },
            ConfigurationOperationKind::ModelDiscovery,
            endpoint_id,
            Revision::new(7),
            "2026-08-31T00:00:00Z".parse::<UtcTimestamp>().expect("timestamp"),
        )
    }

    fn manifest(
        identity: &UserConfigurationOperationIdentity,
        discovery_run_id: ModelDiscoveryRunId,
    ) -> ConfigurationOperationRequestManifestV1 {
        ConfigurationOperationRequestManifestV1 {
            schema_version: CONFIGURATION_MANIFEST_SCHEMA_VERSION,
            command_invocation_id: identity.command_invocation_id(),
            correlation_id: identity.correlation_id(),
            user_configuration_operation_identity_binding_hash: identity
                .binding_hash()
                .expect("identity hash"),
            purpose: ConfigurationRequestPurpose::ModelDiscovery {
                model_discovery_run_id: discovery_run_id,
            },
            transport_attempt_id: fixed("tat_01ARZ3NDEKTSV4RRFFQ69G5FB0"),
            endpoint_id: identity.target_endpoint_id(),
            models_config_revision: identity.config_revision(),
            endpoint_security_fingerprint: ContentHash::blake3(b"endpoint security"),
            target: ConfigurationRequestTargetV1 {
                method: "GET".to_owned(),
                origin: NormalizedOrigin {
                    scheme: "https".to_owned(),
                    host: "api.example.test".to_owned(),
                    port: 443,
                },
                path_and_query: "/v1/models?limit=100".to_owned(),
            },
            non_secret_headers_hash: ContentHash::sha256(b"headers"),
            request_body_hash: ContentHash::sha256(b""),
            request_body_length: 0,
            credential_binding: Some(ConfigurationRequestCredentialBindingV1 {
                credential_id: fixed("crd_01ARZ3NDEKTSV4RRFFQ69G5FB1"),
                credential_revision: CredentialRevision::new(Revision::new(2))
                    .expect("credential revision"),
                credential_binding_fingerprint: CredentialBindingFingerprint(ContentHash::sha256(
                    b"credential binding",
                )),
            }),
            max_request_bytes: ConfigurationTransportPolicyV1::MODEL_DISCOVERY
                .max_request_body_bytes,
            max_response_bytes: ConfigurationTransportPolicyV1::MODEL_DISCOVERY
                .max_response_wire_bytes,
            max_decompressed_response_bytes: ConfigurationTransportPolicyV1::MODEL_DISCOVERY
                .max_response_decompressed_bytes,
            redirect_policy: RedirectPolicy::SameOrigin {
                max_hops: CONFIGURATION_REDIRECT_MAX_HOPS,
            },
            response_media_policy: ConfigurationResponseMediaPolicyV1 {
                accepted_media_types: BTreeSet::from(["application/json".to_owned()]),
                require_content_type: true,
            },
            data_class: ConfigurationRequestDataClass::ProtocolMetadata,
        }
    }

    #[test]
    fn canonical_manifest_golden_vector_is_stable() {
        let endpoint_id: EndpointId = fixed("end_01ARZ3NDEKTSV4RRFFQ69G5FB2");
        let discovery_run_id: ModelDiscoveryRunId = fixed("mdr_01ARZ3NDEKTSV4RRFFQ69G5FB3");
        let identity = identity(endpoint_id);
        let manifest = manifest(&identity, discovery_run_id);
        let context = TransportOperationContext::ModelDiscovery { discovery_run_id };
        manifest.validate(&identity, &context).expect("valid manifest");
        let bytes = manifest.canonical_bytes().expect("canonical manifest");
        let golden = concat!(
            "{\"command_invocation_id\":\"civ_01ARZ3NDEKTSV4RRFFQ69G5FAV\",",
            "\"correlation_id\":\"cor_01ARZ3NDEKTSV4RRFFQ69G5FAW\",",
            "\"credential_binding\":{\"credential_binding_fingerprint\":{",
            "\"algorithm\":\"sha256\",\"hex\":\"02bf08b26b3c3a035df43a1e055633508f483526de87b2e1011af505c091fc9c\"},",
            "\"credential_id\":\"crd_01ARZ3NDEKTSV4RRFFQ69G5FB1\",\"credential_revision\":\"2\"},",
            "\"data_class\":\"protocol_metadata\",",
            "\"endpoint_id\":\"end_01ARZ3NDEKTSV4RRFFQ69G5FB2\",",
            "\"endpoint_security_fingerprint\":{\"algorithm\":\"blake3\",",
            "\"hex\":\"c31964a78ccd833977561fc5dc2bbfeb8dfa766680ec780b164521cdaedebcb5\"},",
            "\"max_decompressed_response_bytes\":16777216,\"max_request_bytes\":4096,",
            "\"max_response_bytes\":8388608,\"models_config_revision\":\"7\",",
            "\"non_secret_headers_hash\":{\"algorithm\":\"sha256\",",
            "\"hex\":\"378f37b1fb33fa4dafba6519a48e06447ecaa58af290729cc3d357bcadd9926c\"},",
            "\"purpose\":{\"model_discovery_run_id\":\"mdr_01ARZ3NDEKTSV4RRFFQ69G5FB3\",",
            "\"type\":\"model_discovery\"},\"redirect_policy\":{\"same_origin\":{\"max_hops\":3}},",
            "\"request_body_hash\":{\"algorithm\":\"sha256\",",
            "\"hex\":\"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\"},",
            "\"request_body_length\":0,\"response_media_policy\":{",
            "\"accepted_media_types\":[\"application/json\"],\"require_content_type\":true},",
            "\"schema_version\":1,\"target\":{\"method\":\"GET\",\"origin\":{",
            "\"host\":\"api.example.test\",\"port\":443,\"scheme\":\"https\"},",
            "\"path_and_query\":\"/v1/models?limit=100\"},",
            "\"transport_attempt_id\":\"tat_01ARZ3NDEKTSV4RRFFQ69G5FB0\",",
            "\"user_configuration_operation_identity_binding_hash\":{\"algorithm\":\"sha256\",",
            "\"hex\":\"01c2729fa37ea026477097d10797d460cec09ea3a24924844193373a5dd5acd0\"}}"
        );
        assert_eq!(bytes, golden.as_bytes());
        let value: serde_json::Value = serde_json::from_slice(&bytes).expect("canonical JSON");
        assert_eq!(value["credential_binding"]["credential_id"], "crd_01ARZ3NDEKTSV4RRFFQ69G5FB1");
        assert_eq!(manifest.manifest_hash().expect("hash"), ContentHash::sha256(&bytes));
        assert!(!bytes.contains(&b'\n'));
        assert!(!bytes.starts_with(&[0xef, 0xbb, 0xbf]));
    }

    #[test]
    fn grant_is_an_exact_copy_of_the_validated_manifest() {
        let endpoint_id: EndpointId = fixed("end_01ARZ3NDEKTSV4RRFFQ69G5FB2");
        let discovery_run_id: ModelDiscoveryRunId = fixed("mdr_01ARZ3NDEKTSV4RRFFQ69G5FB3");
        let identity = identity(endpoint_id);
        let manifest = manifest(&identity, discovery_run_id);
        let context = TransportOperationContext::ModelDiscovery { discovery_run_id };
        let grant = ProviderTransportAttemptGrant::from_configuration_manifest(
            &identity, context, &manifest,
        )
        .expect("grant");
        grant.validate_configuration_copy(&identity, &manifest).expect("copy");

        let issued_at = UtcTimestamp::now();
        let envelope = issue_configuration_provider_transport_grant(
            &identity,
            grant.operation_context.clone(),
            &manifest,
            fixed("nwr_01ARZ3NDEKTSV4RRFFQ69G5FB4"),
            issued_at,
        )
        .expect("core grant");
        assert_eq!(envelope.purpose, grant);
        assert_eq!(envelope.expires_at.value(), issued_at.value() + time::Duration::seconds(30));
    }

    #[test]
    fn configuration_transport_policy_v1_uses_the_frozen_limits() {
        let kib = 1_024_u64;
        let mib = 1_024 * kib;

        assert_eq!(ConfigurationTransportPolicyV1::MODEL_DISCOVERY.max_request_body_bytes, 4 * kib);
        assert_eq!(
            ConfigurationTransportPolicyV1::MODEL_DISCOVERY.max_response_wire_bytes,
            8 * mib
        );
        assert_eq!(
            ConfigurationTransportPolicyV1::MODEL_DISCOVERY.max_response_decompressed_bytes,
            16 * mib
        );
        assert_eq!(
            ConfigurationTransportPolicyV1::MODEL_AVAILABILITY_PROBE.max_request_body_bytes,
            64 * kib
        );
        assert_eq!(
            ConfigurationTransportPolicyV1::MODEL_AVAILABILITY_PROBE.max_response_wire_bytes,
            mib
        );
        assert_eq!(
            ConfigurationTransportPolicyV1::MODEL_AVAILABILITY_PROBE
                .max_response_decompressed_bytes,
            2 * mib
        );
        assert_eq!(ConfigurationTransportPolicyV1::MODEL_DISCOVERY_OPERATION.max_pages, 16);
        assert_eq!(ConfigurationTransportPolicyV1::MODEL_DISCOVERY_OPERATION.max_models, 10_000);
        assert_eq!(
            ConfigurationTransportPolicyV1::MODEL_DISCOVERY_OPERATION
                .max_aggregate_decompressed_bytes,
            32 * mib
        );
    }

    #[test]
    fn agent_response_limit_is_nonzero_checked_and_clamped() {
        let mib = 1_024_u64 * 1_024;

        assert_eq!(ConfigurationTransportPolicyV1::agent_model_response_limit_bytes(0), None);
        assert_eq!(
            ConfigurationTransportPolicyV1::agent_model_response_limit_bytes(1),
            Some(32 * mib)
        );
        assert_eq!(
            ConfigurationTransportPolicyV1::agent_model_response_limit_bytes(100_000),
            Some(8 * mib + 100_000 * 256)
        );
        assert_eq!(
            ConfigurationTransportPolicyV1::agent_model_response_limit_bytes(u64::MAX),
            Some(128 * mib)
        );
    }

    #[test]
    fn purpose_context_and_redirect_mismatches_fail_closed() {
        let endpoint_id: EndpointId = fixed("end_01ARZ3NDEKTSV4RRFFQ69G5FB2");
        let discovery_run_id: ModelDiscoveryRunId = fixed("mdr_01ARZ3NDEKTSV4RRFFQ69G5FB3");
        let identity = identity(endpoint_id);
        let mut manifest = manifest(&identity, discovery_run_id);
        let wrong_context = TransportOperationContext::ModelDiscovery {
            discovery_run_id: ModelDiscoveryRunId::new(),
        };
        assert_eq!(
            manifest.validate(&identity, &wrong_context),
            Err(ProviderGrantContractError::PurposeContext)
        );
        manifest.redirect_policy = RedirectPolicy::SameOrigin { max_hops: 2 };
        let context = TransportOperationContext::ModelDiscovery { discovery_run_id };
        assert_eq!(
            manifest.validate(&identity, &context),
            Err(ProviderGrantContractError::RedirectPolicy)
        );
    }
}
