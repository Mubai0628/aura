use aura_security_contract::{
    NetworkCallerKind, NetworkDataClass, NetworkDestinationClass, ResolvedNetworkDestination,
};

use aura_artifact_contract::ArtifactRecord;
use aura_domain_common::{
    CredentialRef, ExternalGrantId, FileIdentity, NetworkGrantId, NetworkRequestId,
    QuarantineRecordId, RuntimeLeaseId, SandboxExecutionId, SecretNonce, SessionId, UtcTimestamp,
    WorkspaceId, WorkspacePath,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::NetworkEnforcementKind;
use aura_security_contract::{InvocationIdentity, PermissionGrantLifetime};

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "reference_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NetworkDataReference {
    InlineText { label: String, value: String },
    Header { name: String, value: String },
    JsonBody { value: serde_json::Value },
    FileReference { reference_id: String, byte_length: u64 },
    Artifact { artifact_id: String, byte_length: u64 },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RedirectPolicy {
    Deny,
    SameOrigin { max_hops: u8 },
    ReauthorizeEachHop { max_hops: u8 },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkProtocolClass {
    Http,
    HttpsTunnel,
    Tcp,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkDestinationPolicy {
    pub protocol: NetworkProtocolClass,
    pub host: String,
    pub port: u16,
    pub allowed_destination_classes: Vec<NetworkDestinationClass>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkByteLimits {
    pub max_upload_bytes: u64,
    pub max_download_bytes: u64,
    pub max_decompressed_bytes: u64,
}

/// One authorization for one logical Core request with exactly one typed
/// purpose. A purpose may further restrict the common network envelope but may
/// never broaden it.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CoreRequestGrant<P> {
    pub logical_request_id: NetworkRequestId,
    pub purpose: P,
    pub caller: NetworkCallerKind,
    pub destination_policy: NetworkDestinationPolicy,
    pub methods: Vec<String>,
    pub redirect_policy: RedirectPolicy,
    pub data_classes: Vec<NetworkDataClass>,
    pub byte_limits: NetworkByteLimits,
    pub expires_at: UtcTimestamp,
}

/// Request for one Proxy-resolved opaque CONNECT tunnel authorization. The
/// Core freezes the execution/grant/lease and resource budgets, while the
/// Egress Proxy remains responsible for resolving and classifying the target.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OpaqueTunnelGrantRequest {
    pub execution_id: SandboxExecutionId,
    pub execution_lease_id: RuntimeLeaseId,
    pub grant_id: NetworkGrantId,
    pub hostname: String,
    pub port: u16,
    pub byte_limits: NetworkByteLimits,
    pub max_connections: u32,
    pub expires_at: UtcTimestamp,
}

/// Frozen Proxy-side authorization for a blind CONNECT tunnel. The resolved
/// address set is produced and validated by the Proxy, never supplied as an
/// authority-bearing value by Electron, Renderer, or the sandboxed process.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OpaqueTunnelGrant {
    pub execution_id: SandboxExecutionId,
    pub execution_lease_id: RuntimeLeaseId,
    pub grant_id: NetworkGrantId,
    pub hostname: String,
    pub port: u16,
    pub resolved_addresses: Vec<String>,
    pub resolution_expires_at: UtcTimestamp,
    pub byte_limits: NetworkByteLimits,
    pub max_connections: u32,
    pub expires_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterOpaqueTunnelGrantInput {
    pub request: OpaqueTunnelGrantRequest,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterOpaqueTunnelGrantResult {
    pub proxy_host: String,
    pub proxy_port: u16,
    pub access_token: SecretNonce,
    pub grant: OpaqueTunnelGrant,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RevokeEgressGrantInput {
    pub grant_id: NetworkGrantId,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RevokeEgressExecutionInput {
    pub execution_id: SandboxExecutionId,
    pub execution_lease_id: RuntimeLeaseId,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkRequestPlan {
    pub request_id: NetworkRequestId,
    pub identity: InvocationIdentity,
    pub caller: NetworkCallerKind,
    pub method: String,
    pub requested_url: String,
    pub destination: Option<ResolvedNetworkDestination>,
    pub request_data: Vec<NetworkDataReference>,
    pub data_classes: Vec<NetworkDataClass>,
    pub redirect_policy: RedirectPolicy,
    pub byte_limits: NetworkByteLimits,
    pub timeout_ms: u64,
    pub credential_refs: Vec<CredentialRef>,
    pub network_grant_ids: Vec<NetworkGrantId>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkGrantIdentityScope {
    Session,
    TurnTask,
    SandboxExecution,
    SystemComponent,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkGrant {
    pub id: NetworkGrantId,
    pub identity_scope: NetworkGrantIdentityScope,
    pub identity_scope_id: String,
    pub lifetime: PermissionGrantLifetime,
    pub origins: Vec<NormalizedOrigin>,
    pub destination_classes: Vec<NetworkDestinationClass>,
    pub methods: Vec<String>,
    pub allowed_data_classes: Vec<NetworkDataClass>,
    pub max_upload_bytes: Option<u64>,
    pub max_download_bytes: Option<u64>,
    pub source_fingerprint: Option<aura_domain_common::ContentHash>,
    pub expires_at: Option<UtcTimestamp>,
    pub revoked_at: Option<UtcTimestamp>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedOrigin {
    pub scheme: String,
    pub host: String,
    pub port: u16,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorizedNetworkRequest {
    pub plan: NetworkRequestPlan,
    pub destination: ResolvedNetworkDestination,
    pub matched_grant_ids: Vec<NetworkGrantId>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkRequestAuditState {
    Started,
    Authorized,
    Blocked,
    Finished,
    Interrupted,
}

/// What the enforcing component could actually inspect for this network
/// activity. This is deliberately independent from the sandbox enforcement
/// mode so audit consumers cannot infer structured HTTP visibility from a
/// blind CONNECT tunnel or an observed native process.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkInspectionLevel {
    StructuredHttp,
    OpaqueTunnel,
    ObservedNativeOnly,
}

impl NetworkInspectionLevel {
    #[must_use]
    pub const fn for_enforcement(
        enforcement: NetworkEnforcementKind,
    ) -> Option<NetworkInspectionLevel> {
        match enforcement {
            NetworkEnforcementKind::CoreControlPlane => Some(Self::StructuredHttp),
            NetworkEnforcementKind::ObservedNativeNotProxyEnforced => {
                Some(Self::ObservedNativeOnly)
            }
            NetworkEnforcementKind::AppContainerNetworkDenied => None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkRequestAuditRecord {
    pub request_id: NetworkRequestId,
    pub identity: InvocationIdentity,
    pub caller: NetworkCallerKind,
    pub network_enforcement: NetworkEnforcementKind,
    pub inspection_level: NetworkInspectionLevel,
    pub method: String,
    pub origin: Option<NormalizedOrigin>,
    pub destination_classes: Vec<NetworkDestinationClass>,
    pub data_classes: Vec<NetworkDataClass>,
    pub upload_bytes: u64,
    pub download_bytes: u64,
    pub state: NetworkRequestAuditState,
    pub error_code: Option<String>,
    pub started_at: UtcTimestamp,
    pub finished_at: Option<UtcTimestamp>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QuarantineState {
    Quarantined,
    Released,
    Discarded,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QuarantineRecord {
    pub id: QuarantineRecordId,
    pub original_name: String,
    pub source_url: String,
    pub media_type: Option<String>,
    pub content_disposition: Option<String>,
    pub detected_kinds: Vec<String>,
    pub content_hash: aura_domain_common::ContentHash,
    pub byte_length: u64,
    pub state: QuarantineState,
    pub created_at: UtcTimestamp,
    pub resolved_at: Option<UtcTimestamp>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QuarantineRecordDetails {
    pub record: QuarantineRecord,
    #[serde(default)]
    pub artifact: Option<ArtifactRecord>,
    #[serde(default)]
    pub storage_path: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "target_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum QuarantineReleaseTarget {
    Workspace { path: WorkspacePath },
    ExternalGrant { external_grant_id: ExternalGrantId, relative_path: WorkspacePath },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QuarantineReleaseBinding {
    pub quarantine_id: QuarantineRecordId,
    pub content_hash: aura_domain_common::ContentHash,
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub target: QuarantineReleaseTarget,
    pub destination_parent_identity: FileIdentity,
    pub final_destination: aura_security_contract::CanonicalPath,
}

impl QuarantineReleaseBinding {
    pub fn operation_fingerprint(
        &self,
    ) -> Result<aura_domain_common::ContentHash, serde_json::Error> {
        serde_json::to_vec(self).map(|bytes| aura_domain_common::ContentHash::blake3(&bytes))
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseQuarantineInput {
    pub quarantine_id: QuarantineRecordId,
    pub approval_id: aura_domain_common::ApprovalRequestId,
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub target: QuarantineReleaseTarget,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PrepareQuarantineReleaseInput {
    pub quarantine_id: QuarantineRecordId,
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub target: QuarantineReleaseTarget,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PrepareQuarantineReleaseResult {
    pub approval_id: aura_domain_common::ApprovalRequestId,
    pub approval_request_revision: u64,
    pub quarantine_id: QuarantineRecordId,
    pub content_hash: aura_domain_common::ContentHash,
    pub target: QuarantineReleaseTarget,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IssueQuarantineDiscardConfirmationInput {
    pub quarantine_id: QuarantineRecordId,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QuarantineDiscardConfirmation {
    pub quarantine_id: QuarantineRecordId,
    pub content_hash: aura_domain_common::ContentHash,
    pub confirmation_nonce: SecretNonce,
    pub expires_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DiscardQuarantineInput {
    pub quarantine_id: QuarantineRecordId,
    pub confirmation_nonce: SecretNonce,
}
