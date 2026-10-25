//! Explicit user changes: install with the exact successful report, enable, disable,
//! revoke trust and uninstall, each bound to the package revision the user saw.
use super::PackageServices;
use anyhow::Result;
use aura_domain_common::UtcTimestamp;
use aura_persistence_v2::packages::{self, lifecycle};
use aura_tool_domain::{
    PackageChangeRequestV1, PackageEnableRequestV1, PackageInstallRequestV1, PackageSummaryV1,
};

impl PackageServices {
    pub fn install(&self, request: &PackageInstallRequestV1) -> Result<PackageSummaryV1> {
        self.store()?.database.unit_of_work(|tx| {
            packages::install(
                tx,
                &request.version,
                request.expected_package_revision,
                &request.report_hash,
                UtcTimestamp::now(),
            )
        })?;
        self.summary(request.version.package_id)
    }

    pub fn set_enabled(&self, request: &PackageEnableRequestV1) -> Result<PackageSummaryV1> {
        self.store()?.database.unit_of_work(|tx| {
            lifecycle::set_enabled(
                tx,
                request.package_id,
                request.expected_package_revision,
                request.enabled,
                UtcTimestamp::now(),
            )
        })?;
        self.summary(request.package_id)
    }

    pub fn revoke_trust(&self, request: &PackageChangeRequestV1) -> Result<PackageSummaryV1> {
        self.store()?.database.unit_of_work(|tx| {
            lifecycle::revoke_trust(
                tx,
                request.package_id,
                request.expected_package_revision,
                UtcTimestamp::now(),
            )
        })?;
        self.summary(request.package_id)
    }

    /// Content still used by a running call or Conformance stays until that use ends;
    /// everything else is collected in the same transaction.
    pub fn uninstall(&self, request: &PackageChangeRequestV1) -> Result<PackageSummaryV1> {
        self.store()?.database.unit_of_work(|tx| {
            let at = UtcTimestamp::now();
            lifecycle::uninstall(tx, request.package_id, request.expected_package_revision, at)?;
            lifecycle::collect_unreferenced(tx, at)
        })?;
        self.summary(request.package_id)
    }

    /// Collects content whose last reference was released. Startup and the end of a
    /// run call this; a failure leaves content for the next attempt, never deletes more.
    pub fn collect(&self) -> Result<usize> {
        Ok(self
            .store()?
            .database
            .unit_of_work(|tx| lifecycle::collect_unreferenced(tx, UtcTimestamp::now()))?)
    }
}
