//! Read-only installation identity verification, independent of backend readiness.
//!
//! Answering "who owns this capability ACE?" must not require the installation's
//! runtime images, System CMD state or a Ready Maintainer: those are execution
//! concerns. It requires that the published record is the one this root published,
//! that the root is the installation the record names, and that the capability SID
//! in the record is the one Aura derives for that installation.
//!
//! Compilation: only always-available read-only primitives are used
//! (`open_verified_publish_parent`, `open_verified_existing`,
//! `inspect_existing_path`, `verify_execution_installation_acl`) plus
//! [`crate::capability`], so Core, the standalone sandbox build and the elevated
//! helper all compile it. The execution-runtime feature gates the device
//! *modification* paths, which nothing here needs.
//!
//! Owner identity: the ACL requirement is checked against the owner the manifest
//! names, and the record must agree with the manifest. This deliberately does not
//! claim "this is the calling user's installation" — that check remains in the
//! backend's full `Installation::open`, which this module neither replaces nor
//! loosens. For the guard's purpose every published Aura installation's capability
//! deserves protection, and being conservative here can only skip a change, never
//! authorise one.
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::capability::{CapabilityDevice, is_capability_sid_shape, null_capability_sid};

/// What the read-only identity check could establish.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InstallationIdentity {
    /// No published installation exists here: there is no Aura capability ACE to
    /// protect, and the caller may fall back to its strict rule. Only a missing
    /// base directory, or one without candidates, produces this.
    Absent,
    /// Exactly one candidate verified, and its capability SID is the derived one.
    Verified(VerifiedInstallation),
    /// The question could not be answered — unreadable, damaged, conflicting or
    /// ambiguous. Never a synonym for `Absent`, never permission to replace.
    Unverified(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedInstallation {
    pub root: PathBuf,
    pub installation_id: String,
    pub null_capability_sid: String,
}

/// The capability ACE Aura installs for this installation on the NUL device.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NullCapabilityAce {
    pub sid: String,
    pub mask: u32,
}

/// NUL readiness under the verified-installation rule — the single answer every
/// readiness consumer uses, so a probe, a launch gate and a plan cannot disagree.
///
/// A readiness gate must treat `Unanswerable` as *not ready*: a gate may not turn
/// "I could not check" into "go".
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NullDeviceVerdict {
    /// The frozen requirements hold, with at most the verified capability ACE.
    Ready,
    /// The frozen requirements do not hold; the strict rule applies.
    NotReady,
    /// A published installation candidate exists but did not verify, so whether a
    /// replacement would delete a capability is unknown.
    Unanswerable(String),
}

impl NullDeviceVerdict {
    #[must_use]
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }

    #[must_use]
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Unanswerable(reason) => Some(reason),
            Self::Ready | Self::NotReady => None,
        }
    }
}

/// The directory Setup publishes installation roots under.
pub fn published_installations_base() -> std::io::Result<PathBuf> {
    Ok(crate::common_application_data_directory()?.join("Aura/R06-Spike"))
}

/// NUL readiness for this host, read-only.
///
/// * no published installation: the strict rule,
/// * one verified installation: the coexistence rule for exactly its capability,
/// * a candidate that did not verify: [`NullDeviceVerdict::Unanswerable`].
///
/// A missing or unreadable ProgramData is `Unanswerable` too: not being able to
/// look is not evidence that there is nothing to protect.
#[must_use]
pub fn probe_null_device_readiness() -> NullDeviceVerdict {
    match published_installations_base() {
        Ok(base) => probe_null_device_readiness_at(&base),
        // Not being able to look is not evidence that there is nothing to
        // protect, so this is unanswerable rather than the strict rule.
        Err(error) => NullDeviceVerdict::Unanswerable(format!(
            "the installation base is unavailable: {error}"
        )),
    }
}

/// [`probe_null_device_readiness`] against an explicit installation base.
#[must_use]
pub fn probe_null_device_readiness_at(base: &Path) -> NullDeviceVerdict {
    let identity = verify_installation_identity(base);
    if let InstallationIdentity::Unverified(reason) = &identity {
        return NullDeviceVerdict::Unanswerable(reason.clone());
    }
    match identity.null_device_ready() {
        Ok(true) => NullDeviceVerdict::Ready,
        Ok(false) => NullDeviceVerdict::NotReady,
        Err(reason) => NullDeviceVerdict::Unanswerable(reason),
    }
}

