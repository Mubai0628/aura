use super::{
    Ledger, Limits, Usage,
    ledger::{validate_usage, write_usage},
    storage,
};
use anyhow::{Result, ensure};
use rusqlite::{Connection, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::Path, sync::atomic::Ordering};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReconciledRequest {
    pub id: i64,
    /// Confirmed provider usage, or `null` when it cannot be confirmed: the request then stays
    /// charged at its whole reservation and is marked `charged_unknown`, never released.
    pub usage: Option<Usage>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Reconciliation {
    pub explanation: String,
    pub requests: Vec<ReconciledRequest>,
}

impl Ledger {
    /// Reopens with the original policy; recovery never raises any budget or deletes cost.
    pub fn open_for_reconciliation(path: &Path) -> Result<Self> {
        ensure!(path.is_file(), "eval.ledger_missing");
        let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let encoded: String = db.query_row("SELECT limits FROM policy", [], |r| r.get(0))?;
        let limits: Limits = serde_json::from_str(&encoded)?;
        drop(db);
        Self::open(path, limits)
    }
    pub fn reconcile(&self, note: &Reconciliation) -> Result<()> {
        ensure!(
            !note.explanation.trim().is_empty() && note.explanation.chars().count() <= 2000,
            "eval.reconciliation_explanation_required"
        );
        ensure!(
            crate::meter::sanitized_text(&note.explanation, 2000) == note.explanation,
            "eval.reconciliation_contains_sensitive_text"
        );
        let mut db = self.db.lock().map_err(|_| anyhow::anyhow!("eval.ledger_lock_failed"))?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure!(
            tx.query_row("SELECT halted FROM policy", [], |r| r.get::<_, bool>(0))?,
            "eval.ledger_not_halted"
        );
        let reason: String = tx.query_row("SELECT reason FROM stop_reason", [], |r| r.get(0))?;
        let outstanding = {
            let mut query =
                tx.prepare("SELECT id FROM requests WHERE state IN ('pending','unknown')")?;
            query.query_map([], |r| r.get::<_, i64>(0))?.collect::<Result<BTreeSet<_>, _>>()?
        };
        let provided = note.requests.iter().map(|r| r.id).collect::<BTreeSet<_>>();
        ensure!(
            provided == outstanding && provided.len() == note.requests.len(),
            "eval.reconciliation_must_cover_every_unknown_request"
        );
        for row in &note.requests {
            match row.usage {
                Some(usage) => {
                    validate_usage(usage)?;
                    write_usage(&tx, row.id, usage)?;
                }
                None => {
                    tx.execute(
                        "UPDATE requests SET state='charged_unknown' WHERE id=?1",
                        [row.id],
                    )?;
                }
            }
        }
        let total = storage::total(&tx, None)?;
        ensure!(total < self.limits.cumulative, "eval.cumulative_budget_cannot_be_recovered");
        tx.execute("INSERT INTO reconciliations(explanation,previous_reason,charged_tokens,created_at) VALUES(?1,?2,?3,strftime('%Y-%m-%dT%H:%M:%fZ','now'))",params![note.explanation,reason,storage::sql(total)?])?;
        tx.execute("DELETE FROM stop_reason", [])?;
        tx.execute("UPDATE policy SET halted=0", [])?;
        tx.commit()?;
        self.failed.store(false, Ordering::SeqCst);
        Ok(())
    }
}
