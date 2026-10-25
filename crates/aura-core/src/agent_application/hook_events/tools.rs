//! `tool.before_call`, `tool.after_call` and `tool.on_error`.
use super::*;

impl AgentApplication {
    /// `tool.before_call`, before admission: each call's Hooks run in order and their combined
    /// decision is kept. Admission then checks changed arguments from the start and refuses a
    /// blocked call, telling the model which Hook stopped it and why.
    pub(in crate::agent_application) async fn decide_tool_calls(
        &self,
        run_id: AgentRunId,
        step_id: ModelStepId,
    ) -> anyhow::Result<()> {
        if !self.hooks_registered() {
            return Ok(());
        }
        let pending = self
            .store
            .read(StoreStep::run(run_id, "agent.hooks.tool_calls"), move |db| {
                hook_store::calls_awaiting_hooks(db, step_id)
            })
            .await?;
        if pending.is_empty() {
            return Ok(());
        }
        let turn = self.turn(run_id).await?;
        for pending in pending {
            let mut call = self.hook_call(
                HookEvent::ToolBeforeCall,
                &turn,
                serde_json::json!({ "tool": pending.tool_name, "arguments": pending.arguments }),
            );
            call.tool_call_id = Some(pending.tool_call_id);
            call.tool_name = Some(pending.tool_name.clone());
            call.subject = Some(pending.tool_call_id.to_string());
            let outcome = self.run_hooks(call).await?;
            let decision = match (outcome.block, outcome.transformed) {
                (Some(block), _) => {
                    ToolCallHookDecision::Blocked { hook: block.hook_name, reason: block.reason }
                }
                (None, true) => ToolCallHookDecision::Arguments {
                    arguments: outcome.payload["arguments"].clone(),
                },
                (None, false) => ToolCallHookDecision::Continue,
            };
            let tool_call_id = pending.tool_call_id;
            self.store
                .write(StoreStep::run(run_id, "agent.hooks.tool_call_decision"), move |journal| {
                    journal.database.unit_of_work(|tx| {
                        hook_store::save_tool_call_decision(
                            tx,
                            tool_call_id,
                            &decision,
                            UtcTimestamp::now(),
                        )
                    })
                })
                .await?;
        }
        Ok(())
    }

    /// `tool.after_call` and `tool.on_error`, once the batch is done: a Hook may replace what
    /// the model sees of a result or error; the stored result itself never changes.
    pub(in crate::agent_application) async fn review_tool_results(
        &self,
        run_id: AgentRunId,
        batch_id: ToolBatchId,
    ) -> anyhow::Result<()> {
        if !self.hooks_registered() {
            return Ok(());
        }
        let completed = self
            .store
            .read(StoreStep::run(run_id, "agent.hooks.tool_results"), move |db| {
                hook_store::completed_tool_calls(db, batch_id)
            })
            .await?;
        if completed.is_empty() {
            return Ok(());
        }
        let turn = self.turn(run_id).await?;
        for done in completed {
            let event = hook_store::result_event(done.succeeded);
            let field = if done.succeeded { "result" } else { "error" };
            let mut call = self.hook_call(
                event,
                &turn,
                serde_json::json!({
                    "tool": done.tool_name,
                    "arguments": done.arguments,
                    field: done.content,
                }),
            );
            call.tool_call_id = Some(done.tool_call_id);
            call.tool_name = Some(done.tool_name.clone());
            call.subject = Some(done.tool_call_id.to_string());
            let outcome = self.run_hooks(call).await?;
            let text = outcome
                .transformed
                .then(|| outcome.payload[field].as_str().map(str::to_owned))
                .flatten();
            let (tool_call_id, succeeded) = (done.tool_call_id, done.succeeded);
            self.store
                .write(StoreStep::run(run_id, "agent.hooks.tool_result_decision"), move |journal| {
                    journal.database.unit_of_work(|tx| {
                        hook_store::save_result_decision(
                            tx,
                            tool_call_id,
                            succeeded,
                            text.as_deref(),
                            UtcTimestamp::now(),
                        )
                    })
                })
                .await?;
        }
        Ok(())
    }
}
