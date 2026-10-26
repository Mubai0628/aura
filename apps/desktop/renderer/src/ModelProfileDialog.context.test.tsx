import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { ModelProfileDialog } from './ModelProfileDialog';
import { MODEL_ENDPOINT_ID, MODEL_PROFILE_ID, modelSettingsProjection } from './app-test-fixtures';

// Editing a model saves its context window with the rest of the profile, and the field shows the
// window the model really gets.

const update = vi.fn();
const correct = vi.fn();
const clear = vi.fn();
const onClose = vi.fn();
const onNotify = vi.fn();
beforeEach(() => {
  for (const mock of [update, correct, clear, onClose, onNotify]) mock.mockReset();
  update.mockResolvedValue({
    ok: true,
    value: { revision: '9', value: { id: MODEL_PROFILE_ID, display_name: 'gpt-5.6-sol' } },
  });
  correct.mockResolvedValue({ ok: true, value: { revision: '10' } });
  clear.mockResolvedValue({ ok: true, value: { revision: '10' } });
  Object.assign(window, {
    aura: {
      v1: {
        updateModelProfile: update,
        correctModelContextWindow: correct,
        clearModelContextWindowOverride: clear,
      },
    },
  });
});
afterEach(cleanup);

const dialog = (context: {
  catalog?: number | null;
  override?: number | null;
  effective: number;
}) => {
  const settings = structuredClone(modelSettingsProjection);
  const profile = settings.registry.models[MODEL_PROFILE_ID]!;
  if (context.catalog !== undefined) profile.context.catalog_window_tokens = context.catalog;
  if (context.override !== undefined)
    profile.context.user_override_window_tokens = context.override;
  profile.context.runtime_effective_window_tokens = context.effective;
  render(
    <ModelProfileDialog
      endpoint={settings.registry.endpoints[MODEL_ENDPOINT_ID]!}
      onClose={onClose}
      onNotify={onNotify}
      onSaved={vi.fn(async () => {})}
      profile={profile}
      settings={settings}
    />,
  );
};

const windowField = () => screen.getByLabelText('上下文窗口 Tokens') as HTMLInputElement;
const save = (user: ReturnType<typeof userEvent.setup>) =>
  user.click(screen.getByRole('button', { name: '保存 Profile' }));

test('a model with no own or catalog window shows the window it really gets', () => {
  dialog({ catalog: null, override: null, effective: 256_000 });
  expect(windowField().value).toBe('256000');
});

test('a changed window is saved after the profile, at the revision the profile save returned', async () => {
  const user = userEvent.setup();
  dialog({ catalog: null, override: null, effective: 256_000 });
  await user.clear(windowField());
  await user.type(windowField(), '512000');
  await save(user);
  expect(update).toHaveBeenCalledTimes(1);
  expect(correct).toHaveBeenCalledWith({
    expected_revision: '9',
    model_profile_id: MODEL_PROFILE_ID,
    window_tokens: 512_000,
  });
  expect(update.mock.invocationCallOrder[0]).toBeLessThan(correct.mock.invocationCallOrder[0]!);
  expect(onClose).toHaveBeenCalled();
});

test('saving another setting leaves the window alone', async () => {
  const user = userEvent.setup();
  dialog({ effective: 1_047_576 });
  await user.type(screen.getByLabelText('显示名称'), ' x');
  await save(user);
  expect(update).toHaveBeenCalledTimes(1);
  expect(correct).not.toHaveBeenCalled();
  expect(clear).not.toHaveBeenCalled();
  expect(screen.queryByRole('button', { name: '应用上下文修正' })).toBeNull();
});

test("emptying the field clears the user's own window", async () => {
  const user = userEvent.setup();
  dialog({ override: 200_000, effective: 200_000 });
  expect(windowField().value).toBe('200000');
  await user.clear(windowField());
  await save(user);
  expect(clear).toHaveBeenCalledWith({
    expected_revision: '9',
    model_profile_id: MODEL_PROFILE_ID,
  });
  expect(correct).not.toHaveBeenCalled();
});

test('a model the catalog knows saves a changed window the same way', async () => {
  const user = userEvent.setup();
  dialog({ catalog: 1_047_576, override: null, effective: 1_047_576 });
  await user.clear(windowField());
  await user.type(windowField(), '500000');
  await save(user);
  expect(correct).toHaveBeenCalledWith(
    expect.objectContaining({ expected_revision: '9', window_tokens: 500_000 }),
  );
});

test('a window that could not be saved keeps the dialog open and says the rest was saved', async () => {
  const user = userEvent.setup();
  correct.mockResolvedValue({
    ok: false,
    error: {
      code: 'models.revision_conflict',
      message: '模型配置已被别处修改，请重新打开后再保存。',
    },
  });
  dialog({ catalog: null, override: null, effective: 256_000 });
  await user.clear(windowField());
  await user.type(windowField(), '512000');
  await save(user);
  expect(onNotify).toHaveBeenCalledWith(
    expect.objectContaining({ title: '上下文窗口未保存', tone: 'danger' }),
  );
  expect(onNotify.mock.calls.at(-1)![0].message).toContain('其他设置已保存');
  expect(onClose).not.toHaveBeenCalled();
});
