use std::{
    collections::BTreeSet,
    io::{Read, Take},
};

use aura_domain_common::{ContentHash, WorkspacePath};
use aura_windows_platform::NativeFileAccess;
use thiserror::Error;

use crate::{SecurePathResolver, WorkspaceError, decode_text};

const AGENTS_FILE_NAME: &str = "AGENTS.md";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentsScopeTargetKind {
    File,
    Directory,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentsScopeTarget {
    pub path: WorkspacePath,
    pub kind: AgentsScopeTargetKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentsInstructionDocument {
    pub path: WorkspacePath,
    pub normalized_content: String,
    pub content_hash: ContentHash,
    pub byte_length: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentsScopeResolution {
    pub documents: Vec<AgentsInstructionDocument>,
}

#[derive(Debug, Error)]
pub enum AgentsScopeError {
    #[error(transparent)]
    Workspace(#[from] WorkspaceError),
    #[error("workspace.agents_document_too_large: {path} exceeds {limit_bytes} bytes")]
    DocumentTooLarge { path: WorkspacePath, limit_bytes: u64 },
    #[error("workspace.agents_document_read_failed: {path}: {source}")]
    ReadFailed { path: WorkspacePath, source: std::io::Error },
    #[error("workspace.agents_document_contains_forbidden_control: {path}")]
    ForbiddenControl { path: WorkspacePath },
    #[error("workspace.agents_policy_invalid: max_document_bytes must be positive")]
    InvalidPolicy,
}

#[derive(Clone)]
pub struct AgentsScopeResolver {
    resolver: SecurePathResolver,
    max_document_bytes: u64,
}

impl AgentsScopeResolver {
    pub fn new(
        resolver: SecurePathResolver,
        max_document_bytes: u64,
    ) -> Result<Self, AgentsScopeError> {
        if max_document_bytes == 0 {
            return Err(AgentsScopeError::InvalidPolicy);
        }
        Ok(Self { resolver, max_document_bytes })
    }

    pub fn resolve(
        &self,
        targets: &[AgentsScopeTarget],
    ) -> Result<AgentsScopeResolution, AgentsScopeError> {
        let candidates = candidates_for_targets(targets)?;
        let mut documents = Vec::new();
        for path in candidates {
            match self.read_document(&path) {
                Ok(document) => documents.push(document),
                Err(AgentsScopeError::Workspace(WorkspaceError::NotFound { .. })) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(AgentsScopeResolution { documents })
    }

    fn read_document(
        &self,
        path: &WorkspacePath,
    ) -> Result<AgentsInstructionDocument, AgentsScopeError> {
        let mut verified = self.resolver.open_existing(path, NativeFileAccess::Read)?;
        if verified.file.info().is_directory {
            return Err(WorkspaceError::WrongKind { path: path.clone() }.into());
        }
        let limit =
            self.max_document_bytes.checked_add(1).ok_or(AgentsScopeError::InvalidPolicy)?;
        let mut bytes = Vec::new();
        let mut reader: Take<&mut std::fs::File> = verified.file.file_mut().take(limit);
        reader
            .read_to_end(&mut bytes)
            .map_err(|source| AgentsScopeError::ReadFailed { path: path.clone(), source })?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > self.max_document_bytes {
            return Err(AgentsScopeError::DocumentTooLarge {
                path: path.clone(),
                limit_bytes: self.max_document_bytes,
            });
        }
        let decoded = decode_text(&bytes)?;
        let normalized_content = decoded.text.replace("\r\n", "\n").replace('\r', "\n");
        if normalized_content.chars().any(|character| {
            (character <= '\u{001f}' && !matches!(character, '\n' | '\t'))
                || character == '\u{007f}'
        }) {
            return Err(AgentsScopeError::ForbiddenControl { path: path.clone() });
        }
        let byte_length = u64::try_from(normalized_content.len()).unwrap_or(u64::MAX);
        Ok(AgentsInstructionDocument {
            path: path.clone(),
            content_hash: ContentHash::sha256(normalized_content.as_bytes()),
            normalized_content,
            byte_length,
        })
    }
}

fn candidates_for_targets(
    targets: &[AgentsScopeTarget],
) -> Result<Vec<WorkspacePath>, AgentsScopeError> {
    let mut candidates = BTreeSet::from([
        WorkspacePath::parse(AGENTS_FILE_NAME).map_err(|_| AgentsScopeError::InvalidPolicy)?
    ]);
    for target in targets {
        if target.path.is_root() {
            continue;
        }
        let components = target.path.as_str().split('/').collect::<Vec<_>>();
        let directory_count = match target.kind {
            AgentsScopeTargetKind::File => components.len().saturating_sub(1),
            AgentsScopeTargetKind::Directory => components.len(),
        };
        for depth in 1..=directory_count {
            let relative = format!("{}/{}", components[..depth].join("/"), AGENTS_FILE_NAME);
            candidates.insert(
                WorkspacePath::parse(relative).map_err(|_| AgentsScopeError::InvalidPolicy)?,
            );
        }
    }
    let mut candidates = candidates.into_iter().collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        path_depth(left).cmp(&path_depth(right)).then_with(|| left.as_str().cmp(right.as_str()))
    });
    Ok(candidates)
}

fn path_depth(path: &WorkspacePath) -> usize {
    path.as_str().split('/').count()
}

#[cfg(test)]
mod tests {
    use aura_domain_common::{Revision, WorkspaceId, WorkspacePath};
    use aura_workspace_domain::{CatalogFreshness, WorkspaceState, WorkspaceSummary};

    use super::*;
    use crate::domain_identity;

    fn resolver_for(root: &std::path::Path) -> AgentsScopeResolver {
        let info = SecurePathResolver::inspect_selected_root(root).expect("root info");
        let summary = WorkspaceSummary {
            workspace_id: WorkspaceId::new(),
            display_name: "test".to_owned(),
            selected_root: root.display().to_string(),
            canonical_root: info.final_path.display().to_string(),
            root_identity: Some(domain_identity(&info.identity)),
            state: WorkspaceState::Online,
            revision: Revision::INITIAL,
            catalog_freshness: CatalogFreshness::Unavailable,
            authorization_epoch: Revision::INITIAL,
        };
        AgentsScopeResolver::new(
            SecurePathResolver::from_workspace(&summary).expect("path resolver"),
            1_024,
        )
        .expect("agents resolver")
    }

    #[test]
    fn resolves_root_to_deepest_and_deduplicates_shared_scopes() {
        let root = tempfile::tempdir().expect("root");
        std::fs::create_dir_all(root.path().join("src/nested")).expect("directories");
        std::fs::write(root.path().join("AGENTS.md"), "root\r\n").expect("root agents");
        std::fs::write(root.path().join("src/AGENTS.md"), "src\n").expect("src agents");
        std::fs::write(root.path().join("src/nested/AGENTS.md"), "nested\n")
            .expect("nested agents");
        let resolution = resolver_for(root.path())
            .resolve(&[
                AgentsScopeTarget {
                    path: WorkspacePath::parse("src/nested/file.rs").expect("path"),
                    kind: AgentsScopeTargetKind::File,
                },
                AgentsScopeTarget {
                    path: WorkspacePath::parse("src/other.rs").expect("path"),
                    kind: AgentsScopeTargetKind::File,
                },
            ])
            .expect("resolution");
        assert_eq!(
            resolution.documents.iter().map(|document| document.path.as_str()).collect::<Vec<_>>(),
            vec!["AGENTS.md", "src/AGENTS.md", "src/nested/AGENTS.md"]
        );
        assert_eq!(resolution.documents[0].normalized_content, "root\n");
    }

    #[test]
    fn rejects_oversized_documents_without_returning_partial_content() {
        let root = tempfile::tempdir().expect("root");
        std::fs::write(root.path().join("AGENTS.md"), vec![b'a'; 1_025]).expect("agents");
        let error = resolver_for(root.path()).resolve(&[]).expect_err("oversized");
        assert!(matches!(error, AgentsScopeError::DocumentTooLarge { .. }));
    }
}
