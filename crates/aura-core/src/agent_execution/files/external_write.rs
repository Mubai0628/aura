use super::*;

/// Opens the policy-approved target through its verified parent or existing file.
pub(super) fn policy_external_write_target(
    call: &CallContext,
    path: &aura_security_contract::CanonicalPath,
    creating: bool,
) -> Result<aura_fs_tools::ExternalWriteTarget> {
    let absolute = std::path::PathBuf::from(&path.display);
    let kind = if creating {
        let leaf_name = absolute
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("agent.external_target_invalid: no file name"))?
            .to_owned();
        let parent_path = absolute
            .parent()
            .ok_or_else(|| anyhow::anyhow!("agent.external_target_invalid: no parent directory"))?;
        let parent = aura_windows_platform::open_verified_publish_parent(parent_path)
            .map_err(|source| anyhow::anyhow!("agent.external_file_unavailable: {source}"))?;
        // A create has no object yet, so what the approval named is the **directory** the
        // file will appear in. That directory is what has to still be the approved one: a
        // parent replaced between classification and dispatch would put the new file
        // somewhere the user never approved, under a name they did.
        if let Some(expected) = policy_external_parent_identity(call)
            && aura_workspace::domain_identity(&parent.info().identity) != *expected
        {
            anyhow::bail!(
                "agent.external_parent_replaced: the approved directory is no longer the one at \
                 this path; the create was refused instead of publishing into a different \
                 directory"
            );
        }
        aura_fs_tools::ExternalWriteKind::New { parent, leaf_name, final_path: absolute }
    } else {
        // The object the policy decided about is the object this must write: a target
        // replaced between classification and dispatch is refused here, before anything
        // is staged or published. This applies to every class of external object, including
        // a system location and one of the user's credential stores.
        let (_path, file) =
            open_authorized_external(call, aura_windows_platform::NativeFileAccess::ReadWrite)?;
        aura_fs_tools::ExternalWriteKind::Existing(file)
    };
    Ok(aura_fs_tools::ExternalWriteTarget {
        grant_id: None,
        session_id: call.snapshot.session_id,
        display_path: path.display.clone(),
        relative_path: WorkspacePath::parse(WorkspacePath::ROOT)?,
        kind,
    })
}

/// The identity of the approved parent directory of a create, when classification recorded one.
///
/// A create has no object yet, so the identity the resource carries for it is the identity of
/// the **directory** the file will appear in. The create path therefore verifies the parent it
/// publishes into rather than the file that does not exist. A `SystemPath`/`SensitiveUserPath`
/// create carries it the same way an ordinary one does.
pub(super) fn policy_external_parent_identity(
    call: &CallContext,
) -> Option<&aura_domain_common::FileIdentity> {
    call.operation.content.resources.iter().find_map(|resource| match resource {
        ResourceDescriptor::ExternalPath { parent_identity, .. }
        | ResourceDescriptor::SystemPath { parent_identity, .. }
        | ResourceDescriptor::SensitiveUserPath { parent_identity, .. } => parent_identity.as_ref(),
        _ => None,
    })
}

/// The destination a policy-authorized external write will publish to.
///
/// The path is the one the policy decided about, so the lock key is the same object the
/// approval named rather than a spelling from the call.
pub(super) fn policy_external_lock_destination(
    _call: &CallContext,
    path: &aura_security_contract::CanonicalPath,
    creating: bool,
) -> Result<aura_fs_tools::ExternalWriteDestination> {
    use aura_fs_tools::ExternalWriteDestination;
    let absolute = std::path::PathBuf::from(&path.display);
    if !creating {
        return Ok(ExternalWriteDestination::Existing(absolute));
    }
    let leaf_name = absolute
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("agent.external_target_invalid: no file name"))?
        .to_owned();
    let parent = absolute
        .parent()
        .ok_or_else(|| anyhow::anyhow!("agent.external_target_invalid: no parent directory"))?
        .to_path_buf();
    Ok(ExternalWriteDestination::New { parent, leaf_name })
}

