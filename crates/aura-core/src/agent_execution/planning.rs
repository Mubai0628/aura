use super::*;
use aura_tool_registry::agent_release::AgentBuiltinHandler as H;
use aura_tool_scheduler::{
    PackageResourcePreflightRejectionV1, ResolvedPackageResourceSetV1, ToolCallResourcePreflightV1,
};
use serde_json::json;

/// A rejection that names *why* the resource could not be resolved.
///
/// The distinction matters to the model: a path it spelled wrongly and a path
/// outside its authorized scope need different corrections, and collapsing both
/// into one generic failure leaves it guessing. The code is the only channel the
/// model sees, so the class of failure has to be carried here.
pub(super) fn rejection_with(
    reason: ToolNotExecutedReasonV1,
    code: ToolResourceErrorCode,
) -> ToolCallResourcePreflightV1 {
    ToolCallResourcePreflightV1::Rejected(PackageResourcePreflightRejectionV1 {
        reason,
        error_code: code.error_code(),
        claim_key: None,
    })
}

/// A user's Hook stopped this call; it is refused like a permission denial and the
/// model is told which Hook and why.
pub(super) fn hook_rejection() -> ToolCallResourcePreflightV1 {
    ToolCallResourcePreflightV1::Rejected(PackageResourcePreflightRejectionV1 {
        reason: ToolNotExecutedReasonV1::PermissionDenied,
        error_code: ToolHookErrorCode::Blocked.error_code(),
        claim_key: None,
    })
}

/// Classifies a resource-resolution failure into the class the model needs.
///
/// The markers come from `path_input`, which is the single place a model path is
/// normalized, so the classification stays tied to the real cause rather than to
/// message text matched after the fact.
pub(super) fn resource_error_code(error: &anyhow::Error) -> ToolResourceErrorCode {
    if matches!(
        error.downcast_ref::<aura_network_control::NetworkControlError>(),
        Some(aura_network_control::NetworkControlError::SecretDetected { .. })
    ) {
        return ToolResourceErrorCode::HttpModelAuthenticationForbidden;
    }
    let text = error.to_string();
    if text.starts_with("http.credential_rebind_required:") {
        ToolResourceErrorCode::HttpCredentialRebindRequired
    } else if text.contains("http.credential_origin_denied") {
        ToolResourceErrorCode::HttpCredentialOriginDenied
    } else if text.contains("http.credential") {
        ToolResourceErrorCode::HttpCredentialUnavailable
    } else if text.starts_with("http.") {
        ToolResourceErrorCode::HttpRequestInvalid
    } else if text.contains("agent.path_outside_workspace") {
        // The spelling is valid but the target is not authorized.
        ToolResourceErrorCode::ScopeResolutionFailed
    } else if text.contains("agent.path_invalid")
        || text.contains("agent.path_unsafe_component")
        || text.contains("agent.path_argument")
    {
        // The spelling itself is wrong, and the message says how.
        ToolResourceErrorCode::CanonicalizationFailed
    } else {
        ToolResourceErrorCode::IdentityResolutionFailed
    }
}

