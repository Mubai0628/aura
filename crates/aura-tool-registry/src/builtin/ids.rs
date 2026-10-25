use super::*;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BuiltinToolIdEntryV1 {
    pub builtin_key: ToolCanonicalName,
    pub tool_id: ToolId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuiltinToolIdRegistryV1 {
    entries: Vec<BuiltinToolIdEntryV1>,
    source_hash: ContentHash,
    canonical_hash: CanonicalHashV1,
}

impl BuiltinToolIdRegistryV1 {
    #[must_use]
    pub fn entries(&self) -> &[BuiltinToolIdEntryV1] {
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

    #[must_use]
    pub fn tool_id(&self, builtin_key: &str) -> Option<ToolId> {
        self.entries
            .binary_search_by(|entry| entry.builtin_key.as_str().cmp(builtin_key))
            .ok()
            .map(|index| self.entries[index].tool_id)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BuiltinToolIdRegistryWireV1 {
    schema_version: u32,
    entries: Vec<BuiltinToolIdEntryV1>,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum BuiltinToolIdRegistryError {
    #[error("embedded Builtin Tool ID Registry source hash is invalid")]
    SourceHashMismatch,
    #[error("Builtin Tool ID Registry JSON or schema is invalid")]
    InvalidDocument,
    #[error("Builtin Tool ID Registry schema version is unsupported")]
    UnsupportedSchemaVersion,
    #[error("Builtin Tool ID Registry entries are not canonically ordered and unique")]
    InvalidEntryOrder,
    #[error("Builtin Tool ID Registry contains a duplicate ToolId")]
    DuplicateToolId,
}

static BUILTIN_REGISTRY: OnceLock<Result<BuiltinToolIdRegistryV1, BuiltinToolIdRegistryError>> =
    OnceLock::new();

pub fn embedded_builtin_tool_id_registry_v1()
-> Result<&'static BuiltinToolIdRegistryV1, BuiltinToolIdRegistryError> {
    BUILTIN_REGISTRY.get_or_init(parse_embedded_registry).as_ref().map_err(Clone::clone)
}

pub(super) fn parse_embedded_registry()
-> Result<BuiltinToolIdRegistryV1, BuiltinToolIdRegistryError> {
    let source_hash = ContentHash::sha256(BUILTIN_TOOL_ID_REGISTRY_SOURCE);
    if source_hash.hex != BUILTIN_TOOL_ID_REGISTRY_SOURCE_SHA256_V1 {
        return Err(BuiltinToolIdRegistryError::SourceHashMismatch);
    }
    parse_registry(BUILTIN_TOOL_ID_REGISTRY_SOURCE, source_hash)
}

pub(super) fn parse_registry(
    source: &[u8],
    source_hash: ContentHash,
) -> Result<BuiltinToolIdRegistryV1, BuiltinToolIdRegistryError> {
    let value =
        parse_json_strict(source).map_err(|_| BuiltinToolIdRegistryError::InvalidDocument)?;
    let wire: BuiltinToolIdRegistryWireV1 =
        serde_json::from_value(value).map_err(|_| BuiltinToolIdRegistryError::InvalidDocument)?;
    if wire.schema_version != BUILTIN_TOOL_ID_REGISTRY_SCHEMA_VERSION_V1 {
        return Err(BuiltinToolIdRegistryError::UnsupportedSchemaVersion);
    }
    if wire.entries.is_empty()
        || !wire.entries.windows(2).all(|pair| pair[0].builtin_key < pair[1].builtin_key)
    {
        return Err(BuiltinToolIdRegistryError::InvalidEntryOrder);
    }
    let unique_ids = wire.entries.iter().map(|entry| entry.tool_id).collect::<BTreeSet<_>>();
    if unique_ids.len() != wire.entries.len() {
        return Err(BuiltinToolIdRegistryError::DuplicateToolId);
    }
    let canonical_hash =
        canonical_sha256(&wire).map_err(|_| BuiltinToolIdRegistryError::InvalidDocument)?;
    Ok(BuiltinToolIdRegistryV1 { entries: wire.entries, source_hash, canonical_hash })
}
