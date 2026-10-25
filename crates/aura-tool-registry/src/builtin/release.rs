use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuiltinToolDefinitionSetV1 {
    release_revision: Revision,
    definitions: Vec<BuiltinToolDefinitionV1>,
    entries: Vec<SignedBuiltinReleaseEntryV1>,
    source_hash: ContentHash,
    canonical_hash: CanonicalHashV1,
}

/// Exact source-owned entry in the embedded signed Built-in release.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SignedBuiltinReleaseEntryV1 {
    pub definition: BuiltinToolDefinitionV1,
    pub tool_instance_id: ToolInstanceId,
    pub tool_instance_revision: Revision,
    pub definition_fingerprint: CanonicalHashV1,
    pub source_contract_fingerprint: SignedBuiltinToolContractFingerprintV1,
    pub execution_binding: ToolInstanceExecutionBindingV1,
}

impl SignedBuiltinReleaseEntryV1 {
    pub fn definition_record(
        &self,
        created_at: UtcTimestamp,
    ) -> Result<ToolDefinitionRecordV1, BuiltinToolDefinitionSetError> {
        let record = build_definition_record(
            &self.definition,
            self.execution_binding.builtin_handler(),
            created_at,
        )?;
        if record.definition_fingerprint != self.definition_fingerprint {
            return Err(BuiltinToolDefinitionSetError::DefinitionMismatch);
        }
        Ok(record)
    }

    pub fn instance_record(&self, created_at: UtcTimestamp) -> ToolInstanceRecordV1 {
        ToolInstanceRecordV1 {
            tool_instance_id: self.tool_instance_id,
            tool_id: self.definition.tool_id,
            tool_version: self.definition.tool_version.clone(),
            source_kind: ToolSourceKindV1::BuiltIn,
            source_contract_fingerprint: ToolSourceContractFingerprintV1::BuiltIn {
                signed_builtin_tool_contract_fingerprint: self.source_contract_fingerprint.clone(),
            },
            enabled: true,
            runtime_health: ToolRuntimeHealthV1::Healthy,
            overrides: ToolInstanceOverridesV1 {
                timeout_ms: None,
                max_retries: None,
                max_concurrent_attempts: None,
                subagent_enabled: None,
            },
            entity_revision: self.tool_instance_revision,
            created_at,
            updated_at: created_at,
        }
    }
}

impl BuiltinToolDefinitionSetV1 {
    #[must_use]
    pub fn definitions(&self) -> &[BuiltinToolDefinitionV1] {
        &self.definitions
    }

    #[must_use]
    pub const fn release_revision(&self) -> Revision {
        self.release_revision
    }

    #[must_use]
    pub fn entries(&self) -> &[SignedBuiltinReleaseEntryV1] {
        &self.entries
    }

    #[must_use]
    pub const fn source_hash(&self) -> &ContentHash {
        &self.source_hash
    }

    #[must_use]
    pub const fn canonical_hash(&self) -> &CanonicalHashV1 {
        &self.canonical_hash
    }

    /// Canonical signed Built-in release hash. The embedded source SHA pins
    /// the bytes while this JCS hash is the source reference used by global
    /// Tool Definition projections.
    #[must_use]
    pub const fn signed_release_hash(&self) -> &CanonicalHashV1 {
        &self.canonical_hash
    }

    #[must_use]
    pub fn definition(&self, builtin_key: &str) -> Option<&BuiltinToolDefinitionV1> {
        self.definitions
            .binary_search_by(|definition| definition.builtin_key.as_str().cmp(builtin_key))
            .ok()
            .map(|index| &self.definitions[index])
    }

