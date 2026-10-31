use aura_core_base::append_core_log;
use aura_domain_common::jcs::canonical_sha256;
use aura_domain_common::{ArtifactId, UtcTimestamp};
use aura_execution_contract::*;
use aura_persistence_core::{PersistenceCoreError as Error, artifact, operation};
use aura_persistence_v2::{V2Store, execution};
use aura_security_contract::AuthorizationDecision;
use std::{
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
};

/// The process's one V2 write authority. Other Core owners that must write the same
/// database (the package Conformance owner) share this exact mutex; none opens a
/// second product writer.
pub struct BrokerJournal(
    Arc<Mutex<V2Store>>,
    Arc<aura_memory::MemoryService>,
    super::store_check::StoreCheck,
);

/// How one dispatched command ended, as the ledger is allowed to record it.
///
/// THREE, NOT TWO. A handler that answered with the Core's own "outcome unknown" envelope did
/// not fail: it may have committed, and recording `Failed` would state the opposite. `Unknown`
/// records the attempt as terminal-with-an-unproven-end instead, which keeps a retry decision
/// from being made on a result that was never established.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionVerdict {
    Succeeded,
    Failed,
    Unknown,
}

impl CompletionVerdict {
    #[must_use]
    const fn outcome(self) -> ExecutionOutcome {
        match self {
            Self::Succeeded => ExecutionOutcome::Succeeded,
            Self::Failed => ExecutionOutcome::Failed,
            Self::Unknown => ExecutionOutcome::Unknown,
        }
    }

    #[must_use]
    const fn termination(self, evidence_ref: ArtifactId) -> TerminationEvidence {
        match self {
            // Only a real answer confirms the attempt's end. An unknown outcome leaves the
            // termination unproven, which is exactly what a reader must be told.
            Self::Succeeded | Self::Failed => TerminationEvidence::Confirmed { evidence_ref },
            Self::Unknown => TerminationEvidence::Unknown,
        }
    }
}

impl BrokerJournal {
    pub fn open(root: &Path) -> anyhow::Result<Self> {
        let store = if root.exists() { V2Store::open(root)? } else { V2Store::create(root)? };
        let memory = crate::memory_services::open(
            root.parent().ok_or_else(|| anyhow::anyhow!("memory.data_directory"))?,
        )?;
        let check =
            super::store_check::StoreCheck::start(store.pending_check(), store.database_path());
        Ok(Self(Arc::new(Mutex::new(store)), memory, check))
    }

