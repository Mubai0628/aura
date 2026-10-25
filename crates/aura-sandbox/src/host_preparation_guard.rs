//! Pre-write guard for the Host Preparation broker plan.
//!
//! The broker writes unowned resources: the `C:\` DACL and, when a plan says so,
//! the NUL device's security descriptor. A whole-descriptor replacement is the
//! dangerous one — it deletes whatever it does not name, and among the things it
//! does not name is the capability ACE Aura itself installs for a published
//! installation.
//!
//! The rule this module enforces: **the process that writes is the process that
//! checks**. The plan arrives from another process, so "Core already verified the
//! installation" is not evidence this process may act on; the identity is
//! re-verified here, read-only, and the descriptor that is compared is the one
//! present at the moment of the write. A refusal is a refusal: it happens before
//! the first write, so it can honestly claim that nothing was modified.
//!
//! Everything here is decided over [`HostPreparationResources`], so a test can
//! count the writes a refused plan did *not* perform — real Win32 calls cannot
//! show that, and "we refused it" without a write count is an assumption.
use std::path::Path;

use aura_permission_domain::{
    HostPreparationAction, HostPreparationBrokerPlan, HostPreparationBrokerResult,
};
use aura_windows_platform::{
    DaclTransitionV1, SystemDriveCompatibilityChanges, prepare_null_device_compatibility,
    prepare_system_drive_compatibility_with_transition, probe_windows_sandbox,
    remove_system_drive_compatibility_with_transition, restore_null_device_security_descriptor,
};

use crate::{SandboxError, path_from_canonical};

/// Whether the NUL device satisfies the readiness rule an AppContainer launch needs.
///
/// One rule for every gate: the frozen requirements plus, for a *verified*
/// published installation, exactly its capability ACE. A candidate that did not
/// verify answers "not ready" — a launch gate may not turn "I could not check"
/// into "go", and must not fall back to replacing the descriptor either.
pub(crate) fn host_preparation_nul_ready() -> bool {
    aura_windows_platform::installation_identity::probe_null_device_readiness().is_ready()
}

/// What a plan would write to the NUL device, if anything.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NullDeviceWrite<'a> {
    /// The plan writes no NUL descriptor.
    None,
    /// The frozen compatibility descriptor: a whole-descriptor replacement.
    Compatibility,
    /// A previously frozen original descriptor: a rollback.
    Frozen(&'a str),
}

impl<'a> NullDeviceWrite<'a> {
    /// The descriptor this write installs, if it writes at all.
    const fn replacement(self) -> Option<&'a str> {
        match self {
            Self::None => None,
            Self::Compatibility => Some(aura_windows_platform::NULL_DEVICE_COMPATIBILITY_SDDL),
            Self::Frozen(sddl) => Some(sddl),
        }
    }
}

/// What a frozen plan would do to the NUL device.
fn planned_null_device_write(plan: &HostPreparationBrokerPlan) -> NullDeviceWrite<'_> {
    match plan.action {
        HostPreparationAction::Prepare | HostPreparationAction::Repair
            if plan.update_null_device_security =>
        {
            NullDeviceWrite::Compatibility
        }
        HostPreparationAction::Remove => plan
            .ownership_before
            .original_null_device_sddl
            .as_deref()
            .map_or(NullDeviceWrite::None, NullDeviceWrite::Frozen),
        HostPreparationAction::Prepare | HostPreparationAction::Repair => NullDeviceWrite::None,
    }
}

/// The facts the pre-write guard decides on, gathered in the writing process.
#[derive(Clone, Debug)]
struct NullDeviceGuard {
    identity: aura_windows_platform::installation_identity::InstallationIdentity,
    current_descriptor: String,
}

/// Gather the guard facts, in this process, from the current host.
///
/// A plan that writes no NUL descriptor needs no facts, so the common
/// `C:\`-only plan reads nothing here.
fn gather_null_device_guard(
    plan: &HostPreparationBrokerPlan,
) -> Result<Option<NullDeviceGuard>, SandboxError> {
    use aura_windows_platform::installation_identity::{
        published_installations_base, verify_installation_identity,
    };
    if planned_null_device_write(plan).replacement().is_none() {
        return Ok(None);
    }
    let base = published_installations_base().map_err(|error| {
        SandboxError::HostPreparationCapabilityConflict(format!(
            "the installation base is unavailable: {error}"
        ))
    })?;
    let current_descriptor = aura_windows_platform::capture_null_device_security_descriptor()
        .map_err(SandboxError::Platform)?;
    Ok(Some(NullDeviceGuard { identity: verify_installation_identity(&base), current_descriptor }))
}

