import { expect, test } from 'vitest';
import { validateAgentRequest, validateAgentResponse } from './agent-validation.js';

const session = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const model = 'mdl_01ARZ3NDEKTSV4RRFFQ69G5FAV';

test('old messages remain valid and each user entry accepts only the two desktop modes', () => {
  const requests = [
    {
      command: 'agent.submit',
      value: { session_id: session, text: 'fixture', destination: { lane: 'next_turn' } },
    },
    {
      command: 'agent.start',
      value: {
        session_id: session,
        model_profile_id: model,
        text: 'fixture',
        mode: 'normal',
        goal_budget: null,
      },
    },
    {
      command: 'agent.init.start',
      value: { session_id: session, model_profile_id: model, text: '' },
    },
    {
      command: 'agent.compaction.start',
      value: { session_id: session, model_profile_id: model, focus: null },
    },
  ];
  for (const request of requests) {
    expect(validateAgentRequest(request), request.command).toBe(true);
    for (const computer_mode of ['foreground', 'child_session']) {
      expect(
        validateAgentRequest({ ...request, value: { ...request.value, computer_mode } }),
        request.command,
      ).toBe(true);
    }
    expect(
      validateAgentRequest({ ...request, value: { ...request.value, computer_mode: 'fallback' } }),
    ).toBe(false);
  }
});

test('the real controlling projection accepts child status without requiring it on old results', () => {
  expect(validateAgentResponse({ command: 'computer.controlling', value: { apps: [] } })).toBe(
    true,
  );
  expect(
    validateAgentResponse({
      command: 'computer.controlling',
      value: {
        apps: [{ app_id: 'fixture', name: 'Fixture', foreground: true, mode: 'child_session' }],
        pip: { phase: 'waiting_for_login' },
      },
    }),
  ).toBe(true);
  expect(
    validateAgentResponse({
      command: 'computer.controlling',
      value: { apps: [], pip: { phase: 'pretend_ready' } },
    }),
  ).toBe(false);
});

test('giving the desktop back names only the conversation and answers what it controls', () => {
  expect(
    validateAgentRequest({ command: 'computer.give_back', value: { session_id: session } }),
  ).toBe(true);
  expect(
    validateAgentRequest({
      command: 'computer.give_back',
      value: { session_id: session, app_id: 'fixture' },
    }),
  ).toBe(false);
  expect(
    validateAgentResponse({
      command: 'computer.give_back',
      value: {
        apps: [{ app_id: 'fixture', name: 'Fixture', foreground: true, taken_over: true }],
      },
    }),
  ).toBe(true);
});
