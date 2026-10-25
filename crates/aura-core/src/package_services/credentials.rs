//! D3: the user's explicit choice of credential for one HTTP package tool slot. The
//! credential must exist for this exact tool slot, be configured at the chosen
//! revision and allow the tool's exact origin; every use re-checks the same facts.
use super::PackageServices;
use anyhow::{Result, ensure};
use aura_domain_common::UtcTimestamp;
use aura_persistence_v2::packages;
use aura_tool_domain::*;

impl PackageServices {
    pub fn bind_credential(
        &self,
        request: &PackageCredentialBindRequestV1,
    ) -> Result<PackageSummaryV1> {
        let slot =
            self.credential_slot(request.package_id, &request.tool_key, &request.slot_key)?;
        let record = self
            .persistence
            .list_managed_tool_credentials_v1(slot.target.clone())?
            .into_iter()
            .find(|record| record.credential_id == request.credential_id)
            .ok_or_else(|| anyhow::anyhow!("package.credential_not_found: 这个凭据不属于该工具"))?;
        ensure!(
            record.state == ToolCredentialStateV1::Configured
                && record.credential_revision == request.credential_revision,
            "package.credential_changed: 凭据已经变化，请刷新后重新选择"
        );
        ensure!(
            record.allowed_origins.binary_search(&slot.origin).is_ok(),
            "package.credential_origin_denied: 这个凭据的允许网站不包含工具的地址"
        );
        self.store()?
            .database
            .unit_of_work(|tx| packages::credentials::bind(tx, request, UtcTimestamp::now()))?;
        self.summary(request.package_id)
    }

    pub fn clear_credential(
        &self,
        request: &PackageCredentialClearRequestV1,
    ) -> Result<PackageSummaryV1> {
        self.store()?.database.unit_of_work(|tx| {
            packages::credentials::clear(
                tx,
                request.package_id,
                &request.tool_key,
                &request.slot_key,
                request.expected_binding_revision,
                UtcTimestamp::now(),
            )
        })?;
        self.summary(request.package_id)
    }

    fn credential_slot(
        &self,
        package: aura_domain_common::ToolPackageId,
        tool: &aura_tool_contract::ToolKey,
        slot: &aura_tool_contract::CredentialSlotKey,
    ) -> Result<PackageCredentialSlotViewV1> {
        let summary = self.summary(package)?;
        let installed = summary
            .installed
            .filter(|installed| installed.trusted)
            .ok_or_else(|| anyhow::anyhow!("package.not_installed"))?;
        installed
            .tools
            .into_iter()
            .find(|candidate| &candidate.tool_key == tool)
            .and_then(|candidate| candidate.credential)
            .filter(|candidate| &candidate.slot_key == slot)
            .ok_or_else(|| {
                anyhow::anyhow!("package.credential_slot_missing: 这个工具没有声明该凭据")
            })
    }
}
