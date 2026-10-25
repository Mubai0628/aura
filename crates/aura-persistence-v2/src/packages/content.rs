use aura_domain_common::jcs::canonical_json_jcs;
use aura_domain_common::{ArtifactId, UtcTimestamp};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, artifact, record};
use aura_tool_contract::{
    PackageRelativePath, TOOL_PACKAGE_MANIFEST_NAME, ToolPackageFileSha256V1,
};
use aura_tool_domain::{PackageContentDescriptorV1, PackageVersionBindingV1};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use std::str::FromStr;

pub struct PackageFileBytes<'a> {
    pub path: &'a PackageRelativePath,
    pub bytes: &'a [u8],
}

pub struct LoadedPackageContent {
    pub descriptor: PackageContentDescriptorV1,
    /// Manifest is in `descriptor`; only authored payload files are returned here.
    pub files: Vec<(PackageRelativePath, Vec<u8>)>,
}

pub(super) fn validate_files(
    descriptor: &PackageContentDescriptorV1,
    files: &[PackageFileBytes<'_>],
) -> StoreResult<Vec<u8>> {
    descriptor.manifest.validate().map_err(|_| Error::Invalid("package manifest"))?;
    if descriptor.manifest.files.len() != files.len() {
        return Err(Error::Invalid("package closed file set"));
    }
    for (expected, actual) in descriptor.manifest.files.iter().zip(files) {
        if &expected.path != actual.path
            || expected.byte_length != actual.bytes.len() as u64
            || expected.sha256 != ToolPackageFileSha256V1::sha256(actual.bytes)
        {
            return Err(Error::Invalid("package file identity"));
        }
    }
    canonical_json_jcs(&descriptor.manifest)
        .map_err(|_| Error::Invalid("package canonical manifest"))
}

pub(super) fn owner(fingerprint: &str) -> String {
    format!("tool-package-payload:{fingerprint}")
}

pub(super) fn put_file(
    tx: &Transaction<'_>,
    fingerprint: &str,
    path: &str,
    bytes: &[u8],
    at: UtcTimestamp,
) -> StoreResult<()> {
    let id = ArtifactId::new();
    artifact::put(tx, id, &owner(fingerprint), "application/octet-stream", bytes, at)?;
    tx.execute(
        "INSERT INTO tool_package_files VALUES(?1,?2,?3)",
        params![fingerprint, path, id.to_string()],
    )?;
    Ok(())
}

pub(super) fn read_file(db: &Connection, fingerprint: &str, path: &str) -> StoreResult<Vec<u8>> {
    let id: String = db.query_row(
        "SELECT artifact_id FROM tool_package_files WHERE fingerprint=?1 AND path=?2",
        params![fingerprint, path],
        |row| row.get(0),
    )?;
    let id = ArtifactId::from_str(&id).map_err(|_| Error::Invalid("package artifact identity"))?;
    artifact::read(db, id, &owner(fingerprint))
}

/// Reads only content referenced by this exact package version. Artifact hashes,
/// the canonical manifest and every file length/SHA-256 are checked again.
/// Runtime fingerprint eligibility is still derived by the staging verifier,
/// never from these persisted metadata rows alone.
pub fn load_content(
    db: &Connection,
    binding: &PackageVersionBindingV1,
) -> StoreResult<LoadedPackageContent> {
    let fingerprint = binding.package_fingerprint.0.hex();
    let payload: Option<String> = db.query_row(
        "SELECT p.descriptor FROM tool_package_versions v JOIN tool_package_payloads p ON p.fingerprint=v.payload_fingerprint WHERE v.package_id=?1 AND v.version=?2 AND v.fingerprint=?3",
        params![binding.package_id.to_string(),binding.package_version.to_string(),fingerprint], |row| row.get(0),
    ).optional()?;
    let descriptor: PackageContentDescriptorV1 =
        record::decode(&payload.ok_or(Error::Invalid("package.content_unavailable"))?)?;
    if descriptor.package_fingerprint != binding.package_fingerprint
        || descriptor.manifest.package_version != binding.package_version
    {
        return Err(Error::Invalid("package version identity"));
    }
    let mut files = Vec::with_capacity(descriptor.manifest.files.len());
    for file in &descriptor.manifest.files {
        files.push((file.path.clone(), read_file(db, fingerprint, file.path.as_str())?));
    }
    let actual =
        files.iter().map(|(path, bytes)| PackageFileBytes { path, bytes }).collect::<Vec<_>>();
    let canonical = validate_files(&descriptor, &actual)?;
    if canonical != read_file(db, fingerprint, TOOL_PACKAGE_MANIFEST_NAME)? {
        return Err(Error::Invalid("package canonical manifest identity"));
    }
    let count: i64 = db.query_row(
        "SELECT count(*) FROM tool_package_files WHERE fingerprint=?1",
        [fingerprint],
        |row| row.get(0),
    )?;
    if count != (files.len() + 1) as i64 {
        return Err(Error::Invalid("package closed file set"));
    }
    Ok(LoadedPackageContent { descriptor, files })
}