pub(super) fn resources(
    service: &AgentExecution,
    context: &CallContext,
) -> anyhow::Result<ResolvedPackageResourceSetV1> {
    if context.validated.definition.tool_package_id.is_some() {
        return super::package_tools::resources(service, context);
    }
    if context.validated.definition.source_kind == aura_tool_contract::ToolSourceKindV1::Mcp {
        return super::mcp_tools::resources(context);
    }
    let entry = operation::entry(&context.validated)?;
    let id = &context.batch.invocation_identity;
    let mut claims = vec![];
    let state_key = format!("agent-state:{}:{}", id.session_id, id.turn_task_id);
    let write = entry.definition.execution_semantics.may_create_persistent_effects;
    let task_only = !matches!(
        entry.handler,
        H::FileRead
            | H::MediaProbe
            | H::MediaFrames
            | H::FileList
            | H::FileWrite
            | H::FilePatch
            | H::FileDelete
            | H::FileSearch
            | H::FileFind
            | H::PowerShell
            | H::Cmd
            | H::TerminalOpen
            | H::TerminalWrite
            | H::TerminalRead
            | H::TerminalClose
            | H::TerminalSensitiveInput
            | H::WebSearch
            | H::WebFetch
            | H::WebRead
            | H::HttpRequest
            | H::HttpDownload
            | H::GitCommit
            | H::GitBranch
            | H::GitSync
            | H::GitPush
            | H::MemorySearch
            | H::MemoryRead
            | H::MemoryRecent
            | H::MemoryCreate
            | H::MemoryUpdate
            | H::MemoryLink
            | H::MemoryForget
            | H::MemoryArchive
            | H::MemoryDelete
            | H::MemoryValidate
            | H::MemoryRepair
            | H::WorkspaceInitComplete
            | H::CommandsSave
            | H::CommandsDelete
    );
    // State effects serialize with readers. Ordinary external work reads task
    // authority and does not claim that it mutates task-local bookkeeping.
    claims.push(claim(
        ResolvedResourceKindV1::AgentTaskState,
        if task_only && write {
            ResolvedResourceAccessV1::Control
        } else {
            ResolvedResourceAccessV1::Read
        },
        state_key,
        &context.operation.content.identity,
    )?);
    if context.operation.content.action_kind.starts_with("git.") {
        let facts = super::git::inspect(service, context)?;
        claims.push(claim(
            ResolvedResourceKindV1::GitRepository,
            if write { ResolvedResourceAccessV1::Write } else { ResolvedResourceAccessV1::Read },
            format!(
                "git-repository:{}",
                aura_domain_common::jcs::canonical_sha256(&facts["repository"])?.hex()
            ),
            &facts["repository"],
        )?);
    }
    if let [ResourceDescriptor::MemoryVault { scope, vault_identity }] =
        context.operation.content.resources.as_slice()
    {
        claims.push(claim(
            ResolvedResourceKindV1::MemoryVault,
            if context.operation.content.capabilities == [ResolvedCapability::MemoryRead] {
                ResolvedResourceAccessV1::Read
            } else {
                ResolvedResourceAccessV1::Write
            },
            format!(
                "memory-vault:{}",
                aura_domain_common::jcs::canonical_sha256(&(scope, vault_identity))?.hex()
            ),
            (scope, vault_identity),
        )?);
    }
    if matches!(
        entry.handler,
        H::FileRead
            | H::MediaProbe
            | H::MediaFrames
            | H::FileList
            | H::FileWrite
            | H::FilePatch
            | H::FileDelete
            | H::FileSearch
            | H::FileFind
            | H::PowerShell
            | H::Cmd
            | H::HttpDownload
            | H::WorkspaceInitComplete
            | H::CommandsSave
            | H::CommandsDelete
    ) {
        let resolver = service.workspace.resolver(context.snapshot.workspace_id)?;
        resolver.verify_root_identity()?;
        if matches!(
            entry.handler,
            H::FileRead
                | H::FileList
                | H::FileWrite
                | H::FilePatch
                | H::FileDelete
                | H::FileSearch
                | H::FileFind
        ) {
            let inspected = files::inspect(service, context)?;
            if inspected.get("read_refusal").is_some() {
                return Ok(ResolvedPackageResourceSetV1 {
                    scheduler_claims: claims,
                    permission_scopes: vec![],
                    risk_flags: vec![],
                    resolved_directories: Default::default(),
                });
            }
        }
        // The claim is built from the **real files this call names**, not from the workspace.
        //
        // It used to be one `agent-workspace:<WorkspaceId>` key at `Write` for every side-effecting
        // file handler, plus a `workspace-owner:<WorkspaceId>` control claim. The scheduler treats
        // a non-shared access as exclusive, so every file write in a workspace excluded every other
        // file operation in that same workspace — different files, same workspace, serialised — and
        // an ordinary read conflicted with a write for no reason. That is exactly the false
        // conflict batch A removes, and fixing only the Main queue while leaving the Agent side
        // serial across a whole workspace was explicitly not acceptable.
        //
        // What replaces it:
        // * one claim per named path, at the access the call really needs, so two different files
        //   run in parallel and the same file still serialises;
        // * reads are `Read`, which the scheduler already treats as shared, so a read never blocks
        //   another read and only conflicts with a write of the same path;
        // * Shell keeps its honest unknown write set: `PowerShell`/`Cmd` still claim a
        //   representative key with no reliable per-file resolution, and the free-Shell rule is
        //   unchanged — no global Shell lock is invented and no complete write set is guessed from
        //   a script's text.
        for claim in file_claims(service, context, write)? {
            claims.push(claim);
        }
    }
    claims.sort_by(|a, b| a.canonical_lock_key.cmp(&b.canonical_lock_key));
    Ok(ResolvedPackageResourceSetV1 {
        scheduler_claims: claims,
        permission_scopes: vec![],
        risk_flags: vec![],
        resolved_directories: Default::default(),
    })
}

