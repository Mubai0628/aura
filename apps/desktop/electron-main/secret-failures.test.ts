// @vitest-environment node
import { expect, test, vi } from 'vitest';
import { OUTCOME_RECOVERY_NOT_SENT } from '../shared/ipc-request-failure.js';

vi.mock('electron', () => ({ ipcMain: { handle: vi.fn() } }));
vi.mock('../shared/agent-validation.js', () => ({ isAgentParams: () => true }));
vi.mock('../shared/tool-credential-validation.js', () => ({
  isToolCredentialParams: () => true,
  isToolCredentialSecretCommand: () => true,
}));

const { ipcMain } = await import('electron');
const { coreUnreachable } = await import('./core-unavailable.js');
const { modelKeyFailure } = await import('./secret-failures.js');
const { registerTerminalSecretIpc } = await import('./terminal-secret-ipc.js');
const { registerExtensionSecretIpc } = await import('./mcp-secret-ipc.js');
const { registerToolSecretIpc } = await import('./tool-secret-ipc.js');

type Answer = { ok: boolean; error?: { code: string; message: string; details: object } };
const handler = (channel: string) => {
  const found = vi.mocked(ipcMain.handle).mock.calls.find(([name]) => name === channel);
  if (found === undefined) throw new Error(`${channel} is not registered`);
  return found[1] as unknown as (
    event: unknown,
    first: unknown,
    source: unknown,
  ) => Promise<Answer>;
};
const typed = () => new Uint8Array([0x61, 0x62, 0x63]);

const NOT_SENT =
  'Aura Core 正在重启或暂时连不上，这次输入没有发出，什么都没有保存；等 Core 就绪后重新输入即可。';

// A secret Core never received is said to be not sent and safe to type again, never "unconfirmed".
test('a secret typed while Core restarts says it was not sent and nothing was saved', async () => {
  const supervisor = {
    requestAgentSecret: vi.fn().mockRejectedValue(coreUnreachable()),
    requestToolCredential: vi.fn().mockRejectedValue(coreUnreachable()),
  };
  registerTerminalSecretIpc(
    () => supervisor as never,
    () => undefined,
  );
  registerExtensionSecretIpc(
    () => supervisor as never,
    () => undefined,
  );
  registerToolSecretIpc(
    () => supervisor as never,
    () => undefined,
  );

  const terminal = await handler('aura:v1:terminal-secret')({}, {}, typed());
  expect(terminal.error?.message).toBe(
    'Aura Core 正在重启或暂时连不上，这次输入没有发出，终端没有收到任何内容；等 Core 就绪后重新输入即可。',
  );
  expect(terminal.error?.details).toEqual({ outcome_recovery: OUTCOME_RECOVERY_NOT_SENT });
  for (const channel of ['aura:v1:mcp-secret', 'aura:v1:plugin-secret']) {
    const answer = await handler(channel)({}, {}, typed());
    expect(answer.error?.message).toBe(NOT_SENT);
    expect(answer.error?.details).toEqual({ outcome_recovery: OUTCOME_RECOVERY_NOT_SENT });
  }
  const tool = await handler('aura:v1:tool-secret')(
    {},
    { command: 'tools.credential.create', params: {} },
    typed(),
  );
  expect(tool.error?.message).toBe(NOT_SENT);
});

test('a secret typed before Core is ready says it was not sent', async () => {
  vi.mocked(ipcMain.handle).mockClear();
  registerTerminalSecretIpc(
    () => undefined,
    () => undefined,
  );
  registerExtensionSecretIpc(
    () => undefined,
    () => undefined,
  );
  for (const channel of ['aura:v1:terminal-secret', 'aura:v1:mcp-secret']) {
    const answer = await handler(channel)({}, {}, typed());
    expect(answer.error?.message).toBe('Aura Core 尚未就绪，这次输入没有发出；请等它就绪后再试。');
  }
});

test('a model key that did not reach Core, or whose outcome is not known, says which', () => {
  expect(modelKeyFailure(coreUnreachable())).toMatchObject({
    message: 'Aura Core 正在重启或暂时连不上，密钥没有发出，也没有保存；等 Core 就绪后再保存一次。',
    details: { outcome_recovery: OUTCOME_RECOVERY_NOT_SENT },
  });
  expect(modelKeyFailure(new Error('unexpected'))).toMatchObject({
    message: '密钥是否已保存没有确认（Aura 内部出错）；请刷新后看是否已保存，再决定是否重新保存。',
  });
});
