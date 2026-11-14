//! Package HTTP calls: the exact request is part of the operation, so the approval,
//! the dispatch and the gateway all see the same bytes. Credentials are resolved
//! from the user's binding at each use and never enter the operation.
use super::InstalledPackageTool;
use anyhow::{Result, ensure};
use aura_domain_common::NetworkRequestId;
use aura_execution_contract::OperationContent;
use aura_network_control::GatewayRequest;
use aura_package_service::{package_http, package_services};
use aura_permission_domain::{NetworkByteLimits, NetworkRequestPlan, RedirectPolicy};
use aura_persistence::PersistenceCoordinator;
use aura_security_contract::{NetworkCallerKind, ResourceDescriptor};
use aura_tool_contract::{MAX_HTTP_RESPONSE_DECOMPRESSED_BYTES, MAX_HTTP_RESPONSE_WIRE_BYTES};
use aura_tool_service::tool_credential_service;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

// Why a package HTTP call was not sent, for the model: the cause and what the user can change.
pub(super) const CREDENTIAL_UNBOUND: &str = "http.credential_unbound: 这个工具需要的凭据还没有绑定，请求没有发出。请用户在工具包页给这个工具绑定凭据。";
const CREDENTIAL_CHANGED: &str = "http.credential_changed: 这次调用批准之后，这个工具的凭据绑定变了，请求没有发出。可以重新调用，会按现在的绑定再确认。";
const ORIGIN_MISSING: &str =
    "package.origin_missing: 这次调用的记录里没有它要请求的网站，请求没有发出。可以重新调用。";

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EmbeddedRequest {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body_base64: String,
    success_status_ranges: Vec<(u16, u16)>,
}

/// The operation action: the model's arguments plus the exact request Core built.
pub(in crate::agent_execution) fn action(
    arguments: &Value,
    request: &package_http::PackageHttpRequest,
    credential: Option<&aura_permission_domain::ToolCredentialUseV1>,
) -> Result<Value> {
    let hash =
        aura_network_control::ordinary_tools::ordinary_request_hash(&request.gateway, credential)?;
    Ok(json!({
        "arguments": arguments,
        "request": EmbeddedRequest {
            method: request.gateway.plan.method.clone(),
            url: request.gateway.plan.requested_url.clone(),
            headers: request.gateway.headers.clone(),
            body_base64: base64::engine::general_purpose::STANDARD.encode(&request.gateway.body),
            success_status_ranges: request.prepared.success_status_ranges.clone(),
        },
        "network_request_hash": hash,
    }))
}

fn embedded(content: &OperationContent) -> Result<EmbeddedRequest> {
    Ok(serde_json::from_value(content.action["request"].clone())?)
}

/// The gateway request an approved package operation names, rebuilt from the
/// operation itself. Redirects are refused: the package declared one origin.
pub(crate) fn gateway_request(content: &OperationContent) -> Result<GatewayRequest> {
    let request = embedded(content)?;
    let body = base64::engine::general_purpose::STANDARD.decode(&request.body_base64)?;
    let scheme = url::Url::parse(&request.url)?.scheme().to_owned();
    ensure!(
        scheme == "https",
        "package.https_required: 这个工具只能请求 https:// 地址，这次要请求的是 {scheme}:// 地址，请求没有发出。请告诉用户这个工具包的地址需要改成 https。"
    );
    Ok(GatewayRequest {
        plan: NetworkRequestPlan {
            request_id: NetworkRequestId::new(),
            identity: content.identity.clone(),
            caller: NetworkCallerKind::AuraTool,
            method: request.method,
            requested_url: request.url,
            destination: None,
            request_data: vec![],
            data_classes: vec![],
            redirect_policy: RedirectPolicy::Deny,
            byte_limits: NetworkByteLimits {
                max_upload_bytes: body.len() as u64,
                max_download_bytes: MAX_HTTP_RESPONSE_WIRE_BYTES as u64,
                max_decompressed_bytes: MAX_HTTP_RESPONSE_DECOMPRESSED_BYTES as u64,
            },
            timeout_ms: 30_000,
            credential_refs: vec![],
            network_grant_ids: vec![],
        },
        headers: request.headers,
        body,
    })
}

/// The success statuses the manifest declared for this exact request.
pub(crate) fn success_ranges(content: &OperationContent) -> Result<Vec<(u16, u16)>> {
    Ok(embedded(content)?.success_status_ranges)
}

/// The URL the operation reaches, for the controlled-web origin binding.
pub(crate) fn target(content: &OperationContent) -> Result<url::Url> {
    Ok(url::Url::parse(&embedded(content)?.url)?)
}

pub(crate) fn credential_target(
    tool: &InstalledPackageTool,
    slot: &aura_tool_contract::CredentialSlotKey,
) -> Result<aura_tool_domain::ToolCredentialManagementTargetV1> {
    let (_, placement, _) =
        package_services::http_credential(&tool.tool, tool.settings.as_slice())?
            .filter(|(declared, _, _)| declared == slot)
            .ok_or_else(|| anyhow::anyhow!(CREDENTIAL_UNBOUND))?;
    Ok(aura_tool_domain::ToolCredentialManagementTargetV1 {
        tool_instance_id: tool.instance.tool_instance_id,
        expected_instance_revision: tool.trust_revision,
        credential_slot_key: slot.clone(),
        placement,
        workspace_id: None,
        session_id: None,
    })
}

