use aura_domain_common::CanonicalSemVer;
use aura_domain_common::jcs::{
    CanonicalJsonError, CanonicalSha256HashV1, canonical_blake3, canonical_json_jcs,
    canonical_sha256,
};
use aura_tool_contract::{
    PackageFingerprintV1, PackageKey, PackageRuntimeClosureHashV1, PackageSecurityFingerprintV1,
    ToolContractFingerprintV1, ToolKey, ToolPackageErrorCode,
};
use aura_tool_domain::{
    CanonicalToolPackageManifestV1, CanonicalToolPackageToolV1, ToolPackageManifestContractError,
};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::Serialize;
use thiserror::Error;

use crate::staging::PackageRuntimeClosureV1;

const TOOL_CONTRACT_FINGERPRINT_KIND_V1: &str = "aura.p07.tool_contract_fingerprint";
const PACKAGE_FINGERPRINT_KIND_V1: &str = "aura.p07.package_fingerprint";
const PACKAGE_SECURITY_FINGERPRINT_KIND_V1: &str = "aura.p07.package_security_fingerprint";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FingerprintPreimageVersionV1;

impl Serialize for FingerprintPreimageVersionV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u32(1)
    }
}

impl JsonSchema for FingerprintPreimageVersionV1 {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("FingerprintPreimageVersionV1")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "integer",
            "const": 1
        })
    }
}

macro_rules! fingerprint_kind {
    ($name:ident, $schema_name:literal, $wire:expr) => {
        #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
        pub struct $name;

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                serializer.serialize_str($wire)
            }
        }

        impl JsonSchema for $name {
            fn schema_name() -> std::borrow::Cow<'static, str> {
                std::borrow::Cow::Borrowed($schema_name)
            }

            fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
                json_schema!({
                    "type": "string",
                    "const": $wire
                })
            }
        }
    };
}

fingerprint_kind!(
    ToolContractFingerprintKindV1,
    "ToolContractFingerprintKindV1",
    TOOL_CONTRACT_FINGERPRINT_KIND_V1
);
fingerprint_kind!(
    PackageFingerprintKindV1,
    "PackageFingerprintKindV1",
    PACKAGE_FINGERPRINT_KIND_V1
);
fingerprint_kind!(
    PackageSecurityFingerprintKindV1,
    "PackageSecurityFingerprintKindV1",
    PACKAGE_SECURITY_FINGERPRINT_KIND_V1
);

/// Exact V1 preimage for one Package-authored Tool contract. Fields are
/// private and the type cannot be deserialized independently of authoritative
/// staging evidence.
#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolContractFingerprintPreimageV1 {
    kind: ToolContractFingerprintKindV1,
    preimage_version: FingerprintPreimageVersionV1,
    package_key: PackageKey,
    tool_key: ToolKey,
    tool_contract: CanonicalToolPackageToolV1,
    runtime_closure_hash: PackageRuntimeClosureHashV1,
}

impl ToolContractFingerprintPreimageV1 {
    fn from_manifest_tool(
        manifest: &CanonicalToolPackageManifestV1,
        tool_contract: &CanonicalToolPackageToolV1,
        runtime_closure_hash: &PackageRuntimeClosureHashV1,
    ) -> Result<Self, ToolPackageFingerprintErrorV1> {
        let tool_key = tool_contract.tool_key.clone();
        let manifest_tool = manifest
            .tools
            .iter()
            .find(|candidate| candidate.tool_key == tool_key)
            .ok_or(ToolPackageFingerprintErrorV1::ToolContractMismatch)?;
        if manifest_tool != tool_contract || tool_contract.tool_key != tool_key {
            return Err(ToolPackageFingerprintErrorV1::ToolContractMismatch);
        }
        Ok(Self {
            kind: ToolContractFingerprintKindV1,
            preimage_version: FingerprintPreimageVersionV1,
            package_key: manifest.package_key.clone(),
            tool_key,
            tool_contract: tool_contract.clone(),
            runtime_closure_hash: runtime_closure_hash.clone(),
        })
    }

    #[must_use]
    pub const fn package_key(&self) -> &PackageKey {
        &self.package_key
    }

