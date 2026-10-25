use std::str::FromStr;

use aura_domain_common::{
    ContentHash, CredentialId, CredentialRef, CredentialRefParseError, EndpointId,
};
use aura_model_domain::{
    CredentialBindingFingerprint, EndpointAuthScheme, ModelApiFormat, ModelCredentialSlot,
    ModelDiscoveryMode, ModelEndpointConfig,
};
use aura_runtime_contract::BundleNamespace;
use serde::Serialize;
use thiserror::Error;
use url::Url;

#[derive(Debug, Error)]
pub enum ModelUrlError {
    #[error("model base URL is invalid")]
    InvalidBaseUrl,
    #[error("model base URL must not contain user information")]
    UserInfoForbidden,
    #[error("model base URL must not contain a query or fragment")]
    BaseUrlQueryOrFragment,
    #[error("model API path must be a non-blank same-origin relative path or query")]
    InvalidRelativePath,
    #[error("model API path escaped the configured origin")]
    OriginChanged,
    #[error("failed to encode the endpoint fingerprint")]
    FingerprintEncoding,
}

#[derive(Serialize)]
struct EndpointFingerprintInput<'a> {
    endpoint_id: EndpointId,
    api_format: ModelApiFormat,
    normalized_base_url: &'a str,
    discovery_mode: &'a ModelDiscoveryMode,
    chat_output_token_field: Option<aura_model_domain::ChatOutputTokenField>,
    chat_instruction_role: Option<aura_model_domain::ChatInstructionRole>,
    auth_scheme: EndpointAuthScheme,
    auth_injection_kind: &'static str,
    openai_organization_id: Option<&'a str>,
    openai_project_id: Option<&'a str>,
    anthropic_workspace_id: Option<&'a str>,
}

#[derive(Serialize)]
struct CredentialBindingFingerprintInput<'a> {
    bundle_namespace_key: &'a str,
    credential_id: CredentialId,
    backend_kind: &'static str,
    wincred_type: &'static str,
    final_target: &'a str,
    credential_slot: ModelCredentialSlot,
}

const CREDENTIAL_BACKEND_KIND: &str = "wincred";
const WINCRED_TYPE: &str = "generic";

pub fn normalize_base_url(value: &str) -> Result<Url, ModelUrlError> {
    let mut url = Url::parse(value).map_err(|_| ModelUrlError::InvalidBaseUrl)?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ModelUrlError::UserInfoForbidden);
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(ModelUrlError::BaseUrlQueryOrFragment);
    }
    let mut path = url.path().to_owned();
    if !path.ends_with('/') {
        path.push('/');
        url.set_path(&path);
    }
    Ok(url)
}

pub fn resolve_api_url(base_url: &str, api_format: ModelApiFormat) -> Result<Url, ModelUrlError> {
    let base = normalize_base_url(base_url)?;
    join_relative(&base, default_api_path(api_format))
}

pub fn resolve_models_url(
    base_url: &str,
    discovery_mode: &ModelDiscoveryMode,
) -> Result<Option<Url>, ModelUrlError> {
    let base = normalize_base_url(base_url)?;
    match discovery_mode {
        ModelDiscoveryMode::Automatic => join_relative(&base, "models").map(Some),
        ModelDiscoveryMode::Custom { path_and_query } => {
            resolve_exact_path_and_query(&base, path_and_query).map(Some)
        }
        ModelDiscoveryMode::Disabled => Ok(None),
    }
}