/// Writes one ordinary external file the session policy allowed directly.
pub(super) fn write_policy_external(
    service: &AgentExecution,
    call: &CallContext,
    path: &aura_security_contract::CanonicalPath,
    input: AgentFileWriteInput,
    creating: bool,
    encoding: Option<TextEncoding>,
    line_ending: Option<LineEnding>,
) -> Result<aura_workspace_domain::AgentFileMutationResult> {
    let previous_hash = match &input.expected {
        FsExpectedState::FileHash { hash } => Some(hash.to_string()),
        _ => None,
    };
    let display = path.display.clone();
    let lock_destination = policy_external_lock_destination(call, path, creating)?;
    publish_external(
        service,
        call,
        &display,
        lock_destination,
        aura_persistence_v2::tool::external_change::ExternalChangeAuthority::Policy,
        previous_hash,
        || policy_external_write_target(call, path, creating),
        |target| {
            service.fs.write_external(aura_fs_tools::FsExternalWriteRequest {
                target,
                content: input.text,
                expected: input.expected,
                encoding,
                line_ending,
            })
        },
        |written| Some(written.logical_hash.to_string()),
        |written| written.observed_previous_hash.as_ref().map(ToString::to_string),
    )
    .and_then(|written| external_write_result(service, call, written))
}

/// Patches one ordinary external file the session policy allowed directly.
pub(super) fn patch_policy_external(
    service: &AgentExecution,
    call: &CallContext,
    path: &aura_security_contract::CanonicalPath,
    input: AgentFilePatchInput,
) -> Result<aura_workspace_domain::AgentFileMutationResult> {
    let expected_hash = input.expected_hash.clone();
    let display = path.display.clone();
    let lock_destination = policy_external_lock_destination(call, path, false)?;
    publish_external(
        service,
        call,
        &display,
        lock_destination,
        aura_persistence_v2::tool::external_change::ExternalChangeAuthority::Policy,
        Some(expected_hash.to_string()),
        || policy_external_write_target(call, path, false),
        |target| {
            service.fs.patch_external(aura_fs_tools::FsExternalPatchRequest {
                target,
                unified_diff: input.unified_diff,
                expected_hash: input.expected_hash,
            })
        },
        |written| Some(written.logical_hash.to_string()),
        |written| written.observed_previous_hash.as_ref().map(ToString::to_string),
    )
    .and_then(|written| external_write_result(service, call, written))
}

/// The common resource admission for one external destination, held across the real publication.
///
/// # Why the previous shape was not coordination
///
/// The pre-fix code held a process-local `Mutex` keyed by a **textual** key: the canonical path when
/// it resolved, otherwise parent-plus-leaf, always lowercased. Both halves of that were wrong, and
/// each wrong in the direction that matters:
///
/// * a key made of path text cannot see that two different names are **one object**. A hardlink, or a
///   second name for one file, produced two different keys, so two calls the file layer would treat
///   as one target both entered; and
/// * lowercasing is a **guess about the volume**. On a case-sensitive directory two genuinely
///   different files folded to one key and falsely serialised — the opposite failure, and one that
///   made unrelated work wait.
///
/// There was also no absolute deadline (`thread::sleep` in a doubling loop that never gave up) and
/// only a search cancellation source, so a cancelled or disconnected call could sit in it.
///
/// # What replaces it
///
/// The destination's **real resource** — the object's identity when it exists, or the verified
/// parent directory plus the leaf name when it is being created — derived by the shared
/// [`aura_workspace::ResourceFacts`] rules, and committed through the **one** `EligibilityOwner`
/// every other entry commits through. So an
/// external write and an in-workspace write reaching one physical object are one resource, and two
/// genuinely different files are two.
///
/// The commit is the *short* commit: it is taken after the destination has been resolved and is held
/// until the publication and its journal settlement are over. A caller that finds the resource held
/// gets the ordinary retryable refusal and is retried by its own asynchronous owner within the
/// original budget — this type never sleeps.
pub(super) struct ExternalDestinationAdmission {
    owner: std::sync::Arc<aura_workspace_domain::EligibilityOwner>,
    commit: Option<aura_workspace_domain::ResourceCommit>,
    _activity: Option<Box<dyn Send>>,
}