    #[must_use]
    pub const fn tool_key(&self) -> &ToolKey {
        &self.tool_key
    }

    #[must_use]
    pub const fn tool_contract(&self) -> &CanonicalToolPackageToolV1 {
        &self.tool_contract
    }

    #[must_use]
    pub const fn runtime_closure_hash(&self) -> &PackageRuntimeClosureHashV1 {
        &self.runtime_closure_hash
    }
}

#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolContractFingerprintEntryV1 {
    tool_key: ToolKey,
    fingerprint: ToolContractFingerprintV1,
}

impl ToolContractFingerprintEntryV1 {
    #[must_use]
    pub const fn tool_key(&self) -> &ToolKey {
        &self.tool_key
    }

    #[must_use]
    pub const fn fingerprint(&self) -> &ToolContractFingerprintV1 {
        &self.fingerprint
    }
}

#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageFingerprintPreimageV1 {
    kind: PackageFingerprintKindV1,
    preimage_version: FingerprintPreimageVersionV1,
    package_key: PackageKey,
    package_version: CanonicalSemVer,
    canonical_manifest: CanonicalToolPackageManifestV1,
    runtime_closure: PackageRuntimeClosureV1,
    #[schemars(length(min = 1))]
    tool_contract_fingerprints: Vec<ToolContractFingerprintEntryV1>,
}

impl PackageFingerprintPreimageV1 {
    #[must_use]
    pub const fn package_key(&self) -> &PackageKey {
        &self.package_key
    }

    #[must_use]
    pub const fn package_version(&self) -> &CanonicalSemVer {
        &self.package_version
    }

    #[must_use]
    pub const fn canonical_manifest(&self) -> &CanonicalToolPackageManifestV1 {
        &self.canonical_manifest
    }

    #[must_use]
    pub const fn runtime_closure(&self) -> &PackageRuntimeClosureV1 {
        &self.runtime_closure
    }

    #[must_use]
    pub fn tool_contract_fingerprints(&self) -> &[ToolContractFingerprintEntryV1] {
        &self.tool_contract_fingerprints
    }
}

#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageSecurityFingerprintPreimageV1 {
    kind: PackageSecurityFingerprintKindV1,
    preimage_version: FingerprintPreimageVersionV1,
    package_key: PackageKey,
    package_version: CanonicalSemVer,
    canonical_manifest: CanonicalToolPackageManifestV1,
    runtime_closure: PackageRuntimeClosureV1,
    #[schemars(length(min = 1))]
    tool_contract_fingerprints: Vec<ToolContractFingerprintEntryV1>,
}

impl PackageSecurityFingerprintPreimageV1 {
    #[must_use]
    pub const fn package_key(&self) -> &PackageKey {
        &self.package_key
    }

    #[must_use]
    pub const fn package_version(&self) -> &CanonicalSemVer {
        &self.package_version
    }

    #[must_use]
    pub const fn canonical_manifest(&self) -> &CanonicalToolPackageManifestV1 {
        &self.canonical_manifest
    }

    #[must_use]
    pub const fn runtime_closure(&self) -> &PackageRuntimeClosureV1 {
        &self.runtime_closure
    }

    #[must_use]
    pub fn tool_contract_fingerprints(&self) -> &[ToolContractFingerprintEntryV1] {
        &self.tool_contract_fingerprints
    }
}

/// Pure W2 output. It contains no Installed, Trust, Registry, or Runtime
/// identity and can only be constructed from a verified staged package.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolPackageFingerprintEvidenceV1 {
    tool_contract_preimages: Vec<ToolContractFingerprintPreimageV1>,
    tool_contract_fingerprints: Vec<ToolContractFingerprintEntryV1>,
    package_preimage: PackageFingerprintPreimageV1,
    package_fingerprint: PackageFingerprintV1,
    package_security_preimage: PackageSecurityFingerprintPreimageV1,
    package_security_fingerprint: PackageSecurityFingerprintV1,
}

