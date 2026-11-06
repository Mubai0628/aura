import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, test, vi } from 'vitest';
import { App } from './App';
import { installAppTestEnvironment, modelProviderCatalog } from './app-test-fixtures';

installAppTestEnvironment();

/** The 模型配置 title row, with the catalog's refresh answered as `refresh` answers. */
async function openModels(refresh: ReturnType<typeof vi.fn>) {
  Object.assign(window.aura.v1, { refreshModelProviderCatalog: refresh });
  const user = userEvent.setup();
  render(<App />);
  await user.click(screen.getByRole('button', { name: '设置' }));
  const heading = await screen.findByRole('heading', { name: '模型配置' });
  const row = heading.closest<HTMLElement>('.settings-content-heading');
  if (row === null) throw new Error('the title row');
  return { row, user };
}

describe('refreshing the models.dev catalog', () => {
  test('the refresh button sits in the 模型配置 title row and refreshes the catalog', async () => {
    const refresh = vi.fn().mockResolvedValue({ ok: true, value: modelProviderCatalog });
    const { row, user } = await openModels(refresh);
    await user.click(within(row).getByRole('button', { name: '刷新 models.dev 目录' }));
    expect(refresh).toHaveBeenCalledTimes(1);
    expect(screen.getAllByRole('button', { name: '刷新 models.dev 目录' })).toHaveLength(1);
    expect(screen.queryByText(/models\.dev 目录 ·/u)).toBeNull();
    expect(screen.queryByText('models.dev 目录未刷新')).toBeNull();
  });

  test('a refresh that fails says so', async () => {
    const refresh = vi.fn().mockResolvedValue({
      ok: false,
      error: { code: 'catalog.fetch_failed', message: '无法连接 models.dev。' },
    });
    const { row, user } = await openModels(refresh);
    await user.click(within(row).getByRole('button', { name: '刷新 models.dev 目录' }));
    expect(await screen.findByText('models.dev 目录未刷新')).toBeTruthy();
  });
});
