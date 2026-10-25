//! Ephemeral, bounded maintenance lanes. A completed turn is never reopened.
mod facts;
mod model;
use super::*;
use aura_memory::{
    MemoryMaintenanceRun, MemoryMaintenanceState, MemoryScope, MemoryWorkspace, WriteActor,
};
use std::collections::{BTreeSet, VecDeque};
use std::sync::{OnceLock, Weak};

#[derive(Default)]
pub(super) struct Maintenance {
    lanes: Mutex<BTreeMap<WorkspaceId, Lane>>,
    stopped: std::sync::atomic::AtomicBool,
    #[cfg(test)]
    pub enabled: std::sync::atomic::AtomicBool,
}
struct Lane {
    queue: VecDeque<facts::Pending>,
    seen: BTreeSet<AgentRunId>,
    cancel: ModelCancellationToken,
    running: bool,
}
impl Maintenance {
    /// Runtime identity is minted by Core. Weak entries retain no task data;
    /// a live application rebuild reuses the process's existing queue owner.
    pub fn for_runtime(runtime: RuntimeInstanceId) -> Arc<Self> {
        type Owners = Mutex<BTreeMap<RuntimeInstanceId, Weak<Maintenance>>>;
        static OWNERS: OnceLock<Owners> = OnceLock::new();
        let mut owners =
            OWNERS.get_or_init(Mutex::default).lock().unwrap_or_else(|e| e.into_inner());
        owners.retain(|_, owner| owner.strong_count() > 0);
        if let Some(owner) = owners.get(&runtime).and_then(Weak::upgrade) {
            return owner;
        }
        let owner = Arc::new(Self::default());
        owners.insert(runtime, Arc::downgrade(&owner));
        owner
    }
    pub fn stop(&self) {
        self.stopped.store(true, std::sync::atomic::Ordering::Release);
        for lane in self.lanes.lock().unwrap_or_else(|e| e.into_inner()).values() {
            lane.cancel.cancel();
        }
    }
}
impl AgentApplication {
    pub(super) async fn schedule_memory(self: &Arc<Self>, run: AgentRunId) -> anyhow::Result<()> {
        #[cfg(test)]
        if !self.memory_maintenance.enabled.load(std::sync::atomic::Ordering::Acquire) {
            return Ok(());
        }
        let Some(mut pending) = self.memory_facts(run).await? else { return Ok(()) };
        let settings = self.settings.memory_settings();
        let mode =
            self.execution.persistence.session_permission_state(pending.turn.session_id)?.mode;
        let skip = if self.memory_maintenance.stopped.load(std::sync::atomic::Ordering::Acquire) {
            Some("关机中；未整理")
        } else if mode == aura_security_contract::PermissionMode::ReadOnly {
            Some("只读会话未整理")
        } else if !settings.settings.automatic_maintenance {
            Some("自动整理已关闭")
        } else {
            None
        };
        if let Some(reason) = skip {
            pending.record.state = MemoryMaintenanceState::Skipped;
            pending.record.reason = Some(reason.into());
            pending.record.finished_at = Some(UtcTimestamp::now());
            self.save_memory_run(&pending.workspace, pending.record).await?;
            return Ok(());
        }
        self.save_memory_run(&pending.workspace, pending.record.clone()).await?;
        let workspace = pending.turn.workspace_id;
        let start = {
            let mut lanes = self.memory_maintenance.lanes.lock().unwrap_or_else(|e| e.into_inner());
            if lanes.values().map(|lane| lane.queue.len()).sum::<usize>() >= 64
                || (!lanes.contains_key(&workspace) && lanes.len() >= 32)
            {
                false
            } else {
                let lane = lanes.entry(workspace).or_insert_with(|| Lane {
                    queue: VecDeque::new(),
                    seen: BTreeSet::new(),
                    cancel: ModelCancellationToken::default(),
                    running: false,
                });
                if self.memory_maintenance.stopped.load(std::sync::atomic::Ordering::Acquire) {
                    lane.cancel.cancel();
                }
                if !lane.seen.insert(run) {
                    return Ok(());
                }
                // Bound metadata too: old completed identities are safe to drop because
                // the durable per-run record is checked before enqueueing.
                if lane.seen.len() > 512 {
                    lane.seen.pop_first();
                }
                if lane.queue.len() >= 8 {
                    false
                } else {
                    lane.queue.push_back(pending.clone());
                    if lane.running {
                        return Ok(());
                    }
                    lane.running = true;
                    true
                }
            }
        };
        if !start {
            pending.record.state = MemoryMaintenanceState::Skipped;
            pending.record.reason = Some("整理队列已满（工作区8、全局64、工作区并行32）".into());
            pending.record.finished_at = Some(UtcTimestamp::now());
            self.save_memory_run(&pending.workspace, pending.record).await?;
            return Ok(());
        }
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            loop {
                let Some(app) = weak.upgrade() else { break };
                let item = {
                    let mut lanes =
                        app.memory_maintenance.lanes.lock().unwrap_or_else(|e| e.into_inner());
                    let Some(lane) = lanes.get_mut(&workspace) else { break };
                    match lane.queue.pop_front() {
                        Some(p) => (p, lane.cancel.clone()),
                        None => {
                            lanes.remove(&workspace);
                            break;
                        }
                    }
                };
                let (pending, cancel) = item;
                let mut record = pending.record.clone();
                let outcome = crate::agent_supervision::CatchUnwind::new(app.consolidate_memory(
                    &pending,
                    &cancel,
                    &mut record,
                ))
                .await;
                match outcome {
                    Ok(Ok(())) => (),
                    Ok(Err(error)) => {
                        #[cfg(test)]
                        eprintln!("memory maintenance fixture failure: {error}");
                        crate::append_core_log(
                            "memory.maintenance_failed",
                            serde_json::json!({"run":pending.run,"reason":error.to_string()}),
                        );
                        record.state = if cancel.is_cancelled() {
                            MemoryMaintenanceState::Interrupted
                        } else {
                            MemoryMaintenanceState::Failed
                        };
                        record.reason = Some(
                            if cancel.is_cancelled() {
                                "已中断；不重放"
                            } else {
                                "整理未完成；原任务结果不变。请查看 Core 诊断。"
                            }
                            .into(),
                        );
                    }
                    Err(_) => {
                        record.state = MemoryMaintenanceState::Failed;
                        record.reason = Some("整理任务异常；原任务结果不变".into());
                    }
                }
                if !record.usage_complete {
                    record.reason = Some(format!(
                        "{} 用量未确认完整；显示的是已记录数。",
                        record.reason.as_deref().unwrap_or("整理未完成。")
                    ));
                }
                record.finished_at = Some(UtcTimestamp::now());
                if let Err(error) = app.save_memory_run(&pending.workspace, record).await {
                    crate::append_core_log(
                        "memory.maintenance_record_failed",
                        serde_json::json!({"run":pending.run,"error":error.to_string()}),
                    );
                }
            }
        });
        Ok(())
    }
    async fn save_memory_run(
        &self,
        workspace: &MemoryWorkspace,
        record: MemoryMaintenanceRun,
    ) -> anyhow::Result<()> {
        let memory = self.execution.journal.memory().clone();
        let workspace = workspace.clone();
        tokio::task::spawn_blocking(move || {
            memory.vault(MemoryScope::Project, &workspace)?.record_maintenance_run(&record)
        })
        .await??;
        Ok(())
    }
    async fn consolidate_memory(
        &self,
        pending: &facts::Pending,
        cancel: &ModelCancellationToken,
        record: &mut MemoryMaintenanceRun,
    ) -> anyhow::Result<()> {
        if !self.memory_write_allowed(pending.turn.session_id)? {
            record.state = MemoryMaintenanceState::Skipped;
            record.reason = Some("只读会话或设置变更；未整理".into());
            return Ok(());
        }
        anyhow::ensure!(!cancel.is_cancelled(), "memory.cancelled");
        record.state = MemoryMaintenanceState::Running;
        self.save_memory_run(&pending.workspace, record.clone()).await?;
        let output = self.memory_model(pending, cancel, record).await?;
        anyhow::ensure!(
            output.usage.input_tokens <= i64::MAX as u64
                && output.usage.output_tokens <= i64::MAX as u64,
            "memory.usage_out_of_range"
        );
        record.input_tokens = output.usage.input_tokens;
        record.output_tokens = output.usage.output_tokens;
        record.usage_complete = true;
        anyhow::ensure!(!cancel.is_cancelled(), "memory.cancelled");
        anyhow::ensure!(
            output.stop_reason == NormalizedStopReason::EndTurn
                && output.tool_calls.is_empty()
                && output.refusal.is_none(),
            "memory.incomplete_output"
        );
        let text = output.assistant_blocks.iter().map(|b| b.text.as_str()).collect::<String>();
        anyhow::ensure!(text.len() <= 512 * 1024, "memory.output_bound");
        let mut output = aura_memory::parse_maintenance_output(&text)
            .map_err(|_| anyhow::anyhow!("memory.invalid_structured_output"))?;
        // One candidate's slip no longer discards the others.
        aura_memory::normalize_maintenance_output(&mut output);
        let memory = self.execution.journal.memory().clone();
        let workspace = pending.workspace.clone();
        let run = record.run_id.clone();
        let session = pending.turn.session_id;
        let actor = WriteActor {
            kind: "automatic_maintenance".into(),
            session_id: Some(session),
            turn_id: Some(pending.turn.turn_task_id.to_string()),
        };
        let persistence = self.execution.persistence.clone();
        let settings = self.settings.clone();
        let cancel = cancel.clone();
        let applied = tokio::task::spawn_blocking(move || -> anyhow::Result<bool> {
            persistence.with_session_permission_authority(
                session,
                |mode| -> anyhow::Result<bool> {
                    let current = settings.memory_settings();
                    if mode.mode == aura_security_contract::PermissionMode::ReadOnly
                        || !current.settings.automatic_maintenance
                        || cancel.is_cancelled()
                    {
                        return Ok(false);
                    }
                    memory.apply_maintenance(
                        &output,
                        &workspace,
                        &actor,
                        &run,
                        current.settings.event_retention_days,
                        || !cancel.is_cancelled(),
                    )?;
                    Ok(true)
                },
            )?
        })
        .await??;
        record.state = if applied {
            MemoryMaintenanceState::Completed
        } else {
            MemoryMaintenanceState::Skipped
        };
        record.reason = Some(
            if applied {
                "已校验并整理；用量不计入 Goal 预算"
            } else {
                "只读会话或设置变更；未整理"
            }
            .into(),
        );
        Ok(())
    }
    fn memory_write_allowed(&self, session: SessionId) -> anyhow::Result<bool> {
        Ok(self.settings.memory_settings().settings.automatic_maintenance
            && self.execution.persistence.session_permission_state(session)?.mode
                != aura_security_contract::PermissionMode::ReadOnly)
    }
}
