//! File operations reuse the R05 secure resolver, editor ownership, atomic
//! changeset and compensation journal. They never invoke an external shell.
mod external_read;
mod external_write;
pub(super) mod invocation;
pub(crate) mod publication;
mod rejection;
mod traversal;
use super::*;
use anyhow::{Result, ensure};
use aura_workspace_domain::*;
use external_read::*;
pub(crate) use external_write::external_side_effect;
use external_write::*;
pub(super) use publication::invoke;
use serde_json::{Value, json};

/// The canonical workspace paths one call really names.
///
/// `pub(super)` because the resource planner builds its per-file scheduler claims from exactly
/// these paths: the ones `path_input` normalized and the permission check already judged. Two
/// independent derivations of "which file is this" would be two chances to disagree.
pub(super) fn paths(call: &CallContext) -> Vec<WorkspacePath> {
    call.operation
        .content
        .resources
        .iter()
        .filter_map(|r| match r {
            ResourceDescriptor::WorkspacePath { canonical_relative_path, .. } => {
                Some(canonical_relative_path.clone())
            }
            ResourceDescriptor::WorkspaceRoot { .. } => {
                WorkspacePath::parse(WorkspacePath::ROOT).ok()
            }
            _ => None,
        })
        .collect()
}
fn is_read(call: &CallContext) -> bool {
    matches!(
        call.operation.content.action_kind.as_str(),
        "fs.read" | "fs.search" | "fs.list" | "fs.find"
    )
}

