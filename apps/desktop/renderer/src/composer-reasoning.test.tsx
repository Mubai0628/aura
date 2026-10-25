import type { SessionModelSelectorProjection } from '@aura/contracts/generated';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { ModelQuickSelector } from './ComposerQuickMenus';
import { MODEL_ENDPOINT_ID, MODEL_PROFILE_ID, MODEL_PROVIDER_ID } from './app-test-fixtures';
import { forgetSessionReasoning, sessionReasoning } from './reasoning-effort';

// Beside the session's model, its thinking level.
// The choice is the conversation's own,
// per model; it changes nothing in the model's settings, and a conversation that chose none
// follows the model's default.

const SESSION = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const OTHER = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAW';
const ready = { is_executable: true, issue_code: null } as never;
const selector = (
  session: string,
  levels: string[],
  fallback: string | null = 'low',
): SessionModelSelectorProjection =>
  ({
    session_id: session,
    binding_revision: '2',
    selected_model_profile_id: MODEL_PROFILE_ID,
    default_model_profile_id: null,
    providers: [{ provider_id: MODEL_PROVIDER_ID, display_name: 'OpenAI', order: 0 }],
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
        reasoning_efforts: levels,
        reasoning_effort: fallback,
      },
    ],
    selected_model_execution_readiness: ready,
    projection_revision: '3',
  }) as never;

const update = vi.fn();
beforeEach(() => {
  localStorage.clear();
  forgetSessionReasoning();
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

test('a conversation keeps its own level for its model; the model settings are untouched', async () => {
  const user = userEvent.setup();
  quick(selector(SESSION, ['low', 'medium', 'high', 'xhigh', 'max']));
  // No choice yet: the model's default, which the settings hold. Named with the level it stands for.
  await user.click(screen.getByRole('button', { name: '思考程度：默认（低）' }));
  expect(screen.getByRole('menu', { name: '思考程度' }).textContent).toContain('超高');
  await user.click(screen.getByRole('menuitemradio', { name: '最高' }));
  expect(screen.getByRole('button', { name: '思考程度：最高' })).toBeTruthy();
  expect(sessionReasoning(SESSION, MODEL_PROFILE_ID)).toBe('max');
  expect(update).not.toHaveBeenCalled();

  // Another conversation with the same model still follows the default.
  cleanup();
  quick(selector(OTHER, ['low', 'medium', 'high', 'xhigh', 'max']));
  expect(screen.getByRole('button', { name: '思考程度：默认（低）' })).toBeTruthy();

  // Back to the first, after the window reloads: its choice is still there; 默认 clears it.
  cleanup();
  forgetSessionReasoning();
  quick(selector(SESSION, ['low', 'medium', 'high', 'xhigh', 'max']));
  await user.click(screen.getByRole('button', { name: '思考程度：最高' }));
  await user.click(screen.getByRole('menuitemradio', { name: '默认（低）' }));
  expect(sessionReasoning(SESSION, MODEL_PROFILE_ID)).toBeUndefined();
});

test("a model whose settings leave the level to the provider shows the provider's default", () => {
  quick(selector(SESSION, ['low', 'high'], null));
  expect(screen.getByRole('button', { name: '思考程度：默认' })).toBeTruthy();
});

test('a model offering no level shows no thinking choice', () => {
  quick(selector(SESSION, []));
  expect(screen.queryByRole('button', { name: /思考程度/u })).toBeNull();
});
