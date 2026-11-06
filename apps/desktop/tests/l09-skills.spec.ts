import { expect, test, type Page } from '@playwright/test';
import { mkdir, mkdtemp, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { armReadOnlyWrite } from './s06/setup';
import { finished, launch, submit } from './s06/harness';
import { writeProvider, type ProviderRequest } from './s06-file-write-provider.mjs';
import { expectSecureSurface } from './electron-shell/renderer-api';
import { storedZip } from './zip-fixture';

const INSTRUCTION = 'E2E-SKILL-INSTRUCTION: answer in three short bullet points.';
const GUIDE = 'E2E-SKILL-GUIDE: keep each bullet under ten words.\n';

/** A Skill folder: SKILL.md with instructions and one reference file. */
async function skillFolder(parent: string, name: string): Promise<string> {
  const directory = join(parent, name);
  await mkdir(join(directory, 'references'), { recursive: true });
  await writeFile(
    join(directory, 'SKILL.md'),
    `---\nname: ${name}\ndescription: Notes in bullet points (${name})\n---\n\n${INSTRUCTION}\nSee [the guide](references/guide.md).\n`,
  );
  await writeFile(join(directory, 'references', 'guide.md'), GUIDE);
  return directory;
}

/** The last durable tool result Core sent back to the model in this request. */
function lastTool(request: ProviderRequest): Record<string, unknown> | undefined {
  const message = request.messages
    .filter((value) => (value as { role?: string }).role === 'tool')
    .at(-1) as { content: string } | undefined;
  return message ? (JSON.parse(message.content) as Record<string, unknown>) : undefined;
}

type Payload = { payload?: { value?: unknown } };
const value = (request: ProviderRequest) =>
  ((lastTool(request)?.outcome as Payload | undefined)?.payload?.value ?? undefined) as
    Record<string, unknown> | undefined;

type Content = { name: string; folder: string; files: string[]; instructions: string };
type Recorded = 'pinnedContext' | 'chosenContext';

/**
 * The model side. The first turn lists the user's Skills, loads `notes` by name and reads its
 * guide from the folder the load gave; a later turn reads the guide again; a turn with a pinned
 * or chosen Skill records what its first step carries and answers.
 */
async function skillProvider() {
  const phases = new Map<string, string>();
  const facts: {
    instructions?: string;
    searched?: Array<{ name: string }> | undefined;
    loaded?: Content | undefined;
    read?: unknown;
    reread?: unknown;
    pinnedContext?: string;
    chosenContext?: string;
  } = {};
  let mode: 'load' | 'reread' | Recorded = 'load';
  const done = { tool: null, arguments: {} };
  const guide = () => ({
    tool: 'fs.read',
    arguments: {
      path: join(facts.loaded?.folder ?? '', 'references', 'guide.md'),
      range: { range_kind: 'full' },
      explicit_encoding: null,
    },
  });
  const provider = await writeProvider({
    path: 'unused.txt',
    text: '',
    planAction(request) {
      const phase = phases.get(request.turn);
      phases.set(request.turn, 'done');
      if (mode === 'pinnedContext' || mode === 'chosenContext') {
        if (phase === undefined) facts[mode] = JSON.stringify(request.messages);
        return done;
      }
      if (mode === 'reread') {
        if (phase === undefined) {
          phases.set(request.turn, 'read');
          return guide();
        }
        if (phase === 'read') facts.reread = value(request)?.text;
        return done;
      }
      switch (phase) {
        case undefined:
          facts.instructions = JSON.stringify(request.messages);
          phases.set(request.turn, 'searched');
          return { tool: 'skills.search', arguments: { query: '' } };
        case 'searched':
          facts.searched = value(request)?.skills as Array<{ name: string }> | undefined;
          phases.set(request.turn, 'loaded');
          return { tool: 'skills.load', arguments: { name: 'notes' } };
        case 'loaded':
          facts.loaded = value(request) as Content | undefined;
          if (!facts.loaded?.folder) throw new Error('the load must give the Skill folder');
          phases.set(request.turn, 'read');
          return guide();
        case 'read':
          facts.read = value(request)?.text;
          return done;
        default:
          return done;
      }
    },
  });
  return {
    provider,
    facts,
    next: (next: 'reread' | Recorded) => (mode = next),
  };
}

async function skillSettings(page: Page) {
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await page
    .locator('.settings-navigation')
    .getByRole('button', { name: 'Skill', exact: true })
    .click();
  return page.getByRole('region', { name: 'Skill 设置' });
}

// eslint-disable-next-line no-empty-pattern -- real Electron/Core, TEMP Skill folders and a TEMP profile; scripted loopback model only.
test("L15 the user's Skills are usable as found, loaded by name, read in ReadOnly without a question, pinned and chosen", async ({}, info) => {
  test.setTimeout(300_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l09-skills-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const workspace = join(root, 'workspace');
  await mkdir(workspace);
  const source = await skillFolder(join(root, 'source'), 'notes');
  // The (TEMP) user's own folders: one Skill in .agents, one in .claude.
  await skillFolder(join(root, 'user-profile', '.agents', 'skills'), 'found');
  await skillFolder(join(root, 'user-profile', '.claude', 'skills'), 'drafts');
  const model = await skillProvider();
  const { app, page } = await launch(root);
  try {
    await expectSecureSurface(page);
    await app.evaluate(({ dialog }, path) => {
      Reflect.set(dialog, 'showOpenDialog', async () => ({ canceled: false, filePaths: [path] }));
    }, source);

    // Settings → Skill: the user's own Skills are usable as found, with no trust step; one is
    // switched off. The page carries no explanation.
    const settings = await skillSettings(page);
    await expect(settings.getByRole('note')).toHaveCount(0);
    await expect(settings.getByRole('list', { name: 'Skill 位置' })).toContainText(
      `我的 .claude：${join(root, 'user-profile', '.claude', 'skills')}`,
    );
    const list = settings.getByRole('list', { name: 'Skill', exact: true });
    const drafts = list.locator('li', { hasText: '我的 .claude' });
    await expect(drafts).toContainText('drafts');
    await expect(drafts).toContainText('可用');
    const found = list.locator('li', { hasText: '我的 .agents' });
    await expect(found).toContainText('可用');
    await found.getByRole('button', { name: '停用', exact: true }).click();
    await expect(settings.getByText('已停用 found')).toBeVisible();
    await expect(found).toContainText('已停用');

    // Importing a folder makes it usable at once.
    await settings.getByRole('button', { name: '导入文件夹…' }).click();
    await expect(settings.getByText('已导入，可以使用')).toBeVisible();
    const notes = list.locator('li', { hasText: '已导入 Aura' });
    await expect(notes).toContainText('可用');
    // The rest of a Skill is in its dialog, opened from its row.
    await notes.getByRole('button', { name: '配置 notes' }).click();
    const shown = page.getByRole('dialog', { name: 'notes' });
    await shown.getByRole('button', { name: '预览', exact: true }).click();
    const preview = shown.getByRole('region', { name: '预览 Skill notes' });
    await expect(preview).toContainText(INSTRUCTION);
    await expect(preview).toContainText('references/guide.md');
    await expect(preview.getByRole('button', { name: /信任/u })).toHaveCount(0);
    await shown.getByRole('button', { name: '收起', exact: true }).click();
    const storePath = (await shown.locator('.skill-row-path code').textContent()) ?? '';
    expect(storePath).not.toContain(join(root, 'source'));
    // "打开文件夹": Main opens the folder Core lists for this Skill (the shell is stubbed).
    await app.evaluate(({ shell }) => {
      Reflect.set(shell, 'openPath', async (path: string) => {
        Reflect.set(globalThis, 'auraOpenedSkillFolder', path);
        return '';
      });
    });
    await shown.getByRole('button', { name: '打开文件夹' }).click();
    await expect(shown.getByText('已打开 Skill 文件夹')).toBeVisible();
    expect(await app.evaluate(() => Reflect.get(globalThis, 'auraOpenedSkillFolder'))).toBe(
      storePath,
    );
    await shown.getByRole('button', { name: '关闭' }).click();
    await page.getByRole('button', { name: '返回会话' }).click();

    // ReadOnly: the instructions name the usable Skills; the model loads one by name and reads
    // its guide from the folder it was given, in Aura's own data, with no approval at all.
    const setup = await armReadOnlyWrite(page, workspace, model.provider.url, {
      title: 'L15 Skills',
      text: '按我的笔记 Skill 回答',
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    await finished(page, session);
    expect(model.facts.instructions).toContain('Skills you can load');
    expect(model.facts.instructions).toContain('- drafts');
    expect(model.facts.instructions).toContain('- notes');
    expect(model.facts.instructions).not.toContain('- found');
    // The Skill that comes with Aura is listed beside the user's own.
    expect(model.facts.searched?.map((skill) => skill.name)).toEqual([
      'drafts',
      'notes',
      'write-aura-plugin',
    ]);
    expect(model.facts.loaded?.name).toBe('notes');
    expect(model.facts.loaded?.folder).toBe(storePath);
    expect(model.facts.loaded?.files).toEqual(['references/guide.md']);
    expect(model.facts.loaded?.instructions).toContain(INSTRUCTION);
    expect(model.facts.read).toBe(GUIDE);

    // The imported copy changes: it stays usable, and the next read gets the new text.
    await writeFile(join(storePath, 'references', 'guide.md'), 'changed guide\n');
    const again = await skillSettings(page);
    await again.getByRole('button', { name: '刷新' }).click();
    await expect(
      again
        .getByRole('list', { name: 'Skill', exact: true })
        .locator('li', { hasText: '已导入 Aura' }),
    ).toContainText('可用');
    await page.getByRole('button', { name: '返回会话' }).click();
    model.next('reread');
    await submit(page, session, '再读一次那个 Skill 的说明');
    await finished(page, session);
    expect(model.facts.reread).toBe('changed guide\n');

    // Pinned: the next turn's instructions carry it whole, and the name list no longer names it.
    const pinning = await skillSettings(page);
    const pinned = pinning
      .getByRole('list', { name: 'Skill', exact: true })
      .locator('li', { hasText: '已导入 Aura' });
    await pinned.getByRole('button', { name: '配置 notes' }).click();
    const pinningDialog = page.getByRole('dialog', { name: 'notes' });
    await pinningDialog.getByRole('button', { name: '固定加载', exact: true }).click();
    await expect(pinningDialog.getByText('已固定加载 notes')).toBeVisible();
    await expect(pinned).toContainText(/固定加载 约 \d+ token/u);
    await expect(pinning.locator('.skill-pin-total')).toHaveText(/固定加载 约 \d+ \/ 20000 token/u);
    await pinningDialog.getByRole('button', { name: '关闭' }).click();
    await page.getByRole('button', { name: '返回会话' }).click();
    model.next('pinnedContext');
    await submit(page, session, '按固定的 Skill 回答');
    await finished(page, session);
    expect(model.facts.pinnedContext).toContain('Pinned Skill \\"notes\\"');
    expect(model.facts.pinnedContext).toContain(INSTRUCTION);
    expect(model.facts.pinnedContext).toContain('- drafts');
    expect(model.facts.pinnedContext).not.toContain('- notes');

    // A Skill packed as a ZIP file: Main's file dialog, then Core unpacks and checks it.
    const packed = join(root, 'brief.zip');
    await writeFile(
      packed,
      storedZip([
        {
          name: 'brief/SKILL.md',
          text: '---\nname: brief\ndescription: Short answers (brief)\n---\n\nAnswer in one sentence.\n',
        },
        { name: 'brief/references/style.md', text: 'One sentence.\n' },
      ]),
    );
    await app.evaluate(({ dialog }, path) => {
      Reflect.set(dialog, 'showOpenDialog', async () => ({ canceled: false, filePaths: [path] }));
    }, packed);
    const unpacked = await skillSettings(page);
    await unpacked.getByRole('button', { name: '导入 ZIP…' }).click();
    await expect(unpacked.getByText('已导入，可以使用')).toBeVisible();
    const brief = unpacked
      .getByRole('list', { name: 'Skill', exact: true })
      .locator('li', { hasText: 'brief' });
    await expect(brief).toContainText('已导入 Aura');
    await expect(brief).toContainText('可用');
    await page.getByRole('button', { name: '返回会话' }).click();

    // Chosen in the composer: the message it is sent with carries it from its first step.
    model.next('chosenContext');
    await page.getByRole('button', { name: '添加', exact: true }).click();
    await page.getByRole('menuitem', { name: /Skill/u }).click();
    await page
      .getByRole('region', { name: '选择 Skill' })
      .getByRole('button', { name: /brief/u })
      .click();
    const chips = page.getByRole('group', { name: '本轮使用的 Skill' });
    await expect(chips).toContainText('Skill：brief');
    await page.getByRole('textbox', { name: '输入消息' }).click();
    await page.keyboard.type('用选中的 Skill 回答');
    await page.keyboard.press('Enter');
    await expect
      .poll(() => model.facts.chosenContext ?? '', { timeout: 90_000 })
      .toContain('Answer in one sentence.');
    expect(model.facts.chosenContext).toContain('The user chose this Skill with this message');
    await finished(page, session);
    await expect(chips).toHaveCount(0);
    await expect(page.getByText('已选 Skill：brief')).toBeVisible();
    expect(model.provider.errors).toEqual([]);
  } finally {
    await app.close();
    await model.provider.close();
  }
});
