import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test, vi } from 'vitest';
import { ReasoningPicker } from './ReasoningPicker';

// Beside the model, its thinking level —
// only the levels that model offers, and the provider's own default first.

afterEach(cleanup);

test('a model offering no level shows no choice', () => {
  render(<ReasoningPicker levels={[]} level={undefined} onSelect={vi.fn(async () => {})} />);
  expect(screen.queryByRole('button')).toBeNull();
});

test('the default first, then the levels the model offers; a choice is saved', async () => {
  const user = userEvent.setup();
  const onSelect = vi.fn(async () => {});
  render(
    <ReasoningPicker levels={['low', 'medium', 'high']} level={undefined} onSelect={onSelect} />,
  );
  await user.click(screen.getByRole('button', { name: '思考程度：默认' }));
  const menu = screen.getByRole('menu', { name: '思考程度' });
  const items = within(menu).getAllByRole('menuitemradio');
  expect(items.map((item) => item.textContent)).toEqual(['默认', '低', '中', '高']);
  expect(items[0]!.getAttribute('aria-checked')).toBe('true');
  await user.click(within(menu).getByRole('menuitemradio', { name: '高' }));
  expect(onSelect).toHaveBeenCalledWith('high');
});

test('the level in use is checked; going back to the default saves none', async () => {
  const user = userEvent.setup();
  const onSelect = vi.fn(async () => {});
  render(<ReasoningPicker levels={['off', 'low', 'high']} level="off" onSelect={onSelect} />);
  await user.click(screen.getByRole('button', { name: '思考程度：关闭' }));
  const menu = screen.getByRole('menu', { name: '思考程度' });
  expect(
    within(menu).getByRole('menuitemradio', { name: '关闭' }).getAttribute('aria-checked'),
  ).toBe('true');
  await user.click(within(menu).getByRole('menuitemradio', { name: '默认' }));
  expect(onSelect).toHaveBeenCalledWith(null);
});

test('a failed save says so and keeps the menu open', async () => {
  const user = userEvent.setup();
  const onSelect = vi.fn(async () => {
    throw new Error('models.revision_conflict: 模型配置已变化，请重试。');
  });
  render(<ReasoningPicker levels={['low', 'high']} level={undefined} onSelect={onSelect} />);
  await user.click(screen.getByRole('button', { name: '思考程度：默认' }));
  await user.click(screen.getByRole('menuitemradio', { name: '高' }));
  expect(await screen.findByRole('alert')).toBeTruthy();
  expect(screen.getByRole('menu', { name: '思考程度' })).toBeTruthy();
});
