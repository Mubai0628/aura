import { expect, test } from '@playwright/test';
import { mkdtemp, mkdir, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { armReadOnlyWrite } from './s06/setup';
import { launch, pending, finished, submit, attemptCount } from './s06/harness';
import { networkProvider } from './s07/provider';
import type { ProviderRequest } from './s06-file-write-provider.mjs';

function result(request: ProviderRequest): {
  outcome: {
    type: string;
    payload?: { value: Record<string, unknown> };
    error?: { message: string };
  };
} {
  const tool = request.messages
    .filter((value) => (value as { role?: string }).role === 'tool')
    .at(-1) as { content: string } | undefined;
  if (!tool) throw new Error('missing durable tool result');
  return JSON.parse(tool.content) as ReturnType<typeof result>;
}
// eslint-disable-next-line no-empty-pattern -- isolated Electron, Core and loopback scripted provider.
test('Agent terminals share Shell approval, pause model reads/writes on takeover and survive as ended records', async ({}, info) => {
  test.setTimeout(240_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l02-agent-'));
  const work = join(root, 'workspace');
  await mkdir(work);
  const results: ReturnType<typeof result>[] = [];
  const provider = await networkProvider((request) => results.push(result(request)));
  provider.setAction('terminal.open', { shell: 'cmd', title: 'L02 Agent CMD' });
  let current = await launch(root, { AURA_TERMINAL_TEST_NO_PROFILE: '1' });
  try {
    const setup = await armReadOnlyWrite(current.page, work, provider.url, {
      title: 'L02 Agent',
      text: '打开本地 Agent 终端',
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    const approval = await pending(current.page, session);
    expect(approval.requested_capabilities).toEqual(['process_execute']);
    expect(await attemptCount(root, approval.operation.operation_id)).toBe(0);
    await current.page
      .locator('section[aria-label="当前任务审批"]')
      .getByRole('button', { name: '本会话允许', exact: true })
      .click();
    await finished(current.page, session);
    const opened = results.at(-1)!;
    expect(opened.outcome.type).toBe('succeeded');
    const terminal = opened.outcome.payload!.value.terminal_id as string;
    await current.page.locator('.session-row').first().click();
    // The model's terminal becomes a tab when asked for — here from the call that opened
    // it, in the finished turn's closed process section (the finished turn opens first).
    await current.page.getByTitle('展开过程').first().click();
    await current.page.locator('.agent-process-toggle[aria-expanded="false"]').first().click();
    await current.page.getByRole('button', { name: '查看终端', exact: true }).click();
    await expect(current.page.getByRole('tab', { name: /L02 Agent CMD/u })).toHaveAttribute(
      'aria-selected',
      'true',
    );
    await expect(current.page.locator('.terminal-status')).toContainText('控制：Agent');
    await current.page.getByRole('button', { name: '接管', exact: true }).click();
    await expect(current.page.locator('.terminal-status')).toContainText('控制：用户');
    for (const tool of ['terminal.write', 'terminal.read']) {
      provider.setAction(
        tool,
        tool.endsWith('write')
          ? { terminal_id: terminal, input: 'echo forbidden>forbidden.txt\n', wait_ms: 0 }
          : { terminal_id: terminal },
      );
      await submit(current.page, session, `接管后尝试 ${tool}`);
      await finished(current.page, session);
      expect(results.at(-1)!.outcome.type).toBe('failed');
      expect(results.at(-1)!.outcome.error!.message).toContain('用户已接管');
    }
    await expect
      .poll(async () => {
        try {
          await readFile(join(work, 'forbidden.txt'));
          return false;
        } catch {
          return true;
        }
      })
      .toBe(true);
    await current.page.locator('.terminal-host').click();
    await current.page.keyboard.type('echo user-owner>user-owner.txt');
    await current.page.keyboard.press('Enter');
    await expect
      .poll(async () => {
        try {
          return await readFile(join(work, 'user-owner.txt'), 'utf8');
        } catch {
          return '';
        }
      })
      .toContain('user-owner');
    await current.page.getByRole('button', { name: '交还', exact: true }).click();
    provider.setAction('terminal.write', {
      terminal_id: terminal,
      input: 'echo L02-RESUMED & echo resumed>agent-resumed.txt\n',
      wait_ms: 2000,
    });
    await submit(current.page, session, '交还后继续');
    await finished(current.page, session);
    expect(results.at(-1)!.outcome.type).toBe('succeeded');
    expect(results.at(-1)!.outcome.payload!.value.output).toContain('L02-RESUMED');
    expect(await readFile(join(work, 'agent-resumed.txt'), 'utf8')).toContain('resumed');
    provider.setAction('terminal.read', { terminal_id: terminal, max_bytes: 65536 });
    await submit(current.page, session, '读取记录');
    await finished(current.page, session);
    expect(results.at(-1)!.outcome.payload!.value.output).toContain('L02-RESUMED');
    expect(String(results.at(-1)!.outcome.payload!.value.output)).not.toContain('\x1b');
    provider.setAction('shell.cmd', { script: 'echo shared>shell-shared.txt' });
    await submit(current.page, session, 'Shell 复用同一批准');
    await finished(current.page, session);
    expect(await readFile(join(work, 'shell-shared.txt'), 'utf8')).toContain('shared');
    await current.page.screenshot({ path: info.outputPath('agent-terminal.png') });
    await info.attach('agent-terminal', {
      path: info.outputPath('agent-terminal.png'),
      contentType: 'image/png',
    });
    await current.app.close();
    current = await launch(root, { AURA_TERMINAL_TEST_NO_PROFILE: '1' });
    const records = await current.page.evaluate(async (session_id) => {
      const value = await window.aura.v1.terminal!.list({ session_id });
      if (!value.ok) throw new Error(value.error.message);
      return value.value.terminals;
    }, session);
    expect(records.find((record) => record.terminal_id === terminal)?.end?.reason).toBe(
      'core_stopped',
    );
    provider.setAction('terminal.write', {
      terminal_id: terminal,
      input: 'echo replay>replay.txt\n',
    });
    await submit(current.page, session, 'Core 重启后旧终端不可重开');
    await finished(current.page, session);
    expect(results.at(-1)!.outcome.type).toBe('failed');
    expect(results.at(-1)!.outcome.error!.message).toContain('已结束');
    expect(provider.errors).toEqual([]);
  } finally {
    await current.app.close();
    await provider.close();
  }
});
