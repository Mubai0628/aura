//! A read-only completion check on an explicit workspace artifact, from its disk handle.
use super::super::{AgentExecution, control_resources, protected_paths};
use aura_agent_domain::ExactArtifactValue;
use aura_domain_common::{ContentHash, ContentHashAlgorithm, WorkspaceId, WorkspacePath};
use aura_windows_platform::NativeFileAccess;
use std::io::Read as _;

const MAX_REUSE_HASH_BYTES: u64 = 2 * 1024 * 1024;

impl AgentExecution {
    pub async fn unverified_delivery_value(
        self: &std::sync::Arc<Self>,
        workspace: WorkspaceId,
        values: Vec<ExactArtifactValue>,
    ) -> anyhow::Result<Option<ExactArtifactValue>> {
        if values.is_empty() {
            return Ok(None);
        }
        let service = self.clone();
        Ok(tokio::task::spawn_blocking(move || {
            values
                .into_iter()
                .find(|value| !service.verify_delivered_value(workspace, value).unwrap_or(false))
        })
        .await?)
    }

    fn verify_delivered_value(
        &self,
        workspace: WorkspaceId,
        value: &ExactArtifactValue,
    ) -> anyhow::Result<bool> {
        let mut opened = self.open_evidence_file(workspace, &value.path)?;
        let observed_path = opened.file.info().final_path.clone();
        let text = self.fs.read_opened_delivery_text(opened.file.file_mut(), &observed_path)?;
        Ok(value.matches(&text))
    }

    /// Check a small recorded artifact against the bytes at its workspace path.
    pub fn recorded_workspace_file_matches(
        &self,
        workspace: WorkspaceId,
        path: &WorkspacePath,
        expected: &ContentHash,
    ) -> bool {
        let Ok(mut opened) = self.open_evidence_file(workspace, path) else { return false };
        let mut bytes = Vec::new();
        let file = opened.file.file_mut();
        if !file.metadata().is_ok_and(|value| value.len() <= MAX_REUSE_HASH_BYTES)
            || file.take(MAX_REUSE_HASH_BYTES + 1).read_to_end(&mut bytes).is_err()
            || bytes.len() as u64 > MAX_REUSE_HASH_BYTES
        {
            return false;
        }
        let observed = match expected.algorithm {
            ContentHashAlgorithm::Blake3 => ContentHash::blake3(&bytes),
            ContentHashAlgorithm::Sha256 => ContentHash::sha256(&bytes),
        };
        &observed == expected
    }

    fn open_evidence_file(
        &self,
        workspace: WorkspaceId,
        path: &WorkspacePath,
    ) -> anyhow::Result<aura_workspace::VerifiedWorkspaceFile> {
        let resolver = self.workspace.resolver(workspace)?;
        let opened = resolver.open_existing(path, NativeFileAccess::Read)?;
        let info = opened.file.info();
        anyhow::ensure!(
            !info.is_directory
                && !info.is_reparse_point
                && !self.control_resources.contains(info)
                && !control_resources::in_folders(&self.control_resources.data_folders(), info)
                && protected_paths::classify(&info.final_path)
                    == protected_paths::ExternalPathClass::Ordinary,
            "workspace evidence file is unavailable"
        );
        Ok(opened)
    }
}