impl ToolPackageFingerprintEvidenceV1 {
    pub(crate) fn from_authoritative_closure(
        canonical_manifest_bytes: &[u8],
        canonical_manifest: &CanonicalToolPackageManifestV1,
        runtime_closure: &PackageRuntimeClosureV1,
    ) -> Result<Self, ToolPackageFingerprintErrorV1> {
        validate_manifest_and_closure(
            canonical_manifest_bytes,
            canonical_manifest,
            runtime_closure,
        )?;
        let runtime_closure_hash = runtime_closure
            .canonical_hash()
            .map_err(|_| ToolPackageFingerprintErrorV1::RuntimeClosureMismatch)?;

        let mut tool_contract_preimages = Vec::with_capacity(canonical_manifest.tools.len());
        let mut tool_contract_fingerprints = Vec::with_capacity(canonical_manifest.tools.len());
        for tool_contract in &canonical_manifest.tools {
            let preimage = ToolContractFingerprintPreimageV1::from_manifest_tool(
                canonical_manifest,
                tool_contract,
                &runtime_closure_hash,
            )?;
            let fingerprint = ToolContractFingerprintV1 {
                package_runtime_closure_hash: runtime_closure_hash.clone(),
                fingerprint: canonical_sha256(&preimage)
                    .map_err(ToolPackageFingerprintErrorV1::Canonicalization)?,
            };
            tool_contract_preimages.push(preimage);
            tool_contract_fingerprints.push(ToolContractFingerprintEntryV1 {
                tool_key: tool_contract.tool_key.clone(),
                fingerprint,
            });
        }

        // This is the only Fingerprint-level canonical sort. Illegal ordering
        // in the embedded Manifest or Closure has already been rejected.
        tool_contract_fingerprints.sort_by(|left, right| {
            left.tool_key.as_str().as_bytes().cmp(right.tool_key.as_str().as_bytes())
        });
        validate_tool_fingerprint_set(canonical_manifest, &tool_contract_fingerprints)?;

        let package_preimage = PackageFingerprintPreimageV1 {
            kind: PackageFingerprintKindV1,
            preimage_version: FingerprintPreimageVersionV1,
            package_key: canonical_manifest.package_key.clone(),
            package_version: canonical_manifest.package_version.clone(),
            canonical_manifest: canonical_manifest.clone(),
            runtime_closure: runtime_closure.clone(),
            tool_contract_fingerprints: tool_contract_fingerprints.clone(),
        };
        let package_fingerprint = PackageFingerprintV1(
            canonical_sha256(&package_preimage)
                .map_err(ToolPackageFingerprintErrorV1::Canonicalization)?,
        );

        let package_security_preimage = PackageSecurityFingerprintPreimageV1 {
            kind: PackageSecurityFingerprintKindV1,
            preimage_version: FingerprintPreimageVersionV1,
            package_key: canonical_manifest.package_key.clone(),
            package_version: canonical_manifest.package_version.clone(),
            canonical_manifest: canonical_manifest.clone(),
            runtime_closure: runtime_closure.clone(),
            tool_contract_fingerprints: tool_contract_fingerprints.clone(),
        };
        let package_security_fingerprint = PackageSecurityFingerprintV1(
            canonical_blake3(&package_security_preimage)
                .map_err(ToolPackageFingerprintErrorV1::Canonicalization)?,
        );

        let evidence = Self {
            tool_contract_preimages,
            tool_contract_fingerprints,
            package_preimage,
            package_fingerprint,
            package_security_preimage,
            package_security_fingerprint,
        };
        evidence.verify(canonical_manifest_bytes, canonical_manifest, runtime_closure)?;
        Ok(evidence)
    }

