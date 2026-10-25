use std::{
    collections::BTreeMap,
    fs::OpenOptions,
    io::{self, Cursor, Read, Seek, SeekFrom, Write},
    path::Path,
};

use aura_tool_contract::{
    PackageRelativePath, TOOL_PACKAGE_IMPORT_POLICY_V1, TOOL_PACKAGE_MANIFEST_NAME,
    ToolPackageFileEntryV1, ToolPackageFileSha256V1, ToolPackageImportPolicyV1,
};
use aura_windows_platform::{NativeFileAccess, inspect_existing_path, open_verified_import_source};
use sha2::{Digest, Sha256};
use zip::{CompressionMethod, ZipArchive};

use crate::staging::{
    COPY_BUFFER_BYTES, StagedToolPackageV1, ToolPackageStagingErrorV1, validate_declared_files,
    validate_windows_case_uniqueness, write_canonical_manifest,
};
use crate::{
    DraftPackageContentFileV1, DraftPackageContentV1, ParsedToolPackageManifestV1,
    ToolDraftWorkflowErrorV1, parse_tool_package_manifest_with_policy_v1,
};

const CENTRAL_DIRECTORY_ENTRY_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x01, 0x02];
const CENTRAL_DIRECTORY_FIXED_BYTES: usize = 46;
const ZIP_FILE_TYPE_MASK: u32 = 0o170_000;
const ZIP_REGULAR_FILE: u32 = 0o100_000;
const ZIP_DIRECTORY: u32 = 0o040_000;

#[derive(Clone)]
struct ZipEntryPlanV1 {
    index: usize,
    relative_path: PackageRelativePath,
    byte_length: u64,
    compressed_bytes: u64,
    directory: bool,
    manifest: bool,
}

struct ZipPreflightV1 {
    entries: Vec<ZipEntryPlanV1>,
    source_entry_count: u32,
    expanded_bytes: u64,
}

/// Stages a Package V1 ZIP after validating the complete central-directory
/// inventory. The implementation deliberately never calls `ZipArchive::extract`:
/// every ordinary file is copied to a `create_new` destination only after the
/// archive-wide preflight has succeeded.
pub fn stage_zip_package_v1(
    zip_source: &Path,
    staging_parent: &Path,
) -> Result<StagedToolPackageV1, ToolPackageStagingErrorV1> {
    stage_zip_package_with_policy_v1(zip_source, staging_parent, TOOL_PACKAGE_IMPORT_POLICY_V1)
}

/// Imports an already-authorized ZIP as unvalidated Draft bytes. Archive-wide
/// traversal, entry-kind, case-collision, compression, and quota checks run
/// before any entry is decompressed; Manifest semantics remain a later
/// Validate concern.
pub fn import_zip_draft_content_v1(
    zip_source: &Path,
) -> Result<DraftPackageContentV1, ToolDraftWorkflowErrorV1> {
    let policy = TOOL_PACKAGE_IMPORT_POLICY_V1;
    let source =
        open_verified_import_source(zip_source, NativeFileAccess::Read).map_err(|error| {
            if error.kind() == io::ErrorKind::InvalidInput {
                ToolPackageStagingErrorV1::InvalidZipSource
            } else {
                ToolPackageStagingErrorV1::Io(error)
            }
        })?;
    if source.info().is_directory {
        return Err(ToolPackageStagingErrorV1::InvalidZipSource.into());
    }
    let zip_input_bytes = source.file().metadata().map_err(ToolPackageStagingErrorV1::Io)?.len();
    if zip_input_bytes > policy.max_zip_input_bytes {
        return Err(ToolPackageStagingErrorV1::ZipInputTooLarge.into());
    }

    import_zip_draft_content_from_reader_v1(source.into_file(), zip_input_bytes, policy)
}

/// Imports already-authorized immutable Artifact bytes through the same ZIP
/// preflight as a no-follow filesystem import. The caller remains responsible
/// for proving the Artifact access binding before supplying the bytes.
pub fn import_zip_draft_content_bytes_v1(
    bytes: Vec<u8>,
) -> Result<DraftPackageContentV1, ToolDraftWorkflowErrorV1> {
    let byte_length =
        u64::try_from(bytes.len()).map_err(|_| ToolPackageStagingErrorV1::ZipInputTooLarge)?;
    import_zip_draft_content_from_reader_v1(
        Cursor::new(bytes),
        byte_length,
        TOOL_PACKAGE_IMPORT_POLICY_V1,
    )
}

