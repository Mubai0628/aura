use super::*;

/// Writes `bytes` to `target` with the caller's **original content condition** reaching the final
/// exclusive handle.
///
/// # Why the condition has to travel to the handle
///
/// The previous form wrote a sibling temporary and then called the generic `atomic_replace_file`,
/// which takes no condition at all. Everything the caller had decided was checked *before* that
/// call, so there was a window between the check and the publication in which:
///
/// * an independent external writer could rewrite the file **in place** — same `FileIdentity`,
///   different bytes — and the stale decision would still win; or
/// * another writer could create the destination first, and the generic helper's
///   `ReplaceFileW` branch would then replace a file this call never observed, instead of failing
///   a creation whose whole point was "fail if it exists".
///
/// A preceding hash cannot close either window, and a wrapper's name cannot either: the condition
/// must be re-checked while this call holds the destination with no sharing, immediately before the
/// rename. So the write goes through the platform's checked, handle-bound publication, and the
/// expected state is translated into that call's own condition type:
///
/// * `FileHash` becomes the caller's **original** algorithm and hex, recomputed over the held
///   handle — never reinterpreted under another algorithm and never refreshed to a newer hash;
/// * `Absent` becomes "the destination must not exist", and creation is refused if it appeared;
/// * `Directory` never reaches here, because writing a directory is rejected before this point.
///
/// # Why the condition is captured before the staging step
///
/// The changeset path stages the existing object aside *before* it publishes the new one, so by the
/// time this call runs the destination name is empty and its identity can no longer be read. The
/// caller therefore captures the condition from the live object while it is still in place — the
/// same instant it validates `expected` — and passes it in. Reading it here would mean reading the
/// staged-away state, which is not what the caller decided about.
pub(super) fn write_atomic(
    resolver: &SecurePathResolver,
    target: &WorkspacePath,
    bytes: &[u8],
    condition: &HandleCondition,
    previous_identity: Option<&NativeFileIdentity>,
) -> Result<PublishedResult, FsToolError> {
    let parent_validated = resolver.validate_parent(target)?;
    let destination = parent_validated.parent_absolute_path.join(&parent_validated.leaf_name);
    let parent = open_verified_publish_parent(&parent_validated.parent_absolute_path)
        .map_err(|source| io_error("open verified publish parent", &destination, source))?;
    let info = match condition {
        // A first creation's final action must be "fail if it exists". The generic replace helper
        // could not express that: it replaces whatever is there, so a writer that created the
        // destination between the caller's decision and this call would be silently overwritten.
        HandleCondition::Absent => {
            let published = handle_bound_create_absent(&parent, &parent_validated.leaf_name, bytes)
                .map_err(|error| publish_error(target, error))?;
            published.commit().map_err(|error| publish_error(target, error))?
        }
        condition @ HandleCondition::File { .. } => {
            // The structural guard: an `Absent` must never reach the replace branch, because the
            // replace entry has no way to express "the destination must not exist" and its content
            // parameter would then be `Unchecked`. Asserting here means a routing mistake is a
            // visible failure instead of a publication that silently dropped the caller's byte
            // condition — which is the defect this batch repairs, so it must not be re-introducible.
            assert!(
                condition.carries_byte_condition(),
                "the replace path must carry the caller's byte condition, never Unchecked"
            );
            let backup_name = format!(".aura-write-{}.bak", ulid::Ulid::new());
            let identity = condition
                .replace_identity()
                .expect("a File condition always carries the identity it was decided against");
            let published = handle_bound_replace_with_backup_checked(
                &parent,
                &parent_validated.leaf_name,
                identity,
                condition.content(),
                bytes,
                std::ffi::OsStr::new(&backup_name),
            )
            .map_err(|error| publish_error(target, error))?;
            // The publication is real from here; commit deletes the superseded backup. A commit
            // failure rolls the publication back inside the platform and reports which stage
            // really failed, so the caller is never told "not executed" about bytes that did
            // reach the disk.
            published.commit().map_err(|error| publish_error(target, error))?
        }
        // An identity-only condition is a **compensation/undo-move** fact and never belongs on the
        // publication entry. A publish always states what the caller decided about the *bytes* it is
        // replacing, so reaching here with no byte condition would be a routing mistake that dropped
        // the caller's condition — exactly what this batch repairs. It is a loud refusal, not an
        // `Unchecked` publication.
        HandleCondition::IdentityOnly { .. } => {
            return Err(FsToolError::InvalidRequest(format!(
                "internal: {target} was published under an identity-only condition, which carries \
                 no byte condition and must never reach a publication"
            )));
        }
    };
    let receipt = file_result(&info, bytes);
    resolver.verify_after_mutation(target).map_err(|error| {
        FsToolError::ReconciliationRequired(format!(
            "published {target} but verification failed: {error}"
        ))
    })?;
    // Staging may have moved the verified old object aside, making the actual
    // publication an Absent creation. The receipt still names that original
    // object; the caller supplies the identity verified on the staging handle.
    crate::publication_receipt::published(
        &parent,
        &parent_validated.leaf_name,
        previous_identity,
        &info,
    );
    staging_fault::reached(staging_fault::Stage::FilePublished, &destination);
    Ok(receipt)
}
