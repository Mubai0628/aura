import assert from 'node:assert/strict';
import { test } from 'node:test';
import { observe, results, history } from './metrics.mjs';

test('history uses a valid Core page limit and follows the within-turn cursor', async () => {
  const requests = [];
  const stats = { calls: new Set(), models: new Set(), unknown: new Set() };
  const cursor = {
    turn_task_id: 'turn',
    before_model_step_id: 'model',
    before_user_event_id: null,
  };
  globalThis.window = {
    aura: {
      v1: {
        readAgentHistory: async (input) => {
          requests.push(input);
          if (input.limit < 1 || input.limit > 50)
            return { ok: false, error: { code: 'agent.request_rejected' } };
          return {
            ok: true,
            value: {
              turns: [
                {
                  turn: { turn_task_id: 'turn' },
                  next_messages: input.within_turn ? null : cursor,
                  assistant_messages: [
                    {
                      model_step_id: input.within_turn ? 'first' : 'later',
                      tool_call_ids: [input.within_turn ? 'shell-1' : 'shell-2'],
                    },
                  ],
                },
              ],
            },
          };
        },
      },
    },
  };
  try {
    await history({ evaluate: (callback, input) => callback(input) }, 'session', stats);
    assert.equal(requests.length, 2);
    assert.deepEqual(requests[1].within_turn, cursor);
    assert.deepEqual([...stats.calls].sort(), ['shell-1', 'shell-2']);
  } finally {
    delete globalThis.window;
  }
});

test('history observations retain every tool call and do not double count repeated polls', () => {
  const stats = { calls: new Set(), models: new Set(), unknown: new Set() };
  const view = (model, calls) => ({
    turns: [
      {
        turn: { turn_task_id: 'turn' },
        assistant_messages: [{ model_step_id: model, tool_call_ids: calls }],
      },
    ],
  });
  observe(view('first', ['shell-1']), stats);
  observe(view('first', ['shell-1']), stats);
  observe(view('later', ['shell-2']), stats);
  assert.deepEqual([...stats.calls], ['shell-1', 'shell-2']);
  assert.equal(stats.models.size, 2);
});

test('parameter errors exclude approval denials, permission changes and external failures', async () => {
  const outcomes = [
    { type: 'not_executed', reason: 'approval_denied' },
    { type: 'not_executed', reason: 'permission_denied' },
    { type: 'cancelled', reason: 'user_stopped' },
    {
      type: 'failed',
      error: { code: 'tool.runtime.http_gateway_failed', category: 'unavailable' },
    },
    {
      type: 'indeterminate',
      error: { code: 'tool.runtime.dispatch_outcome_unknown', category: 'unavailable' },
    },
    {
      type: 'not_executed',
      reason: 'schema_validation_failed',
      error: {
        code: 'tool.schema.arguments_invalid',
        category: 'validation',
        message: 'The top level may contain only `_aura` and `arguments`.',
      },
    },
    {
      type: 'not_executed',
      reason: 'schema_validation_failed',
      error: {
        code: 'tool.schema.arguments_invalid',
        category: 'validation',
        message: 'The call was not executed: /aura_purpose must be a non-empty text.',
      },
    },
    {
      type: 'not_executed',
      reason: 'schema_validation_failed',
      error: {
        code: 'tool.schema.arguments_invalid',
        category: 'validation',
        message: 'Correct the following fields: /limit: minimum',
      },
    },
    {
      type: 'operation_rejected',
      error: { code: 'tool.resource.http_request_invalid', category: 'validation' },
    },
    { type: 'not_executed', reason: 'tool_not_found' },
  ];
  const page = { evaluate: (callback, args) => callback(args) };
  globalThis.window = {
    aura: {
      v1: {
        readShellExecution: async () => ({ ok: false }),
        readAgentToolActivity: async ({ call_ids }) => ({
          ok: true,
          value: {
            calls: call_ids.map((id) => ({
              summary: {
                tool_id: Number(id) % 2 ? 'B' : 'A',
                display_name: Number(id) % 2 ? 'second tool' : 'first tool',
              },
            })),
          },
        }),
        readAgentResult: async ({ tool_call_id }) => ({
          ok: true,
          value: { text: JSON.stringify({ outcome: outcomes[Number(tool_call_id)] }) },
        }),
      },
    },
  };
  try {
    const report = await results(page, 'session', {
      calls: new Set(outcomes.map((_, i) => String(i))),
      unknown: new Set(),
    });
    assert.deepEqual(report.parameter_errors, {
      envelope: 2,
      schema: 1,
      business: 1,
      unknown_tool: 1,
    });
    assert.deepEqual(report.non_parameter_outcomes, {
      approval_denied: 1,
      permission_denied: 1,
      cancelled: 1,
      external_or_runtime: 1,
      indeterminate: 1,
      configuration: 0,
      unclassified: 0,
    });
    assert.equal(report.tool_errors, outcomes.length);
    assert.deepEqual(
      report.tool_call_counts.map((v) => [v.tool_id, v.count]),
      [
        ['A', 5],
        ['B', 5],
      ],
    );
  } finally {
    delete globalThis.window;
  }
});
