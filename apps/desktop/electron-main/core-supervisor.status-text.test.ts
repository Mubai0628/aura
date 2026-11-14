// @vitest-environment node
import { spawn, type ChildProcess } from 'node:child_process';
import { expect, test, vi } from 'vitest';
import { CoreTransport } from './core-transport.js';
import { CoreExitRecovery } from './core-exit-recovery.js';
import {
  createFixture,
  delay,
  loggedFields,
  pipeClosed,
  waitUntil,
} from './core-supervisor-tests/fixture.js';

vi.mock('node:child_process', () => ({ spawn: vi.fn() }));
vi.mock('node:fs/promises', () => ({ mkdir: vi.fn(), appendFile: vi.fn() }));
vi.mock('./core-transport.js', () => ({ CoreTransport: { connect: vi.fn() } }));

// What the window says about Core is a Chinese sentence with the cause, what it means and what to
// do; the technical reason goes to the Main log.

const MISSING =
  '找不到 aura-core.exe（可能被安全软件隔离或安装不完整），Aura 现在不能使用；请到安全软件的隔离区恢复它，或重新运行安装程序修复，然后重启 Aura。';
const BLOCKED =
  'Windows 或安全软件阻止了 aura-core.exe 启动，Aura 现在不能使用；请在安全软件中允许它后重启 Aura。';

test('a Core that would not start says whether it is missing or was blocked', async () => {
  for (const [code, said] of [
    ['ENOENT', MISSING],
    ['EPERM', BLOCKED],
  ] as const) {
    const f = await createFixture();
    Object.assign(f.child, { pid: undefined });
    vi.mocked(spawn).mockImplementationOnce(() => {
      process.nextTick(() =>
        f.child.emit('error', Object.assign(new Error(`spawn aura-core.exe ${code}`), { code })),
      );
      return f.child as unknown as ChildProcess;
    });
    await f.supervisor.start();
    await waitUntil(() => f.supervisor.snapshot.payload.status_code === 'core.executable_missing');
    await delay(50);
    expect(f.supervisor.snapshot.payload.state).toBe('unavailable');
    expect(f.supervisor.snapshot.payload.status_message).toBe(said);
    f.supervisor.stop();
  }
});

test('a Core from another release says so in words and logs the check that failed', async () => {
  const f = await createFixture({ handshake: 'foreign-bundle' });
  await f.supervisor.start();
  expect(f.supervisor.snapshot.payload.state).toBe('incompatible');
  expect(f.supervisor.snapshot.payload.status_message).toBe(
    'Aura 界面与 Aura Core 不是同一个版本，Core 没有启动，所有功能暂不可用；请重新运行安装程序修复，或在启动器中回到上一版本。',
  );
  expect(String(loggedFields('core_incompatible')[0]?.reason)).toContain('Release Bundle');
  f.supervisor.stop();
});

test('a lost connection says Core still runs, and a restart says what happens to the tasks', async () => {
  const f = await createFixture({ startupTimeouts: { connectMs: 5_000, handshakeMs: 250 } });
  await f.supervisor.start();
  const connect = vi.mocked(CoreTransport.connect);
  const original = connect.getMockImplementation();
  connect.mockImplementation(async () => new Promise<never>(() => {}));
  try {
    f.callbacks().onClosed(pipeClosed);
    await waitUntil(
      () => f.supervisor.snapshot.payload.status_code === 'core.transport_reconnecting',
    );
    expect(f.supervisor.snapshot.payload.status_message).toBe(
      '与 Aura Core 的连接断了，正在重新连接（Core 仍在运行）；这期间的操作会失败，连上后再试即可。',
    );
    await waitUntil(
      () => f.supervisor.snapshot.payload.status_code === 'core.reconnect_budget_exhausted',
    );
    expect(f.supervisor.snapshot.payload.status_message).toBe(
      '与 Aura Core 的连接没能恢复，正在重启 Core；正在运行的任务会暂停，重启后 Aura 会自动接着做，没有接上的在会话里发“继续”即可。',
    );
  } finally {
    if (original !== undefined) connect.mockImplementation(original);
    f.supervisor.stop();
  }
});

test('a Core that kept crashing names the launcher in Chinese', () => {
  const recovery = new CoreExitRecovery();
  const plans = [0, 1, 2].map((at) => recovery.plan('ready', at));
  const escalated = plans[2];
  expect(escalated?.kind).toBe('escalate');
  expect(escalated?.kind === 'stopped' ? '' : escalated?.statusMessage).toBe(
    'Aura Core 反复意外退出，Aura 将关闭并交给启动器恢复；启动器会显示恢复窗口，可以重新启动、进入安全模式或回到上一版本。',
  );
});

// Before Core's handshake every interface reads as not offered; the window hears that Core is
// starting, with Main's code for it, not "does not support".
test('a request made while Core starts is refused as not ready, in words', async () => {
  const f = await createFixture();
  let refused: unknown;
  try {
    await f.supervisor.requestP03('workspace.files.snapshot' as never, {} as never);
  } catch (error) {
    refused = error;
  }
  const envelope = (refused as { envelope?: { code: string; message: string } }).envelope;
  expect(envelope?.code).toBe('ipc.core_not_ready');
  expect(envelope?.message).toBe('Aura Core 正在启动或重启，这次操作没有发出；就绪后再试。');
  f.supervisor.stop();
});