    fn verify(
        &self,
        canonical_manifest_bytes: &[u8],
        canonical_manifest: &CanonicalToolPackageManifestV1,
        runtime_closure: &PackageRuntimeClosureV1,
    ) -> Result<(), ToolPackageFingerprintErrorV1> {
        validate_manifest_and_closure(
            canonical_manifest_bytes,
            canonical_manifest,
            runtime_closure,
        )?;
        let closure_hash = runtime_closure
            .canonical_hash()
            .map_err(|_| ToolPackageFingerprintErrorV1::RuntimeClosureMismatch)?;
        if self.tool_contract_preimages.len() != canonical_manifest.tools.len()
            || self.tool_contract_fingerprints.len() != canonical_manifest.tools.len()
        {
            return Err(ToolPackageFingerprintErrorV1::ToolSetMismatch);
        }

        validate_tool_fingerprint_set(canonical_manifest, &self.tool_contract_fingerprints)?;
        for ((tool_contract, preimage), entry) in canonical_manifest
            .tools
            .iter()
            .zip(&self.tool_contract_preimages)
            .zip(&self.tool_contract_fingerprints)
        {
            if preimage.package_key != canonical_manifest.package_key
                || preimage.tool_key != tool_contract.tool_key
                || preimage.tool_contract != *tool_contract
                || preimage.runtime_closure_hash != closure_hash
                || entry.tool_key != tool_contract.tool_key
                || entry.fingerprint.package_runtime_closure_hash != closure_hash
                || entry.fingerprint.fingerprint
                    != canonical_sha256(preimage)
                        .map_err(ToolPackageFingerprintErrorV1::Canonicalization)?
            {
                return Err(ToolPackageFingerprintErrorV1::ToolContractMismatch);
            }
        }

        validate_package_preimage(
            &self.package_preimage.package_key,
            &self.package_preimage.package_version,
            &self.package_preimage.canonical_manifest,
            &self.package_preimage.runtime_closure,
            &self.package_preimage.tool_contract_fingerprints,
            &self.tool_contract_fingerprints,
            canonical_manifest_bytes,
            canonical_manifest,
            runtime_closure,
        )?;
        if self.package_fingerprint.0
            != canonical_sha256(&self.package_preimage)
                .map_err(ToolPackageFingerprintErrorV1::Canonicalization)?
        {
            return Err(ToolPackageFingerprintErrorV1::FingerprintMismatch);
        }

        validate_package_preimage(
            &self.package_security_preimage.package_key,
            &self.package_security_preimage.package_version,
            &self.package_security_preimage.canonical_manifest,
            &self.package_security_preimage.runtime_closure,
            &self.package_security_preimage.tool_contract_fingerprints,
            &self.tool_contract_fingerprints,
            canonical_manifest_bytes,
            canonical_manifest,
            runtime_closure,
        )?;
        if self.package_security_fingerprint.0
            != canonical_blake3(&self.package_security_preimage)
                .map_err(ToolPackageFingerprintErrorV1::Canonicalization)?
        {
            return Err(ToolPackageFingerprintErrorV1::FingerprintMismatch);
        }
        Ok(())
    }

    #[must_use]
    pub fn tool_contract_preimages(&self) -> &[ToolContractFingerprintPreimageV1] {
        &self.tool_contract_preimages
    }

    #[must_use]
    pub fn tool_contract_fingerprints(&self) -> &[ToolContractFingerprintEntryV1] {
        &self.tool_contract_fingerprints
    }

    #[must_use]
    pub const fn package_preimage(&self) -> &PackageFingerprintPreimageV1 {
        &self.package_preimage
    }

    #[must_use]
    pub const fn package_fingerprint(&self) -> &PackageFingerprintV1 {
        &self.package_fingerprint
    }

    #[must_use]
    pub const fn package_security_preimage(&self) -> &PackageSecurityFingerprintPreimageV1 {
        &self.package_security_preimage
    }

    #[must_use]
    pub const fn package_security_fingerprint(&self) -> &PackageSecurityFingerprintV1 {
        &self.package_security_fingerprint
    }
}

