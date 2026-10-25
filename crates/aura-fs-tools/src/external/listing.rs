use super::*;

impl FsToolService {
    /// Lists one already-authorized external directory.
    ///
    /// An external tree has no index, so this reads the directory itself: one level,
    /// sorted exactly as a workspace listing is, bounded by the caller's limit, and
    /// never descending into a child. The handle the permission owner verified fixes
    /// the directory; the identity is re-checked afterwards so a directory replaced
    /// while it was read is reported instead of being listed under its old name.
    pub fn list_external(
        &self,
        request: FsExternalListRequest,
    ) -> Result<FsExternalListResult, FsToolError> {
        let FsExternalListRequest { target, limit, after_name } = request;
        if !(1..=MAX_EXTERNAL_LIMIT).contains(&limit) {
            return Err(FsToolError::InvalidRequest(format!(
                "limit must be in 1..={MAX_EXTERNAL_LIMIT}"
            )));
        }
        let final_path = target.directory.info().final_path.clone();
        if !target.directory.info().is_directory {
            return Err(FsToolError::InvalidRequest(
                "an external listing target must name a directory".into(),
            ));
        }
        let identity = aura_workspace::domain_identity(&target.directory.info().identity);
        let mut entries = std::fs::read_dir(&final_path)
            .map_err(|source| {
                crate::error::io_error(
                    "list the authorized external directory",
                    &final_path,
                    source,
                )
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|source| {
                crate::error::io_error(
                    "read the authorized external directory entry",
                    &final_path,
                    source,
                )
            })?;
        entries.sort_by(|left, right| {
            let left = left.file_name();
            let right = right.file_name();
            let ordering =
                aura_windows_platform::windows_ordinal_compare_ignore_case(&left, &right)
                    .unwrap_or(std::cmp::Ordering::Equal);
            ordering.then_with(|| left.cmp(&right))
        });
        // The version describes the directory, not the page: it covers every child the
        // directory holds, so an unchanged directory reports one version for every page
        // and a continuation is rejected only for a change that really happened. A
        // page-scoped digest would make each continuation look stale.
        let all = entries
            .iter()
            .filter_map(|entry| {
                let child =
                    aura_windows_platform::inspect_existing_path(&entry.path(), true).ok()?;
                if !crate::read_scope::allows(&child) {
                    return None;
                }
                let name = entry.file_name().into_string().ok()?;
                let file_type = entry.file_type().ok()?;
                let metadata = entry.metadata().ok()?;
                Some((name, file_type, metadata))
            })
            .collect::<Vec<_>>();
        let mut facts = all
            .iter()
            .map(|(name, file_type, metadata)| {
                (name.clone(), file_type.is_dir(), metadata.len(), false)
            })
            .collect::<Vec<_>>();
        facts.sort();
        let scope_revision = external_scope_version(&facts)?;
        // A continuation token may arrive in either spelling the caller holds: the
        // entry name this listing returns, or the grant-relative path it returns for
        // the same entry. Both name one position, so the token is normalized to the
        // name first; comparing `tree/a.txt` as a name would skip every entry that
        // sorts before it.
        let after = after_name.as_deref().map(|after| resume_name(&target.relative_path, after));
        let listed = all
            .into_iter()
            .filter(|(name, _, _)| {
                after.as_ref().is_none_or(|after| {
                    let ordering = aura_windows_platform::windows_ordinal_compare_ignore_case(
                        std::ffi::OsStr::new(name),
                        std::ffi::OsStr::new(after),
                    )
                    .unwrap_or(std::cmp::Ordering::Equal);
                    ordering.then_with(|| name.as_str().cmp(after.as_str()))
                        == std::cmp::Ordering::Greater
                })
            })
            .collect::<Vec<_>>();
        let truncated = listed.len() > limit as usize;
        let page = listed.into_iter().take(limit as usize).collect::<Vec<_>>();
        let entries = page
            .into_iter()
            .map(|(name, file_type, metadata)| {
                let relative_path = child_relative(&target.relative_path, &name)?;
                Ok(FsExternalListEntry {
                    relative_path,
                    name,
                    kind: if file_type.is_dir() { FsNodeKind::Directory } else { FsNodeKind::File },
                    byte_length: (!file_type.is_dir()).then_some(metadata.len()),
                    reparse_tag: None,
                })
            })
            .collect::<Result<Vec<_>, FsToolError>>()?;
        let next_after_name =
            truncated.then(|| entries.last().map(|entry| entry.name.clone())).flatten();
        // A directory replaced while it was read must not be reported as the
        // directory the permission owner verified.
        ensure_same_file(&final_path, &identity)?;
        Ok(FsExternalListResult {
            grant_id: target.grant_id,
            display_path: target.display_path,
            relative_path: target.relative_path,
            entries,
            scope_revision,
            truncated,
            next_after_name,
        })
    }
}
