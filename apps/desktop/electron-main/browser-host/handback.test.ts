// @vitest-environment node
import { ipcMain } from 'electron';
import { expect, test, vi } from 'vitest';
import { BROWSER_COMMAND_CHANNEL, MODEL_PAGE_ID } from '../../shared/browser-api.js';
import { browserCommandNotification } from './core-channel.js';
import { command, S } from './executor-fakes.js';
import { installBrowserHost } from './index.js';

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

/**
 * The
 * user who took the model's page over answers its waiting request by handing the page back.
 */
test("handing the model's page back answers its request that waits for the user", async () => {
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
  const user = (kind: string) => handler({}, { kind, sessionId: S, pageId: MODEL_PAGE_ID });
  const fromCore = (id: string, action: Parameters<typeof command>[0]) => {
    const sent = { ...command(action), command_id: id };
    browserCommandNotification(core, 'browser.command.v1', sent, sent.runtime_instance_id);
  };
  const results = () => core.requestWithBudget.mock.calls.map(([, result]) => result);

  fromCore('bcm-1', { kind: 'open', url: 'https://example.com/' });
  await vi.waitFor(() => expect(results()).toHaveLength(1));
  expect(await user('take-control')).toEqual({ ok: true });
  fromCore('bcm-2', { kind: 'request_user', reason: '请操作完后点完成', timeout_ms: 60_000 });
  await vi.waitFor(() => expect(host.pages.state(S, MODEL_PAGE_ID)?.assist).toBeDefined());

  expect(await user('release-control')).toEqual({ ok: true });
  await vi.waitFor(() => expect(results()).toHaveLength(2));
  expect(results()[1]).toMatchObject({
    command_id: 'bcm-2',
    outcome: { outcome: 'completed', result: { outcome: 'done' } },
  });
  expect(host.pages.state(S, MODEL_PAGE_ID)).toMatchObject({ userInControl: false });
  expect(host.pages.state(S, MODEL_PAGE_ID)?.assist).toBeUndefined();
});
