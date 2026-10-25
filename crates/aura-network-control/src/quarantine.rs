#[cfg(test)]
use std::{fs::OpenOptions, io::Write, path::Path};

use aura_artifact_contract::{ArtifactAccess, ArtifactOwnerRef, ArtifactScope};
use aura_domain_common::{ContentHash, CorrelationId, QuarantineRecordId, SessionId, UtcTimestamp};
use aura_permission_domain::{QuarantineRecord, QuarantineRecordDetails, QuarantineState};
use aura_persistence::{
    ARTIFACT_RANGE_LIMIT, ArtifactReadRequest as PersistenceArtifactReadRequest,
    ArtifactWriteRequest, PersistenceCoordinator,
};

use crate::NetworkControlError;

#[derive(Clone, Copy, Debug, Default)]
pub struct ExecutableDetector;

impl ExecutableDetector {
    #[must_use]
    pub fn detect(
        &self,
        original_name: &str,
        media_type: Option<&str>,
        content_disposition: Option<&str>,
        bytes: &[u8],
    ) -> Vec<String> {
        let mut kinds = Vec::new();
        let lower = original_name.to_ascii_lowercase();
        if [".exe", ".dll", ".sys", ".scr", ".com", ".cpl"]
            .iter()
            .any(|suffix| lower.ends_with(suffix))
        {
            kinds.push("windows_pe_extension".to_owned());
        }
        if [".msi", ".msp"].iter().any(|suffix| lower.ends_with(suffix)) {
            kinds.push("windows_installer_extension".to_owned());
        }
        if [".msix", ".appx", ".msixbundle", ".appxbundle"]
            .iter()
            .any(|suffix| lower.ends_with(suffix))
        {
            kinds.push("windows_package_extension".to_owned());
        }
        if valid_pe_magic(bytes) {
            kinds.push("pe_magic".to_owned());
        }
        if looks_like_msi_compound(bytes) {
            kinds.push("windows_installer_magic".to_owned());
        }
        if media_type.is_some_and(|media| {
            matches!(
                media.to_ascii_lowercase().as_str(),
                "application/x-msdownload"
                    | "application/x-ms-installer"
                    | "application/vnd.microsoft.portable-executable"
                    | "application/msix"
            )
        }) {
            kinds.push("executable_media_type".to_owned());
        }
        if content_disposition.is_some_and(|value| {
            let value = value.to_ascii_lowercase();
            [".exe", ".msi", ".msix", ".appx", ".scr", ".com"]
                .iter()
                .any(|suffix| value.contains(suffix))
        }) {
            kinds.push("executable_content_disposition".to_owned());
        }
        kinds.sort();
        kinds.dedup();
        kinds
    }
}

/// Executable-only Quarantine backed by P02's content-addressed Artifact
/// Store. The Quarantine record is a logical reference; it never owns a
/// separate physical file that can bypass Artifact ref-counting.
#[derive(Clone)]
pub struct QuarantineStore {
    persistence: PersistenceCoordinator,
}

pub struct QuarantineWriteRequest<'a> {
    pub original_name: String,
    pub source_url: String,
    pub media_type: Option<String>,
    pub content_disposition: Option<String>,
    pub bytes: &'a [u8],
    pub session_id: SessionId,
    pub correlation_id: CorrelationId,
}

impl QuarantineStore {
    #[must_use]
    pub const fn new(persistence: PersistenceCoordinator) -> Self {
        Self { persistence }
    }

    pub fn quarantine_if_executable(
        &self,
        request: QuarantineWriteRequest<'_>,
    ) -> Result<Option<QuarantineRecordDetails>, NetworkControlError> {
        let QuarantineWriteRequest {
            original_name,
            source_url,
            media_type,
            content_disposition,
            bytes,
            session_id,
            correlation_id,
        } = request;
        let detected_kinds = ExecutableDetector.detect(
            &original_name,
            media_type.as_deref(),
            content_disposition.as_deref(),
            bytes,
        );
        if detected_kinds.is_empty() {
            return Ok(None);
        }
        let content_hash = ContentHash::blake3(bytes);
        let artifact = self.persistence.write_artifact(ArtifactWriteRequest {
            scope: ArtifactScope::Quarantine,
            media_type: media_type.clone().unwrap_or_else(|| "application/octet-stream".to_owned()),
            access: ArtifactAccess::USER_READ.union(ArtifactAccess::EXPORT),
            owner: ArtifactOwnerRef::Session { session_id },
            bytes: bytes.to_vec(),
        })?;
        let now = UtcTimestamp::now();
        let details = QuarantineRecordDetails {
            record: QuarantineRecord {
                id: QuarantineRecordId::new(),
                original_name,
                source_url: sanitize_source_url(&source_url),
                media_type,
                content_disposition,
                detected_kinds,
                content_hash,
                byte_length: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
                state: QuarantineState::Quarantined,
                created_at: now,
                resolved_at: None,
            },
            artifact: Some(artifact.clone()),
            storage_path: None,
        };
        match self.persistence.create_quarantine_record(details, correlation_id) {
            Ok(details) => Ok(Some(details)),
            Err(error) => {
                let _ = self.persistence.enqueue_artifact_gc_if_unreferenced(
                    artifact.artifact_id,
                    "quarantine_record_creation_failed",
                );
                Err(error.into())
            }
        }
    }

