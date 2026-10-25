use super::*;

/// Replaces the named file in the verified parent, all-or-nothing, and reports the
/// publication's real stage when it fails.
///
/// Three checks stand between the caller's decision and the rename, and they answer different
/// questions:
///
/// * the destination is still the *object* the caller was authorized for — a swap under the
///   same name fails here regardless of what bytes the new object happens to hold;
/// * the destination still holds the exact *bytes* this call decided about, re-read now
///   rather than trusted from the caller, because an outside process can rewrite the same
///   file object in place and leave its identity intact;
/// * the platform's publication re-opens the destination and compares its identity once more
///   before it moves anything, so a swap in the moment between the two checks still fails.
///
/// All three happen after the caller's publication lock is held, so they are the final
/// re-check rather than another opinion gathered earlier.
pub(super) fn replace_through_parent(
    final_path: &Path,
    target: &aura_windows_platform::NativePathInfo,
    expected_disk_hash: &ContentHash,
    bytes: &[u8],
    display_path: &str,
) -> Result<(), ExternalWriteError> {
    crate::external_faults::reached_before_final_checks(display_path);
    // The object is checked before the contents, because they answer different questions and
    // the object is the one the caller was authorized for. A destination that is now a
    // different file object fails here regardless of what its bytes happen to be; only once
    // it is the verified object does "does it still hold the bytes this call decided about"
    // mean anything.
    let current = aura_windows_platform::inspect_existing_path(final_path, true).map_err(|_| {
        ExternalWriteError::refused(FsToolError::ExternalFileReplaced {
            path: PathBuf::from(display_path),
        })
    })?;
    if current.identity != target.identity {
        return Err(ExternalWriteError::refused(FsToolError::ExternalFileReplaced {
            path: PathBuf::from(display_path),
        }));
    }
    let leaf = leaf_name(final_path).map_err(ExternalWriteError::refused)?;
    let parent = open_publish_parent(final_path, target).map_err(ExternalWriteError::refused)?;
    crate::external_faults::reached_before_platform_publish(display_path);
    // The content is verified **through the publication's own exclusive handle**, not by a
    // separate read that would leave a window: while that handle is open, Windows refuses
    // every other open of the destination, so an in-place rewrite cannot land between the
    // check and the rename. This is the boundary between what the process can coordinate
    // (the caller's per-destination lock, which only orders Aura's own calls) and what the OS
    // enforces (this handle, which excludes every other writer including outside processes).
    let published = aura_windows_platform::handle_bound_replace_checked(
        &parent,
        &leaf,
        &target.identity,
        &expected_disk_hash.hex,
        bytes,
    )
    .map_err(|error| publish_failure(error, display_path))?;
    // Past this point the platform has already renamed the new content into place: a
    // finalization failure is a failure to *confirm* the publication, never evidence that
    // the object was left alone. Reading it as "no effect" is exactly the defect this
    // boundary exists to stop.
    let info = published.commit().map_err(|error| publish_failure(error, display_path))?;
    ensure_same_file(final_path, &aura_workspace::domain_identity(&info.identity)).map_err(
        |error| ExternalWriteError {
            stage: ExternalPublishStage::PublishedUnconfirmed,
            error: Box::new(error),
            observed_previous_hash: None,
        },
    )?;
    crate::publication_receipt::published(&parent, &leaf, Some(&target.identity), &info);
    Ok(())
}