impl ExternalDestinationAdmission {
    /// Takes the destination's real resource for `session_id`, or reports the live refusal.
    ///
    /// `target` is resolved **before** this runs, deliberately: the resource must name the object
    /// this call is really about, and a target that has not been resolved yet has no identity to
    /// name. The caller re-resolves nothing afterwards — the publication uses the very handle that
    /// was resolved here.
    fn acquire(
        service: &AgentExecution,
        call: &CallContext,
        target: &aura_fs_tools::ExternalWriteTarget,
    ) -> Result<Self, aura_fs_tools::FsToolError> {
        let owner = std::sync::Arc::clone(&service.workspace.eligibility_owner());
        let resources = external_target_resources(call, target)?;
        let commit = owner
            .commit(
                &aura_workspace_domain::CommitRequest::per_resource(resources),
                call.snapshot.session_id,
                aura_workspace_domain::ExecutionId::next(),
                None,
            )
            .map_err(|refusal| aura_fs_tools::FsToolError::ResourceBusy(Box::new(refusal)))?;
        let activity = aura_fs_tools::observe_file_execution(commit.requests());
        Ok(Self { owner, commit: Some(commit), _activity: activity })
    }

    /// Releases the destination's resource once the publication and its settlement are over.
    fn release(mut self) {
        if let Some(commit) = self.commit.take() {
            self.owner.release_commit(commit);
        }
    }
}

impl Drop for ExternalDestinationAdmission {
    fn drop(&mut self) {
        if let Some(commit) = self.commit.take() {
            self.owner.release_commit(commit);
        }
    }
}

/// The real resource set one authorized external destination names.
///
/// An existing target names its **object** and the **entry** that names it, exactly as an
/// in-workspace overwrite does. A create has no object yet, so it names the verified parent's entry
/// plus the leaf — the shape a second create of the same name also computes, which is what makes the
/// two serialize. Both come from the shared [`aura_workspace`] derivation so this entry cannot drift
/// from Main Fs, Buffer or P04.
pub(super) fn external_target_resources(
    call: &CallContext,
    target: &aura_fs_tools::ExternalWriteTarget,
) -> Result<Vec<aura_workspace_domain::ResourceRequest>, aura_fs_tools::FsToolError> {
    external_target_resources_for(call.snapshot.workspace_id, call.snapshot.session_id, target)
}