    /// This process is exiting normally (Main asked). The next open owes only the quick
    /// check, unless this one is not known to be sound: then it owes the full check. Says
    /// whether the normal exit was recorded.
    pub fn prepare_exit(&self) -> anyhow::Result<bool> {
        let store = self.lock()?;
        let marked = self.2.record_exit(&store)?;
        append_core_log("store_clean_exit", serde_json::json!({ "marked": marked }));
        Ok(marked)
    }
    /// The background check found the Store damaged; Core's status says so.
    pub fn store_damaged(&self) -> bool {
        self.2.damaged()
    }
    pub fn memory(&self) -> &Arc<aura_memory::MemoryService> {
        &self.1
    }
    /// The same writer, for an owner that needs it beyond this journal's API.
    pub fn shared_store(&self) -> Arc<Mutex<V2Store>> {
        Arc::clone(&self.0)
    }
    pub fn lock(&self) -> Result<MutexGuard<'_, V2Store>, Error> {
        self.0.lock().map_err(|_| Error::Invalid("broker store lock"))
    }

    /// The database file this journal owns, so a reader can open the same authoritative data
    /// read-only instead of queueing behind a write transaction.
    pub fn database_path(&self) -> anyhow::Result<std::path::PathBuf> {
        Ok(self.lock()?.database_path())
    }
    pub fn reserve(
        &self,
        dispatch: &AuthorizedDispatch,
        authority: &AuthorizationDecision,
        key: &str,
    ) -> anyhow::Result<()> {
        let r = dispatch.request();
        let now = UtcTimestamp::now();
        self.lock()?.database.unit_of_work(|tx| {
            let created = operation::begin(
                tx,
                &operation::OperationDraft {
                    id: r.operation.operation_id,
                    domain: "core_broker",
                    kind: &r.operation.content.action_kind,
                    request_key: key,
                    version: 1,
                    payload: &serde_json::to_value(&r.operation)?,
                    at: now,
                },
            )?;
            if created != operation::BeginOperation::Created {
                return Err(Error::Conflict);
            }
            // A Core Broker dispatch is authorized by the live authenticated Main
            // channel for one Operation, so it carries no reusable consent and has
            // no session permission revision to compare against.
            execution::prepare(tx, dispatch, authority, 0, now)?;
            Ok::<_, Error>(())
        })?;
        Ok(())
    }
    pub fn claim(&self, request: &DispatchRequest) -> anyhow::Result<()> {
        let a = &request.binding.attempt;
        self.lock()?.database.unit_of_work(|tx| {
            let current = execution::load(tx, a.operation_id, a.ordinal.get())?;
            if current.binding != request.binding {
                return Err(Error::Conflict);
            }
            execution::mark_dispatched(tx, a.operation_id, a.ordinal.get(), UtcTimestamp::now())
        })?;
        Ok(())
    }
    pub fn complete(
        &self,
        request: &DispatchRequest,
        evidence: BrokerEvidence,
        verdict: CompletionVerdict,
    ) -> anyhow::Result<()> {
        self.complete_noting(request, evidence, verdict, &[])
    }

    /// [`Self::complete`] with registered issue codes the backend observed itself.
    pub fn complete_noting(
        &self,
        request: &DispatchRequest,
        evidence: BrokerEvidence,
        verdict: CompletionVerdict,
        notes: &[&str],
    ) -> anyhow::Result<()> {
        let id = ArtifactId::new();
        let now = UtcTimestamp::now();
        let side_effect = evidence.side_effect;
        let result = ExecutionResult {
            outcome: verdict.outcome(),
            side_effect,
            receipt: ExecutionReceipt {
                attempt: request.binding.attempt.clone(),
                evidence_ref: id,
                postcondition_fingerprint: (evidence.after != serde_json::Value::Null)
                    .then(|| canonical_sha256(&evidence.after))
                    .transpose()?,
                rollback: match side_effect {
                    SideEffectStateV1::Compensated => {
                        RollbackEvidence::Completed { evidence_ref: id }
                    }
                    SideEffectStateV1::NoEffect => RollbackEvidence::NotApplicable,
                    _ => RollbackEvidence::Unavailable,
                },
            },
            termination: verdict.termination(id),
            completed_at: now,
            // An unproven end is a warning the attempt carries, alongside the existing
            // "the side effect was not verified" marker: the ledger states what it does not
            // know instead of leaving a reader to infer it from a missing field.
            warnings: match (verdict, side_effect) {
                (CompletionVerdict::Unknown, _) => {
                    vec!["tool.outcome.unsettled".into()]
                }
                (_, SideEffectStateV1::Unknown) => vec!["tool.side_effect.unverified".into()],
                _ => vec![],
            }
            .into_iter()
            .chain(notes.iter().map(|note| (*note).to_owned()))
            .collect(),
        };
        let attempt = &request.binding.attempt;
        self.lock()?.database.unit_of_work(|tx| {
            artifact::put(
                tx,
                id,
                &attempt.operation_id.to_string(),
                "application/vnd.aura.broker-evidence+json",
                &serde_json::to_vec(&evidence)?,
                now,
            )?;
            execution::complete(tx, attempt.operation_id, attempt.ordinal.get(), &result)
        })?;
        Ok(())
    }
    pub fn reconcile(&self, attempt: AttemptRef) -> anyhow::Result<ReconcileResult> {
        let store = self.lock()?;
        Ok(
            match execution::result(
                store.database.connection(),
                attempt.operation_id,
                attempt.ordinal.get(),
            )? {
                Some(result) => ReconcileResult::Completed { result: Box::new(result) },
                // Never infer completion or retry a claim from the absence of a Result.
                None => ReconcileResult::Unknown { attempt },
            },
        )
    }
    pub fn verify_handle(&self, handle: &ExecutionHandle) -> anyhow::Result<()> {
        let store = self.lock()?;
        let current = execution::load(
            store.database.connection(),
            handle.attempt.operation_id,
            handle.attempt.ordinal.get(),
        )?;
        anyhow::ensure!(
            canonical_sha256(&current.binding)? == handle.dispatch_fingerprint,
            "dispatch handle mismatch"
        );
        Ok(())
    }
}