    pub fn read_verified(
        &self,
        details: &QuarantineRecordDetails,
    ) -> Result<Vec<u8>, NetworkControlError> {
        if details.record.state != QuarantineState::Quarantined {
            return Err(NetworkControlError::QuarantineNotFound);
        }
        let artifact = details.artifact.as_ref().ok_or(NetworkControlError::QuarantineIntegrity)?;
        let session_id = match artifact.owner {
            ArtifactOwnerRef::Session { session_id } => session_id,
            _ => return Err(NetworkControlError::QuarantineIntegrity),
        };
        if artifact.scope != ArtifactScope::Quarantine
            || artifact.content_hash != details.record.content_hash
            || artifact.byte_length != details.record.byte_length
        {
            return Err(NetworkControlError::QuarantineIntegrity);
        }
        let mut bytes = Vec::with_capacity(
            usize::try_from(artifact.byte_length).unwrap_or(ARTIFACT_RANGE_LIMIT),
        );
        let mut offset = 0_u64;
        while offset < artifact.byte_length {
            let remaining = artifact.byte_length.saturating_sub(offset);
            let length = usize::try_from(remaining)
                .unwrap_or(ARTIFACT_RANGE_LIMIT)
                .min(ARTIFACT_RANGE_LIMIT);
            let chunk = self.persistence.read_artifact_range(PersistenceArtifactReadRequest {
                artifact_id: artifact.artifact_id,
                session_id: Some(session_id),
                required_access: ArtifactAccess::EXPORT,
                offset,
                length,
                allow_quarantine: true,
            })?;
            if chunk.is_empty() {
                return Err(NetworkControlError::QuarantineIntegrity);
            }
            offset = offset.saturating_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX));
            bytes.extend_from_slice(&chunk);
        }
        if ContentHash::blake3(&bytes) != details.record.content_hash
            || u64::try_from(bytes.len()).unwrap_or(u64::MAX) != details.record.byte_length
        {
            return Err(NetworkControlError::QuarantineIntegrity);
        }
        Ok(bytes)
    }

    /// Exercises create-new publication in unit tests only. Production release
    /// is intentionally owned by Aura Core's exact `QuarantineReleaseBinding`
    /// and consumed `AllowOnce` approval path.
    #[cfg(test)]
    fn release_new_file_for_test(
        &self,
        details: &QuarantineRecordDetails,
        destination: &Path,
    ) -> Result<(), NetworkControlError> {
        let bytes = self.read_verified(details)?;
        let mut destination_file =
            OpenOptions::new().write(true).create_new(true).open(destination)?;
        destination_file.write_all(&bytes)?;
        destination_file.sync_all()?;
        Ok(())
    }

    pub fn discard(
        &self,
        details: &QuarantineRecordDetails,
        correlation_id: CorrelationId,
    ) -> Result<(), NetworkControlError> {
        let artifact = details.artifact.as_ref().ok_or(NetworkControlError::QuarantineIntegrity)?;
        self.persistence.resolve_quarantine(
            details.record.id,
            QuarantineState::Discarded,
            correlation_id,
        )?;
        self.persistence
            .enqueue_artifact_gc_if_unreferenced(artifact.artifact_id, "quarantine_discarded")?;
        Ok(())
    }
}

fn valid_pe_magic(bytes: &[u8]) -> bool {
    if bytes.len() < 64 || !bytes.starts_with(b"MZ") {
        return false;
    }
    let offset = u32::from_le_bytes([bytes[60], bytes[61], bytes[62], bytes[63]]) as usize;
    bytes.get(offset..offset.saturating_add(4)) == Some(b"PE\0\0".as_slice())
}

fn looks_like_msi_compound(bytes: &[u8]) -> bool {
    const COMPOUND_HEADER: &[u8] = &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    bytes.starts_with(COMPOUND_HEADER)
        && ["_StringPool", "_StringData", "_Tables"]
            .iter()
            .all(|name| contains_utf16le_ascii(bytes, name))
}

