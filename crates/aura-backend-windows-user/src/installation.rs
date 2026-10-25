//! Read-only installation verification. Setup publishes this separate, versioned
//! execution manifest only after the existing owned resource ledger is valid.
use anyhow::{Result, ensure};
use aura_domain_common::{
    ContentHash,
    jcs::{CanonicalSha256HashV1, canonical_sha256},
};
use aura_windows_platform::{AccountNetworkPolicy, AccountNetworkSpec};
pub use aura_windows_user_contract::{
    ExecutionAccount, ExecutionFileBinding, InstallationManifest, SYSTEM_CMD_APPLY_VERSION,
    SYSTEM_CMD_CONVERSION_VERSION, SystemCmdApplyPlan, SystemCmdConversion,
    SystemCmdConversionState,
};
use std::path::{Path, PathBuf};

/// How long each stage of opening one installation took, in whole milliseconds.
///
/// Verification is the expensive part of a descriptor or readiness query — the whole
/// published package and every image are hashed — so where that time went is recorded as
/// a fact about the installation this process verified, instead of being guessed from a
/// total. The numbers are measured once per open and never recomputed for a cached
/// binding, which is why a cached readiness answer stays cheap.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InstallationStages {
    pub published_ms: u64,
    pub package_ms: u64,
    pub images_ms: u64,
    pub cmd_ms: u64,
    /// Time spent re-checking a cached binding's resources. An open that hashed the
    /// package records that work in the stages above; a cache hit records only this.
    pub verify_ms: u64,
    /// Time this caller spent waiting to learn the shared answer. A shared
    /// failure records this wait and does not copy the first check's duration.
    pub wait_ms: u64,
}

impl InstallationStages {
    #[must_use]
    pub const fn total_ms(&self) -> u64 {
        self.published_ms
            .saturating_add(self.package_ms)
            .saturating_add(self.images_ms)
            .saturating_add(self.cmd_ms)
            .saturating_add(self.verify_ms)
            .saturating_add(self.wait_ms)
    }
}

pub struct Installation {
    pub root: PathBuf,
    pub manifest: InstallationManifest,
    pub fingerprint: CanonicalSha256HashV1,
    /// Stage durations of the open that produced this value.
    pub stages: InstallationStages,
    record: serde_json::Value,
    _images: Vec<aura_windows_platform::VerifiedFile>,
}

pub struct PublishedInstallation {
    pub root: PathBuf,
    pub manifest: InstallationManifest,
    pub fingerprint: CanonicalSha256HashV1,
    pub record: serde_json::Value,
    pub images: Vec<aura_windows_platform::VerifiedFile>,
}

/// Discover the published installation root under `base`, read-only.
///
/// Same rule as the execution backend's discovery: a root that carries
/// `execution.json` and whose installation ACL matches the current owner, and
/// only when exactly one such root exists. Nothing is created or repaired here.
#[must_use]
pub fn discover_published(base: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(base).ok()?;
    let (owner, _) = aura_windows_platform::current_user_token_identity().ok()?;
    let mut found = None;
    for entry in entries.flatten() {
        let root = entry.path();
        if !root.join("execution.json").exists() {
            continue;
        }
        if aura_windows_platform::verify_execution_installation_acl(&root, &owner, false).is_ok() {
            if found.is_some() {
                return None;
            }
            found = Some(root);
        }
    }
    found
}

