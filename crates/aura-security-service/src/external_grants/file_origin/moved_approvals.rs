//! Session file approvals that are out of their Workspace: answered at another folder than the one
//! the Workspace is at now, or recorded before approvals named their folder in a Workspace past its
//! first authorization epoch. They cover nothing already (`file_consent_in_its_workspace`); this
//! withdraws them, with their directory companions, so the settings say so too.
//!
//! A move commits in `aura.db` and the approvals live in the V2 store, so the withdrawal after a
//! move can be lost to a stop in between. The withdrawal is therefore idempotent and also runs
//! once the journal is bound at start, and it withdraws the companions before the approvals: a
//! stop between the two leaves approvals in force, which the next run finds again, rather than
//! companions whose approval it can no longer see.
use super::*;
use anyhow::{Context, Result};
use aura_security_contract::{
    ApprovalWorkspace, AuthorizationGrant, FileSessionConsentRef, file_consent_in_its_workspace,
};
use std::collections::BTreeMap;

#[cfg(test)]
mod tests;

impl ExternalGrantService {
    /// Withdraws the session file approvals that are out of their Workspace, in `only` or in
    /// every Workspace, and returns how many it withdrew.
    pub fn withdraw_out_of_workspace_file_approvals(
        &self,
        only: Option<WorkspaceId>,
        correlation: CorrelationId,
    ) -> Result<usize> {
        let journal = self.bound_journal().context("permission.journal_unbound: 批准记录还没准备好，搬走的工作区里的文件批准这次没有撤回；下次启动 Aura 时会再撤回。")?;
        let live = {
            let store = journal.lock().map_err(|_| anyhow::anyhow!("permission.journal_lost: 批准记录不可用（Aura 内部出错），搬走的工作区里的文件批准这次没有撤回；下次启动 Aura 时会再撤回。"))?;
            let db = store.database.connection();
            let mut live = Vec::new();
            for grant in aura_persistence_v2::file_consents::live(db)? {
                let Some(consent) = grant.scope.file_consent.as_ref() else { continue };
                let Some(session) =
                    aura_persistence_v2::workspace::session(db, consent.session_id)?
                else {
                    continue;
                };
                if only.is_none_or(|workspace| workspace == session.workspace_id) {
                    live.push((grant, session.workspace_id));
                }
            }
            live
        };
        // Both stores are read one after the other, never with the journal held.
        let mut workspaces = BTreeMap::new();
        for (_, id) in &live {
            if workspaces.contains_key(id) {
                continue;
            }
            if let Ok(workspace) = self.persistence.workspace(*id) {
                workspaces.insert(
                    *id,
                    ApprovalWorkspace {
                        canonical_root: workspace.canonical_root,
                        authorization_epoch: workspace.authorization_epoch,
                    },
                );
            }
        }
        self.withdraw_file_approvals(out_of_workspace(live, &workspaces), correlation)
    }

    fn withdraw_file_approvals(
        &self,
        approvals: Vec<(AuthorizationGrant, FileSessionConsentRef)>,
        correlation: CorrelationId,
    ) -> Result<usize> {
        if approvals.is_empty() {
            return Ok(0);
        }
        for (_, consent) in &approvals {
            self.withdraw_file_companions(consent, correlation)?;
        }
        let journal = self.bound_journal().context("permission.journal_unbound: 批准记录还没准备好，搬走的工作区里的文件批准这次没有撤回；下次启动 Aura 时会再撤回。")?;
        let mut store = journal.lock().map_err(|_| anyhow::anyhow!("permission.journal_lost: 批准记录不可用（Aura 内部出错），搬走的工作区里的文件批准这次没有撤回；下次启动 Aura 时会再撤回。"))?;
        let now = UtcTimestamp::now();
        let mut withdrawn = 0;
        for (grant, consent) in approvals {
            let revoked = store.database.unit_of_work(|tx| {
                aura_persistence_v2::file_consents::revoke(
                    tx,
                    consent.session_id,
                    grant.grant_id,
                    grant.revision,
                    now,
                )
            });
            match revoked {
                Ok(()) => withdrawn += 1,
                // One that changed since it was read, as by its own withdrawal, is left for the
                // next run to read again.
                Err(aura_persistence_core::PersistenceCoreError::Conflict) => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(withdrawn)
    }
}

/// The approvals out of their Workspace as Core holds it. One whose Workspace Core cannot read
/// now is left as it is: the rule at use time already refuses it.
fn out_of_workspace(
    live: Vec<(AuthorizationGrant, WorkspaceId)>,
    workspaces: &BTreeMap<WorkspaceId, ApprovalWorkspace>,
) -> Vec<(AuthorizationGrant, FileSessionConsentRef)> {
    live.into_iter()
        .filter_map(|(grant, workspace)| {
            let consent = grant.scope.file_consent.clone()?;
            let workspace = workspaces.get(&workspace)?;
            (!file_consent_in_its_workspace(&consent, Some(workspace))).then_some((grant, consent))
        })
        .collect()
}
