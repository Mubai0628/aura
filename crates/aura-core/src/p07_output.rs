use crate::package_program::media_type::valid_media_type_v1;
use std::{collections::BTreeMap, io::Read as _, path::Path};

use aura_artifact_contract::{
    ArtifactAccess, ArtifactOwnerRef, ArtifactRecord, ArtifactScope, ToolArtifactOwnerRef,
};
use aura_domain_common::{ContentHash, ContentHashAlgorithm};
use aura_domain_common::{ToolConformanceRunId, ToolExecutionAttemptId};
use aura_persistence::{ArtifactWriteRequest, PersistenceCoordinator, PersistenceError};
use aura_tool_contract::{
    StdioArtifactReadyPayloadV1, ToolOutputSlotKeyV1, ToolRuntimeOutputAuthorizationV1,
};
use aura_tool_runtime::StdioObservedArtifactReadyV1;
use aura_windows_platform::{
    NativeFileIdentity, open_verified_tool_output_from_root, open_verified_tool_output_root,
};
use thiserror::Error;

#[derive(Clone, Debug)]
pub(crate) struct VerifiedToolOutputArtifactV1 {
    pub sequence: u64,
    pub slot: ToolOutputSlotKeyV1,
    pub media_type: String,
    pub artifact: ArtifactRecord,
}

