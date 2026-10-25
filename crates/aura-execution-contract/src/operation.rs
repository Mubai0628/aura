use aura_domain_common::jcs::canonical_sha256;
use aura_domain_common::{SecurityOperationId, ToolWireVersionV1};
use aura_security_contract::{
    CredentialUseRef, InvocationIdentity, OperationFingerprint, OperationRiskFlag,
    ResolvedCapability, ResourceDescriptor, SecuritySourceRef,
};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

/// Immutable domain action facts. The domain owns the action contract and
/// decoder. Backend selection cannot rewrite this canonical payload.
#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OperationContent {
    pub compatibility: std::collections::BTreeSet<crate::ExecutionCompatibility>,
    pub network: crate::OperationNetwork,
    pub identity: InvocationIdentity,
    pub source: SecuritySourceRef,
    pub source_revision_fingerprint: Option<aura_domain_common::ContentHash>,
    pub capabilities: Vec<ResolvedCapability>,
    pub resources: Vec<ResourceDescriptor>,
    pub risks: Vec<OperationRiskFlag>,
    pub credentials: Vec<CredentialUseRef>,
    pub purpose: Option<String>,
    pub action_kind: String,
    pub action_revision: aura_domain_common::Revision,
    pub action: serde_json::Value,
}

#[derive(Clone, Debug, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SecuredOperation {
    pub schema_version: ToolWireVersionV1,
    pub operation_id: SecurityOperationId,
    pub content: OperationContent,
    pub fingerprint: OperationFingerprint,
}

#[derive(Clone, Debug, thiserror::Error, Eq, PartialEq)]
pub enum ExecutionContractError {
    #[error("operation content is incomplete or not canonical JSON")]
    InvalidContent,
    #[error("immutable operation, plan, or dispatch fingerprint mismatch")]
    FingerprintMismatch,
    #[error("authorization is stale, expired or belongs to a different operation")]
    AuthorizationMismatch,
    #[error("backend does not meet the exact authorization requirements")]
    BackendMismatch,
    #[error("attempt identity, cancellation state or lifecycle evidence is inconsistent")]
    AttemptMismatch,
    #[error("terminal result lacks compatible side-effect or termination evidence")]
    InvalidResult,
}

impl SecuredOperation {
    pub fn new(
        operation_id: SecurityOperationId,
        content: OperationContent,
    ) -> Result<Self, ExecutionContractError> {
        let fingerprint = OperationFingerprint(
            canonical_sha256(&content).map_err(|_| ExecutionContractError::InvalidContent)?,
        );
        let operation =
            Self { schema_version: ToolWireVersionV1, operation_id, content, fingerprint };
        operation.validate()?;
        Ok(operation)
    }

    pub fn validate(&self) -> Result<(), ExecutionContractError> {
        let c = &self.content;
        if c.capabilities.is_empty()
            || c.resources.is_empty()
            || c.action_kind.trim().is_empty()
            || c.action_revision.value() == 0
            || c.identity.requires_model_purpose()
                && c.purpose.as_deref().is_none_or(|p| p.trim().is_empty())
        {
            return Err(ExecutionContractError::InvalidContent);
        }
        let canonical = canonical_sha256(c).map_err(|_| ExecutionContractError::InvalidContent)?;
        if canonical == self.fingerprint.0 {
            return Ok(());
        }
        // Operations written after protected-path identity was added, and before unbound
        // identity was omitted, stored `identity: null` inside the signed content. That
        // shape is the only other accepted canonical form. It is not a second reading of an
        // arbitrary fingerprint: the path, parent, and any real identity stay in the hash,
        // and a fingerprint that matches neither form is still rejected.
        if protected_path_explicit_null_identity(c)
            .map_err(|_| ExecutionContractError::InvalidContent)?
            == self.fingerprint.0
        {
            return Ok(());
        }
        Err(ExecutionContractError::FingerprintMismatch)
    }
}

/// Canonical bytes of `content` with unbound system and sensitive-user identities written
/// as explicit null, which is the intermediate shape those operations were signed with.
fn protected_path_explicit_null_identity(
    content: &OperationContent,
) -> Result<
    aura_domain_common::jcs::CanonicalSha256HashV1,
    aura_domain_common::jcs::CanonicalJsonError,
> {
    let mut value = serde_json::to_value(content)
        .map_err(|_| aura_domain_common::jcs::CanonicalJsonError::Serialization)?;
    let Some(resources) = value.get_mut("resources").and_then(|item| item.as_array_mut()) else {
        return canonical_sha256(&value);
    };
    for resource in resources {
        let kind = resource.get("scope_kind").and_then(|item| item.as_str());
        if !matches!(kind, Some("system_path" | "sensitive_user_path")) {
            continue;
        }
        let Some(object) = resource.as_object_mut() else { continue };
        if !object.contains_key("identity") {
            object.insert("identity".to_owned(), serde_json::Value::Null);
        }
    }
    canonical_sha256(&value)
}

impl<'de> Deserialize<'de> for SecuredOperation {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            schema_version: ToolWireVersionV1,
            operation_id: SecurityOperationId,
            content: OperationContent,
            fingerprint: OperationFingerprint,
        }
        let w = Wire::deserialize(deserializer)?;
        let operation = Self {
            schema_version: w.schema_version,
            operation_id: w.operation_id,
            content: w.content,
            fingerprint: w.fingerprint,
        };
        operation.validate().map_err(D::Error::custom)?;
        Ok(operation)
    }
}
