import { act, render, screen } from '@testing-library/react';
import { expect, test, vi } from 'vitest';
import type { CoreHealthProjectionChange } from '../../shared/aura-api';
import { App } from './App';
import { installAppTestEnvironment, readyProjection } from './app-test-fixtures';

installAppTestEnvironment();

// While Core deletes records in the background, the status says so — not "已就绪", and not that
// Core is busy: other actions do not wait for it — then it is ready again.
test('the status says what Core is deleting in the background', async () => {
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
  const status = await screen.findByText('正在后台删除会话记录');
  expect(status.closest('p')?.getAttribute('title') ?? '').not.toContain('排队');
  expect(screen.queryByText(/Core 正忙/u)).toBeNull();
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

// A damaged store is said even while a deletion runs in the background.
test('a damaged store is said before a background deletion', async () => {
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
        busy: { activity: 'removing_workspace', since: '2026-10-19T08:00:00Z' },
      },
    }),
  );
  expect(await screen.findByText('数据库检查发现问题')).toBeTruthy();
  expect(screen.queryByText('正在后台删除工作区记录')).toBeNull();
});
