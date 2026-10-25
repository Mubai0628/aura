//! Actual per-object copy receipts; cleanup never enumerates a tree and claims its occupants.
use super::*;

#[derive(Clone)]
pub(super) struct OwnMember {
    pub path: WorkspacePath,
    pub result: PublishedResult,
}

pub(super) struct DirectoryCopy {
    pub result: PublishedResult,
    pub members: Vec<OwnMember>,
}

/// Pre-effect snapshot of a directory about to be moved/staged. This is not
/// used to claim ownership of a copied tree; copies retain kernel creation receipts.
pub(super) fn snapshot_members(
    resolver: &SecurePathResolver,
    root: &WorkspacePath,
) -> Result<Vec<OwnMember>, FsToolError> {
    fn collect(
        resolver: &SecurePathResolver,
        directory: &Path,
        result: &mut Vec<OwnMember>,
    ) -> Result<(), FsToolError> {
        let mut children = fs::read_dir(directory)
            .map_err(|error| io_error("snapshot directory", directory, error))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| io_error("snapshot directory entry", directory, error))?;
        children.sort_by_key(std::fs::DirEntry::file_name);
        for child in children {
            let path = child.path();
            let mut file = aura_windows_platform::open_held_for_read(&path)
                .map_err(|error| io_error("hold directory snapshot member", &path, error))?;
            let info = file.info().clone();
            let receipt = if info.is_directory {
                PublishedResult::Directory { identity: Some(domain_identity_of(&info.identity)) }
            } else {
                PublishedResult::File {
                    identity: domain_identity_of(&info.identity),
                    hash: hash_held(file.file_mut())?,
                }
            };
            result.push(OwnMember { path: logical(resolver, &path)?, result: receipt });
            if info.is_directory && !info.is_reparse_point {
                collect(resolver, &path, result)?;
            }
        }
        Ok(())
    }
    let mut result = Vec::new();
    collect(resolver, &resolver.absolute_path(root), &mut result)?;
    Ok(result)
}

pub(super) fn rebase(
    members: &mut [OwnMember],
    from: &WorkspacePath,
    to: &WorkspacePath,
) -> Result<(), FsToolError> {
    let prefix = format!("{}/", from.as_str());
    for member in members {
        let tail = member.path.as_str().strip_prefix(&prefix).ok_or_else(|| {
            FsToolError::ReconciliationRequired("directory snapshot member escaped its root".into())
        })?;
        member.path = WorkspacePath::parse(format!("{}/{tail}", to.as_str()))
            .map_err(|error| FsToolError::InvalidRequest(error.to_string()))?;
    }
    Ok(())
}

pub(super) fn verify_members(
    resolver: &SecurePathResolver,
    root: &WorkspacePath,
    expected: &[OwnMember],
) -> Result<(), FsToolError> {
    let mut actual = snapshot_members(resolver, root)?;
    actual.sort_by(|a, b| a.path.as_str().cmp(b.path.as_str()));
    let mut expected = expected.to_vec();
    expected.sort_by(|a, b| a.path.as_str().cmp(b.path.as_str()));
    if actual.len() != expected.len()
        || actual.iter().zip(&expected).any(|(a, b)| a.path != b.path || a.result != b.result)
    {
        return Err(FsToolError::ReconciliationRequired(format!(
            "directory {root} gained, lost or changed members after this execution's snapshot; no restore or undo move was performed"
        )));
    }
    Ok(())
}

pub(super) fn file_result(
    info: &aura_windows_platform::NativePathInfo,
    bytes: &[u8],
) -> PublishedResult {
    PublishedResult::File {
        identity: domain_identity_of(&info.identity),
        hash: ContentHash::blake3(bytes),
    }
}

/// Receipts use an internal BLAKE3 digest. The caller's expected algorithm is
/// checked separately by HandleCondition and is never replaced by this digest.
pub(super) fn hash_held(file: &mut std::fs::File) -> Result<ContentHash, FsToolError> {
    use std::io::Seek;
    file.rewind().map_err(|error| FsToolError::ReconciliationRequired(error.to_string()))?;
    let mut buffer = [0_u8; 64 * 1024];
    let mut digest = blake3::Hasher::new();
    loop {
        let size = file
            .read(&mut buffer)
            .map_err(|error| FsToolError::ReconciliationRequired(error.to_string()))?;
        if size == 0 {
            break;
        }
        digest.update(&buffer[..size]);
    }
    ContentHash::new(ContentHashAlgorithm::Blake3, digest.finalize().to_hex().to_string())
        .map_err(|error| FsToolError::InvalidRequest(error.to_string()))
}

fn logical(resolver: &SecurePathResolver, path: &Path) -> Result<WorkspacePath, FsToolError> {
    let relative = path.strip_prefix(resolver.canonical_root()).map_err(|error| {
        FsToolError::ReconciliationRequired(format!(
            "copied object is outside its verified root: {error}"
        ))
    })?;
    WorkspacePath::parse(relative.to_string_lossy().replace('\\', "/"))
        .map_err(|error| FsToolError::InvalidRequest(error.to_string()))
}

