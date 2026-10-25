use super::*;

/// Closed Core handler identity covered by the signed Built-in release.
///
/// It is serializable only for the embedded, Core-owned release resource and
/// SQLite execution binding. It is intentionally not a JSON Schema type and
/// is never accepted from a Package, model, Renderer, or public Runtime
/// contract.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BuiltinCoreHandlerV1 {
    ToolsSearch,
    ToolsDescribe,
    ToolsLoad,
    ToolsUnload,
    ToolsListLoaded,
    /// The Agent-executable run-information query.
    ///
    /// The route `agent.activity.query` already existed for Main↔Core IPC, but the model had no way
    /// to ask it: the automatic summary named no query entry, so the documented capability ("ask who
    /// else is touching the file you just operated on") was unreachable from a model step. This
    /// handler is that entry. It is deliberately a **read-only observer**: it takes no resource
    /// claim, starts and stops nothing, and cannot influence any tool's fate. See
    /// [`Self::is_read_only_observer`].
    ActivityQuery,
}

impl BuiltinCoreHandlerV1 {
    #[must_use]
    pub const fn builtin_key(self) -> &'static str {
        match self {
            Self::ToolsSearch => "tools.search",
            Self::ToolsDescribe => "tools.describe",
            Self::ToolsLoad => "tools.load",
            Self::ToolsUnload => "tools.unload",
            Self::ToolsListLoaded => "tools.list_loaded",
            Self::ActivityQuery => "agent.activity.query",
        }
    }

    /// Whether this handler only observes, so it needs no resource scope of its own.
    ///
    /// A query about what is running changes nothing, so demanding a resource scope for it would be
    /// asking permission for an effect that does not exist - and, worse, would make the model's
    /// query fail exactly when a resource is busy, which is the moment it is most useful.
    #[must_use]
    pub const fn is_read_only_observer(self) -> bool {
        matches!(self, Self::ActivityQuery)
    }

    #[must_use]
    pub const fn all() -> &'static [Self] {
        &[
            Self::ToolsSearch,
            Self::ToolsDescribe,
            Self::ToolsLoad,
            Self::ToolsUnload,
            Self::ToolsListLoaded,
            Self::ActivityQuery,
        ]
    }
}

/// Source-owned execution binding kept outside Manifest V1 and public Runtime
/// contracts.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolInstanceExecutionBindingV1 {
    BuiltInCoreHandler { handler: BuiltinCoreHandlerV1 },
}

impl ToolInstanceExecutionBindingV1 {
    #[must_use]
    pub const fn builtin_handler(self) -> BuiltinCoreHandlerV1 {
        match self {
            Self::BuiltInCoreHandler { handler } => handler,
        }
    }
}

/// Durable P04-to-Core one-shot binding for one signed Built-in handler
/// attempt. This is an internal execution record, not a Package Manifest or
/// public Runtime contract.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BuiltinCoreHandlerDispatchBindingV1 {
    pub binding_version: u32,
    pub operation_id: SecurityOperationId,
    pub operation_identity: InvocationIdentity,
    pub operation_fingerprint: ContentHash,
    pub permission_snapshot: PermissionSnapshot,
    pub authorization_basis: PermissionAllowAuthorizationBasisV1,
    pub tool_batch_id: ToolBatchId,
    pub tool_call_id: ToolCallId,
    pub tool_execution_attempt_id: ToolExecutionAttemptId,
    pub execution_plan_hash: CanonicalHashV1,
    pub source: ToolSecuritySourceRefV1,
    pub source_security_fingerprint: ContentHash,
    pub handler: BuiltinCoreHandlerV1,
    pub prepared_at: UtcTimestamp,
    pub binding_hash: CanonicalHashV1,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BuiltinCoreHandlerDispatchBindingHashMaterialV1<'a> {
    binding_version: u32,
    operation_id: SecurityOperationId,
    operation_identity: &'a InvocationIdentity,
    operation_fingerprint: &'a ContentHash,
    permission_snapshot: &'a PermissionSnapshot,
    authorization_basis: &'a PermissionAllowAuthorizationBasisV1,
    tool_batch_id: ToolBatchId,
    tool_call_id: ToolCallId,
    tool_execution_attempt_id: ToolExecutionAttemptId,
    execution_plan_hash: &'a CanonicalHashV1,
    source: &'a ToolSecuritySourceRefV1,
    source_security_fingerprint: &'a ContentHash,
    handler: BuiltinCoreHandlerV1,
    prepared_at: UtcTimestamp,
}

