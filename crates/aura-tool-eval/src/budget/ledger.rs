use super::{Limits, RequestRecord, Usage, storage, types::FlowStop};
use anyhow::{Result, bail, ensure};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::{
    path::Path,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use storage::{number, optional_number, sql};

type SettlementRow = (u64, u64, String, Option<u64>, Option<u64>, Option<u64>);

/// Metadata only. Payloads, provider headers and credential bytes have no storage API.
pub struct Ledger {
    pub(super) db: Mutex<Connection>,
    pub(super) limits: Limits,
    pub(super) failed: AtomicBool,
}
impl Ledger {
    pub fn open(path: &Path, limits: Limits) -> Result<Self> {
        ensure!(
            limits.cumulative <= super::BudgetStage::MAX_CUMULATIVE && limits.cumulative > 0,
            "eval.invalid_cumulative_budget"
        );
        ensure!(
            limits.task > 0 && limits.request_input > 0 && limits.request_output > 0,
            "eval.invalid_limits"
        );
        let db = Connection::open(path)?;
        storage::initialize(&db, limits)?;
        Ok(Self { db: Mutex::new(db), limits, failed: AtomicBool::new(false) })
    }
    fn checked<T>(&self, operation: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T> {
        ensure!(!self.failed.load(Ordering::SeqCst), "eval.ledger_failed_closed");
        let mut db = match self.db.lock() {
            Ok(db) => db,
            Err(_) => {
                self.failed.store(true, Ordering::SeqCst);
                bail!("eval.ledger_lock_failed");
            }
        };
        match operation(&mut db) {
            Ok(value) => Ok(value),
            Err(error) if error.downcast_ref::<FlowStop>().is_some() => Err(error),
            Err(error) => {
                self.failed.store(true, Ordering::SeqCst);
                let _ = storage::halt(&db, "ledger_failure");
                Err(error)
            }
        }
    }
    pub fn reserve(&self, task: &str, input: u64, output: u64, kind: &str) -> Result<i64> {
        ensure!(!task.is_empty() && task.len() <= 80, "eval.invalid_task");
        ensure!(
            ["generation", "retry", "compaction", "probe", "auxiliary"].contains(&kind),
            "eval.invalid_request_kind"
        );
        self.checked(|db| {
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            if tx.query_row("SELECT halted FROM policy", [], |r| r.get::<_,bool>(0))? {
                return Err(FlowStop::GlobalHalt.into());
            }
            if storage::task_reason(&tx,task)?.is_some() {
                return Err(FlowStop::TaskStopped.into());
            }
            tx.execute("INSERT OR IGNORE INTO task_limits VALUES(?1,?2,?3,?4)",params![task,sql(self.limits.task)?,sql(self.limits.request_input)?,sql(self.limits.request_output)?])?;
            let (cap,input_cap,output_cap):(u64,u64,u64)=tx.query_row("SELECT cap,input_cap,output_cap FROM task_limits WHERE task=?1",[task],|r|Ok((number(r,0)?,number(r,1)?,number(r,2)?)))?;
            if self.limits.task > cap || self.limits.request_input > input_cap || self.limits.request_output > output_cap {
                tx.execute("INSERT INTO task_stops VALUES(?1,'policy_widening_refused')",[task])?;
                tx.commit()?;
                return Err(FlowStop::PolicyWidening.into());
            }
            if input > input_cap || output > output_cap {
                tx.execute("INSERT INTO task_stops VALUES(?1,'budget_exhausted')",[task])?;
                tx.commit()?;
                return Err(FlowStop::RequestBudget.into());
            }
            let reserved=input.checked_add(output).ok_or_else(||anyhow::anyhow!("eval.token_overflow"))?;
            let used=storage::total(&tx,Some(task))?;
            if used.checked_add(reserved).is_none_or(|v|v>cap.min(self.limits.task)) {
                tx.execute("INSERT INTO task_stops VALUES(?1,'budget_exhausted')",[task])?;
                tx.commit()?;
                return Err(FlowStop::TaskBudget.into());
            }
            let total=storage::total(&tx,None)?;
            if total.checked_add(reserved).is_none_or(|v|v>self.limits.cumulative) {
                storage::halt(&tx,"budget_exhausted")?;
                tx.commit()?;
                return Err(FlowStop::CumulativeBudget.into());
            }
            tx.execute("INSERT INTO requests(task,kind,reserve_input,reserve_output,state) VALUES(?1,?2,?3,?4,'pending')",params![task,kind,sql(input)?,sql(output)?])?;
            let id=tx.last_insert_rowid();
            tx.commit()?;
            Ok(id)
        })
    }
    pub fn settle(&self, id: i64, usage: Usage) -> Result<()> {
        self.checked(|db| {
            let tx=db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let cache=usage.cached_input_reported.then_some(usage.cached_input);
            let old:Option<SettlementRow>=tx.query_row("SELECT reserve_input,reserve_output,state,input,output,cached_input FROM requests WHERE id=?1",[id],|r|Ok((number(r,0)?,number(r,1)?,r.get(2)?,optional_number(r,3)?,optional_number(r,4)?,optional_number(r,5)?))).optional()?;
            let Some((input,output,state,a,b,c))=old else {bail!("eval.unknown_reservation")};
            if state=="settled" {
                let details:(Option<u64>,Option<u64>)=tx.query_row("SELECT reasoning_tokens,cache_write_tokens FROM usage_details WHERE id=?1",[id],|r|Ok((optional_number(r,0)?,optional_number(r,1)?)))?;
                ensure!(a==Some(usage.input)&&b==Some(usage.output)&&c==cache&&details==(usage.reasoning_tokens,usage.cache_write_tokens),"eval.usage_changed");
                return Ok(());
            }
            ensure!(state=="pending","eval.invalid_usage_state");
            validate_usage(usage)?;
            write_usage(&tx,id,usage)?;
            let overrun=usage.input>input||usage.output>output;
            if overrun {storage::halt(&tx,"provider_limit_exceeded")?;}
            else if storage::total(&tx,None)? >= self.limits.cumulative {storage::halt(&tx,"budget_exhausted")?;}
            tx.commit()?;
            ensure!(!overrun,"eval.provider_exceeded_reserved_limit");
            Ok(())
        })
    }
    /// A complete bounded 4xx body is charged at its full reservation, never released.
    pub fn charge_refusal(&self, id: i64, stop_task: bool) -> Result<()> {
        self.checked(|db| {
            let tx=db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            ensure!(tx.execute("UPDATE requests SET state='charged_refusal' WHERE id=?1 AND state='pending'",[id])?==1,"eval.reservation_not_pending");
            if stop_task {
                tx.execute("INSERT OR IGNORE INTO task_stops SELECT task,'provider_refused' FROM requests WHERE id=?1",[id])?;
            }
            if storage::total(&tx,None)? >= self.limits.cumulative {storage::halt(&tx,"budget_exhausted")?;}
            tx.commit()?;
            Ok(())
        })
    }
    /// The request was sent but the Provider never confirmed what it used: a 5xx, a timeout or
    /// a lost connection. It may still have generated output, so the whole reservation is
    /// charged and never released, and this task stops. Other tasks continue: the charge is
    /// already this request's worst case, which is what a later reconciliation would record.
    pub fn charge_unconfirmed(&self, id: i64) -> Result<()> {
        self.checked(|db| {
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            ensure!(
                tx.execute(
                    "UPDATE requests SET state='charged_unknown' WHERE id=?1 AND state='pending'",
                    [id]
                )? == 1,
                "eval.reservation_not_pending"
            );
            tx.execute(
                "INSERT OR IGNORE INTO task_stops SELECT task,'provider_unconfirmed' FROM requests WHERE id=?1",
                [id],
            )?;
            if storage::total(&tx, None)? >= self.limits.cumulative {
                storage::halt(&tx, "budget_exhausted")?;
            }
            tx.commit()?;
            Ok(())
        })
    }
    pub fn unresolved(&self, id: i64) -> Result<()> {
        self.checked(|db| {
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            ensure!(
                tx.execute(
                    "UPDATE requests SET state='unknown' WHERE id=?1 AND state='pending'",
                    [id]
                )? == 1,
                "eval.unknown_reservation"
            );
            storage::halt(&tx, "usage_unknown")?;
            tx.commit()?;
            Ok(())
        })
    }
    pub fn charged(&self) -> Result<u64> {
        let db = self.db.lock().map_err(|_| anyhow::anyhow!("eval.ledger_lock_failed"))?;
        storage::total(&db, None)
    }
    pub fn records(&self) -> Result<Vec<RequestRecord>> {
        let db = self.db.lock().map_err(|_| anyhow::anyhow!("eval.ledger_lock_failed"))?;
        storage::records(&db)
    }
    pub fn halt(&self) {
        self.failed.store(true, Ordering::SeqCst);
        if let Ok(db) = self.db.lock() {
            let _ = storage::halt(&db, "ledger_failure");
        }
    }
    pub fn stop_task(&self, task: &str, reason: &str) -> Result<()> {
        ensure!(
            [
                "budget_exhausted",
                "invalid_request",
                "provider_refused",
                "controller_failed",
                "core_disconnected"
            ]
            .contains(&reason),
            "eval.invalid_stop_reason"
        );
        self.checked(|db| {
            db.execute("INSERT OR IGNORE INTO task_stops VALUES(?1,?2)", params![task, reason])?;
            Ok(())
        })
    }
    pub fn task_stop_reason(&self, task: &str) -> Result<Option<String>> {
        let db = self.db.lock().map_err(|_| anyhow::anyhow!("eval.ledger_lock_failed"))?;
        storage::task_reason(&db, task)
    }
    pub fn stop_reason(&self) -> Result<Option<String>> {
        let db = self.db.lock().map_err(|_| anyhow::anyhow!("eval.ledger_lock_failed"))?;
        Ok(db.query_row("SELECT reason FROM stop_reason", [], |r| r.get(0)).optional()?)
    }
    pub fn provider_status(&self, id: i64, status: u16) -> Result<()> {
        self.checked(|db| {
            ensure!(db.execute("INSERT INTO provider_status SELECT id,?2 FROM requests WHERE id=?1 AND state='pending'",params![id,i64::from(status)])?==1,"eval.reservation_not_pending");Ok(())
        })
    }
    pub fn tool_definition_bytes(&self, id: i64, bytes: u64) -> Result<()> {
        self.checked(|db| {
            ensure!(
                db.execute(
                    "UPDATE requests SET tool_definition_bytes=?2 WHERE id=?1 AND state='pending'",
                    params![id, sql(bytes)?]
                )? == 1,
                "eval.reservation_not_pending"
            );
            Ok(())
        })
    }
    pub fn request_body_bytes(&self, id: i64, bytes: u64) -> Result<()> {
        self.checked(|db| {
            ensure!(db.execute("INSERT INTO request_sizes SELECT id,?2 FROM requests WHERE id=?1 AND state='pending'",params![id,sql(bytes)?])?==1,"eval.reservation_not_pending");Ok(())
        })
    }
}

pub(super) fn validate_usage(usage: Usage) -> Result<()> {
    ensure!(usage.cached_input <= usage.input, "eval.invalid_usage");
    ensure!(
        usage.reasoning_tokens.is_none_or(|v| v <= usage.output)
            && usage.cache_write_tokens.is_none_or(|v| v <= usage.input),
        "eval.invalid_usage_details"
    );
    Ok(())
}
pub(super) fn write_usage(db: &Connection, id: i64, usage: Usage) -> Result<()> {
    let cache = usage.cached_input_reported.then_some(usage.cached_input);
    db.execute(
        "UPDATE requests SET state='settled',input=?2,output=?3,cached_input=?4 WHERE id=?1",
        params![id, sql(usage.input)?, sql(usage.output)?, cache.map(sql).transpose()?],
    )?;
    db.execute(
        "INSERT INTO usage_details VALUES(?1,?2,?3)",
        params![
            id,
            usage.reasoning_tokens.map(sql).transpose()?,
            usage.cache_write_tokens.map(sql).transpose()?
        ],
    )?;
    Ok(())
}