pub fn load_published(root: &Path) -> Result<PublishedInstallation> {
    ensure!(root.is_absolute(), "runner.install_root");
    let root_handle = aura_windows_platform::open_verified_publish_parent(root)?;
    ensure!(root_handle.info().is_directory, "runner.install_root");
    let (_, mut manifest_file) = super::file_binding::capture(&root.join("execution.json"), true)?;
    use std::io::{Read, Seek};
    manifest_file.file_mut().rewind()?;
    let mut bytes = Vec::new();
    manifest_file.file_mut().take(64 * 1024 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 64 * 1024, "runner.install_manifest_size");
    let manifest: InstallationManifest = serde_json::from_slice(&bytes)?;
    ensure!(
        manifest.assurance_version == aura_windows_user_contract::ASSURANCE_VERSION
            && manifest.installation_version == aura_windows_user_contract::INSTALLATION_VERSION
            && manifest.revision.value() > 0,
        "runner.install_version"
    );
    let fingerprint = canonical_sha256(&manifest)?;
    ensure!(
        root.file_name().and_then(|v| v.to_str()) == Some(manifest.installation_id.as_str()),
        "runner.installation_path_identity"
    );
    for parent in root.ancestors() {
        if parent.is_absolute() {
            ensure!(
                !aura_windows_platform::inspect_existing_path(parent, false)?.is_reparse_point,
                "runner.installation_ancestor_reparse"
            );
        }
    }
    for path in [root.to_path_buf(), root.join("execution.json"), root.join("record.json")] {
        aura_windows_platform::verify_execution_installation_acl(
            &path,
            &manifest.owner_sid,
            false,
        )?;
    }
    let (_, mut record_file) = super::file_binding::capture(&root.join("record.json"), true)?;
    record_file.file_mut().rewind()?;
    let mut record_bytes = Vec::new();
    record_file.file_mut().take(1024 * 1024 + 1).read_to_end(&mut record_bytes)?;
    ensure!(record_bytes.len() <= 1024 * 1024, "runner.resource_record_size");
    ensure!(
        ContentHash::sha256(&record_bytes) == manifest.resource_record_hash,
        "runner.resource_record_changed"
    );
    let record: serde_json::Value = serde_json::from_slice(&record_bytes)?;
    ensure!(
        record["state"] == "installed"
            && record["install_id"] == manifest.installation_id
            && record["owner_sid"] == manifest.owner_sid
            && record["offline_sid"] == manifest.offline.sid
            && record["account"] == manifest.offline.name,
        "runner.account_record_changed"
    );
    ensure!(
        manifest
            .online
            .as_ref()
            .is_none_or(|v| v.sid != manifest.offline.sid && v.name != manifest.offline.name),
        "runner.account_modes_mixed"
    );
    Ok(PublishedInstallation {
        root: root.into(),
        manifest,
        fingerprint,
        record,
        images: vec![root_handle, manifest_file, record_file],
    })
}

impl Installation {
    /// Requires the explicit production manifest; an old Spike record alone is
    /// SetupRequired, even when its historical tests passed.
    pub fn open(root: &Path) -> Result<Self> {
        Self::open_with_stages(root).0
    }

    /// The same open, with the stage timings it measured even when it failed.
    ///
    /// A failed check is exactly the case where the numbers matter: "not ready" can mean a
    /// missing publication, a package that does not verify, or an image that moved, and
    /// those differ in what has to happen next. Every stage that finished records its own
    /// elapsed time as it finishes, so a failure reports the work that really ran before it
    /// stopped; the stage that failed itself has no completed duration.
    pub(crate) fn open_with_stages(root: &Path) -> (Result<Self>, InstallationStages) {
        let mut stages = InstallationStages::default();
        let outcome = Self::open_tracked(root, &mut stages);
        (outcome, stages)
    }

    fn open_tracked(root: &Path, stages: &mut InstallationStages) -> Result<Self> {
        let stage = std::time::Instant::now();
        let published = load_published(root)?;
        stages.published_ms = elapsed_ms(stage);
        let stage = std::time::Instant::now();
        let package =
            super::runtime_package::verify_and_retain(&published.root, &published.record)?;
        stages.package_ms = elapsed_ms(stage);
        let stage = std::time::Instant::now();
        let mut images = published.images;
        images.extend(package);
        for image in [
            &published.manifest.core_image,
            &published.manifest.runner_image,
            &published.manifest.powershell_image,
        ] {
            ensure!(image.sha256.is_some(), "runner.image_hash_missing");
            images.push(super::file_binding::verify(image)?);
        }
        // The image hashes above are the images stage; the command captures below are
        // their own stage because they touch the live system.
        stages.images_ms = elapsed_ms(stage);
        let stage = std::time::Instant::now();
        ensure!(published.manifest.cmd_image.sha256.is_some(), "runner.image_hash_missing");
        images.push(super::file_binding::verify_system_cmd(&published.manifest.cmd_image)?);
        let (cmd, _) = super::file_binding::capture(
            &aura_windows_platform::windows_directory()?.join("System32/cmd.exe"),
            true,
        )?;
        // The manifest is the location authority for the images it hashes: each binary is
        // captured where the manifest says it is, with the installation root as the
        // fallback for a manifest that names a bare file name. The hash still decides
        // whether the file is the right one, so this chooses a location, never a content,
        // and a manifest that points at a build output directory no longer makes an
        // installed root unreadable.
        let (ps, _) = super::file_binding::capture(
            &resolve_image(&published.root, &published.manifest.powershell_image.path),
            true,
        )?;
        let (runner, _) = super::file_binding::capture(
            &resolve_image(&published.root, &published.manifest.runner_image.path),
            true,
        )?;
        let (core, _) = super::file_binding::capture(
            &resolve_image(&published.root, &published.manifest.core_image.path),
            true,
        )?;
        ensure!(
            ps == published.manifest.powershell_image
                && runner == published.manifest.runner_image
                && core == published.manifest.core_image,
            "runner.runtime_location_changed"
        );
        ensure!(cmd == published.manifest.cmd_image, "runner.system_cmd_serviced");
        stages.cmd_ms = elapsed_ms(stage);
        let installed = Self {
            root: published.root,
            manifest: published.manifest,
            fingerprint: published.fingerprint,
            stages: *stages,
            record: published.record,
            _images: images,
        };
        installed.verify_resources()?;
        Ok(installed)
    }