impl BuiltinCoreHandlerDispatchBindingV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        operation_id: SecurityOperationId,
        operation_identity: InvocationIdentity,
        operation_fingerprint: ContentHash,
        permission_snapshot: PermissionSnapshot,
        authorization_basis: PermissionAllowAuthorizationBasisV1,
        tool_batch_id: ToolBatchId,
        tool_call_id: ToolCallId,
        tool_execution_attempt_id: ToolExecutionAttemptId,
        execution_plan_hash: CanonicalHashV1,
        source: ToolSecuritySourceRefV1,
        source_security_fingerprint: ContentHash,
        handler: BuiltinCoreHandlerV1,
        prepared_at: UtcTimestamp,
    ) -> Result<Self, BuiltinCoreHandlerDispatchBindingErrorV1> {
        let mut value = Self {
            binding_version: BUILTIN_CORE_HANDLER_DISPATCH_BINDING_VERSION_V1,
            operation_id,
            operation_identity,
            operation_fingerprint,
            permission_snapshot,
            authorization_basis,
            tool_batch_id,
            tool_call_id,
            tool_execution_attempt_id,
            execution_plan_hash,
            source,
            source_security_fingerprint,
            handler,
            prepared_at,
            binding_hash: CanonicalHashV1::sha256(&[]),
        };
        value.binding_hash = value.compute_hash()?;
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), BuiltinCoreHandlerDispatchBindingErrorV1> {
        let authorization_basis_matches_snapshot = match &self.authorization_basis {
            PermissionAllowAuthorizationBasisV1::PermissionMode { mode } => {
                *mode == self.permission_snapshot.mode
            }
            PermissionAllowAuthorizationBasisV1::PermissionGrant {
                permission_grant_id, ..
            } => self.permission_snapshot.active_grant_ids.contains(permission_grant_id),
        };
        if self.binding_version != BUILTIN_CORE_HANDLER_DISPATCH_BINDING_VERSION_V1
            || self.operation_identity.permission_snapshot_id != Some(self.permission_snapshot.id)
            || self.operation_identity.workspace_id != Some(self.permission_snapshot.workspace_id)
            || self.operation_identity.session_id != Some(self.permission_snapshot.session_id)
            || !authorization_basis_matches_snapshot
            || self.source.source_kind != ToolSourceKindV1::BuiltIn
            || self.binding_hash != self.compute_hash()?
        {
            return Err(BuiltinCoreHandlerDispatchBindingErrorV1::BindingInvalid);
        }
        self.source
            .validate()
            .map_err(|_| BuiltinCoreHandlerDispatchBindingErrorV1::SourceMismatch)?;
        let release = embedded_builtin_tool_definition_set_v1()
            .map_err(|_| BuiltinCoreHandlerDispatchBindingErrorV1::ReleaseUnavailable)?;
        let entry = release
            .entries()
            .iter()
            .find(|entry| {
                entry.definition.tool_id == self.source.definition_ref.tool_id
                    && entry.definition.tool_version == self.source.definition_ref.tool_version
                    && entry.tool_instance_id == self.source.tool_instance_id
            })
            .ok_or(BuiltinCoreHandlerDispatchBindingErrorV1::SourceMismatch)?;
        if entry.definition_fingerprint != self.source.definition_fingerprint
            || (ToolSourceContractFingerprintV1::BuiltIn {
                signed_builtin_tool_contract_fingerprint: entry.source_contract_fingerprint.clone(),
            }) != self.source.source_contract_fingerprint
            || entry.execution_binding.builtin_handler() != self.handler
            || self.handler.builtin_key() != entry.definition.builtin_key.as_str()
        {
            return Err(BuiltinCoreHandlerDispatchBindingErrorV1::SourceMismatch);
        }
        Ok(())
    }

    fn compute_hash(&self) -> Result<CanonicalHashV1, BuiltinCoreHandlerDispatchBindingErrorV1> {
        canonical_sha256(&BuiltinCoreHandlerDispatchBindingHashMaterialV1 {
            binding_version: self.binding_version,
            operation_id: self.operation_id,
            operation_identity: &self.operation_identity,
            operation_fingerprint: &self.operation_fingerprint,
            permission_snapshot: &self.permission_snapshot,
            authorization_basis: &self.authorization_basis,
            tool_batch_id: self.tool_batch_id,
            tool_call_id: self.tool_call_id,
            tool_execution_attempt_id: self.tool_execution_attempt_id,
            execution_plan_hash: &self.execution_plan_hash,
            source: &self.source,
            source_security_fingerprint: &self.source_security_fingerprint,
            handler: self.handler,
            prepared_at: self.prepared_at,
        })
        .map_err(|_| BuiltinCoreHandlerDispatchBindingErrorV1::Canonicalization)
    }
}

impl<'de> Deserialize<'de> for BuiltinCoreHandlerDispatchBindingV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            binding_version: u32,
            operation_id: SecurityOperationId,
            operation_identity: InvocationIdentity,
            operation_fingerprint: ContentHash,
            permission_snapshot: PermissionSnapshot,
            authorization_basis: PermissionAllowAuthorizationBasisV1,
            tool_batch_id: ToolBatchId,
            tool_call_id: ToolCallId,
            tool_execution_attempt_id: ToolExecutionAttemptId,
            execution_plan_hash: CanonicalHashV1,
            source: ToolSecuritySourceRefV1,
            source_security_fingerprint: ContentHash,
            handler: BuiltinCoreHandlerV1,
            prepared_at: UtcTimestamp,
            binding_hash: CanonicalHashV1,
        }
        let wire = Wire::deserialize(deserializer)?;
        let value = Self {
            binding_version: wire.binding_version,
            operation_id: wire.operation_id,
            operation_identity: wire.operation_identity,
            operation_fingerprint: wire.operation_fingerprint,
            permission_snapshot: wire.permission_snapshot,
            authorization_basis: wire.authorization_basis,
            tool_batch_id: wire.tool_batch_id,
            tool_call_id: wire.tool_call_id,
            tool_execution_attempt_id: wire.tool_execution_attempt_id,
            execution_plan_hash: wire.execution_plan_hash,
            source: wire.source,
            source_security_fingerprint: wire.source_security_fingerprint,
            handler: wire.handler,
            prepared_at: wire.prepared_at,
            binding_hash: wire.binding_hash,
        };
        value.validate().map_err(serde::de::Error::custom)?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum BuiltinCoreHandlerDispatchBindingErrorV1 {
    #[error("the Built-in Core handler dispatch binding is invalid")]
    BindingInvalid,
    #[error("the Built-in Core handler dispatch source does not match the signed release")]
    SourceMismatch,
    #[error("the signed Built-in release is unavailable")]
    ReleaseUnavailable,
    #[error("the Built-in Core handler dispatch binding could not be canonicalized")]
    Canonicalization,
}
