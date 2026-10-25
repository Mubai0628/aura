//! Revalidate live authority while retaining the original narrower execution plan.
use super::port::CorePort;
use anyhow::{Result, ensure};
use aura_backend_windows_user::{ScopeFacts, WindowsUserCorePort};
use aura_execution_contract::{DispatchRequest, OperationNetwork};
use aura_security_contract::AuthorizationBasis;
use aura_windows_user_contract::WindowsUserPreparation;

impl CorePort {
    pub(super) fn retained_scope(&self, request: &DispatchRequest) -> Result<ScopeFacts> {
        let prepared: WindowsUserPreparation =
            serde_json::from_value(request.plan.prepared.preparation.clone())?;
        let live = self.scope(&request.operation)?;
        let store = self.journal.lock()?;
        let authority = aura_persistence_v2::execution::dispatch_authority(
            store.database.connection(),
            request,
            aura_domain_common::UtcTimestamp::now(),
        )?;
        let c = &request.operation.content;
        match authority.basis {
            AuthorizationBasis::PermissionMode { .. } => {
                ensure!(
                    aura_permission_engine::mode_ceiling::permits(
                        c.identity.actor,
                        live.mode,
                        &c.capabilities,
                        c.network != OperationNetwork::None,
                        false
                    ),
                    "command.mode_changed"
                );
                ensure!(
                    prepared.write_roots.iter().all(|root| live.write_roots.contains(root)),
                    "command.scope_changed"
                );
            }
            AuthorizationBasis::Grant { .. } => {
                if let Some(consent) = &authority.scope.consent {
                    ensure!(
                        consent.lifetime == aura_security_contract::ShellConsentLifetime::Session
                            || consent.session_permission_revision == live.revision,
                        "command.native_consent_changed"
                    );
                }
                ensure!(
                    prepared.write_roots.iter().all(|root| live.read_roots.contains(root)),
                    "command.scope_changed"
                );
            }
            AuthorizationBasis::Composite { slices } => {
                for slice in slices {
                    if matches!(
                        slice.authority,
                        aura_security_contract::CapabilityAuthority::PermissionMode { .. }
                    ) {
                        ensure!(
                            aura_security_contract::ordinary_mode_covers_capability(
                                live.mode,
                                &c.source,
                                &c.action_kind,
                                slice.capability,
                                &c.resources
                            ),
                            "command.mode_changed"
                        );
                    }
                    // Store checked immutable Grant slices above; D2 does not grow their scope.
                }
            }
            _ => anyhow::bail!("command.authority_not_supported"),
        }
        ensure!(live.read_roots == prepared.read_roots, "command.scope_changed");
        Ok(ScopeFacts {
            mode: prepared.mode,
            revision: prepared.permission_revision,
            read_roots: prepared.read_roots,
            write_roots: prepared.write_roots,
        })
    }
}