    /// Dedicated-account identity check. Core already retained the runtime
    /// package and inspected WFP/maintenance for this dispatch; repeating that
    /// work here races the execute window and can close the pipe as Unknown.
    pub fn open_for_runner(root: &Path) -> Result<Self> {
        let mut published = load_published(root)?;
        ensure!(published.manifest.runner_image.sha256.is_some(), "runner.image_hash_missing");
        published.images.push(super::file_binding::verify(&published.manifest.runner_image)?);
        let (runner, _) =
            super::file_binding::capture(&published.root.join("aura-command-runner.exe"), true)?;
        ensure!(runner == published.manifest.runner_image, "runner.runtime_location_changed");
        Ok(Self {
            root: published.root,
            manifest: published.manifest,
            fingerprint: published.fingerprint,
            // This path inspects the published images only, so it records no per-stage
            // durations rather than inventing numbers it did not measure.
            stages: InstallationStages::default(),
            record: published.record,
            _images: published.images,
        })
    }

    /// Named OS enforcement resources are checked independently of the held,
    /// immutable package. Readiness refresh never rehashes 658 runtime files.
    /// The installation UUID this root is bound to (from the verified manifest).
    #[must_use]
    pub fn installation_id(&self) -> &str {
        self.manifest.installation_id.as_str()
    }

    /// The NUL capability SID recorded in the verified resource record.
    ///
    /// Read-only, and only meaningful together with the rest of this type: the
    /// record was checked against the manifest digest and the install ACLs by
    /// [Self::open], so this is a verified fact about *this* installation, not
    /// a value taken from arbitrary JSON.
    #[must_use]
    pub fn null_capability_sid(&self) -> Option<&str> {
        self.record
            .get("null_capability")
            .and_then(|capability| capability.get("sid"))
            .and_then(|sid| sid.as_str())
    }
    pub fn verify_resources(&self) -> Result<()> {
        let (caller, _) = aura_windows_platform::current_user_token_identity()?;
        if caller == self.manifest.owner_sid {
            // Core holds this private exclusion across dispatch. Runner must
            // not gain private-directory access just to repeat that check.
            let setup_lock = aura_windows_platform::inspect_existing_path(
                &self.root.join("private/execution-setup.lock"),
                false,
            )?;
            ensure!(
                !setup_lock.is_directory
                    && !setup_lock.is_reparse_point
                    && setup_lock.number_of_links == 1,
                "runner.setup_lock_invalid"
            );
        }
        crate::maintenance::verify_current(
            &self.manifest.maintenance_config_sha256,
            &self.manifest.installation_id,
            &self.manifest.owner_sid,
            &self.manifest.offline.sid,
            self.manifest.online.as_ref().map(|v| v.sid.as_str()),
        )?;
        let policy = &self.record["wfp"]["policy"];
        // The current guarantee includes direct loopback blocking. An older
        // allow_loopback installation does not silently acquire this guarantee.
        ensure!(policy == "block_all", "runner.direct_network_policy_mismatch");
        let weight = self.record["wfp"]["sublayer_weight"]
            .as_u64()
            .and_then(|v| u16::try_from(v).ok())
            .ok_or_else(|| anyhow::anyhow!("runner.wfp_weight_missing"))?;
        let observed = aura_windows_platform::manage_account_network(
            &AccountNetworkSpec {
                install_id: self.manifest.installation_id.clone(),
                offline_sid: self.manifest.offline.sid.clone(),
                policy: AccountNetworkPolicy::BlockAll,
                expected_sublayer_weight: Some(weight),
            },
            "inspect",
        )?;
        ensure!(observed["ready"] == true, "runner.direct_network_repair_required");
        Ok(())
    }

