//! Explicit lifecycle decisions and editor publication facts.
use super::*;

impl WorkspaceRuntime {
    pub fn resolve_lifecycle(
        &self,
        input: ResolveWorkspaceLifecycleInput,
    ) -> Result<WorkspaceLifecycleSummary, WorkspaceError> {
        self.resolve_lifecycle_with_publications(input).result
    }

    pub fn resolve_lifecycle_with_publications(
        &self,
        input: ResolveWorkspaceLifecycleInput,
    ) -> crate::WorkspacePublicationOutcome<WorkspaceLifecycleSummary> {
        crate::WorkspacePublicationOutcome::capture(|publications| {
            let mut lifecycles = self.lifecycle_operations()?;
            let workspace_id = find_workspace_for_operation(&lifecycles, input.operation_id)?;
            let current = lifecycles.get(&workspace_id).ok_or(WorkspaceError::LifecycleNotFound)?;
            validate_operation_revision_and_renderer(
                current,
                input.expected_lifecycle_revision,
                input.renderer_instance_id,
            )?;
            if current.record.summary.state != WorkspaceLifecycleState::Blocked {
                return Err(WorkspaceError::LifecycleTransitionInProgress);
            }
            if input.action == WorkspaceLifecycleResolution::DiscardDirtyBuffersAndContinue {
                self.validate_discard_confirmation(current, &input)?;
            }
            let mut entry =
                lifecycles.remove(&workspace_id).ok_or(WorkspaceError::LifecycleNotFound)?;

            match input.action {
                WorkspaceLifecycleResolution::ReturnToEditing => {
                    self.transition(&mut entry, WorkspaceLifecycleState::Cancelled, None, None)?;
                    self.buffers.cancel_lifecycle(workspace_id)?;
                    self.notify_projection_change()?;
                    Ok(entry.record.summary)
                }
                WorkspaceLifecycleResolution::SaveAllAndContinue => {
                    entry.discard_confirmation = None;
                    self.transition(&mut entry, WorkspaceLifecycleState::Saving, None, None)?;
                    self.notify_projection_change()?;
                    let save_result = self.resolver(workspace_id).and_then(|resolver| {
                        let saved =
                            self.buffers.save_all_dirty_for_lifecycle_with_publications(&resolver);
                        publications.extend(saved.publications);
                        saved.result
                    });
                    if save_result.is_err() {
                        let current = self.buffers.current_lifecycle_states(workspace_id)?;
                        let revision = increment(entry.record.summary.lifecycle_revision)?;
                        let preflight = self.build_preflight(&entry, &current, revision)?;
                        self.transition(
                            &mut entry,
                            WorkspaceLifecycleState::Blocked,
                            Some(preflight),
                            Some("workspace.lifecycle_save_failed".to_owned()),
                        )?;
                        let summary = entry.record.summary.clone();
                        lifecycles.insert(workspace_id, entry);
                        self.notify_projection_change()?;
                        return Ok(summary);
                    }
                    self.reflight_and_commit(&mut entry, &mut lifecycles)
                }
                WorkspaceLifecycleResolution::DiscardDirtyBuffersAndContinue => {
                    let confirmation = entry
                        .discard_confirmation
                        .take()
                        .ok_or(WorkspaceError::DiscardConfirmationRequired)?;
                    self.transition(&mut entry, WorkspaceLifecycleState::Discarding, None, None)?;
                    self.notify_projection_change()?;
                    self.buffers.discard_dirty_for_lifecycle(
                        workspace_id,
                        &confirmation.confirmation.affected_buffers,
                    )?;
                    self.reflight_and_commit(&mut entry, &mut lifecycles)
                }
            }
        })
    }
}