fn import_zip_draft_content_from_reader_v1<R: Read + Seek>(
    source: R,
    zip_input_bytes: u64,
    policy: ToolPackageImportPolicyV1,
) -> Result<DraftPackageContentV1, ToolDraftWorkflowErrorV1> {
    if zip_input_bytes > policy.max_zip_input_bytes {
        return Err(ToolPackageStagingErrorV1::ZipInputTooLarge.into());
    }
    let archive = ZipArchive::new(source).map_err(ToolPackageStagingErrorV1::InvalidZipArchive)?;
    let central_directory_start = archive.central_directory_start();
    let unique_entry_count = archive.len();
    let mut source_file = archive.into_inner();
    let raw_entry_count = count_central_directory_entries(
        &mut source_file,
        central_directory_start,
        policy.max_entries,
    )?;
    if usize::try_from(raw_entry_count).ok() != Some(unique_entry_count) {
        return Err(ToolPackageStagingErrorV1::CaseCollision.into());
    }
    let mut archive =
        ZipArchive::new(source_file).map_err(ToolPackageStagingErrorV1::InvalidZipArchive)?;
    let preflight = preflight_zip(&mut archive, policy, raw_entry_count)?;
    let manifest = preflight
        .entries
        .iter()
        .find(|entry| entry.manifest)
        .ok_or(ToolPackageStagingErrorV1::ManifestMissing)?;
    let raw_manifest = read_zip_entry(&mut archive, manifest)?;
    let files = preflight
        .entries
        .iter()
        .filter(|entry| !entry.directory && !entry.manifest)
        .map(|plan| {
            Ok(DraftPackageContentFileV1::new(
                plan.relative_path.clone(),
                read_zip_entry(&mut archive, plan)?,
            ))
        })
        .collect::<Result<Vec<_>, ToolPackageStagingErrorV1>>()?;
    DraftPackageContentV1::new(raw_manifest, files)
}

fn read_zip_entry<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    plan: &ZipEntryPlanV1,
) -> Result<Vec<u8>, ToolPackageStagingErrorV1> {
    let input =
        archive.by_index(plan.index).map_err(ToolPackageStagingErrorV1::InvalidZipArchive)?;
    if input.name() != plan.relative_path.as_str()
        || input.size() != plan.byte_length
        || input.compressed_size() != plan.compressed_bytes
        || input.is_dir()
    {
        return Err(ToolPackageStagingErrorV1::InvalidZipArchive(
            zip::result::ZipError::InvalidArchive("ZIP entry metadata changed".into()),
        ));
    }
    let capacity = usize::try_from(plan.byte_length)
        .map_err(|_| ToolPackageStagingErrorV1::FileSizeExceeded)?;
    let limit =
        plan.byte_length.checked_add(1).ok_or(ToolPackageStagingErrorV1::FileSizeExceeded)?;
    let mut bytes = Vec::with_capacity(capacity);
    input.take(limit).read_to_end(&mut bytes).map_err(|error| {
        ToolPackageStagingErrorV1::InvalidZipArchive(zip::result::ZipError::Io(error))
    })?;
    if u64::try_from(bytes.len()).ok() != Some(plan.byte_length) {
        return Err(ToolPackageStagingErrorV1::FileLengthMismatch);
    }
    Ok(bytes)
}

