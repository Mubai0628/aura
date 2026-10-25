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

/** A Skill folder: SKILL.md with instructions and one reference file. */
async function skillFolder(parent: string, name: string): Promise<string> {
  const directory = join(parent, name);
  await mkdir(join(directory, 'references'), { recursive: true });
  await writeFile(
    join(directory, 'SKILL.md'),
    `---\nname: ${name}\ndescription: Notes in bullet points (${name})\n---\n\n${INSTRUCTION}\nSee [the guide](references/guide.md).\n`,
  );
  await writeFile(
    join(directory, 'references', 'guide.md'),
    'E2E-SKILL-GUIDE: keep each bullet under ten words.\n',
  );
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

/**
 * The model side: find the user's Skills, load the one named `notes`, read its guide, answer.
 * After the content changes it only searches and records what it found.
 */
async function skillProvider() {
  const phases = new Map<string, string>();
  const facts: {
    searched?: unknown;
    read?: unknown;
    contextAfterLoad?: string;
    foundAfterChange?: unknown;
    chosenContext?: string;
  } = {};
  let changed = false;
  let choosing = false;
  let id: string | undefined;
  const done = { tool: null, arguments: {} };
  const search = { tool: 'skills.search', arguments: { query: '' } };
  const provider = await writeProvider({
    path: 'unused.txt',
    text: '',
    planAction(request) {
      // A turn whose Skill the user chose: record what its first step carries, then answer.
      if (choosing) {
        choosing = false;
        facts.chosenContext = JSON.stringify(request.messages);
        return done;
      }
      const phase = phases.get(request.turn);
      if (changed) {
        if (phase === undefined) {
          phases.set(request.turn, 'searched');
          return search;
        }
        if (phase === 'searched') {
          facts.foundAfterChange = value(request)?.skills;
          phases.set(request.turn, 'done');
        }
        return done;
      }
      switch (phase) {
        case undefined:
          phases.set(request.turn, 'searched');
          return search;
        case 'searched': {
          facts.searched = value(request)?.skills;
          const hit = (
            facts.searched as Array<{ name: string; skill_id: string }> | undefined
          )?.find((skill) => skill.name === 'notes');
          if (!hit) throw new Error('the trusted Skill must be found');
          id = hit.skill_id;
          phases.set(request.turn, 'loaded');
          return { tool: 'skills.load', arguments: { skill_ids: [id] } };
        }
        case 'loaded':
          phases.set(request.turn, 'read');
          return {
            tool: 'skills.read_resource',
            arguments: { skill_id: id, path: 'references/guide.md' },
          };
        case 'read':
          facts.read = value(request)?.text;
          facts.contextAfterLoad = JSON.stringify(request.messages);
          phases.set(request.turn, 'done');
          return done;
        default:
          return done;
      }
    },
  });
  return {
    provider,
    facts,
    change: () => (changed = true),
    chooseNext: () => (choosing = true),
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
test('L09 a Skill is imported, trusted, loaded in ReadOnly without a question and stops after it changes', async ({}, info) => {
  test.setTimeout(300_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l09-skills-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const workspace = join(root, 'workspace');
  await mkdir(workspace);
  const source = await skillFolder(join(root, 'source'), 'notes');
  // One Skill already in the (TEMP) user's .agents folder: found, off and untrusted.
  await skillFolder(join(root, 'user-profile', '.agents', 'skills'), 'found');
  const model = await skillProvider();
  const { app, page } = await launch(root);
  try {
    await expectSecureSurface(page);
    await app.evaluate(({ dialog }, path) => {
      Reflect.set(dialog, 'showOpenDialog', async () => ({ canceled: false, filePaths: [path] }));
    }, source);

    // Settings → Skill: the found Skill is off; import the folder, preview, trust exactly it.
    // The page carries no explanation.
    const settings = await skillSettings(page);
    await expect(settings.getByRole('note')).toHaveCount(0);
    const list = settings.getByRole('list', { name: 'Skill', exact: true });
    await expect(list.locator('li', { hasText: 'found' })).toContainText('未启用，未信任');
    await settings.getByRole('button', { name: '导入文件夹…' }).click();
    await expect(settings.getByText('已导入；查看并信任后模型才能使用')).toBeVisible();
    const notes = list.locator('li', { hasText: '已导入 Aura' });
    await expect(notes).toContainText('未信任，模型看不到');
    await notes.getByRole('button', { name: '查看并信任' }).click();
    const preview = settings.getByRole('region', { name: '预览 Skill notes' });
    await expect(preview).toContainText(INSTRUCTION);
    await expect(preview).toContainText('references/guide.md');
    await preview.getByRole('button', { name: '信任并启用' }).click();
    await expect(notes).toContainText('可用');
    const storePath = (await notes.locator('.skill-row-path code').textContent()) ?? '';
    expect(storePath).not.toContain(join(root, 'source'));
    // "打开文件夹": Main opens the folder Core lists for this Skill (the shell is stubbed).
    await app.evaluate(({ shell }) => {
      Reflect.set(shell, 'openPath', async (path: string) => {
        Reflect.set(globalThis, 'auraOpenedSkillFolder', path);
        return '';
      });
    });
    await notes.getByRole('button', { name: '打开文件夹' }).click();
    await expect(settings.getByText('已打开 Skill 文件夹')).toBeVisible();
    expect(await app.evaluate(() => Reflect.get(globalThis, 'auraOpenedSkillFolder'))).toBe(
      storePath,
    );
    await page.getByRole('button', { name: '返回会话' }).click();

    // ReadOnly: the model finds, loads and reads it with no approval at all.
    const setup = await armReadOnlyWrite(page, workspace, model.provider.url, {
      title: 'L09 Skills',
      text: '按我的笔记 Skill 回答',
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    await finished(page, session);
    const found = (model.facts.searched as Array<{ name: string }>).map((skill) => skill.name);
    expect(found).toEqual(['notes']);
    expect(model.facts.read).toBe('E2E-SKILL-GUIDE: keep each bullet under ten words.\n');
    // The loaded instructions reached the model, labelled as a Skill, not as a user request.
    expect(model.facts.contextAfterLoad).toContain(INSTRUCTION);
    expect(model.facts.contextAfterLoad).toContain('Loaded Skill \\"notes\\"');

    // The imported copy changes: Settings asks for trust again and the next turn finds nothing.
    await writeFile(join(storePath, 'references', 'guide.md'), 'changed guide\n');
    const again = await skillSettings(page);
    await again.getByRole('button', { name: '刷新' }).click();
    await expect(
      again
        .getByRole('list', { name: 'Skill', exact: true })
        .locator('li', { hasText: '已导入 Aura' }),
    ).toContainText('内容已变化，需要重新信任');
    await page.getByRole('button', { name: '返回会话' }).click();
    model.change();
    await submit(page, session, '再用一次那个 Skill');
    await finished(page, session);
    expect(model.facts.foundAfterChange).toEqual([]);
    expect(model.provider.errors).toEqual([]);

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
    await expect(unpacked.getByText('已导入；查看并信任后模型才能使用')).toBeVisible();
    const brief = unpacked
      .getByRole('list', { name: 'Skill', exact: true })
      .locator('li', { hasText: 'brief' });
    await expect(brief).toContainText('已导入 Aura');
    await expect(brief).toContainText('未信任，模型看不到');

    // Trust it and choose it in the composer: the turn it starts carries it from its first step.
    await brief.getByRole('button', { name: '查看并信任' }).click();
    await unpacked
      .getByRole('region', { name: '预览 Skill brief' })
      .getByRole('button', { name: '信任并启用' })
      .click();
    await expect(brief).toContainText('可用');
    await page.getByRole('button', { name: '返回会话' }).click();
    model.chooseNext();
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
    expect(model.facts.chosenContext).toContain('The user chose this Skill for this turn');
    await finished(page, session);
    await expect(chips).toHaveCount(0);
    await expect(page.getByText('已选 Skill：brief')).toBeVisible();
    expect(model.provider.errors).toEqual([]);
  } finally {
    await app.close();
    await model.provider.close();
  }
});
