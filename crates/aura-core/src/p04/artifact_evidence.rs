use super::*;

impl P04Services {
    /// Bind export preparation to the artifact owner, current grant and live
    /// target handle. The existing export code owns publication and rollback.
    pub(crate) fn broker_artifact_facts(
        &self,
        input: &ArtifactExportRequest,
        correlation: CorrelationId,
    ) -> Result<Value, IpcErrorEnvelope> {
        ensure_session_workspace(
            &self.persistence,
            input.session_id,
            input.workspace_id,
            correlation,
        )?;
        let artifact = self
            .persistence
            .artifact_record(ArtifactMetadataRequest {
                artifact_id: input.artifact_id,
                session_id: Some(input.session_id),
                required_access: ArtifactAccess::EXPORT,
                allow_quarantine: false,
            })
            .map_err(|e| persistence_error(e, correlation))?;
        let target = match &input.target {
            ArtifactExportTarget::Workspace { path } => {
                let resolver = self
                    .workspace
                    .resolver(input.workspace_id)
                    .map_err(|e| internal_error(e, correlation))?;
                match resolver.open_existing(path, NativeFileAccess::Read) {
                    Ok(mut file) => {
                        let identity = domain_identity(&file.file.info().identity);
                        let hash = hash_verified_reader(
                            file.file.file_mut(),
                            artifact.content_hash.algorithm,
                            correlation,
                        )?;
                        serde_json::json!({"identity":identity,"hash":hash})
                    }
                    Err(WorkspaceError::NotFound { .. }) => {
                        let parent = resolver
                            .validate_parent(path)
                            .map_err(|e| internal_error(e, correlation))?;
                        serde_json::json!({"absent":true,"parent_identity":domain_identity(&parent.parent.info().identity)})
                    }
                    Err(e) => return Err(internal_error(e, correlation)),
                }
            }
            ArtifactExportTarget::ExternalGrant { external_grant_id, relative_path } => {
                let capability = if matches!(input.expected, FsExpectedState::Absent) {
                    ExternalGrantCapability::Create
                } else {
                    ExternalGrantCapability::Write
                };
                let resolved = self.external_grants.resolve_existing(
                    *external_grant_id,
                    input.session_id,
                    relative_path,
                    capability,
                    NativeFileAccess::Read,
                    correlation,
                );
                match resolved {
                    Ok(mut target) => {
                        let identity = domain_identity(&target.file.info().identity);
                        let hash = hash_verified_reader(
                            target.file.file_mut(),
                            artifact.content_hash.algorithm,
                            correlation,
                        )?;
                        serde_json::json!({"grant":target.grant,"identity":identity,"hash":hash})
                    }
                    Err(_) if matches!(input.expected, FsExpectedState::Absent) => {
                        let parent = self
                            .external_grants
                            .validate_create_parent(
                                *external_grant_id,
                                input.session_id,
                                relative_path,
                                correlation,
                            )
                            .map_err(|e| external_grant_error(e, correlation))?;
                        serde_json::json!({"grant":parent.grant,"absent":true,"parent_identity":domain_identity(&parent.parent.info().identity)})
                    }
                    Err(e) => return Err(external_grant_error(e, correlation)),
                }
            }
        };
        Ok(
            serde_json::json!({"artifact_id":artifact.artifact_id,"artifact_hash":artifact.content_hash,"target":target}),
        )
    }
}