    pub fn account(&self, online: bool) -> Result<&ExecutionAccount> {
        if online {
            self.manifest
                .online
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("runner.online_setup_required"))
        } else {
            Ok(&self.manifest.offline)
        }
    }

    /// Recapture the live System32 CMD. Core, Runner, and pwsh stay exact.
    /// Inspect still requires the new binding afterwards; this does not write.
    pub fn recapture_system_cmd(root: &Path) -> Result<InstallationManifest> {
        Ok(prepare_system_cmd_inner(root)?.0)
    }

    /// Owner-only plan for the product CMD apply entry. Does not write.
    pub fn prepare_system_cmd(root: &Path) -> Result<SystemCmdApplyPlan> {
        ensure_system_cmd_apply_caller(root)?;
        let (manifest, conversion, manifest_sha256) = prepare_system_cmd_inner(root)?;
        Ok(SystemCmdApplyPlan {
            schema: SYSTEM_CMD_APPLY_VERSION.into(),
            manifest,
            conversion,
            manifest_sha256,
        })
    }

    /// Exclusive lock plus handle-bound replace of execution.json. Publication
    /// journals the hashes; inspect/open stay fail-closed until this commits.
    pub fn apply_system_cmd(root: &Path, expected_sha256: &str) -> Result<SystemCmdApplyPlan> {
        ensure_system_cmd_apply_caller(root)?;
        let _lock =
            aura_windows_platform::lock_execution_resources(&root.join("private/execution.lock"))
                .map_err(|error| {
                if error.raw_os_error() == Some(32) {
                    anyhow::anyhow!("runner.execution_in_use")
                } else {
                    anyhow::Error::from(error)
                }
            })?;
        let mut plan = Self::prepare_system_cmd(root)?;
        ensure!(
            plan.manifest_sha256.eq_ignore_ascii_case(expected_sha256),
            "runner.system_cmd_apply_changed"
        );
        let Some(conversion) = plan.conversion.as_mut() else {
            return Ok(plan);
        };
        conversion.state = SystemCmdConversionState::Pending;
        let bytes = serde_json::to_vec(&plan.manifest)?;
        ensure!(
            ContentHash::sha256(&bytes).hex.eq_ignore_ascii_case(&plan.manifest_sha256),
            "runner.system_cmd_apply_bytes"
        );
        let execution = root.join("execution.json");
        let current = std::fs::read(&execution)?;
        if ContentHash::sha256(&current).hex.eq_ignore_ascii_case(&plan.manifest_sha256) {
            conversion.state = SystemCmdConversionState::Applied;
            return Ok(plan);
        }
        let parent = aura_windows_platform::open_verified_publish_parent(root)?;
        let expected = aura_windows_platform::inspect_existing_path(&execution, false)?;
        aura_windows_platform::handle_bound_replace(
            &parent,
            std::ffi::OsStr::new("execution.json"),
            &expected.identity,
            &bytes,
        )
        .map_err(|error| anyhow::anyhow!("runner.system_cmd_apply_publish:{error}"))?
        .commit()
        .map_err(|error| anyhow::anyhow!("runner.system_cmd_apply_commit:{error}"))?;
        conversion.state = SystemCmdConversionState::Applied;
        Ok(plan)
    }

    /// Confirm a journaled conversion against the committed execution.json.
    /// Reconstructs the previous revision; does not write or bump revision.
    pub fn verify_recorded_system_cmd_conversion(
        root: &Path,
        conversion: SystemCmdConversion,
    ) -> Result<SystemCmdConversion> {
        ensure_system_cmd_apply_caller(root)?;
        let published = load_published(root)?;
        applied_system_cmd_conversion(&published.manifest, conversion)
    }
}

fn ensure_system_cmd_apply_caller(root: &Path) -> Result<()> {
    let published = load_published(root)?;
    let (caller, restricted) = aura_windows_platform::current_user_token_identity()?;
    ensure!(!restricted, "runner.system_cmd_apply_restricted");
    ensure!(caller == published.manifest.owner_sid, "runner.system_cmd_apply_owner");
    Ok(())
}

