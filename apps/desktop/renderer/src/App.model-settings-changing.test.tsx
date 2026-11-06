import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, test, vi } from 'vitest';
import { App } from './App';
import { installAppTestEnvironment, modelSettingsProjection } from './app-test-fixtures';

installAppTestEnvironment();

/** Core's answer once its own retries found the settings still being written. */
const factsChanged = {
  error: {
    category: 'conflict',
    code: 'broker.facts_changed',
    correlation_id: 'cor_01ARZ3NDEKTSV4RRFFQ69G5FAV',
    details: {},
    message: '相关内容正在被其他操作修改，这次请求没有执行，请稍后再试。',
    retryable: true,
    terminal: 'failed',
  },
  ok: false,
} as const;

describe('model settings while another change to them is being written', () => {
  test('a read says so instead of a reconnect, and the settings are read again', async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(screen.getByRole('button', { name: '设置' }));
    expect(await screen.findByRole('heading', { level: 2, name: '模型配置' })).toBeTruthy();
    await user.click(screen.getByRole('button', { name: '返回会话' }));

    vi.mocked(window.aura.v1.getModelSettings).mockResolvedValue(factsChanged);
    await user.click(screen.getByRole('button', { name: '设置' }));

    expect(await screen.findAllByText('模型设置正在被修改，请稍后再试。')).not.toHaveLength(0);
    expect(screen.queryByText(/正在重新连接/u)).toBeNull();
    // The other change is written; nobody refreshes, the page reads the settings again itself.
    vi.mocked(window.aura.v1.getModelSettings).mockResolvedValue({
      ok: true,
      value: modelSettingsProjection,
    });
    await waitFor(
      () => expect(screen.queryAllByText('模型设置正在被修改，请稍后再试。')).toHaveLength(0),
      { timeout: 3_000 },
    );
  });

  test('a write says so in its failure notice', async () => {
    vi.mocked(window.aura.v1.deleteModelEndpoint).mockResolvedValue(factsChanged);
    const user = userEvent.setup();
    render(<App />);

    await user.click(screen.getByRole('button', { name: '设置' }));
    await user.click(await screen.findByRole('button', { name: '配置 OpenAI' }));
    const providerDialog = screen.getByRole('dialog', { name: 'OpenAI' });
    await user.click(within(providerDialog).getByRole('button', { name: /OpenAI 主连接/u }));
    await user.click(within(providerDialog).getByRole('button', { name: '删除 Endpoint' }));
    const confirmation = screen.getByRole('dialog', { name: '确认删除配置' });
    await user.click(within(confirmation).getByRole('button', { name: '删除 Endpoint' }));

    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain('Endpoint 未删除');
    expect(alert.textContent).toContain('模型设置正在被修改，请稍后再试。');
  });
});
