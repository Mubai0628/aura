//! One vocabulary for "which backend can run this call, and if none can, why".
//!
//! Readiness projection, selection, preparation and final dispatch all describe
//! the same facts about the same installation, so they have to spell them the
//! same way. Layer-local string guessing is what previously turned a concrete
//! `runner.core_binding_mismatch.path` into a generic `runtime_unavailable` and
//! left the Native fallback unexplained. Every producer and consumer of a
//! backend refusal therefore shares [`BackendFailure`], and the codes below are
//! the stable public spelling of each one.
//!
//! Two rules make this safe to use on an executable boundary:
//!
//! * a classification never carries a path, script, token or credential — only a
//!   bounded ASCII code and facts that are already public to the projection,
//! * classification is by *prefix and structure* of a stable code, never by
//!   matching free-form text, so a new detail can be added without silently
//!   changing which class an existing failure lands in.
//!
//! The scope distinction matters as much as the class. `Availability` means one
//! backend cannot run this call and another may; `Integrity` means this Core is
//! not the process the installation is bound to, and the ordinary backend must
//! refuse before a runner starts instead of falling through to a broader one.

use std::fmt;

/// Which layer observed the failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendFailureStage {
    /// Reading or verifying the published installation.
    Installation,
    /// Comparing this Core process with the installation's Core binding.
    CoreBinding,
    /// Checking the dedicated offline/online account and its network policy.
    Account,
    /// Comparing a call's requirement with what a backend offers.
    Capability,
    /// Permission policy refused the call.
    Policy,
    /// The backend refused after selection, before a runner started.
    Dispatch,
}

/// What kind of fact the failure is, independent of which layer found it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendFailureKind {
    /// Nothing is installed yet, so Setup is the next step.
    InstallationMissing,
    /// Something is published, but its manifest, record, package or image no
    /// longer verifies. Repair is the next step.
    InstallationDamaged,
    /// The running Core is not the file object, owner or hash the installation
    /// binds. This is an identity boundary, not a reason to pick another backend.
    CoreBindingMismatch,
    /// The running Core's own identity could not be read at all. Still an
    /// identity boundary: an unreadable answer is not a match.
    CoreIdentityUnreadable,
    /// The dedicated account, its network policy, or the maintenance resource
    /// does not currently verify.
    AccountOrNetwork,
    /// The backend cannot honor what this Operation requires.
    CapabilityIncompatible,
    /// Policy refused the Operation itself.
    PolicyDenied,
    /// The backend refused after selection for a reason that is not an identity
    /// boundary, for example a scope or preparation change.
    DispatchRefused,
}

impl BackendFailureKind {
    /// Whether this failure forbids every ordinary backend for the call.
    ///
    /// Identity boundaries are never "this backend is unavailable, try a broader
    /// one". They stop the call and are reported as a repair of the execution
    /// install.
    #[must_use]
    pub const fn is_identity_boundary(self) -> bool {
        matches!(self, Self::CoreBindingMismatch | Self::CoreIdentityUnreadable)
    }

    /// Whether a healthy backend of another class could still run this call.
    #[must_use]
    pub const fn is_backend_availability(self) -> bool {
        matches!(
            self,
            Self::InstallationMissing
                | Self::InstallationDamaged
                | Self::AccountOrNetwork
                | Self::CapabilityIncompatible
        )
    }

    /// The stable public code prefix for this class.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::InstallationMissing => "backend.installation_missing",
            Self::InstallationDamaged => "backend.installation_damaged",
            Self::CoreBindingMismatch => "backend.core_binding_mismatch",
            Self::CoreIdentityUnreadable => "backend.core_identity_unreadable",
            Self::AccountOrNetwork => "backend.account_or_network",
            Self::CapabilityIncompatible => "backend.capability_incompatible",
            Self::PolicyDenied => "backend.policy_denied",
            Self::DispatchRefused => "backend.dispatch_refused",
        }
    }
}

/// A classified backend refusal: the class plus the exact bounded detail.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackendFailure {
    pub stage: BackendFailureStage,
    pub kind: BackendFailureKind,
    /// The raw stable code this classification came from. Already bounded and
    /// already free of paths and secrets by construction.
    pub detail: String,
}

impl BackendFailure {
    #[must_use]
    pub fn new(
        stage: BackendFailureStage,
        kind: BackendFailureKind,
        detail: impl Into<String>,
    ) -> Self {
        Self { stage, kind, detail: detail.into() }
    }

