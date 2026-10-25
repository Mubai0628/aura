import { expect, test } from '@playwright/test';
import { mkdtemp, mkdir, access, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { launch } from './s06/harness';
import { chooseDefaultTerminal, openTerminalTab } from './l10-8/terminal-tabs';

test('user PS7 and CMD: echo, stable resize, UI Ctrl+C, and confirmed close', async ({
  browserName,
}, info) => {
  void browserName;
  test.setTimeout(180_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l02-user-'));
  const work = join(root, 'workspace');
  await mkdir(work);
  const { app, page } = await launch(root, { AURA_TERMINAL_TEST_NO_PROFILE: '1' });
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  try {
    const created = await page.evaluate(async (selected_root) => {
      const workspace = await window.aura.v1.createWorkspace({
        display_name: 'L02 terminal local',
        selected_root,
      });
      if (!workspace.ok) throw new Error(workspace.error.message);
      const session = await window.aura.v1.createSession({
        workspace_id: workspace.value.workspace.workspace_id,
        title: 'L02 terminal local',
      });
      if (!session.ok) throw new Error(session.error.message);
      return session.value.session_id;
    }, work);
    await page.locator('.session-row').filter({ hasText: 'L02 terminal local' }).click();
    const firstOpenStarted = Date.now();
    // A terminal tab opens the default terminal, PowerShell 7, at once.
    await openTerminalTab(page);
    // Core readiness precedes the background whole-package PS7 verification.
    // A fresh model-free session can open before that verification completes.
    await expect(page.locator('.xterm')).toHaveCount(1, { timeout: 30_000 });
    await info.attach('first-terminal-open', {
      body: JSON.stringify({ elapsed_ms: Date.now() - firstOpenStarted }),
      contentType: 'application/json',
    });
    await page.locator('.terminal-host').click();
    const screen = page.locator('.xterm-accessibility-tree');
    await page.keyboard.type("Write-Output ('L02-' + 'USER-READY')");
    await page.keyboard.press('Enter');
    await expect(screen).toContainText('L02-USER-READY', { timeout: 15_000 });
    // The packaged page's CSP blocks inline <style>, where xterm writes its font, cell, cursor and
    // ANSI color rules. The terminal must still render with them.
    await page.keyboard.type("Write-Host ('L02-' + 'RED') -ForegroundColor Red");
    await page.keyboard.press('Enter');
    await expect(screen).toContainText('L02-RED');
    const rendered = await page.locator('.xterm-rows').evaluate((rows) => {
      const spans = [...rows.querySelectorAll('span')];
      const red = spans.find((span) => span.textContent === 'L02-RED');
      return {
        font: getComputedStyle(rows).fontFamily,
        foreground: getComputedStyle(rows).color,
        red: red ? getComputedStyle(red).color : null,
        cell: spans[0] ? getComputedStyle(spans[0]).display : null,
      };
    });
    expect(rendered.font).toContain('Consolas');
    expect(rendered.cell).toBe('inline-block');
    expect(rendered.red).not.toBeNull();
    expect(rendered.red).not.toBe(rendered.foreground);
    await expect
      .poll(() =>
        page
          .locator('.xterm-rows .xterm-cursor')
          .evaluateAll((items) => items.map((item) => getComputedStyle(item).backgroundColor)),
      )
      .toEqual([expect.stringMatching(/^rgb\(/)]);
    const original = await page.locator('.xterm').elementHandle();
    const divider = page.getByRole('separator', { name: '调整右侧工作区宽度' });
    for (let index = 0; index < 50; index++) {
      const box = await divider.boundingBox();
      if (!box) throw new Error('missing sidebar divider');
      await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
      await page.mouse.down();
      await page.mouse.move(box.x + (index % 2 ? 48 : -48), box.y + box.height / 2);
      await page.mouse.up();
    }
    await page.locator('.terminal-host').click();
    await page.keyboard.type("Write-Output ('L02-RESIZE-' + 'COMPLETE')");
    await page.keyboard.press('Enter');
    await expect(screen).toContainText('L02-RESIZE-COMPLETE');
    expect(await original!.evaluate((element) => element.isConnected)).toBe(true);
    expect((await screen.innerText()).split('L02-RESIZE-COMPLETE')).toHaveLength(2);
    await page.keyboard.type(
      "[Console]::Write(('X' * 10485760)); [Console]::WriteLine(('L02-LARGE-' + 'COMPLETE'))",
    );
    await page.keyboard.press('Enter');
    await expect(screen).toContainText('L02-LARGE-COMPLETE', { timeout: 30_000 });
    const started = join(work, 'started.txt');
    const finished = join(work, 'should-not-finish.txt');
    const quote = (value: string) => value.replaceAll("'", "''");
    await page.keyboard.type(
      `Set-Content -LiteralPath '${quote(started)}' -Value started; Start-Sleep -Seconds 60; Set-Content -LiteralPath '${quote(finished)}' -Value finished`,
    );
    await page.keyboard.press('Enter');
    await expect
      .poll(async () => {
        try {
          await access(started);
          return true;
        } catch {
          return false;
        }
      })
      .toBe(true);
    const before = await screen.innerText();
    await page.keyboard.press('Control+c');
    await expect.poll(() => screen.innerText(), { timeout: 10_000 }).not.toBe(before);
    await page.waitForTimeout(150);
    await page.keyboard.type("Write-Output ('L02-' + 'AFTER-INTERRUPT')");
    await page.keyboard.press('Enter');
    await expect(screen).toContainText('L02-AFTER-INTERRUPT', { timeout: 10_000 });
    await expect(access(finished)).rejects.toThrow();
    const list = await page.evaluate(async (session_id) => {
      const result = await window.aura.v1.terminal!.list({ session_id });
      if (!result.ok) throw new Error(result.error.message);
      return result.value;
    }, created);
    const pid = list.terminals[0]!.process_id;
    await page.screenshot({ path: join(root, 'terminal.png') });
    await info.attach('terminal-screenshot', {
      path: join(root, 'terminal.png'),
      contentType: 'image/png',
    });
    await page.getByRole('button', { name: '关闭终端 1', exact: true }).click();
    await expect(page.locator('.xterm')).toHaveCount(0);
    await expect
      .poll(async () => {
        const { stdout } = await promisify(execFile)('tasklist.exe', [
          '/FI',
          `PID eq ${pid}`,
          '/FO',
          'CSV',
          '/NH',
        ]);
        return stdout.includes(`"${pid}"`);
      })
      .toBe(false);
    await chooseDefaultTerminal(page, 'CMD');
    await openTerminalTab(page);
    await page.locator('.terminal-host').click();
    await page.keyboard.type('echo L02-CMD-READY');
    await page.keyboard.press('Enter');
    await expect(screen).toContainText('L02-CMD-READY');
    // A tab keeps its number in the conversation: the second terminal is 终端 2.
    await page.getByRole('button', { name: '关闭终端 2', exact: true }).click();
    await expect(page.locator('.xterm')).toHaveCount(0);
    expect(errors).toEqual([]);
    await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  } catch (error) {
    const screenshot = info.outputPath('failed-terminal.png');
    await page.screenshot({ path: screenshot });
    await info.attach('failed-terminal', { path: screenshot, contentType: 'image/png' });
    await writeFile(info.outputPath('failed-dom.html'), await page.content());
    throw error;
  } finally {
    await app.close();
  }
});
