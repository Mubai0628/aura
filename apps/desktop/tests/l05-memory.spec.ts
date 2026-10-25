import { expect, test, type Page } from '@playwright/test';
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import type {
  MemoryDesktopSnapshot,
  MemoryMutationResult,
  MemoryMaintenanceRun,
} from '@aura/contracts/generated';
import { armReadOnlyWrite } from './s06/setup';
import { answer, changeMode, finished, launch, pending, submit, absent } from './s06/harness';
import {
  writeProvider,
  type ProviderAction,
  type ProviderRequest,
} from './s06-file-write-provider.mjs';
import { throughEntry } from './tool-entry';
import { expectSecureSurface } from './electron-shell/renderer-api';
function lastTool(request: ProviderRequest): Record<string, unknown> | undefined {
  const last = request.messages
    .filter((value) => (value as { role?: string }).role === 'tool')
    .at(-1) as { content: string } | undefined;
  return last ? (JSON.parse(last.content) as Record<string, unknown>) : undefined;
}
async function snapshot(page: Page, workspace_id: string): Promise<MemoryDesktopSnapshot> {
  return page.evaluate(async (workspace_id) => {
    const result = await window.aura.v1.memory.invoke('memory.vaults.get', { workspace_id });
    if (!result.ok) throw new Error(JSON.stringify(result.error));
    return result.value;
  }, workspace_id);
}
async function settledRun(
  page: Page,
  workspace: string,
  session: string,
  state: MemoryMaintenanceRun['state'],
) {
  await expect
    .poll(
      async () => {
        const view = await snapshot(page, workspace);
        return view.recent_runs.find((run) => run.session_id === session && run.state === state)
          ?.state;
      },
      { timeout: 60_000 },
    )
    .toBe(state);
}
// eslint-disable-next-line no-empty-pattern -- real Electron/Core and TEMP files; scripted loopback model only.
test('L05 Memory asks in ReadOnly, deletes once, consolidates work, and manages Vaults from settings', async ({}, info) => {
  test.setTimeout(300_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l05-memory-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const workspace = join(root, 'workspace');
  await mkdir(workspace);
  const phases = new Map<string, string>();
  let action: ProviderAction = {
    tool: 'memory.create',
    arguments: {
      scope: 'project',
      items: [
        {
          path: 'rules/owner.md',
          category: 'rules',
          body: '# 项目构建约定\n使用本机临时夹具。',
          tags: [],
          confidence: 1,
          importance: 3,
          recall_weight: 1,
          related_ids: [],
          supersedes_ids: [],
          conflicts_with_ids: [],
        },
      ],
    },
  };
  let receipt: MemoryMutationResult | undefined;
  const model = await writeProvider({
    path: 'unused',
    text: '',
    maintenanceOutput: {
      schema_version: 1,
      candidates: [
        {
          action: 'create',
          scope: 'project',
          path: 'experience/automatic.md',
          content: {
            category: 'experience',
            body: '# 已验证的自动整理\n本机文件写入已完成。',
            basis: '本轮 fs.write 成功回执',
            tags: [],
            confidence: 1,
            importance: 3,
          },
        },
      ],
      events: [{ title: '完成临时工作', summary: '已写入结果文件', basis: '真实工具回执' }],
    },
    planAction(request) {
      const phase = phases.get(request.turn);
      if (phase === 'called') {
        if (action.tool === 'memory.create')
          receipt = (lastTool(request)?.outcome as { payload?: { value: MemoryMutationResult } })
            ?.payload?.value;
        return { tool: null, arguments: {} };
      }
      if (
        request.tools.some((tool) =>
          (tool as { function: { name: string } }).function.name.startsWith(
            `${action.tool!.replaceAll('.', '_')}__`,
          ),
        )
      ) {
        phases.set(request.turn, 'called');
        return action;
      }
      if (phase === 'searched') {
        const hits = (
          lastTool(request)?.outcome as {
            payload?: { value: { results: Array<{ canonical_name: string; tool_id: string }> } };
          }
        )?.payload?.value.results;
        const hit = hits?.find((hit) => hit.canonical_name === action.tool);
        if (!hit) throw new Error('Memory delete must be discoverable');
        phases.set(request.turn, 'loaded');
        return { tool: 'tools.load', arguments: { tool_ids: [hit.tool_id], replace: [] } };
      }
      if (phase === 'loaded') {
        phases.set(request.turn, 'called');
        return throughEntry(action);
      }
      phases.set(request.turn, 'searched');
      return { tool: 'tools.search', arguments: { query: action.tool, limit: 50, cursor: null } };
    },
  });
  const current = await launch(root);
  try {
    const { app, page } = current;
    await expectSecureSurface(page);
    const setup = await armReadOnlyWrite(page, workspace, model.url, {
      title: 'L05 Memory',
      text: '记下项目构建约定',
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    const workspaceId = setup.workspaceId as string;
    const question = await pending(page, session);
    expect(question.operation.content.action_kind).toBe('memory.create');
    expect(question.approval.allowed_decisions).toContain('allow_once');
    expect(await absent(join(workspace, '.memories'))).toBe(true);
    await answer(page, session, 'allow_once');
    await finished(page, session);
    await settledRun(page, workspaceId, session, 'skipped');
    expect(await readFile(join(workspace, '.memories/rules/owner.md'), 'utf8')).toContain(
      '项目构建约定',
    );
    expect(model.maintenanceRequests).toHaveLength(0);
    expect(receipt?.entries[0]?.hash).toBeTruthy();
    await changeMode(page, session, 'workspace_read_write');
    action = {
      tool: 'fs.write',
      arguments: {
        path: 'result.txt',
        text: 'actual local work',
        expected: { expected_kind: 'absent' },
      },
    };
    await submit(page, session, '执行实际临时工作');
    await finished(page, session);
    await settledRun(page, workspaceId, session, 'completed');
    expect(model.maintenanceRequests).toHaveLength(1);
    expect(await readFile(join(workspace, '.memories/experience/automatic.md'), 'utf8')).toContain(
      '已验证的自动整理',
    );
    expect(await readFile(join(workspace, '.memories/events/recent.md'), 'utf8')).toContain(
      '完成临时工作',
    );
    action = {
      tool: 'memory.delete',
      arguments: {
        scope: 'project',
        items: [
          {
            target: { kind: 'path', path: 'rules/owner.md' },
            expected_hash: receipt!.entries[0]!.hash,
          },
        ],
      },
    };
    await submit(page, session, '删除先前项目约定');
    const deletion = await pending(page, session);
    expect(deletion.operation.content.action_kind).toBe('memory.delete');
    expect(deletion.approval.allowed_decisions).toEqual(['allow_once', 'deny']);
    await answer(page, session, 'allow_once');
    await finished(page, session);
    expect(await absent(join(workspace, '.memories/rules/owner.md'))).toBe(true);
    // Select the real workspace in navigation before opening its settings.
    await page.getByRole('button', { name: '设置', exact: true }).click();
    await page
      .locator('.settings-navigation')
      .getByRole('button', { name: '记忆', exact: true })
      .click();
    const memory = page.getByRole('region', { name: '记忆设置' });
    const project = memory.getByRole('region', { name: '项目记忆', exact: true });
    await expect(project).toBeVisible();
    await expect(project).toContainText(workspace);
    await expect(memory).toContainText('只读会话未整理');
    const toggle = memory.getByRole('switch', { name: '任务完成后自动整理记忆' });
    await toggle.uncheck();
    await memory.getByRole('button', { name: '保存记忆设置' }).click();
    await expect(memory).toContainText('记忆设置已保存');
    await writeFile(join(workspace, '.memories/rules/bad.md'), '# invalid manual note\n');
    await project.getByRole('button', { name: '校验', exact: true }).click();
    await expect(project).toContainText('rules/bad.md');
    const before = await readFile(join(workspace, '.memories/rules/bad.md'), 'utf8');
    await project.getByRole('button', { name: '预览修复', exact: true }).click();
    await expect(project.getByRole('button', { name: '应用此预览' })).toBeVisible();
    expect(await readFile(join(workspace, '.memories/rules/bad.md'), 'utf8')).toBe(before);
    await project.getByRole('button', { name: '应用此预览' }).click();
    await expect(memory).toContainText('修复已应用');
    await project.getByRole('button', { name: '重建索引', exact: true }).click();
    await expect(memory).toContainText('索引已从文件重建');
    await app.evaluate(
      ({ dialog, shell }, destination) => {
        Reflect.set(dialog, 'showSaveDialog', async () => ({
          canceled: false,
          filePath: destination,
        }));
        Reflect.set(shell, 'openPath', async () => '');
      },
      join(root, 'vault.zip'),
    );
    await project.getByRole('button', { name: '打开目录', exact: true }).click();
    await expect(project).toContainText('已打开 Vault 目录');
    await project.getByRole('button', { name: '导出 ZIP', exact: true }).click();
    await expect(project).toContainText('Vault 已完整导出为 ZIP');
    expect((await readFile(join(root, 'vault.zip'))).readUInt32LE(0)).toBe(0x04034b50);
    expect(model.errors).toEqual([]);
    const screenshot = info.outputPath('memory-settings.png');
    await page.screenshot({ path: screenshot, fullPage: true });
    await info.attach('memory-settings-ui', { path: screenshot, contentType: 'image/png' });
    await info.attach('memory-desktop-final', {
      body: JSON.stringify(await snapshot(page, workspaceId), null, 2),
      contentType: 'application/json',
    });
  } finally {
    await current.app.close();
    await model.close();
  }
});