#[derive(Clone)]
struct AcceptedOutputV1 {
    declaration: StdioArtifactReadyPayloadV1,
    identity: NativeFileIdentity,
    artifact: VerifiedToolOutputArtifactV1,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ToolOutputFailureClassV1 {
    ArtifactInvalid,
    IoLimitExceeded,
    ProtocolViolation,
    CredentialCanaryDetected,
}

#[derive(Debug, Error)]
pub(crate) enum ToolOutputProcessingErrorV1 {
    #[error("the Runtime output authorization does not match this Attempt")]
    Authorization,
    #[error("the Runtime declared conflicting output facts for one slot")]
    SlotConflict,
    #[error("two Runtime output slots resolve to the same file")]
    FileAlias,
    #[error("the Runtime output count or byte limit was exceeded")]
    LimitExceeded,
    #[error("the Runtime output media type is invalid")]
    InvalidMediaType,
    #[error("the Runtime output file failed its no-follow identity checks")]
    InvalidFile,
    #[error("the Runtime output length or SHA-256 does not match its declaration")]
    ContentMismatch,
    #[error("the Runtime output contains the Attempt credential canary")]
    CredentialCanary,
    #[error("the Attempt output root could not be cleaned")]
    Cleanup,
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
}

impl ToolOutputProcessingErrorV1 {
    #[must_use]
    pub const fn class(&self) -> ToolOutputFailureClassV1 {
        match self {
            Self::LimitExceeded => ToolOutputFailureClassV1::IoLimitExceeded,
            Self::SlotConflict => ToolOutputFailureClassV1::ProtocolViolation,
            Self::CredentialCanary => ToolOutputFailureClassV1::CredentialCanaryDetected,
            Self::Authorization
            | Self::FileAlias
            | Self::InvalidMediaType
            | Self::InvalidFile
            | Self::ContentMismatch
            | Self::Cleanup
            | Self::Persistence(_) => ToolOutputFailureClassV1::ArtifactInvalid,
        }
    }
}

/// Verifies Runtime-declared outputs against the one-shot dispatch authority,
/// imports only bytes read through held no-follow handles, and leaves every
/// imported Artifact inaccessible until ToolResult finalization adds an
/// explicit Session access link.
pub(crate) fn import_stdio_attempt_outputs_v1(
    persistence: &PersistenceCoordinator,
    attempt_id: ToolExecutionAttemptId,
    output_root: &Path,
    authorization: &ToolRuntimeOutputAuthorizationV1,
    observations: &[StdioObservedArtifactReadyV1],
    credential_canary: Option<&[u8]>,
) -> Result<Vec<VerifiedToolOutputArtifactV1>, ToolOutputProcessingErrorV1> {
    verify_and_import_stdio_attempt_outputs_v1(
        attempt_id,
        output_root,
        authorization,
        observations,
        credential_canary,
        |payload, bytes| {
            persistence
                .write_artifact(ArtifactWriteRequest {
                    scope: ArtifactScope::ToolExecution,
                    media_type: payload.media_type.clone(),
                    // This is only the capability ceiling. With no AccessLink
                    // the Attempt-owned output remains inaccessible.
                    access: ArtifactAccess::MODEL_READ.union(ArtifactAccess::USER_READ),
                    owner: ArtifactOwnerRef::Tool {
                        owner: ToolArtifactOwnerRef::ExecutionAttempt {
                            tool_execution_attempt_id: attempt_id,
                        },
                    },
                    bytes,
                })
                .map_err(ToolOutputProcessingErrorV1::Persistence)
        },
    )
}

/// W3 counterpart to Attempt output import. It uses the same no-follow,
/// identity, hash, media, canary and quota checks, but assigns every accepted
/// Artifact directly to the exact Conformance run and never creates an
/// installed Tool Attempt or Session access link.
pub(crate) fn import_stdio_conformance_outputs_v1(
    persistence: &PersistenceCoordinator,
    conformance_run_id: ToolConformanceRunId,
    attempt_id: ToolExecutionAttemptId,
    output_root: &Path,
    authorization: &ToolRuntimeOutputAuthorizationV1,
    observations: &[StdioObservedArtifactReadyV1],
    credential_canary: Option<&[u8]>,
) -> Result<Vec<VerifiedToolOutputArtifactV1>, ToolOutputProcessingErrorV1> {
    verify_and_import_stdio_attempt_outputs_v1(
        attempt_id,
        output_root,
        authorization,
        observations,
        credential_canary,
        |payload, bytes| {
            persistence
                .write_artifact(ArtifactWriteRequest {
                    scope: ArtifactScope::Quarantine,
                    media_type: payload.media_type.clone(),
                    access: ArtifactAccess::USER_READ,
                    owner: ArtifactOwnerRef::Tool {
                        owner: ToolArtifactOwnerRef::ConformanceRun {
                            tool_conformance_run_id: conformance_run_id,
                        },
                    },
                    bytes,
                })
                .map_err(ToolOutputProcessingErrorV1::Persistence)
        },
    )
}

fn verify_and_import_stdio_attempt_outputs_v1<F>(
    attempt_id: ToolExecutionAttemptId,
    output_root: &Path,
    authorization: &ToolRuntimeOutputAuthorizationV1,
    observations: &[StdioObservedArtifactReadyV1],
    credential_canary: Option<&[u8]>,
    mut import: F,
) -> Result<Vec<VerifiedToolOutputArtifactV1>, ToolOutputProcessingErrorV1>
where
    F: FnMut(
        &StdioArtifactReadyPayloadV1,
        Vec<u8>,
    ) -> Result<ArtifactRecord, ToolOutputProcessingErrorV1>,
{
    authorization.validate().map_err(|_| ToolOutputProcessingErrorV1::Authorization)?;
    if authorization.tool_execution_attempt_id != attempt_id {
        return Err(ToolOutputProcessingErrorV1::Authorization);
    }

    // Even an Attempt with no declared outputs must prove that its private
    // root is the exact non-reparse directory bound into the launch plan.
    drop(
        open_verified_tool_output_root(output_root)
            .map_err(|_| ToolOutputProcessingErrorV1::InvalidFile)?,
    );
    let mut cleanup = OutputRootCleanupGuardV1::new(output_root);

    let mut accepted_by_slot = BTreeMap::<String, AcceptedOutputV1>::new();
    let mut accepted_identities = Vec::<NativeFileIdentity>::new();
    let mut total_bytes = 0_u64;

    for observation in observations {
        let payload = &observation.payload;
        if let Some(existing) = accepted_by_slot.get(payload.slot.as_str()) {
            if existing.declaration == *payload {
                continue;
            }
            return Err(ToolOutputProcessingErrorV1::SlotConflict);
        }
        if u32::try_from(accepted_by_slot.len()).unwrap_or(u32::MAX)
            >= authorization.max_artifact_count
            || payload.byte_length > authorization.max_single_artifact_bytes
        {
            return Err(ToolOutputProcessingErrorV1::LimitExceeded);
        }
        if !valid_media_type_v1(&payload.media_type) {
            return Err(ToolOutputProcessingErrorV1::InvalidMediaType);
        }

        let relative = Path::new(payload.relative_path.as_str());
        let mut source = open_verified_tool_output_from_root(output_root, relative)
            .map_err(|_| ToolOutputProcessingErrorV1::InvalidFile)?;
        if accepted_identities.iter().any(|identity| identity == &source.info().identity) {
            return Err(ToolOutputProcessingErrorV1::FileAlias);
        }
        let observed_length =
            source.file().metadata().map_err(|_| ToolOutputProcessingErrorV1::InvalidFile)?.len();
        if observed_length != payload.byte_length {
            return Err(ToolOutputProcessingErrorV1::ContentMismatch);
        }
        total_bytes = total_bytes
            .checked_add(observed_length)
            .ok_or(ToolOutputProcessingErrorV1::LimitExceeded)?;
        if total_bytes > authorization.max_total_artifact_bytes {
            return Err(ToolOutputProcessingErrorV1::LimitExceeded);
        }

        let read_limit = authorization
            .max_single_artifact_bytes
            .checked_add(1)
            .ok_or(ToolOutputProcessingErrorV1::LimitExceeded)?;
        let mut bytes = Vec::with_capacity(
            usize::try_from(observed_length)
                .map_err(|_| ToolOutputProcessingErrorV1::LimitExceeded)?,
        );
        source
            .file_mut()
            .take(read_limit)
            .read_to_end(&mut bytes)
            .map_err(|_| ToolOutputProcessingErrorV1::InvalidFile)?;
        if u64::try_from(bytes.len()).ok() != Some(observed_length)
            || payload.content_hash.algorithm != ContentHashAlgorithm::Sha256
            || ContentHash::sha256(&bytes) != payload.content_hash
        {
            return Err(ToolOutputProcessingErrorV1::ContentMismatch);
        }
        if credential_canary.is_some_and(|canary| contains_bytes(&bytes, canary)) {
            return Err(ToolOutputProcessingErrorV1::CredentialCanary);
        }

        let artifact = import(payload, bytes)?;
        let verified = VerifiedToolOutputArtifactV1 {
            sequence: observation.sequence,
            slot: payload.slot.clone(),
            media_type: payload.media_type.clone(),
            artifact,
        };
        accepted_identities.push(source.info().identity.clone());
        accepted_by_slot.insert(
            payload.slot.as_str().to_owned(),
            AcceptedOutputV1 {
                declaration: payload.clone(),
                identity: source.info().identity.clone(),
                artifact: verified,
            },
        );
    }

    let mut accepted = accepted_by_slot
        .into_values()
        .map(|value| {
            let _ = value.identity;
            value.artifact
        })
        .collect::<Vec<_>>();
    accepted.sort_by_key(|artifact| artifact.sequence);
    cleanup.run()?;
    Ok(accepted)
}

struct OutputRootCleanupGuardV1<'a> {
    output_root: &'a Path,
    armed: bool,
}

