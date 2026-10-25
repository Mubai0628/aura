//! Buffer save result and publication facts for Core composition.
use super::*;

impl WorkspaceRuntime {
    pub fn save_buffer(
        &self,
        command: BufferSaveCommand,
    ) -> Result<LogicalFileStatus, WorkspaceError> {
        self.save_buffer_with_publications(command).result
    }

    pub fn save_buffer_with_publications(
        &self,
        command: BufferSaveCommand,
    ) -> crate::WorkspacePublicationOutcome<LogicalFileStatus> {
        crate::WorkspacePublicationOutcome::capture(|publications| {
            let resolver = self.resolver(command.workspace_id)?;
            let status = self.buffers.save_with_publications(
                &resolver,
                command.session_id,
                &command.path,
                command.reservation_id,
                command.reservation_epoch,
                command.expected_buffer_revision,
            );
            publications.extend(status.publications);
            let status = status.result?;
            self.notify_projection_change()?;
            Ok(status)
        })
    }
}
