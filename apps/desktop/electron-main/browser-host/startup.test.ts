// @vitest-environment node
import { expect, test, vi } from 'vitest';
import { browserCommandNotification } from './core-channel.js';
import { command } from './executor-fakes.js';
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

const settings = () => ({
  revision: 1,
  issue: null,
  settings: { enabled: true, allowed_sites: [], blocked_sites: [], debugging: false },
});

function fixture() {
  const core = {
    requestAgent: vi.fn(async () => settings()),
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
  const run = (id = 'bcm-1') => {
    const open = { ...command({ kind: 'open', url: 'https://example.com/' }), command_id: id };
    expect(
      browserCommandNotification(core, 'browser.command.v1', open, open.runtime_instance_id),
    ).toBe(true);
  };
  return { core, host, run };
}

test('the first model open loads settings without any user page having been opened', async () => {
  const { core, host, run } = fixture();
  run();
  await vi.waitFor(() => expect(core.requestWithBudget).toHaveBeenCalled());
  expect(core.requestAgent).toHaveBeenCalledWith('browser.settings.get', {});
  expect(core.requestWithBudget.mock.calls[0]?.[1]).toMatchObject({
    outcome: { outcome: 'completed' },
  });
  expect(host.pages.contents(command({ kind: 'close' }).session_id, 'model')?.getURL()).toBe(
    'https://example.com/',
  );
});

test('startup explicitly loads rules, while an unavailable Core keeps the browser closed', async () => {
  const { core, host } = fixture();
  await host.refreshSettings();
  expect(host.pages.policy.enabled).toBe(true);
  core.requestAgent.mockRejectedValueOnce(new Error('not ready'));
  await host.refreshSettings();
  expect(host.pages.policy.enabled).toBe(false);
});

test('a model command withdrawn during settings refresh never starts', async () => {
  const { core, host, run } = fixture();
  let finish!: (value: ReturnType<typeof settings>) => void;
  core.requestAgent.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  run();
  await vi.waitFor(() => expect(core.requestAgent).toHaveBeenCalled());
  const cancel = command({ kind: 'close' });
  browserCommandNotification(
    core,
    'browser.cancel.v1',
    {
      command_id: 'bcm-1',
      runtime_instance_id: cancel.runtime_instance_id,
    },
    cancel.runtime_instance_id,
  );
  finish(settings());
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(core.requestWithBudget).not.toHaveBeenCalled();
  expect(host.pages.states()).toEqual([]);
});

test('a late settings read cannot overwrite a newer save and concurrent readers share it', async () => {
  const { core, host } = fixture();
  let finish!: (value: ReturnType<typeof settings>) => void;
  core.requestAgent.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const first = host.refreshSettings();
  const second = host.refreshSettings();
  const latest = settings();
  latest.revision = 2;
  latest.settings.enabled = false;
  host.applySettings(latest);
  finish(settings());
  await Promise.all([first, second]);
  expect(host.pages.policy.enabled).toBe(false);
  expect(core.requestAgent).toHaveBeenCalledTimes(1);
});