fn contains_utf16le_ascii(bytes: &[u8], value: &str) -> bool {
    let needle = value.bytes().flat_map(|byte| [byte, 0]).collect::<Vec<_>>();
    bytes.windows(needle.len()).any(|window| window == needle)
}

fn sanitize_source_url(value: &str) -> String {
    if let Ok(mut url) = url::Url::parse(value) {
        let _ = url.set_username("");
        let _ = url.set_password(None);
        url.set_query(None);
        url.set_fragment(None);
        url.into()
    } else {
        value.split(['?', '#']).next().unwrap_or_default().to_owned()
    }
}

#[cfg(test)]
mod tests {
    use aura_domain_common::{CorrelationId, Revision, RuntimeInstanceId, WorkspaceId};
    use aura_persistence::{PersistenceOptions, PersistenceStartup};
    use aura_workspace_domain::{
        CatalogFreshness, SessionSummary, WorkspaceState, WorkspaceSummary,
    };

    use super::*;

    fn store(directory: &tempfile::TempDir) -> (QuarantineStore, SessionId) {
        let persistence = match PersistenceCoordinator::start(PersistenceOptions::new(
            directory.path().join("aura.db"),
            directory.path().join("artifacts"),
            RuntimeInstanceId::new(),
        ))
        .expect("start persistence")
        {
            PersistenceStartup::Ready(value) => value,
            PersistenceStartup::RecoveryRequired(value) => panic!("recovery: {}", value.reason),
        };
        let workspace_id = WorkspaceId::new();
        let session_id = SessionId::new();
        persistence
            .upsert_workspace_session(
                WorkspaceSummary {
                    workspace_id,
                    display_name: "Quarantine test".to_owned(),
                    selected_root: r"C:\Aura Quarantine Test".to_owned(),
                    canonical_root: r"C:\Aura Quarantine Test".to_owned(),
                    root_identity: None,
                    state: WorkspaceState::Online,
                    revision: Revision::INITIAL,
                    catalog_freshness: CatalogFreshness::Unavailable,
                    authorization_epoch: Revision::INITIAL,
                },
                SessionSummary {
                    session_id,
                    workspace_id,
                    title: "Quarantine test".to_owned(),
                    state: "active".to_owned(),
                },
                CorrelationId::new(),
            )
            .expect("seed quarantine owner session");
        (QuarantineStore::new(persistence), session_id)
    }

    #[test]
    fn detects_pe_magic_even_with_a_text_extension() {
        let mut bytes = vec![0_u8; 132];
        bytes[0..2].copy_from_slice(b"MZ");
        bytes[60..64].copy_from_slice(&128_u32.to_le_bytes());
        bytes[128..132].copy_from_slice(b"PE\0\0");
        let kinds = ExecutableDetector.detect("notes.txt", Some("text/plain"), None, &bytes);
        assert_eq!(kinds, vec!["pe_magic"]);
    }

    #[test]
    fn ordinary_ole_document_is_not_treated_as_an_installer() {
        let bytes = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0, 0, 0, 0, 0, 0, 0, 0];
        assert!(ExecutableDetector.detect("report.doc", None, None, &bytes).is_empty());
    }

    #[test]
    fn detects_disguised_msi_compound_structure() {
        let mut bytes = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
        for name in ["_StringPool", "_StringData", "_Tables"] {
            bytes.extend(name.bytes().flat_map(|byte| [byte, 0]));
        }
        assert_eq!(
            ExecutableDetector.detect("archive.bin", None, None, &bytes),
            vec!["windows_installer_magic"]
        );
    }

    #[test]
    fn quarantine_uses_artifact_store_and_release_never_overwrites() {
        let directory = tempfile::tempdir().expect("temp directory");
        let (store, session_id) = store(&directory);
        let details = store
            .quarantine_if_executable(QuarantineWriteRequest {
                original_name: "tool.exe".to_owned(),
                source_url: "https://example.test/tool.exe?secret=redacted".to_owned(),
                media_type: None,
                content_disposition: None,
                bytes: b"not-a-real-executable",
                session_id,
                correlation_id: CorrelationId::new(),
            })
            .expect("quarantine")
            .expect("executable detected");
        assert!(details.artifact.is_some());
        assert_eq!(details.record.source_url, "https://example.test/tool.exe");
        let destination = directory.path().join("released.exe");
        store.release_new_file_for_test(&details, &destination).expect("release");
        assert!(store.release_new_file_for_test(&details, &destination).is_err());
        assert_eq!(std::fs::read(destination).expect("released bytes"), b"not-a-real-executable");
    }
}
