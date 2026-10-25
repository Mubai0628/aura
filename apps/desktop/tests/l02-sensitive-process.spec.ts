import { expect, test } from '@playwright/test';
import { mkdtemp, mkdir, readFile, readdir, stat, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { armReadOnlyWrite } from './s06/setup';
import { launch, pending, finished, submit } from './s06/harness';
import { viewModelTerminal } from './l10-8/terminal-tabs';
import { networkProvider } from './s07/provider';
import type { ProviderRequest } from './s06-file-write-provider.mjs';
function result(request: ProviderRequest) {
  const last = request.messages
    .filter((value) => (value as { role?: string }).role === 'tool')
    .at(-1) as { content: string };
  return JSON.parse(last.content) as {
    outcome: { type: string; payload?: { value: Record<string, unknown> } };
  };
}
// eslint-disable-next-line no-empty-pattern -- real Electron/Core/ConPTY and a loopback provider.
test('secure PS input stays out of durable data and model requests; process page stops Shell and Agent work', async ({}, info) => {
  test.setTimeout(240_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l02-sensitive-'));
  const work = join(root, 'workspace');
  await mkdir(work);
  await writeFile(
    join(work, 'sensitive.ps1'),
    "[Console]::WriteLine('L02_INPUT_READY'); $s=Read-Host -AsSecureString -Prompt '测试密码'; [Console]::WriteLine(('L02_LENGTH:' + $s.Length)); $s.Dispose()",
    'utf8',
  );
  const testValue = 'l02-temporary-' + Date.now().toString(36);
  const results: ReturnType<typeof result>[] = [];
  const provider = await networkProvider((request) => results.push(result(request)));
  provider.setAction('terminal.open', { title: 'B4 PowerShell' });
  const current = await launch(root, {
    AURA_TERMINAL_TEST_NO_PROFILE: '1',
    AURA_IPC_TRACE_FILE: join(root, 'ipc-trace.log'),
  });
  let closed = false;
  try {
    const setup = await armReadOnlyWrite(current.page, work, provider.url, {
      title: 'L02 B4',
      text: '打开交互终端',
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    await pending(current.page, session);
    await current.page
      .locator('section[aria-label="当前任务审批"]')
      .getByRole('button', { name: '本会话允许', exact: true })
      .click();
    await finished(current.page, session);
    const terminal = results.at(-1)!.outcome.payload!.value.terminal_id as string;
    await current.page.locator('.session-row').first().click();
    // The model's terminal becomes a tab when asked for — here from 进程.
    await viewModelTerminal(current.page);
    await expect(current.page.getByRole('tab', { name: /B4 PowerShell/u })).toHaveAttribute(
      'aria-selected',
      'true',
    );
    provider.setAction('terminal.write', {
      terminal_id: terminal,
      input: "& './sensitive.ps1'\n",
      wait_ms: 1000,
    });
    await submit(current.page, session, '运行本机测试脚本');
    await finished(current.page, session);
    await expect(current.page.locator('.xterm-screen')).toContainText('L02_INPUT_READY', {
      timeout: 15000,
    });
    provider.setAction('terminal.request_sensitive_input', {
      terminal_id: terminal,
      prompt: '请为本机测试脚本输入临时值',
    });
    await submit(current.page, session, '请求安全输入');
    const form = current.page.getByRole('form', { name: '终端安全输入' });
    await expect(form).toBeVisible({ timeout: 15000 });
    await current.page.screenshot({ path: info.outputPath('sensitive-input.png') });
    expect(await form.getByLabel('安全输入', { exact: true }).getAttribute('type')).toBe(
      'password',
    );
    await form.getByLabel('安全输入', { exact: true }).fill(testValue);
    await form.getByRole('button', { name: '提交', exact: true }).click();
    await finished(current.page, session);
    await expect(form).toHaveCount(0);
    expect(results.at(-1)!.outcome.payload!.value).toEqual({ status: 'completed' });
    await expect(current.page.locator('.xterm-screen')).toContainText(
      `L02_LENGTH:${testValue.length}`,
      { timeout: 15000 },
    );
    provider.setAction('terminal.read', { terminal_id: terminal, max_bytes: 65536 });
    await submit(current.page, session, '读取脱敏记录');
    await finished(current.page, session);
    expect(results.at(-1)!.outcome.payload!.value.output).toContain(
      `L02_LENGTH:${testValue.length}`,
    );
    expect(JSON.stringify(provider.requests).includes(testValue)).toBe(false);
    await current.page.screenshot({ path: info.outputPath('sensitive-completed.png') });
    provider.setAction('terminal.request_sensitive_input', {
      terminal_id: terminal,
      prompt: '取消测试，不需要填写',
    });
    await submit(current.page, session, '请求可取消输入');
    await expect(form).toBeVisible({ timeout: 15000 });
    await form.getByRole('button', { name: '取消', exact: true }).click();
    await finished(current.page, session);
    expect(results.at(-1)!.outcome.payload!.value).toEqual({ status: 'cancelled' });
    provider.setAction('shell.cmd', {
      script:
        'start "" /b cmd /d /c "echo child>child-start.txt & ping -n 120 127.0.0.1 >nul"\necho foreground',
      timeout_ms: 10000,
      max_output_bytes: 4096,
    });
    await submit(current.page, session, '启动本机后台服务');
    await finished(current.page, session);
    // 进程 is open already (the terminal was shown from it); show its tab again.
    await current.page.getByRole('tab', { name: '进程', exact: true }).click();
    const processes = current.page.getByRole('region', { name: '会话后台进程' });
    await expect(processes.getByText('Shell 后台进程', { exact: true })).toBeVisible({
      timeout: 15000,
    });
    await expect(processes.getByText('Agent 终端', { exact: true })).toBeVisible();
    const shell = processes.locator('li').filter({ hasText: 'Shell 后台进程' });
    await shell.getByRole('button', { name: '查看输出', exact: true }).click();
    await expect(processes.getByRole('region', { name: 'Shell 执行记录' })).toContainText(
      'foreground',
    );
    await current.page.screenshot({ path: info.outputPath('processes.png') });
    await shell.getByRole('button', { name: '停止', exact: true }).click();
    await expect(processes.getByText('Shell 后台进程', { exact: true })).toHaveCount(0, {
      timeout: 15000,
    });
    const agent = processes.locator('li').filter({ hasText: 'Agent 终端' });
    await agent.getByRole('button', { name: '停止', exact: true }).click();
    await expect(processes.getByText('本会话没有仍在运行的后台进程。')).toBeVisible({
      timeout: 15000,
    });
    expect(provider.errors).toEqual([]);
    expect(JSON.stringify(provider.requests).includes(testValue)).toBe(false);
    await current.app.close();
    closed = true;
    expect((await readFile(join(root, 'ipc-trace.log'))).byteLength).toBeGreaterThan(0);
    const matches: string[] = [];
    const files = await readdir(root, { recursive: true });
    let scanned = 0;
    for (const relative of files) {
      const path = join(root, relative);
      const metadata = await stat(path);
      if (!metadata.isFile()) continue;
      const data = await readFile(path);
      scanned += 1;
      if (data.includes(Buffer.from(testValue)) || data.includes(Buffer.from(testValue, 'utf16le')))
        matches.push(relative);
    }
    expect(scanned).toBeGreaterThan(10);
    expect(matches, 'temporary profile DB/artifacts/events/logs scan').toEqual([]);
    const scan = JSON.stringify({
      root,
      scanned,
      matched_paths: matches,
      provider_requests: provider.requests.length,
    });
    await writeFile(info.outputPath('sensitive-scan.json'), scan, 'utf8');
    await info.attach('sensitive-scan', {
      path: info.outputPath('sensitive-scan.json'),
      contentType: 'application/json',
    });
  } finally {
    if (!closed) await current.app.close();
    await provider.close();
  }
});