/// Verify the published installation identity, read-only.
#[must_use]
pub fn verify_installation_identity(base: &Path) -> InstallationIdentity {
    if !base.exists() {
        // A base that was never created means Setup has not run here.
        return InstallationIdentity::Absent;
    }
    let root = match discover_root(base) {
        Ok(Discovery::None) => return InstallationIdentity::Absent,
        Ok(Discovery::Found(root)) => root,
        Ok(Discovery::Ambiguous) => {
            return InstallationIdentity::Unverified(format!(
                "more than one installation candidate under {}",
                base.display()
            ));
        }
        Err(reason) => return InstallationIdentity::Unverified(reason),
    };
    match verify_root(&root) {
        Ok(installation) => InstallationIdentity::Verified(installation),
        // A candidate that exists but cannot be verified is reported, never
        // skipped: silence here would look exactly like "no installation".
        Err(reason) => InstallationIdentity::Unverified(format!(
            "candidate {} did not verify: {reason}",
            root.display()
        )),
    }
}

impl InstallationIdentity {
    /// The capability ACE to allow, when the identity is verified.
    ///
    /// `Absent` yields `Ok(None)` because there is genuinely no Aura capability
    /// here. `Unverified` yields an error: the caller must not read "I could not
    /// check" as "there is nothing to protect".
    pub fn null_capability(&self) -> Result<Option<NullCapabilityAce>, String> {
        match self {
            Self::Absent => Ok(None),
            Self::Unverified(reason) => Err(reason.clone()),
            Self::Verified(installation) => {
                let derived = null_capability_sid(&installation.installation_id)
                    .map_err(|error| format!("capability derivation failed: {error}"))?;
                if derived != installation.null_capability_sid {
                    return Err(format!(
                        "recorded capability SID {} does not match the derived {derived}",
                        installation.null_capability_sid
                    ));
                }
                Ok(Some(NullCapabilityAce {
                    sid: derived,
                    mask: CapabilityDevice::Null.access_mask(),
                }))
            }
        }
    }

    /// Whether `descriptor` grants the verified capability ACE.
    ///
    /// `descriptor` is a NUL security descriptor. The check parses the ACE list
    /// and compares SIDs: a substring test could match unrelated text and is not
    /// an ACE identity. A SID that appears only as text is not a grant.
    pub fn descriptor_carries_capability(&self, descriptor: &str) -> Result<bool, String> {
        let Some(capability) = self.null_capability()? else {
            return Ok(false);
        };
        Ok(descriptor_aces(descriptor)?.iter().any(|ace| ace.sid == capability.sid))
    }

    /// Whether replacing `current` with `replacement` would drop the verified
    /// capability ACE.
    ///
    /// Both descriptors are checked by parsed ACE identity. This is the question
    /// both NUL writes ask: the strict replacement always writes the frozen
    /// constant, and a rollback writes the frozen original, so a capability
    /// created after the freeze is only safe if the replacement still carries it.
    pub fn replacement_drops_capability(
        &self,
        current: &str,
        replacement: &str,
    ) -> Result<bool, String> {
        if !self.descriptor_carries_capability(current)? {
            return Ok(false);
        }
        Ok(!self.descriptor_carries_capability(replacement)?)
    }

    /// NUL readiness under the verified-capability policy.
    ///
    /// * `Absent`: the strict rule applies — there is no Aura capability to allow.
    /// * `Verified`: the limited rule applies for exactly that ACE.
    /// * `Unverified`: an error, so a caller cannot turn "unanswerable" into "go".
    pub fn null_device_ready(&self) -> Result<bool, String> {
        match self {
            Self::Absent => crate::host_preparation::probe_null_device_compatibility()
                .map_err(|error| format!("NUL probe failed: {error}")),
            Self::Unverified(reason) => Err(reason.clone()),
            Self::Verified(_) => {
                let capability = self
                    .null_capability()?
                    .ok_or_else(|| "verified installation has no capability".to_owned())?;
                let ace = crate::host_preparation::VerifiedCapabilityAce::from_derived(
                    capability.sid,
                    capability.mask,
                )
                .map_err(|error| format!("capability ACE is malformed: {error}"))?;
                crate::host_preparation::probe_null_device_compatibility_limited(Some(&ace))
                    .map_err(|error| format!("NUL probe failed: {error}"))
            }
        }
    }
}