fn stage_zip_package_with_policy_v1(
    zip_source: &Path,
    staging_parent: &Path,
    policy: ToolPackageImportPolicyV1,
) -> Result<StagedToolPackageV1, ToolPackageStagingErrorV1> {
    let staging_parent_info =
        inspect_existing_path(staging_parent, false).map_err(ToolPackageStagingErrorV1::Io)?;
    if !staging_parent_info.is_directory || staging_parent_info.is_reparse_point {
        return Err(ToolPackageStagingErrorV1::InvalidStagingParent);
    }

    let source =
        open_verified_import_source(zip_source, NativeFileAccess::Read).map_err(|error| {
            if error.kind() == io::ErrorKind::InvalidInput {
                ToolPackageStagingErrorV1::InvalidZipSource
            } else {
                ToolPackageStagingErrorV1::Io(error)
            }
        })?;
    if source.info().is_directory {
        return Err(ToolPackageStagingErrorV1::InvalidZipSource);
    }
    let zip_input_bytes = source.file().metadata().map_err(ToolPackageStagingErrorV1::Io)?.len();
    if zip_input_bytes > policy.max_zip_input_bytes {
        return Err(ToolPackageStagingErrorV1::ZipInputTooLarge);
    }

    let archive = ZipArchive::new(source.into_file())
        .map_err(ToolPackageStagingErrorV1::InvalidZipArchive)?;
    let central_directory_start = archive.central_directory_start();
    let unique_entry_count = archive.len();
    let mut source_file = archive.into_inner();
    let raw_entry_count = count_central_directory_entries(
        &mut source_file,
        central_directory_start,
        policy.max_entries,
    )?;
    if usize::try_from(raw_entry_count).ok() != Some(unique_entry_count) {
        return Err(ToolPackageStagingErrorV1::CaseCollision);
    }
    let mut archive =
        ZipArchive::new(source_file).map_err(ToolPackageStagingErrorV1::InvalidZipArchive)?;
    let preflight = preflight_zip(&mut archive, policy, raw_entry_count)?;
    let parsed_manifest = parse_zip_manifest(&mut archive, &preflight, policy)?;
    let declared_files = &parsed_manifest.canonical_manifest().files;
    validate_declared_files(declared_files, policy)?;
    validate_zip_closed_file_set(&preflight.entries, declared_files)?;

    let directory = tempfile::Builder::new()
        .prefix("aura-tool-package-")
        .tempdir_in(staging_parent)
        .map_err(ToolPackageStagingErrorV1::Io)?;
    let declared_by_path =
        declared_files.iter().map(|entry| (entry.path.as_str(), entry)).collect::<BTreeMap<_, _>>();
    for plan in preflight.entries.iter().filter(|entry| !entry.directory && !entry.manifest) {
        let declared = declared_by_path
            .get(plan.relative_path.as_str())
            .copied()
            .ok_or(ToolPackageStagingErrorV1::ClosedFileSetMismatch)?;
        copy_zip_entry(&mut archive, plan, directory.path(), declared)?;
    }
    write_canonical_manifest(directory.path(), parsed_manifest.canonical_manifest_bytes())?;

    StagedToolPackageV1::from_runtime_root(
        directory,
        parsed_manifest,
        preflight.source_entry_count,
        preflight.expanded_bytes,
        policy,
    )
}

fn count_central_directory_entries<R: Read + Seek>(
    source: &mut R,
    central_directory_start: u64,
    max_entries: u32,
) -> Result<u32, ToolPackageStagingErrorV1> {
    source.seek(SeekFrom::Start(central_directory_start)).map_err(ToolPackageStagingErrorV1::Io)?;
    let mut count = 0_u32;
    loop {
        let mut signature = [0_u8; 4];
        source.read_exact(&mut signature).map_err(|error| {
            ToolPackageStagingErrorV1::InvalidZipArchive(zip::result::ZipError::Io(error))
        })?;
        if signature != CENTRAL_DIRECTORY_ENTRY_SIGNATURE {
            return Ok(count);
        }
        count = count.checked_add(1).ok_or(ToolPackageStagingErrorV1::EntryCountExceeded)?;
        if count > max_entries {
            return Err(ToolPackageStagingErrorV1::EntryCountExceeded);
        }

        let mut fixed = [0_u8; CENTRAL_DIRECTORY_FIXED_BYTES - 4];
        source.read_exact(&mut fixed).map_err(|error| {
            ToolPackageStagingErrorV1::InvalidZipArchive(zip::result::ZipError::Io(error))
        })?;
        let variable_bytes = u64::from(u16::from_le_bytes([fixed[24], fixed[25]]))
            + u64::from(u16::from_le_bytes([fixed[26], fixed[27]]))
            + u64::from(u16::from_le_bytes([fixed[28], fixed[29]]));
        source
            .seek(SeekFrom::Current(i64::try_from(variable_bytes).map_err(|_| {
                ToolPackageStagingErrorV1::InvalidZipArchive(zip::result::ZipError::InvalidArchive(
                    "central directory is too large".into(),
                ))
            })?))
            .map_err(ToolPackageStagingErrorV1::Io)?;
    }
}

