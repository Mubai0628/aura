import { expect, test, type Page } from '@playwright/test';
import { mkdir, mkdtemp, readdir, readFile, stat, writeFile } from 'node:fs/promises';
import { DatabaseSync } from 'node:sqlite';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { writeProvider } from './s06-file-write-provider.mjs';
import { armReadOnlyWrite } from './s06/setup';
import { answer, finished, launch, pending } from './s06/harness';

const MARKER = 'AURA-L107-E2E-MARKER-5R1';
/** Global records that may name a deleted conversation without holding any of it. */
const P03_GLOBAL = ['mutation_journals.payload_json'];

async function store(root: string, name: 'aura.db' | 'aura-v2.db') {
  const files = (await readdir(root, { recursive: true })).filter(
    (file) => file.endsWith(`\\${name}`) && file.includes('\\data\\'),
  );
  expect(files, name).toHaveLength(1);
  return join(root, files[0]!);
}

/** `table.column` of every row of the store that contains `needle` (search tables through
 * themselves, not their shadow tables). */
function traces(file: string, needle: string): string[] {
  const db = new DatabaseSync(file, { readOnly: true });
  try {
    const tables = db
      .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")
      .all()
      .map((row) => String(row.name));
    const shadow = (name: string) =>
      ['_data', '_idx', '_docsize', '_config', '_content'].some((s) => name.endsWith(s)) &&
      tables.some((table) => table.endsWith('_fts') && name.startsWith(table));
    const found: string[] = [];
    for (const table of tables.filter((name) => !shadow(name))) {
      const columns = db
        .prepare(`SELECT name FROM pragma_table_info('${table}')`)
        .all()
        .map((row) => String(row.name));
      for (const column of columns) {
        const row = db
          .prepare(
            `SELECT count(*) AS n FROM "${table}" WHERE instr(CAST("${column}" AS BLOB), CAST(? AS BLOB)) > 0`,
          )
          .get(needle);
        if (Number(row!.n) > 0) found.push(`${table}.${column}`);
      }
    }
    return found;
  } finally {
    db.close();
  }
}

async function projection(page: Page) {
  return page.evaluate(async () => {
    const value = await window.aura.v1.getWorkspaceSessions();
    if (!value.ok) throw new Error(JSON.stringify(value));
    return value.value.payload;
  });
}

async function archivePage(page: Page) {
  const navigation = page.locator('.settings-navigation');
  if (!(await navigation.isVisible()))
    await page.getByRole('button', { name: '设置', exact: true }).click();
  await navigation.getByRole('button', { name: '归档', exact: true }).click();
  return {
    workspaces: page.getByRole('region', { name: '已归档的工作区' }),
    sessions: page.getByRole('region', { name: '已归档的会话' }),
  };
}

