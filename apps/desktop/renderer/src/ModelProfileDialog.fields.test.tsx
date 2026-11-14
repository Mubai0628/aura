import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { ModelProfileDialog } from './ModelProfileDialog';
import { MODEL_ENDPOINT_ID, MODEL_PROFILE_ID, modelSettingsProjection } from './app-test-fixtures';

// A field whose value cannot be saved says why under it, and the grey 保存 says the same, so the
// user can tell what to fix.

beforeEach(() => {
  Object.assign(window, { aura: { v1: { updateModelProfile: vi.fn() } } });
});
afterEach(cleanup);

const dialog = (existing: boolean) => {
  const settings = structuredClone(modelSettingsProjection);
  render(
    <ModelProfileDialog
      endpoint={settings.registry.endpoints[MODEL_ENDPOINT_ID]!}
      onClose={vi.fn()}
      onNotify={vi.fn()}
      onSaved={vi.fn(async () => {})}
      {...(existing ? { profile: settings.registry.models[MODEL_PROFILE_ID]! } : {})}
      settings={settings}
    />,
  );
};

const save = () => screen.getByRole('button', { name: '保存 Profile' }) as HTMLButtonElement;

test('a number that cannot be saved is marked with its reason', async () => {
  const user = userEvent.setup();
  dialog(true);
  const window = screen.getByLabelText('上下文窗口 Tokens') as HTMLInputElement;
  await user.clear(window);
  await user.type(window, '0');
  expect(window.getAttribute('aria-invalid')).toBe('true');
  expect(screen.getByText('上下文窗口要大于 0', { selector: 'small' })).toBeTruthy();
  expect(save().disabled).toBe(true);
  expect(screen.getByRole('tooltip').textContent).toBe('上下文窗口要大于 0');

  const output = screen.getByLabelText('最大输出 Tokens') as HTMLInputElement;
  await user.clear(output);
  await user.type(output, '0');
  expect(screen.getByText('最大输出要大于 0', { selector: 'small' })).toBeTruthy();

  await user.clear(window);
  await user.type(window, '8000');
  expect(window.getAttribute('aria-invalid')).toBe('false');
  expect(screen.getByRole('tooltip').textContent).toBe('最大输出要大于 0');
});

test('a name that was emptied is marked; a new profile says on 保存 what is missing', async () => {
  const user = userEvent.setup();
  dialog(true);
  await user.clear(screen.getByLabelText('显示名称'));
  expect(screen.getByText('请填写显示名称', { selector: 'small' })).toBeTruthy();
  cleanup();

  dialog(false);
  expect(save().disabled).toBe(true);
  expect(screen.queryByText('请填写模型标识', { selector: 'small' })).toBeNull();
  expect(screen.getByRole('tooltip').textContent).toBe('请填写模型标识');
});