/// The authorized external grant this operation was built against, when it has one.
///
/// The operation carries it as its resource, so nothing about the target is taken
/// from the model's own spelling: the grant id came from classification and the
/// path argument is already relative to that grant's root.
fn external_grant(call: &CallContext) -> Option<aura_domain_common::ExternalGrantId> {
    call.operation.content.resources.iter().find_map(|resource| match resource {
        ResourceDescriptor::ExternalGrant { grant_id } => Some(*grant_id),
        _ => None,
    })
}
/// The ordinary external target the session policy allowed directly, when it has one.
///
/// The operation carries the absolute path and — for every class — the identity the platform
/// verified at classification, so nothing about the target is taken from the model's own
/// spelling at this point. A system location or one of the user's credential stores carries
/// its identity exactly as an ordinary path does: the approved object is the one the user
/// approved, and a same-named replacement is a different object.
fn policy_external_target(
    call: &CallContext,
) -> Option<(&aura_security_contract::CanonicalPath, Option<&aura_domain_common::FileIdentity>)> {
    call.operation.content.resources.iter().find_map(|resource| match resource {
        ResourceDescriptor::ExternalPath { canonical_path, identity, .. }
        | ResourceDescriptor::SystemPath { canonical_path, identity, .. }
        | ResourceDescriptor::SensitiveUserPath { canonical_path, identity, .. } => {
            Some((canonical_path, identity.as_ref()))
        }
        _ => None,
    })
}
/// The path one policy-authorized external operation acts on.
fn policy_external_path(call: &CallContext) -> Option<aura_security_contract::CanonicalPath> {
    policy_external_target(call).map(|(path, _)| path.clone())
}
/// True when this call mutates an object outside the workspace.
///
/// The answer comes from the frozen operation — the resource it was authorized against —
/// and not from the tool's reply, because the reply is exactly what can be missing while the
/// mutation really happened. The external capabilities are checked too, so a workspace write
/// can never be mistaken for an external one.
fn external_mutation_target(call: &CallContext) -> bool {
    let external_resource = call.operation.content.resources.iter().any(|resource| {
        matches!(
            resource,
            ResourceDescriptor::ExternalPath { .. }
                | ResourceDescriptor::ExternalGrant { .. }
                | ResourceDescriptor::SystemPath { .. }
                | ResourceDescriptor::SensitiveUserPath { .. }
        )
    });
    let external_capability = call.operation.content.capabilities.iter().any(|capability| {
        matches!(
            capability,
            aura_security_contract::ResolvedCapability::ExternalFileWrite
                | aura_security_contract::ResolvedCapability::ExternalFileCreate
                | aura_security_contract::ResolvedCapability::ExternalFileDelete
        )
    });
    external_resource && external_capability
}
/// Opens the external object the operation was authorized for.
///
/// The handle comes from the operation's own path, and when classification recorded the
/// object's identity the opened handle has to *be* that object. A target replaced between
/// classification and dispatch is therefore refused instead of being read or written as
/// if it were the one the policy decided about — re-checking after the fact is not the
/// same guarantee, because by then the bytes are already in hand.
fn open_authorized_external(
    call: &CallContext,
    access: aura_windows_platform::NativeFileAccess,
) -> Result<(aura_security_contract::CanonicalPath, aura_windows_platform::VerifiedFile)> {
    let (path, identity) = policy_external_target(call)
        .ok_or_else(|| anyhow::anyhow!("agent.external_file_unavailable: no external resource"))?;
    let absolute = std::path::PathBuf::from(&path.display);
    let file = aura_windows_platform::open_verified_existing(&absolute, access)
        .map_err(|source| anyhow::anyhow!("agent.external_file_unavailable: {source}"))?;
    if let Some(expected) = identity
        && aura_workspace::domain_identity(&file.info().identity) != *expected
    {
        anyhow::bail!(
            "agent.external_object_replaced: the authorized object is no longer the one at this \
             path; the operation was refused instead of acting on a different object"
        );
    }
    Ok((path.clone(), file))
}
/// Reads one ordinary external file the session policy allowed directly.
///
/// The absolute path is the operation's own resource, so the model cannot move it by
/// respelling anything; the handle is opened here and has to still be the classified
/// object. There is no grant, and the result says so rather than inventing one.
fn read_policy_external(
    service: &AgentExecution,
    call: &CallContext,
    _path: &aura_security_contract::CanonicalPath,
    input: AgentFileReadInput,
) -> Result<FsReadTextResult> {
    let (path, file) =
        open_authorized_external(call, aura_windows_platform::NativeFileAccess::Read)?;
    let read = service.fs.read_external_text(aura_fs_tools::FsExternalTextReadRequest {
        target: aura_fs_tools::ExternalTextReadTarget {
            grant_id: None,
            session_id: call.snapshot.session_id,
            display_path: path.display.clone(),
            relative_path: WorkspacePath::parse(WorkspacePath::ROOT)?,
            file,
        },
        range: input.range,
        explicit_encoding: input.explicit_encoding,
    })?;
    Ok(FsReadTextResult {
        path: read.relative_path,
        text: read.text,
        encoding: read.encoding,
        line_ending: read.line_ending,
        source: FileStorageSource::Disk,
        logical_hash: read.logical_hash,
        disk_hash: read.disk_hash,
        buffer_revision: None,
        is_dirty: false,
        active_editor_session: None,
        disk_may_be_stale: false,
        total_characters: read.total_characters,
        total_lines: read.total_lines,
        returned_range: read.returned_range,
        external: Some(FsExternalReadTarget { grant_id: None, display_path: read.display_path }),
    })
}
pub(super) fn inspect(service: &AgentExecution, call: &CallContext) -> Result<Value> {
    let facts = crate::broker::files::facts(
        &service.fs,
        &service.workspace,
        call.snapshot.workspace_id,
        call.snapshot.session_id,
        &paths(call),
    );
    match facts {
        Err(error) if is_read(call) => {
            let action = call.operation.content.action_kind.as_str();
            match rejection::rejection(action, &Value::Null, &error) {
                Some(rejection) => Ok(json!({"read_refusal": rejection})),
                None => Err(error),
            }
        }
        Ok(facts) if is_read(call) && facts_absent(&facts) => {
            Ok(json!({"read_refusal": rejection::not_found()}))
        }
        result => result,
    }
}
fn invoke_inner(service: &AgentExecution, call: &CallContext) -> Result<Value> {
    let workspace_id = call.snapshot.workspace_id;
    let session_id = call.snapshot.session_id;
    let content = &call.operation.content;
    let correlation = content.identity.correlation_id;
    Ok(match content.action_kind.as_str() {
        "fs.list" => {
            let input: AgentFileListInput = serde_json::from_value(content.action.clone())?;
            match external_grant(call) {
                Some(grant_id) => {
                    serde_json::to_value(list_external(service, call, grant_id, input)?)?
                }
                None if policy_external_path(call).is_some() => {
                    let path = policy_external_path(call).expect("checked above").clone();
                    serde_json::to_value(list_policy_external(service, call, &path, input)?)?
                }
                None => serde_json::to_value(service.fs.list_for_agent(
                    workspace_id,
                    session_id,
                    input,
                )?)?,
            }
        }
        "fs.read" => {
            let i: AgentFileReadInput = serde_json::from_value(content.action.clone())?;
            match external_grant(call) {
                Some(grant_id) => serde_json::to_value(read_external(service, call, grant_id, i)?)?,
                None if policy_external_path(call).is_some() => {
                    let path = policy_external_path(call).expect("checked above").clone();
                    serde_json::to_value(read_policy_external(service, call, &path, i)?)?
                }
                None => serde_json::to_value(service.fs.read_text(FsReadTextRequest {
                    workspace_id,
                    session_id,
                    target: WorkspaceFileTarget { path: i.path.into_relative() },
                    range: i.range,
                    explicit_encoding: i.explicit_encoding,
                })?)?,
            }
        }
        "fs.search" => {
            let i: AgentFileSearchInput = serde_json::from_value(content.action.clone())?;
            if let Some(grant_id) = external_grant(call) {
                serde_json::to_value(search_external_grant(service, call, grant_id, i)?)?
            } else if policy_external_path(call).is_some() {
                let path = policy_external_path(call).expect("checked above").clone();
                serde_json::to_value(search_policy_external(service, call, &path, i)?)?
            } else {
                let cancel = aura_fs_tools::search_flight(correlation).unwrap_or_default();
                aura_fs_tools::register_search(correlation, cancel.clone());
                struct Unregister(aura_domain_common::CorrelationId);
                impl Drop for Unregister {
                    fn drop(&mut self) {
                        aura_fs_tools::release_search_flight(self.0);
                    }
                }
                let _unregister = Unregister(correlation);
                serde_json::to_value(
                    service
                        .fs
                        .search_text_with_cancel(
                            FsSearchTextRequest {
                                workspace_id,
                                session_id,
                                pattern: i.pattern,
                                root: i.root.into_relative(),
                                path_glob: i.path_glob,
                                include_ignored: i.include_ignored,
                                include_generated: i.include_generated,
                                limit: i.limit,
                            },
                            &cancel,
                        )
                        .map(|(result, _)| result)?,
                )?
            }
        }
        "fs.write" => {
            let i: AgentFileWriteInput = serde_json::from_value(content.action.clone())?;
            // A new file is created as UTF-8 with LF. Overwriting an existing file
            // passes neither, so the mutation layer keeps that file's detected
            // encoding and line-ending convention instead of transcoding it.
            let creating = i.expected == FsExpectedState::Absent;
            let line_ending = creating.then_some(LineEnding::Lf);
            let encoding = creating.then_some(TextEncoding::Utf8);
            match external_grant(call) {
                Some(grant_id) => serde_json::to_value(write_external(
                    service,
                    call,
                    grant_id,
                    i,
                    creating,
                    encoding,
                    line_ending,
                )?)?,
                None if policy_external_path(call).is_some() => {
                    let path = policy_external_path(call).expect("checked above").clone();
                    serde_json::to_value(write_policy_external(
                        service,
                        call,
                        &path,
                        i,
                        creating,
                        encoding,
                        line_ending,
                    )?)?
                }
                None => serde_json::to_value(service.fs.write_file_for_agent(
                    FsWriteFileRequest {
                        workspace_id,
                        session_id,
                        target: WorkspaceFileTarget { path: i.path.into_relative() },
                        content: i.text,
                        expected: i.expected,
                        encoding,
                        line_ending,
                    },
                    correlation,
                )?)?,
            }
        }
        "fs.patch" => {
            let i: AgentFilePatchInput = serde_json::from_value(content.action.clone())?;
            match external_grant(call) {
                Some(grant_id) => {
                    serde_json::to_value(patch_external(service, call, grant_id, i)?)?
                }
                None if policy_external_path(call).is_some() => {
                    let path = policy_external_path(call).expect("checked above").clone();
                    serde_json::to_value(patch_policy_external(service, call, &path, i)?)?
                }
                None => serde_json::to_value(service.fs.apply_patch_for_agent(
                    FsApplyPatchRequest {
                        workspace_id,
                        session_id,
                        target: WorkspaceFileTarget { path: i.path.into_relative() },
                        unified_diff: i.unified_diff,
                        expected_hash: i.expected_hash,
                    },
                    correlation,
                )?)?,
            }
        }
        "fs.find" => {
            let input: AgentFileFindInput = serde_json::from_value(content.action.clone())?;
            match external_grant(call) {
                Some(grant_id) => {
                    serde_json::to_value(find_external(service, call, grant_id, input)?)?
                }
                None if policy_external_path(call).is_some() => {
                    let path = policy_external_path(call).expect("checked above").clone();
                    serde_json::to_value(find_policy_external(service, call, &path, input)?)?
                }
                None => serde_json::to_value(service.fs.find_for_agent(
                    workspace_id,
                    session_id,
                    input,
                )?)?,
            }
        }
        // To the Recycle Bin, or for good when the user approved this one call.
        "fs.delete" => serde_json::to_value(super::deletion::delete(service, call)?)?,
        _ => anyhow::bail!("agent.file_handler_missing"),
    })
}

