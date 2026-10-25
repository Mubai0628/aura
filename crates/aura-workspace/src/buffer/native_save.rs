//! Conditional publication on held native handles.
use super::*;

/// Saves one buffer's bytes with the caller's **original content condition** reaching the final
/// exclusive handle, and returns the identity the replacement really produced.
///
/// # Why the condition has to travel to the handle
///
/// The previous form wrote a sibling temporary and then called the generic `atomic_replace_file`,
/// which takes no condition. The caller's expectation — the `disk_hash` the buffer was read from —
/// was checked before that call, so an independent external writer that rewrote the file **in
/// place** between the check and the publication kept the same `FileIdentity` and the stale
/// decision still won. A preceding hash cannot close that window and neither can the wrapper's
/// name; the condition must be re-checked while this call holds the destination with no sharing.
///
/// So the save uses the platform's checked, handle-bound publication with the caller's own
/// algorithm and hex. The expected hash is **never** refreshed to whatever the file holds now: a
/// destination that no longer holds the expected bytes is refused, and the caller is told so.
pub(super) fn atomic_save(
    resolver: &SecurePathResolver,
    path: &WorkspacePath,
    bytes: &[u8],
    expected: &ContentHash,
    expected_identity: &FileIdentity,
    publications: &mut Vec<crate::BufferPublicationReceipt>,
) -> Result<FileIdentity, WorkspaceError> {
    let verified = resolver.open_existing(path, NativeFileAccess::Metadata)?;
    let final_path = verified.file.info().final_path.clone();
    let parent_path =
        final_path.parent().ok_or_else(|| WorkspaceError::WrongKind { path: path.clone() })?;
    let parent = open_verified_publish_parent(parent_path)
        .map_err(|source| io_error("open verified buffer publish parent", parent_path, source))?;
    let leaf =
        final_path.file_name().ok_or_else(|| WorkspaceError::WrongKind { path: path.clone() })?;
    // The expected identity is the domain form; the platform compares its own native form. The
    // conversion goes through the same hex encoding `domain_identity` produced, so the two agree
    // by construction rather than by two independent formats drifting apart.
    let expected_target = native_identity(expected_identity)?;
    let name = leaf.to_string_lossy();
    let native_name_key = match aura_windows_platform::directory_case_rule(&parent) {
        aura_windows_platform::VolumeCaseRule::Sensitive => Some(name.into_owned()),
        aura_windows_platform::VolumeCaseRule::InsensitivePreserving => {
            aura_windows_platform::directory_name_comparison(&parent).map(|map| map.key(&name))
        }
        aura_windows_platform::VolumeCaseRule::Unknown => None,
    };
    let backup_name = format!(".aura-buffer-{}.bak", ulid::Ulid::new());
    let published = handle_bound_replace_with_backup_checked(
        &parent,
        leaf,
        &expected_target,
        HandleBoundReplaceContent::Hex {
            algorithm: match expected.algorithm {
                ContentHashAlgorithm::Blake3 => PlatformHashAlgorithm::Blake3,
                ContentHashAlgorithm::Sha256 => PlatformHashAlgorithm::Sha256,
            },
            expected: &expected.hex,
        },
        bytes,
        std::ffi::OsStr::new(&backup_name),
    )
    .map_err(|error| {
        WorkspaceError::Watcher(format!("atomic buffer save failed for {path}: {error}"))
    })?;
    // Read the replacement's identity from the held handle before the commit consumes it: the
    // identity is the fact the buffer is re-keyed to, and it must come from the object this call
    // really published rather than from a fresh path lookup that a later writer could change.
    let new_identity = crate::domain_identity(&published.info().identity);
    published.commit().map_err(|error| {
        WorkspaceError::Watcher(format!("atomic buffer save commit failed for {path}: {error}"))
    })?;
    if let Some(native_name_key) = native_name_key {
        publications.push(crate::BufferPublicationReceipt {
            path: path.clone(),
            workspace_id: resolver.workspace_id(),
            previous_identity: expected_identity.clone(),
            published_identity: new_identity.clone(),
            parent_identity: crate::domain_identity(&parent.info().identity),
            native_name_key,
        });
    }
    resolver.verify_after_mutation(path)?;
    Ok(new_identity)
}

/// The platform's native form of a domain identity.
///
/// `domain_identity` writes the file id as lowercase hex and the volume serial as decimal text, so
/// this reads exactly those back. A malformed value is a real conflict rather than a silent zero:
/// comparing against a made-up identity would report "the object changed" for a file that did not.
fn native_identity(identity: &FileIdentity) -> Result<NativeFileIdentity, WorkspaceError> {
    let invalid = || {
        WorkspaceError::Watcher(format!(
            "buffer save expected identity is not a native identity: volume={} file_id={}",
            identity.volume_serial_number, identity.file_id
        ))
    };
    let volume_serial_number =
        identity.volume_serial_number.parse::<u64>().map_err(|_| invalid())?;
    if identity.file_id.len() != 32 {
        return Err(invalid());
    }
    let mut file_id = [0_u8; 16];
    for (index, slot) in file_id.iter_mut().enumerate() {
        let pair = &identity.file_id[index * 2..index * 2 + 2];
        *slot = u8::from_str_radix(pair, 16).map_err(|_| invalid())?;
    }
    Ok(NativeFileIdentity { volume_serial_number, file_id })
}