fn preflight_zip<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    policy: ToolPackageImportPolicyV1,
    source_entry_count: u32,
) -> Result<ZipPreflightV1, ToolPackageStagingErrorV1> {
    let mut entries = Vec::with_capacity(archive.len());
    let mut all_paths = Vec::with_capacity(archive.len());
    let mut expanded_bytes = 0_u64;
    let mut compressed_bytes = 0_u64;
    let mut manifest_count = 0_u32;

    for index in 0..archive.len() {
        let entry =
            archive.by_index_raw(index).map_err(ToolPackageStagingErrorV1::InvalidZipArchive)?;
        if entry.encrypted() {
            return Err(ToolPackageStagingErrorV1::EncryptedZipEntryDenied);
        }
        if !matches!(entry.compression(), CompressionMethod::Stored | CompressionMethod::Deflated) {
            return Err(ToolPackageStagingErrorV1::ZipCompressionMethodDenied);
        }

        let directory = entry.is_dir();
        validate_zip_entry_kind(directory, entry.is_symlink(), entry.unix_mode(), entry.size())?;
        let relative_path = canonical_zip_path(entry.name(), directory)?;
        let path_depth =
            u32::try_from(relative_path.as_str().split('/').count()).unwrap_or(u32::MAX);
        if path_depth > policy.max_relative_depth {
            return Err(ToolPackageStagingErrorV1::PathDepthExceeded);
        }
        all_paths.push(relative_path.as_str().to_owned());

        let manifest = !directory && relative_path.as_str() == TOOL_PACKAGE_MANIFEST_NAME;
        if manifest {
            manifest_count =
                manifest_count.checked_add(1).ok_or(ToolPackageStagingErrorV1::ManifestMissing)?;
            if entry.size() > policy.max_manifest_bytes {
                return Err(ToolPackageStagingErrorV1::ManifestTooLarge);
            }
        }
        if !directory && entry.size() > policy.max_file_bytes {
            return Err(ToolPackageStagingErrorV1::FileSizeExceeded);
        }
        validate_single_entry_ratio(entry.size(), entry.compressed_size(), policy)?;
        expanded_bytes = expanded_bytes
            .checked_add(entry.size())
            .ok_or(ToolPackageStagingErrorV1::ExpandedSizeExceeded)?;
        if expanded_bytes > policy.max_expanded_bytes {
            return Err(ToolPackageStagingErrorV1::ExpandedSizeExceeded);
        }
        compressed_bytes = compressed_bytes
            .checked_add(entry.compressed_size())
            .ok_or(ToolPackageStagingErrorV1::ZipCompressionRatioExceeded)?;
        entries.push(ZipEntryPlanV1 {
            index,
            relative_path,
            byte_length: entry.size(),
            compressed_bytes: entry.compressed_size(),
            directory,
            manifest,
        });
    }

    if manifest_count == 0 {
        return Err(ToolPackageStagingErrorV1::ManifestMissing);
    }
    validate_windows_case_uniqueness(&all_paths)?;
    validate_aggregate_ratio(expanded_bytes, compressed_bytes, policy)?;
    Ok(ZipPreflightV1 { entries, source_entry_count, expanded_bytes })
}

fn parse_zip_manifest<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    preflight: &ZipPreflightV1,
    policy: ToolPackageImportPolicyV1,
) -> Result<ParsedToolPackageManifestV1, ToolPackageStagingErrorV1> {
    let plan = preflight
        .entries
        .iter()
        .find(|entry| entry.manifest)
        .ok_or(ToolPackageStagingErrorV1::ManifestMissing)?;
    let mut input =
        archive.by_index(plan.index).map_err(ToolPackageStagingErrorV1::InvalidZipArchive)?;
    if input.name() != TOOL_PACKAGE_MANIFEST_NAME
        || input.size() != plan.byte_length
        || input.compressed_size() != plan.compressed_bytes
    {
        return Err(ToolPackageStagingErrorV1::InvalidZipArchive(
            zip::result::ZipError::InvalidArchive("ZIP Manifest metadata changed".into()),
        ));
    }
    let parsed = parse_tool_package_manifest_with_policy_v1(&mut input, policy)
        .map_err(ToolPackageStagingErrorV1::ManifestInvalid)?;
    if parsed.source_byte_length() != plan.byte_length {
        return Err(ToolPackageStagingErrorV1::FileLengthMismatch);
    }
    Ok(parsed)
}

fn canonical_zip_path(
    name: &str,
    directory: bool,
) -> Result<PackageRelativePath, ToolPackageStagingErrorV1> {
    let logical = if directory {
        name.strip_suffix('/').ok_or(ToolPackageStagingErrorV1::InvalidPath)?
    } else {
        name
    };
    let relative =
        PackageRelativePath::new(logical).map_err(|_| ToolPackageStagingErrorV1::InvalidPath)?;
    let expected =
        if directory { format!("{}/", relative.as_str()) } else { relative.as_str().to_owned() };
    if expected != name {
        return Err(ToolPackageStagingErrorV1::InvalidPath);
    }
    Ok(relative)
}

