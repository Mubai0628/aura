import { expect, test, type ElectronApplication, type Page } from '@playwright/test';
import { mkdir, mkdtemp, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { armReadOnlyWrite } from './s06/setup';
import { answer, launch } from './s06/harness';
import {
  writeProvider,
  type ProviderAction,
  type ProviderRequest,
} from './s06-file-write-provider.mjs';
import { allTurns, answered, openStep, send } from './l10-14/conversation';

// In the real window with the real Core; only the model is a local script.
// A question takes the user's own words, can be declined, and left alone shows a countdown and
// ends as "no response" while the task goes on; its tool record says what was asked and how it
// ended. A long step scrolls inside a dashed box. The 近期活动 card's close icon sits centred. A
// long conversation's branch opens at once and carries the rest behind it. The file tree's width
// follows its splitter. The settings pages read as cards with fewer words.

const ASK_OWN = '问我：要不要覆盖原文件';
const ASK_DECLINE = '问我：要不要提交';
const ASK_IDLE = '问我：用哪个分支名';
const RUN_SCRIPT = '运行检查脚本';
const OWN_WORDS = '先别动文件，我稍后自己决定';
const SCRIPT = Array.from(
  { length: 60 },
  (_, line) => `Write-Output "第 ${line + 1} 行：检查配置项 ${line + 1}"`,
).join('\n');
const TURNS = 14;

const question = (id: string, text: string, labels: string[]): ProviderAction => ({
  tool: 'query.user_question',
  arguments: {
    questions: [
      {
        id,
        text,
        answer_type: {
          type: 'single_choice',
          options: labels.map((label, index) => ({ id: `${id}_${index}`, label })),
        },
      },
    ],
  },
});

/** The turn's own words: the newest user message the model was sent. */
const asked = (request: ProviderRequest) =>
  JSON.stringify(
    (request.messages as { role?: string; content?: unknown }[]).findLast(
      (message) => message.role === 'user',
    )?.content ?? '',
  );

function plan(request: ProviderRequest): ProviderAction {
  if (request.step > 0) return { tool: null, arguments: {} };
  const said = asked(request);
  if (said.includes(ASK_OWN))
    return question('overwrite', '要覆盖原来的 notes.md 吗？', [
      '保留原文件（推荐）',
      '覆盖原文件',
    ]);
  if (said.includes(ASK_DECLINE))
    return question('commit', '要现在提交改动吗？', ['先不提交（推荐）', '现在提交']);
  if (said.includes(ASK_IDLE))
    return question('branch', '新分支叫什么？', ['feature/notes（推荐）', 'fix/notes']);
  if (said.includes(RUN_SCRIPT)) return { tool: 'shell.powershell', arguments: { script: SCRIPT } };
  return { tool: null, arguments: {} };
}

async function width(app: ElectronApplication, pixels: number) {
  await app.evaluate(({ BrowserWindow }, pixels) => {
    const window = BrowserWindow.getAllWindows()[0]!;
    const [, height] = window.getSize();
    window.setSize(pixels, Math.max(height!, 820));
  }, pixels);
}

/** What the model was sent in the step after the user's reply to a question. */
const readBack = (requests: ProviderRequest[], turn: string) =>
  JSON.stringify(requests.filter((r) => r.turn === turn && r.step > 0).map((r) => r.messages));

/** Nothing on the settings page is wider than the page. */
async function fitsTheWidth(page: Page) {
  const overflow = await page
    .locator('.settings-content')
    .evaluate((pane) => pane.scrollWidth - pane.clientWidth);
  expect(overflow).toBeLessThanOrEqual(0);
}

// eslint-disable-next-line no-empty-pattern -- real Electron/Core in TEMP; scripted loopback model only.
test('L10.14 questions, long steps, the activity card, a long branch, the file splitter and settings', async ({}, info) => {
  test.setTimeout(540_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l10-14-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const workspace = join(root, 'workspace');
  await mkdir(join(workspace, 'docs'), { recursive: true });
  await writeFile(join(workspace, 'notes.md'), '# 说明\n\n'.concat('一行说明。\n'.repeat(40)));
  await writeFile(join(workspace, 'docs', 'guide.md'), '# 指南\n');
  const model = await writeProvider({
    path: 'unused.md',
    text: '',
    finalText: '好的，已经按你的回复处理。',
    planAction: (request: ProviderRequest): ProviderAction => plan(request),
  });
  const { app, page } = await launch(root);
  try {
    await width(app, 1400);
    const setup = await armReadOnlyWrite(page, workspace, model.url, {
      title: 'L10.14',
      text: ASK_OWN,
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    await page.locator('.session-row').first().click();
    const form = page.getByRole('form', { name: '回答当前问题' });

    // Item 3: every question takes the user's own words.
    await expect(form).toBeVisible({ timeout: 90_000 });
    await expect(form).toContainText('要覆盖原来的 notes.md 吗？');
    await form.screenshot({ path: info.outputPath('question.png') });
    await form.getByText('自定义回复', { exact: true }).click();
    await form.getByRole('textbox', { name: /自定义回复/u }).fill(OWN_WORDS);
    await form.getByRole('button', { name: '提交回答' }).click();
    await answered(page, session, 1);
    const firstTurn = (await allTurns(page, session))[0]!;
    const own = page.locator('.agent-turn').first();
    const ownCard = own.locator('.agent-query-reply');
    await expect(ownCard).toHaveAttribute('data-outcome', 'answered');
    await expect(ownCard).toContainText('要覆盖原来的 notes.md 吗？');
    await expect(ownCard).toContainText(OWN_WORDS);
    await expect(ownCard.locator('.agent-query-reply-custom')).toHaveText('自定义');
    expect(readBack(model.requests, firstTurn.id)).toContain(OWN_WORDS);
    // The record shows the question, the replies the model offered (its recommendation marked)
    // and the reply.
    const ownStep = await openStep(own, '向你提问');
    await expect(ownStep.locator('.agent-tool-brief')).toHaveText('已回答');
    await expect(ownStep.locator('.agent-tool-facts')).toContainText(
      '问题 1：要覆盖原来的 notes.md 吗？',
    );
    await expect(ownStep.locator('.agent-tool-facts')).toContainText(
      '提供的回复：保留原文件（推荐）；覆盖原文件',
    );
    await expect(ownStep.locator('.agent-tool-facts')).toContainText(`（自定义）${OWN_WORDS}`);
    await own.screenshot({ path: info.outputPath('question-answered.png') });

    // Item 3: declining, and what the model is told.
    await send(page, session, ASK_DECLINE);
    await expect(form).toBeVisible({ timeout: 90_000 });
    await form.getByRole('button', { name: '拒绝回答' }).click();
    await answered(page, session, 2);
    const declinedTurn = (await allTurns(page, session))[1]!;
    const declined = page.locator('.agent-turn').nth(1);
    await expect(declined.locator('.agent-query-reply')).toHaveAttribute(
      'data-outcome',
      'declined',
    );
    expect(readBack(model.requests, declinedTurn.id)).toContain('chose not to answer');
    const declinedStep = await openStep(declined, '向你提问');
    await expect(declinedStep.locator('.agent-tool-brief')).toHaveText('你拒绝了回答');
    await expect(declinedStep.locator('.agent-tool-facts')).toContainText(
      '问题 1：要现在提交改动吗？',
    );

    // Item 3: left alone, a countdown shows after 30 s; at its end the question ends as "no
    // response" and the task goes on. Nothing here touches the question.
    await send(page, session, ASK_IDLE);
    await expect(form).toBeVisible({ timeout: 90_000 });
    const shownAt = Date.now();
    const timer = form.getByRole('timer');
    await expect(timer).toBeVisible({ timeout: 45_000 });
    const countdownAfter = Date.now() - shownAt;
    await expect(timer).toContainText('秒后视为暂时没有回应');
    await form.screenshot({ path: info.outputPath('question-countdown.png') });
    await answered(page, session, 3);
    const endedAfter = Date.now() - shownAt;
    console.log(`L10.14 countdown after ${countdownAfter} ms; no response after ${endedAfter} ms`);
    expect(countdownAfter).toBeGreaterThan(20_000);
    expect(endedAfter).toBeGreaterThan(50_000);
    const idleTurn = (await allTurns(page, session))[2]!;
    const idle = page.locator('.agent-turn').nth(2);
    await expect(idle.locator('.agent-query-reply')).toHaveAttribute('data-outcome', 'no_response');
    await expect(idle.locator('.agent-query-reply')).toContainText('你可能在忙或不在电脑前');
    expect(readBack(model.requests, idleTurn.id)).toContain('busy or away');
    expect(readBack(model.requests, idleTurn.id)).toContain('no_response');
    const idleStep = await openStep(idle, '向你提问');
    await expect(idleStep.locator('.agent-tool-brief')).toHaveText('无人回应，已超时');

    // Item 5: a step longer than its box scrolls inside a dashed frame. The script is refused, so
    // nothing runs.
    await send(page, session, RUN_SCRIPT);
    await answer(page, session, 'deny');
    await answered(page, session, 4);
    const scripted = await openStep(page.locator('.agent-turn').nth(3), '运行命令');
    const box = scripted.locator('.scroll-box');
    await expect(box).toHaveAttribute('data-overflowing', 'true');
    const frame = await box.evaluate((element) => {
      const style = getComputedStyle(element);
      return {
        border: style.borderTopStyle,
        overflow: style.overflowY,
        scrolls: element.scrollHeight > element.clientHeight,
        height: element.clientHeight,
      };
    });
    expect(frame).toMatchObject({ border: 'dashed', overflow: 'auto', scrolls: true });
    expect(frame.height).toBeLessThanOrEqual(362);
    await box.evaluate((element) => element.scrollTo({ top: element.scrollHeight }));
    await expect(box).toContainText('第 60 行');
    await scripted.screenshot({ path: info.outputPath('long-step.png') });

    // A long conversation for the branch: ten more turns that only answer.
    for (let turn = 5; turn <= TURNS; turn += 1) {
      await send(page, session, `第 ${turn} 轮：继续`);
      await answered(page, session, turn);
    }
    const source = await allTurns(page, session);
    expect(source).toHaveLength(TURNS);

    // Item 1: in a narrow window the 近期活动 card's close icon sits in the middle of its button.
    await width(app, 980);
    const entry = page
      .locator('.conversation-header')
      .getByRole('button', { name: /展开近期活动/u });
    await expect(entry).toBeVisible();
    await entry.click();
    const close = page
      .getByRole('complementary', { name: '近期活动' })
      .getByRole('button', { name: '收起卡片' });
    await expect(close).toBeVisible();
    const centred = await close.evaluate((button) => {
      const outer = button.getBoundingClientRect();
      const icon = button.querySelector('svg')!.getBoundingClientRect();
      return {
        width: outer.width,
        height: outer.height,
        dx: Math.abs(outer.left + outer.width / 2 - (icon.left + icon.width / 2)),
        dy: Math.abs(outer.top + outer.height / 2 - (icon.top + icon.height / 2)),
      };
    });
    console.log(`L10.14 activity close button: ${JSON.stringify(centred)}`);
    expect(centred.dx).toBeLessThanOrEqual(0.75);
    expect(centred.dy).toBeLessThanOrEqual(0.75);
    // A square (measured while the card may still be scaling open, so not exactly 26 px).
    expect(Math.abs(centred.width - centred.height)).toBeLessThan(0.5);
    await page.getByRole('complementary', { name: '近期活动' }).screenshot({
      path: info.outputPath('activity-card.png'),
    });
    await close.click();
    await width(app, 1400);

    // Decision 1: a long conversation's branch opens at once with its newest turns; the older ones
    // follow behind it while the branch can be used.
    await page.evaluate(() => {
      const seen: string[] = [];
      Reflect.set(window, 'auraTestCopying', seen);
      new MutationObserver(() => {
        const line = document.querySelector('.agent-branch-copying');
        if (line?.textContent && seen.at(-1) !== line.textContent) seen.push(line.textContent);
      }).observe(document.body, { childList: true, subtree: true, characterData: true });
    });
    const last = page.locator('.agent-turn').last().locator(':scope > .agent-turn-footer');
    await last.getByRole('button', { name: '新建分支' }).click();
    const started = Date.now();
    await page.getByRole('dialog').getByRole('button', { name: '新建分支' }).click();
    await expect(page.getByRole('note', { name: '分支起点' })).toBeVisible({ timeout: 30_000 });
    const opened = Date.now() - started;
    const branch = (await page.locator('.agent-conversation').getAttribute('data-session-id'))!;
    expect(branch).not.toBe(session);
    // At once, before the copy has finished: the user writes in the branch.
    const before = model.requests.length;
    await send(page, branch, '在分支里：最早那一轮问了什么？');
    await answered(page, branch, TURNS + 1);
    const carried = await allTurns(page, branch);
    expect(carried.slice(0, TURNS).map((turn) => turn.user)).toEqual(source.map((t) => t.user));
    expect(carried.slice(0, TURNS).every((turn) => turn.imported)).toBe(true);
    // The model was sent the whole carried conversation, its oldest turn included.
    const sent = JSON.stringify(model.requests.slice(before).map((r) => r.messages));
    expect(sent).toContain(ASK_OWN);
    expect(sent).toContain(OWN_WORDS);
    await expect(page.locator('.agent-branch-copying')).toHaveCount(0);
    const copying = await page.evaluate(() => Reflect.get(window, 'auraTestCopying') as string[]);
    console.log(
      `L10.14 branch of ${TURNS} turns opened after ${opened} ms; copying lines seen: ${JSON.stringify(copying)}`,
    );
    expect(opened).toBeLessThan(5_000);
    await page.screenshot({ path: info.outputPath('branch.png') });

    // Item 4: the splitter between the file tree and an open file sets the tree's width.
    await page.getByRole('button', { name: '打开工具', exact: true }).click();
    await page
      .getByRole('menu', { name: '选择工具' })
      .getByRole('menuitem', { name: /^文件/u })
      .click();
    const tree = page.getByRole('navigation', { name: '文件树' });
    await tree.getByRole('button', { name: 'notes.md', exact: true }).click();
    // Beside a conversation the 文件 page is narrow and stacks the tree above the file; widened
    // by its own splitter, the tree and the file sit side by side with a line between them.
    const side = (await page.getByRole('separator', { name: '调整右侧工作区宽度' }).boundingBox())!;
    await page.mouse.move(side.x + side.width / 2, side.y + side.height / 2);
    await page.mouse.down();
    await page.mouse.move(side.x - 360, side.y + side.height / 2, { steps: 8 });
    await page.mouse.up();
    const splitter = page.getByRole('separator', { name: '调整文件树宽度' });
    await expect(splitter).toBeVisible();
    const treeWidth = async () => (await tree.boundingBox())!.width;
    const start = await treeWidth();
    const handle = (await splitter.boundingBox())!;
    const y = handle.y + handle.height / 2;
    await page.mouse.move(handle.x + handle.width / 2, y);
    await page.mouse.down();
    await page.mouse.move(handle.x + handle.width / 2 + 70, y, { steps: 8 });
    await page.mouse.up();
    await expect.poll(treeWidth).toBeGreaterThan(start + 60);
    const stored = await page.evaluate(() => localStorage.getItem('aura.ui.v1.files-tree-width'));
    expect(Number(stored)).toBeGreaterThan(start + 60);
    // Dragged far, the open file keeps its part of the page.
    const wide = (await splitter.boundingBox())!;
    await page.mouse.move(wide.x + wide.width / 2, y);
    await page.mouse.down();
    await page.mouse.move(wide.x + 2000, y, { steps: 8 });
    await page.mouse.up();
    const viewer = page.locator('.files-panel:not([hidden]) .files-viewer');
    expect((await viewer.boundingBox())!.width).toBeGreaterThanOrEqual(239);
    await page
      .locator('.files-panel:not([hidden])')
      .screenshot({ path: info.outputPath('files.png') });

    // Item 6: the settings pages and the provider dialog, with fewer words and no overflow. Host
    // preparation's own buttons are never pressed here.
    await page.getByRole('button', { name: '设置', exact: true }).click();
    const card = page.locator('.model-card').first();
    await expect(card).toBeVisible();
    await expect(card.locator('.model-card-facts dt')).toHaveText([
      '可用上下文',
      '目录标称',
      '子 Agent',
    ]);
    // In view: test, default and edit; the rest is behind 更多 (the icon button without words).
    await expect(card.getByRole('button')).toHaveText([
      '测试可用性',
      /^(设为默认|当前默认)$/u,
      '编辑',
      '',
    ]);
    await card.getByRole('button', { name: /的更多操作$/u }).click();
    await expect(page.getByRole('menu')).toContainText('删除这个模型');
    await page.keyboard.press('Escape');
    await fitsTheWidth(page);
    await page.screenshot({ path: info.outputPath('settings-models.png') });
    await page
      .getByRole('region', { name: '未配置目录' })
      .getByRole('button', { name: /^配置 /u })
      .first()
      .click();
    const intro = page.getByRole('dialog');
    await expect(intro.getByRole('list', { name: '配置步骤' })).toBeVisible();
    await expect(intro.getByRole('button', { name: '添加此提供商' })).toBeVisible();
    await intro.screenshot({ path: info.outputPath('settings-provider.png') });
    // The catalog's model list, opened from the same dialog.
    await intro.getByRole('button', { name: '浏览模型列表' }).click();
    const browse = page.getByRole('dialog', { name: /模型目录$/u });
    await expect(browse).toBeVisible();
    await browse.screenshot({ path: info.outputPath('settings-catalog.png') });
    await browse.getByRole('button', { name: '关闭模型目录' }).click();
    if (await intro.isVisible())
      await intro.getByRole('button', { name: '关闭 Provider 配置' }).click();
    // The other dialogs these pages open; each is only looked at and closed, nothing is saved.
    await page
      .getByRole('region', { name: '已配置 Provider' })
      .getByRole('button', { name: /^配置 /u })
      .first()
      .click();
    const connections = page.getByRole('dialog');
    await expect(connections.getByRole('heading', { name: 'Endpoint / 连接' })).toBeVisible();
    await connections.screenshot({ path: info.outputPath('settings-connections.png') });
    await connections.locator('.connection-preview').first().click();
    await connections.screenshot({ path: info.outputPath('settings-endpoint.png') });
    await connections.getByRole('button', { name: '关闭 Provider 配置' }).click();
    await expect(connections).toHaveCount(0);
    await card.getByRole('button', { name: '编辑', exact: true }).click();
    const editor = page.getByRole('dialog', { name: '编辑模型' });
    await editor.screenshot({ path: info.outputPath('settings-model-editor.png') });
    await editor.getByRole('button', { name: '关闭模型编辑' }).click();
    await page.getByRole('button', { name: /^添加提供商/u }).click();
    const custom = page.getByRole('dialog', { name: '添加自定义 Provider' });
    await custom.screenshot({ path: info.outputPath('settings-custom-provider.png') });
    await custom.getByRole('button', { name: '关闭 Provider 配置' }).click();
    await page
      .getByRole('region', { name: '未配置目录' })
      .getByRole('button', { name: /^查看更多/u })
      .click();
    await page
      .getByRole('region', { name: '未配置目录' })
      .screenshot({ path: info.outputPath('settings-catalog-expanded.png') });
    await page
      .locator('.settings-navigation')
      .getByRole('button', { name: '权限与沙箱', exact: true })
      .click();
    // The saved default is marked; the isolation facts are rows of states.
    await expect(page.locator('.permission-default-current')).toHaveText('当前默认');
    await expect(page.locator('.security-facts > div')).toHaveCount(4);
    await fitsTheWidth(page);
    await page.screenshot({ path: info.outputPath('settings-security.png'), fullPage: true });
    expect(model.errors).toEqual([]);
  } finally {
    await app.close();
    await model.close();
  }
});