/// The resource claims a model file call really needs, from the paths it named.
///
/// The paths come from the operation's own `ResourceDescriptor::WorkspacePath` entries — the ones
/// `path_input` already normalized and the permission check already judged — so the claim keys
/// describe the same targets the handler will use, and nothing here re-derives a path from raw
/// argument text.
///
/// # What replaced the path-text key, and why
///
/// The previous key was `agent-path:<WorkspaceId>:<lowercased path>`: workspace id, colon, path
/// text. That is **partitioning by spelling**, and it fails in exactly the two directions the batch
/// cares about:
///
/// * the same physical file reached through a second workspace got a different key, so the
///   scheduler saw two unrelated claims and let both run — the cross-workspace association the
///   resource model exists to provide was simply absent; and
/// * a directory and a file inside it are unrelated strings, so a directory mutation did not
///   exclude a write under it.
///
/// The key is now the **real resource identity**, derived from the resolver's verified chain: for an
/// existing file the object's volume + file id, for a target that does not exist yet the verified
/// parent identity plus the leaf under the volume's real case rule, and for a subtree the
/// directory's identity. The workspace root identity is carried as the resolution evidence, so a
/// reader can still see which tree the claim was established in — but it is evidence, not the key,
/// because using it as the key is what split one file across two workspaces.
///
/// # The honest fallback
///
/// A call that names no workspace path — a Shell, or a call whose only resource is the workspace
/// root — does **not** get a guessed write set. It keeps one claim on the workspace root identity,
/// which is precisely "this call may touch anything in this workspace and I cannot say which": it is
/// stricter than the call may turn out to need and never less strict, and it never claims to know a
/// write set the caller cannot establish. Guessing a set from a script's text or a cwd is what this
/// deliberately does not do.
fn file_claims(
    service: &AgentExecution,
    context: &CallContext,
    write: bool,
) -> anyhow::Result<Vec<ResolvedResourceClaimV1>> {
    let access =
        if write { ResolvedResourceAccessV1::Write } else { ResolvedResourceAccessV1::Read };
    // A protected or granted external object is not a workspace path, so `files::paths` cannot
    // name it — but it is still the real object this call would touch, and it has to be claimed
    // as itself. Leaving it out was not neutral: with no workspace paths the planner fell back
    // to claiming the whole workspace tree, and the protected target then failed to resolve at
    // all. A call that reaches a protected child would therefore be refused at admission
    // instead of reaching its own approval decision.
    let mut claims = external_object_claims(context, access, write)?;
    let root = service.workspace.resolver(context.snapshot.workspace_id)?;
    let mut resolved = files::paths(context);
    resolved.sort();
    resolved.dedup();
    if resolved.is_empty() {
        if !claims.is_empty() {
            // The call names real objects, just not workspace ones. Claiming the workspace tree
            // as well would be a second, unrelated exclusion.
            claims.sort_by(|a, b| a.canonical_lock_key.cmp(&b.canonical_lock_key));
            return Ok(claims);
        }
        // The honest fallback: this call names no workspace file, so the only thing it can state is
        // the workspace tree it may reach. Stricter than necessary, never less strict, and never a
        // fabricated write set.
        return Ok(vec![claim(
            ResolvedResourceKindV1::WorkspacePath,
            access,
            format!("agent-workspace:{}", context.snapshot.workspace_id),
            root.root_identity(),
        )?]);
    }
    let facts = service
        .workspace
        .resource_facts(context.snapshot.workspace_id, context.snapshot.session_id)?;
    let mut observations = Vec::new();
    for path in resolved {
        // The object (when one exists) **and** the entry that names it. A write must exclude both:
        // "replace this file" and "the name this replacement takes" are two real things, and a
        // create-versus-delete-recreate race is an entry conflict while a write-versus-write on
        // existing bytes is an object conflict.
        let object = facts.object(&path)?;
        let slot = facts.slot(&path)?;
        for resource in object.into_iter().chain(std::iter::once(slot)) {
            observations.push(aura_workspace_domain::ResourceRequest::new(
                resource.clone(),
                if write {
                    aura_workspace_domain::ResourceAccess::Write
                } else {
                    aura_workspace_domain::ResourceAccess::Read
                },
            ));
            let key = resource.lock_key(aura_workspace_domain::ResourceAccess::Write);
            claims.push(claim(
                ResolvedResourceKindV1::WorkspacePath,
                access,
                key,
                // The evidence names the tree and the real identity, so the claim is inspectable
                // without the workspace id ever becoming the identity.
                json!({
                    "workspace_root": root.root_identity(),
                    "workspace_id": context.snapshot.workspace_id,
                    "resource": resource,
                }),
            )?);
        }
    }
    if let Ok(mut observed) = context.observed_resources.try_lock() {
        *observed = crate::agent_activity::resources::project(&observations);
    }
    Ok(claims)
}