pub(super) fn copy_directory(
    resolver: &SecurePathResolver,
    source: &Path,
    destination: &Path,
) -> Result<DirectoryCopy, FsToolError> {
    let mut receipts = Vec::new();
    let copied = walk(resolver, source, destination, &mut receipts);
    if let Err(primary) = copied {
        let mut failures = Vec::new();
        for own in receipts.iter().rev() {
            if let Err(error) = StagingArea::remove_own_result(resolver, &own.path, &own.result) {
                failures.push(error.to_string());
            }
        }
        return if failures.is_empty() {
            Err(primary)
        } else {
            Err(FsToolError::ReconciliationRequired(format!(
                "directory copy failed ({primary}); its own partial material could not all be removed: {}",
                failures.join("; "),
            )))
        };
    }
    let mut items = receipts.into_iter();
    let root = items.next().ok_or_else(|| {
        FsToolError::ReconciliationRequired("directory copy returned no creation receipt".into())
    })?;
    Ok(DirectoryCopy { result: root.result, members: items.collect() })
}

fn walk(
    resolver: &SecurePathResolver,
    source: &Path,
    destination: &Path,
    receipts: &mut Vec<OwnMember>,
) -> Result<(), FsToolError> {
    let source_handle = aura_windows_platform::open_held_for_read(source)
        .map_err(|error| io_error("hold copy source directory", source, error))?;
    if !source_handle.info().is_directory || source_handle.info().is_reparse_point {
        return Err(FsToolError::InvalidRequest(
            "directory copy does not traverse reparse points".into(),
        ));
    }
    resolver.open_absolute_existing(source, NativeFileAccess::Metadata)?;
    let parent_path = destination
        .parent()
        .ok_or_else(|| FsToolError::InvalidRequest("copy destination has no parent".into()))?;
    let parent = resolver.open_absolute_existing(parent_path, NativeFileAccess::Metadata)?;
    let leaf = destination
        .file_name()
        .ok_or_else(|| FsToolError::InvalidRequest("copy destination has no name".into()))?;
    let created =
        aura_windows_platform::handle_bound_create_directory(&parent, leaf).map_err(|error| {
            FsToolError::ReconciliationRequired(format!("create copied directory: {error}"))
        })?;
    let result =
        PublishedResult::Directory { identity: Some(domain_identity_of(&created.info().identity)) };
    let path = logical(resolver, destination)?;
    receipts.push(OwnMember { path, result: result.clone() });
    drop(created);
    // A publication parent denies rename without requesting DELETE. The platform
    // may independently reopen that parent to verify its result; a DELETE handle
    // here would conflict with that verification's own no-share-delete handle.
    let held_destination = open_verified_publish_parent(destination)
        .map_err(|error| io_error("hold copied publication parent", destination, error))?;
    if let PublishedResult::Directory { identity } = result
        && identity.as_ref() != Some(&domain_identity_of(&held_destination.info().identity))
    {
        return Err(FsToolError::ReconciliationRequired(
            "copied directory was replaced after creation".into(),
        ));
    }
    let mut children = fs::read_dir(source)
        .map_err(|error| io_error("enumerate copy source", source, error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| io_error("read copy entry", source, error))?;
    children.sort_by_key(std::fs::DirEntry::file_name);
    for child in children {
        let from = child.path();
        let to = destination.join(child.file_name());
        let mut input = aura_windows_platform::open_held_for_read(&from)
            .map_err(|error| io_error("hold copy source member", &from, error))?;
        if input.info().is_reparse_point {
            return Err(FsToolError::InvalidRequest(format!(
                "recursive copy does not traverse reparse point {}",
                from.display()
            )));
        }
        if input.info().is_directory {
            drop(input);
            walk(resolver, &from, &to, receipts)?;
        } else {
            resolver.open_absolute_existing(&from, NativeFileAccess::Metadata)?;
            let mut bytes = Vec::new();
            input
                .file_mut()
                .read_to_end(&mut bytes)
                .map_err(|error| io_error("read held copy source", &from, error))?;
            let published =
                handle_bound_create_absent(&held_destination, &child.file_name(), &bytes).map_err(
                    |error| {
                        FsToolError::ReconciliationRequired(format!("publish copy member: {error}"))
                    },
                )?;
            receipts.push(OwnMember {
                path: logical(resolver, &to)?,
                result: file_result(published.info(), &bytes),
            });
            published.commit().map_err(|error| {
                FsToolError::ReconciliationRequired(format!("settle copied member: {error}"))
            })?;
            staging_fault::reached(staging_fault::Stage::FilePublished, &to);
        }
    }
    Ok(())
}