fn validate_zip_entry_kind(
    directory: bool,
    symlink: bool,
    unix_mode: Option<u32>,
    byte_length: u64,
) -> Result<(), ToolPackageStagingErrorV1> {
    if symlink || (directory && byte_length != 0) {
        return Err(ToolPackageStagingErrorV1::SpecialFileDenied);
    }
    if let Some(mode) = unix_mode {
        let file_type = mode & ZIP_FILE_TYPE_MASK;
        let expected = if directory { ZIP_DIRECTORY } else { ZIP_REGULAR_FILE };
        if file_type != 0 && file_type != expected {
            return Err(ToolPackageStagingErrorV1::SpecialFileDenied);
        }
    }
    Ok(())
}

fn validate_single_entry_ratio(
    expanded: u64,
    compressed: u64,
    policy: ToolPackageImportPolicyV1,
) -> Result<(), ToolPackageStagingErrorV1> {
    if expanded == 0 {
        return Ok(());
    }
    if compressed == 0
        || u128::from(expanded)
            > u128::from(compressed) * u128::from(policy.max_single_entry_compression_ratio)
    {
        return Err(ToolPackageStagingErrorV1::ZipCompressionRatioExceeded);
    }
    Ok(())
}

fn validate_aggregate_ratio(
    expanded: u64,
    compressed: u64,
    policy: ToolPackageImportPolicyV1,
) -> Result<(), ToolPackageStagingErrorV1> {
    if expanded != 0
        && (compressed == 0
            || u128::from(expanded)
                > u128::from(compressed) * u128::from(policy.max_aggregate_compression_ratio))
    {
        return Err(ToolPackageStagingErrorV1::ZipCompressionRatioExceeded);
    }
    Ok(())
}

fn validate_zip_closed_file_set(
    entries: &[ZipEntryPlanV1],
    declared_files: &[ToolPackageFileEntryV1],
) -> Result<(), ToolPackageStagingErrorV1> {
    let actual =
        entries.iter().filter(|entry| !entry.directory && !entry.manifest).collect::<Vec<_>>();
    if actual.len() != declared_files.len() {
        return Err(ToolPackageStagingErrorV1::ClosedFileSetMismatch);
    }
    for declared in declared_files {
        let Some(actual) = actual.iter().find(|entry| entry.relative_path == declared.path) else {
            return Err(ToolPackageStagingErrorV1::ClosedFileSetMismatch);
        };
        if actual.byte_length != declared.byte_length {
            return Err(ToolPackageStagingErrorV1::FileLengthMismatch);
        }
    }
    Ok(())
}