/// Refuse a plan that would drop a verified Aura capability ACE.
///
/// This is a decision about one write, made from the descriptor that is present
/// now and the identity this process verified itself. Three outcomes are
/// allowed: no published installation (nothing to protect), a verified
/// installation whose ACE the replacement keeps, and a plan that writes no NUL
/// descriptor at all. Everything else — including "a candidate exists but did not
/// verify" — is a refusal, because an unknown must never be read as permission.
fn ensure_capability_preserved(
    write: NullDeviceWrite<'_>,
    guard: Option<&NullDeviceGuard>,
) -> Result<(), SandboxError> {
    let Some(replacement) = write.replacement() else {
        return Ok(());
    };
    let Some(guard) = guard else {
        return Err(SandboxError::HostPreparationCapabilityConflict(
            "the plan writes the NUL device but no guard facts were gathered".to_owned(),
        ));
    };
    match guard.identity.null_capability() {
        Ok(None) => Ok(()),
        Err(reason) => Err(SandboxError::HostPreparationCapabilityConflict(reason)),
        Ok(Some(_)) => match guard
            .identity
            .replacement_drops_capability(&guard.current_descriptor, replacement)
        {
            Ok(false) => Ok(()),
            Ok(true) => {
                let capability = guard
                    .identity
                    .null_capability()
                    .ok()
                    .flatten()
                    .map_or_else(|| "unknown".to_owned(), |ace| ace.sid);
                Err(SandboxError::HostPreparationCapabilityConflict(format!(
                    "the plan would replace the NUL device descriptor and delete the capability ACE {capability} \
                     of a verified published installation; nothing was written (current descriptor: {}, planned replacement: {replacement})",
                    guard.current_descriptor
                )))
            }
            Err(reason) => Err(SandboxError::HostPreparationCapabilityConflict(reason)),
        },
    }
}

/// The resource writes one broker plan performs, behind a seam.
///
/// A refused plan must write *nothing*, and real Win32 calls cannot show that:
/// this seam lets a test count the writes a refusal did not perform.
trait HostPreparationResources {
    fn elevated(&self) -> bool;
    /// Applies the planned ACEs and reports whether the verified write lost the
    /// kernel-owned auto-inherited marker (the one approved difference).
    fn prepare_system_drive(
        &mut self,
        root: &Path,
        requested: SystemDriveCompatibilityChanges,
    ) -> std::io::Result<(SystemDriveCompatibilityChanges, bool)>;
    /// Revokes the owned ACEs and reports the same marker outcome.
    fn remove_system_drive(
        &mut self,
        root: &Path,
        changes: SystemDriveCompatibilityChanges,
    ) -> std::io::Result<(SystemDriveCompatibilityChanges, bool)>;
    fn write_null_device(&mut self, write: NullDeviceWrite<'_>) -> std::io::Result<()>;
}

/// The real host: elevation is probed, writes are the platform calls.
struct PlatformHostPreparationResources;

impl HostPreparationResources for PlatformHostPreparationResources {
    fn elevated(&self) -> bool {
        probe_windows_sandbox().elevated_process
    }

    fn prepare_system_drive(
        &mut self,
        root: &Path,
        requested: SystemDriveCompatibilityChanges,
    ) -> std::io::Result<(SystemDriveCompatibilityChanges, bool)> {
        let (changes, transition) =
            prepare_system_drive_compatibility_with_transition(root, requested)?;
        Ok((changes, transition == DaclTransitionV1::ApprovedAutoInheritedMarker))
    }

    fn remove_system_drive(
        &mut self,
        root: &Path,
        changes: SystemDriveCompatibilityChanges,
    ) -> std::io::Result<(SystemDriveCompatibilityChanges, bool)> {
        let (changes, transition) =
            remove_system_drive_compatibility_with_transition(root, changes)?;
        Ok((changes, transition == DaclTransitionV1::ApprovedAutoInheritedMarker))
    }

