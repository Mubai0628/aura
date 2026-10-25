use super::*;

pub(super) fn push_children(
    listing: &mut ChildListing<'_>,
) -> Result<DirectoryListing, FsToolError> {
    let info = inspect_contained(listing.root_info, listing.absolute)?;
    if info.is_reparse_point || !info.is_directory {
        return Err(FsToolError::ExternalFileReplaced { path: listing.absolute.to_path_buf() });
    }
    let modified = path_modified(listing.absolute)?;
    let resumed = {
        let replaced =
            listing.memory.recall(listing.root_key, listing.directory).is_some_and(|seen| {
                seen.identity != info.identity
                    || seen.is_reparse != info.is_reparse_point
                    || (listing.resume && seen.modified != modified)
            });
        if replaced {
            listing.memory.drop_unread(listing.root_key, listing.directory);
            return Err(FsToolError::ExternalFileReplaced { path: listing.absolute.to_path_buf() });
        }
        let unread = if listing.resume {
            listing.memory.take_unread(listing.root_key, listing.directory)
        } else {
            None
        };
        let kept_modified = if listing.resume {
            listing
                .memory
                .recall(listing.root_key, listing.directory)
                .map(|seen| seen.modified)
                .unwrap_or(modified)
        } else {
            modified
        };
        listing.memory.remember(
            listing.root_key,
            listing.directory,
            RememberedObject {
                identity: info.identity.clone(),
                modified: kept_modified,
                is_reparse: false,
                unread: None,
            },
        );
        unread
    };
    // A resumed iterator is already past the names the previous page read. Opening
    // the directory again would repeat that work; every entry this call actually
    // pulls from the iterator counts against the budget, including a prefix that
    // had to be read because the iterator was no longer available.
    let mut entries = match resumed {
        Some(entries) => entries,
        None => std::fs::read_dir(listing.absolute).map_err(|source| {
            crate::error::io_error(
                "search the authorized external directory",
                listing.absolute,
                source,
            )
        })?,
    };
    let mut read = 0_u64;
    let mut accepted = Vec::new();
    loop {
        if listing.cancel.is_some_and(crate::search::SearchCancel::is_cancelled) {
            return Err(FsToolError::Cancelled);
        }
        if read >= listing.budget {
            listing.memory.store_unread(listing.root_key, listing.directory, entries);
            return Ok(DirectoryListing { progress: read, truncated: true, accepted });
        }
        let Some(entry) = entries.next() else {
            return Ok(DirectoryListing { progress: read, truncated: false, accepted });
        };
        let entry = entry.map_err(|source| {
            crate::error::io_error(
                "read the authorized external directory entry",
                listing.absolute,
                source,
            )
        })?;
        let name = entry.file_name().into_string().map_err(|name| {
            FsToolError::InvalidRequest(format!(
                "the authorized external directory contains an entry whose name is not \
                 representable: {}",
                PathBuf::from(name).display()
            ))
        })?;
        let child_path = entry.path();
        let child = inspect_contained(listing.root_info, &child_path)?;
        read = read.saturating_add(1);
        if !crate::read_scope::allows(&child) {
            accepted.push(name);
            continue;
        }
        if listing.consumed.iter().any(|seen| seen == &name) {
            continue;
        }
        let relative_path = child_relative(listing.directory, &name)?;
        let kind = kind_of(&child);
        let byte_length = (kind == FsNodeKind::File)
            .then(|| std::fs::metadata(&child_path).map(|meta| meta.len()).unwrap_or(0));
        if !child.is_reparse_point
            && let Ok(child_modified) = path_modified(&child_path)
        {
            listing.memory.remember(
                listing.root_key,
                &relative_path,
                RememberedObject {
                    identity: child.identity,
                    modified: child_modified,
                    is_reparse: child.is_reparse_point,
                    unread: None,
                },
            );
        }
        accepted.push(name.clone());
        listing.pending.push(PendingExternalEntry {
            relative_path,
            absolute: child_path,
            name,
            kind,
            byte_length,
        });
    }
}
