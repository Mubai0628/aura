import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test, vi } from 'vitest';
import { ModelPicker, type ModelChoice } from './ModelPicker';

// The providers first, each opening
// its own list of models beside it; both lists keep within a height and scroll.

afterEach(cleanup);

const codex = Array.from({ length: 13 }, (_, n) => ({
  id: `codex-${n}`,
  label: `gpt-${n}`,
  provider: 'Codex',
  detail: '1',
}));
const choices: ModelChoice[] = [
  { id: 'flash', label: 'DeepSeek V4 Flash', provider: 'DeepSeek', detail: 'DeepSeek Responses' },
  { id: 'pro', label: 'DeepSeek V4 Pro', provider: 'DeepSeek', detail: 'DeepSeek Responses' },
  ...codex,
];

const picker = (selectedId: string, onSelect = vi.fn(async () => {})) =>
  render(
    <ModelPicker
      onManageSettings={vi.fn()}
      onSelect={onSelect}
      state={{ choices, selectedId, loading: false }}
    />,
  );

test('the menu lists the providers; a provider opens its models beside it', async () => {
  const user = userEvent.setup();
  const onSelect = vi.fn(async () => {});
  picker('flash', onSelect);
  await user.click(screen.getByRole('button', { name: /DeepSeek V4 Flash/u }));
  const menu = screen.getByRole('menu', { name: '快速选择模型' });
  const providers = within(menu).getByRole('group', { name: '提供商' });
  expect(
    within(providers)
      .getAllByRole('menuitem')
      .map((item) => item.textContent),
  ).toEqual(['DeepSeek2', 'Codex13']);
  expect(providers.className).toContain('model-menu-scroll');
  // The provider of the model in use is open at first, with that model checked.
  const deepseek = screen.getByRole('menu', { name: 'DeepSeek 的模型' });
  expect(within(deepseek).getByRole('menuitemradio', { name: /DeepSeek V4 Flash/u })).toBeTruthy();
  expect(
    within(deepseek)
      .getByRole('menuitemradio', { name: /DeepSeek V4 Flash/u })
      .getAttribute('aria-checked'),
  ).toBe('true');
  await user.click(within(providers).getByRole('menuitem', { name: /Codex/u }));
  const models = screen.getByRole('menu', { name: 'Codex 的模型' });
  expect(models.className).toContain('model-menu-scroll');
  expect(within(models).getAllByRole('menuitemradio')).toHaveLength(13);
  expect(screen.queryByRole('menu', { name: 'DeepSeek 的模型' })).toBeNull();
  await user.click(within(models).getByRole('menuitemradio', { name: /gpt-7/u }));
  expect(onSelect).toHaveBeenCalledWith('codex-7');
});

test('pointing at a provider opens its models; the keyboard opens them too', async () => {
  const user = userEvent.setup();
  picker('flash');
  await user.click(screen.getByRole('button', { name: /DeepSeek V4 Flash/u }));
  await user.hover(screen.getByRole('menuitem', { name: /Codex/u }));
  expect(screen.getByRole('menu', { name: 'Codex 的模型' })).toBeTruthy();
  screen.getByRole('menuitem', { name: /DeepSeek/u }).focus();
  await user.keyboard('{ArrowRight}');
  expect(screen.getByRole('menu', { name: 'DeepSeek 的模型' })).toBeTruthy();
  expect(document.activeElement).toBe(
    screen.getByRole('menuitemradio', { name: /DeepSeek V4 Flash/u }),
  );
});

test('a single provider lists its models at once', async () => {
  const user = userEvent.setup();
  render(
    <ModelPicker
      onManageSettings={vi.fn()}
      onSelect={vi.fn(async () => {})}
      state={{ choices: choices.slice(0, 2), selectedId: 'pro', loading: false }}
    />,
  );
  await user.click(screen.getByRole('button', { name: /DeepSeek V4 Pro/u }));
  expect(screen.queryByRole('group', { name: '提供商' })).toBeNull();
  const menu = screen.getByRole('menu', { name: '快速选择模型' });
  expect(within(menu).getAllByRole('menuitemradio')).toHaveLength(2);
  expect(within(menu).getByRole('menuitem', { name: /管理模型/u })).toBeTruthy();
});

// A model that did not switch says why, that the conversation keeps its model, and what to do.
test('a switch that failed says why and that the conversation keeps its model', async () => {
  const user = userEvent.setup();
  const onSelect = vi
    .fn<(id: string) => Promise<void>>()
    .mockRejectedValueOnce(
      Object.assign(new Error('Core request timed out'), { code: 'ipc.request_timeout' }),
    )
    .mockRejectedValueOnce(
      Object.assign(new Error('模型设置已在别处更新，请刷新后再试。'), {
        code: 'models.configuration.revision_conflict',
      }),
    );
  render(
    <ModelPicker
      onManageSettings={vi.fn()}
      onSelect={onSelect}
      state={{ choices: choices.slice(0, 2), selectedId: 'pro', loading: false }}
    />,
  );
  await user.click(screen.getByRole('button', { name: /DeepSeek V4 Pro/u }));
  await user.click(screen.getByRole('menuitemradio', { name: /DeepSeek V4 Flash/u }));
  expect(await screen.findByRole('alert')).toHaveProperty(
    'textContent',
    '模型没有切换（Aura Core 没有及时回复），这个会话仍使用原来的模型；请重新选择一次。',
  );
  await user.click(screen.getByRole('menuitemradio', { name: /DeepSeek V4 Flash/u }));
  expect(await screen.findByText(/刚刚在别处改过/u)).toHaveProperty(
    'textContent',
    '这个会话的模型刚刚在别处改过，这次没有切换；请看现在用的模型后再选。',
  );
});

test('opening a list that could not be read reads it again', async () => {
  const user = userEvent.setup();
  const retry = vi.fn();
  render(
    <ModelPicker
      onManageSettings={vi.fn()}
      onSelect={vi.fn(async () => {})}
      state={{ choices: [], loading: false, error: '模型列表没有读到。', retry }}
    />,
  );
  await user.click(screen.getByRole('button', { name: /选择模型/u }));
  expect(retry).toHaveBeenCalledOnce();
});
