import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { expect, test, vi } from 'vitest';
import type { CoreHealthProjectionChange } from '../../shared/aura-api';
import { App } from './App';
import { installAppTestEnvironment, readyProjection } from './app-test-fixtures';

installAppTestEnvironment();

function healthPushes() {
  let push: ((change: CoreHealthProjectionChange) => void) | undefined;
  Object.assign(window.aura.v1, {
    onCoreHealth: vi.fn((listener: (change: CoreHealthProjectionChange) => void) => {
      push = listener;
      return () => undefined;
    }),
  });
  return (change: CoreHealthProjectionChange) => act(() => push?.(change));
}

/** The App once the window has seen Core ready. */
async function readyApp() {
  const { container } = render(<App />);
  await waitFor(() =>
    expect(container.querySelector('.app-shell')?.getAttribute('data-core-state')).toBe('ready'),
  );
  return container;
}

/** Core's status in the settings page's header. */
async function settingsStatus() {
  await userEvent.click(screen.getByRole('button', { name: '设置' }));
  const status = document.querySelector<HTMLElement>('.settings-core-status');
  if (status === null) throw new Error("the settings page's Core status");
  return status;
}

const damaged = (revision: string): CoreHealthProjectionChange => ({
  ...readyProjection,
  revision,
  payload: {
    ...readyProjection.payload,
    status_code: 'core.store_check_failed',
    status_message: 'Aura 的数据库检查发现了问题；建议先备份 Aura 的数据文件夹。',
  },
});

// While Core deletes records in the background, its status says so — not "已就绪", and not that
// Core is busy: other actions do not wait for it — then it is ready again.
test('the status says what Core is deleting in the background', async () => {
  const push = healthPushes();
  await readyApp();
  const status = await settingsStatus();
  expect(status.textContent).toBe('Aura Core 已就绪');
  push({
    ...readyProjection,
    revision: '3',
    payload: {
      ...readyProjection.payload,
      busy: { activity: 'deleting_session', since: '2026-10-19T08:00:00Z' },
    },
  });
  await waitFor(() => expect(status.textContent).toBe('正在后台删除会话记录'));
  expect(status.title).not.toContain('排队');
  expect(screen.queryByText(/Core 正忙/u)).toBeNull();
  push({ ...readyProjection, revision: '4' });
  await waitFor(() => expect(status.textContent).toBe('Aura Core 已就绪'));
});

// The full check runs while Core serves; damage it finds is said in a notice, with what to do,
// that stays until it is closed or the check passes again.
test('a notice says when the background check found the store damaged', async () => {
  const push = healthPushes();
  await readyApp();
  push(damaged('3'));
  const notice = await within(screen.getByLabelText('系统消息')).findByRole('alert');
  expect(within(notice).getByText('数据库检查发现问题')).toBeTruthy();
  expect(notice.textContent).toContain('备份');
  push({ ...readyProjection, revision: '4' });
  await waitFor(() =>
    expect(within(screen.getByLabelText('系统消息')).queryByRole('alert')).toBeNull(),
  );
});

// A damaged store is said even while a deletion runs in the background.
test('a damaged store is said before a background deletion', async () => {
  const push = healthPushes();
  await readyApp();
  const status = await settingsStatus();
  push({
    ...damaged('3'),
    payload: {
      ...damaged('3').payload,
      busy: { activity: 'removing_workspace', since: '2026-10-19T08:00:00Z' },
    },
  });
  await waitFor(() => expect(status.textContent).toBe('数据库检查发现问题'));
  expect(status.title).toContain('备份');
  expect(screen.queryByText('正在后台删除工作区记录')).toBeNull();
});
