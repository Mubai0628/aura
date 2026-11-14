import { expect, test, type ElectronApplication, type Page } from '@playwright/test';
import { execFile } from 'node:child_process';
import { access, mkdir, mkdtemp, readdir, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { promisify } from 'node:util';
import { launch } from './s06/harness';
import { pixelAudit, type PixelFinding } from './pixel/audit';

const repository = resolve(import.meta.dirname, '../../..');
const core = join(repository, 'target/debug/aura-core.exe');
const manifest = join(repository, 'target/aura-dev-runtime/component-manifest.json');
const run = promisify(execFile);

/** Main's folder picker answers with `folder`, as the user choosing it would. */
async function pickFolder(app: ElectronApplication, folder: string) {
  await app.evaluate(({ dialog }, path) => {
    Reflect.set(dialog, 'showOpenDialog', async () => ({ canceled: false, filePaths: [path] }));
  }, folder);
}

async function openStorage(page: Page) {
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await page
    .locator('.settings-navigation')
    .getByRole('button', { name: '存储', exact: true })
    .click();
  const data = page.getByRole('region', { name: '数据', exact: true });
  await expect(data).toBeVisible({ timeout: 30_000 });
  return data;
}

async function overview(page: Page) {
  return page.evaluate(async () => {
    const result = await window.aura.v1.storage?.overview();
    if (!result?.ok) throw new Error(JSON.stringify(result));
    return result.value;
  });
}

/**
 * Aura closes, and Core after it: Core is Electron's child and finishes its shutdown on its own,
 * so the move waits until no Core started by that Electron runs.
 */
async function close(app: ElectronApplication) {
  const electron = app.process().pid;
  await app.close();
  for (let attempt = 0; ; attempt += 1) {
    const { stdout } = await run('powershell.exe', [
      '-NoProfile',
      '-Command',
      `@(Get-CimInstance Win32_Process -Filter "ParentProcessId=${electron} AND Name='aura-core.exe'").Count`,
    ]);
    if (stdout.trim() === '0') return;
    if (attempt >= 60) throw new Error(`Core started by Electron ${electron} is still running`);
    await new Promise((done) => setTimeout(done, 500));
  }
}

/** What a development start does before its window opens (`scripts/dev/move-data.mjs`). */
async function moveData(root: string) {
  const env = { ...process.env, APPDATA: join(root, 'Roaming'), LOCALAPPDATA: join(root, 'Local') };
  const answer = (stdout: string) =>
    stdout
      .split(/\r?\n/u)
      .filter((line) => line.trim() !== '')
      .map((line) => JSON.parse(line) as Record<string, unknown>);
  try {
    const { stdout } = await run(core, ['--move-data', `--component-manifest=${manifest}`], {
      env,
      timeout: 120_000,
      windowsHide: true,
    });
    return { code: 0, lines: answer(stdout) };
  } catch (error) {
    const failed = error as { code?: number; stdout?: string };
    return { code: failed.code, lines: answer(failed.stdout ?? '') };
  }
}

const exists = (path: string) =>
  access(path).then(
    () => true,
    () => false,
  );

/** The pixel audit once the page has settled: a dialog measures smaller while it opens. */
async function settledAudit(page: Page) {
  let findings: PixelFinding[] = await pixelAudit(page);
  for (let tries = 0; findings.length > 0 && tries < 10; tries += 1) {
    await page.waitForTimeout(500);
    findings = await pixelAudit(page);
  }
  return findings;
}

async function conversation(page: Page, folder: string) {
  return page.evaluate(
    async ({ selectedRoot }) => {
      const api = window.aura.v1;
      const workspace = await api.createWorkspace({
        display_name: 'L20.4 搬家',
        selected_root: selectedRoot,
      });
      if (!workspace.ok) throw new Error(JSON.stringify(workspace));
      const session = await api.createSession({
        workspace_id: workspace.value.workspace.workspace_id,
        title: 'L20.4 搬家前的对话',
      });
      if (!session.ok) throw new Error(JSON.stringify(session));
      return session.value.session_id;
    },
    { selectedRoot: folder },
  );
}

async function sessionIds(page: Page) {
  return page.evaluate(async () => {
    const value = await window.aura.v1.getWorkspaceSessions();
    if (!value.ok) throw new Error(JSON.stringify(value));
    return value.value.payload.sessions.map((session) => session.session_id);
  });
}

// eslint-disable-next-line no-empty-pattern -- real Electron and Core, isolated data root.
test('L20.4 the data moves to the folder the user picks at the next start, keeps its conversations, and moves back', async ({}, info) => {
  test.setTimeout(480_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l20-4-move-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const project = join(root, 'project');
  const chosen = join(root, 'Elsewhere');
  await mkdir(project);
  await mkdir(chosen);
  let current = await launch(root);
  try {
    const session = await conversation(current.page, project);
    let data = await openStorage(current.page);
    const before = await overview(current.page);
    expect(before.data_moved).toBe(false);
    expect(before.install).toBeNull();
    expect(before.data.path.startsWith(join(root, 'Local', 'Aura'))).toBe(true);
    expect(before.data.bytes).toBeGreaterThan(0);
    await expect(data).toContainText(before.data.path);
    await expect(current.page.getByRole('region', { name: '安装' })).toHaveCount(0);
    await expect(data.getByRole('button', { name: '移回默认位置' })).toHaveCount(0);

    // An empty folder the user picks is checked, then confirmed; a development run moves at its
    // next start, so nothing closes.
    await pickFolder(current.app, chosen);
    await data.getByRole('button', { name: '移动到…' }).click();
    const dialog = current.page.getByRole('dialog');
    await expect(dialog).toContainText(`搬到${chosen}？`);
    await expect(dialog).toContainText('要复制 ');
    // A development run moves its data (and a window profile when it has one). Global memory is
    // the released product's, and so is the Core log at the default place, which every
    // installation there writes: both stay.
    await expect(dialog.getByRole('listitem').first()).toContainText('数据');
    for (const stays of ['Core 日志', '全局记忆']) {
      await expect(dialog.getByRole('listitem').filter({ hasText: stays })).toHaveCount(0);
    }
    const sharedLog = join(root, 'Local', 'Aura', 'logs', 'core.jsonl');
    expect(await exists(sharedLog)).toBe(true);
    expect(await settledAudit(current.page)).toEqual([]);
    // Each part reads on one line beside its size, in the dialog's text column.
    const parts = await dialog.getByRole('listitem').evaluateAll((items) =>
      items.map((item) => {
        const box = item.getBoundingClientRect();
        const style = getComputedStyle(item);
        const line = parseFloat(style.lineHeight) || parseFloat(style.fontSize) * 1.5;
        return { text: item.textContent, width: box.width, lines: box.height / line };
      }),
    );
    expect(parts.length).toBeGreaterThan(0);
    for (const part of parts) {
      expect(part.width, JSON.stringify(part)).toBeGreaterThan(200);
      expect(part.lines, JSON.stringify(part)).toBeLessThan(1.5);
    }
    await dialog.getByRole('button', { name: '下次启动时搬' }).click();
    await expect(
      current.page.getByRole('status').filter({ hasText: '下次启动时搬到' }),
    ).toContainText(chosen);
    await expect(data.getByRole('button', { name: '移动到…' })).toBeDisabled();
    expect((await overview(current.page)).requested).toEqual({ destination: chosen });

    // The next start moves it.
    await close(current.app);
    const moved = await moveData(root);
    expect(moved.code, JSON.stringify(moved.lines)).toBe(0);
    expect(String(moved.lines.at(-1)?.outcome)).toMatch(/^Moved \{ from: None, to: Some\(/u);
    expect(await exists(join(chosen, 'data'))).toBe(true);

    current = await launch(root);
    data = await openStorage(current.page);
    const after = await overview(current.page);
    expect(after.data.path).toBe(chosen);
    expect(after.data_moved).toBe(true);
    expect(after.requested).toBeNull();
    expect(after.failure).toBeNull();
    await expect(data).toContainText(chosen);
    expect(await sessionIds(current.page)).toContain(session);

    // The copy left at the default place goes once Core has been ready a while.
    expect(after.old_copies).toBe(true);
    await expect(data.getByText('旧位置的副本正在删除')).toBeVisible();
    await expect(data.getByText('旧位置的副本正在删除')).toHaveCount(0, { timeout: 120_000 });
    expect((await overview(current.page)).old_copies).toBe(false);
    // The default place keeps only what never moves: the Chrome folder and the location record.
    const left = await readdir(before.data.path);
    expect(
      left.filter((name) => !['chrome', 'location.v1.json'].includes(name)),
      `${left}`,
    ).toEqual([]);
    expect(await exists(sharedLog)).toBe(true);
    expect(await exists(join(chosen, 'logs', 'core.jsonl'))).toBe(true);

    // And back to the default place.
    await data.getByRole('button', { name: '移回默认位置' }).click();
    await expect(current.page.getByRole('dialog')).toContainText('搬到默认位置？');
    await current.page.getByRole('dialog').getByRole('button', { name: '下次启动时搬' }).click();
    await expect(
      current.page.getByRole('status').filter({ hasText: '下次启动时搬到' }),
    ).toContainText('默认位置');
    await close(current.app);
    const back = await moveData(root);
    expect(back.code, JSON.stringify(back.lines)).toBe(0);
    expect(String(back.lines.at(-1)?.outcome)).toMatch(/^Moved \{ from: Some\(.+\), to: None \}$/u);

    current = await launch(root);
    data = await openStorage(current.page);
    const home = await overview(current.page);
    expect(home.data.path).toBe(before.data.path);
    expect(home.data_moved).toBe(false);
    expect(await sessionIds(current.page)).toContain(session);
    await expect
      .poll(async () => (await overview(current.page)).old_copies, { timeout: 120_000 })
      .toBe(false);
    expect(await exists(chosen)).toBe(false);
  } finally {
    await current.app.close().catch(() => undefined);
  }
});

// eslint-disable-next-line no-empty-pattern -- real Electron and Core, isolated data root.
test('L20.4 a folder that cannot take the data says why, a waiting move can be taken back, and a move that fails says so at the next start', async ({}, info) => {
  test.setTimeout(300_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l20-4-refused-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const busy = join(root, 'Busy');
  await mkdir(busy);
  let current = await launch(root);
  try {
    let data = await openStorage(current.page);
    const before = await overview(current.page);

    // A folder inside the data is refused with the reason; nothing is asked.
    const inside = join(before.data.path, 'inside');
    await mkdir(inside);
    await pickFolder(current.app, inside);
    await data.getByRole('button', { name: '移动到…' }).click();
    const refused = data.getByRole('alert');
    await expect(refused).toContainText(`不能搬到${inside}：`);
    await expect(refused.getByRole('listitem')).toContainText(['在现在的数据文件夹里']);
    await expect(current.page.getByRole('dialog')).toHaveCount(0);
    expect((await overview(current.page)).requested).toBeNull();

    // A move asked for can be taken back before the next start.
    await pickFolder(current.app, busy);
    await data.getByRole('button', { name: '移动到…' }).click();
    await current.page.getByRole('dialog').getByRole('button', { name: '下次启动时搬' }).click();
    const waiting = current.page.getByRole('status').filter({ hasText: '下次启动时搬到' });
    await expect(waiting).toContainText(busy);
    await waiting.getByRole('button', { name: '取消' }).click();
    await expect(waiting).toHaveCount(0);
    expect((await overview(current.page)).requested).toBeNull();

    // Asked again, and the folder gets a file of the user's before the next start: the move does
    // not touch it, the data stays, and the next start says why.
    await data.getByRole('button', { name: '移动到…' }).click();
    await current.page.getByRole('dialog').getByRole('button', { name: '下次启动时搬' }).click();
    await expect(waiting).toContainText(busy);
    await close(current.app);
    await writeFile(join(busy, 'notes.txt'), 'the user’s');
    const failed = await moveData(root);
    expect(failed.code, JSON.stringify(failed.lines)).toBe(3);
    expect(String(failed.lines.at(-1)?.outcome)).toContain('不是空文件夹');
    expect(await readdir(busy)).toEqual(['notes.txt']);
    expect(await readFile(join(busy, 'notes.txt'), 'utf8')).toBe('the user’s');

    current = await launch(root);
    data = await openStorage(current.page);
    const after = await overview(current.page);
    expect(after.data.path).toBe(before.data.path);
    expect(after.requested).toBeNull();
    const reason = current.page.getByRole('alert').filter({ hasText: '没有搬到' });
    await expect(reason).toContainText(`没有搬到${busy}：`);
    await expect(reason).toContainText('不是空文件夹');
    expect(await settledAudit(current.page)).toEqual([]);
    await reason.getByRole('button', { name: '知道了' }).click();
    await expect(reason).toHaveCount(0);
    expect((await overview(current.page)).failure).toBeNull();
  } finally {
    await current.app.close().catch(() => undefined);
  }
});