/// Reads one authorized external file.
///
/// The permission owner resolves the grant here, on the call path, so the decision
/// is made against the live grant (lifecycle, capability, root identity, scope) and
/// not against the classification that built the operation. The handle it returns is
/// verified; the file layer reads that handle and re-checks the identity afterwards.
fn read_external(
    service: &AgentExecution,
    call: &CallContext,
    grant_id: aura_domain_common::ExternalGrantId,
    input: AgentFileReadInput,
) -> Result<FsReadTextResult> {
    let session_id = call.snapshot.session_id;
    let relative = input.path.into_relative();
    let record = service.external_grants.record(grant_id)?;
    // The path the user selected is what they will recognise; the canonical root is
    // the authority the scope and identity checks use, and it is not what a model or
    // a person should be shown.
    let display_path = join_display(&record.selected_root.display, &relative);
    let resolved = service.external_grants.resolve_existing(
        grant_id,
        session_id,
        &relative,
        aura_permission_domain::ExternalGrantCapability::Read,
        aura_windows_platform::NativeFileAccess::Read,
        call.operation.content.identity.correlation_id,
    )?;
    let read = service.fs.read_external_text(aura_fs_tools::FsExternalTextReadRequest {
        target: aura_fs_tools::ExternalTextReadTarget {
            grant_id: Some(grant_id),
            session_id,
            display_path: display_path.clone(),
            relative_path: relative.clone(),
            file: resolved.file,
        },
        range: input.range,
        explicit_encoding: input.explicit_encoding,
    })?;
    Ok(FsReadTextResult {
        path: read.relative_path,
        text: read.text,
        encoding: read.encoding,
        line_ending: read.line_ending,
        source: FileStorageSource::Disk,
        logical_hash: read.logical_hash,
        disk_hash: read.disk_hash,
        buffer_revision: None,
        is_dirty: false,
        // An external file is never owned by a workspace editor buffer, so there is
        // no session whose in-memory copy could be authoritative.
        active_editor_session: None,
        disk_may_be_stale: false,
        total_characters: read.total_characters,
        total_lines: read.total_lines,
        returned_range: read.returned_range,
        external: Some(FsExternalReadTarget {
            grant_id: Some(grant_id),
            display_path: read.display_path,
        }),
    })
}

