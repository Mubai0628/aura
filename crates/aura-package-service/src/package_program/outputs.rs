use super::media_type::valid_media_type_v1;
use anyhow::{Result, ensure};
use aura_domain_common::{ContentHash, ContentHashAlgorithm};
use aura_tool_contract::{
    MAX_TOOL_ARTIFACT_BYTES, MAX_TOOL_OUTPUT_ARTIFACTS_V1, StdioArtifactReadyPayloadV1,
};
use std::{collections::BTreeMap, io::Read, path::Path};

pub struct VerifiedProgramArtifact {
    pub declaration: StdioArtifactReadyPayloadV1,
    pub bytes: Vec<u8>,
}

/// No path returned by the program is trusted as file authority. Native code
/// can write elsewhere; only no-follow handles under this exact root are imported.
pub(super) fn collect(
    root: &Path,
    declared: bool,
    observed: &[StdioArtifactReadyPayloadV1],
) -> Result<Vec<VerifiedProgramArtifact>> {
    let _root = aura_windows_platform::open_verified_tool_output_root(root)?;
    ensure!(declared || observed.is_empty(), "package.artifact_not_declared");
    let mut seen = BTreeMap::<String, StdioArtifactReadyPayloadV1>::new();
    let mut identities = Vec::new();
    let mut total = 0_u64;
    let mut artifacts = Vec::new();
    for payload in observed {
        if let Some(previous) = seen.get(payload.slot.as_str()) {
            ensure!(previous == payload, "package.artifact_slot_conflict");
            continue;
        }
        ensure!(
            artifacts.len() < MAX_TOOL_OUTPUT_ARTIFACTS_V1 as usize
                && payload.byte_length <= MAX_TOOL_ARTIFACT_BYTES as u64,
            "package.artifact_limit"
        );
        ensure!(valid_media_type_v1(&payload.media_type), "package.artifact_media_type");
        let mut file = aura_windows_platform::open_verified_tool_output_from_root(
            root,
            Path::new(payload.relative_path.as_str()),
        )?;
        ensure!(!identities.contains(&file.info().identity), "package.artifact_alias");
        ensure!(file.file().metadata()?.len() == payload.byte_length, "package.artifact_length");
        total = total
            .checked_add(payload.byte_length)
            .ok_or_else(|| anyhow::anyhow!("package.artifact_limit"))?;
        ensure!(total <= 256 * 1024 * 1024, "package.artifact_limit");
        let mut bytes = Vec::with_capacity(payload.byte_length as usize);
        file.file_mut().take(MAX_TOOL_ARTIFACT_BYTES as u64 + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 == payload.byte_length
                && payload.content_hash.algorithm == ContentHashAlgorithm::Sha256
                && ContentHash::sha256(&bytes) == payload.content_hash,
            "package.artifact_identity"
        );
        identities.push(file.info().identity.clone());
        seen.insert(payload.slot.as_str().to_owned(), payload.clone());
        artifacts.push(VerifiedProgramArtifact { declaration: payload.clone(), bytes });
    }
    Ok(artifacts)
}