    /// The projection-facing code: class plus the original detail suffix.
    #[must_use]
    pub fn code(&self) -> String {
        let detail = self.detail.trim();
        if detail.is_empty() {
            return self.kind.code().to_owned();
        }
        format!("{}.{}", self.kind.code(), detail)
    }
}

impl fmt::Display for BackendFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.code())
    }
}

/// Classifies a `runner.*` code produced by installation verification, the Core
/// binding handshake, or the runner handshake.
///
/// This is the single place the `runner.` codes are interpreted, so the readiness
/// card, the selector, preparation and dispatch cannot disagree about what a
/// given installation state means.
#[must_use]
pub fn classify_runner_code(reason: &str) -> BackendFailure {
    let detail = reason.strip_prefix("runner.").unwrap_or(reason);
    // The Core identity handshake reports `core_binding_mismatch.<fact>` and
    // `core_binding_unreadable`; both are identity boundaries even when the fact
    // is only "I could not read it".
    if let Some(rest) = detail.strip_prefix("core_binding_mismatch") {
        return BackendFailure::new(
            BackendFailureStage::CoreBinding,
            BackendFailureKind::CoreBindingMismatch,
            rest.trim_start_matches('.'),
        );
    }
    if let Some(rest) = detail.strip_prefix("core_binding_unreadable") {
        return BackendFailure::new(
            BackendFailureStage::CoreBinding,
            BackendFailureKind::CoreIdentityUnreadable,
            rest.trim_start_matches('.'),
        );
    }
    // Legacy spelling of the same two facts. `core_owner_mismatch` was a
    // distinct code and is a binding mismatch, not an unreadable identity.
    if detail.starts_with("core_file_mismatch") || detail == "core_owner_mismatch" {
        return BackendFailure::new(
            BackendFailureStage::CoreBinding,
            BackendFailureKind::CoreBindingMismatch,
            detail.strip_prefix("core_file_mismatch.").unwrap_or(detail),
        );
    }
    if detail.starts_with("core_image_capture") {
        return BackendFailure::new(
            BackendFailureStage::CoreBinding,
            BackendFailureKind::CoreIdentityUnreadable,
            detail,
        );
    }
    if detail.starts_with("core_process_sid") {
        return BackendFailure::new(
            BackendFailureStage::CoreBinding,
            BackendFailureKind::CoreIdentityUnreadable,
            detail,
        );
    }
    // Account, network policy and the maintenance resource.
    if detail.starts_with("account_")
        || detail.starts_with("online_setup_required")
        || detail.starts_with("account_modes_mixed")
        || detail.starts_with("direct_network")
        || detail.starts_with("wfp_")
        || detail.starts_with("maintenance_")
        || detail.starts_with("setup_lock")
    {
        return BackendFailure::new(
            BackendFailureStage::Account,
            BackendFailureKind::AccountOrNetwork,
            detail,
        );
    }
    // Publication exists but does not verify. Everything here means Repair, not
    // Setup: an `execution.json` is present and is failing to describe a usable
    // installation.
    if detail.starts_with("install_")
        || detail.starts_with("resource_")
        || detail.starts_with("package_")
        || detail.starts_with("image_")
        || detail.starts_with("runtime_")
        || detail.starts_with("system_cmd")
        || detail.starts_with("invalid_cmd_image")
        || detail.starts_with("aliased_file")
        || detail.starts_with("expected_file")
        || detail.starts_with("relative_path")
        || detail.starts_with("reparse_path")
        || detail.starts_with("path_changed")
        || detail.starts_with("file_binding_changed")
    {
        return BackendFailure::new(
            BackendFailureStage::Installation,
            BackendFailureKind::InstallationDamaged,
            detail,
        );
    }
    if detail.starts_with("unsupported_or_unready")
        || detail.starts_with("network_mode")
        || detail.starts_with("readonly_scope")
        || detail.starts_with("credential_use")
    {
        return BackendFailure::new(
            BackendFailureStage::Capability,
            BackendFailureKind::CapabilityIncompatible,
            detail,
        );
    }
    BackendFailure::new(BackendFailureStage::Dispatch, BackendFailureKind::DispatchRefused, detail)
}