fn copy_zip_entry<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    plan: &ZipEntryPlanV1,
    staging_root: &Path,
    declared: &ToolPackageFileEntryV1,
) -> Result<(), ToolPackageStagingErrorV1> {
    let mut input =
        archive.by_index(plan.index).map_err(ToolPackageStagingErrorV1::InvalidZipArchive)?;
    if input.name() != plan.relative_path.as_str()
        || input.size() != plan.byte_length
        || input.compressed_size() != plan.compressed_bytes
    {
        return Err(ToolPackageStagingErrorV1::InvalidZipArchive(
            zip::result::ZipError::InvalidArchive("ZIP entry metadata changed".into()),
        ));
    }

    let destination = staging_root.join(plan.relative_path.as_str().replace('/', "\\"));
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(ToolPackageStagingErrorV1::Io)?;
    }
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(destination)
        .map_err(ToolPackageStagingErrorV1::Io)?;
    let mut hasher = Sha256::new();
    let mut copied = 0_u64;
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    loop {
        let read = input.read(&mut buffer).map_err(|error| {
            ToolPackageStagingErrorV1::InvalidZipArchive(zip::result::ZipError::Io(error))
        })?;
        if read == 0 {
            break;
        }
        copied = copied
            .checked_add(
                u64::try_from(read).map_err(|_| ToolPackageStagingErrorV1::ExpandedSizeExceeded)?,
            )
            .ok_or(ToolPackageStagingErrorV1::ExpandedSizeExceeded)?;
        if copied > plan.byte_length {
            return Err(ToolPackageStagingErrorV1::FileLengthMismatch);
        }
        hasher.update(&buffer[..read]);
        output.write_all(&buffer[..read]).map_err(ToolPackageStagingErrorV1::Io)?;
    }
    output.flush().map_err(ToolPackageStagingErrorV1::Io)?;
    output.sync_all().map_err(ToolPackageStagingErrorV1::Io)?;
    if copied != plan.byte_length {
        return Err(ToolPackageStagingErrorV1::FileLengthMismatch);
    }
    let content_hash = ToolPackageFileSha256V1::new(format!("{:x}", hasher.finalize()))
        .map_err(|_| ToolPackageStagingErrorV1::FileHashMismatch)?;
    if declared.sha256 != content_hash {
        return Err(ToolPackageStagingErrorV1::FileHashMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File};

    use aura_tool_contract::TOOL_PACKAGE_IMPORT_POLICY_V1;
    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::*;

    const STDIO_MANIFEST: &[u8] =
        include_bytes!("../../../fixtures/tool-package/manifest-v1/valid/aura-stdio.json");
    const STDIO_RUNTIME: &[u8] = b"stdio fixture runtime";

    fn write_zip(
        destination: &Path,
        entries: &[(&str, &[u8], CompressionMethod)],
        directories: &[&str],
    ) {
        let output = File::create(destination).expect("create ZIP fixture");
        let mut writer = ZipWriter::new(output);
        for directory in directories {
            writer.add_directory(*directory, SimpleFileOptions::default()).expect("add directory");
        }
        for (name, bytes, compression) in entries {
            writer
                .start_file(*name, SimpleFileOptions::default().compression_method(*compression))
                .expect("start ZIP file");
            writer.write_all(bytes).expect("write ZIP file");
        }
        writer.finish().expect("finish ZIP fixture");
    }

    fn patch_first_signature_field(
        path: &Path,
        signature: [u8; 4],
        field_offset: usize,
        value: [u8; 2],
    ) {
        let mut bytes = fs::read(path).expect("read ZIP fixture");
        let index = bytes
            .windows(signature.len())
            .position(|window| window == signature)
            .expect("ZIP signature");
        bytes[index + field_offset..index + field_offset + value.len()].copy_from_slice(&value);
        fs::write(path, bytes).expect("patch ZIP fixture");
    }

    fn patch_nth_central_name(path: &Path, ordinal: usize, replacement: &[u8]) {
        let mut bytes = fs::read(path).expect("read ZIP fixture");
        let mut starts =
            bytes.windows(CENTRAL_DIRECTORY_ENTRY_SIGNATURE.len()).enumerate().filter_map(
                |(index, window)| (window == CENTRAL_DIRECTORY_ENTRY_SIGNATURE).then_some(index),
            );
        let start = starts.nth(ordinal).expect("central entry");
        let name_length = usize::from(u16::from_le_bytes([bytes[start + 28], bytes[start + 29]]));
        assert_eq!(name_length, replacement.len());
        bytes[start + CENTRAL_DIRECTORY_FIXED_BYTES
            ..start + CENTRAL_DIRECTORY_FIXED_BYTES + name_length]
            .copy_from_slice(replacement);
        fs::write(path, bytes).expect("patch central name");
    }

    #[test]
    fn zip_draft_import_preserves_invalid_manifest_after_safe_preflight() {
        let source = tempfile::tempdir().expect("source");
        let zip_path = source.path().join("draft.zip");
        write_zip(
            &zip_path,
            &[
                (TOOL_PACKAGE_MANIFEST_NAME, b"not-json", CompressionMethod::Stored),
                ("bin/fixture.exe", STDIO_RUNTIME, CompressionMethod::Deflated),
            ],
            &[],
        );

        let content = import_zip_draft_content_v1(&zip_path).expect("draft ZIP import");
        assert_eq!(content.raw_manifest(), b"not-json");
        assert_eq!(content.files().len(), 1);
        assert_eq!(content.files()[0].path().as_str(), "bin/fixture.exe");
        assert_eq!(content.files()[0].bytes(), STDIO_RUNTIME);
    }

    #[test]
    fn zip_import_preflights_then_stages_only_declared_files() {
        let source = tempfile::tempdir().expect("source");
        let staging = tempfile::tempdir().expect("staging");
        let zip_path = source.path().join("package.zip");
        write_zip(
            &zip_path,
            &[
                (TOOL_PACKAGE_MANIFEST_NAME, STDIO_MANIFEST, CompressionMethod::Stored),
                ("bin/fixture.exe", STDIO_RUNTIME, CompressionMethod::Deflated),
            ],
            &["unused/"],
        );

        let staged = stage_zip_package_v1(&zip_path, staging.path()).expect("stage ZIP");
        assert_eq!(staged.source_entry_count(), 3);
        assert_eq!(
            staged.source_expanded_bytes(),
            u64::try_from(STDIO_MANIFEST.len() + STDIO_RUNTIME.len()).expect("source bytes")
        );
        assert!(staged.root().join("bin/fixture.exe").is_file());
        assert!(!staged.root().join("unused").exists());
        assert_eq!(
            staged.source_manifest_sha256(),
            &aura_domain_common::ContentHash::sha256(STDIO_MANIFEST)
        );
        assert_eq!(
            fs::read(staged.root().join(TOOL_PACKAGE_MANIFEST_NAME)).expect("runtime Manifest"),
            staged.canonical_manifest_bytes()
        );
        staged.verify_integrity().expect("ZIP staging integrity");

        let directory_source = tempfile::tempdir().expect("directory source");
        fs::create_dir_all(directory_source.path().join("bin")).expect("runtime directory");
        fs::write(directory_source.path().join(TOOL_PACKAGE_MANIFEST_NAME), STDIO_MANIFEST)
            .expect("directory Manifest");
        fs::write(directory_source.path().join("bin/fixture.exe"), STDIO_RUNTIME)
            .expect("directory runtime");
        let directory_staging = tempfile::tempdir().expect("directory staging");
        let directory =
            crate::stage_directory_package_v1(directory_source.path(), directory_staging.path())
                .expect("stage directory package");
        assert_eq!(staged.runtime_closure(), directory.runtime_closure());
        assert_eq!(
            staged.fingerprint_evidence().expect("ZIP fingerprint evidence"),
            directory.fingerprint_evidence().expect("directory fingerprint evidence")
        );
    }

    #[test]
    fn zip_import_rejects_path_aliases_traversal_and_symlink() {
        let source = tempfile::tempdir().expect("source");
        let staging = tempfile::tempdir().expect("staging");
        let zip_path = source.path().join("case.zip");
        write_zip(
            &zip_path,
            &[
                (TOOL_PACKAGE_MANIFEST_NAME, b"{}", CompressionMethod::Stored),
                ("Tool.exe", b"one", CompressionMethod::Stored),
                ("tool.EXE", b"two", CompressionMethod::Stored),
            ],
            &[],
        );
        assert!(matches!(
            stage_zip_package_v1(&zip_path, staging.path()),
            Err(ToolPackageStagingErrorV1::CaseCollision)
        ));

        let traversal_path = source.path().join("traversal.zip");
        write_zip(
            &traversal_path,
            &[
                (TOOL_PACKAGE_MANIFEST_NAME, b"{}", CompressionMethod::Stored),
                ("../escape", b"no", CompressionMethod::Stored),
            ],
            &[],
        );
        assert!(matches!(
            stage_zip_package_v1(&traversal_path, staging.path()),
            Err(ToolPackageStagingErrorV1::InvalidPath)
        ));

        let symlink_path = source.path().join("symlink.zip");
        let output = File::create(&symlink_path).expect("symlink ZIP");
        let mut writer = ZipWriter::new(output);
        writer
            .start_file(TOOL_PACKAGE_MANIFEST_NAME, SimpleFileOptions::default())
            .expect("manifest");
        writer.write_all(b"{}").expect("manifest bytes");
        writer
            .add_symlink("runtime-link", "target", SimpleFileOptions::default())
            .expect("symlink");
        writer.finish().expect("finish symlink ZIP");
        assert!(matches!(
            stage_zip_package_v1(&symlink_path, staging.path()),
            Err(ToolPackageStagingErrorV1::SpecialFileDenied)
        ));
    }

    #[test]
    fn zip_import_enforces_input_entry_expansion_and_ratio_limits() {
        let source = tempfile::tempdir().expect("source");
        let staging = tempfile::tempdir().expect("staging");
        let zip_path = source.path().join("limits.zip");
        let repeated = vec![b'a'; 4096];
        write_zip(
            &zip_path,
            &[
                (TOOL_PACKAGE_MANIFEST_NAME, b"{}", CompressionMethod::Stored),
                ("data.bin", &repeated, CompressionMethod::Deflated),
            ],
            &[],
        );
        let mut policy = TOOL_PACKAGE_IMPORT_POLICY_V1;
        policy.max_zip_input_bytes = 1;
        assert!(matches!(
            stage_zip_package_with_policy_v1(&zip_path, staging.path(), policy),
            Err(ToolPackageStagingErrorV1::ZipInputTooLarge)
        ));

        let mut policy = TOOL_PACKAGE_IMPORT_POLICY_V1;
        policy.max_entries = 1;
        assert!(matches!(
            stage_zip_package_with_policy_v1(&zip_path, staging.path(), policy),
            Err(ToolPackageStagingErrorV1::EntryCountExceeded)
        ));

        let mut policy = TOOL_PACKAGE_IMPORT_POLICY_V1;
        policy.max_expanded_bytes = 4096;
        policy.max_single_entry_compression_ratio = 1000;
        policy.max_aggregate_compression_ratio = 1000;
        assert!(matches!(
            stage_zip_package_with_policy_v1(&zip_path, staging.path(), policy),
            Err(ToolPackageStagingErrorV1::ExpandedSizeExceeded)
        ));

        let mut policy = TOOL_PACKAGE_IMPORT_POLICY_V1;
        policy.max_single_entry_compression_ratio = 1;
        assert!(matches!(
            stage_zip_package_with_policy_v1(&zip_path, staging.path(), policy),
            Err(ToolPackageStagingErrorV1::ZipCompressionRatioExceeded)
        ));

        let mut policy = TOOL_PACKAGE_IMPORT_POLICY_V1;
        policy.max_single_entry_compression_ratio = 1000;
        policy.max_aggregate_compression_ratio = 1;
        assert!(matches!(
            stage_zip_package_with_policy_v1(&zip_path, staging.path(), policy),
            Err(ToolPackageStagingErrorV1::ZipCompressionRatioExceeded)
        ));
    }

    #[test]
    fn zip_import_rejects_encryption_and_non_v1_compression() {
        let source = tempfile::tempdir().expect("source");
        let staging = tempfile::tempdir().expect("staging");
        let encrypted = source.path().join("encrypted.zip");
        write_zip(
            &encrypted,
            &[(TOOL_PACKAGE_MANIFEST_NAME, b"{}", CompressionMethod::Stored)],
            &[],
        );
        patch_first_signature_field(&encrypted, CENTRAL_DIRECTORY_ENTRY_SIGNATURE, 8, [1, 0]);
        assert!(matches!(
            stage_zip_package_v1(&encrypted, staging.path()),
            Err(ToolPackageStagingErrorV1::EncryptedZipEntryDenied)
        ));

        let unsupported = source.path().join("unsupported.zip");
        write_zip(
            &unsupported,
            &[(TOOL_PACKAGE_MANIFEST_NAME, b"{}", CompressionMethod::Stored)],
            &[],
        );
        patch_first_signature_field(
            &unsupported,
            CENTRAL_DIRECTORY_ENTRY_SIGNATURE,
            10,
            12_u16.to_le_bytes(),
        );
        assert!(matches!(
            stage_zip_package_v1(&unsupported, staging.path()),
            Err(ToolPackageStagingErrorV1::ZipCompressionMethodDenied)
        ));
    }

    #[test]
    fn zip_import_detects_exact_duplicate_names_before_index_collapse() {
        let source = tempfile::tempdir().expect("source");
        let staging = tempfile::tempdir().expect("staging");
        let zip_path = source.path().join("duplicate.zip");
        write_zip(
            &zip_path,
            &[
                (TOOL_PACKAGE_MANIFEST_NAME, b"{}", CompressionMethod::Stored),
                ("a.bin", b"a", CompressionMethod::Stored),
                ("b.bin", b"b", CompressionMethod::Stored),
            ],
            &[],
        );
        patch_nth_central_name(&zip_path, 2, b"a.bin");
        assert!(matches!(
            stage_zip_package_v1(&zip_path, staging.path()),
            Err(ToolPackageStagingErrorV1::CaseCollision)
        ));
    }

    #[test]
    fn zip_import_rejects_unlisted_and_hash_mismatched_files() {
        let source = tempfile::tempdir().expect("source");
        let staging = tempfile::tempdir().expect("staging");
        let extra_path = source.path().join("extra.zip");
        write_zip(
            &extra_path,
            &[
                (TOOL_PACKAGE_MANIFEST_NAME, STDIO_MANIFEST, CompressionMethod::Stored),
                ("bin/fixture.exe", STDIO_RUNTIME, CompressionMethod::Stored),
                ("extra.bin", b"undeclared", CompressionMethod::Stored),
            ],
            &[],
        );
        assert!(matches!(
            stage_zip_package_v1(&extra_path, staging.path()),
            Err(ToolPackageStagingErrorV1::ClosedFileSetMismatch)
        ));

        let zip_path = source.path().join("closed.zip");
        write_zip(
            &zip_path,
            &[
                (TOOL_PACKAGE_MANIFEST_NAME, STDIO_MANIFEST, CompressionMethod::Stored),
                ("bin/fixture.exe", b"xxxxxxxxxxxxxxxxxxxxx", CompressionMethod::Stored),
            ],
            &[],
        );
        assert!(matches!(
            stage_zip_package_v1(&zip_path, staging.path()),
            Err(ToolPackageStagingErrorV1::FileHashMismatch)
        ));
        assert_eq!(fs::read_dir(staging.path()).expect("read staging parent").count(), 0);
    }
}
