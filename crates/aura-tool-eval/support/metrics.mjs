// Only identifiers, counters and executed scripts leave the isolated Core projection.
export function observe(view, stats) {
  for (const turn of view.turns) {
    for (const message of turn.assistant_messages) {
      stats.models.add(message.model_step_id);
      for (const id of message.tool_call_ids) stats.calls.add(id);
    }
    if (/binding_invalid|unknown_tool/.test(turn.diagnostic ?? turn.turn.state?.reason ?? ''))
      stats.unknown.add(turn.turn.turn_task_id);
  }
  if (stats.calls.size > 10000) throw new Error('eval.too_many_tool_calls');
}

export async function history(page, session_id, stats) {
  const read = (within_turn) =>
    page.evaluate(
      async ({ session_id, within_turn }) => {
        const r = await globalThis.window.aura.v1.readAgentHistory({
          session_id,
          within_turn,
          limit: 50,
        });
        if (!r.ok) throw new Error(r.error.code);
        return r.value;
      },
      { session_id, within_turn },
    );
  const view = await read(null);
  observe(view, stats);
  for (const turn of view.turns) {
    let cursor = turn.next_messages;
    let pages = 0;
    while (cursor) {
      if (++pages > 1000) throw new Error('eval.history_pagination_unbounded');
      const older = await read(cursor);
      observe(older, stats);
      cursor = older.turns.find(
        (t) => t.turn.turn_task_id === turn.turn.turn_task_id,
      )?.next_messages;
    }
  }
  return view;
}

export async function results(page, session_id, stats) {
  return page.evaluate(
    async ({ session_id, calls, unknown }) => {
      const api = globalThis.window.aura.v1;
      const shell_commands = [];
      const parameter_errors = { envelope: 0, schema: 0, business: 0, unknown_tool: unknown };
      const non_parameter_outcomes = {
        approval_denied: 0,
        permission_denied: 0,
        cancelled: 0,
        external_or_runtime: 0,
        indeterminate: 0,
        configuration: 0,
        unclassified: 0,
      };
      let tool_errors = 0;
      let unknown_results = 0;
      let stop_confirmed = true;
      const by_tool = new Map();
      if (api.readAgentToolActivity) {
        for (let start = 0; start < calls.length; start += 100) {
          const activity = await api.readAgentToolActivity({
            session_id,
            before_call_id: null,
            limit: 100,
            call_ids: calls.slice(start, start + 100),
          });
          if (!activity.ok) throw new Error('eval.tool_activity_unavailable');
          for (const item of activity.value.calls) {
            const summary = item.summary;
            const counter = by_tool.get(summary.tool_id) ?? {
              tool_id: summary.tool_id,
              display_name: summary.display_name,
              count: 0,
            };
            counter.count++;
            by_tool.set(summary.tool_id, counter);
          }
        }
      }
      for (const id of calls) {
        const shell = await api.readShellExecution({
          session_id,
          input: { tool_call_id: id, stdout_offset: 0, stderr_offset: 0, max_bytes: 1 },
        });
        if (shell.ok && shell.value.process_id != null) {
          const original = shell.value;
          // Stop is a user-control route, independent of the model's mode or approval.
          const stopped = await api.stopShellExecution({ session_id, input: { tool_call_id: id } });
          const confirmed =
            stopped.ok &&
            ['completed', 'failed', 'interrupted'].includes(stopped.value.state) &&
            !['running', 'unknown'].includes(stopped.value.background);
          stop_confirmed &&= confirmed;
          shell_commands.push({
            tool_call_id: id,
            script: original.submitted_script ?? original.original_input.script,
            exit_code: original.exit_code,
            state: original.state,
            stop_confirmed: confirmed,
          });
        }
        const result = await api.readAgentResult({
          session_id,
          tool_call_id: id,
          offset: 0,
          limit: 65536,
        });
        if (!result.ok) continue;
        let r;
        try {
          r = JSON.parse(result.value.text);
        } catch {
          continue;
        }
        const outcome = r.outcome ?? r;
        if (
          ['not_executed', 'operation_rejected', 'failed', 'cancelled', 'indeterminate'].includes(
            outcome.type,
          )
        ) {
          tool_errors++;
          const code = String(outcome.code ?? outcome.error?.code ?? r.error?.code ?? '');
          const category = outcome.error?.category;
          const reason = outcome.reason;
          // These are Core-owned envelopes (`aura_purpose` is envelope 3's field). Only the
          // classification leaves this page;
          // messages, request bodies and outputs never enter the evaluation ledger.
          const envelope =
            reason === 'purpose_required' ||
            (reason === 'schema_validation_failed' &&
              /`_aura`|\/_aura|`arguments`|\/arguments|aura_purpose|not a complete JSON value|appears twice in the same JSON object/.test(
                outcome.error?.message ?? '',
              ));
          if (reason === 'approval_denied') non_parameter_outcomes.approval_denied++;
          else if (reason === 'permission_denied' || category === 'permission')
            non_parameter_outcomes.permission_denied++;
          else if (outcome.type === 'cancelled' || category === 'cancelled')
            non_parameter_outcomes.cancelled++;
          else if (outcome.type === 'indeterminate') non_parameter_outcomes.indeterminate++;
          else if (
            /credential_rebind_required|credential_unavailable|snapshot_unavailable|disabled/.test(
              code,
            ) ||
            reason === 'disabled'
          )
            non_parameter_outcomes.configuration++;
          else if (
            reason === 'tool_not_found' ||
            /tool_not_found|unknown_tool|binding_invalid/.test(code)
          )
            unknown_results++;
          else if (envelope) parameter_errors.envelope++;
          else if (reason === 'schema_validation_failed' || /^tool\.schema\./.test(code))
            parameter_errors.schema++;
          else if (category === 'validation') parameter_errors.business++;
          else if (
            ['unavailable', 'transport', 'protocol', 'internal'].includes(category) ||
            reason === 'runtime_unavailable'
          )
            non_parameter_outcomes.external_or_runtime++;
          else non_parameter_outcomes.unclassified++;
        }
      }
      // A binding abort may be visible in both history and its result. Avoid double counting it.
      parameter_errors.unknown_tool = Math.max(parameter_errors.unknown_tool, unknown_results);
      return {
        tool_calls: calls.length,
        tool_call_counts: [...by_tool.values()].sort((a, b) => a.tool_id.localeCompare(b.tool_id)),
        tool_errors,
        parameter_errors,
        non_parameter_outcomes,
        shell_commands,
        stop_confirmed,
      };
    },
    { session_id, calls: [...stats.calls], unknown: stats.unknown.size },
  );
}
