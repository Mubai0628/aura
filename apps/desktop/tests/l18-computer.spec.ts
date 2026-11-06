import { expect, test, type Page } from '@playwright/test';
import { copyFile, mkdir, mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { isAbsolute, join, relative, resolve } from 'node:path';
import { writeProvider } from './s06-file-write-provider.mjs';
import type { ProviderRequest } from './s06-file-write-provider.mjs';
import { armReadOnlyWrite } from './s06/setup';
import { finished, launch, submit } from './s06/harness';
import { computerScript, field, lastResult } from './l18/model';
import { openTestWindow, type TestWindow } from './l18/window';

// Real Electron and Core, the desktop host Core starts, and Aura's own test window, added in the
// settings as a program by its path and started by the test. Only the model is scripted. The
// test window comes to the front when the model clicks a button in it, so this runs while no one
// is using the computer.
const NAME = 'L18 测试窗口';
const fixture = resolve(import.meta.dirname, '../../../target/debug/aura-desktop-fixture.exe');

type Envelope<T> = { ok: true; value: T } | { ok: false; error: unknown };
const value = <T>(result: Envelope<T>): T => {
  if (!result.ok) throw new Error(JSON.stringify(result.error));
  return result.value;
};

async function turnOn(page: Page, path: string) {
  return value(
    await page.evaluate(
      async ({ path, name }) => {
        const api = window.aura.v1.computer!;
        const current = await api.settings();
        if (!current.ok) return current;
        return api.updateSettings({
          expected_revision: current.value.revision ?? 0,
          settings: { ...current.value.settings, enabled: true, added: [{ name, path }] },
        });
      },
      { path, name: NAME },
    ),
  );
}

const catalog = async (page: Page) =>
  value(await page.evaluate(async (query) => window.aura.v1.computer!.catalog({ query }), NAME));
const controlling = async (page: Page, session: string) =>
  value(
    await page.evaluate(
      async (session_id) => window.aura.v1.computer!.controlling({ session_id }),
      session,
    ),
  );

/** What the model read after the step of the first turn. */
function after(requests: ProviderRequest[], turn: string, step: number): string {
  const next = requests.find((request) => request.turn === turn && request.step === step + 1);
  if (next === undefined) throw new Error(`no request after step ${step}`);
  return lastResult(next);
}

async function cleanup(root: string) {
  const rel = relative(resolve(tmpdir()), resolve(root));
  if (isAbsolute(rel) || rel.startsWith('..') || !rel.startsWith('aura-l18-computer-'))
    throw new Error(`Refusing to remove a non-fixture directory: ${root}`);
  await rm(root, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
}

// eslint-disable-next-line no-empty-pattern -- real processes on isolated data; see above.
test('L18 a model reads and acts on a program of the computer until the user stops it', async ({}, info) => {
  test.setTimeout(300_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l18-computer-'));
  await mkdir(join(root, 'programs'));
  const program = join(root, 'programs', 'aura-l18-window.exe');
  await copyFile(fixture, program);
  const provider = await writeProvider({
    path: 'unused.txt',
    text: '',
    planAction: computerScript(program),
    finalText: '点过确定、截了带编号的图；点卡住那一步被你停止了。',
  });
  let testWindow: TestWindow | undefined;
  let aura: Awaited<ReturnType<typeof launch>> | undefined;
  try {
    aura = await launch(root);
    const { page } = aura;

    // Computer control on, with the test window added by its path; the catalog lists it so.
    const on = await turnOn(page, program);
    expect(on.settings).toMatchObject({ enabled: true, added: [{ name: NAME, path: program }] });
    await expect
      .poll(async () => (await catalog(page)).apps.map((app) => app.app_id), { timeout: 40_000 })
      .toContain(program);
    testWindow = await openTestWindow(program, join(root, 'window.log'), [
      '--title',
      NAME,
      '--hang-ms',
      '30000',
      '--at',
      '160,160',
    ]);

    const workspace = join(root, 'workspace');
    await mkdir(workspace);
    const setup = await armReadOnlyWrite(page, workspace, provider.url, {
      title: 'L18 Computer',
      text: `在「${NAME}」里点一下确定，再截一张带编号的图`,
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    await page.locator('.session-row').first().click();

    // The first call on the program asks, in read-only mode too; the card names the program.
    const card = page.locator('section[aria-label="当前任务审批"]');
    const programLine = card.locator('.agent-approval-program');
    await expect(programLine).toContainText(NAME, {
      timeout: 90_000,
    });
    await expect(programLine).toBeVisible();
    await expect(programLine.locator('img.program-icon, svg')).toBeVisible();
    await expect(card.getByRole('button', { name: '总是允许', exact: true })).toBeVisible();
    await page.screenshot({ path: info.outputPath('approval.png') });
    await card.getByRole('button', { name: '本会话允许', exact: true }).click();

    // 卡住 keeps the window from answering, so the call waits and 正在操控 lists the program.
    await expect.poll(() => testWindow!.events(), { timeout: 120_000 }).toContain('click hang');
    expect(testWindow.events()).toContain('click ok 1');
    const entry = page.locator('.activity-compact-entry');
    if (await entry.isVisible()) await entry.click();
    const list = page.locator('section[aria-label="正在操控"]');
    await expect(list).toContainText(NAME, { timeout: 10_000 });
    await page.screenshot({ path: info.outputPath('controlling.png') });
    await list.getByRole('button', { name: `停止操控 ${NAME}`, exact: true }).click();
    await finished(page, session);
    await expect(list).toHaveCount(0);
    expect((await controlling(page, session)).apps).toEqual([]);
    expect(provider.errors).toEqual([]);

    // What the model read after each step; no step asked again.
    const turn = provider.requests[0]!.turn;
    const failure = /computer\.[a-z_]+: /u;
    expect(field(after(provider.requests, turn, 0), 'already_running')).toBe(true);
    const read = field(after(provider.requests, turn, 1), 'elements');
    expect(read).toContain('button "确定"');
    expect(read).toContain('button "卡住"');
    expect(after(provider.requests, turn, 2)).not.toMatch(failure);
    expect(field(after(provider.requests, turn, 3), 'marks')).toBeGreaterThan(1);
    // The model takes images, so the screenshot follows its result.
    const shown = provider.requests.find((request) => request.turn === turn && request.step === 4);
    expect(JSON.stringify(shown!.messages)).toContain('data:image/png;base64,');
    for (const step of [4, 5]) {
      const arranged = after(provider.requests, turn, step);
      expect(arranged).not.toMatch(failure);
      expect(field(arranged, 'changed')).toBe(true);
    }
    expect(after(provider.requests, turn, 6)).toContain('computer.stopped');

    // Stopping took back the session's answer: the next call on the program asks again.
    await submit(page, session, `再读一次「${NAME}」`);
    await expect(programLine).toContainText(NAME, {
      timeout: 90_000,
    });
    await expect(programLine).toBeVisible();
    await card.getByRole('button', { name: '拒绝', exact: true }).click();
    await finished(page, session);
    expect(testWindow.events().filter((event) => event.startsWith('click'))).toEqual([
      'click ok 1',
      'click hang',
    ]);
  } finally {
    testWindow?.close();
    await aura?.app.close();
    await provider.close();
    await cleanup(root);
  }
});