/// Where one declared image is read from.
///
/// The declaration decides first: an absolute path is used as written and a relative one
/// is resolved against the installation root. When the declared location does not exist,
/// the same file name is tried in the root, which is where this repository's publisher
/// puts the binaries beside the manifest. The manifest's hash is checked against whatever
/// is found, so falling back can never accept a different file — it only picks the place
/// to look.
/// Whole milliseconds since `started`, for the stage facts above.
fn elapsed_ms(started: std::time::Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}
pub(crate) fn resolve_image(root: &Path, declared: &str) -> PathBuf {
    let named = {
        let path = Path::new(declared);
        if path.is_absolute() { path.to_path_buf() } else { root.join(path) }
    };
    if named.exists() {
        return named;
    }
    match Path::new(declared).file_name() {
        Some(name) if root.join(name).exists() => root.join(name),
        _ => named,
    }
}
fn prepare_system_cmd_inner(
    root: &Path,
) -> Result<(InstallationManifest, Option<SystemCmdConversion>, String)> {
    let published = load_published(root)?;
    let mut held = Vec::new();
    for image in [
        &published.manifest.core_image,
        &published.manifest.runner_image,
        &published.manifest.powershell_image,
    ] {
        ensure!(image.sha256.is_some(), "runner.image_hash_missing");
        held.push(super::file_binding::verify(image)?);
    }
    let (ps, ps_file) = super::file_binding::capture(
        &resolve_image(&published.root, &published.manifest.powershell_image.path),
        true,
    )?;
    let (runner, runner_file) = super::file_binding::capture(
        &resolve_image(&published.root, &published.manifest.runner_image.path),
        true,
    )?;
    let (core, core_file) = super::file_binding::capture(
        &resolve_image(&published.root, &published.manifest.core_image.path),
        true,
    )?;
    held.extend([ps_file, runner_file, core_file]);
    ensure!(
        ps == published.manifest.powershell_image
            && runner == published.manifest.runner_image
            && core == published.manifest.core_image,
        "runner.runtime_location_changed"
    );
    let live = super::file_binding::capture_trusted_system_cmd()?;
    drop(held);
    if live == published.manifest.cmd_image {
        let bytes = serde_json::to_vec(&published.manifest)?;
        return Ok((
            published.manifest,
            None,
            ContentHash::sha256(&bytes).hex.to_ascii_uppercase(),
        ));
    }
    let revision = published
        .manifest
        .revision
        .value()
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("runner.revision_exhausted"))?;
    let manifest = InstallationManifest {
        revision: aura_domain_common::Revision::new(revision),
        cmd_image: live,
        ..published.manifest.clone()
    };
    let conversion = system_cmd_conversion(&published.manifest, &manifest)?;
    let bytes = serde_json::to_vec(&manifest)?;
    Ok((manifest, Some(conversion), ContentHash::sha256(&bytes).hex.to_ascii_uppercase()))
}

pub fn verify_system_cmd_conversion(
    before: &InstallationManifest,
    after: &InstallationManifest,
    conversion: &SystemCmdConversion,
) -> Result<()> {
    ensure!(
        conversion.schema == SYSTEM_CMD_CONVERSION_VERSION,
        "runner.system_cmd_conversion_schema"
    );
    ensure!(
        conversion.installation_id == before.installation_id
            && before.installation_id == after.installation_id,
        "runner.system_cmd_conversion_install"
    );
    ensure!(
        conversion.from_revision == before.revision && conversion.to_revision == after.revision,
        "runner.system_cmd_conversion_revision"
    );
    ensure!(
        conversion.from_revision.value().checked_add(1) == Some(conversion.to_revision.value()),
        "runner.system_cmd_conversion_step"
    );
    ensure!(
        conversion.from_cmd_image == before.cmd_image && conversion.to_cmd_image == after.cmd_image,
        "runner.system_cmd_conversion_cmd"
    );
    ensure!(before.cmd_image != after.cmd_image, "runner.system_cmd_conversion_unchanged");
    let reconstructed = InstallationManifest {
        revision: before.revision,
        cmd_image: before.cmd_image.clone(),
        ..after.clone()
    };
    ensure!(
        canonical_sha256(&reconstructed)? == canonical_sha256(before)?,
        "runner.system_cmd_conversion_other_fields"
    );
    ensure!(
        before.core_image.sha256.as_ref() == Some(&conversion.core_sha256)
            && before.runner_image.sha256.as_ref() == Some(&conversion.runner_sha256)
            && before.powershell_image.sha256.as_ref() == Some(&conversion.powershell_sha256),
        "runner.system_cmd_conversion_hashes"
    );
    ensure!(
        conversion.from_fingerprint == canonical_sha256(before)?
            && conversion.to_fingerprint == canonical_sha256(after)?,
        "runner.system_cmd_conversion_fingerprint"
    );
    Ok(())
}

