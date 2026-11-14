// @vitest-environment node
import { ipcMain } from 'electron';
import { expect, test, vi } from 'vitest';
import { BROWSER_COMMAND_CHANNEL } from '../../shared/browser-api.js';
import { browserCommandNotification } from './core-channel.js';
import { command, S } from './executor-fakes.js';
import { installBrowserHost } from './index.js';
import { readCommand } from './protocol.js';

vi.mock('electron', async () => {
  const { Contents, Tools } = await import('./executor-fakes.js');
  return {
    ipcMain: { handle: vi.fn() },
    session: { fromPartition: vi.fn() },
    WebContentsView: class {
      webContents = Object.assign(new Contents(), { debugger: new Tools() });
      setBackgroundColor() {}
      setBounds() {}
      setVisible() {}
    },
  };
});
vi.mock('./profile.js', () => ({ BROWSER_PARTITION: 'test-browser', guardProfile: vi.fn() }));

test("the user's page goes to the model with hand-over, the model's commands reach it by its id, and handing it back answers a wait on it", async () => {
  const core = {
    requestAgent: vi.fn(async () => ({
      revision: 1,
      issue: null,
      settings: { enabled: true, allowed_sites: [], blocked_sites: [], debugging: false },
    })),
    requestWithBudget: vi.fn<
      (method: string, params: unknown, timeoutMilliseconds: number) => Promise<unknown>
    >(async () => ({ accepted: true })),
    refreshWorkspaceSessionSnapshot: vi.fn(),
  };
  const host = installBrowserHost({
    core: () => core as never,
    window: () => undefined,
    assertTrustedSender() {},
    log() {},
  });
  host.bindCore(core);
  const handler = vi
    .mocked(ipcMain.handle)
    .mock.calls.filter(([channel]) => channel === BROWSER_COMMAND_CHANNEL)
    .at(-1)?.[1] as (event: unknown, value: unknown) => Promise<{ ok: boolean }>;
  const user = (kind: string, pageId: string) => handler({}, { kind, sessionId: S, pageId });
  const fromCore = (id: string, action: Parameters<typeof command>[0]) => {
    const sent = { ...command(action), command_id: id, page: 'p1' };
    browserCommandNotification(core, 'browser.command.v1', sent, sent.runtime_instance_id);
  };
  const results = () => core.requestWithBudget.mock.calls.map(([, result]) => result);

  expect(await user('open', 'page-a')).toMatchObject({ ok: true });
  expect(
    await handler({}, { kind: 'navigate', sessionId: S, pageId: 'page-a', input: '127.0.0.1:9/a' }),
  ).toEqual({ ok: true });
  expect(await user('hand-over', 'page-a')).toMatchObject({
    ok: true,
    state: { pageId: 'page-a', handedAs: 'p1' },
  });
  expect(await user('hand-over', 'model')).toMatchObject({ ok: false });

  fromCore('bcm-1', { kind: 'snapshot', max_chars: 12_000 });
  await vi.waitFor(() => expect(results()).toHaveLength(1));
  expect(results()[0]).toMatchObject({
    command_id: 'bcm-1',
    page_url: 'http://127.0.0.1:9/a',
    outcome: { outcome: 'completed' },
  });
  expect(host.pages.state(S, 'model')).toBeUndefined();

  expect(await user('take-control', 'page-a')).toEqual({ ok: true });
  fromCore('bcm-2', { kind: 'request_user', reason: '请操作完后点完成', timeout_ms: 60_000 });
  await vi.waitFor(() => expect(host.pages.state(S, 'page-a')?.assist).toBeDefined());
  expect(await user('release-control', 'page-a')).toEqual({ ok: true });
  await vi.waitFor(() => expect(results()).toHaveLength(2));
  expect(results()[1]).toMatchObject({
    command_id: 'bcm-2',
    outcome: { outcome: 'completed', result: { outcome: 'done' } },
  });
  expect(host.pages.state(S, 'page-a')).toMatchObject({ userInControl: false, handedAs: 'p1' });
});

test("Core's command names a handed page by its id, or none for the model's own page", () => {
  const base = command({ kind: 'snapshot', max_chars: 12_000 });
  const runtime = base.runtime_instance_id;
  expect(readCommand({ ...base, page: 'p1' }, runtime)).toMatchObject({ page: 'p1' });
  expect(readCommand({ ...base, page: 'p12' }, runtime)).toBeDefined();
  expect(readCommand({ ...base, page: null }, runtime)).toBeDefined();
  expect(readCommand(base, runtime)).toBeDefined();
  for (const page of ['page-a', 'p0', 'model', 'p12345', 7, ''])
    expect(readCommand({ ...base, page }, runtime)).toBeUndefined();
});