/// The scheduler claims for the protected and granted objects one call really reaches.
///
/// A protected location and a granted external object are as real as a workspace file, and each
/// is claimed as the object it is. The key is the descriptor's own canonical path, which is the
/// **verified final path** classification recorded — so two spellings of one object, or an alias
/// and its target, produce one key and serialise against each other, while two different objects
/// stay independent.
///
/// Reads are `Read` and writes are `Write`, matching the workspace case: a read of a protected
/// object does not exclude another read, and only a genuine write conflicts.
fn external_object_claims(
    context: &CallContext,
    access: ResolvedResourceAccessV1,
    write: bool,
) -> anyhow::Result<Vec<ResolvedResourceClaimV1>> {
    let mut claims = Vec::new();
    for resource in &context.operation.content.resources {
        let (canonical_path, identity) = match resource {
            ResourceDescriptor::SystemPath { canonical_path, identity, .. }
            | ResourceDescriptor::SensitiveUserPath { canonical_path, identity, .. }
            | ResourceDescriptor::ExternalPath { canonical_path, identity, .. } => {
                (canonical_path, identity)
            }
            _ => continue,
        };
        // The identity is included when classification verified one, so the claim records the
        // object rather than only a name. It is not required: a create names a target that does
        // not exist yet.
        claims.push(claim(
            ResolvedResourceKindV1::ExternalFile,
            access,
            canonical_path.display.clone(),
            json!({
                "canonical_path": canonical_path.display,
                "identity": identity,
                "access": if write { "write" } else { "read" },
            }),
        )?);
    }
    Ok(claims)
}

fn claim(
    kind: ResolvedResourceKindV1,
    access: ResolvedResourceAccessV1,
    key: String,
    facts: impl serde::Serialize,
) -> anyhow::Result<ResolvedResourceClaimV1> {
    Ok(ResolvedResourceClaimV1 {
        kind,
        access,
        canonical_resource_identity: key.clone(),
        canonical_lock_key: key,
        resolution_evidence_hash: admission::evidence(facts)?,
        core_derived: true,
    })
}

#[cfg(test)]
mod tests {
    use super::{path_input, resource_error_code};
    use aura_tool_contract::ToolResourceErrorCode;
    use std::path::Path;

    /// A path error must reach the model as the class it really is, because each
    /// class needs a different correction. These assertions run the real
    /// normalization against a real root, so the classification is tied to the
    /// actual error text rather than to a hand-written string.
    #[test]
    fn a_real_path_error_is_classified_by_its_cause() {
        let root = tempfile::tempdir().expect("workspace root");
        let outside = tempfile::tempdir().expect("outside root");
        std::fs::write(outside.path().join("secret.txt"), "outside").expect("secret");

        // A path outside the authorized root is a scope problem.
        let outside_path = outside.path().join("secret.txt").display().to_string();
        let scope_error = path_input::normalize_action_paths(
            &mut serde_json::json!({ "path": outside_path }),
            path_input::PATH_FIELDS,
            root.path(),
        )
        .expect_err("an outside path must be refused");
        assert_eq!(
            resource_error_code(&scope_error),
            ToolResourceErrorCode::ScopeResolutionFailed,
            "an unauthorized location must not be reported as a spelling problem"
        );

        // A reserved device name is a canonicalization problem.
        let mut invalid = serde_json::json!({ "path": "src/CON.txt" });
        let invalid_error =
            path_input::normalize_action_paths(&mut invalid, path_input::PATH_FIELDS, root.path())
                .expect_err("a reserved name must be refused");
        assert_eq!(
            resource_error_code(&invalid_error),
            ToolResourceErrorCode::CanonicalizationFailed
        );

        // A non-string path argument is a spelling problem too.
        let mut wrong_type = serde_json::json!({ "path": 7 });
        let wrong_type_error = path_input::normalize_action_paths(
            &mut wrong_type,
            path_input::PATH_FIELDS,
            root.path(),
        )
        .expect_err("a non-string path must be refused");
        assert_eq!(
            resource_error_code(&wrong_type_error),
            ToolResourceErrorCode::CanonicalizationFailed
        );

        // Anything else stays the generic identity failure rather than being
        // guessed into a class it does not belong to.
        let unrelated = anyhow::anyhow!("agent.builtin_unavailable");
        assert_eq!(
            resource_error_code(&unrelated),
            ToolResourceErrorCode::IdentityResolutionFailed
        );
        let _ = Path::new(".");
    }
}
