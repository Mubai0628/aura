import { act, render, screen } from '@testing-library/react';
import { expect, test, vi } from 'vitest';
import type { CoreHealthProjectionChange } from '../../shared/aura-api';
import { App } from './App';
import { installAppTestEnvironment, readyProjection } from './app-test-fixtures';

installAppTestEnvironment();

// While long work holds Core's stores, the status says Core is
// busy and with what — not "已就绪" — and that other actions wait for it; then it is ready again.
test('the status says Core is busy, and with what, while long work holds its stores', async () => {
  let push: ((change: CoreHealthProjectionChange) => void) | undefined;
  Object.assign(window.aura.v1, {
    onCoreHealth: vi.fn((listener: (change: CoreHealthProjectionChange) => void) => {
      push = listener;
      return () => undefined;
    }),
  });
  render(<App />);
  expect(await screen.findByText('Aura Core 已就绪')).toBeTruthy();
  act(() =>
    push?.({
      ...readyProjection,
      revision: '3',
      payload: {
        ...readyProjection.payload,
        busy: { activity: 'deleting_session', since: '2026-10-19T08:00:00Z' },
      },
    }),
  );
  const status = await screen.findByText('Core 正忙：正在删除会话');
  expect(status.closest('p')?.getAttribute('title')).toContain('其他操作会排队');
  expect(screen.queryByText('Aura Core 已就绪')).toBeNull();
  act(() => push?.({ ...readyProjection, revision: '4' }));
  expect(await screen.findByText('Aura Core 已就绪')).toBeTruthy();
});

// The full check runs while Core serves; damage it finds stays in
// the status, with what to do, instead of "已就绪".
test('the status says when the background check found the store damaged', async () => {
  let push: ((change: CoreHealthProjectionChange) => void) | undefined;
  Object.assign(window.aura.v1, {
    onCoreHealth: vi.fn((listener: (change: CoreHealthProjectionChange) => void) => {
      push = listener;
      return () => undefined;
    }),
  });
  render(<App />);
  expect(await screen.findByText('Aura Core 已就绪')).toBeTruthy();
  act(() =>
    push?.({
      ...readyProjection,
      revision: '3',
      payload: {
        ...readyProjection.payload,
        status_code: 'core.store_check_failed',
        status_message: 'Aura 的数据库检查发现了问题；建议先备份 Aura 的数据文件夹。',
      },
    }),
  );
  const status = await screen.findByText('数据库检查发现问题');
  expect(status.closest('p')?.getAttribute('title')).toContain('备份');
  expect(screen.queryByText('Aura Core 已就绪')).toBeNull();
});
