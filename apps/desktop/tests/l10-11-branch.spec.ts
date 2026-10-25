import { expect, test, type Page } from '@playwright/test';
import { mkdir, mkdtemp } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { armReadOnlyWrite } from './s06/setup';
import { answer, history, launch } from './s06/harness';
import {
  writeProvider,
  type ProviderAction,
  type ProviderRequest,
} from './s06-file-write-provider.mjs';
import { copiedThroughMain, stubMainClipboard } from './l10-12/main-clipboard';

// In the real window with the real Core; only the model is a local script. 新建分支
// under an answer asks first, makes the branch and opens it at once; the branch shows the carried
// turn, copies it, continues on its own with the carried conversation sent first, keeps the
// source's permission mode, and stays whole after the source is deleted.

const USER = '把说明写进 l10-11-note.md';
const ANSWER = '已写好 **l10-11-note.md**。';
const NEXT = '在分支里继续：再说一遍写了什么';

/** The conversation's last turn has answered and nothing runs. */
async function answered(page: Page, session: string, turns: number) {
  await expect
    .poll(
      async () => {
        const view = await history(page, session);
        return !view.active && view.turns.length === turns
          ? view.turns.at(-1)?.turn.state.state
          : undefined;
      },
      { timeout: 90_000 },
    )
    .toBe('completed');
}

// eslint-disable-next-line no-empty-pattern -- real Electron/Core in TEMP; scripted loopback model only.
test('L10.11 a branch carries the conversation, opens at once, continues on its own and outlives its source', async ({}, info) => {
  test.setTimeout(300_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l10-11-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const workspace = join(root, 'workspace');
  await mkdir(workspace);
  // The first turn writes the note; every later turn of either conversation just answers.
  let first: string | undefined;
  const model = await writeProvider({
    path: 'l10-11-note.md',
    text: '说明\n',
    finalText: ANSWER,
    planAction: (request: ProviderRequest, action: ProviderAction): ProviderAction =>
      (first ??= request.turn) === request.turn && request.step === 0
        ? action
        : { tool: null, arguments: {} },
  });
  const { app, page } = await launch(root);
  try {
    await stubMainClipboard(app);
    const setup = await armReadOnlyWrite(page, workspace, model.url, {
      title: 'L10.11',
      text: USER,
    });
    expect(setup.error).toBeUndefined();
    const source = setup.sessionId as string;
    await page.locator('.session-row').first().click();
    await answer(page, source, 'allow_once');
    await answered(page, source, 1);

    // 新建分支 asks first and says what is carried.
    const footer = page.locator('.agent-turn').last().locator(':scope > .agent-turn-footer');
    await footer.getByRole('button', { name: '新建分支' }).click();
    const dialog = page.getByRole('dialog', { name: '从这一轮新建分支？' });
    await expect(dialog).toContainText('文件不会分支');
    await expect(dialog).toContainText('仍会先问你');
    await dialog.screenshot({ path: info.outputPath('branch-dialog.png') });
    await dialog.getByRole('button', { name: '新建分支' }).click();
    await expect(dialog).toHaveCount(0);

    // The branch opens at once: the carried turn, then the line naming where it came from.
    const line = page.getByRole('note', { name: '分支起点' });
    await expect(line).toContainText('第 1 轮分支', { timeout: 30_000 });
    const branch = (await page.locator('.agent-conversation').getAttribute('data-session-id'))!;
    expect(branch).not.toBe(source);
    await expect(page.locator('.session-row', { hasText: '分支：' })).toHaveCount(1);
    const carried = page.locator('.agent-turn').first();
    await expect(carried.locator('.agent-user-message')).toContainText(USER);
    await expect(carried.locator('.agent-final-report')).toContainText('已写好');
    await page.screenshot({ path: info.outputPath('branch-opened.png') });
    const mode = await page.evaluate(async (session_id) => {
      const state = await window.aura.v1.getSessionPermissionState({ session_id });
      return state.ok ? state.value.mode : JSON.stringify(state);
    }, branch);
    expect(mode, 'the branch keeps the source’s permission mode').toBe('read_only');

    // The carried answer copies whole, through Main.
    await carried
      .locator(':scope > .agent-turn-footer')
      .getByRole('button', { name: '复制', exact: true })
      .click();
    await expect.poll(() => copiedThroughMain(app)).toContain(ANSWER);

    // The branch continues on its own; the model is sent the carried conversation first.
    const before = model.requests.length;
    const submitted = await page.evaluate(
      async ({ session_id, text }) =>
        window.aura.v1.submitAgentMessage({ session_id, text, destination: { lane: 'next_turn' } }),
      { session_id: branch, text: NEXT },
    );
    expect(submitted.ok, JSON.stringify(submitted)).toBe(true);
    await answered(page, branch, 2);
    const sent = JSON.stringify(model.requests.slice(before).map((r) => r.messages));
    expect(sent).toContain(USER);
    expect(sent).toContain('l10-11-note.md');
    expect(sent).toContain(NEXT);

    // Deleting the source leaves the branch whole. A conversation is archived first.
    const deleted = await page.evaluate(async (session_id) => {
      const archived = await window.aura.v1.archiveSession({ session_id });
      return archived.ok ? window.aura.v1.deleteSession({ session_id }) : archived;
    }, source);
    expect(deleted.ok, JSON.stringify(deleted)).toBe(true);
    const view = await history(page, branch);
    expect(view.turns.map((turn) => Boolean(turn.imported))).toEqual([true, false]);
    expect(view.branch_origin?.source_session_id).toBe(source);
    // Once the sidebar no longer lists the source, the line says it is gone.
    await expect(page.locator('.session-row')).toHaveCount(1);
    await line.getByRole('button', { name: '打开原会话' }).click();
    await expect(line).toContainText('原会话已不存在');
    await expect(page.locator('.agent-turn')).toHaveCount(2);
    await page.screenshot({ path: info.outputPath('branch-continued.png') });
    expect(model.errors).toEqual([]);
  } finally {
    await app.close();
  }
});