enum Discovery {
    None,
    Found(PathBuf),
    Ambiguous,
}

/// Discover the single published installation root under `base`.
///
/// Every failure is propagated: an unreadable directory, an unreadable entry or an
/// entry that cannot be inspected is a reason to say "unverified", not to pretend
/// the candidate was never there.
fn discover_root(base: &Path) -> Result<Discovery, String> {
    let entries = std::fs::read_dir(base)
        .map_err(|error| format!("installation base {} is unreadable: {error}", base.display()))?;
    let mut found: Option<PathBuf> = None;
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!("an entry under {} could not be read: {error}", base.display())
        })?;
        let root = entry.path();
        let metadata = entry
            .metadata()
            .map_err(|error| format!("{} could not be inspected: {error}", root.display()))?;
        if !metadata.is_dir() || !root.join("execution.json").is_file() {
            continue;
        }
        if found.is_some() {
            return Ok(Discovery::Ambiguous);
        }
        found = Some(root);
    }
    Ok(match found {
        Some(root) => Discovery::Found(root),
        None => Discovery::None,
    })
}

/// Verify one root's published identity, read-only.
fn verify_root(root: &Path) -> Result<VerifiedInstallation, String> {
    if !root.is_absolute() {
        return Err("installation root is not absolute".to_owned());
    }
    let root_handle = crate::open_verified_publish_parent(root)
        .map_err(|error| format!("root is not a verified publish parent: {error}"))?;
    if !root_handle.info().is_directory {
        return Err("root is not a directory".to_owned());
    }
    for parent in root.ancestors().filter(|parent| parent.is_absolute()) {
        let inspected = crate::inspect_existing_path(parent, false)
            .map_err(|error| format!("ancestor is unreadable: {error}"))?;
        if inspected.is_reparse_point {
            return Err("path contains a reparse point".to_owned());
        }
    }
    // The manifest is read through a verified handle; the ACL of the same paths is
    // checked below, so what is inspected is what is read.
    let manifest_file =
        crate::open_verified_existing(&root.join("execution.json"), crate::NativeFileAccess::Read)
            .map_err(|error| format!("execution.json is unreadable: {error}"))?;
    let manifest_bytes = read_bounded(manifest_file, 64 * 1024, "execution.json")?;
    let manifest: Value = serde_json::from_slice(&manifest_bytes)
        .map_err(|error| format!("execution.json is not valid JSON: {error}"))?;
    let installation_id = required_str(&manifest, "installation_id", "execution.json")?.to_owned();
    let owner_sid = required_str(&manifest, "owner_sid", "execution.json")?.to_owned();
    let offline_sid = required_str(&manifest, "offline.sid", "execution.json")?.to_owned();
    let offline_name = required_str(&manifest, "offline.name", "execution.json")?.to_owned();
    // Published format, not merely parseable JSON. Each field is read with the
    // representation the published contract itself uses — the revision is a
    // canonical decimal *string* (`Revision`), the versions are strings, and the
    // record digest is an algorithm/hex object — because a shape of my own
    // invention would "verify" a file the product would refuse and refuse the file
    // the product published.
    let revision: aura_domain_common::Revision = serde_json::from_value(
        manifest
            .get("revision")
            .cloned()
            .ok_or_else(|| "execution.json has no revision".to_owned())?,
    )
    .map_err(|error| format!("execution.json revision is not a canonical revision: {error}"))?;
    if revision.value() == 0 {
        return Err("execution.json revision is not positive".to_owned());
    }
    for field in ["assurance_version", "installation_version"] {
        required_str(&manifest, field, "execution.json")?;
    }
    let (record_algorithm, record_hex) = match manifest.get("resource_record_hash") {
        Some(Value::Object(hash)) => (
            hash.get("algorithm")
                .and_then(Value::as_str)
                .ok_or_else(|| "resource_record_hash has no algorithm".to_owned())?
                .to_owned(),
            hash.get("hex")
                .and_then(Value::as_str)
                .ok_or_else(|| "resource_record_hash has no hex".to_owned())?
                .to_owned(),
        ),
        _ => return Err("execution.json has no resource_record_hash".to_owned()),
    };
    if record_algorithm != "sha256" || record_hex.len() != 64 {
        return Err(format!(
            "resource_record_hash must be a sha256 digest, got {record_algorithm}"
        ));
    }
    if root.file_name().and_then(|name| name.to_str()) != Some(installation_id.as_str()) {
        return Err("installation directory does not match the recorded installation id".to_owned());
    }
    for path in [root.to_path_buf(), root.join("execution.json"), root.join("record.json")] {
        crate::verify_execution_installation_acl(&path, &owner_sid, false).map_err(|error| {
            format!("installation ACL check failed for {}: {error}", path.display())
        })?;
    }
    let record_file =
        crate::open_verified_existing(&root.join("record.json"), crate::NativeFileAccess::Read)
            .map_err(|error| format!("record.json is unreadable: {error}"))?;
    let record_bytes = read_bounded(record_file, 1024 * 1024, "record.json")?;
    let digest = format!("{:x}", Sha256::digest(&record_bytes));
    if !digest.eq_ignore_ascii_case(&record_hex) {
        return Err("record.json does not match the manifest's resource_record_hash".to_owned());
    }
    let record: Value = serde_json::from_slice(&record_bytes)
        .map_err(|error| format!("record.json is not valid JSON: {error}"))?;
    let agrees = record.get("state").and_then(Value::as_str) == Some("installed")
        && record.get("install_id").and_then(Value::as_str) == Some(installation_id.as_str())
        && record.get("owner_sid").and_then(Value::as_str) == Some(owner_sid.as_str())
        && record.get("offline_sid").and_then(Value::as_str) == Some(offline_sid.as_str())
        && record.get("account").and_then(Value::as_str) == Some(offline_name.as_str());
    if !agrees {
        return Err("record.json identity does not match the manifest".to_owned());
    }
    let null_capability_sid =
        required_str(&record, "null_capability.sid", "record.json")?.to_owned();
    if !is_capability_sid_shape(&null_capability_sid) {
        return Err("recorded null_capability.sid is not a capability SID".to_owned());
    }
    Ok(VerifiedInstallation { root: root.to_path_buf(), installation_id, null_capability_sid })
}

