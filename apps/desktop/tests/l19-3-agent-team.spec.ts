import { expect, test, type ElectronApplication, type Locator } from '@playwright/test';
import { mkdir, mkdtemp, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { armReadOnlyWrite } from './s06/setup';
import { finished, launch, submit } from './s06/harness';
import {
  writeProvider,
  type ProviderAction,
  type ProviderRequest,
} from './s06-file-write-provider.mjs';

// In the real window with the real Core; only the model is a local script. The conversation's
// model starts a sub-agent and waits for its report; the sub-agent first reads a file outside the
// workspace, which waits for the user. The 多 Agent page shows what needs the user first, then
// the main agent and its sub-agents on a canvas, a line from the main agent to each, lit while a
// task sheet or a report passes along it, or in a list. Picking one shows its whole record, read
// only, in the main area over the conversation, and the conversation's title leads back. The
// user decides the approval on the page, and in the next turn stops a second sub-agent from its
// record; the main agent's wait ends with the reason. The 近期活动 card counts the sub-agents
// and opens the page, which can fill the main area until something is picked.

const FIRST = { name: '读代码', file: 'entry-notes.md', said: '派一个子 Agent 读代码' };
const SECOND = { name: '查测试', file: 'test-notes.md', said: '再派一个子 Agent 查测试' };
const TOLD = '用户在多 Agent 页停止了这个子 Agent';
const PATH = /^M\d+ \d+( L\d+ \d+)+$/u;

/** Whether the request offers `tool`: a sub-agent's requests offer `agent.report`. */
const offers = (request: ProviderRequest, tool: string) =>
  (request.tools as { function: { name: string } }[]).some((offered) =>
    offered.function.name.startsWith(`${tool.replaceAll('.', '_')}__`),
  );

/** The model's side. The conversation's first turn starts 读代码, the second 查测试; each waits
 *  for its sub-agent, then answers. A sub-agent reads the file its task sheet names, then
 *  answers: its final answer is its report. */
function team(outside: string) {
  const turns: string[] = [];
  return (request: ProviderRequest): ProviderAction => {
    if (offers(request, 'agent.report')) {
      const job = JSON.stringify(request.messages).includes(FIRST.file) ? FIRST : SECOND;
      if (request.step === 0) {
        return {
          tool: 'fs.read',
          arguments: {
            path: join(outside, job.file),
            range: { range_kind: 'full' },
            explicit_encoding: null,
          },
        };
      }
      return { tool: null, arguments: {} };
    }
    if (!turns.includes(request.turn)) turns.push(request.turn);
    const job = turns.indexOf(request.turn) === 0 ? FIRST : SECOND;
    if (request.step === 0) {
      return {
        tool: 'agents.spawn',
        arguments: {
          name: job.name,
          task: {
            goal: `说出入口在哪个文件；先读 ${join(outside, job.file)}`,
            done_when: '说出入口文件',
          },
        },
      };
    }
    if (request.step === 1) {
      const agent = job === FIRST ? 'A1' : 'A2';
      return { tool: 'agents.wait', arguments: { agents: [agent], timeout_seconds: 300 } };
    }
    return { tool: null, arguments: {} };
  };
}

async function width(app: ElectronApplication, pixels: number) {
  await app.evaluate(({ BrowserWindow }, pixels) => {
    const window = BrowserWindow.getAllWindows()[0]!;
    const [, height] = window.getSize();
    window.setSize(pixels, Math.max(height!, 860));
  }, pixels);
}

/** Whether every card lies wholly on the board. */
async function allInSight(board: Locator) {
  const frame = await board.boundingBox();
  if (!frame) return false;
  const cards = await board.locator('.agent-canvas-node').all();
  for (const card of cards) {
    const box = await card.boundingBox();
    if (!box) return false;
    const inside =
      box.x >= frame.x - 1 &&
      box.y >= frame.y - 1 &&
      box.x + box.width <= frame.x + frame.width + 1 &&
      box.y + box.height <= frame.y + frame.height + 1;
    if (!inside) return false;
  }
  return cards.length > 0;
}

// eslint-disable-next-line no-empty-pattern -- real Electron and Core in TEMP; scripted loopback model only.
test('L19.3 the 多 Agent page shows, decides and stops sub-agents', async ({}, info) => {
  test.setTimeout(300_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l19-3-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const workspace = join(root, 'workspace');
  const outside = join(root, 'outside');
  await mkdir(join(workspace, 'src'), { recursive: true });
  await mkdir(outside, { recursive: true });
  await writeFile(join(workspace, 'src', 'main.rs'), 'fn main() {}\n');
  await writeFile(join(outside, FIRST.file), '# 入口\n\n入口在 src/main.rs。\n');
  await writeFile(join(outside, SECOND.file), '# 测试\n\n测试在 tests/。\n');
  const model = await writeProvider({
    path: 'unused.md',
    text: '',
    finalText: '收到。',
    planAction: team(outside),
  });
  const { app, page } = await launch(root);
  try {
    await width(app, 1440);
    const setup = await armReadOnlyWrite(page, workspace, model.url, {
      title: 'L19.3',
      text: FIRST.said,
      subagents: true,
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    await page.locator('.session-row').first().click();

    // The approval the sub-agent waits for shows above the composer, marked with its name, and
    // the 近期活动 card counts it.
    await expect(page.getByText(`子 Agent · ${FIRST.name}`)).toHaveCount(1, { timeout: 90_000 });
    const card = page.getByRole('complementary', { name: '近期活动', includeHidden: true });
    const entry = page
      .locator('.conversation-header')
      .getByRole('button', { name: /展开近期活动/u });
    if (await entry.isVisible()) await entry.click();
    const counted = card.getByRole('region', { name: '子 Agent', exact: true });
    await expect(counted).toContainText('1 个在运行 · 共 1 个 · 1 个要你处理');
    await counted.screenshot({ path: info.outputPath('l19-3-activity-card.png') });
    await counted.getByRole('button', { name: '查看', exact: true }).click();

    // The page: what needs the user, then the main agent and its sub-agent.
    const agentsPage = page.getByRole('region', { name: '多 Agent', exact: true });
    await expect(agentsPage).toBeVisible();
    const attention = agentsPage.getByRole('region', { name: '要你处理', exact: true });
    const approval = attention.getByRole('region', { name: '当前任务审批' });
    await expect(approval).toContainText(`子 Agent · ${FIRST.name}`);
    // The canvas shows first: the main agent above its sub-agent, a line between them, and the
    // approval marked on the sub-agent's card.
    const board = agentsPage.getByRole('group', { name: 'Agent 画布' });
    await expect(board).toBeVisible();
    const leadCard = board.getByRole('button', { name: /主 Agent/u });
    const firstCard = board.getByRole('button', { name: new RegExp(FIRST.name, 'u') });
    const firstLine = board.locator('.agent-canvas-line-path[data-agent="A1"]');
    await expect(leadCard).toContainText('S06 fixture model');
    await expect(leadCard).toContainText('运行中');
    await expect(leadCard).toContainText('等子 Agent 报告');
    await expect(firstCard).toContainText('A1');
    await expect(firstCard).toContainText('等审批');
    await expect(firstCard).toContainText('读取文件');
    await expect(firstCard.locator('.agent-canvas-badge')).toHaveText('1');
    await expect(firstLine).toHaveAttribute('d', PATH);
    // The canvas's own stylesheet applies under the production content security policy.
    await expect(firstCard).toHaveCSS('position', 'absolute');
    await expect.poll(() => allInSight(board)).toBe(true);
    await agentsPage.screenshot({ path: info.outputPath('l19-3-canvas-approval.png') });
    // Nothing is picked at first: the main area shows the conversation.
    const area = page.locator('.conversation-workspace');
    const canvas = page.locator('.conversation-canvas');
    await expect(leadCard).toHaveAttribute('aria-current', 'true');
    await expect(page.getByRole('region', { name: /子 Agent 记录/u })).toHaveCount(0);
    // Picking its card shows its whole record in the main area over the conversation, which takes
    // no input meanwhile: read only, with 停止 and the approval it waits for. The page shows none.
    await firstCard.click();
    await expect(firstCard).toHaveAttribute('aria-current', 'true');
    await expect(firstLine).toHaveAttribute('data-chosen', 'true');
    const record = area.getByRole('region', { name: `子 Agent 记录：${FIRST.name}` });
    await expect(record).toContainText(FIRST.file);
    await expect(agentsPage.getByRole('region', { name: /子 Agent 记录/u })).toHaveCount(0);
    await expect(record.getByRole('textbox')).toHaveCount(0);
    await expect(record.getByRole('button', { name: '停止', exact: true })).toBeVisible();
    await expect(record.getByRole('region', { name: '当前任务审批' })).toContainText(
      `子 Agent · ${FIRST.name}`,
    );
    await expect(canvas).toHaveAttribute('inert', '');
    // What floats in the conversation underneath, such as its header's 近期活动 entry, stays
    // under the record: each part is a stacking context below it.
    for (const part of [page.locator('.conversation-header'), canvas])
      await expect(part).toHaveCSS('z-index', '0');
    await page.screenshot({ path: info.outputPath('l19-3-page-approval.png') });

    // The user decides the approval on the page: the sub-agent reads and reports, its report runs
    // up its line to the main agent as light, it is done, and the main agent answers.
    await approval.getByRole('button', { name: '仅此一次', exact: true }).click();
    const firstFlow = board.locator('.agent-canvas-flow[data-agent="A1"]');
    await expect(firstFlow).toHaveAttribute('data-kind', 'report', { timeout: 30_000 });
    await agentsPage.screenshot({ path: info.outputPath('l19-3-canvas-report.png') });
    await finished(page, session);
    await expect(firstCard).toContainText('已交报告', { timeout: 30_000 });
    await expect(attention).toHaveCount(0);
    await expect(page.getByText(`子 Agent · ${FIRST.name}`)).toHaveCount(0);
    await expect(leadCard).toContainText('空闲', { timeout: 30_000 });
    // The light goes out once the report has passed.
    await expect(firstFlow).toHaveCount(0, { timeout: 10_000 });
    // The record follows the sub-agent to its answer.
    await expect(record).toContainText('收到。', { timeout: 30_000 });
    // The conversation's title in the record's path leads back to the conversation.
    await record.getByRole('navigation', { name: '会话路径' }).getByRole('button').click();
    await expect(record).toHaveCount(0);
    await expect(leadCard).toHaveAttribute('aria-current', 'true');
    await expect(canvas).not.toHaveAttribute('inert', '');

    // The list shows the same agents, a row each.
    await agentsPage.getByRole('button', { name: '列表', exact: true }).click();
    await expect(board).toHaveCount(0);
    const rows = agentsPage.getByRole('list', { name: 'Agent 列表' }).getByRole('listitem');
    await expect(rows).toHaveCount(2);
    const lead = rows.nth(0);
    await expect(lead).toContainText('主 Agent');
    await expect(lead).toContainText('S06 fixture model');
    await expect(lead).toContainText('空闲');
    await expect(lead.locator('.agent-team-tokens')).toHaveText(/^[1-9][\d,]*$/u);
    await expect(lead.getByRole('button')).toHaveAttribute('aria-current', 'true');
    const first = rows.nth(1);
    await expect(first).toContainText(FIRST.name);
    await expect(first).toContainText('A1');
    await expect(first).toContainText('S06 fixture model');
    await expect(first).toContainText('已交报告');
    await expect(first.locator('.agent-team-tokens')).toHaveText(/^[1-9][\d,]*$/u);
    await expect(first.getByRole('button')).toHaveCSS('display', 'grid');
    await expect(first.locator('.agent-team-state')).toHaveCSS('white-space', 'nowrap');

    // The page can fill the main area, and goes back.
    const main = page.locator('main.workspace-main');
    await agentsPage.getByRole('button', { name: '放大多 Agent 页', exact: true }).click();
    await expect(main).toHaveAttribute('data-right-expanded', 'true');
    await expect
      .poll(async () => (await agentsPage.boundingBox())?.width ?? 0)
      .toBeGreaterThan(900);
    // Wide, the list's columns line up under their names.
    await expect(agentsPage.locator('.agent-team-head')).toHaveCSS('display', 'grid');
    await page.screenshot({ path: info.outputPath('l19-3-page-expanded.png') });
    // The canvas fills it too, every agent still in sight.
    await agentsPage.getByRole('button', { name: '画布', exact: true }).click();
    await expect(board).toBeVisible();
    await expect.poll(() => allInSight(board)).toBe(true);
    await page.screenshot({ path: info.outputPath('l19-3-canvas-expanded.png') });
    await agentsPage.getByRole('button', { name: '还原多 Agent 页', exact: true }).click();
    await expect(main).not.toHaveAttribute('data-right-expanded', 'true');
    // A pick while it fills the main area shows there, and the page goes back to the sidebar.
    await agentsPage.getByRole('button', { name: '放大多 Agent 页', exact: true }).click();
    await expect(main).toHaveAttribute('data-right-expanded', 'true');
    await firstCard.click();
    await expect(main).not.toHaveAttribute('data-right-expanded', 'true');
    await expect(record).toBeVisible();

    // The next turn starts 查测试: its task sheet runs down a new line to it as light, and it
    // waits for the user too; the user stops it from its record, and the waiting main agent is
    // told why its wait ended.
    await submit(page, session, SECOND.said);
    const secondCard = board.getByRole('button', { name: new RegExp(SECOND.name, 'u') });
    const secondLine = board.locator('.agent-canvas-line-path[data-agent="A2"]');
    await expect(board.locator('.agent-canvas-flow[data-agent="A2"]')).toHaveAttribute(
      'data-kind',
      'task',
      { timeout: 90_000 },
    );
    await expect(secondLine).toHaveAttribute('d', PATH);
    await expect(secondCard).toContainText('等审批', { timeout: 90_000 });
    await expect(attention.getByRole('region', { name: '当前任务审批' })).toContainText(
      `子 Agent · ${SECOND.name}`,
    );
    await secondCard.click();
    const stopped = area.getByRole('region', { name: `子 Agent 记录：${SECOND.name}` });
    await expect(stopped).toContainText(SECOND.file);
    await stopped.getByRole('button', { name: '停止', exact: true }).click();
    await expect(secondCard).toHaveCount(0);
    await leadCard.click({ button: 'right' });
    await agentsPage.getByRole('menuitem', { name: '展开全部已停止（1）' }).click();
    // What Aura tells the main agent about the stop runs up the line as the stop.
    await expect(board.locator('.agent-canvas-flow[data-agent="A2"]')).toHaveAttribute(
      'data-kind',
      'failed',
    );
    await expect(board.locator('.agent-canvas-chip[data-kind="failed"]')).toHaveText('已停止');
    await expect(secondCard).toContainText('已停止');
    await expect(secondCard).toHaveClass(/is-ended/u);
    await expect(secondLine).toHaveAttribute('data-ended', 'true');
    await expect(stopped.getByRole('button', { name: '停止', exact: true })).toHaveCount(0);
    await expect(attention).toHaveCount(0, { timeout: 30_000 });
    await finished(page, session);
    await expect(leadCard).toContainText('空闲', { timeout: 30_000 });
    expect(
      model.requests.some(
        (request) =>
          !offers(request, 'agent.report') && JSON.stringify(request.messages).includes(TOLD),
      ),
    ).toBe(true);
    await page.screenshot({ path: info.outputPath('l19-3-page-stopped.png') });
    await stopped.getByRole('navigation', { name: '会话路径' }).getByRole('button').click();
    await expect(stopped).toHaveCount(0);
    // With the sidebar open the card sits behind the entry in the conversation's header.
    if (!(await counted.isVisible()) && (await entry.isVisible())) await entry.click();
    await expect(counted).toContainText('没有在运行的 · 共 2 个');

    // In a narrower window the canvas keeps every agent in sight.
    await width(app, 1100);
    await expect.poll(() => allInSight(board)).toBe(true);
    await agentsPage.screenshot({ path: info.outputPath('l19-3-canvas-narrow.png') });
    expect(model.errors).toEqual([]);
  } finally {
    await app.close();
    await model.close();
  }
});