impl<'a> OutputRootCleanupGuardV1<'a> {
    const fn new(output_root: &'a Path) -> Self {
        Self { output_root, armed: true }
    }

    fn run(&mut self) -> Result<(), ToolOutputProcessingErrorV1> {
        cleanup_output_root(self.output_root)?;
        self.armed = false;
        Ok(())
    }
}

impl Drop for OutputRootCleanupGuardV1<'_> {
    fn drop(&mut self) {
        if self.armed {
            let _ = cleanup_output_root(self.output_root);
        }
    }
}

pub(crate) fn cleanup_output_root(output_root: &Path) -> Result<(), ToolOutputProcessingErrorV1> {
    drop(
        open_verified_tool_output_root(output_root)
            .map_err(|_| ToolOutputProcessingErrorV1::Cleanup)?,
    );
    std::fs::remove_dir_all(output_root).map_err(|_| ToolOutputProcessingErrorV1::Cleanup)
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|candidate| candidate == needle)
}

/// RFC token-compatible media type with optional token/quoted parameters.
/// This validates syntax only; the Runtime cannot use it to choose access.
#[cfg(test)]
mod tests {
    use aura_artifact_contract::{
        ArtifactAccess, ArtifactOwnerRef, ArtifactRecord, ArtifactScope, ToolArtifactOwnerRef,
    };
    use aura_domain_common::{ArtifactId, ContentHash, UtcTimestamp};
    use aura_domain_common::{ToolCallId, ToolExecutionAttemptId};
    use aura_tool_contract::{
        StdioArtifactReadyPayloadV1, ToolOutputRelativePathV1, ToolOutputSlotKeyV1,
        ToolRuntimeOutputAuthorizationV1,
    };
    use aura_tool_runtime::StdioObservedArtifactReadyV1;

