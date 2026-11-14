//! Explicit file approval management uses the same V2 grants dispatch consumes.
use super::*;
use anyhow::bail;
use aura_core_base::append_core_log;
use aura_core_base::envelope::error_envelope;
use aura_permission_domain::{FileSessionConsentAvailability as A, FileSessionConsentView};
use aura_security_contract::{
    AuthorizationGrant, FileConsentTarget, file_entry_key, file_object_key,
};

impl P04Services {
    pub(super) fn file_consent_command(
        &self,
        method_name: &str,
        params: &serde_json::Value,
        correlation: CorrelationId,
    ) -> Result<serde_json::Value, IpcErrorEnvelope> {
        let work = || -> Result<serde_json::Value> {
            let journal = self
                .external_grants
                .bound_journal()
                .ok_or_else(|| anyhow!("permission.journal_unbound: Aura 的批准记录还没准备好（刚启动或正在恢复），这次没有完成；请稍后再试。"))?;
            let mut store = journal.lock()?;
            match method_name {
                method::FILE_SESSION_CONSENT_LIST => {
                    let input: aura_permission_domain::ListFileSessionConsentsInput =
                        serde_json::from_value(params.clone())?;
                    let views = aura_persistence_v2::file_consents::list(
                        store.database.connection(),
                        input.session_id,
                    )?
                    .into_iter()
                    .filter_map(view)
                    .filter(|v| input.include_inactive || v.availability == A::InForce)
                    .collect::<Vec<_>>();
                    Ok(serde_json::to_value(views)?)
                }
                method::FILE_SESSION_CONSENT_REVOKE => {
                    let input: aura_permission_domain::RevokeFileSessionConsentInput =
                        serde_json::from_value(params.clone())?;
                    store.database.unit_of_work(|tx| {
                        aura_persistence_v2::file_consents::revoke(
                            tx,
                            input.session_id,
                            input.grant_id,
                            input.expected_revision,
                            UtcTimestamp::now(),
                        )
                    })?;
                    let grant = aura_persistence_v2::security::grant(
                        store.database.connection(),
                        input.grant_id,
                    )?
                    .ok_or_else(|| anyhow!("permission.grant_state_changed: 没能确认这项文件批准的最新状态，请刷新列表核对。"))?;
                    drop(store);
                    if let Some(consent) = grant.scope.file_consent.as_ref() {
                        self.external_grants.withdraw_file_companions(consent, correlation)?;
                    }
                    Ok(serde_json::to_value(
                        view(grant).ok_or_else(|| anyhow!("permission.grant_state_changed: 没能确认这项文件批准的最新状态，请刷新列表核对。"))?,
                    )?)
                }
                _ => bail!(
                    "permission.unknown_consent_command: Aura Core 处理不了这个文件批准请求（Aura 内部出错），没有执行；请重启 Aura 后重试。"
                ),
            }
        };
        work().map_err(|error| error_envelope(correlation, error))
    }

    /// Lets the grant lists read the session's current turn and its file approvals from the agent
    /// journal, then withdraws the session file approvals that are out of their Workspace. A stop
    /// between a Workspace move and the withdrawal after it leaves such approvals in force in the
    /// store, though they cover nothing; every start binds the journal, so this finishes that
    /// withdrawal. A failure is logged and leaves them for the next start.
    pub fn bind_turn_journal(&self, journal: Arc<aura_core_broker::broker::BrokerJournal>) {
        self.external_grants.bind_turn_journal(journal);
        let correlation = CorrelationId::new();
        match self.external_grants.withdraw_out_of_workspace_file_approvals(None, correlation) {
            Ok(0) => {}
            Ok(count) => append_core_log(
                "security.moved_file_approvals.withdrawn",
                serde_json::json!({"correlation_id": correlation, "count": count}),
            ),
            Err(error) => append_core_log(
                "security.moved_file_approvals.kept",
                serde_json::json!({"correlation_id": correlation, "reason": error.to_string()}),
            ),
        }
    }

    /// Withdraws the file approvals a move left out of the Workspace, with their directory
    /// companions: those given at the folder it was at before. An approval follows a file's
    /// identity rather than its path, and a folder moved on its own drive keeps every identity;
    /// the approval already covers nothing at the new folder, and this makes the settings say so.
    /// The move has already committed when this runs; a failure is logged and leaves the
    /// approvals to the next start.
    pub(crate) fn on_workspace_relocated(
        &self,
        workspace: WorkspaceId,
        correlation: CorrelationId,
    ) {
        match self
            .external_grants
            .withdraw_out_of_workspace_file_approvals(Some(workspace), correlation)
        {
            Ok(count) => append_core_log(
                "security.workspace_relocated.file_approvals_withdrawn",
                serde_json::json!({
                    "correlation_id": correlation,
                    "workspace_id": workspace,
                    "count": count,
                }),
            ),
            Err(error) => append_core_log(
                "security.workspace_relocated.file_approvals_kept",
                serde_json::json!({
                    "correlation_id": correlation,
                    "workspace_id": workspace,
                    "reason": error.to_string(),
                }),
            ),
        }
    }
}

fn view(grant: AuthorizationGrant) -> Option<FileSessionConsentView> {
    let scope = grant.scope.file_consent?;
    if !scope.accepts_grant_revision(grant.revision) {
        return None;
    }
    let now = UtcTimestamp::now();
    let availability = if grant.revoked_at.is_some() {
        A::Revoked
    } else if grant.expires_at.is_some_and(|end| now >= end) {
        A::Expired
    } else if now < grant.issued_at {
        A::NotYetValid
    } else {
        scope.targets.iter().map(target_state).find(|s| *s != A::InForce).unwrap_or(A::InForce)
    };
    Some(FileSessionConsentView {
        grant_id: grant.grant_id,
        grant_revision: grant.revision,
        scope,
        availability,
        issued_at: grant.issued_at,
        expires_at: grant.expires_at,
        revoked_at: grant.revoked_at,
    })
}

fn target_state(target: &FileConsentTarget) -> A {
    let path = std::path::Path::new(&target.canonical_path);
    match aura_windows_platform::inspect_existing_path(path, true) {
        Ok(info) => {
            if file_object_key(&aura_workspace::domain_identity(&info.identity))
                == target.identity_key
            {
                A::InForce
            } else {
                A::TargetChanged
            }
        }
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                && target.identity_key.starts_with("entry:") =>
        {
            let Some(parent) = path.parent() else { return A::Unavailable };
            let Ok(parent) = aura_windows_platform::open_verified_existing(
                parent,
                aura_windows_platform::NativeFileAccess::Metadata,
            ) else {
                return A::Unavailable;
            };
            let Some(leaf) = path.file_name() else { return A::Unavailable };
            let leaf = leaf.to_string_lossy();
            let key = match aura_windows_platform::directory_case_rule(&parent) {
                aura_windows_platform::VolumeCaseRule::Sensitive => leaf.into_owned(),
                aura_windows_platform::VolumeCaseRule::InsensitivePreserving => {
                    let Some(map) = aura_windows_platform::directory_name_comparison(&parent)
                    else {
                        return A::Unavailable;
                    };
                    map.key(&leaf)
                }
                aura_windows_platform::VolumeCaseRule::Unknown => return A::Unavailable,
            };
            if file_entry_key(&aura_workspace::domain_identity(&parent.info().identity), &key)
                == target.identity_key
            {
                A::InForce
            } else {
                A::TargetChanged
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => A::TargetChanged,
        Err(_) => A::Unavailable,
    }
}
