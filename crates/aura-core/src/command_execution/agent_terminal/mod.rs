//! Agent terminal ownership, output persistence and trusted desktop control.
mod calls;
mod handback;
mod privacy;
pub(crate) mod sensitive;
#[cfg(test)]
pub(crate) mod test_support;
mod text;
mod wait;
use super::CommandExecution;
use crate::agent_store::{AgentStore, StoreAccess, StoreStep};
use anyhow::{Context, Result, ensure};
use aura_backend_windows_user::compat::{TerminalRegistry, TerminalSession};
use aura_domain_common::*;
use aura_persistence_v2::terminal::{self as records, AgentTerminalRecord};
use aura_windows_user_contract::*;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

/// How long the output worker waits after the terminal printed before it saves. Keys typed close
/// together are saved in one append; every append rewrites the output's last segment.
const SAVE_AFTER: Duration = Duration::from_millis(200);

struct Live {
    terminal: Option<Arc<TerminalSession>>,
    controller: TerminalControllerV1,
    /// The record's view as last saved, so input answers without reading the journal.
    view: TerminalViewV1,
    offset: u64,
    text: text::PlainText,
    privacy: privacy::EchoPrivacy,
}
/// A terminal of this Core. Input needs only `live`; a capture holds `saving` from reading the
/// output until it is saved, so saves keep the output's order without holding `live` while the
/// database works.
struct Entry {
    live: Mutex<Live>,
    saving: Mutex<()>,
}
impl Entry {
    fn lock(&self) -> Result<MutexGuard<'_, Live>> {
        self.live.lock().map_err(|_| anyhow::anyhow!("terminal.owner_lock"))
    }
}
pub(super) struct AgentTerminals {
    journal: Arc<crate::broker::BrokerJournal>,
    pub(super) store: Arc<AgentStore>,
    registry: Arc<TerminalRegistry>,
    live: Mutex<BTreeMap<String, Arc<Entry>>>,
    publisher: Mutex<Option<crate::AsyncPriorityQueue>>,
    runtime: RuntimeInstanceId,
    sensitive: Mutex<BTreeMap<ToolCallId, sensitive::Pending>>,
}
impl AgentTerminals {
    pub fn new(
        journal: Arc<crate::broker::BrokerJournal>,
        store: Arc<AgentStore>,
        registry: Arc<TerminalRegistry>,
        runtime: RuntimeInstanceId,
    ) -> Result<Self> {
        // A previous Core's terminals are never relaunched. Metadata and text
        // survive; native handles and input authority do not.
        journal.lock()?.database.unit_of_work(|tx| -> aura_persistence_core::StoreResult<()> {
            for mut record in records::list(tx, None)? {
                if record.view.end.is_none() {
                    record.view.end = Some(TerminalEnd {
                        reason: TerminalEndReason::CoreStopped,
                        exit_code: None,
                        stop_confirmed: false,
                        error: None,
                    });
                    record.view.revision = Revision::new(record.view.revision.value() + 1);
                    records::save(tx, &record)?;
                }
            }
            Ok(())
        })?;
        Ok(Self {
            journal,
            store,
            registry,
            live: Default::default(),
            publisher: Default::default(),
            runtime,
            sensitive: Default::default(),
        })
    }
    fn entry(&self, id: &str) -> Result<Arc<Entry>> {
        self.live
            .lock()
            .map_err(|_| anyhow::anyhow!("terminal.owner_lock"))?
            .get(id)
            .cloned()
            .context("terminal.ended: Core stopped or the terminal exited; open a new terminal")
    }
    /// Whether `id` is a terminal of this Core, ended or not, known without the journal.
    pub fn is_live(&self, id: &str) -> Result<bool> {
        Ok(self.live.lock().map_err(|_| anyhow::anyhow!("terminal.owner_lock"))?.contains_key(id))
    }
    pub fn record(&self, session: SessionId, id: &str) -> Result<AgentTerminalRecord> {
        Ok(records::get(self.journal.lock()?.database.connection(), session, id)?)
    }
    pub fn list(&self, session: SessionId) -> Result<Vec<AgentTerminalRecord>> {
        Ok(records::list(self.journal.lock()?.database.connection(), Some(session))?)
    }
    pub async fn output_page(
        self: &Arc<Self>,
        record: AgentTerminalRecord,
        offset: u64,
    ) -> Result<AgentTerminalResultV1> {
        let owner = self.clone();
        self.store
            .run_critical(
                StoreStep::operation("terminal.history.subscribe"),
                StoreAccess::Read,
                move || {
                    Ok(records::read(
                        owner.journal.lock()?.database.connection(),
                        &record,
                        offset,
                        65536,
                    )?)
                },
            )
            .await
    }
    async fn publish(&self, view: &TerminalViewV1) -> Result<()> {
        let queue =
            self.publisher.lock().map_err(|_| anyhow::anyhow!("terminal.publisher_lock"))?.clone();
        if let Some(queue) = queue {
            crate::send_notification(
                &queue,
                crate::QueuePriority::P2,
                "terminal.state.v1",
                &TerminalStateChangedV1 { runtime_instance_id: self.runtime, view: view.clone() },
            )
            .await?;
        }
        Ok(())
    }
    fn register(
        self: &Arc<Self>,
        record: AgentTerminalRecord,
        terminal: Arc<TerminalSession>,
    ) -> Result<()> {
        self.journal.lock()?.database.unit_of_work(|tx| records::save(tx, &record))?;
        self.live.lock().map_err(|_| anyhow::anyhow!("terminal.owner_lock"))?.insert(
            record.view.terminal_id.clone(),
            Arc::new(Entry {
                live: Mutex::new(Live {
                    terminal: Some(terminal.clone()),
                    controller: TerminalControllerV1::Agent,
                    view: record.view.clone(),
                    offset: 0,
                    text: Default::default(),
                    privacy: Default::default(),
                }),
                saving: Mutex::new(()),
            }),
        );
        let owner = self.clone();
        let session = record.view.session_id;
        let id = record.view.terminal_id;
        let mut changed = terminal.subscribe_output();
        // The worker keeps only ownership metadata, not a second terminal Arc.
        // capture releases the kernel registry after final bytes are durable.
        tokio::spawn(async move {
            loop {
                let capture_owner = owner.clone();
                let capture_id = id.clone();
                let result = owner
                    .store
                    .run_critical(
                        StoreStep::operation("terminal.output.append"),
                        StoreAccess::Write,
                        move || capture_owner.capture(session, &capture_id),
                    )
                    .await;
                match result {
                    Ok(view) => {
                        if view.end.is_some() {
                            let _ = owner.publish(&view).await;
                            break;
                        }
                        if owner.pending(&id).unwrap_or(false) {
                            continue;
                        }
                        if changed.changed().await.is_err() {
                            break;
                        }
                        tokio::time::sleep(SAVE_AFTER).await;
                    }
                    Err(error) => {
                        eprintln!("terminal output persistence failed: {error}");
                        let registry = owner.registry.clone();
                        let stop_id = id.clone();
                        let _ = tokio::task::spawn_blocking(move || {
                            registry.close_and_remove(
                                session,
                                &stop_id,
                                TerminalEndReason::OutputFailed,
                            )
                        })
                        .await;
                        break;
                    }
                }
            }
        });
        Ok(())
    }
    fn capture(&self, session: SessionId, id: &str) -> Result<TerminalViewV1> {
        let entry = self.entry(id)?;
        let _saving = entry.saving.lock().map_err(|_| anyhow::anyhow!("terminal.owner_lock"))?;
        let mut live = entry.lock()?;
        let Some(terminal) = live.terminal.clone() else {
            drop(live);
            return Ok(self.record(session, id)?.view);
        };
        let mut text = String::new();
        let mut bytes = 0;
        loop {
            let page = terminal.read(live.offset, 65536)?;
            if page.truncated {
                live.text = Default::default();
                text.push_str("\n[较早的终端输出已超过保留上限]\n");
            }
            live.offset = page.next_offset;
            if page.bytes.is_empty() {
                break;
            }
            bytes += page.bytes.len();
            text.push_str(&live.text.push(&page.bytes, false));
            if bytes >= 2 * 1024 * 1024 {
                break;
            }
        }
        let pending = !terminal.read(live.offset, 1)?.bytes.is_empty();
        let end = if pending { None } else { terminal.end()? };
        if end.is_some() {
            text.push_str(&live.text.push(&[], true));
        }
        let text = live.privacy.push(&text, end.is_some());
        drop(live);
        // The output is appended; the record changes only when the terminal ended, and then it is
        // re-read in the same transaction. A capture never writes back a copy of the record read
        // before another change.
        let view = self.journal.lock()?.database.unit_of_work(|tx| {
            records::append(tx, session, id, &text)?;
            let mut record = records::get(tx, session, id)?;
            if let Some(end) = &end {
                record.view.end = Some(end.clone());
                record.view.revision = Revision::new(record.view.revision.value() + 1);
                records::save(tx, &record)?;
            }
            Ok::<_, aura_persistence_core::PersistenceCoreError>(record.view)
        })?;
        if let Some(end) = &view.end {
            ensure!(end.stop_confirmed, "terminal.stop_unconfirmed");
            self.registry.close_and_remove(session, id, end.reason)?;
            let mut live = entry.lock()?;
            live.terminal = None;
            live.text = Default::default();
            live.privacy = Default::default();
            remember(&mut live, &view);
            drop(live);
            self.cancel_sensitive_terminal(id);
        }
        Ok(view)
    }
    fn pending(&self, id: &str) -> Result<bool> {
        let entry = self.entry(id)?;
        let live = entry.lock()?;
        Ok(live.terminal.as_ref().is_some_and(|terminal| {
            terminal.read(live.offset, 1).is_ok_and(|page| !page.bytes.is_empty())
        }))
    }
    fn model_read(
        &self,
        session: SessionId,
        id: &str,
        offset: u64,
        max: usize,
    ) -> Result<AgentTerminalResultV1> {
        let mut record = self.record(session, id)?;
        ensure!(record.view.controller == TerminalControllerV1::Agent, handback::TAKEN_OVER);
        if self.live.lock().map_err(|_| anyhow::anyhow!("terminal.owner_lock"))?.contains_key(id) {
            self.capture(session, id)?;
            record = self.record(session, id)?;
        }
        Ok(records::read(self.journal.lock()?.database.connection(), &record, offset, max)?)
    }
    pub async fn control(
        self: &Arc<Self>,
        input: TerminalControlRequestV1,
    ) -> Result<TerminalViewV1> {
        let owner = self.clone();
        let view = self
            .store
            .run_critical(
                StoreStep::operation("terminal.user.control"),
                StoreAccess::Write,
                move || {
                    let ended =
                        owner.record(input.session_id, &input.terminal_id)?.view.end.is_some();
                    if !ended {
                        owner.entry(&input.terminal_id)?.lock()?.controller = input.controller;
                    }
                    if input.controller == TerminalControllerV1::User {
                        owner.cancel_sensitive_terminal(&input.terminal_id);
                    }
                    // Read and saved in one transaction, so the change is made to the record as it
                    // is now, not to a copy read before a capture saved.
                    let view = owner.journal.lock()?.database.unit_of_work(|tx| {
                        let mut record = records::get(tx, input.session_id, &input.terminal_id)?;
                        record.view.controller = input.controller;
                        record.view.revision = Revision::new(record.view.revision.value() + 1);
                        records::save(tx, &record)?;
                        Ok::<_, aura_persistence_core::PersistenceCoreError>(record.view)
                    })?;
                    if let Ok(entry) = owner.entry(&input.terminal_id) {
                        remember(&mut *entry.lock()?, &view);
                    }
                    Ok(view)
                },
            )
            .await?;
        self.publish(&view).await?;
        Ok(view)
    }
    /// A key from the desktop. It reads only this Core's live terminal: no journal, and no wait for
    /// an output save.
    pub fn user_write(
        &self,
        session: SessionId,
        id: &str,
        input: &str,
        interrupt: bool,
    ) -> Result<TerminalViewV1> {
        let entry = self.entry(id)?;
        let live = entry.lock()?;
        ensure!(
            live.view.session_id == session,
            "terminal.not_found_in_session: only Agent terminals in this session are visible"
        );
        let cursor_reply = input
            .strip_prefix("\u{1b}[")
            .and_then(|value| value.strip_suffix('R'))
            .is_some_and(|value| {
                value.split_once(';').is_some_and(|(row, col)| {
                    [row, col].iter().all(|value| {
                        !value.is_empty()
                            && value.len() <= 4
                            && value.bytes().all(|byte| byte.is_ascii_digit())
                    })
                })
            });
        ensure!(
            live.view.owner == TerminalOwnerV1::Agent
                && (live.controller == TerminalControllerV1::User || cursor_reply && !interrupt),
            "请先接管该 Agent 终端，再输入。"
        );
        let terminal = live.terminal.as_ref().context("终端已结束，请新建终端。")?;
        if interrupt {
            terminal.interrupt()?;
        }
        if !input.is_empty() {
            terminal.write(input.as_bytes())?;
        }
        Ok(live.view.clone())
    }
    pub async fn close(
        self: &Arc<Self>,
        session: SessionId,
        id: String,
        reason: TerminalEndReason,
    ) -> Result<TerminalViewV1> {
        let owner = self.clone();
        let view = self
            .store
            .run_critical(StoreStep::operation("terminal.close"), StoreAccess::Write, move || {
                let record = owner.record(session, &id)?;
                if record.view.end.is_some() {
                    return Ok(record.view);
                }
                owner.registry.get(session, &id)?.close(reason)?;
                owner.capture(session, &id)
            })
            .await?;
        self.publish(&view).await?;
        Ok(view)
    }
    pub fn flush_shutdown(&self) -> Result<()> {
        let entries = records::list(self.journal.lock()?.database.connection(), None)?;
        for record in entries {
            if record.view.end.is_none() {
                while self.capture(record.view.session_id, &record.view.terminal_id)?.end.is_none()
                    && self.pending(&record.view.terminal_id)?
                {}
            }
        }
        Ok(())
    }
    pub async fn remove_absent_sessions(
        self: &Arc<Self>,
        live: &[aura_workspace_domain::SessionSummary],
    ) -> Result<()> {
        // On the reader: a deletion's write steps do not hold this back.
        let entries = self
            .store
            .read(StoreStep::operation("terminal.deleted_sessions"), move |db| {
                records::list(db, None)
            })
            .await?;
        let deleted = entries
            .iter()
            .filter(|record| {
                !live.iter().any(|session| session.session_id == record.view.session_id)
            })
            .map(|record| record.view.session_id)
            .collect::<std::collections::BTreeSet<_>>();
        for session in deleted {
            for record in entries.iter().filter(|record| record.view.session_id == session) {
                self.close(
                    session,
                    record.view.terminal_id.clone(),
                    TerminalEndReason::SessionDeleted,
                )
                .await?;
            }
            let owner = self.clone();
            self.store
                .run_critical(
                    StoreStep::operation("terminal.delete_records"),
                    StoreAccess::Write,
                    move || {
                        let entries = owner.list(session)?;
                        owner
                            .journal
                            .lock()?
                            .database
                            .unit_of_work(|tx| records::delete_session(tx, session))?;
                        let mut live = owner
                            .live
                            .lock()
                            .map_err(|_| anyhow::anyhow!("terminal.owner_lock"))?;
                        for entry in entries {
                            live.remove(&entry.view.terminal_id);
                        }
                        Ok(())
                    },
                )
                .await?;
        }
        Ok(())
    }
}
/// Keeps the newer of two saved views: a takeover and an end saved together may report back in
/// either order.
fn remember(live: &mut Live, view: &TerminalViewV1) {
    if view.revision.value() >= live.view.revision.value() {
        live.view = view.clone();
    }
}
impl CommandExecution {
    pub(crate) fn set_terminal_publisher(&self, queue: crate::AsyncPriorityQueue) -> Result<()> {
        *self
            .agent_terminals
            .publisher
            .lock()
            .map_err(|_| anyhow::anyhow!("terminal.publisher_lock"))? = Some(queue);
        Ok(())
    }
}