    use super::{
        ToolOutputProcessingErrorV1, valid_media_type_v1,
        verify_and_import_stdio_attempt_outputs_v1,
    };

    #[test]
    fn media_type_syntax_accepts_safe_parameters_and_rejects_controls() {
        assert!(valid_media_type_v1("application/json"));
        assert!(valid_media_type_v1("text/plain; charset=utf-8"));
        assert!(valid_media_type_v1("text/plain; name=\"Aura output.txt\""));
        assert!(!valid_media_type_v1("text"));
        assert!(!valid_media_type_v1("text/plain\r\nX-Aura: value"));
        assert!(!valid_media_type_v1("text/plain; charset"));
    }

    #[test]
    fn output_import_is_handle_verified_idempotent_and_attempt_owned() {
        let root = tempfile::tempdir().expect("data root");
        let output_root = root.path().join("output");
        std::fs::create_dir(&output_root).expect("output root");
        let bytes = b"verified fixture output";
        std::fs::write(output_root.join("result.txt"), bytes).expect("output file");
        let call_id = ToolCallId::new();
        let attempt_id = ToolExecutionAttemptId::new();
        let authorization = ToolRuntimeOutputAuthorizationV1::new(call_id, attempt_id);
        let declaration = declaration("result", "result.txt", bytes, 3);
        let imported = verify_and_import_stdio_attempt_outputs_v1(
            attempt_id,
            &output_root,
            &authorization,
            &[declaration.clone(), declaration],
            None,
            fake_import(attempt_id),
        )
        .expect("verified output import");
        assert_eq!(imported.len(), 1);
        assert_eq!(imported[0].slot.as_str(), "result");
        assert_eq!(
            imported[0].artifact.owner,
            aura_artifact_contract::ArtifactOwnerRef::Tool {
                owner: aura_artifact_contract::ToolArtifactOwnerRef::ExecutionAttempt {
                    tool_execution_attempt_id: attempt_id,
                },
            }
        );
        assert!(!output_root.exists(), "private output root was cleaned");
    }

