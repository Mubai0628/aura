import type { AgentSessionProjection } from '@aura/contracts/generated';
import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { Conversation } from './Conversation';
import { agentDiagnosticIdentity } from './diagnostic-identity';

/** The bounded line Core stores for a refused Provider tool call. */
const STORED =
  'provider_tool_call_binding_invalid: wire name _aura is not in request step=stp_01ARZ3NDEKTSV4RRFFQ69G5FAV snapshot=mes_01arz3ndektsv4rrffq69g5fav attempt=tat_01ARZ3NDEKTSV4RRFFQ69G5FAW adapter=openai_responses@1.0 item=function_call frozen_tools=sha256:9f2c count=28';

beforeEach(() => {
  Object.defineProperty(globalThis, 'IntersectionObserver', {
    configurable: true,
    value: class {
      observe(): void {
        /* the sentinel is not the subject of these cases */
      }
      disconnect(): void {
        /* nothing to release in the fake */
      }
    },
  });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { startRelatedAgentTurn: vi.fn() } },
  });
});
afterEach(cleanup);

test('the identity line names the refusal, the item kind and the identities', () => {
  const identity = agentDiagnosticIdentity(STORED);
  expect(identity).toBe(
    'provider_tool_call_binding_invalid · item=function_call · adapter=openai_responses@1.0 · step=stp_01ARZ3NDEKTSV4RRFFQ69G5FAV · attempt=tat_01ARZ3NDEKTSV4RRFFQ69G5FAW',
  );
});

test('nothing outside the whitelisted tokens can reach the identity line', () => {
  const hostile =
    'provider_tool_call_binding_invalid: wire name _aura is not in request body={"secret":"sk-live-123"} step=ok item=<script>attempt=</script>';
  const identity = agentDiagnosticIdentity(hostile);
  expect(identity).toContain('step=ok');
  expect(identity).not.toContain('secret');
  expect(identity).not.toContain('sk-live-123');
  expect(identity).not.toContain('<script>');
  // A field that appears twice cannot be rewritten by the later copy.
  expect(agentDiagnosticIdentity(`${hostile} step=forged`)).toContain('step=ok');
  // Another diagnostic is not explained by this line.
  expect(agentDiagnosticIdentity('agent.transport_interrupted')).toBeUndefined();
  expect(agentDiagnosticIdentity(undefined)).toBeUndefined();
});

test('the alert shows the product sentence and the bounded identity', () => {
  const projection = {
    session_id: 'session',
    runtime_instance_id: 'runtime',
    active: null,
    turns: [
      {
        turn: {
          session_id: 'session',
          turn_task_id: 'turn-1',
          revision: '1',
          state: { state: 'recovery_required', reason: 'agent.execution_interrupted' },
        },
        user_messages: [],
        assistant_messages: [],
        plan: null,
        diagnostic: STORED,
      },
    ],
  } as unknown as AgentSessionProjection;
  render(
    <Conversation
      projection={projection}
      viewportRef={{ current: null }}
      onChanged={vi.fn()}
      loading={false}
      error={undefined}
      loadOlder={vi.fn()}
      loadTurnOlder={vi.fn()}
    />,
  );
  const alert = screen.getByRole('alert');
  expect(alert.textContent).toContain('模型返回的工具名称「_aura」不在这次请求的工具清单里');
  expect(alert.textContent).toContain('不会自动重放已完成的写入');
  expect(alert.textContent).toContain('item=function_call');
  expect(alert.textContent).toContain('attempt=tat_01ARZ3NDEKTSV4RRFFQ69G5FAW');
  expect(alert.textContent).not.toContain('frozen_tools');
});