/// The real resource set one external destination names, for a caller's own facts.
///
/// Split from [`external_target_resources`] so the derivation takes exactly the two identity fields
/// it reads. Nothing about the operation, the model's arguments or the DTO is consulted to decide
/// which physical object an external destination is — that is the whole point of deriving it from
/// the resolved handle instead.
pub(super) fn external_target_resources_for(
    workspace_id: aura_domain_common::WorkspaceId,
    session_id: aura_domain_common::SessionId,
    target: &aura_fs_tools::ExternalWriteTarget,
) -> Result<Vec<aura_workspace_domain::ResourceRequest>, aura_fs_tools::FsToolError> {
    use aura_workspace_domain::{FileResource, ResourceAccess, ResourceRequest};
    let label = aura_workspace_domain::ResourceOwner {
        workspace_id,
        session_id,
        path: target.relative_path.clone(),
    };
    let mut resources = Vec::new();
    match &target.kind {
        aura_fs_tools::ExternalWriteKind::Existing(file) => {
            let info = file.info().clone();
            resources.push(ResourceRequest::new(
                FileResource::Object {
                    identity: aura_workspace::domain_identity(&info.identity),
                    location: aura_workspace::external_ancestry(file)?,
                    owner: label.clone(),
                },
                ResourceAccess::Write,
            ));
            // The entry that names the object, so a create of the same name serializes against this
            // replacement even though the two name different things.
            let parent_path = info.final_path.parent().ok_or_else(|| {
                aura_fs_tools::FsToolError::InvalidRequest(
                    "agent.external_resource_facts: no parent".into(),
                )
            })?;
            let parent = aura_windows_platform::open_verified_existing(
                parent_path,
                aura_windows_platform::NativeFileAccess::Metadata,
            )
            .map_err(|error| {
                aura_fs_tools::FsToolError::InvalidRequest(format!(
                    "agent.external_resource_facts: required parent could not be verified: {error}"
                ))
            })?;
            let leaf = info.final_path.file_name().ok_or_else(|| {
                aura_fs_tools::FsToolError::InvalidRequest(
                    "agent.external_resource_facts: no leaf".into(),
                )
            })?;
            resources.push(ResourceRequest::new(
                aura_workspace::external_slot(&parent, &leaf.to_string_lossy(), label)?,
                ResourceAccess::Write,
            ));
        }
        aura_fs_tools::ExternalWriteKind::New { parent, leaf_name, .. } => {
            resources.push(ResourceRequest::new(
                aura_workspace::external_slot(parent, &leaf_name.to_string_lossy(), label)?,
                ResourceAccess::Write,
            ));
        }
    }
    if resources.is_empty() {
        return Err(aura_fs_tools::FsToolError::InvalidRequest(
            "agent.external_resource_facts: the destination named no resource".to_owned(),
        ));
    }
    Ok(resources)
}

/// Which stage an external mutation reached, from the stage the file layer really reported.
///
/// An error type could not answer this: `ExternalFileReplaced` arrives both when the
/// destination was refused before anything moved and when the new content had already been
/// published and could not be confirmed afterwards. The file layer reports the stage
/// explicitly, and this is only the translation into the persisted vocabulary.
pub(super) fn external_failure_stage(
    error: &aura_fs_tools::ExternalWriteError,
) -> aura_persistence_v2::tool::external_change::ExternalChangeStage {
    use aura_fs_tools::ExternalPublishStage as From;
    use aura_persistence_v2::tool::external_change::ExternalChangeStage as To;
    match error.stage {
        // The new content reached the destination and was removed again. That is a
        // compensation, not an absence of effect, and it stays one all the way into the
        // persisted fact and the model's envelope.
        From::PublishedThenRolledBack => To::Compensated,
        // A publication that was attempted and could not be confirmed is likewise not "no
        // effect". The unreconciled case is the strictest form: neither version is known to be
        // on disk, so only a separate, identity-bound check may settle it.
        From::PublishedUnconfirmed | From::PublishedUnreconciled => To::Unknown,
        From::NeverPublished | From::Refused => To::NotPerformed,
    }
}

/// The side-effect state an external mutation reports, from the stage it reached.
///
/// Kept as one table so the reading of a fact cannot drift from its writing: a published
/// change is applied, a rolled-back publication is compensated, a refusal before any
/// publication is no effect, and a prepared or unconfirmable change — including one whose
/// fact is missing on a call that really ran — is Unknown. Unknown is the state that stops
/// an automatic replay, so it is never traded for the cheaper "no effect".
pub(crate) fn external_side_effect(
    stage: Option<aura_persistence_v2::tool::external_change::ExternalChangeStage>,
) -> SideEffectStateV1 {
    use aura_persistence_v2::tool::external_change::ExternalChangeStage as Stage;
    match stage {
        Some(Stage::Published) => SideEffectStateV1::FullyApplied,
        Some(Stage::Compensated) => SideEffectStateV1::Compensated,
        Some(Stage::NotPerformed) => SideEffectStateV1::NoEffect,
        Some(Stage::Prepared | Stage::Unknown) | None => SideEffectStateV1::Unknown,
    }
}