/// The absolute display path of one file inside an authorized root.
fn join_display(root: &str, relative: &WorkspacePath) -> String {
    let mut text = root.trim_end_matches(['\\', '/']).to_owned();
    if !relative.is_root() {
        for component in relative.as_str().split('/') {
            text.push(std::path::MAIN_SEPARATOR);
            text.push_str(component);
        }
    }
    text
}

fn list_external(
    service: &AgentExecution,
    call: &CallContext,
    grant_id: aura_domain_common::ExternalGrantId,
    input: AgentFileListInput,
) -> Result<AgentFileListResult> {
    let session_id = call.snapshot.session_id;
    let directory = input.path.into_relative();
    let record = service.external_grants.record(grant_id)?;
    let display_path = join_display(&record.selected_root.display, &directory);
    let resolved = service.external_grants.resolve_existing(
        grant_id,
        session_id,
        &directory,
        aura_permission_domain::ExternalGrantCapability::Read,
        aura_windows_platform::NativeFileAccess::Read,
        call.operation.content.identity.correlation_id,
    )?;
    if !resolved.file.info().is_directory {
        anyhow::bail!(
            "agent.external_list_target_not_a_directory: the authorized target is a file, not a \
             directory"
        );
    }
    // Listing reads the target's entries. A direct child of a non-recursive root may be
    // listed as an object, but its own entries are grandchildren of the grant root and are
    // not authorized.
    if !crate::external_grants::contents_are_authorized(&record, &directory) {
        anyhow::bail!(
            "agent.external_scope_contents: the directory is authorized as an object, but its \
             contents are outside this session's authorized scope; the grant covers {} and its \
             direct children only",
            record.selected_root.display
        );
    }
    let after_name =
        input.cursor.as_ref().map(|cursor| cursor.after_path.relative().as_str().to_owned());
    let listed = service.fs.list_external(aura_fs_tools::FsExternalListRequest {
        target: aura_fs_tools::ExternalListTarget {
            grant_id: Some(grant_id),
            session_id,
            display_path: display_path.clone(),
            relative_path: directory.clone(),
            directory: resolved.file,
        },
        limit: input.limit,
        after_name: after_name.clone(),
    })?;
    // A continuation belongs to one grant, one directory and one listing state; a
    // changed directory means the caller has to start again rather than receive a
    // page that silently skips entries.
    if let Some(cursor) = &input.cursor
        && (cursor.external_grant_id != Some(grant_id)
            || cursor.directory.relative() != &directory
            || cursor.catalog_revision != listed.scope_revision)
    {
        anyhow::bail!(
            "agent.external_list_cursor_stale: the directory changed or the cursor belongs to \
             another listing; restart with cursor=null"
        );
    }
    let entries = listed
        .entries
        .into_iter()
        .map(|entry| FsListEntry {
            path: entry.relative_path,
            name: entry.name,
            kind: entry.kind,
            byte_length: entry.byte_length,
            // An external directory has no workspace ignore rules or generated-path
            // classification, so both flags stay false rather than being invented.
            ignored: false,
            generated: false,
            reparse_tag: entry.reparse_tag,
        })
        .collect::<Vec<_>>();
    let next_cursor =
        listed.truncated.then(|| entries.last()).flatten().map(|last| AgentFileListCursor {
            directory: directory.clone().into(),
            catalog_revision: listed.scope_revision,
            after_path: last.path.clone().into(),
            external_grant_id: Some(grant_id),
        });
    Ok(AgentFileListResult {
        skipped_scopes: Vec::new(),
        entries,
        catalog_revision: listed.scope_revision,
        next_cursor,
        truncated: listed.truncated,
        external: Some(FsExternalListTarget {
            grant_id: Some(grant_id),
            display_path: listed.display_path,
        }),
    })
}
pub(super) fn evidence(
    service: &AgentExecution,
    call: &CallContext,
    request: &DispatchRequest,
    output: &Result<Value>,
    invoked: bool,
) -> Result<BrokerEvidence> {
    let before = request.plan.prepared.preparation["domain"].clone();
    let after = inspect(service, call).unwrap_or(Value::Null);
    let journal = service
        .persistence
        .workspace_broker_evidence(call.operation.content.identity.correlation_id)?;
    let states = journal
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v["state"].as_str())
        .collect::<Vec<_>>();
    let mutation = super::workspace_init::writes_files(&call.operation.content.action_kind);
    // An authorized external write is not part of the workspace index and does not enter
    // the workspace mutation journal, so the workspace inspection cannot see it at all:
    // `before` and `after` are the same facts and the journal is empty. Reading that as
    // "no effect" would tell the model the opposite of what happened, and reading the tool
    // reply instead is only half a fact — the reply is exactly what is missing when a
    // publication succeeded and its result never made it back. The stages this mutation
    // really reached are recorded around the publication, so the state comes from them:
    // published means applied, a rolled-back publication means compensated, a refusal
    // before anything could be published means no effect, and everything else — including
    // "the publication was attempted and we cannot confirm it" — is Unknown, which is what
    // stops an automatic replay from acting twice.
    let external_mutation = mutation && external_mutation_target(call);
    let external_stage = if external_mutation {
        let store = service.journal.lock()?;
        // The read-back path knows the call but not the batch it belonged to, so it looks the
        // fact up by the call's own identity. Scanning a window of recent events instead is
        // what used to report a recorded fact as no fact at all — and "no fact on a call that
        // really ran" is read as an unconfirmed side effect, the expensive answer.
        aura_persistence_v2::tool::external_change::latest_for_call(
            store.database.connection(),
            call.validated.call.tool_call_id,
        )?
        .map(|fact| fact.stage)
    } else {
        None
    };
    let side_effect = if call.operation.content.action_kind == "fs.delete" {
        super::deletion::side_effect(service, call, output, invoked)
    } else if !invoked || !mutation {
        SideEffectStateV1::NoEffect
    } else if external_mutation {
        external_side_effect(external_stage)
    } else if !states.is_empty() && states.iter().all(|s| *s == "compensated") {
        SideEffectStateV1::Compensated
    } else if !states.is_empty() && states.iter().all(|s| *s == "completed") && after != Value::Null
    {
        SideEffectStateV1::FullyApplied
    } else if states.is_empty() && before == after {
        SideEffectStateV1::NoEffect
    } else {
        SideEffectStateV1::Unknown
    };
    ensure!(
        side_effect != SideEffectStateV1::Compensated || output.is_err(),
        "agent.compensated_success"
    );
    // A refusal names a cause only for a call that changed nothing; anything else keeps the
    // recorded side effect as the whole answer.
    let operation_rejection = before.get("read_refusal").cloned().or_else(|| {
        let error = output.as_ref().err()?;
        let action = call.operation.content.action_kind.as_str();
        (side_effect == SideEffectStateV1::NoEffect)
            .then(|| rejection::rejection(action, &before, error))
            .flatten()
    });
    Ok(BrokerEvidence {
        action_kind: call.operation.content.action_kind.clone(),
        request_fingerprint: call.operation.fingerprint.0.clone(),
        before,
        after,
        domain_journal: json!({"schema":"agent-file-evidence-v2", "operation_rejection":operation_rejection, "journal":journal, "output":output.as_ref().ok(), "error":output.as_ref().err().map(ToString::to_string), "invoked":invoked}),
        side_effect,
    })
}

fn facts_absent(facts: &Value) -> bool {
    facts["paths"]
        .as_array()
        .is_some_and(|paths| paths.iter().any(|path| path["absent"].as_bool() == Some(true)))
}
#[cfg(test)]
mod external_resource_admission;
