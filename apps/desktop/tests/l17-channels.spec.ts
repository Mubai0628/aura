import { expect, test } from '@playwright/test';
import { cp, copyFile, mkdir, mkdtemp, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { launch } from './s06/harness';
import { writeProvider, type ProviderRequest } from './s06-file-write-provider.mjs';
import { expectSecureSurface } from './electron-shell/renderer-api';
import { pollingPlatform, socketPlatform, SKILL } from './l17/platforms';
import {
  addAndTrust,
  configure,
  pluginRow,
  pluginSettings,
  scriptedDefaults,
  workspace,
} from './l17/setup';

/** The newest user message as the model reads it. */
function userText(request: ProviderRequest): string {
  const user = request.messages
    .filter((value) => (value as { role?: string }).role === 'user')
    .at(-1) as { content?: unknown } | undefined;
  const content = user?.content;
  if (typeof content === 'string') return content;
  return Array.isArray(content)
    ? content.map((part) => String((part as { text?: string }).text ?? '')).join('')
    : '';
}

// The written file's answer: three paragraphs, longer together than the example's channel takes
// in one message (500 characters), so it reaches the platform in three pieces.
const PARAGRAPHS = ['甲', '乙', '丙'].map((word) => word.repeat(300));

/** The model side: a turn asked to write a file writes `from-chat.txt`; any other just answers. */
async function chatProvider() {
  const nothing = { tool: null, arguments: {} };
  return writeProvider({
    path: 'from-chat.txt',
    text: '来自聊天',
    finalText: PARAGRAPHS.join('\n\n'),
    planAction(request, action) {
      if (request.step > 0) return nothing;
      return userText(request).includes('写一个文件') ? action : nothing;
    },
  });
}

/** The Skill's example plugin, copied out with its Python helper as its README says. */
async function examplePlugin(root: string) {
  const folder = join(root, 'plugins', 'fake-chat');
  await cp(join(SKILL, 'examples/fake-chat/plugin'), folder, { recursive: true });
  await copyFile(join(SKILL, 'sdk/python/aura_plugin.py'), join(folder, 'service/aura_plugin.py'));
  return folder;
}

/** These tests' WebSocket adapter, written with the Skill's Node.js helper. */
async function socketPlugin(root: string) {
  const folder = join(root, 'plugins', 'ws-chat');
  await cp(join(import.meta.dirname, 'l17/ws-chat'), folder, { recursive: true });
  await copyFile(join(SKILL, 'sdk/node/aura-plugin.mjs'), join(folder, 'service/aura-plugin.mjs'));
  return folder;
}

const friend = { chat_type: 'private', chat_id: 'u1', user_id: 'u1', user_name: '老王' };
const buddy = { chat_type: 'private', chat_id: 'u2', user_id: 'u2', user_name: '小李' };

// eslint-disable-next-line no-empty-pattern -- real Electron/Core, two plugins in development with their real services, local fake platforms and isolated settings; scripted loopback model only.
test('L17 chats on two kinds of fake platform reach their conversations, get the replies and decide an approval', async ({}, info) => {
  test.setTimeout(300_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l17-channels-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const [chatsRoot, socketRoot] = [join(root, 'chats'), join(root, 'socket')];
  await mkdir(chatsRoot);
  await mkdir(socketRoot);
  const polling = await pollingPlatform();
  const socket = await socketPlatform();
  const model = await chatProvider();
  const { app, page } = await launch(root, { PYTHONDONTWRITEBYTECODE: '1' });
  try {
    await expectSecureSurface(page);
    await scriptedDefaults(page, model.url, 'read_only');
    const chats = await workspace(page, chatsRoot, 'Chats');
    const sockets = await workspace(page, socketRoot, 'Sockets');

    // Both added in development, reviewed and trusted on Settings → 插件; what only that page
    // changes is set there: the service on, the platform's address, the scope, the allowlist,
    // and for the example 远程批准.
    await addAndTrust(app, page, await examplePlugin(root), 'fake-chat');
    await addAndTrust(app, page, await socketPlugin(root), 'ws-chat');
    await configure(page, 'fake-chat', {
      service: true,
      text: { address: polling.address },
      scope: { workspaces: [chats], sessions: [] },
      allowed: [{ channel: 'fake-main', id: 'u1', name: '老王' }],
      remote_approval: true,
    });
    await configure(page, 'ws-chat', {
      service: true,
      text: { address: socket.address },
      scope: { workspaces: [sockets], sessions: [] },
      allowed: [{ channel: 'ws-main', id: 'u2', name: '小李' }],
    });

    // Long polling, through the Python helper: the chat's commands are Aura's.
    await polling.push({ ...friend, text: '/help', message_id: 'h-1' });
    const help = await polling.posted('/help answered', (sent) => sent.reply_to === 'h-1');
    expect(help.text).toContain('/approve 编号（可选） — 批准');

    // A message starts a turn in a new conversation of the scope. In 只读 its write asks first,
    // and the approval goes to the chat as text, since the platform has no buttons.
    await polling.push({ ...friend, text: '写一个文件', message_id: 'm-1' });
    const asked = await polling.posted('the approval handed to the chat', (sent) =>
      String(sent.text).startsWith('Aura 请你决定'),
    );
    expect(asked.chat).toEqual({ type: 'private', id: 'u1' });
    expect(asked.text).toContain('（编号 1）：写入文件');
    expect(asked.text).toContain('回复 /approve 批准这一次');
    await polling.push({ ...friend, text: '/approve', message_id: 'm-2' });
    const soon = await polling.posted('too soon', (sent) => sent.reply_to === 'm-2');
    expect(soon.text).toBe('审批交给你后 3 秒内的回复不算，请再发一次。');
    // The rule's own wait: a decision counts 3 seconds after the approval was handed.
    await page.waitForTimeout(3_200);
    await polling.push({ ...friend, text: '/approve 1', message_id: 'm-3' });
    const decided = await polling.posted('decided', (sent) => sent.reply_to === 'm-3');
    expect(decided.text).toBe('已批准（仅此一次）。');
    // The turn's answer, in the pieces the channel takes, in order.
    const pieces = async () => (await polling.sent()).filter((sent) => sent.reply_to === 'm-1');
    await expect.poll(async () => (await pieces()).length, { timeout: 90_000 }).toBe(3);
    expect(await pieces()).toEqual(
      PARAGRAPHS.map((text) => ({ chat: { type: 'private', id: 'u1' }, reply_to: 'm-1', text })),
    );
    expect(await readFile(join(chatsRoot, 'from-chat.txt'), 'utf8')).toBe('来自聊天');

    // The platform delivers m-1 again: the same message, no second turn. The adapter hands its
    // messages over in order, so h-2's answer comes after m-1's.
    const requests = model.requests.length;
    await polling.push({ ...friend, text: '写一个文件', message_id: 'm-1' });
    await polling.push({ ...friend, text: '/help', message_id: 'h-2' });
    await polling.posted('/help answered again', (sent) => sent.reply_to === 'h-2');
    expect(model.requests.length).toBe(requests);
    expect(await pieces()).toHaveLength(3);

    // A group: who may reach Aura is told by who wrote, not by the group; a conversation of its
    // own once someone allowed asks for one, named after the group; its answers go to the group.
    const group = { chat_type: 'group', chat_id: 'g1' };
    await polling.push({
      ...group,
      user_id: 'u5',
      user_name: '路人',
      text: '大家好',
      message_id: 'g-0',
    });
    await polling.push({
      ...group,
      user_id: 'u1',
      user_name: '老王',
      text: '/new',
      message_id: 'g-1',
    });
    const created = await polling.posted('/new in the group', (sent) => sent.reply_to === 'g-1');
    expect(created).toEqual({
      chat: { type: 'group', id: 'g1' },
      reply_to: 'g-1',
      text: '已新建会话 假聊天 · 群 g1，这个聊天已换过去。',
    });
    await polling.push({
      ...group,
      user_id: 'u1',
      user_name: '老王',
      text: '群里问一句',
      message_id: 'g-2',
    });
    const answeredInGroup = await polling.posted(
      'the group answered',
      (sent) => sent.reply_to === 'g-2',
    );
    expect(answeredInGroup).toEqual({
      chat: { type: 'group', id: 'g1' },
      reply_to: 'g-2',
      text: '模型生成正常结束。',
    });
    expect((await polling.sent()).filter((sent) => sent.reply_to === 'g-0')).toEqual([]);
    const unknown = await page.evaluate(async () => {
      const listed = await window.aura.v1.listPlugins({});
      if (!listed.ok) return JSON.stringify(listed);
      const plugin = listed.value.plugins.find((each) => each.name === 'fake-chat');
      return (plugin?.remote?.unknown ?? []).map(({ id, name, chat }) => ({ id, name, chat }));
    });
    expect(unknown).toEqual([{ id: 'u5', name: '路人', chat: 'group' }]);

    // A WebSocket that pushes, through the Node.js helper: its service says in its row on
    // Settings → 插件 that it is connected, the platform's command menu is Aura's for what the
    // plugin declared (session.send only: no conversation lists, switches or approvals), and a
    // message goes to another conversation, in its own plugin's scope.
    await expect(
      pluginRow(await pluginSettings(page), 'ws-chat').locator('.plugin-status-line'),
    ).toContainText('已连上假平台');
    await page.getByRole('button', { name: '返回会话' }).click();
    await expect
      .poll(async () => {
        const menu = (await socket.menus()).at(-1) as { commands?: { name: string }[] } | undefined;
        return menu?.commands?.map((command) => command.name).sort();
      })
      .toEqual(['goal', 'help', 'init', 'plan']);
    await socket.push({ ...buddy, text: '在吗', id: 'w-1' });
    const answered = await socket.posted(
      'the socket chat replied',
      (sent) => sent.reply_to === 'w-1',
    );
    expect(answered).toEqual({
      chat: { type: 'private', id: 'u2' },
      reply_to: 'w-1',
      content: '模型生成正常结束。',
    });
    const before = model.requests.length;
    await socket.push({ ...buddy, text: '在吗', id: 'w-1' });
    await socket.push({ ...buddy, text: '/help', id: 'w-2' });
    const listed = await socket.posted('/help on the socket', (sent) => sent.reply_to === 'w-2');
    expect(String(listed.content)).toContain('/help — 命令帮助');
    expect(String(listed.content)).not.toContain('/approve');
    expect(model.requests.length).toBe(before);

    // In Aura's window: each chat's conversation in its workspace, who wrote its message, and
    // who allowed the write. The sidebar lists the conversations of the workspaces opened in it.
    for (const name of ['Chats', 'Sockets']) {
      const open = page.getByRole('button', { name: `展开 ${name}`, exact: true });
      if (await open.isVisible()) await open.click();
    }
    const rows = page.locator('.session-row');
    await expect(rows.filter({ hasText: 'WS 聊天 · 小李' })).toHaveCount(1);
    await expect(rows.filter({ hasText: '假聊天 · 群 g1' })).toHaveCount(1);
    await rows.filter({ hasText: '假聊天 · 老王' }).click();
    await expect(page.getByText('来自插件 fake-chat · 其他平台 · 老王')).toBeVisible();
    // A finished turn's work is folded away, and so is each part of it; opened, the write's row
    // says who allowed it.
    const written = page.locator('.agent-turn').filter({ hasText: '写一个文件' });
    await written.locator('.agent-turn-toggle').click();
    for (const part of await written.locator('.agent-process-toggle[aria-expanded="false"]').all())
      await part.click();
    await expect(
      page.getByText('经插件 fake-chat 由 其他平台 用户 老王 批准（仅此一次）'),
    ).toBeVisible();
    expect(model.errors).toEqual([]);
  } finally {
    await app.close();
    await model.close();
    polling.close();
    socket.close();
  }
});
