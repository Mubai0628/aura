import type { SessionModelSelectorProjection } from '@aura/contracts/generated';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { ModelQuickSelector } from './ComposerQuickMenus';
import { MODEL_ENDPOINT_ID, MODEL_PROFILE_ID, MODEL_PROVIDER_ID } from './app-test-fixtures';
import { forgetSessionSpeed, sessionSpeed } from './speed-mode';

// Beside the session's model and its thinking level, the turn's speed mode: 标准, then each mode
// the model's settings allow. The choice is the conversation's own, per model, and changes
// nothing in the model's settings; a model that allows none shows no choice.

const SESSION = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const OTHER = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAW';
const ready = { is_executable: true, issue_code: null } as never;
const selector = (session: string, modes: string[]): SessionModelSelectorProjection =>
  ({
    session_id: session,
    binding_revision: '2',
    selected_model_profile_id: MODEL_PROFILE_ID,
    default_model_profile_id: null,
    providers: [{ provider_id: MODEL_PROVIDER_ID, display_name: 'Codex', order: 0 }],
    endpoints: [
      {
        endpoint_id: MODEL_ENDPOINT_ID,
        provider_id: MODEL_PROVIDER_ID,
        display_name: 'Responses',
        api_format: 'open_ai_responses',
        order: 0,
      },
    ],
    models: [
      {
        model_profile_id: MODEL_PROFILE_ID,
        endpoint_id: MODEL_ENDPOINT_ID,
        display_name: 'gpt-6.1-sol',
        provider_model_name: 'gpt-6.1-sol',
        effective_context_window_tokens: 1_047_576,
        effective_max_output_tokens: 32_768,
        execution_readiness: ready,
        order: 0,
        ...(modes.length ? { speed_modes: modes } : {}),
      },
    ],
    selected_model_execution_readiness: ready,
    projection_revision: '3',
  }) as never;

const update = vi.fn();
beforeEach(() => {
  localStorage.clear();
  forgetSessionSpeed();
  update.mockReset();
  Object.assign(window, { aura: { v1: { updateModelProfile: update } } });
});
afterEach(cleanup);

const quick = (projection: SessionModelSelectorProjection) =>
  render(
    <ModelQuickSelector
      onManageSettings={vi.fn()}
      onSelect={vi.fn(async () => {})}
      projection={projection}
    />,
  );
const items = () => screen.getAllByRole('menuitemradio').map((item) => item.textContent);

test('a conversation turns fast mode on and off for its model; the settings are untouched', async () => {
  const user = userEvent.setup();
  quick(selector(SESSION, ['fast', 'ultrafast']));
  const standard = screen.getByRole('button', { name: '速度：标准' });
  expect(standard.dataset.on).toBe('false');
  await user.click(standard);
  expect(items()).toEqual(['标准', '快速', '极速']);
  await user.click(screen.getByRole('menuitemradio', { name: '快速' }));
  const fast = screen.getByRole('button', { name: '速度：快速' });
  expect(fast.dataset.on).toBe('true');
  expect(sessionSpeed(SESSION, MODEL_PROFILE_ID)).toBe('fast');
  expect(update).not.toHaveBeenCalled();

  // Another conversation with the same model is still in the standard mode.
  cleanup();
  quick(selector(OTHER, ['fast', 'ultrafast']));
  expect(screen.getByRole('button', { name: '速度：标准' })).toBeTruthy();

  // Back to the first, after the window reloads: still fast; 标准 turns it off.
  cleanup();
  forgetSessionSpeed();
  quick(selector(SESSION, ['fast', 'ultrafast']));
  await user.click(screen.getByRole('button', { name: '速度：快速' }));
  await user.click(screen.getByRole('menuitemradio', { name: '标准' }));
  expect(screen.getByRole('button', { name: '速度：标准' })).toBeTruthy();
  expect(sessionSpeed(SESSION, MODEL_PROFILE_ID)).toBeUndefined();
});

test('a model whose settings allow no speed mode shows no choice', () => {
  quick(selector(SESSION, []));
  expect(screen.queryByRole('button', { name: /^速度/u })).toBeNull();
});

test('a mode the model no longer allows reads as the standard mode', () => {
  localStorage.setItem(
    'aura.session-speed.v1',
    JSON.stringify({ [SESSION]: { [MODEL_PROFILE_ID]: 'ultrafast' } }),
  );
  forgetSessionSpeed();
  quick(selector(SESSION, ['fast']));
  expect(screen.getByRole('button', { name: '速度：标准' }).dataset.on).toBe('false');
});
