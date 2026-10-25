use aura_workspace::WorkspaceError;

use crate::FsToolError;

pub(crate) enum ClassifiedWorkspace {
    Temporary,
    Changed,
    Fatal(FsToolError),
}

pub(crate) fn classify_workspace(error: WorkspaceError) -> ClassifiedWorkspace {
    match error {
        WorkspaceError::NotFound { .. } | WorkspaceError::Busy { .. } => {
            ClassifiedWorkspace::Temporary
        }
        WorkspaceError::BufferRevisionConflict { .. }
        | WorkspaceError::FileVersionConflict { .. } => ClassifiedWorkspace::Changed,
        other => ClassifiedWorkspace::Fatal(FsToolError::Workspace(other)),
    }
}

#[cfg(test)]
mod tests {
    use aura_domain_common::WorkspacePath;

    use super::*;

    #[test]
    fn security_failures_are_fatal_not_temporary() {
        for error in [
            WorkspaceError::RootIdentityChanged,
            WorkspaceError::Offline,
            WorkspaceError::Archived,
            WorkspaceError::ScopeUnavailable { path: WorkspacePath::parse("a.txt").expect("path") },
            WorkspaceError::PathResolutionFailed {
                path: std::path::PathBuf::from("a.txt"),
                source: std::io::Error::new(std::io::ErrorKind::InvalidInput, "reparse"),
            },
        ] {
            assert!(matches!(classify_workspace(error), ClassifiedWorkspace::Fatal(_)));
        }
    }

    #[test]
    fn not_found_and_busy_stay_temporary() {
        assert!(matches!(
            classify_workspace(WorkspaceError::NotFound {
                path: WorkspacePath::parse("a.txt").expect("path"),
            }),
            ClassifiedWorkspace::Temporary
        ));
        assert!(matches!(
            classify_workspace(WorkspaceError::Busy { reason: "lease".into() }),
            ClassifiedWorkspace::Temporary
        ));
    }
}