fn validate_manifest_and_closure(
    canonical_manifest_bytes: &[u8],
    canonical_manifest: &CanonicalToolPackageManifestV1,
    runtime_closure: &PackageRuntimeClosureV1,
) -> Result<(), ToolPackageFingerprintErrorV1> {
    canonical_manifest
        .validate()
        .map_err(ToolPackageFingerprintErrorV1::CanonicalManifestInvalid)?;
    let expected_manifest_bytes = canonical_json_jcs(canonical_manifest)
        .map_err(ToolPackageFingerprintErrorV1::Canonicalization)?;
    if canonical_manifest_bytes != expected_manifest_bytes {
        return Err(ToolPackageFingerprintErrorV1::CanonicalManifestBytesMismatch);
    }
    if runtime_closure.canonical_manifest_hash()
        != &CanonicalSha256HashV1::sha256(canonical_manifest_bytes)
        || runtime_closure.files().len() != canonical_manifest.files.len()
        || !runtime_closure
            .files()
            .windows(2)
            .all(|pair| pair[0].path.as_str().as_bytes() < pair[1].path.as_str().as_bytes())
        || !runtime_closure.files().iter().zip(&canonical_manifest.files).all(
            |(closure_file, manifest_file)| {
                closure_file.path == manifest_file.path
                    && closure_file.role == manifest_file.role
                    && closure_file.byte_length == manifest_file.byte_length
                    && closure_file.raw_file_sha256 == manifest_file.sha256
            },
        )
    {
        return Err(ToolPackageFingerprintErrorV1::RuntimeClosureMismatch);
    }
    Ok(())
}