/// Runs one external publication with the facts it needs around it.
///
/// The destination is held for the whole of it, and every step that decides *what* to write
/// happens inside that hold: the target is re-resolved against the live grant, the identity
/// and expected state are re-checked, the publication runs, and the outcome is confirmed.
/// Resolving before taking the lock would leave exactly the window this exists to close —
/// the permission a call acted under, and the file it acted on, could both change while it
/// waited its turn.
///
/// The intention is recorded *before* the platform is asked to publish — a crash after that
/// point leaves a `prepared` fact, which is exactly what stops the mutation from being read
/// as "no effect" or replayed — and the outcome is recorded from the stage the file layer
/// reported, never from the shape of the error.
#[allow(clippy::too_many_arguments)]
pub(super) fn publish_external<T>(
    service: &AgentExecution,
    call: &CallContext,
    _display_path: &str,
    _lock_destination: aura_fs_tools::ExternalWriteDestination,
    authority: aura_persistence_v2::tool::external_change::ExternalChangeAuthority,
    previous_hash: Option<String>,
    resolve: impl FnOnce() -> Result<aura_fs_tools::ExternalWriteTarget>,
    publish: impl FnOnce(
        aura_fs_tools::ExternalWriteTarget,
    ) -> std::result::Result<T, aura_fs_tools::ExternalWriteError>,
    resulting_hash: impl FnOnce(&T) -> Option<String>,
    observed_previous_hash: impl FnOnce(&T) -> Option<String>,
) -> Result<T> {
    use aura_persistence_v2::tool::external_change::{ExternalChangeFact, ExternalChangeStage};
    // The destination is resolved **first**, because the resource this call must take is the real
    // object it is about: a target that has not been resolved yet has no identity to name, and a
    // resource derived from the model's own spelling would be a second, weaker decision about which
    // file this is. Resolution re-runs the grant's live check (revocation, expiry, scope, the
    // verified identity), so the handle this returns is the one the permission owner just approved.
    let target = resolve()?;
    let display_path = target.display_path.clone();
    // **The common admission, held across the journal and the real publication.**
    //
    // The resource is the destination's real object (or, for a create, the verified parent plus the
    // leaf) committed through the one `EligibilityOwner` every other entry uses. Nothing here sleeps:
    // a live refusal is the ordinary retryable `ResourceBusy`, which this call's own asynchronous
    // owner retries within the original configuration and request budget — and every retry resolves
    // the target again, so a revoked grant or a replaced object is refused rather than acted on.
    let admission = ExternalDestinationAdmission::acquire(service, call, &target)
        .map_err(anyhow::Error::from)?;
    let destination = target.destination();
    let identity = policy_external_target(call)
        .and_then(|(_, identity)| identity)
        .and_then(|identity| serde_json::to_string(identity).ok());
    let fact = |stage, resulting_hash: Option<String>, reason: Option<String>| ExternalChangeFact {
        call_id: call.validated.call.tool_call_id,
        operation_id: Some(call.operation.operation_id),
        stage,
        display_path: display_path.clone(),
        previous_hash: previous_hash.clone(),
        resulting_hash,
        identity: identity.clone(),
        observed_previous_hash: None,
        observed_identity: None,
        authority: authority.clone(),
        reason,
        at: UtcTimestamp::now(),
    };
    let record = |value: ExternalChangeFact| -> Result<()> {
        let mut store = service.journal.lock()?;
        store.database.unit_of_work(|tx| {
            aura_persistence_v2::tool::external_change::record(tx, call.batch.tool_batch_id, &value)
        })?;
        Ok(())
    };
    // The identity the destination really carries now, so the fact records the object rather
    // than the path spelling it was reached by. A create has no previous object to name.
    let destination_identity = match &destination {
        aura_fs_tools::ExternalWriteDestination::Existing(path) => {
            aura_windows_platform::inspect_existing_path(path, true).ok().and_then(|info| {
                serde_json::to_string(&aura_workspace::domain_identity(&info.identity)).ok()
            })
        }
        aura_fs_tools::ExternalWriteDestination::New { .. } => None,
    };
    record(fact(ExternalChangeStage::Prepared, None, None))?;
    // The publication and its outcome receipt are the whole of what the resource protects: the
    // decision about *what* to write, the write, and the record of which stage it reached. The
    // release happens on every path, including the error path below.
    let outcome = match publish(target) {
        Ok(value) => {
            let hash = resulting_hash(&value);
            let mut published = fact(ExternalChangeStage::Published, hash, None);
            published.observed_previous_hash = observed_previous_hash(&value);
            published.observed_identity = destination_identity;
            record(published)?;
            Ok(value)
        }
        Err(error) => {
            let message = error.to_string();
            let stage = external_failure_stage(&error);
            let observed = error.observed_previous_hash.as_ref().map(ToString::to_string);
            let mut failed = fact(stage, None, Some(message));
            failed.observed_previous_hash = observed;
            failed.observed_identity = destination_identity;
            record(failed)?;
            Err(error.into())
        }
    };
    admission.release();
    outcome
}