fn required_str<'a>(value: &'a Value, pointer: &str, file: &str) -> Result<&'a str, String> {
    value
        .pointer(&format!("/{}", pointer.replace('.', "/")))
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| format!("{file} has no {pointer}"))
}

fn read_bounded(
    mut file: crate::VerifiedFile,
    limit: usize,
    label: &str,
) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    file.file_mut()
        .take(u64::try_from(limit).unwrap_or(u64::MAX) + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("{label} could not be read: {error}"))?;
    if bytes.len() > limit {
        return Err(format!("{label} exceeds its size limit"));
    }
    Ok(bytes)
}

#[derive(Debug)]
pub(crate) struct DescriptorAce {
    pub(crate) sid: String,
}

/// The ACE SIDs of a descriptor, parsed rather than substring-matched.
pub(crate) fn descriptor_aces(descriptor: &str) -> Result<Vec<DescriptorAce>, String> {
    let end = descriptor.find("S:(").unwrap_or(descriptor.len());
    let dacl = &descriptor[..end];
    let start = dacl.find("D:").ok_or_else(|| "descriptor has no DACL".to_owned())?;
    let mut rest = &dacl[start + 2..];
    while let Some(first) = rest.chars().next() {
        if first == 'P' {
            rest = &rest[1..];
        } else if rest.starts_with("AR") || rest.starts_with("AI") {
            rest = &rest[2..];
        } else {
            break;
        }
    }
    let mut aces = Vec::new();
    while let Some(inner) = rest.strip_prefix('(') {
        let end = inner.find(')').ok_or_else(|| "descriptor has an unterminated ACE".to_owned())?;
        let fields: Vec<&str> = inner[..end].split(';').collect();
        if fields.len() != 6 || fields[5].is_empty() {
            return Err("descriptor has a malformed ACE".to_owned());
        }
        aces.push(DescriptorAce { sid: fields[5].to_owned() });
        rest = &inner[end + 1..];
    }
    Ok(aces)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEV_INSTALLATION: &str = "073773f9-1798-4a46-a6d1-b8d5c9e3d19a";

    #[test]
    fn the_capability_derivation_matches_the_installed_machines_ace() {
        // Derived, not assumed: the same rule the install plan uses, checked
        // against the SID observed on this machine's NUL device.
        assert_eq!(
            null_capability_sid(DEV_INSTALLATION).expect("derive"),
            "S-1-5-21-1945148321-1189346774-2431522260-765611959"
        );
        assert_eq!(CapabilityDevice::Null.access_mask(), 0x0012_019f);
        assert!(crate::capability::capability_sid(CapabilityDevice::Null, "not-a-uuid").is_err());
    }

    #[test]
    fn a_missing_base_is_absent_but_a_damaged_candidate_is_not() {
        let root = tempfile::tempdir().expect("temp");
        // Never created: genuinely no installation.
        let missing = root.path().join("never-created");
        assert_eq!(verify_installation_identity(&missing), InstallationIdentity::Absent);
        // Created but empty: still no candidate.
        let empty = root.path().join("empty");
        std::fs::create_dir(&empty).expect("dir");
        assert_eq!(verify_installation_identity(&empty), InstallationIdentity::Absent);
        // A candidate whose manifest is unreadable JSON must NOT look absent.
        let damaged = root.path().join("damaged");
        std::fs::create_dir(&damaged).expect("dir");
        std::fs::write(damaged.join("execution.json"), b"{ not json").expect("write");
        match verify_installation_identity(root.path()) {
            InstallationIdentity::Unverified(reason) => {
                assert!(reason.contains("did not verify"), "{reason}");
            }
            other => panic!("a damaged candidate must be unverified, got {other:?}"),
        }
    }

    #[test]
    fn two_candidates_are_ambiguous_not_absent() {
        let root = tempfile::tempdir().expect("temp");
        for name in ["one", "two"] {
            let candidate = root.path().join(name);
            std::fs::create_dir(&candidate).expect("dir");
            std::fs::write(candidate.join("execution.json"), b"{}").expect("write");
        }
        match verify_installation_identity(root.path()) {
            InstallationIdentity::Unverified(reason) => {
                assert!(reason.contains("more than one"), "{reason}");
            }
            other => panic!("expected ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn unverified_never_reports_absence_of_a_capability() {
        // The dangerous reading is "I could not check, so there is nothing to
        // protect": it must be an error, and it must also refuse to be used as a
        // NUL readiness verdict.
        let unverified = InstallationIdentity::Unverified("record unreadable".to_owned());
        assert!(unverified.null_capability().is_err());
        assert!(unverified.null_device_ready().is_err());
        assert!(
            unverified.descriptor_carries_capability("D:(A;;0x12019f;;;S-1-5-21-1-2-3-4)").is_err()
        );
        // Absent is the only state that yields "no capability".
        assert_eq!(InstallationIdentity::Absent.null_capability(), Ok(None));
        assert!(
            !InstallationIdentity::Absent
                .descriptor_carries_capability("D:(A;;FA;;;SY)")
                .expect("decide")
        );
    }

    #[test]
    fn replacement_deletion_is_decided_on_parsed_aces() {
        let sid = null_capability_sid(DEV_INSTALLATION).expect("derive");
        let identity = InstallationIdentity::Verified(VerifiedInstallation {
            root: PathBuf::from("C:/unused"),
            installation_id: DEV_INSTALLATION.to_owned(),
            null_capability_sid: sid.clone(),
        });
        let with_capability =
            format!("D:(A;;0x1201bf;;;WD)(A;;FA;;;SY)(A;;0x12019f;;;{sid})S:(ML;;NW;;;LW)");
        assert!(identity.descriptor_carries_capability(&with_capability).expect("decide"));
        assert!(!identity.descriptor_carries_capability("D:(A;;FA;;;SY)").expect("decide"));
        // A SID that appears only as text (not as an ACE trustee) is not a grant.
        assert!(
            !identity
                .descriptor_carries_capability(&format!("D:(A;;FA;;;SY)(A;;FA;;;BA){sid}"))
                .expect("decide")
        );
        assert!(identity.descriptor_carries_capability("O:BAG:SY").is_err());
    }

    #[test]
    fn an_unverified_installation_makes_nul_readiness_unanswerable() {
        // A candidate that exists but does not verify must never be reported as
        // "no installation, strict rule" — that reading would authorise the very
        // replacement that deletes the capability.
        let root = tempfile::tempdir().expect("temp");
        let damaged = root.path().join("damaged");
        std::fs::create_dir(&damaged).expect("dir");
        std::fs::write(damaged.join("execution.json"), b"{ not json").expect("write");
        match probe_null_device_readiness_at(root.path()) {
            NullDeviceVerdict::Unanswerable(reason) => {
                assert!(reason.contains("did not verify"), "{reason}");
                assert!(!NullDeviceVerdict::Unanswerable(reason).is_ready());
            }
            other => panic!("a damaged candidate must be unanswerable, got {other:?}"),
        }
        // A base that was never created is the strict rule on this host: a
        // definite answer, never "unanswerable".
        let absent = probe_null_device_readiness_at(&root.path().join("missing"));
        assert!(absent.reason().is_none(), "{absent:?}");
    }

    #[test]
    fn a_replacement_is_refused_only_when_it_would_drop_the_capability() {
        let sid = null_capability_sid(DEV_INSTALLATION).expect("derive");
        let identity = InstallationIdentity::Verified(VerifiedInstallation {
            root: PathBuf::from("C:/unused"),
            installation_id: DEV_INSTALLATION.to_owned(),
            null_capability_sid: sid.clone(),
        });
        let current = format!("D:(A;;FA;;;SY)(A;;0x12019f;;;{sid})S:(ML;;NW;;;LW)");
        let strict = "D:(A;;GRGWGX;;;WD)(A;;FA;;;SY)S:(ML;;NW;;;LW)";
        assert!(identity.replacement_drops_capability(&current, strict).expect("decide"));
        // A rollback whose frozen original already carries the capability keeps it.
        assert!(!identity.replacement_drops_capability(&current, &current).expect("decide"));
        // Nothing to drop: the capability is not currently granted.
        assert!(!identity.replacement_drops_capability("D:(A;;FA;;;SY)", strict).expect("decide"));
        // Absent installation: no capability exists, so no replacement can drop one.
        assert!(
            !InstallationIdentity::Absent
                .replacement_drops_capability(&current, strict)
                .expect("decide")
        );
        // Unanswerable: refused rather than treated as "nothing to protect".
        assert!(
            InstallationIdentity::Unverified("unreadable".to_owned())
                .replacement_drops_capability(&current, strict)
                .is_err()
        );
    }

    #[test]
    fn the_published_base_is_under_the_common_application_data_directory() {
        let base = published_installations_base().expect("this machine has ProgramData");
        assert!(base.is_absolute());
        assert!(base.ends_with("Aura/R06-Spike") || base.ends_with(r"Aura\R06-Spike"), "{base:?}");
    }

    #[test]
    fn a_recorded_capability_that_disagrees_with_the_derivation_is_refused() {
        let identity = InstallationIdentity::Verified(VerifiedInstallation {
            root: PathBuf::from("C:/unused"),
            installation_id: DEV_INSTALLATION.to_owned(),
            null_capability_sid: "S-1-5-21-9-9-9-9".to_owned(),
        });
        let reason = identity.null_capability().expect_err("must refuse");
        assert!(reason.contains("does not match the derived"), "{reason}");
    }
}