/// How the gateway places the bound credential. Only HTTP placements exist here.
pub(crate) fn usage_placement(
    placement: &aura_tool_domain::ToolCredentialPlacementV1,
) -> Result<aura_permission_domain::ToolCredentialPlacementV1> {
    use aura_permission_domain::ToolCredentialPlacementV1 as Use;
    use aura_tool_domain::ToolCredentialPlacementV1 as Declared;
    Ok(match placement {
        Declared::HttpBearerAuthorization => Use::BearerAuthorization,
        Declared::HttpFixedHeader { header_name } => {
            Use::FixedHeader { header_name: header_name.clone() }
        }
        Declared::CommandEnvironment { .. } => anyhow::bail!(
            "http.credential_placement_invalid: 这个工具的凭据设成了给程序用的环境变量，HTTP 请求用不了它，请求没有发出。请告诉用户这个工具包的凭据设置有误。"
        ),
    })
}

/// The user's bound credential as it stands now: still configured, at the bound
/// revision, and still allowing this exact origin.
pub(crate) fn current_credential(
    persistence: &PersistenceCoordinator,
    tool: &InstalledPackageTool,
    target: &aura_tool_domain::ToolCredentialManagementTargetV1,
    origin: &ResourceDescriptor,
) -> Result<aura_tool_domain::ToolCredentialAuthorityRecordV1> {
    let binding = tool.credential.as_ref().ok_or_else(|| anyhow::anyhow!(CREDENTIAL_UNBOUND))?;
    let record = persistence
        .list_managed_tool_credentials_v1(target.clone())?
        .into_iter()
        .find(|record| record.credential_id == binding.credential_id)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "http.credential_unbound: 这个工具绑定的凭据已经被删除，请求没有发出。请用户在工具包页给它重新绑定凭据。"
            )
        })?;
    ensure!(
        record.state == aura_tool_domain::ToolCredentialStateV1::Configured
            && record.credential_revision == binding.credential_revision,
        "http.credential_rebind_required: 这个工具绑定的凭据之后改过或需要重新填写，请求没有发出。请用户在工具包页重新绑定。"
    );
    let allowed = origin_text(origin)?;
    ensure!(
        record.allowed_origins.binary_search(&allowed).is_ok(),
        "http.credential_origin_denied: 这个工具绑定的凭据不允许发往 {allowed}，请求没有发出。请用户在工具包页把这个网站加进凭据的允许网站。"
    );
    Ok(record)
}

/// `https://host[:port]` as credential allowed websites spell it.
pub(crate) fn origin_text(resource: &ResourceDescriptor) -> Result<String> {
    let ResourceDescriptor::NetworkOrigin { scheme, host, port } = resource else {
        anyhow::bail!(ORIGIN_MISSING)
    };
    let url = url::Url::parse(&format!("{scheme}://{host}:{port}/"))?;
    Ok(url.origin().ascii_serialization())
}

/// The secret for one approved package HTTP operation, re-checked against the
/// user's current binding. The operation names the credential it was approved with.
pub(crate) fn load_secret(
    service: &super::super::AgentExecution,
    content: &OperationContent,
    tool: &InstalledPackageTool,
) -> Result<
    Option<(
        aura_permission_domain::ToolCredentialUseV1,
        tool_credential_service::execution::HttpSecret,
    )>,
> {
    let Some(slot) = &tool.credential_slot else {
        ensure!(content.credentials.is_empty(), CREDENTIAL_CHANGED);
        return Ok(None);
    };
    let binding = tool.credential.as_ref().ok_or_else(|| anyhow::anyhow!(CREDENTIAL_UNBOUND))?;
    let reference = content
        .credentials
        .iter()
        .find(|reference| {
            reference.credential_id == binding.credential_id
                && reference.credential_revision == binding.credential_revision
        })
        .ok_or_else(|| anyhow::anyhow!(CREDENTIAL_CHANGED))?;
    ensure!(content.credentials.len() == 1, CREDENTIAL_CHANGED);
    let target = credential_target(tool, slot)?;
    let origin_resource = content
        .resources
        .iter()
        .find(|resource| matches!(resource, ResourceDescriptor::NetworkOrigin { .. }))
        .ok_or_else(|| anyhow::anyhow!(ORIGIN_MISSING))?;
    let ResourceDescriptor::NetworkOrigin { scheme, host, port } = origin_resource else {
        anyhow::bail!(ORIGIN_MISSING)
    };
    // The record may have changed since approval: re-check it at the point of use.
    current_credential(&service.persistence, tool, &target, origin_resource)?;
    let usage = aura_permission_domain::ToolCredentialUseV1 {
        credential_id: reference.credential_id,
        credential_revision: reference.credential_revision,
        slot_key: slot.clone(),
        placement: usage_placement(&target.placement)?,
    };
    let secret = tool_credential_service::execution::HttpSecret::load_managed(
        &service.persistence,
        target,
        usage.clone(),
        aura_permission_domain::NormalizedOrigin {
            scheme: scheme.clone(),
            host: host.clone(),
            port: *port,
        },
    )?;
    Ok(Some((usage, secret)))
}