/// The destination a granted external write will publish to, known before the lock is taken.
///
/// The grant root is the one the permission owner resolved and the relative path is the one
/// the operation was built with. For a create the file does not exist yet, so the destination
/// is the parent directory plus the leaf name: that is also what a second create of the same
/// name computes, which is what makes the two serialize.
pub(super) fn external_write_destination(
    service: &AgentExecution,
    call: &CallContext,
    relative: &WorkspacePath,
    creating: bool,
) -> Result<aura_fs_tools::ExternalWriteDestination> {
    use aura_fs_tools::ExternalWriteDestination;
    if let Some(grant_id) = external_grant(call) {
        let display = join_display(
            &service.external_grants.record(grant_id)?.selected_root.display,
            relative,
        );
        if !creating {
            return Ok(ExternalWriteDestination::Existing(std::path::PathBuf::from(display)));
        }
        let leaf_name = relative
            .as_str()
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!("agent.external_write_path: the create target has no file name")
            })?
            .to_owned();
        let target = std::path::PathBuf::from(display);
        let parent = target.parent().map_or_else(|| target.clone(), std::path::Path::to_path_buf);
        return Ok(ExternalWriteDestination::New { parent, leaf_name: leaf_name.into() });
    }
    let (path, _) = policy_external_target(call)
        .ok_or_else(|| anyhow::anyhow!("agent.external_file_unavailable: no external resource"))?;
    Ok(ExternalWriteDestination::Existing(std::path::PathBuf::from(&path.display)))
}

/// The authorized external target one write or patch will use.
///
/// Authorization is decided here, on the call path, against the live grant: a create
/// needs a validated parent directory, a replacement needs the file itself opened
/// for writing. The file layer then performs the write on those verified handles and
/// re-checks them, so nothing about the target comes from the model's own spelling.
pub(super) fn external_write_target(
    service: &AgentExecution,
    call: &CallContext,
    grant_id: aura_domain_common::ExternalGrantId,
    relative: &WorkspacePath,
    creating: bool,
) -> Result<aura_fs_tools::ExternalWriteTarget> {
    let session_id = call.snapshot.session_id;
    let correlation = call.operation.content.identity.correlation_id;
    let record = service.external_grants.record(grant_id)?;
    let display_path = join_display(&record.selected_root.display, relative);
    let kind = if creating {
        let parent = service.external_grants.validate_create_parent(
            grant_id,
            session_id,
            relative,
            correlation,
        )?;
        aura_fs_tools::ExternalWriteKind::New {
            parent: parent.parent,
            leaf_name: parent.leaf_name,
            final_path: std::path::PathBuf::from(&parent.final_destination.display),
        }
    } else {
        let resolved = service.external_grants.resolve_existing(
            grant_id,
            session_id,
            relative,
            aura_permission_domain::ExternalGrantCapability::Write,
            aura_windows_platform::NativeFileAccess::ReadWrite,
            correlation,
        )?;
        aura_fs_tools::ExternalWriteKind::Existing(resolved.file)
    };
    Ok(aura_fs_tools::ExternalWriteTarget {
        grant_id: Some(grant_id),
        session_id,
        display_path,
        relative_path: relative.clone(),
        kind,
    })
}