    fn write_null_device(&mut self, write: NullDeviceWrite<'_>) -> std::io::Result<()> {
        match write {
            NullDeviceWrite::None => Ok(()),
            NullDeviceWrite::Compatibility => prepare_null_device_compatibility(),
            NullDeviceWrite::Frozen(sddl) => restore_null_device_security_descriptor(sddl),
        }
    }
}

pub(crate) fn apply_host_preparation_broker_plan(
    plan: &HostPreparationBrokerPlan,
) -> Result<HostPreparationBrokerResult, SandboxError> {
    // The guard runs before the elevation check on purpose: a refused plan must be
    // refused as a refusal — not disguised as a privilege problem — and must write
    // nothing even in a process that is already elevated.
    let guard = gather_null_device_guard(plan)?;
    let mut resources = PlatformHostPreparationResources;
    apply_host_preparation_broker_writes(plan, guard.as_ref(), &mut resources)
}

/// These errors are classified as pre-write refusals on the helper wire. Keep
/// every pure plan consistency check here, before the first resource write;
/// rollback cannot establish that no write occurred (in particular, AI may be lost).
fn validate_plan_consistency(plan: &HostPreparationBrokerPlan) -> Result<(), SandboxError> {
    match plan.action {
        HostPreparationAction::Prepare | HostPreparationAction::Repair => {
            if plan.update_null_device_security
                && plan.ownership_before.original_null_device_sddl.is_none()
            {
                return Err(SandboxError::InvalidHostPreparationPlan(
                    "NUL update has no frozen original security descriptor".to_owned(),
                ));
            }
        }
        HostPreparationAction::Remove => {
            if plan.add_all_application_packages_ace
                != plan.ownership_before.all_application_packages_ace_owned
                || plan.add_all_restricted_application_packages_ace
                    != plan.ownership_before.all_restricted_application_packages_ace_owned
                || plan.update_null_device_security
                    != plan.ownership_before.original_null_device_sddl.is_some()
            {
                return Err(SandboxError::InvalidHostPreparationPlan(
                    "remove plan does not exactly match Aura-owned compatibility changes"
                        .to_owned(),
                ));
            }
        }
    }
    Ok(())
}

/// Guard, then write, over explicit facts and an explicit resource target.
fn apply_host_preparation_broker_writes(
    plan: &HostPreparationBrokerPlan,
    guard: Option<&NullDeviceGuard>,
    resources: &mut impl HostPreparationResources,
) -> Result<HostPreparationBrokerResult, SandboxError> {
    validate_plan_consistency(plan)?;
    let null_write = planned_null_device_write(plan);
    ensure_capability_preserved(null_write, guard)?;
    if !resources.elevated() {
        return Err(SandboxError::HostPreparationRequiresElevation);
    }
    let root = path_from_canonical(&plan.system_drive_root)?;
    let mut ownership_after = plan.ownership_before.clone();
    ownership_after.system_drive_root = Some(plan.system_drive_root.clone());
    // The one approved descriptor difference the verified writes may leave behind is
    // carried out of the guard as an outcome, so the caller's evidence can state it
    // instead of implying a byte-for-byte restoration.
    let mut approved_auto_inherited_marker_lost = false;
    match plan.action {
        HostPreparationAction::Prepare | HostPreparationAction::Repair => {
            let requested = SystemDriveCompatibilityChanges {
                all_application_packages_added: plan.add_all_application_packages_ace,
                all_restricted_application_packages_added: plan
                    .add_all_restricted_application_packages_ace,
            };
            let (changes, marker_lost) =
                resources.prepare_system_drive(&root, requested).map_err(SandboxError::Platform)?;
            approved_auto_inherited_marker_lost |= marker_lost;
            ownership_after.all_application_packages_ace_owned |=
                changes.all_application_packages_added;
            ownership_after.all_restricted_application_packages_ace_owned |=
                changes.all_restricted_application_packages_added;
            if plan.update_null_device_security
                && let Err(error) = resources.write_null_device(null_write)
            {
                let rollback = resources.remove_system_drive(&root, changes);
                if let Err(rollback_error) = rollback {
                    return Err(SandboxError::CleanupFailed(format!(
                        "{error}; rollback failed: {rollback_error}"
                    )));
                }
                return Err(SandboxError::Platform(error));
            }
        }
        HostPreparationAction::Remove => {
            if let Some(original) = &plan.ownership_before.original_null_device_sddl {
                resources
                    .write_null_device(NullDeviceWrite::Frozen(original))
                    .map_err(SandboxError::Platform)?;
                ownership_after.original_null_device_sddl = None;
            }
            let (_removed, marker_lost) = resources
                .remove_system_drive(
                    &root,
                    SystemDriveCompatibilityChanges {
                        all_application_packages_added: plan
                            .ownership_before
                            .all_application_packages_ace_owned,
                        all_restricted_application_packages_added: plan
                            .ownership_before
                            .all_restricted_application_packages_ace_owned,
                    },
                )
                .map_err(SandboxError::Platform)?;
            approved_auto_inherited_marker_lost |= marker_lost;
            ownership_after.all_application_packages_ace_owned = false;
            ownership_after.all_restricted_application_packages_ace_owned = false;
            ownership_after.system_drive_root = None;
        }
    }
    Ok(HostPreparationBrokerResult {
        operation_id: plan.operation_id,
        succeeded: true,
        ownership_after,
        error_code: None,
        approved_auto_inherited_marker_lost,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use aura_domain_common::SecretNonce;

    use super::*;
    use crate::tests::test_host_preparation_plan;

    /// A resource target that records every write instead of performing it.
    ///
    /// The real calls cannot show that a refused plan wrote nothing, so the
    /// refusal counter-examples are decided over this double: the count is the
    /// evidence.
    #[derive(Default)]
    struct CountingResources {
        elevated: bool,
        writes: Vec<String>,
        system_drive_changes: SystemDriveCompatibilityChanges,
        /// What the double reports for the verified write: a real targeted write on an
        /// auto-inherited object loses the marker, and the guard has to carry that out.
        marker_lost: bool,
        fail_after_drive_write: bool,
        fail_null_write: bool,
        fail_revoke: bool,
    }

    impl CountingResources {
        fn elevated() -> Self {
            Self { elevated: true, ..Self::default() }
        }

        fn writes(&self) -> usize {
            self.writes.len()
        }
    }

    impl HostPreparationResources for CountingResources {
        fn elevated(&self) -> bool {
            self.elevated
        }

        fn prepare_system_drive(
            &mut self,
            _root: &Path,
            requested: SystemDriveCompatibilityChanges,
        ) -> std::io::Result<(SystemDriveCompatibilityChanges, bool)> {
            self.writes.push("system_drive".to_owned());
            self.system_drive_changes = requested;
            if self.fail_after_drive_write {
                return Err(std::io::Error::other(
                    "host_compatibility_unapproved_control_transition",
                ));
            }
            Ok((requested, self.marker_lost))
        }

        fn remove_system_drive(
            &mut self,
            _root: &Path,
            changes: SystemDriveCompatibilityChanges,
        ) -> std::io::Result<(SystemDriveCompatibilityChanges, bool)> {
            self.writes.push("system_drive_rollback".to_owned());
            if self.fail_revoke {
                return Err(std::io::Error::other("revoke failed after write"));
            }
            Ok((changes, self.marker_lost))
        }

        fn write_null_device(&mut self, write: NullDeviceWrite<'_>) -> std::io::Result<()> {
            self.writes.push(format!("null_device:{:?}", write.replacement().unwrap_or("none")));
            if self.fail_null_write {
                return Err(std::io::Error::other("NUL write failed"));
            }
            Ok(())
        }
    }

    fn verified_installation_identity()
    -> (aura_windows_platform::installation_identity::InstallationIdentity, String) {
        use aura_windows_platform::installation_identity::{
            InstallationIdentity, VerifiedInstallation,
        };
        let installation_id = "073773f9-1798-4a46-a6d1-b8d5c9e3d19a";
        let sid = aura_windows_platform::null_capability_sid(installation_id).expect("derive");
        let identity = InstallationIdentity::Verified(VerifiedInstallation {
            root: PathBuf::from("C:/unused-by-the-guard-test"),
            installation_id: installation_id.to_owned(),
            null_capability_sid: sid.clone(),
        });
        (identity, sid)
    }

    fn guard_with(
        identity: aura_windows_platform::installation_identity::InstallationIdentity,
        current_descriptor: &str,
    ) -> NullDeviceGuard {
        NullDeviceGuard { identity, current_descriptor: current_descriptor.to_owned() }
    }

    #[test]
    fn a_missing_frozen_nul_original_is_refused_before_drive_write_or_rollback() {
        use aura_windows_platform::installation_identity::InstallationIdentity;
        let guard = guard_with(InstallationIdentity::Absent, "D:(A;;FA;;;SY)");
        for action in [HostPreparationAction::Prepare, HostPreparationAction::Repair] {
            let mut plan = test_host_preparation_plan(&SecretNonce::from_bytes([43; 32]));
            plan.action = action;
            plan.update_null_device_security = true;
            assert!(plan.ownership_before.original_null_device_sddl.is_none());
            plan.plan_hash = crate::host_preparation_plan_hash(&plan).expect("plan hash");
            let mut resources = CountingResources::elevated();
            resources.marker_lost = true;
            let error = apply_host_preparation_broker_writes(&plan, Some(&guard), &mut resources)
                .expect_err("missing original is a plan error, before any write");
            assert_eq!(
                error.pre_write_refusal_code(),
                Some("sandbox.host_preparation_plan_invalid")
            );
            assert_eq!(
                resources.writes(),
                0,
                "even a successful rollback would not mean zero writes"
            );
        }
    }

    #[test]
    fn remove_plan_ownership_mismatches_are_all_refused_before_writes() {
        for mismatch in 0..3 {
            let mut plan = test_host_preparation_plan(&SecretNonce::from_bytes([44; 32]));
            plan.action = HostPreparationAction::Remove;
            plan.add_all_application_packages_ace = mismatch == 0;
            plan.add_all_restricted_application_packages_ace = mismatch == 1;
            plan.update_null_device_security = mismatch == 2;
            plan.plan_hash = crate::host_preparation_plan_hash(&plan).expect("plan hash");
            let mut resources = CountingResources::elevated();
            let error = apply_host_preparation_broker_writes(&plan, None, &mut resources)
                .expect_err("remove may only revoke the frozen ownership");
            assert_eq!(
                error.pre_write_refusal_code(),
                Some("sandbox.host_preparation_plan_invalid")
            );
            assert_eq!(resources.writes(), 0);
        }
    }

    #[test]
    fn post_write_verification_and_rollback_failures_never_become_pre_write_refusals() {
        use aura_windows_platform::installation_identity::InstallationIdentity;
        let guard = guard_with(InstallationIdentity::Absent, "D:(A;;FA;;;SY)");
        // First: the platform wrote the drive, then failed read-back verification.
        let mut plan = test_host_preparation_plan(&SecretNonce::from_bytes([45; 32]));
        let mut resources = CountingResources::elevated();
        resources.fail_after_drive_write = true;
        let error = apply_host_preparation_broker_writes(&plan, None, &mut resources)
            .expect_err("post-write verification failed");
        assert!(matches!(error, SandboxError::Platform(_)));
        assert_eq!(error.pre_write_refusal_code(), None);
        assert_eq!(resources.writes(), 1, "no automatic retry or post-verification rollback");

        // Next: NUL fails after a successful drive write; whether the existing
        // cleanup succeeds or fails, the operation has already performed writes.
        plan.update_null_device_security = true;
        plan.ownership_before.original_null_device_sddl = Some("D:(A;;FA;;;SY)".to_owned());
        plan.plan_hash = crate::host_preparation_plan_hash(&plan).expect("plan hash");
        for fail_revoke in [false, true] {
            let mut resources = CountingResources::elevated();
            resources.marker_lost = true;
            resources.fail_null_write = true;
            resources.fail_revoke = fail_revoke;
            let error = apply_host_preparation_broker_writes(&plan, Some(&guard), &mut resources)
                .expect_err("a NUL write failure cannot report a successful operation");
            assert_eq!(error.pre_write_refusal_code(), None);
            assert_eq!(matches!(error, SandboxError::CleanupFailed(_)), fail_revoke);
            assert_eq!(resources.writes(), 3);
        }

        // Revoke itself may also write and then fail; it is not a plan refusal.
        plan.action = HostPreparationAction::Remove;
        plan.update_null_device_security = false;
        plan.ownership_before.original_null_device_sddl = None;
        plan.ownership_before.all_application_packages_ace_owned = true;
        plan.ownership_before.all_restricted_application_packages_ace_owned = true;
        plan.plan_hash = crate::host_preparation_plan_hash(&plan).expect("plan hash");
        let mut resources = CountingResources::elevated();
        resources.fail_revoke = true;
        let error = apply_host_preparation_broker_writes(&plan, None, &mut resources)
            .expect_err("post-write revoke failure");
        assert_eq!(error.pre_write_refusal_code(), None);
        assert_eq!(resources.writes(), 1);
    }

    #[test]
    fn a_legacy_plan_that_would_delete_a_verified_capability_writes_nothing() {
        // The plan a build without identity awareness generates: replace the NUL
        // descriptor with the frozen constant. The installation whose capability
        // ACE is on the device is verified, and the constant does not carry it.
        let (identity, sid) = verified_installation_identity();
        let current = format!("O:BAG:SYD:(A;;FA;;;SY)(A;;0x12019f;;;{sid})S:(ML;;NW;;;LW)");
        let guard = guard_with(identity, &current);
        let mut plan = test_host_preparation_plan(&SecretNonce::from_bytes([7; 32]));
        plan.update_null_device_security = true;
        plan.ownership_before.original_null_device_sddl = Some(current.clone());
        plan.plan_hash = crate::host_preparation_plan_hash(&plan).expect("plan hash");

        let mut resources = CountingResources::elevated();
        let outcome = apply_host_preparation_broker_writes(&plan, Some(&guard), &mut resources);

        match outcome {
            Err(SandboxError::HostPreparationCapabilityConflict(reason)) => {
                assert!(reason.contains(&sid), "{reason}");
                assert!(reason.contains("nothing was written"), "{reason}");
            }
            other => panic!("a capability-deleting replacement must be refused, got {other:?}"),
        }
        assert_eq!(resources.writes(), 0, "a refused plan must write nothing at all");
        assert_eq!(
            SandboxError::HostPreparationCapabilityConflict(String::new()).pre_write_refusal_code(),
            Some("sandbox.host_preparation_capability_conflict"),
            "the refusal must be reportable as a pre-write refusal"
        );
    }

    #[test]
    fn an_accepted_plan_writes_exactly_what_it_declares() {
        // The counterpart to the zero-write test: the same seam does count the
        // writes an accepted plan performs, so "zero" there is not vacuous.
        //
        // The accepted NUL write is a rollback whose frozen original already
        // carried the capability ACE: restoring it keeps the capability, so it is
        // allowed. (A strict compatibility write never is, while that ACE exists —
        // the frozen constant does not contain it.)
        let (identity, sid) = verified_installation_identity();
        let current = format!(
            "O:BAG:SYD:(A;;GRGWGX;;;WD)(A;;FA;;;SY)(A;;FA;;;BA)(A;;GRGX;;;RC)(A;;GRGWGX;;;AC)(A;;GRGWGX;;;S-1-15-2-2)(A;;0x12019f;;;{sid})S:(ML;;NW;;;LW)"
        );
        let guard = guard_with(identity, &current);
        let mut plan = test_host_preparation_plan(&SecretNonce::from_bytes([9; 32]));
        plan.action = HostPreparationAction::Remove;
        plan.add_all_application_packages_ace = false;
        plan.add_all_restricted_application_packages_ace = false;
        plan.update_null_device_security = true;
        plan.ownership_before.all_application_packages_ace_owned = false;
        plan.ownership_before.all_restricted_application_packages_ace_owned = false;
        plan.ownership_before.original_null_device_sddl = Some(current);
        plan.plan_hash = crate::host_preparation_plan_hash(&plan).expect("plan hash");

        let mut resources = CountingResources::elevated();
        let result = apply_host_preparation_broker_writes(&plan, Some(&guard), &mut resources)
            .expect("a rollback that keeps the capability ACE must be allowed");

        assert!(result.succeeded);
        assert_eq!(resources.writes(), 2, "{:?}", resources.writes);
        assert!(resources.writes[0].starts_with("null_device:"), "{:?}", resources.writes);
        assert_eq!(resources.writes[1], "system_drive_rollback");
        assert!(result.ownership_after.original_null_device_sddl.is_none());
    }

    #[test]
    fn a_rollback_that_would_drop_the_capability_writes_nothing() {
        // A remove plan restores the descriptor frozen before Aura's own change.
        // If the capability ACE appeared after that freeze, restoring it would
        // delete the capability, so the rollback is refused too.
        let (identity, sid) = verified_installation_identity();
        let current = format!("O:BAG:SYD:(A;;FA;;;SY)(A;;0x12019f;;;{sid})S:(ML;;NW;;;LW)");
        let guard = guard_with(identity, &current);
        let mut plan = test_host_preparation_plan(&SecretNonce::from_bytes([11; 32]));
        plan.action = HostPreparationAction::Remove;
        plan.add_all_application_packages_ace = false;
        plan.add_all_restricted_application_packages_ace = false;
        plan.update_null_device_security = true;
        plan.ownership_before.all_application_packages_ace_owned = false;
        plan.ownership_before.all_restricted_application_packages_ace_owned = false;
        plan.ownership_before.original_null_device_sddl =
            Some("O:BAG:SYD:(A;;FA;;;SY)S:(ML;;NW;;;LW)".to_owned());
        plan.plan_hash = crate::host_preparation_plan_hash(&plan).expect("plan hash");

        let mut resources = CountingResources::elevated();
        let outcome = apply_host_preparation_broker_writes(&plan, Some(&guard), &mut resources);

        assert!(
            matches!(outcome, Err(SandboxError::HostPreparationCapabilityConflict(_))),
            "{outcome:?}"
        );
        assert_eq!(resources.writes(), 0);
    }

    #[test]
    fn an_unverifiable_installation_refuses_a_nul_plan_and_leaves_the_rest_alone() {
        use aura_windows_platform::installation_identity::InstallationIdentity;
        // A candidate exists but did not verify: no answer about the capability,
        // so no NUL write may be authorised, and the refusal must not be mistaken
        // for "there is nothing to protect".
        let guard = guard_with(
            InstallationIdentity::Unverified("record.json is unreadable".to_owned()),
            "O:BAG:SYD:(A;;FA;;;SY)S:(ML;;NW;;;LW)",
        );
        let mut nul_plan = test_host_preparation_plan(&SecretNonce::from_bytes([13; 32]));
        nul_plan.update_null_device_security = true;
        nul_plan.ownership_before.original_null_device_sddl =
            Some("O:BAG:SYD:(A;;FA;;;SY)S:(ML;;NW;;;LW)".to_owned());
        nul_plan.plan_hash = crate::host_preparation_plan_hash(&nul_plan).expect("plan hash");

        let mut resources = CountingResources::elevated();
        let outcome = apply_host_preparation_broker_writes(&nul_plan, Some(&guard), &mut resources);
        match outcome {
            Err(SandboxError::HostPreparationCapabilityConflict(reason)) => {
                assert!(reason.contains("record.json is unreadable"), "{reason}");
            }
            other => panic!("an unverifiable installation must refuse a NUL write, got {other:?}"),
        }
        assert_eq!(resources.writes(), 0);

        // The same unverifiable identity does not block a plan that writes no NUL
        // descriptor: there is no capability question to answer there.
        let mut drive_only = test_host_preparation_plan(&SecretNonce::from_bytes([17; 32]));
        drive_only.update_null_device_security = false;
        drive_only.plan_hash = crate::host_preparation_plan_hash(&drive_only).expect("plan hash");
        let mut resources = CountingResources::elevated();
        apply_host_preparation_broker_writes(&drive_only, Some(&guard), &mut resources)
            .expect("a system-drive-only plan needs no capability verdict");
        assert_eq!(resources.writes(), 1, "{:?}", resources.writes);
    }

    #[test]
    fn an_absent_installation_may_still_replace_the_descriptor() {
        use aura_windows_platform::installation_identity::InstallationIdentity;
        // No published installation: there is genuinely no Aura capability to
        // protect, and the strict first-preparation path stays available.
        let guard = guard_with(
            InstallationIdentity::Absent,
            "O:BAG:SYD:(A;;FA;;;SY)(A;;0x12019f;;;S-1-5-21-1-2-3-4)S:(ML;;NW;;;LW)",
        );
        let mut plan = test_host_preparation_plan(&SecretNonce::from_bytes([19; 32]));
        plan.update_null_device_security = true;
        plan.ownership_before.original_null_device_sddl =
            Some("O:BAG:SYD:(A;;FA;;;SY)S:(ML;;NW;;;LW)".to_owned());
        plan.plan_hash = crate::host_preparation_plan_hash(&plan).expect("plan hash");

        let mut resources = CountingResources::elevated();
        apply_host_preparation_broker_writes(&plan, Some(&guard), &mut resources)
            .expect("first preparation has no capability to preserve");
        assert_eq!(resources.writes(), 2, "{:?}", resources.writes);
    }

    #[test]
    fn the_guard_runs_before_the_elevation_check_and_before_every_write() {
        // Two orderings in one test: an unelevated process still gets the refusal
        // (not a privilege error), and an allowed plan is refused by the elevation
        // check without writing anything first.
        let (identity, sid) = verified_installation_identity();
        let current = format!("O:BAG:SYD:(A;;FA;;;SY)(A;;0x12019f;;;{sid})S:(ML;;NW;;;LW)");
        let guard = guard_with(identity, &current);

        let mut dangerous = test_host_preparation_plan(&SecretNonce::from_bytes([23; 32]));
        dangerous.update_null_device_security = true;
        dangerous.ownership_before.original_null_device_sddl = Some(current.clone());
        dangerous.plan_hash = crate::host_preparation_plan_hash(&dangerous).expect("plan hash");
        let mut resources = CountingResources::default();
        assert!(matches!(
            apply_host_preparation_broker_writes(&dangerous, Some(&guard), &mut resources),
            Err(SandboxError::HostPreparationCapabilityConflict(_))
        ));
        assert_eq!(resources.writes(), 0);

        let mut allowed = test_host_preparation_plan(&SecretNonce::from_bytes([29; 32]));
        allowed.update_null_device_security = false;
        allowed.plan_hash = crate::host_preparation_plan_hash(&allowed).expect("plan hash");
        let mut resources = CountingResources::default();
        assert!(matches!(
            apply_host_preparation_broker_writes(&allowed, Some(&guard), &mut resources),
            Err(SandboxError::HostPreparationRequiresElevation)
        ));
        assert_eq!(resources.writes(), 0, "the elevation check precedes every write");
    }

    #[test]
    fn the_approved_marker_difference_is_carried_out_of_the_guard_as_evidence() {
        // The verified write reports one allowed difference; the guard has to hand it to
        // the caller's evidence instead of dropping it, and a path that wrote nothing
        // must not claim it.
        let plan = {
            let mut plan = test_host_preparation_plan(&SecretNonce::from_bytes([41; 32]));
            plan.update_null_device_security = false;
            plan.plan_hash = crate::host_preparation_plan_hash(&plan).expect("plan hash");
            plan
        };

        let mut with_difference = CountingResources::elevated();
        with_difference.marker_lost = true;
        let result = apply_host_preparation_broker_writes(&plan, None, &mut with_difference)
            .expect("the allowed plan succeeds over the double");
        assert!(
            result.approved_auto_inherited_marker_lost,
            "the one approved difference has to reach the broker result"
        );

        let mut without_difference = CountingResources::elevated();
        without_difference.marker_lost = false;
        let result = apply_host_preparation_broker_writes(&plan, None, &mut without_difference)
            .expect("the allowed plan succeeds over the double");
        assert!(
            !result.approved_auto_inherited_marker_lost,
            "an unchanged control field must not be reported as a difference"
        );

        // A refusal precedes every write, so it can never carry the difference.
        let (identity, sid) = verified_installation_identity();
        let current = format!("O:BAG:SYD:(A;;FA;;;SY)(A;;0x12019f;;;{sid})S:(ML;;NW;;;LW)");
        let guard = guard_with(identity, &current);
        let mut dangerous = test_host_preparation_plan(&SecretNonce::from_bytes([42; 32]));
        dangerous.update_null_device_security = true;
        dangerous.ownership_before.original_null_device_sddl = Some(current.clone());
        dangerous.plan_hash = crate::host_preparation_plan_hash(&dangerous).expect("plan hash");
        let mut resources = CountingResources::elevated();
        resources.marker_lost = true;
        assert!(matches!(
            apply_host_preparation_broker_writes(&dangerous, Some(&guard), &mut resources),
            Err(SandboxError::HostPreparationCapabilityConflict(_))
        ));
        assert_eq!(resources.writes(), 0, "a refused plan writes nothing");
    }

    #[test]
    fn a_plan_that_writes_nul_without_gathered_facts_is_refused() {
        // The guard facts are gathered from this process; a caller that skips that
        // step must not be able to write the descriptor unguarded.
        let mut plan = test_host_preparation_plan(&SecretNonce::from_bytes([31; 32]));
        plan.update_null_device_security = true;
        plan.ownership_before.original_null_device_sddl =
            Some("O:BAG:SYD:(A;;FA;;;SY)S:(ML;;NW;;;LW)".to_owned());
        plan.plan_hash = crate::host_preparation_plan_hash(&plan).expect("plan hash");

        let mut resources = CountingResources::elevated();
        assert!(matches!(
            apply_host_preparation_broker_writes(&plan, None, &mut resources),
            Err(SandboxError::HostPreparationCapabilityConflict(_))
        ));
        assert_eq!(resources.writes(), 0);
    }

    #[test]
    fn the_gathered_facts_come_from_this_machine() {
        // Real fixture, read-only: the guard's own gathering path must reach the
        // host and produce a decision consistent with what is actually there. On
        // any machine this test says something true: a replacement is refused
        // exactly when a verified capability ACE is present and would be dropped.
        let mut plan = test_host_preparation_plan(&SecretNonce::from_bytes([37; 32]));
        plan.update_null_device_security = true;
        plan.ownership_before.original_null_device_sddl =
            Some("O:BAG:SYD:(A;;FA;;;SY)S:(ML;;NW;;;LW)".to_owned());
        plan.plan_hash = crate::host_preparation_plan_hash(&plan).expect("plan hash");
        let guard = gather_null_device_guard(&plan)
            .expect("gathering facts is read-only and must succeed")
            .expect("a NUL plan gathers facts");
        assert!(
            guard.current_descriptor.contains("D:"),
            "the captured descriptor must be a real one: {}",
            guard.current_descriptor
        );
        let decision = ensure_capability_preserved(NullDeviceWrite::Compatibility, Some(&guard));
        let capability = guard.identity.null_capability();
        match capability {
            Ok(Some(_)) => assert!(
                matches!(decision, Err(SandboxError::HostPreparationCapabilityConflict(_))),
                "a verified capability ACE present now forbids the strict replacement"
            ),
            Ok(None) => assert!(
                decision.is_ok(),
                "with no published installation there is no capability to protect"
            ),
            Err(_) => assert!(
                matches!(decision, Err(SandboxError::HostPreparationCapabilityConflict(_))),
                "an unverifiable candidate must refuse the replacement"
            ),
        }
        // A plan that writes no NUL descriptor gathers nothing at all.
        let mut drive_only = test_host_preparation_plan(&SecretNonce::from_bytes([41; 32]));
        drive_only.update_null_device_security = false;
        drive_only.plan_hash = crate::host_preparation_plan_hash(&drive_only).expect("plan hash");
        assert!(
            gather_null_device_guard(&drive_only).expect("read-only").is_none(),
            "a system-drive-only plan needs no host facts"
        );
    }
}