/// Classifies a readiness answer directly, without going through a string code.
///
/// Readiness already knows which fact it is; this keeps that knowledge instead of
/// round-tripping it through text and re-guessing at the call site.
#[must_use]
pub fn classify_readiness(
    readiness: &crate::BackendReadiness,
    installation_published: bool,
) -> Option<BackendFailure> {
    match readiness {
        crate::BackendReadiness::Ready => None,
        crate::BackendReadiness::SetupRequired { reason } => {
            if installation_published {
                Some(BackendFailure::new(
                    BackendFailureStage::Installation,
                    BackendFailureKind::InstallationDamaged,
                    reason.as_str(),
                ))
            } else {
                Some(BackendFailure::new(
                    BackendFailureStage::Installation,
                    BackendFailureKind::InstallationMissing,
                    reason.as_str(),
                ))
            }
        }
        crate::BackendReadiness::RepairRequired { reason } => {
            let classified = classify_runner_code(reason);
            // A repair-required answer that is really an identity boundary keeps
            // that class; everything else is a damaged publication.
            if classified.kind.is_identity_boundary() {
                Some(classified)
            } else {
                Some(BackendFailure::new(
                    BackendFailureStage::Installation,
                    BackendFailureKind::InstallationDamaged,
                    reason.as_str(),
                ))
            }
        }
        crate::BackendReadiness::Unavailable { reason } => Some(classify_runner_code(reason)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BackendReadiness;

    #[test]
    fn identity_boundaries_stay_identity_boundaries() {
        for reason in [
            "runner.core_binding_mismatch.path",
            "runner.core_file_mismatch.file_id",
            "runner.core_owner_mismatch",
            "runner.core_binding_unreadable",
            "runner.core_image_capture.inspect.os:5",
            "runner.core_process_sid.os:5",
        ] {
            let failure = classify_runner_code(reason);
            assert!(
                failure.kind.is_identity_boundary(),
                "{reason} must forbid every ordinary backend"
            );
            assert_eq!(failure.stage, BackendFailureStage::CoreBinding, "{reason}");
            assert!(!failure.code().contains('\\'), "{reason}");
            assert!(!failure.code().contains("C:"), "{reason}");
        }
    }

    #[test]
    fn installation_damage_is_availability_not_an_identity_boundary() {
        for reason in [
            "runner.install_version",
            "runner.resource_record_changed",
            "runner.runtime_package_changed",
            "runner.runtime_location_changed",
            "runner.image_hash_missing",
            "runner.system_cmd_serviced",
            "runner.file_binding_changed",
        ] {
            let failure = classify_runner_code(reason);
            assert_eq!(failure.kind, BackendFailureKind::InstallationDamaged, "{reason}");
            assert!(failure.kind.is_backend_availability(), "{reason}");
            assert!(!failure.kind.is_identity_boundary(), "{reason}");
        }
    }

    #[test]
    fn account_and_network_failures_are_their_own_class() {
        for reason in [
            "runner.online_setup_required",
            "runner.account_identity",
            "runner.account_record_changed",
            "runner.account_modes_mixed",
            "runner.direct_network_policy_mismatch",
            "runner.wfp_weight_missing",
            "runner.direct_network_repair_required",
            "runner.maintenance_config_changed",
        ] {
            let failure = classify_runner_code(reason);
            assert_eq!(failure.kind, BackendFailureKind::AccountOrNetwork, "{reason}");
            assert_eq!(failure.stage, BackendFailureStage::Account, "{reason}");
        }
    }

    #[test]
    fn a_missing_publication_is_setup_and_a_present_one_is_repair() {
        let missing = classify_readiness(
            &BackendReadiness::SetupRequired {
                reason: "runner.production_installation_required".into(),
            },
            false,
        )
        .expect("setup");
        assert_eq!(missing.kind, BackendFailureKind::InstallationMissing);

        let damaged = classify_readiness(
            &BackendReadiness::SetupRequired { reason: "runner.install_version".into() },
            true,
        )
        .expect("damaged");
        assert_eq!(damaged.kind, BackendFailureKind::InstallationDamaged);

        assert!(
            classify_readiness(&BackendReadiness::Ready, true).is_none(),
            "ready is not a failure"
        );
    }

    #[test]
    fn the_code_keeps_the_specific_detail() {
        let failure = classify_runner_code("runner.core_binding_mismatch.file_id");
        assert_eq!(failure.code(), "backend.core_binding_mismatch.file_id");
        let bare = classify_runner_code("runner.");
        assert_eq!(bare.code(), "backend.dispatch_refused");
    }
}
