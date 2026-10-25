//! What a
//! running model step is writing into tool calls, as the provider streams it — the tool, how many
//! bytes of its input have arrived and the start of that input. It lives in memory only and is
//! forgotten when the step's stream ends; a Core that restarts has no running step to show.
//!
//! These stream events were each written to the store before, in a transaction that stored
//! nothing (`stream::append` keeps visible text and reasoning only). A long file written by a tool
//! call meant hundreds to thousands of such transactions on the shared writer; they are now only
//! counted here.
use aura_domain_common::{ModelStepId, TransportAttemptId};
use aura_model_domain::{ModelStreamEvent, ModelStreamEventEnvelope};
use aura_persistence_v2::agent::desktop::LiveToolInput;
use std::{
    collections::BTreeMap,
    sync::{Mutex, MutexGuard, PoisonError},
};

/// How much of an input is kept: enough for the `"path"` a write names first.
const HEAD_BYTES: usize = 1024;

struct Call {
    provider_call_id: String,
    input: LiveToolInput,
}

type Steps = BTreeMap<ModelStepId, (TransportAttemptId, Vec<Call>)>;
static LIVE: Mutex<Steps> = Mutex::new(BTreeMap::new());

fn live() -> MutexGuard<'static, Steps> {
    LIVE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Takes a stream event that only says how a tool call's input is growing. True when it was
/// one: nothing of it is stored. A new attempt of a step starts its calls again.
pub(super) fn observe(envelope: &ModelStreamEventEnvelope) -> bool {
    let (step, attempt) = (envelope.model_step_id, envelope.transport_attempt_id);
    let mut live = live();
    match &envelope.event {
        ModelStreamEvent::ToolCallStarted { provider_call_id, tool_name, .. } => {
            calls(&mut live, step, attempt).push(Call {
                provider_call_id: provider_call_id.to_string(),
                input: LiveToolInput {
                    wire_name: tool_name.clone(),
                    received_bytes: 0,
                    head: String::new(),
                    complete: false,
                },
            });
            true
        }
        ModelStreamEvent::ToolCallArgumentsDelta { provider_call_id, delta } => {
            let id = provider_call_id.to_string();
            if let Some(call) =
                calls(&mut live, step, attempt).iter_mut().find(|call| call.provider_call_id == id)
            {
                call.input.received_bytes += delta.len() as u64;
                let room = HEAD_BYTES.saturating_sub(call.input.head.len());
                if room > 0 {
                    let end = (0..=room.min(delta.len()))
                        .rev()
                        .find(|end| delta.is_char_boundary(*end))
                        .unwrap_or(0);
                    call.input.head.push_str(&delta[..end]);
                }
            }
            true
        }
        ModelStreamEvent::ToolCallCompleted { provider_call_id } => {
            let id = provider_call_id.to_string();
            if let Some(call) =
                calls(&mut live, step, attempt).iter_mut().find(|call| call.provider_call_id == id)
            {
                call.input.complete = true;
            }
            true
        }
        _ => false,
    }
}

/// The calls of a step's current attempt; an attempt that replaced another starts afresh.
fn calls(live: &mut Steps, step: ModelStepId, attempt: TransportAttemptId) -> &mut Vec<Call> {
    let entry = live.entry(step).or_insert_with(|| (attempt, vec![]));
    if entry.0 != attempt {
        *entry = (attempt, vec![]);
    }
    &mut entry.1
}

/// The step's stream ended, however it ended.
pub(super) fn forget(step: ModelStepId) {
    live().remove(&step);
}

/// Every running step's tool input, for the conversation's view.
pub(super) fn snapshot() -> BTreeMap<ModelStepId, (TransportAttemptId, Vec<LiveToolInput>)> {
    live()
        .iter()
        .map(|(step, (attempt, calls))| {
            (*step, (*attempt, calls.iter().map(|call| call.input.clone()).collect()))
        })
        .collect()
}

/// Forgets the step when its stream loop ends, by return, error or cancellation.
pub(super) struct Forget(pub(super) ModelStepId);

impl Drop for Forget {
    fn drop(&mut self) {
        forget(self.0);
    }
}

#[cfg(test)]
#[path = "tool_input_tests.rs"]
mod tests;