    /// Resolves a handler only when the exact ToolId and ToolVersion match the
    /// signed embedded definition. There is no name-based or latest-version
    /// fallback.
    pub fn handler(
        &self,
        tool_id: ToolId,
        tool_version: &ToolVersion,
    ) -> Result<BuiltinCoreHandlerV1, BuiltinToolDefinitionSetError> {
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.definition.tool_id == tool_id)
            .ok_or(BuiltinToolDefinitionSetError::DefinitionMismatch)?;
        if &entry.definition.tool_version != tool_version {
            return Err(BuiltinToolDefinitionSetError::DefinitionMismatch);
        }
        let handler = entry.execution_binding.builtin_handler();
        (handler.builtin_key() == entry.definition.builtin_key.as_str())
            .then_some(handler)
            .ok_or(BuiltinToolDefinitionSetError::DefinitionMismatch)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BuiltinToolDefinitionSetWireV1 {
    pub(super) schema_version: u32,
    pub(super) release_revision: Revision,
    pub(super) entries: Vec<SignedBuiltinReleaseEntryV1>,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum BuiltinToolDefinitionSetError {
    #[error("Agent Built-in {name}: {detail}")]
    AgentDefinition { name: String, detail: String },
    #[error("embedded Builtin Tool Definition Set source hash is invalid")]
    SourceHashMismatch,
    #[error("Builtin Tool Definition Set JSON or schema is invalid")]
    InvalidDocument,
    #[error("Builtin Tool Definition Set schema version is unsupported")]
    UnsupportedSchemaVersion,
    #[error("Builtin Tool Definition Set entries are not canonical or do not match Core handlers")]
    DefinitionMismatch,
    #[error("Builtin Tool Definition Set does not match the signed Tool ID Registry")]
    IdRegistryMismatch,
}

static BUILTIN_DEFINITION_SET: OnceLock<
    Result<BuiltinToolDefinitionSetV1, BuiltinToolDefinitionSetError>,
> = OnceLock::new();

pub fn embedded_builtin_tool_definition_set_v1()
-> Result<&'static BuiltinToolDefinitionSetV1, BuiltinToolDefinitionSetError> {
    BUILTIN_DEFINITION_SET.get_or_init(parse_embedded_definition_set).as_ref().map_err(Clone::clone)
}

/// Deterministic generator input used by `aura-contract-schema`. Production
/// startup never trusts this reconstruction in place of the embedded,
/// source-hash-pinned resource.
pub fn generated_builtin_tool_definition_set_source_v1()
-> Result<Vec<u8>, BuiltinToolDefinitionSetError> {
    let mut bytes = serde_json::to_vec_pretty(&built_definition_set_wire_v1()?)
        .map_err(|_| BuiltinToolDefinitionSetError::InvalidDocument)?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub(super) fn built_definition_set_wire_v1()
-> Result<BuiltinToolDefinitionSetWireV1, BuiltinToolDefinitionSetError> {
    Ok(BuiltinToolDefinitionSetWireV1 {
        schema_version: BUILTIN_TOOL_DEFINITION_SET_SCHEMA_VERSION_V1,
        release_revision: Revision::INITIAL,
        entries: build_builtin_release_entries()?,
    })
}

/// The definition set built **from Rust source**, with no dependency on the checked-in artifact.
///
/// WHY THIS EXISTS
/// The agent release generator must be able to run on the very change that adds a definition. It
/// cannot read the artifact it is about to produce: on that change the file on disk is stale by
/// construction, so a generator that read it back would fail before it could correct it - a
/// bootstrapping cycle. This builds the set from the Rust definition instead, which is the actual
/// source of truth.
///
/// The set is validated against the Tool ID Registry exactly as the embedded one is, so a
/// definition added without its registry entry is still refused here rather than silently accepted.
pub fn built_definition_set_v1() -> Result<BuiltinToolDefinitionSetV1, BuiltinToolDefinitionSetError>
{
    let wire = built_definition_set_wire_v1()?;
    let id_registry = embedded_builtin_tool_id_registry_v1()
        .map_err(|_| BuiltinToolDefinitionSetError::IdRegistryMismatch)?;
    if wire.entries.len() != id_registry.entries().len()
        || wire.entries.iter().zip(id_registry.entries()).any(|(entry, id)| {
            entry.definition.builtin_key != id.builtin_key || entry.definition.tool_id != id.tool_id
        })
    {
        return Err(BuiltinToolDefinitionSetError::IdRegistryMismatch);
    }
    let instance_ids =
        wire.entries.iter().map(|entry| entry.tool_instance_id).collect::<BTreeSet<_>>();
    if instance_ids.len() != wire.entries.len()
        || !wire
            .entries
            .windows(2)
            .all(|pair| pair[0].definition.builtin_key < pair[1].definition.builtin_key)
    {
        return Err(BuiltinToolDefinitionSetError::DefinitionMismatch);
    }
    let source_hash = ContentHash::sha256(
        &serde_json::to_vec_pretty(&wire)
            .map_err(|_| BuiltinToolDefinitionSetError::InvalidDocument)?,
    );
    let canonical_hash =
        canonical_sha256(&wire).map_err(|_| BuiltinToolDefinitionSetError::InvalidDocument)?;
    Ok(BuiltinToolDefinitionSetV1 {
        release_revision: wire.release_revision,
        definitions: wire.entries.iter().map(|entry| entry.definition.clone()).collect(),
        entries: wire.entries,
        source_hash,
        canonical_hash,
    })
}

pub(super) fn parse_embedded_definition_set()
-> Result<BuiltinToolDefinitionSetV1, BuiltinToolDefinitionSetError> {
    let source_hash = ContentHash::sha256(BUILTIN_TOOL_DEFINITION_SET_SOURCE);
    if source_hash.hex != BUILTIN_TOOL_DEFINITION_SET_SOURCE_SHA256_V1 {
        return Err(BuiltinToolDefinitionSetError::SourceHashMismatch);
    }
    let value = parse_json_strict(BUILTIN_TOOL_DEFINITION_SET_SOURCE)
        .map_err(|_| BuiltinToolDefinitionSetError::InvalidDocument)?;
    let wire: BuiltinToolDefinitionSetWireV1 = serde_json::from_value(value)
        .map_err(|_| BuiltinToolDefinitionSetError::InvalidDocument)?;
    if wire.schema_version != BUILTIN_TOOL_DEFINITION_SET_SCHEMA_VERSION_V1 {
        return Err(BuiltinToolDefinitionSetError::UnsupportedSchemaVersion);
    }
    let expected = build_builtin_release_entries()?;
    if wire.release_revision != Revision::INITIAL || wire.entries != expected {
        return Err(BuiltinToolDefinitionSetError::DefinitionMismatch);
    }
    let id_registry = embedded_builtin_tool_id_registry_v1()
        .map_err(|_| BuiltinToolDefinitionSetError::IdRegistryMismatch)?;
    if wire.entries.len() != id_registry.entries().len()
        || wire.entries.iter().zip(id_registry.entries()).any(|(entry, id)| {
            entry.definition.builtin_key != id.builtin_key || entry.definition.tool_id != id.tool_id
        })
    {
        return Err(BuiltinToolDefinitionSetError::IdRegistryMismatch);
    }
    let instance_ids =
        wire.entries.iter().map(|entry| entry.tool_instance_id).collect::<BTreeSet<_>>();
    if instance_ids.len() != wire.entries.len()
        || !wire
            .entries
            .windows(2)
            .all(|pair| pair[0].definition.builtin_key < pair[1].definition.builtin_key)
    {
        return Err(BuiltinToolDefinitionSetError::DefinitionMismatch);
    }
    let canonical_hash =
        canonical_sha256(&wire).map_err(|_| BuiltinToolDefinitionSetError::InvalidDocument)?;
    let definitions = wire.entries.iter().map(|entry| entry.definition.clone()).collect();
    Ok(BuiltinToolDefinitionSetV1 {
        release_revision: wire.release_revision,
        definitions,
        entries: wire.entries,
        source_hash,
        canonical_hash,
    })
}
