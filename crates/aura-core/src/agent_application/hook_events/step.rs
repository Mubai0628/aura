//! `context.before_build` and `model.before_call`, once per model step.
use super::*;

/// What a step's `context.before_build` and `model.before_call` Hooks decided.
#[derive(Default)]
pub(in crate::agent_application) struct StepHooks {
    /// One block per Hook that injected text, naming the Hook and the content it trusted.
    pub injections: Vec<String>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub max_output_tokens: Option<u64>,
    pub blocked: bool,
}

impl AgentApplication {
    /// Runs (or reuses the kept decisions of) the step's `context.before_build` and
    /// `model.before_call` Hooks, so a retried step sends exactly what it sent before.
    pub(in crate::agent_application) async fn step_hooks(
        &self,
        run_id: AgentRunId,
        turn: &TurnTask,
        step: ModelStepId,
        options: &aura_model_domain::ResolvedModelRequestOptions,
    ) -> anyhow::Result<StepHooks> {
        let mut hooks = StepHooks::default();
        if !self.hooks_registered() {
            return Ok(hooks);
        }
        let owned = turn.clone();
        hooks.injections = self
            .store
            .read(StoreStep::run(run_id, "agent.hooks.message_notes"), move |db| {
                hook_store::message_notes_for_turn(db, &owned)
            })
            .await?;
        hooks.injections.extend(self.completion_guidance(run_id, turn.turn_task_id).await?);
        let runner = self.hook_runner()?;
        let context_payload =
            serde_json::json!({"model_step_id": step, "mode": turn.mode, "injections": []});
        let context = self
            .step_decision(
                run_id,
                turn,
                step,
                HookEvent::ContextBeforeBuild,
                &runner,
                context_payload,
            )
            .await?;
        if context["kind"] == "blocked" {
            hooks.blocked = true;
            return Ok(hooks);
        }
        hooks.injections.extend(
            context["injections"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|i| i.as_str().map(str::to_owned)),
        );
        let model_payload = serde_json::json!({"model_step_id": step, "parameters": {
            "temperature": options.temperature,
            "top_p": options.top_p,
            "max_output_tokens": options.effective_max_output_tokens,
        }});
        let model = self
            .step_decision(run_id, turn, step, HookEvent::ModelBeforeCall, &runner, model_payload)
            .await?;
        if model["kind"] == "blocked" {
            hooks.blocked = true;
            return Ok(hooks);
        }
        let parameters = &model["parameters"];
        hooks.temperature = parameters["temperature"].as_f64();
        hooks.top_p = parameters["top_p"].as_f64();
        hooks.max_output_tokens = parameters["max_output_tokens"]
            .as_u64()
            .filter(|tokens| *tokens < options.effective_max_output_tokens);
        Ok(hooks)
    }

    /// One pre-event per step: the kept decision, or the Hooks' answer kept now.
    async fn step_decision(
        &self,
        run_id: AgentRunId,
        turn: &TurnTask,
        step: ModelStepId,
        event: HookEvent,
        runner: &HookRunner,
        payload: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        let subject = step.to_string();
        let lookup = subject.clone();
        if let Some(kept) = self
            .store
            .read(StoreStep::run(run_id, "agent.hooks.step"), move |db| {
                hook_store::decision(db, &lookup, event)
            })
            .await?
        {
            return Ok(kept);
        }
        if !runner.hears(event) {
            return Ok(serde_json::json!({"kind": "continue"}));
        }
        let outcome = self.run_hooks(self.hook_call(event, turn, payload)).await?;
        let decision = match &outcome.block {
            Some(block) => serde_json::json!({
                "kind": "blocked", "hook": block.hook_name, "reason": block.reason
            }),
            None => {
                let mut decision = outcome.payload.clone();
                if let Some(object) = decision.as_object_mut() {
                    object.insert("kind".into(), serde_json::json!("continue"));
                    if event == HookEvent::ContextBeforeBuild {
                        let texts = object
                            .get("injections")
                            .and_then(|v| v.as_array())
                            .cloned()
                            .unwrap_or_default();
                        object.insert(
                            "injections".into(),
                            serde_json::json!(named(&outcome, &texts)),
                        );
                    }
                }
                decision
            }
        };
        let kept = decision.clone();
        self.store
            .write(StoreStep::run(run_id, "agent.hooks.step_decision"), move |journal| {
                journal.database.unit_of_work(|tx| {
                    hook_store::save_decision(tx, &subject, event, &kept, UtcTimestamp::now())
                })
            })
            .await?;
        Ok(decision)
    }
}

/// Each injection names its Hook and the exact content the user trusted.
fn named(outcome: &HookRunOutcome, texts: &[serde_json::Value]) -> Vec<String> {
    outcome
        .invocations
        .iter()
        .filter(|view| view.outcome == aura_hooks::HookOutcome::Transformed)
        .zip(texts)
        .map(|(view, text)| {
            let short = &view.fingerprint[..16.min(view.fingerprint.len())];
            format!(
                "From Hook {} {} (fingerprint {short}):\n{}",
                view.hook_name,
                view.hook_version,
                text.as_str().unwrap_or_default()
            )
        })
        .collect()
}
