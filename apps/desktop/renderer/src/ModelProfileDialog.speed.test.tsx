import { act, cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { isP05MainParams } from '../../shared/p05-validation';
import { ModelProfileDialog } from './ModelProfileDialog';
import {
  MODEL_ENDPOINT_ID,
  MODEL_PROFILE_ID,
  SECOND_MODEL_PROFILE_ID,
  modelSettingsProjection,
} from './app-test-fixtures';

// A model whose catalog entry offers speed modes has a switch that allows them, with a question
// mark that says to make sure the provider supports them. Allowed, a conversation can choose them
// in the composer; the switch is saved with the model's other settings.

const update = vi.fn();
beforeEach(() => {
  update.mockReset();
  update.mockResolvedValue({
    ok: true,
    value: { revision: '9', value: { id: MODEL_PROFILE_ID, display_name: 'gpt-6.1-sol' } },
  });
  Object.assign(window, { aura: { v1: { updateModelProfile: update } } });
});
afterEach(cleanup);

const dialog = (
  allowed: boolean | undefined,
  modelProfileId: string | null = MODEL_PROFILE_ID,
  stored?: string,
) => {
  const settings = structuredClone(modelSettingsProjection);
  const profile = settings.registry.models[MODEL_PROFILE_ID]!;
  if (allowed !== undefined) profile.request_defaults.allow_speed_modes = allowed;
  if (stored) profile.request_defaults.speed_mode = stored;
  settings.speed_modes =
    modelProfileId !== null
      ? [
          {
            model_profile_id: modelProfileId,
            standard_price: null,
            modes: [
              { name: 'fast', price: null },
              { name: 'ultrafast', price: null },
            ],
          },
        ]
      : [];
  render(
    <ModelProfileDialog
      endpoint={settings.registry.endpoints[MODEL_ENDPOINT_ID]!}
      onClose={vi.fn()}
      onNotify={vi.fn()}
      onSaved={vi.fn(async () => {})}
      profile={profile}
      settings={settings}
    />,
  );
};

// What the dialog sends, which the main process must accept as it is.
const sent = () => {
  const request = update.mock.calls[0]![0];
  expect(isP05MainParams('models.profile.update', request)).toBe(true);
  return request.request_defaults;
};
const SWITCH = '允许使用快速模式';

test('fast mode starts off; allowing it is saved, with the hint on the question mark', async () => {
  const user = userEvent.setup();
  dialog(undefined);
  const allow = screen.getByRole('switch', { name: SWITCH });
  expect(allow.getAttribute('aria-checked')).toBe('false');
  expect(screen.queryByRole('tooltip')).toBeNull();
  await user.hover(screen.getByRole('img', { name: '快速模式' }));
  expect(screen.getByRole('tooltip').textContent).toBe('请确保你的提供商支持该模式');
  await user.unhover(screen.getByRole('img', { name: '快速模式' }));
  expect(screen.queryByRole('tooltip')).toBeNull();
  await user.click(allow);
  expect(allow.getAttribute('aria-checked')).toBe('true');
  await user.click(screen.getByRole('button', { name: /保存/u }));
  await waitFor(() => expect(update).toHaveBeenCalledOnce());
  expect(sent()).toMatchObject({ allow_speed_modes: true, speed_mode: null });
});

test('the keyboard reaches the question mark and its hint', async () => {
  const user = userEvent.setup();
  dialog(true);
  act(() => screen.getByRole('img', { name: '快速模式' }).focus());
  expect(screen.getByRole('tooltip').textContent).toBe('请确保你的提供商支持该模式');
  await user.tab();
  expect(screen.queryByRole('tooltip')).toBeNull();
});

test('an allowed model shows it on; turning it off is saved, and an older named mode goes', async () => {
  const user = userEvent.setup();
  dialog(true, MODEL_PROFILE_ID, 'fast');
  const allow = screen.getByRole('switch', { name: SWITCH });
  expect(allow.getAttribute('aria-checked')).toBe('true');
  await user.click(allow);
  await user.click(screen.getByRole('button', { name: /保存/u }));
  await waitFor(() => expect(update).toHaveBeenCalledOnce());
  expect(sent()).toMatchObject({ allow_speed_modes: false, speed_mode: null });
});

test('a model whose catalog entry offers no speed mode has no switch', () => {
  dialog(undefined, SECOND_MODEL_PROFILE_ID);
  expect(screen.queryByRole('switch', { name: SWITCH })).toBeNull();
  cleanup();
  dialog(true, null);
  expect(screen.queryByRole('switch', { name: SWITCH })).toBeNull();
});