fn system_cmd_conversion(
    before: &InstallationManifest,
    after: &InstallationManifest,
) -> Result<SystemCmdConversion> {
    let conversion = SystemCmdConversion {
        schema: SYSTEM_CMD_CONVERSION_VERSION.into(),
        installation_id: before.installation_id.clone(),
        from_revision: before.revision,
        to_revision: after.revision,
        from_cmd_image: before.cmd_image.clone(),
        to_cmd_image: after.cmd_image.clone(),
        from_fingerprint: canonical_sha256(before)?,
        to_fingerprint: canonical_sha256(after)?,
        core_sha256: before
            .core_image
            .sha256
            .clone()
            .ok_or_else(|| anyhow::anyhow!("runner.image_hash_missing"))?,
        runner_sha256: before
            .runner_image
            .sha256
            .clone()
            .ok_or_else(|| anyhow::anyhow!("runner.image_hash_missing"))?,
        powershell_sha256: before
            .powershell_image
            .sha256
            .clone()
            .ok_or_else(|| anyhow::anyhow!("runner.image_hash_missing"))?,
        state: SystemCmdConversionState::Pending,
    };
    verify_system_cmd_conversion(before, after, &conversion)?;
    Ok(conversion)
}

pub(crate) fn applied_system_cmd_conversion(
    current: &InstallationManifest,
    conversion: SystemCmdConversion,
) -> Result<SystemCmdConversion> {
    let before = InstallationManifest {
        revision: conversion.from_revision,
        cmd_image: conversion.from_cmd_image.clone(),
        ..current.clone()
    };
    let mut verified = conversion;
    verified.state = SystemCmdConversionState::Applied;
    verify_system_cmd_conversion(&before, current, &verified)?;
    Ok(verified)
}

/// How long one failed check is shared instead of being run again.
///
/// A failure is a real answer about the installation as it is *now*, and re-running the
/// whole verification for every caller that asks in the same moment is what made concurrent
/// readiness queries pay for the same work. The window is short on purpose: an installation
/// that becomes ready a moment later has to be readable as soon as it is, so this is a
/// dedup of one in-flight answer, never a cached verdict.
pub(crate) const FAILURE_SHARE_WINDOW: std::time::Duration = std::time::Duration::from_millis(500);

/// The open a binding runs. Production uses [`Installation::open_with_stages`]; a
/// concurrency regression replaces it so it can count real opens without needing a whole
/// published installation on the test host.
type InstallationOpener = std::sync::Arc<dyn Fn(&Path) -> Result<Installation> + Send + Sync>;

/// What the last open of this binding found.
enum BindingState {
    /// Nothing has been observed yet.
    Unknown,
    /// A verified installation, held for the life of the binding.
    Ready(std::sync::Arc<Installation>),
    /// A recent failure, shared for [`FAILURE_SHARE_WINDOW`]. Never readiness.
    Failed { message: String, at: std::time::Instant },
}