pub fn endpoint_fingerprint(endpoint: &ModelEndpointConfig) -> Result<ContentHash, ModelUrlError> {
    let normalized = normalize_base_url(&endpoint.base_url)?.to_string();
    let input = EndpointFingerprintInput {
        endpoint_id: endpoint.id,
        api_format: endpoint.api_format,
        normalized_base_url: &normalized,
        discovery_mode: &endpoint.discovery_mode,
        chat_output_token_field: endpoint.chat_output_token_field,
        chat_instruction_role: endpoint.chat_instruction_role,
        auth_scheme: endpoint.auth_scheme,
        auth_injection_kind: auth_injection_kind(endpoint.auth_scheme),
        openai_organization_id: endpoint.openai_organization_id.as_deref(),
        openai_project_id: endpoint.openai_project_id.as_deref(),
        anthropic_workspace_id: endpoint.anthropic_workspace_id.as_deref(),
    };
    let bytes = serde_json::to_vec(&input).map_err(|_| ModelUrlError::FingerprintEncoding)?;
    Ok(ContentHash::blake3(&bytes))
}

pub fn model_credential_final_target(
    bundle_namespace: &BundleNamespace,
    credential_id: CredentialId,
) -> String {
    format!("Aura/{}/credential/{credential_id}", bundle_namespace.object_name_hash())
}

pub fn credential_ref_for_credential(
    bundle_namespace: &BundleNamespace,
    credential_id: CredentialId,
) -> Result<CredentialRef, CredentialRefParseError> {
    CredentialRef::from_str(&format!(
        "wincred://aura/{}/credential/{credential_id}",
        bundle_namespace.object_name_hash()
    ))
}

pub fn credential_binding_fingerprint(
    bundle_namespace: &BundleNamespace,
    credential_id: CredentialId,
    credential_slot: ModelCredentialSlot,
) -> Result<CredentialBindingFingerprint, ModelUrlError> {
    let bundle_namespace_key = bundle_namespace.object_name_hash();
    let final_target = model_credential_final_target(bundle_namespace, credential_id);
    let input = CredentialBindingFingerprintInput {
        bundle_namespace_key: &bundle_namespace_key,
        credential_id,
        backend_kind: CREDENTIAL_BACKEND_KIND,
        wincred_type: WINCRED_TYPE,
        final_target: &final_target,
        credential_slot,
    };
    let bytes = serde_json::to_vec(&input).map_err(|_| ModelUrlError::FingerprintEncoding)?;
    Ok(CredentialBindingFingerprint(ContentHash::sha256(&bytes)))
}

#[must_use]
pub fn model_name_hash(model_name: &str) -> ContentHash {
    ContentHash::blake3(model_name.as_bytes())
}

fn join_relative(base: &Url, value: &str) -> Result<Url, ModelUrlError> {
    let value = value.trim();
    if value.is_empty()
        || value.starts_with("//")
        || value.contains('\\')
        || value.contains('#')
        || Url::parse(value).is_ok()
        || value.split(['/', '?']).any(|segment| matches!(segment, "." | ".."))
    {
        return Err(ModelUrlError::InvalidRelativePath);
    }
    let relative = value.trim_start_matches('/');
    let resolved = base.join(relative).map_err(|_| ModelUrlError::InvalidRelativePath)?;
    if resolved.origin() != base.origin() {
        return Err(ModelUrlError::OriginChanged);
    }
    Ok(resolved)
}

fn resolve_exact_path_and_query(base: &Url, value: &str) -> Result<Url, ModelUrlError> {
    if value.is_empty()
        || !value.starts_with('/')
        || value.starts_with("//")
        || value.contains(['\\', '#', '\r', '\n'])
    {
        return Err(ModelUrlError::InvalidRelativePath);
    }
    let (path, query) =
        value.split_once('?').map_or((value, None), |(path, query)| (path, Some(query)));
    if path.split('/').any(|segment| matches!(segment, "." | "..")) {
        return Err(ModelUrlError::InvalidRelativePath);
    }
    let mut resolved = base.clone();
    resolved.set_path(path);
    resolved.set_query(query);
    resolved.set_fragment(None);
    if resolved.origin() != base.origin() {
        return Err(ModelUrlError::OriginChanged);
    }
    Ok(resolved)
}