/// The mutation result one external write reports.
///
/// The workspace revision is read, not invented: an external file is not part of the
/// workspace index, so the write cannot move that revision and must not claim it did.
pub(super) fn external_write_result(
    service: &AgentExecution,
    call: &CallContext,
    written: aura_fs_tools::FsExternalWriteResult,
) -> Result<aura_workspace_domain::AgentFileMutationResult> {
    let revision = service.workspace.catalog(call.snapshot.workspace_id)?.revision()?;
    // The model's result names the hash this write reports, for its next change.
    let result = FsMutationResult {
        disposition: FsMutationDisposition::Applied,
        workspace_revision: revision,
        affected_paths: vec![written.relative_path.clone()],
        deferred_mutation_id: None,
        requires_reread: false,
        // There is no index entry to refresh: the file is outside the workspace.
        catalog_refresh_pending: false,
        external: Some(FsExternalWriteTarget {
            grant_id: written.grant_id,
            display_path: written.display_path,
            created: written.created,
            encoding: written.encoding,
            line_ending: written.line_ending,
            logical_hash: written.logical_hash,
            previous_hash: written.previous_hash,
            bytes_written: written.bytes_written,
        }),
    };
    Ok(aura_workspace_domain::AgentFileMutationResult::new(result, None))
}

/// Writes one authorized external file.
pub(super) fn write_external(
    service: &AgentExecution,
    call: &CallContext,
    grant_id: aura_domain_common::ExternalGrantId,
    input: AgentFileWriteInput,
    creating: bool,
    encoding: Option<TextEncoding>,
    line_ending: Option<LineEnding>,
) -> Result<aura_workspace_domain::AgentFileMutationResult> {
    let previous_hash = match &input.expected {
        FsExpectedState::FileHash { hash } => Some(hash.to_string()),
        _ => None,
    };
    let relative = input.path.into_relative();
    let display =
        join_display(&service.external_grants.record(grant_id)?.selected_root.display, &relative);
    let lock_destination = external_write_destination(service, call, &relative, creating)?;
    publish_external(
        service,
        call,
        &display,
        lock_destination,
        aura_persistence_v2::tool::external_change::ExternalChangeAuthority::Grant { grant_id },
        previous_hash,
        || external_write_target(service, call, grant_id, &relative, creating),
        |target| {
            service.fs.write_external(aura_fs_tools::FsExternalWriteRequest {
                target,
                content: input.text,
                expected: input.expected,
                encoding,
                line_ending,
            })
        },
        |written| Some(written.logical_hash.to_string()),
        |written| written.observed_previous_hash.as_ref().map(ToString::to_string),
    )
    .and_then(|written| external_write_result(service, call, written))
}

/// Patches one authorized external file.
pub(super) fn patch_external(
    service: &AgentExecution,
    call: &CallContext,
    grant_id: aura_domain_common::ExternalGrantId,
    input: AgentFilePatchInput,
) -> Result<aura_workspace_domain::AgentFileMutationResult> {
    let expected_hash = input.expected_hash.clone();
    let relative = input.path.into_relative();
    let display =
        join_display(&service.external_grants.record(grant_id)?.selected_root.display, &relative);
    let lock_destination = external_write_destination(service, call, &relative, false)?;
    publish_external(
        service,
        call,
        &display,
        lock_destination,
        aura_persistence_v2::tool::external_change::ExternalChangeAuthority::Grant { grant_id },
        Some(expected_hash.to_string()),
        || external_write_target(service, call, grant_id, &relative, false),
        |target| {
            service.fs.patch_external(aura_fs_tools::FsExternalPatchRequest {
                target,
                unified_diff: input.unified_diff,
                expected_hash: input.expected_hash,
            })
        },
        |written| Some(written.logical_hash.to_string()),
        |written| written.observed_previous_hash.as_ref().map(ToString::to_string),
    )
    .and_then(|written| external_write_result(service, call, written))
}