/// Shared across the three mode-specific Backend registrations. Successful
/// file bindings stay held; Upgrade must close Core and its active executions.
pub struct InstallationBinding {
    pub root: PathBuf,
    pub(crate) resources: crate::runtime::RuntimeResources,
    state: std::sync::Mutex<BindingState>,
    /// What the last open measured, successful or not, for diagnostics.
    last_stages: std::sync::Mutex<Option<(InstallationStages, bool)>>,
    /// The open this binding runs. Production always uses [`Installation::open_with_stages`];
    /// a concurrency regression replaces it so it can count real opens without needing a
    /// whole published installation on the test host.
    opener: Option<InstallationOpener>,
    /// The running Core compared with one install generation. A later poll with the
    /// same fingerprint reuses this answer instead of recapturing the executable.
    core_binding: std::sync::Mutex<Option<(CanonicalSha256HashV1, Option<String>)>>,
}
impl InstallationBinding {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            resources: Default::default(),
            state: std::sync::Mutex::new(BindingState::Unknown),
            last_stages: Default::default(),
            opener: None,
            core_binding: std::sync::Mutex::new(None),
        }
    }

    /// The same binding with a controlled open, for tests.
    #[cfg(test)]
    pub(crate) fn with_opener(root: PathBuf, opener: InstallationOpener) -> Self {
        Self { opener: Some(opener), ..Self::new(root) }
    }

    /// The stage timings of the last open, and whether that open succeeded.
    #[must_use]
    pub fn last_open_stages(&self) -> Option<(InstallationStages, bool)> {
        self.last_stages.lock().ok().and_then(|stages| *stages)
    }

    pub fn current(&self) -> Result<std::sync::Arc<Installation>> {
        self.load(true)
    }

    /// Snapshot/UI advertisement. The first successful observation still opens
    /// and verifies the published installation. Later snapshots reuse the held
    /// binding so Core's sequential IPC loop is not re-entered into WFP and
    /// service inspect on every Renderer poll. Prepare and execute still call
    /// [`Self::current`], which verifies live resources before dispatch.
    pub fn advertised(&self) -> Result<std::sync::Arc<Installation>> {
        self.load(false)
    }

    /// Whether this process is the Core the installation manifest is bound to.
    ///
    /// Path, file identity, owner and hash stay the same facts the runner handshake
    /// checks. A mismatch is a repair of the execution install, not a reason to
    /// edit the script or switch shells. The answer is cached for this process and
    /// install fingerprint.
    pub fn core_binding_reject(
        &self,
        fingerprint: &CanonicalSha256HashV1,
        core_image: &ExecutionFileBinding,
        owner_sid: &str,
    ) -> Result<Option<String>> {
        if let Ok(cache) = self.core_binding.lock()
            && let Some((cached_fingerprint, reject)) = cache.as_ref()
            && cached_fingerprint == fingerprint
        {
            return Ok(reject.clone());
        }
        let path = std::env::current_exe()?;
        let (actual, _) = super::file_binding::capture(&path, false)?;
        let identity = aura_windows_platform::snapshot_process(std::process::id())?;
        let owner = aura_windows_platform::process_account_sid(&identity)?;
        let reject = super::runner::core_identity_handshake_reject(
            Ok(&actual),
            Ok(owner.as_str()),
            core_image,
            owner_sid,
        );
        let reject = reject.map(|detail| {
            let detail = detail.trim_start_matches("runner.");
            format!("runner.core_binding_mismatch.{detail}")
        });
        if let Ok(mut cache) = self.core_binding.lock() {
            *cache = Some((fingerprint.clone(), reject.clone()));
        }
        Ok(reject)
    }

    /// The sentence a person or model sees for a core-binding mismatch.
    ///
    /// The detail is the mismatched fact (path, file identity, owner, hash), not a
    /// filesystem path. Switching shells is not a repair.
    #[must_use]
    pub fn core_binding_user_text(reason: &str) -> String {
        let detail = reason.rsplit('.').next().unwrap_or("identity");
        let label = match detail {
            "path" => "路径",
            "file_id" => "文件身份",
            "volume" => "卷",
            "hash" => "哈希",
            "core_owner_mismatch" => "所有者",
            _ => "身份",
        };
        format!(
            "执行安装绑定的是安装目录里已发布的 Core，与当前进程不是同一个文件（{label}）。对 debug 目录执行 Repair 不会改成当前这个程序：发布会把 Core 复制到安装目录，再绑定那份副本。更换脚本或 Shell 不会修复。"
        )
    }

    /// One open at a time, with its answer — success **or** failure — shared.
    ///
    /// The lock is held across the open on purpose. Opening verifies and hashes the whole
    /// published package and every image, which is the expensive part of a descriptor or
    /// readiness query; two callers arriving together must wait for the first one rather
    /// than each paying for the same verification of the same installation. A failure is
    /// shared the same way for [`FAILURE_SHARE_WINDOW`] and then forgotten: an installation
    /// that is not ready yet becomes readable as soon as it is, and a failed query never
    /// turns into a cached "ready".
    fn load(&self, verify_cached: bool) -> Result<std::sync::Arc<Installation>> {
        let waited = std::time::Instant::now();
        let mut state =
            self.state.lock().map_err(|_| anyhow::anyhow!("runner.installation_lock"))?;
        let wait_ms = u64::try_from(waited.elapsed().as_millis()).unwrap_or(u64::MAX);
        match &*state {
            BindingState::Ready(existing) => {
                let existing = existing.clone();
                if verify_cached {
                    let started = std::time::Instant::now();
                    let verified = existing.verify_resources();
                    // A cached binding did not open the package again. Those stage times stay
                    // with the open that measured them. This call records only the resource
                    // check it actually ran.
                    let stages = InstallationStages {
                        verify_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                        ..InstallationStages::default()
                    };
                    record_installation_open(
                        &stages,
                        verified.is_ok(),
                        true,
                        Some(existing.installation_id()),
                    );
                    verified?;
                }
                return Ok(existing);
            }
            BindingState::Failed { message, at, .. } if at.elapsed() < FAILURE_SHARE_WINDOW => {
                // This caller waited for the lock and then reused the failure. The
                // first check's duration stays on the line that open wrote.
                let stages = InstallationStages { wait_ms, ..InstallationStages::default() };
                record_shared_installation_failure(&stages);
                return Err(anyhow::anyhow!("{message}"));
            }
            BindingState::Unknown | BindingState::Failed { .. } => {}
        }
        let (opened, stages) = match &self.opener {
            Some(opener) => {
                let started = std::time::Instant::now();
                let opened = opener(&self.root);
                // A test open is one check. Its time is the check, not a package hash.
                let stages = InstallationStages {
                    verify_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                    ..InstallationStages::default()
                };
                (opened, stages)
            }
            None => Installation::open_with_stages(&self.root),
        };
        if let Ok(mut last) = self.last_stages.lock() {
            *last = Some((stages, opened.is_ok()));
        }
        // These are the stages of the open that just ran, written by that open.
        record_installation_open(
            &stages,
            opened.is_ok(),
            false,
            opened.as_ref().ok().map(|installed| installed.installation_id()),
        );
        let installed = match opened {
            Ok(installed) => installed,
            Err(error) => {
                let message = format!("{error:#}");
                *state = BindingState::Failed {
                    message: message.clone(),
                    at: std::time::Instant::now(),
                };
                return Err(error);
            }
        };
        let installed = std::sync::Arc::new(installed);
        // Lazy Core initialization may precede service readiness. Reconcile
        // once actual installation resources are valid, before advertising an
        // executable Backend. No stored operation is advanced or retried.
        crate::lease_recovery::recover_idle(
            &self.root,
            &installed.manifest.installation_id,
            &installed.manifest.owner_sid,
        )?;
        *state = BindingState::Ready(installed.clone());
        Ok(installed)
    }
}

