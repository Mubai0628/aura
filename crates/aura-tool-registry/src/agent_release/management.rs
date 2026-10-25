//! Models supply IDs; the Core binds immutable registry and execution authority.
use super::schema::schema_value;
use super::*;

pub(super) fn entry(
    old: &crate::SignedBuiltinReleaseEntryV1,
    at: UtcTimestamp,
) -> Result<AgentBuiltinEntry, BuiltinToolDefinitionSetError> {
    let handler = old.execution_binding.builtin_handler();
    let d = old.definition_record(at)?;
    let (schema, description) = match handler {
        BuiltinCoreHandlerV1::ToolsDescribe => (
            schema_value::<AgentToolDescribeInput>()?,
            "Describe a tool using its tool_id from tools.search. Core binds the exact definition and version from this request's frozen registry.",
        ),
        BuiltinCoreHandlerV1::ToolsListLoaded => (
            schema_value::<aura_agent_domain::EmptyToolInput>()?,
            "List fixed and dynamic tools in this request. Fixed tools are user-managed and cannot be changed by the model.",
        ),
        // The request's tool list stays the same for a whole turn (the Provider reuses its
        // cached prefix), so a load or unload applies from the next turn and a tool needed now is
        // called through tools.call.
        BuiltinCoreHandlerV1::ToolsLoad => (
            schema_value::<AgentToolLoadInput>()?,
            "Add tool_ids to this session's tool list from the next turn on; the list does not change during a turn, so call a tool you need now through tools.call. Membership persists across restarts. Fixed tools do not use dynamic capacity. To replace dynamic tools atomically put their IDs in replace; otherwise use replace=[]. At capacity explicitly unload or replace; never copy registry hashes.",
        ),
        BuiltinCoreHandlerV1::ToolsUnload => (
            schema_value::<AgentToolUnloadInput>()?,
            "Remove dynamic tool_ids from this session's tool list from the next turn on; this turn's list does not change. Cannot remove fixed tools. Unavailable dynamic members may be removed by ID. Existing execution snapshots remain unchanged.",
        ),
        // Its input stays the signed one; the description names tools.call.
        BuiltinCoreHandlerV1::ToolsSearch => (
            d.input_schema.document().clone(),
            "Search the tools of this step by name, summary and description. Each result gives the tool's canonical_name, tool_id and load_state. Call a found tool with tools.call (tool = its canonical_name), after tools.describe when you need its input schema; a tool in this request's list is called directly. tools.load adds a tool to the request's list from the next turn.",
        ),
        // The model-executable run-information query.
        //
        // The summary names this tool as the entry a model uses to ask who else is touching a file
        // it just operated on. Its input deliberately carries no session and no workspace: the
        // execution resolves the caller's own position, so the tool cannot widen its own visibility
        // by naming one.
        BuiltinCoreHandlerV1::ActivityQuery => (
            activity_query_schema()?,
            "Ask which real activities the runtime is running. Use by=resource with a verified target to find another writer of a file you just operated on, by=unknown_write_set to find a Shell whose write set cannot be proven, by=group or by=session to list your own running tools. Read-only: it takes no file protection, starts and stops nothing, and does not disclose another session's private aliases or parameters. A verified target selects related activity; it grants no authority.",
        ),
    };
    let invalid = || BuiltinToolDefinitionSetError::InvalidDocument;
    // The published table names each management tool's revision and instance, as it does for
    // every other built-in tool. Revision 2 was the first one recorded there.
    let (tool_id, revision, instance_id) = super::revisions::identity(d.canonical_name.as_str())?;
    if tool_id != d.tool_id {
        return Err(invalid());
    }
    let version = ToolVersion::new(None, revision).map_err(|_| invalid())?;
    let definition = ToolDefinitionRecordV1::new(
        d.tool_id,
        version.clone(),
        d.tool_package_id,
        d.package_key,
        d.tool_key,
        d.canonical_name,
        d.display_name,
        description.into(),
        description.into(),
        ToolInputSchemaV1::new(schema).map_err(|_| invalid())?,
        d.strict,
        d.result_contract,
        d.runtime,
        d.capabilities,
        d.resource_claims,
        d.audience,
        d.source_kind,
        d.execution_semantics,
        d.runtime_file_closure_hash,
        revision,
        at,
    )
    .map_err(|_| invalid())?;
    let mut instance = old.instance_record(at);
    instance.tool_instance_id = instance_id;
    instance.tool_version = version;
    instance.entity_revision = revision;
    instance.source_contract_fingerprint = ToolSourceContractFingerprintV1::BuiltIn {
        signed_builtin_tool_contract_fingerprint: SignedBuiltinToolContractFingerprintV1::new(
            canonical_sha256(&(RELEASE_VERSION, &definition.definition_fingerprint, handler))
                .map_err(|_| invalid())?,
        ),
    };
    Ok(AgentBuiltinEntry { handler: AgentBuiltinHandler::Registry(handler), definition, instance })
}

/// The activity query's input as the model reads it. The signed built-in set keeps the schema text
/// it was released with, so the descriptions the model needs are set here; a path that no longer
/// exists fails the release instead of leaving a description out.
fn activity_query_schema() -> Result<serde_json::Value, BuiltinToolDefinitionSetError> {
    let mut schema = schema_value::<aura_activity_domain::AgentActivityQueryInput>()?;
    for (path, text) in [
        (
            "",
            "What to look for among the running activities. The query runs as this step's own \
             session: the input cannot name another session, a workspace or a grant.",
        ),
        ("/$defs/AgentActivityQueryBy", "Which question the model is asking."),
        ("/properties/expected_revision", "Not used by the query; leave it out."),
        ("/properties/offset", "Zero-based row offset within the queried set."),
    ] {
        schema
            .pointer_mut(path)
            .and_then(serde_json::Value::as_object_mut)
            .ok_or(BuiltinToolDefinitionSetError::InvalidDocument)?
            .insert("description".into(), text.into());
    }
    Ok(schema)
}