    #[test]
    fn output_import_rejects_aliases_hardlinks_canaries_and_limits() {
        let root = tempfile::tempdir().expect("data root");
        let call_id = ToolCallId::new();
        let attempt_id = ToolExecutionAttemptId::new();
        let authorization = ToolRuntimeOutputAuthorizationV1::new(call_id, attempt_id);

        let alias_root = root.path().join("alias-output");
        std::fs::create_dir(&alias_root).expect("alias root");
        let bytes = b"same file";
        std::fs::write(alias_root.join("result.txt"), bytes).expect("alias file");
        let alias = verify_and_import_stdio_attempt_outputs_v1(
            attempt_id,
            &alias_root,
            &authorization,
            &[
                declaration("first", "result.txt", bytes, 2),
                declaration("second", "result.txt", bytes, 3),
            ],
            None,
            fake_import(attempt_id),
        );
        assert!(matches!(alias, Err(ToolOutputProcessingErrorV1::FileAlias)));
        assert!(!alias_root.exists());

        let hardlink_root = root.path().join("hardlink-output");
        std::fs::create_dir(&hardlink_root).expect("hardlink root");
        std::fs::write(hardlink_root.join("result.txt"), bytes).expect("hardlink source");
        std::fs::hard_link(hardlink_root.join("result.txt"), hardlink_root.join("alias.txt"))
            .expect("create hardlink");
        let hardlink = verify_and_import_stdio_attempt_outputs_v1(
            attempt_id,
            &hardlink_root,
            &authorization,
            &[declaration("result", "result.txt", bytes, 2)],
            None,
            fake_import(attempt_id),
        );
        assert!(matches!(hardlink, Err(ToolOutputProcessingErrorV1::InvalidFile)));
        assert!(!hardlink_root.exists());

        let canary_root = root.path().join("canary-output");
        std::fs::create_dir(&canary_root).expect("canary root");
        let leaked = b"prefix synthetic-secret suffix";
        std::fs::write(canary_root.join("result.txt"), leaked).expect("canary file");
        let canary = verify_and_import_stdio_attempt_outputs_v1(
            attempt_id,
            &canary_root,
            &authorization,
            &[declaration("result", "result.txt", leaked, 2)],
            Some(b"synthetic-secret"),
            fake_import(attempt_id),
        );
        assert!(matches!(canary, Err(ToolOutputProcessingErrorV1::CredentialCanary)));
        assert!(!canary_root.exists());

        let limit_root = root.path().join("limit-output");
        std::fs::create_dir(&limit_root).expect("limit root");
        let mut oversized = declaration("result", "missing.txt", b"", 2);
        oversized.payload.byte_length = authorization.max_single_artifact_bytes + 1;
        let limited = verify_and_import_stdio_attempt_outputs_v1(
            attempt_id,
            &limit_root,
            &authorization,
            &[oversized],
            None,
            fake_import(attempt_id),
        );
        assert!(matches!(limited, Err(ToolOutputProcessingErrorV1::LimitExceeded)));
        assert!(!limit_root.exists());
    }

    fn fake_import(
        attempt_id: ToolExecutionAttemptId,
    ) -> impl FnMut(
        &StdioArtifactReadyPayloadV1,
        Vec<u8>,
    ) -> Result<ArtifactRecord, ToolOutputProcessingErrorV1> {
        move |payload, bytes| {
            Ok(ArtifactRecord {
                artifact_id: ArtifactId::new(),
                owner: ArtifactOwnerRef::Tool {
                    owner: ToolArtifactOwnerRef::ExecutionAttempt {
                        tool_execution_attempt_id: attempt_id,
                    },
                },
                scope: ArtifactScope::ToolExecution,
                media_type: payload.media_type.clone(),
                access: ArtifactAccess::MODEL_READ.union(ArtifactAccess::USER_READ),
                byte_length: u64::try_from(bytes.len()).expect("byte length"),
                content_hash: ContentHash::blake3(&bytes),
                inline: true,
                created_at: UtcTimestamp::now(),
            })
        }
    }

    fn declaration(
        slot: &str,
        path: &str,
        bytes: &[u8],
        sequence: u64,
    ) -> StdioObservedArtifactReadyV1 {
        StdioObservedArtifactReadyV1 {
            sequence,
            payload: StdioArtifactReadyPayloadV1 {
                slot: ToolOutputSlotKeyV1::new(slot).expect("slot"),
                relative_path: ToolOutputRelativePathV1::new(path).expect("path"),
                media_type: "text/plain; charset=utf-8".to_owned(),
                byte_length: u64::try_from(bytes.len()).expect("length"),
                content_hash: ContentHash::sha256(bytes),
            },
        }
    }
}
