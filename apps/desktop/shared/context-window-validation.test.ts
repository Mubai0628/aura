import { expect, test } from 'vitest';
import { isAgentParams } from './agent-validation.js';

test('the generated correction route accepts only its bound user action', () => {
  const request = {
    session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV',
    agent_run_id: 'agt_01ARZ3NDEKTSV4RRFFQ69G5FAV',
    source_model_step_id: 'stp_01ARZ3NDEKTSV4RRFFQ69G5FAV',
    expected_revision: '1',
    action: 'try_smaller_windows',
  };
  expect(isAgentParams('agent.context.window.resolve', request)).toBe(true);
  for (const key of [
    'model_profile_id',
    'permission',
    'mode',
    'execution_snapshot_id',
    'window_tokens',
  ])
    expect(isAgentParams('agent.context.window.resolve', { ...request, [key]: 'untrusted' })).toBe(
      false,
    );
  expect(isAgentParams('agent.context.window.resolve', { ...request, action: 'replay' })).toBe(
    false,
  );
});
