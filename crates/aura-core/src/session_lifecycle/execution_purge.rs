//! Deleting a Session's or a removed Workspace's execution-store rows without holding the store.
//!
//! The plan is read on a connection of its own, so its long reads hold neither the writer nor
//! the shared reader. The writer deletes in steps of about [`STEP`], and other requests are let in
//! between steps; a request waits for one step at most. Rows written after the plan was read, or
//! that something else still pointed at, are found by planning again.
use super::purger::Purger;
use crate::agent_store::{StoreAccess, StoreStep};
use crate::*;
use aura_domain_common::{SessionId, WorkspaceId};
use aura_persistence_v2::session_purge::{self, PurgePlan};
use std::time::Duration;

/// How long one write step may hold the store before it lets other requests in.
const STEP: Duration = Duration::from_millis(40);
/// The pause between steps, so requests that waited for the writer get it first.
const BETWEEN_STEPS: Duration = Duration::from_millis(10);
/// Plans read before giving up for this run; the next start tries again.
const ROUNDS: usize = 4;

#[derive(Clone, Copy)]
pub(super) enum Target {
    Session(SessionId),
    Workspace(WorkspaceId),
}

impl Purger {
    pub(super) async fn purge_rows(&self, target: Target) -> anyhow::Result<()> {
        let path = self.broker.database_path()?;
        for _ in 0..ROUNDS {
            let reading = path.clone();
            let mut plan = tokio::task::spawn_blocking(move || -> anyhow::Result<PurgePlan> {
                let reader = aura_persistence_core::open_reader(&reading)?;
                Ok(match target {
                    Target::Session(session) => session_purge::plan_session(&reader, session)?,
                    Target::Workspace(workspace) => {
                        session_purge::plan_workspace(&reader, workspace)?
                    }
                })
            })
            .await??;
            while !plan.done() {
                let broker = Arc::clone(&self.broker);
                plan = self
                    .store
                    .run_critical(
                        StoreStep::operation("session.purge.step"),
                        StoreAccess::Write,
                        move || {
                            let mut store = broker.lock()?;
                            session_purge::step(&mut store.database, &mut plan, STEP)?;
                            Ok(plan)
                        },
                    )
                    .await?;
                tokio::time::sleep(BETWEEN_STEPS).await;
            }
            if plan.skipped() == 0 {
                return Ok(());
            }
        }
        anyhow::bail!("session.purge_incomplete: 还有记录被其他记录引用，下次启动时再删")
    }
}