fn validate_tool_fingerprint_set(
    manifest: &CanonicalToolPackageManifestV1,
    entries: &[ToolContractFingerprintEntryV1],
) -> Result<(), ToolPackageFingerprintErrorV1> {
    if entries.len() != manifest.tools.len()
        || !entries
            .windows(2)
            .all(|pair| pair[0].tool_key.as_str().as_bytes() < pair[1].tool_key.as_str().as_bytes())
        || !entries.iter().zip(&manifest.tools).all(|(entry, tool)| entry.tool_key == tool.tool_key)
    {
        return Err(ToolPackageFingerprintErrorV1::ToolSetMismatch);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_package_preimage(
    preimage_package_key: &PackageKey,
    preimage_package_version: &CanonicalSemVer,
    preimage_manifest: &CanonicalToolPackageManifestV1,
    preimage_closure: &PackageRuntimeClosureV1,
    preimage_entries: &[ToolContractFingerprintEntryV1],
    expected_entries: &[ToolContractFingerprintEntryV1],
    canonical_manifest_bytes: &[u8],
    canonical_manifest: &CanonicalToolPackageManifestV1,
    runtime_closure: &PackageRuntimeClosureV1,
) -> Result<(), ToolPackageFingerprintErrorV1> {
    if preimage_package_key != &canonical_manifest.package_key
        || preimage_package_version != &canonical_manifest.package_version
        || preimage_manifest != canonical_manifest
        || preimage_closure != runtime_closure
    {
        return Err(ToolPackageFingerprintErrorV1::PackageIdentityMismatch);
    }
    validate_manifest_and_closure(canonical_manifest_bytes, preimage_manifest, preimage_closure)?;
    validate_tool_fingerprint_set(preimage_manifest, preimage_entries)?;
    if preimage_entries != expected_entries {
        return Err(ToolPackageFingerprintErrorV1::ToolSetMismatch);
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum ToolPackageFingerprintErrorV1 {
    #[error("canonical package manifest failed self-validation")]
    CanonicalManifestInvalid(#[source] ToolPackageManifestContractError),
    #[error("canonical package manifest bytes are not its RFC 8785 serialization")]
    CanonicalManifestBytesMismatch,
    #[error("package runtime closure does not match the canonical manifest")]
    RuntimeClosureMismatch,
    #[error("tool contract does not match its canonical manifest member")]
    ToolContractMismatch,
    #[error("tool fingerprint entries do not exactly match the canonical tool set")]
    ToolSetMismatch,
    #[error("package fingerprint preimage identity does not match its canonical manifest")]
    PackageIdentityMismatch,
    #[error("fingerprint preimage RFC 8785 serialization failed")]
    Canonicalization(#[source] CanonicalJsonError),
    #[error("fingerprint does not match its exact canonical preimage")]
    FingerprintMismatch,
}

impl ToolPackageFingerprintErrorV1 {
    #[must_use]
    pub const fn public_code(&self) -> ToolPackageErrorCode {
        ToolPackageErrorCode::FingerprintMismatch
    }
}

/// One stored tool's contract fingerprint, recomputed from the canonical manifest
/// and runtime closure hash exactly as staging computes it. V2 package tools use it
/// as their source contract fingerprint without retaining staging evidence.
pub fn tool_contract_fingerprint_v1(
    canonical_manifest: &CanonicalToolPackageManifestV1,
    tool_contract: &CanonicalToolPackageToolV1,
    runtime_closure_hash: &PackageRuntimeClosureHashV1,
) -> Result<ToolContractFingerprintV1, ToolPackageFingerprintErrorV1> {
    let preimage = ToolContractFingerprintPreimageV1::from_manifest_tool(
        canonical_manifest,
        tool_contract,
        runtime_closure_hash,
    )?;
    Ok(ToolContractFingerprintV1 {
        package_runtime_closure_hash: runtime_closure_hash.clone(),
        fingerprint: canonical_sha256(&preimage)
            .map_err(ToolPackageFingerprintErrorV1::Canonicalization)?,
    })
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use aura_domain_common::jcs::{
        CanonicalBlake3HashV1, CanonicalSha256HashV1, canonical_blake3, canonical_json_jcs,
        canonical_sha256, parse_json_strict,
    };
    use aura_tool_contract::{TOOL_PACKAGE_MANIFEST_NAME, ToolPackageFileSha256V1};
    use serde_json::Value;

    use super::*;
    use crate::stage_directory_package_v1;

    const STDIO_MANIFEST: &[u8] =
        include_bytes!("../../../fixtures/tool-package/manifest-v1/valid/aura-stdio.json");
    const STDIO_RUNTIME: &[u8] = b"stdio fixture runtime";

    fn write_valid_source(root: &Path) {
        fs::create_dir_all(root.join("bin")).expect("runtime directory");
        fs::write(root.join(TOOL_PACKAGE_MANIFEST_NAME), STDIO_MANIFEST).expect("manifest");
        fs::write(root.join("bin/fixture.exe"), STDIO_RUNTIME).expect("runtime file");
    }

    fn write_source(root: &Path, manifest: &[u8], runtime: &[u8]) {
        fs::create_dir_all(root.join("bin")).expect("runtime directory");
        fs::write(root.join(TOOL_PACKAGE_MANIFEST_NAME), manifest).expect("manifest");
        fs::write(root.join("bin/fixture.exe"), runtime).expect("runtime file");
    }

    fn modified_manifest(display_name: Option<&str>, runtime: &[u8]) -> Vec<u8> {
        let mut manifest = parse_json_strict(STDIO_MANIFEST).expect("fixture JSON");
        if let Some(display_name) = display_name {
            manifest["ui"]["display_name"] = Value::String(display_name.to_owned());
        }
        manifest["files"][0]["byte_length"] = serde_json::json!(runtime.len());
        manifest["files"][0]["sha256"] =
            serde_json::to_value(ToolPackageFileSha256V1::sha256(runtime)).expect("file hash JSON");
        serde_json::to_vec(&manifest).expect("raw Manifest JSON")
    }

    fn two_tool_manifest() -> Vec<u8> {
        let mut manifest = parse_json_strict(STDIO_MANIFEST).expect("fixture JSON");
        let mut first = manifest["tools"][0].clone();
        first["tool_key"] = Value::String("zeta".to_owned());
        first["model"]["canonical_suffix"] = Value::String("zeta".to_owned());
        let mut second = manifest["tools"][0].clone();
        second["tool_key"] = Value::String("alpha".to_owned());
        second["model"]["canonical_suffix"] = Value::String("alpha".to_owned());
        manifest["tools"] = Value::Array(vec![first, second]);
        serde_json::to_vec(&manifest).expect("two-Tool raw Manifest JSON")
    }

    #[test]
    fn staged_evidence_uses_exact_domain_separated_jcs_preimages() {
        let source = tempfile::tempdir().expect("source");
        let staging = tempfile::tempdir().expect("staging");
        write_valid_source(source.path());
        let staged =
            stage_directory_package_v1(source.path(), staging.path()).expect("stage package");
        let evidence = staged.fingerprint_evidence().expect("fingerprint evidence");

        assert_eq!(evidence.tool_contract_preimages().len(), 1);
        assert_eq!(evidence.tool_contract_fingerprints().len(), 1);
        let tool_preimage = &evidence.tool_contract_preimages()[0];
        let tool_entry = &evidence.tool_contract_fingerprints()[0];
        let tool_json = serde_json::to_value(tool_preimage).expect("tool preimage JSON");
        assert_eq!(tool_json["kind"], TOOL_CONTRACT_FINGERPRINT_KIND_V1);
        assert_eq!(tool_json["preimage_version"], 1);
        assert_eq!(tool_json["tool_contract"]["tool_key"], tool_json["tool_key"]);
        assert_eq!(
            tool_entry.fingerprint().fingerprint,
            canonical_sha256(tool_preimage).expect("tool hash")
        );

        let package_json =
            serde_json::to_value(evidence.package_preimage()).expect("package preimage JSON");
        let security_json = serde_json::to_value(evidence.package_security_preimage())
            .expect("security preimage JSON");
        assert_eq!(package_json["kind"], PACKAGE_FINGERPRINT_KIND_V1);
        assert_eq!(security_json["kind"], PACKAGE_SECURITY_FINGERPRINT_KIND_V1);
        assert_eq!(package_json["preimage_version"], 1);
        assert_eq!(security_json["preimage_version"], 1);
        assert_ne!(
            canonical_json_jcs(evidence.package_preimage()).expect("package JCS"),
            canonical_json_jcs(evidence.package_security_preimage()).expect("security JCS")
        );
        assert_eq!(
            evidence.package_fingerprint().0,
            canonical_sha256(evidence.package_preimage()).expect("package hash")
        );
        assert_eq!(
            evidence.package_security_fingerprint().0,
            canonical_blake3(evidence.package_security_preimage()).expect("security hash")
        );
        assert_eq!(
            evidence.package_fingerprint().0.algorithm(),
            CanonicalSha256HashV1::sha256(b"").algorithm()
        );
        assert_eq!(
            evidence.package_security_fingerprint().0.algorithm(),
            CanonicalBlake3HashV1::blake3(b"").algorithm()
        );
    }

    #[test]
    fn fingerprints_are_deterministic_across_independent_staging_roots() {
        let first_source = tempfile::tempdir().expect("first source");
        let second_source = tempfile::tempdir().expect("second source");
        let first_staging = tempfile::tempdir().expect("first staging");
        let second_staging = tempfile::tempdir().expect("second staging");
        write_valid_source(first_source.path());
        write_valid_source(second_source.path());

        let first = stage_directory_package_v1(first_source.path(), first_staging.path())
            .expect("first package");
        let second = stage_directory_package_v1(second_source.path(), second_staging.path())
            .expect("second package");
        assert_eq!(first.runtime_closure(), second.runtime_closure());
        assert_eq!(
            first.fingerprint_evidence().expect("first evidence"),
            second.fingerprint_evidence().expect("second evidence")
        );
    }

    #[test]
    fn full_package_visibility_changes_every_fingerprint_for_ui_or_file_changes() {
        let baseline_source = tempfile::tempdir().expect("baseline source");
        let ui_source = tempfile::tempdir().expect("UI source");
        let file_source = tempfile::tempdir().expect("file source");
        let baseline_staging = tempfile::tempdir().expect("baseline staging");
        let ui_staging = tempfile::tempdir().expect("UI staging");
        let file_staging = tempfile::tempdir().expect("file staging");
        write_valid_source(baseline_source.path());
        let ui_manifest = modified_manifest(Some("Changed Package UI"), STDIO_RUNTIME);
        write_source(ui_source.path(), &ui_manifest, STDIO_RUNTIME);
        let changed_runtime = b"changed fixture bytes";
        assert_eq!(changed_runtime.len(), STDIO_RUNTIME.len());
        let file_manifest = modified_manifest(None, changed_runtime);
        write_source(file_source.path(), &file_manifest, changed_runtime);

        let baseline = stage_directory_package_v1(baseline_source.path(), baseline_staging.path())
            .expect("baseline package");
        let ui =
            stage_directory_package_v1(ui_source.path(), ui_staging.path()).expect("UI variant");
        let file = stage_directory_package_v1(file_source.path(), file_staging.path())
            .expect("file variant");
        let baseline_evidence = baseline.fingerprint_evidence().expect("baseline evidence");

        for variant in [&ui, &file] {
            let variant_evidence = variant.fingerprint_evidence().expect("variant evidence");
            assert_ne!(baseline.runtime_closure(), variant.runtime_closure());
            assert_ne!(
                baseline_evidence.tool_contract_fingerprints(),
                variant_evidence.tool_contract_fingerprints()
            );
            assert_ne!(
                baseline_evidence.package_fingerprint(),
                variant_evidence.package_fingerprint()
            );
            assert_ne!(
                baseline_evidence.package_security_fingerprint(),
                variant_evidence.package_security_fingerprint()
            );
        }
    }

    #[test]
    fn consistency_gate_rejects_cross_package_manifest_and_closure() {
        let source = tempfile::tempdir().expect("source");
        let staging = tempfile::tempdir().expect("staging");
        write_valid_source(source.path());
        let staged =
            stage_directory_package_v1(source.path(), staging.path()).expect("stage package");
        let mut other_manifest = staged.canonical_manifest().clone();
        other_manifest.package_version =
            aura_domain_common::CanonicalSemVer::new("1.0.1").expect("other package version");
        let other_bytes = canonical_json_jcs(&other_manifest).expect("other canonical bytes");

        assert!(matches!(
            ToolPackageFingerprintEvidenceV1::from_authoritative_closure(
                &other_bytes,
                &other_manifest,
                staged.runtime_closure(),
            ),
            Err(ToolPackageFingerprintErrorV1::RuntimeClosureMismatch)
        ));
        assert_eq!(
            ToolPackageFingerprintErrorV1::RuntimeClosureMismatch.public_code(),
            ToolPackageErrorCode::FingerprintMismatch
        );
    }

    #[test]
    fn entry_builder_sorts_once_and_rejects_noncanonical_embedded_contracts() {
        let source = tempfile::tempdir().expect("source");
        let staging = tempfile::tempdir().expect("staging");
        write_source(source.path(), &two_tool_manifest(), STDIO_RUNTIME);
        let staged =
            stage_directory_package_v1(source.path(), staging.path()).expect("stage package");
        let evidence = staged.fingerprint_evidence().expect("fingerprint evidence");
        let entry_keys = evidence
            .tool_contract_fingerprints()
            .iter()
            .map(|entry| entry.tool_key().as_str())
            .collect::<Vec<_>>();
        assert_eq!(entry_keys, ["alpha", "zeta"]);
        assert_eq!(
            entry_keys,
            evidence
                .package_preimage()
                .tool_contract_fingerprints()
                .iter()
                .map(|entry| entry.tool_key().as_str())
                .collect::<Vec<_>>()
        );

        let mut invalid_manifest = staged.canonical_manifest().clone();
        invalid_manifest.tools.reverse();
        let invalid_bytes = canonical_json_jcs(&invalid_manifest).expect("invalid-order JCS");
        assert!(matches!(
            ToolPackageFingerprintEvidenceV1::from_authoritative_closure(
                &invalid_bytes,
                &invalid_manifest,
                staged.runtime_closure(),
            ),
            Err(ToolPackageFingerprintErrorV1::CanonicalManifestInvalid(
                ToolPackageManifestContractError::InvalidCollectionOrder
            ))
        ));

        let mut inconsistent_evidence = evidence.clone();
        inconsistent_evidence.package_preimage.tool_contract_fingerprints.clear();
        assert!(matches!(
            inconsistent_evidence.verify(
                staged.canonical_manifest_bytes(),
                staged.canonical_manifest(),
                staged.runtime_closure(),
            ),
            Err(ToolPackageFingerprintErrorV1::ToolSetMismatch)
        ));
    }

    #[test]
    fn schema_exposes_const_domains_and_version_and_closes_preimage_objects() {
        let root = schemars::schema_for!(PackageFingerprintPreimageV1);
        let value = serde_json::to_value(root).expect("schema JSON");
        let text = serde_json::to_string(&value).expect("schema text");
        assert!(text.contains("aura.p07.package_fingerprint"));
        assert!(text.contains("\"const\":1"));
        assert_eq!(value["additionalProperties"], Value::Bool(false));
    }
}