type LogPath = fn() -> Option<PathBuf>;

/// Where installation records go, named by Core when it starts: its own log, wherever that is
/// in this process. A process Core has not set up, such as this crate's tests, records nothing.
static LOG_PATH: std::sync::OnceLock<LogPath> = std::sync::OnceLock::new();

/// Core sends installation records to its own log; `path` says where that is when a record is
/// written.
pub fn route_log(path: LogPath) {
    let _ = LOG_PATH.set(path);
}

pub(crate) fn installation_log_path() -> Option<PathBuf> {
    LOG_PATH.get().and_then(|path| path())
}

/// One open's own stages, written by that open. A cache hit records no package time:
/// those numbers belong to the open that measured them, not to a later caller.
pub(crate) fn record_installation_open(
    stages: &InstallationStages,
    ok: bool,
    cache_hit: bool,
    installation_id: Option<&str>,
) {
    let Some(path) = installation_log_path() else {
        return;
    };
    let _ = write_installation_open_line(&path, stages, ok, cache_hit, installation_id);
}

pub(crate) fn write_installation_open_line(
    path: &Path,
    stages: &InstallationStages,
    ok: bool,
    cache_hit: bool,
    installation_id: Option<&str>,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let entry = serde_json::json!({
        "timestamp": aura_domain_common::UtcTimestamp::now(),
        "component": "windows-user",
        "event": "tool.installation_open",
        "fields": {
            "ok": ok,
            "cache_hit": cache_hit,
            "published_ms": if cache_hit { 0 } else { stages.published_ms },
            "package_ms": if cache_hit { 0 } else { stages.package_ms },
            "images_ms": if cache_hit { 0 } else { stages.images_ms },
            "cmd_ms": if cache_hit { 0 } else { stages.cmd_ms },
            "verify_ms": stages.verify_ms,
            "wait_ms": stages.wait_ms,
            "shared_failure": false,
            "installation_id": installation_id,
        },
    });
    let mut bytes = serde_json::to_vec(&entry)?;
    bytes.push(b'\n');
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(&bytes)?;
    Ok(())
}

fn record_shared_installation_failure(stages: &InstallationStages) {
    let Some(path) = installation_log_path() else {
        return;
    };
    let _ = write_shared_installation_failure(&path, stages);
}

pub(crate) fn write_shared_installation_failure(
    path: &Path,
    stages: &InstallationStages,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let entry = serde_json::json!({
        "timestamp": aura_domain_common::UtcTimestamp::now(),
        "component": "windows-user",
        "event": "tool.installation_open",
        "fields": {
            "ok": false,
            "cache_hit": false,
            "shared_failure": true,
            "published_ms": 0,
            "package_ms": 0,
            "images_ms": 0,
            "cmd_ms": 0,
            "verify_ms": 0,
            "wait_ms": stages.wait_ms,
        },
    });
    let mut bytes = serde_json::to_vec(&entry)?;
    bytes.push(b'\n');
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(&bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn failed_advertisement_is_not_cached_as_ready() {
        let root = tempfile::tempdir().expect("temporary installation");
        let binding = super::InstallationBinding::new(root.path().to_path_buf());
        assert!(binding.advertised().is_err());
        assert!(binding.current().is_err());
        assert!(binding.advertised().is_err());
        assert!(super::Installation::open_for_runner(root.path()).is_err());
    }
}