// eslint-disable-next-line no-empty-pattern -- real Electron/Core, isolated data root, scripted loopback model only.
test('L10.7 a conversation is archived, restored and deleted; a removed Workspace keeps its folder', async ({}, info) => {
  test.setTimeout(300_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l10-7-archive-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const workspace = join(root, 'workspace');
  await mkdir(join(workspace, '.memories'), { recursive: true });
  await writeFile(join(workspace, 'keep.txt'), 'the user file\n');
  await writeFile(join(workspace, '.memories', 'user-note.txt'), 'the user memory\n');
  const provider = await writeProvider({ path: 'result.txt', text: 'written by the model' });
  const { app, page } = await launch(root);
  try {
    // One conversation with a real turn: a write the user allows once.
    const setup = await armReadOnlyWrite(page, workspace, provider.url, {
      title: 'L10.7 归档',
      text: `${MARKER} 写一个文件`,
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    const workspaceId = setup.workspaceId as string;
    await pending(page, session);
    await answer(page, session, 'allow_once');
    await finished(page, session);
    expect(await readFile(join(workspace, 'result.txt'), 'utf8')).toBe('written by the model');
    const kept = await page.evaluate(async (id) => {
      const created = await window.aura.v1.createSession({ workspace_id: id, title: '保留的会话' });
      return created.ok ? created.value.session_id : JSON.stringify(created);
    }, workspaceId);
    // A conversation started through the API keeps an empty title; the sidebar shows 新会话.
    const stored = (await projection(page)).sessions.find((s) => s.session_id === session)!.title;
    const title = stored || '新会话';
    const menu = (name: string) =>
      page.getByRole('button', { name: `${name} 更多操作`, exact: true });
    const v2 = await store(root, 'aura-v2.db');
    const p03 = await store(root, 'aura.db');
    expect(traces(v2, MARKER).length).toBeGreaterThan(0);

    // Archive: the conversation leaves the sidebar and waits in 设置 → 归档; restore brings it back.
    await menu(title).click();
    await page.getByRole('menuitem', { name: '归档会话' }).click();
    await expect(menu(title)).toHaveCount(0);
    const archive = await archivePage(page);
    await expect(archive.sessions).toContainText(title);
    await archive.sessions.getByRole('button', { name: '恢复' }).click();
    await expect(archive.sessions).toContainText('没有归档的会话。');
    await page.getByRole('button', { name: '返回会话' }).click();
    await expect(menu(title)).toBeVisible();
    expect(traces(v2, MARKER).length).toBeGreaterThan(0);

    // Delete:
    // the sidebar only archives; Core refuses a conversation that is not archived, whoever asks.
    await menu(title).click();
    await expect(page.getByRole('menuitem', { name: '删除会话' })).toHaveCount(0);
    const refused = await page.evaluate(
      async (session_id) => window.aura.v1.deleteSession({ session_id }),
      session,
    );
    expect(refused.ok).toBe(false);
    expect(JSON.stringify(refused)).toContain('请先归档这个会话');
    await page.getByRole('menuitem', { name: '归档会话' }).click();
    await expect(menu(title)).toHaveCount(0);
    // In 设置 → 归档 it is asked first, then nothing of the conversation is left in either store.
    const shelved = await archivePage(page);
    await shelved.sessions.getByRole('button', { name: '删除' }).click();
    const dialog = page.getByRole('dialog');
    await expect(dialog).toContainText('无法恢复');
    await dialog.getByRole('button', { name: '删除会话' }).click();
    await expect(dialog).toHaveCount(0);
    await expect(shelved.sessions).toContainText('没有归档的会话。');
    await page.getByRole('button', { name: '返回会话' }).click();
    await expect(menu(title)).toHaveCount(0);
    expect((await projection(page)).sessions.map((s) => s.session_id)).toEqual([kept]);
    expect(traces(v2, session)).toEqual([]);
    expect(traces(v2, MARKER)).toEqual([]);
    expect(traces(p03, session).filter((place) => !P03_GLOBAL.includes(place))).toEqual([]);
    expect(traces(p03, MARKER)).toEqual([]);
    expect(traces(v2, kept).length + traces(p03, kept).length).toBeGreaterThan(0);

    // Delete the Workspace the same way: archived from the sidebar, deleted in 设置 → 归档; its
    // folder keeps every file.
    await menu('L10.7 归档').click();
    await expect(page.getByRole('menuitem', { name: /移除工作区|删除工作区/u })).toHaveCount(0);
    await page.getByRole('menuitem', { name: '归档工作区' }).click();
    await expect(menu('L10.7 归档')).toHaveCount(0);
    const shelf = await archivePage(page);
    await expect(shelf.workspaces).toContainText('L10.7 归档');
    await shelf.workspaces.getByRole('button', { name: '删除' }).click();
    await expect(dialog).toContainText('.memories');
    await dialog.getByRole('button', { name: '删除工作区' }).click();
    await expect(dialog).toHaveCount(0, { timeout: 60_000 });
    await expect(shelf.workspaces).toContainText('没有归档的工作区。');
    const after = await projection(page);
    expect(after.workspaces.map((w) => w.workspace_id)).not.toContain(workspaceId);
    expect(after.sessions).toEqual([]);
    // The execution store's rows go right after, on Core's own task.
    await expect.poll(() => traces(v2, workspaceId), { timeout: 30_000 }).toEqual([]);
    expect(traces(v2, kept)).toEqual([]);
    for (const file of ['keep.txt', 'result.txt', join('.memories', 'user-note.txt')]) {
      expect((await stat(join(workspace, file))).isFile(), file).toBe(true);
    }
    // Nothing complained about the conversation or the Workspace that are gone.
    await expect(page.locator('.notification-viewport [role="alert"]')).toHaveCount(0);
    expect(provider.errors).toEqual([]);
  } finally {
    await app.close();
    await provider.close();
  }
});