const fn default_api_path(api_format: ModelApiFormat) -> &'static str {
    match api_format {
        ModelApiFormat::OpenAiResponses => "responses",
        ModelApiFormat::OpenAiChatCompletions => "chat/completions",
        ModelApiFormat::AnthropicMessages => "messages",
    }
}

const fn auth_injection_kind(auth_scheme: EndpointAuthScheme) -> &'static str {
    match auth_scheme {
        EndpointAuthScheme::None => "none",
        EndpointAuthScheme::OpenAiBearerApiKey => "authorization_bearer",
        EndpointAuthScheme::AnthropicApiKey => "anthropic_x_api_key",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        credential_binding_fingerprint, credential_ref_for_credential,
        model_credential_final_target, normalize_base_url, resolve_api_url, resolve_models_url,
    };
    use aura_domain_common::{ContentHashAlgorithm, CredentialId};
    use aura_model_domain::{ModelApiFormat, ModelCredentialSlot, ModelDiscoveryMode};
    use aura_runtime_contract::BundleNamespace;

    #[test]
    fn joins_api_paths_without_losing_the_configured_base_path() {
        assert_eq!(
            resolve_api_url("https://api.example.test/v1", ModelApiFormat::OpenAiResponses)
                .expect("request URL")
                .as_str(),
            "https://api.example.test/v1/responses"
        );
        assert_eq!(
            resolve_models_url("https://api.example.test/v1", &ModelDiscoveryMode::Automatic)
                .expect("models URL")
                .expect("enabled discovery")
                .as_str(),
            "https://api.example.test/v1/models"
        );
        assert_eq!(
            resolve_models_url(
                "https://api.example.test/v1",
                &ModelDiscoveryMode::Custom { path_and_query: "/models?limit=100".to_owned() }
            )
            .expect("custom models URL")
            .expect("enabled discovery")
            .as_str(),
            "https://api.example.test/models?limit=100"
        );
        assert!(
            resolve_models_url("https://api.example.test/v1", &ModelDiscoveryMode::Disabled)
                .expect("disabled discovery")
                .is_none()
        );
    }

    #[test]
    fn rejects_userinfo_absolute_and_parent_paths() {
        assert!(normalize_base_url("https://user:secret@example.test/v1").is_err());
        assert!(
            resolve_models_url(
                "https://example.test/v1",
                &ModelDiscoveryMode::Custom {
                    path_and_query: "https://other.test/models".to_owned()
                }
            )
            .is_err()
        );
        assert!(
            resolve_models_url(
                "https://example.test/v1",
                &ModelDiscoveryMode::Custom { path_and_query: "/../models".to_owned() }
            )
            .is_err()
        );
    }

    #[test]
    fn credential_binding_uses_sha256_and_only_frozen_storage_identity_inputs() {
        let namespace: BundleNamespace =
            "aura.desktop.test.credential-fingerprint".parse().expect("namespace");
        let credential_id = CredentialId::new();
        let fingerprint =
            credential_binding_fingerprint(&namespace, credential_id, ModelCredentialSlot::ApiKey)
                .expect("binding fingerprint");
        assert_eq!(fingerprint.0.algorithm, ContentHashAlgorithm::Sha256);

        let another_credential_id = CredentialId::new();
        assert_ne!(
            credential_binding_fingerprint(
                &namespace,
                another_credential_id,
                ModelCredentialSlot::ApiKey
            )
            .expect("different credential fingerprint"),
            fingerprint
        );

        let final_target = model_credential_final_target(&namespace, credential_id);
        assert_eq!(
            final_target,
            format!("Aura/{}/credential/{credential_id}", namespace.object_name_hash())
        );
        let reference =
            credential_ref_for_credential(&namespace, credential_id).expect("credential ref");
        assert_eq!(reference.windows_target_name(), final_target);
        assert_eq!(
            reference.as_str(),
            format!("wincred://aura/{}/credential/{credential_id}", namespace.object_name_hash())
        );
    }
}
