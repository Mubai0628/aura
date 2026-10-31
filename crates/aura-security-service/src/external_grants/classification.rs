use super::*;
impl ExternalGrantService {
    /// Test-only scope inspection. Production callers must supply required capabilities.
    #[cfg(any(test, feature = "test-hooks"))]
    pub fn grant_covering_path(
        &self,
        session_id: SessionId,
        workspace_id: WorkspaceId,
        path: &Path,
        turn_task_id: Option<TurnTaskId>,
        correlation_id: CorrelationId,
    ) -> Result<Option<CoveredExternalPath>, ExternalGrantError> {
        self.covering_path(
            session_id,
            workspace_id,
            path,
            turn_task_id,
            correlation_id,
            None,
            false,
        )
    }
    /// Resolves a live grant against both the required capabilities and the requested reach.
    #[allow(clippy::too_many_arguments)]
    pub fn grant_for_capabilities(
        &self,
        session_id: SessionId,
        workspace_id: WorkspaceId,
        path: &Path,
        turn_task_id: Option<TurnTaskId>,
        correlation_id: CorrelationId,
        capabilities: &[aura_security_contract::ResolvedCapability],
        enumerates_contents: bool,
    ) -> Result<Option<CoveredExternalPath>, ExternalGrantError> {
        self.covering_path(
            session_id,
            workspace_id,
            path,
            turn_task_id,
            correlation_id,
            Some(capabilities),
            enumerates_contents,
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn covering_path(
        &self,
        session_id: SessionId,
        workspace_id: WorkspaceId,
        path: &Path,
        turn_task_id: Option<TurnTaskId>,
        correlation_id: CorrelationId,
        capabilities: Option<&[aura_security_contract::ResolvedCapability]>,
        enumerates_contents: bool,
    ) -> Result<Option<CoveredExternalPath>, ExternalGrantError> {
        self.ensure_session_workspace(session_id, workspace_id)?;
        for record in self.records_for_classification(session_id, workspace_id)? {
            if !grant_applies(&record, session_id, workspace_id, turn_task_id) {
                continue;
            }
            if capabilities
                .is_some_and(|required| !self.covers_capabilities(record.id, session_id, required))
            {
                continue;
            }
            // The root has to still be the identity it was authorized for, exactly
            // as the read path requires; a changed root is revoked, not used.
            let Ok(root) = self.inspect_current_root(&record, correlation_id) else {
                continue;
            };
            // The verified final path this grant was matched against. It is carried out so the
            // caller classifies the object that was really identity-checked, not the model's
            // spelling of it.
            let verified_final_path;
            let relative = match inspect_existing_path(path, true) {
                Ok(candidate) => {
                    verified_final_path = candidate.final_path.clone();
                    match record.target_kind {
                        ExternalGrantTargetKind::File => {
                            if candidate.identity != root.identity {
                                continue;
                            }
                            WorkspacePath::parse(WorkspacePath::ROOT).expect("root path")
                        }
                        ExternalGrantTargetKind::Directory => {
                            if !final_path_within(&root, &candidate) {
                                continue;
                            }
                            let Some(relative) =
                                relative_under(&root.final_path, &candidate.final_path)
                            else {
                                continue;
                            };
                            relative
                        }
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    // A path that does not exist yet — a create, or a name under a
                    // missing entry — is classified by its real parent directory, not
                    // by its spelling. The parent must exist and must really be inside
                    // the root, so `..`, a differently spelled drive or a link cannot
                    // move the target, and the two sides of the comparison are both
                    // canonical rather than one canonical and one user spelling. The
                    // grant's own create path re-validates the parent identity before
                    // writing.
                    if record.target_kind != ExternalGrantTargetKind::Directory {
                        continue;
                    }
                    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
                        continue;
                    };
                    let Ok(parent_info) = inspect_existing_path(parent, true) else {
                        continue;
                    };
                    if !parent_info.is_directory || !final_path_within(&root, &parent_info) {
                        continue;
                    }
                    let Some(directory) = relative_under(&root.final_path, &parent_info.final_path)
                    else {
                        continue;
                    };
                    let Some(relative) = child_under(&directory, &name.to_string_lossy()) else {
                        continue;
                    };
                    // The target does not exist, so the object that would be created is named
                    // by its verified parent plus the leaf — the same shape a second create of
                    // that name computes.
                    verified_final_path = parent_info.final_path.join(name);
                    relative
                }
                Err(_) => continue,
            };
            // The depth the grant authorizes is checked here too: a non-recursive
            // directory grant covers its direct children and nothing deeper, so a
            // classification must not accept a path this grant's own resolution would
            // refuse.
            if !within_authorized_depth(&record, &relative)
                || (enumerates_contents && !contents_are_authorized(&record, &relative))
            {
                continue;
            }
            return Ok(Some(CoveredExternalPath {
                grant_id: record.id,
                root_display: record.canonical_root.display.clone(),
                relative_path: relative,
                canonical_path: canonical_path(verified_final_path.display().to_string()),
            }));
        }
        Ok(None)
    }
}
