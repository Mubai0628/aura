use super::*;

pub(in crate::agent_application) fn protocol_completion_reminder(
    mode: AgentTaskMode,
    disposition: bool,
) -> serde_json::Value {
    if disposition {
        return serde_json::json!({
            "instruction": "The previous response ended with EndTurn and no tool calls, and this turn is not completed. This Plan or Goal is waiting for mode.disposition. continue keeps the mode; skip marks it skipped and answers as Normal; query.user_question asks when intent is unclear. Single-turn completion is not mode completion. This reminder is not a user message saying to continue."
        });
    }
    let (completion_hint, mode_tool) = match mode {
        AgentTaskMode::Normal => (
            "Answer the request with your final response when the work is done; a complete normal answer ends the turn.",
            "",
        ),
        AgentTaskMode::Plan => (
            "Plan stays active across turns, so finish this single turn through normal.complete with the user-visible answer.",
            " Call plan.complete only if the entire Plan mode is finished.",
        ),
        AgentTaskMode::Goal => (
            "Goal stays active across turns, so finish this single turn through normal.complete with the user-visible answer.",
            " Call goal.complete only if the entire Goal mode is finished.",
        ),
    };
    serde_json::json!({
        "instruction": format!(
            "The previous response ended without a usable final answer, so this turn is not completed. {completion_hint} If more work remains, call the appropriate tool. If you need user input, call query.user_question.{mode_tool} An empty, truncated or interrupted response never ends the turn. This reminder is not a user message saying to continue."
        )
    })
}

/// What the reminder adds when the previous response ran out of output tokens before it
/// finished: while reasoning, or while writing a tool call that was then dropped. A thinking
/// model counts its reasoning against the output limit. Told only
/// that the answer was missing, DeepSeek's thinking mode reasoned up to the same 8192-token
/// limit eleven times in a row in one evaluation sample before it acted.
const CUT_OFF_WHILE_REASONING: &str = "The previous response reached the output-token limit before it finished, so it contained no answer and no complete tool call. Reasoning counts toward that limit: keep this response's reasoning short, decide the next step, and call a tool or answer early.";

/// A response that stopped at the output limit with no visible text and no complete tool call.
pub(in crate::agent_application) fn cut_off_while_reasoning(
    output: &aura_model_domain::NormalizedModelOutput,
) -> bool {
    output.stop_reason == aura_model_domain::NormalizedStopReason::MaxOutputTokens
        && output.tool_calls.is_empty()
        && aura_persistence_v2::agent::history::visible_text(output).trim().is_empty()
}

/// The completion reminder, opened with the reason the previous response was empty.
pub(in crate::agent_application) fn output_limit_reminder(
    mut reminder: serde_json::Value,
) -> serde_json::Value {
    if let Some(serde_json::Value::String(text)) = reminder.get_mut("instruction") {
        *text = format!("{CUT_OFF_WHILE_REASONING} {text}");
    }
    reminder
}

pub(super) fn consumed_addition_orders(
    db: &aura_persistence_core::Connection,
    turn: &TurnTask,
) -> anyhow::Result<std::collections::BTreeMap<EventId, (u32, Revision)>> {
    let mut order = 0_u32;
    let mut orders = std::collections::BTreeMap::new();
    for group in inbox::consumed(db, turn.turn_task_id)? {
        if group.destination != (AgentInputLane::Addition { turn_task_id: turn.turn_task_id }) {
            continue;
        }
        for id in group.user_event_ids {
            order = order
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("agent.addition_tail_invalid"))?;
            let revision = group
                .message_revisions
                .iter()
                .find(|(event, _)| *event == id)
                .map(|(_, revision)| *revision)
                .unwrap_or(Revision::INITIAL);
            if orders.insert(id, (order, revision)).is_some() {
                anyhow::bail!("agent.addition_tail_invalid");
            }
        }
    }
    Ok(orders)
}

pub(super) fn history_anchor(
    items: &[LogicalConversationItem],
    assistant_event_id: EventId,
    tool_results: usize,
) -> anyhow::Result<usize> {
    let index = items
        .iter()
        .position(|item| {
            matches!(item, LogicalConversationItem::Assistant { source_event_id, .. } if *source_event_id == assistant_event_id)
        })
        .ok_or_else(|| anyhow::anyhow!("agent.addition_anchor_missing"))?;
    let end = index
        .checked_add(1)
        .and_then(|end| end.checked_add(tool_results))
        .ok_or_else(|| anyhow::anyhow!("agent.addition_tail_invalid"))?;
    if end > items.len() {
        anyhow::bail!("agent.addition_tail_invalid");
    }
    Ok(end)
}

pub(super) fn trailing_addition_index(
    items: &[LogicalConversationItem],
    orders: &std::collections::BTreeMap<EventId, (u32, Revision)>,
) -> usize {
    let mut index = items.len();
    while index > 0 {
        match &items[index - 1] {
            LogicalConversationItem::User { source_event_id, .. }
                if orders.contains_key(source_event_id) =>
            {
                index -= 1;
            }
            _ => break,
        }
    }
    index
}

/// The items after a completed group must be this turn's consumed additions,
/// in consumption order. An empty tail leaves the historical suffix unchanged.
pub(super) fn addition_suffix(
    db: &aura_persistence_core::Connection,
    turn: &TurnTask,
    items: &[LogicalConversationItem],
    after: usize,
) -> anyhow::Result<Vec<AcceptedTurnAdditionV1>> {
    let orders = consumed_addition_orders(db, turn)?;
    if after > items.len() {
        anyhow::bail!("agent.addition_tail_invalid");
    }
    let mut suffix = Vec::new();
    let mut last_order = 0_u32;
    for item in &items[after..] {
        let LogicalConversationItem::User { source_event_id, content_hash, .. } = item else {
            anyhow::bail!("agent.addition_tail_invalid");
        };
        let Some((order, revision)) = orders.get(source_event_id) else {
            anyhow::bail!("agent.addition_tail_invalid");
        };
        if *order <= last_order || revision.value() < 1 {
            anyhow::bail!("agent.addition_tail_invalid");
        }
        last_order = *order;
        suffix.push(AcceptedTurnAdditionV1 {
            user_event_id: *source_event_id,
            content_hash: content_hash.clone(),
            session_id: turn.session_id,
            turn_task_id: turn.turn_task_id,
            revision: *revision,
            order: *order,
        });
    }
    Ok(suffix)
}
