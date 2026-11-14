// @vitest-environment node
import { expect, test, vi } from 'vitest';
import type { IpcErrorEnvelope } from '@aura/contracts/generated';
import { readFileSync } from 'node:fs';
import { registerTerminalOpeningIpc } from './terminal-opening-ipc.js';
import { terminalOpeningBridge } from '../preload/terminal-opening.js';
import { gitBridge } from '../preload/git.js';
import { packageBridge } from '../preload/package.js';
import { TERMINAL_OPENING_START_CHANNEL } from '../shared/terminal-opening-api.js';

const ipc = vi.hoisted(() => ({
  handlers: new Map<string, (...args: unknown[]) => Promise<unknown>>(),
  renderer: { invoke: vi.fn(), on: vi.fn(), removeListener: vi.fn() },
}));
vi.mock('electron', () => ({
  ipcMain: {
    handle: (name: string, fn: (...args: unknown[]) => Promise<unknown>) =>
      ipc.handlers.set(name, fn),
  },
  ipcRenderer: ipc.renderer,
}));

const fixture = JSON.parse(
  readFileSync(
    new URL('../../../fixtures/contracts/terminal-opening.v1.json', import.meta.url),
    'utf8',
  ),
);
const error = (code: string, message: string) =>
  ({ code, message, retryable: false }) as IpcErrorEnvelope;
const isError = (value: unknown): value is IpcErrorEnvelope =>
  typeof value === 'object' && value !== null && 'code' in value && 'message' in value;

// A terminal asked for while Core starts says so and what to do.
test('a terminal opened while Core starts or restarts says it did not open and why', async () => {
  registerTerminalOpeningIpc(
    () => undefined,
    () => undefined,
  );
  const begin = ipc.handlers.get(TERMINAL_OPENING_START_CHANNEL)!;
  const owner = { isDestroyed: () => false, once: vi.fn(), send: vi.fn() };
  expect(await begin({ sender: owner }, fixture.start)).toMatchObject({
    ok: false,
    error: { message: 'Aura Core 正在启动或重启，终端没有打开；等 Core 就绪后再点一次。' },
  });
});

const MISMATCH = (what: string) =>
  `Aura 界面和 Core 的版本对不上，${what}的结果读不懂；请重启 Aura，仍然如此请重新运行安装程序修复。`;

// An answer this window cannot read names the version mismatch and what to do.
test('an answer the window cannot read says the versions do not match', async () => {
  const git = gitBridge(
    vi.fn().mockResolvedValue({ ok: true, value: { opened: 'bad' } }),
    error,
    isError,
  );
  expect(await git.openInstallation()).toMatchObject({
    error: { code: 'git.invalid_response', message: MISMATCH('这次 Git 操作') },
  });
  const packages = packageBridge(
    vi.fn().mockResolvedValue({ ok: true, value: { unexpected: true } }) as never,
    error,
    isError,
  );
  expect(await packages.invoke('packages.list', {})).toMatchObject({
    error: { code: 'package.invalid_response', message: MISMATCH('这次工具包操作') },
  });
  const opening = terminalOpeningBridge(isError, error);
  ipc.renderer.invoke.mockResolvedValue({ ok: true, value: 'not-a-cancel-result' });
  expect(await opening.cancelOpen(fixture.reference)).toMatchObject({
    error: { message: MISMATCH('终端打开') },
  });
});
