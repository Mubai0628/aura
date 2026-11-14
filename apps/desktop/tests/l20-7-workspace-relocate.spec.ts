import { expect, test, type ElectronApplication, type Page } from '@playwright/test';
import { cp, mkdir, mkdtemp, readFile, rename, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { writeProvider } from './s06-file-write-provider.mjs';
import { armReadOnlyWrite } from './s06/setup';
import {
  answer,
  attemptCount,
  fileGrants,
  finished,
  hash,
  history,
  launch,
  pending,
  submit,
} from './s06/harness';
import { pixelAudit, type PixelFinding } from './pixel/audit';

async function projection(page: Page) {
  return page.evaluate(async () => {
    const value = await window.aura.v1.getWorkspaceSessions();
    if (!value.ok) throw new Error(JSON.stringify(value));
    return value.value.payload;
  });
}

async function workspaceOf(page: Page, workspaceId: string) {
  const found = (await projection(page)).workspaces.find((w) => w.workspace_id === workspaceId);
  if (found === undefined) throw new Error(`workspace ${workspaceId} is not in the projection`);
  return found;
}

/** Main's folder picker answers with `folder`, as the user choosing it would. */
async function pickFolder(app: ElectronApplication, folder: string) {
  await app.evaluate(({ dialog }, path) => {
    Reflect.set(dialog, 'showOpenDialog', async () => ({ canceled: false, filePaths: [path] }));
  }, folder);
}

/** A folder Core held a moment ago can stay busy for a short while after Aura closes. */
async function moveFolder(from: string, to: string) {
  for (let attempt = 0; ; attempt += 1) {
    try {
      await rename(from, to);
      return;
    } catch (error) {
      const code = (error as NodeJS.ErrnoException).code;
      if (attempt >= 20 || (code !== 'EBUSY' && code !== 'EPERM')) throw error;
      await new Promise((done) => setTimeout(done, 250));
    }
  }
}

/** Whether the turn just submitted stops for an approval or finishes without one. */
async function asksOrFinishes(page: Page, session: string): Promise<'asks' | 'finishes'> {
  let outcome: 'asks' | 'finishes' | undefined;
  await expect
    .poll(
      async () => {
        const view = await history(page, session);
        if ((view.active?.pending_approval_ids.length ?? 0) > 0) outcome = 'asks';
        else if (view.active === null && view.turns[0]?.turn.state.state === 'completed')
          outcome = 'finishes';
        return outcome;
      },
      { timeout: 90_000 },
    )
    .not.toBeUndefined();
  return outcome!;
}

/** The pixel audit once the page has settled: a dialog measures smaller while it opens. */
async function settledAudit(page: Page) {
  let findings: PixelFinding[] = await pixelAudit(page);
  for (let tries = 0; findings.length > 0 && tries < 10; tries += 1) {
    await page.waitForTimeout(500);
    findings = await pixelAudit(page);
  }
  return findings;
}

const relocateButton = (page: Page, name: string) =>
  page.getByRole('button', { name: `为 ${name} 选择新位置`, exact: true });

// eslint-disable-next-line no-empty-pattern -- real Electron and Core, isolated data root, scripted loopback model only.
test('L20.7 a moved Workspace is found again: its conversation carries on and its file approval is asked again', async ({}, info) => {
  test.setTimeout(360_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l20-7-moved-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const project = join(root, 'project');
  const moved = join(root, 'moved', 'project');
  await mkdir(project);
  await mkdir(join(root, 'moved'));
  const provider = await writeProvider({ path: 'approved.txt', text: 'first' });
  let current = await launch(root);
  try {
    // A read-only conversation writes a file and the user allows it for the conversation.
    const setup = await armReadOnlyWrite(current.page, project, provider.url, {
      title: 'L20.7 搬走的工作区',
      text: '写入 approved.txt',
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    const workspaceId = setup.workspaceId as string;
    await pending(current.page, session);
    await answer(current.page, session, 'allow_session');
    await finished(current.page, session);
    expect(await readFile(join(project, 'approved.txt'), 'utf8')).toBe('first');
    const before = await workspaceOf(current.page, workspaceId);
    const granted = await fileGrants(current.page, session);
    expect(granted.map((g) => g.availability)).toEqual(['in_force']);
    // It belongs to the folder the Workspace is at when the user answers.
    expect(granted.map((g) => g.scope.workspace_root)).toEqual([before.canonical_root]);

    // The folder moves on the same drive while Aura is closed: the same folder object, a new path.
    await current.app.close();
    await moveFolder(project, moved);
    current = await launch(root);
    const { app, page } = current;
    await expect(relocateButton(page, 'L20.7 搬走的工作区')).toBeVisible({ timeout: 60_000 });
    expect((await workspaceOf(page, workspaceId)).state).toBe('offline');

    // Choosing the moved folder switches at once: it is the same folder, so nothing is asked.
    await pickFolder(app, moved);
    await relocateButton(page, 'L20.7 搬走的工作区').click();
    await expect(relocateButton(page, 'L20.7 搬走的工作区')).toHaveCount(0, { timeout: 60_000 });
    await expect(page.getByRole('dialog')).toHaveCount(0);
    const after = await workspaceOf(page, workspaceId);
    expect(after.state).toBe('online');
    expect(after.selected_root).toBe(moved);
    expect(BigInt(after.authorization_epoch)).toBeGreaterThan(BigInt(before.authorization_epoch));

    // The conversation is where it was, with its earlier turn.
    const sessions = (await projection(page)).sessions.filter((s) => s.session_id === session);
    expect(sessions.map((s) => s.workspace_id)).toEqual([workspaceId]);
    expect((await history(page, session)).turns.length).toBeGreaterThan(0);

    // The next write in the same conversation is asked again and runs only after the answer:
    // the approval the user gave at the old path no longer covers the file.
    provider.setAction('fs.write', {
      path: 'approved.txt',
      text: 'after the move',
      expected: {
        expected_kind: 'file_hash',
        hash: await hash(page, workspaceId, session, 'approved.txt'),
      },
    });
    await submit(page, session, '搬走以后继续写入');
    expect(await asksOrFinishes(page, session)).toBe('asks');
    const asked = await pending(page, session);
    expect(await attemptCount(root, asked.operation.operation_id)).toBe(0);
    expect(await readFile(join(moved, 'approved.txt'), 'utf8')).toBe('first');
    await answer(page, session, 'allow_once');
    await finished(page, session);
    expect(await readFile(join(moved, 'approved.txt'), 'utf8')).toBe('after the move');
    expect((await history(page, session)).turns.length).toBeGreaterThan(1);
    // The old approval is withdrawn, not merely pointing at a path that is gone.
    expect((await fileGrants(page, session)).map((g) => g.availability)).toEqual(['revoked']);
    await expect(page.locator('.notification-viewport [role="alert"]')).toHaveCount(0);
    expect(provider.errors).toEqual([]);
  } finally {
    await current.app.close();
    await provider.close();
  }
});

/** A Workspace at `folder` with one conversation, made through the bridge as the sidebar does. */
async function workspaceWithSession(page: Page, name: string, folder: string, title: string) {
  return page.evaluate(
    async ({ name, folder, title }) => {
      const workspace = await window.aura.v1.createWorkspace({
        display_name: name,
        selected_root: folder,
      });
      if (!workspace.ok) throw new Error(JSON.stringify(workspace));
      const id = workspace.value.workspace.workspace_id;
      const session = await window.aura.v1.createSession({ workspace_id: id, title });
      if (!session.ok) throw new Error(JSON.stringify(session));
      return { workspaceId: id, sessionId: session.value.session_id };
    },
    { name, folder, title },
  );
}

/** The conversation is still the Workspace's own, opens, and reaches the files of the copy. */
async function carriesOn(
  page: Page,
  name: string,
  created: { workspaceId: string; sessionId: string },
  title: string,
) {
  const sessions = (await projection(page)).sessions;
  expect(
    sessions.filter((s) => s.session_id === created.sessionId).map((s) => s.workspace_id),
  ).toEqual([created.workspaceId]);
  const expand = page.getByRole('button', { name: `展开 ${name}`, exact: true });
  if (await expand.isVisible()) await expand.click();
  await page.getByRole('button', { name: title, exact: true }).click();
  await expect(page.getByRole('heading', { level: 2, name: title })).toBeVisible();
  expect(await hash(page, created.workspaceId, created.sessionId, 'only-in-copy.txt')).toBeTruthy();
}

// eslint-disable-next-line no-empty-pattern -- real Electron and Core with an isolated data root; no model.
test('L20.7 a copied folder is another folder: Aura asks before the Workspace moves to it', async ({}, info) => {
  test.setTimeout(360_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l20-7-copied-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  // Two projects copied to another drive, as C:\Dev was copied to D:\Dev.
  const kept = { source: join(root, 'c', 'kept'), copy: join(root, 'd', 'kept') };
  const gone = { source: join(root, 'c', 'gone'), copy: join(root, 'd', 'gone') };
  for (const folder of [kept, gone]) {
    await mkdir(join(folder.source, 'src'), { recursive: true });
    await writeFile(join(folder.source, 'src', 'main.txt'), 'the project\n');
  }
  let current = await launch(root);
  try {
    const first = await workspaceWithSession(
      current.page,
      'L20.7 拷走的工作区',
      kept.source,
      '拷走前的会话',
    );
    const second = await workspaceWithSession(
      current.page,
      'L20.7 原处删掉的工作区',
      gone.source,
      '删掉前的会话',
    );
    for (const folder of [kept, gone]) {
      await cp(folder.source, folder.copy, { recursive: true });
      await writeFile(join(folder.copy, 'only-in-copy.txt'), 'the copy\n');
    }

    // The old folder is still there, so the Workspace is moved from its menu. The copy is another
    // folder: Aura asks first, and 取消 changes nothing.
    const { app, page } = current;
    const menu = page.getByRole('button', { name: 'L20.7 拷走的工作区 更多操作', exact: true });
    const dialog = page.getByRole('dialog', { name: '这是同一个项目吗？' });
    await pickFolder(app, kept.copy);
    await menu.click();
    await page.getByRole('menuitem', { name: '更改位置…' }).click();
    await expect(dialog).toBeVisible();
    await expect(dialog).toContainText(kept.source);
    await expect(dialog).toContainText(kept.copy);
    expect(await settledAudit(page)).toEqual([]);
    await dialog.getByRole('button', { name: '取消' }).click();
    await expect(dialog).toHaveCount(0);
    expect((await workspaceOf(page, first.workspaceId)).selected_root).toBe(kept.source);
    // 是，切换 moves it to the copy and leaves the old folder as it was.
    await menu.click();
    await page.getByRole('menuitem', { name: '更改位置…' }).click();
    await dialog.getByRole('button', { name: '是，切换' }).click();
    await expect(dialog).toHaveCount(0, { timeout: 60_000 });
    const atCopy = await workspaceOf(page, first.workspaceId);
    expect(atCopy.state).toBe('online');
    expect(atCopy.selected_root).toBe(kept.copy);
    expect(await readFile(join(kept.source, 'src', 'main.txt'), 'utf8')).toBe('the project\n');
    await carriesOn(page, 'L20.7 拷走的工作区', first, '拷走前的会话');

    // The other one's old folder is deleted while Aura is closed: Core cannot find it, and its
    // row offers 选择新位置. The Workspace already at its copy offers nothing.
    await current.app.close();
    await moveFolder(gone.source, join(root, 'c', 'deleted'));
    current = await launch(root);
    const reopened = current.page;
    const button = relocateButton(reopened, 'L20.7 原处删掉的工作区');
    await expect(button).toBeVisible({ timeout: 60_000 });
    await expect(relocateButton(reopened, 'L20.7 拷走的工作区')).toHaveCount(0);
    expect(await settledAudit(reopened)).toEqual([]);
    await pickFolder(current.app, gone.copy);
    await button.click();
    const asked = reopened.getByRole('dialog', { name: '这是同一个项目吗？' });
    await expect(asked).toBeVisible();
    await expect(asked).toContainText(gone.source);
    await expect(asked).toContainText(gone.copy);
    await asked.getByRole('button', { name: '是，切换' }).click();
    await expect(asked).toHaveCount(0, { timeout: 60_000 });
    await expect(button).toHaveCount(0);
    const moved = await workspaceOf(reopened, second.workspaceId);
    expect(moved.state).toBe('online');
    expect(moved.selected_root).toBe(gone.copy);
    await carriesOn(reopened, 'L20.7 原处删掉的工作区', second, '删掉前的会话');
    await expect(reopened.locator('.notification-viewport [role="alert"]')).toHaveCount(0);
  } finally {
    await current.app.close();
  }
});
