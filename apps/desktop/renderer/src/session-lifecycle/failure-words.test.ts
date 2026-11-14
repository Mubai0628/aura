import { expect, test } from 'vitest';
import type { WorkspaceLifecycleSummary } from '@aura/contracts/generated';

import {
  addWorkspaceFailure,
  archiveFailure,
  BRANCH_MODEL_NOT_KEPT,
  deleteFailure,
  lifecycleNotCompleted,
  renameFailure,
  restoreFailure,
} from './failure-words';

// A sidebar or archive action that did not happen says why, that what it acted on is as it was,
// and what to do; one Core did not answer says it may have happened.

const TIMEOUT = { code: 'ipc.request_timeout', message: 'Core request timed out' };
const LOST = { code: 'ipc.transport_lost', message: 'Transport is closed' };

test('a rename says when the name changed elsewhere, or may have been done', () => {
  expect(renameFailure({ code: 'session.title_conflict', message: 'session.title_conflict' })).toBe(
    '会话名称刚被改过，这次没有改名；请关闭后重新命名。',
  );
  expect(renameFailure(TIMEOUT)).toBe(
    'Aura Core 没有及时回复，名称可能已经改好；请看侧栏里的名称。',
  );
  expect(renameFailure({ code: 'x', message: 'y' })).toBe(
    '没有改名，会话名称保持原样；请再试一次。',
  );
});

test("an archive says Core's reason, or that it may have been done", () => {
  expect(
    archiveFailure(
      {
        code: 'agent.command_failed',
        message: 'session.busy: 这个会话还有任务在运行，请先停止当前任务。',
      },
      '会话',
    ),
  ).toBe('没有归档：这个会话还有任务在运行，请先停止当前任务。');
  expect(archiveFailure(LOST, '会话')).toBe(
    '没有等到 Aura Core 的答复（与 Aura Core 的连接断开了），会话可能已经归档；请看侧栏，它还在就再归档一次。',
  );
  expect(archiveFailure(new Error('boom'), '工作区')).toBe(
    '没有归档，工作区保持原样；请再试一次。',
  );
});

test('a folder that cannot be added says why and what to choose instead', () => {
  expect(
    addWorkspaceFailure({
      code: 'workspace.invalid_root',
      message: 'workspace root must be an existing directory',
    }),
  ).toBe('这个位置不是一个现有的文件夹，没有添加工作区；请重新选择。');
  expect(addWorkspaceFailure({ code: 'workspace.root_conflict', message: 'x' })).toBe(
    '这个文件夹与已有的工作区重叠，没有添加；请选择另一个文件夹，或直接打开已有的工作区。',
  );
  expect(addWorkspaceFailure({ code: 'workspace.path_resolution_failed', message: 'x' })).toBe(
    '打不开这个文件夹（可能已被移动、没有访问权限或所在磁盘未连接），没有添加工作区；请确认它能访问后重新选择。',
  );
  expect(addWorkspaceFailure(TIMEOUT)).toBe(
    'Aura Core 没有及时回复，工作区可能已经添加；请看侧栏。',
  );
});

const summary = (over: Partial<WorkspaceLifecycleSummary>): WorkspaceLifecycleSummary =>
  ({
    operation_id: 'wlo_1',
    workspace_id: 'wsp_1',
    renderer_instance_id: 'rnd_1',
    operation: 'archive',
    lifecycle_revision: '2',
    state: 'blocked',
    ...over,
  }) as WorkspaceLifecycleSummary;

test('a Workspace operation stopped by work in it says what still runs and that it is as it was', () => {
  expect(
    lifecycleNotCompleted(
      summary({
        preflight: {
          blockers: [
            { blocker_kind: 'runtime_objects_active', count: 2 },
            { blocker_kind: 'runtime_leases_active', count: 1 },
            { blocker_kind: 'mutation_ownership_active', count: 1 },
          ],
        } as never,
      }),
      '归档',
    ),
  ).toBe(
    '没有归档：工作区里还有 3 个没结束的任务或进程；模型正在改这个工作区里的文件。工作区保持原样；等这些结束或先停止后再归档。',
  );
  expect(
    lifecycleNotCompleted(
      summary({ state: 'failed', error_code: 'workspace.editor_prepare_ack_timeout' }),
      '删除',
    ),
  ).toBe('没有删除：这个窗口没有及时确认，工作区保持原样，请再试一次。');
  expect(lifecycleNotCompleted(summary({ state: 'cancelled' }), '归档')).toBe(
    '没有归档，工作区保持原样；请再试一次。',
  );
});

test('a delete Core did not answer may still run; a restore that failed is still archived', () => {
  expect(deleteFailure(TIMEOUT)).toBe(
    '没有等到 Aura Core 的答复（Aura Core 没有及时回复），删除可能仍在进行；稍后看它是否还在列表里，还在就再删一次。',
  );
  expect(
    deleteFailure({
      code: 'agent.command_failed',
      message: 'session.not_archived: 请先归档这个会话，再到“设置 → 归档”里删除。',
    }),
  ).toBe('没有删除：请先归档这个会话，再到“设置 → 归档”里删除。');
  expect(deleteFailure({ code: 'x', message: 'y' })).toBe('没有删除，它保持原样，可以再试一次。');
  expect(restoreFailure({ code: 'workspace.revision_conflict', message: 'x' })).toBe(
    '没有恢复（它刚在别处有了变化），它仍在归档里，请再点一次“恢复”。',
  );
  expect(restoreFailure(TIMEOUT)).toBe(
    '没有等到 Aura Core 的答复（Aura Core 没有及时回复），可能已经恢复；它还在归档列表里就再点一次“恢复”。',
  );
  expect(restoreFailure({ code: 'x', message: 'y' })).toBe(
    '没有恢复，它仍在归档里，请再点一次“恢复”。',
  );
  expect(BRANCH_MODEL_NOT_KEPT).toBe(
    '分支已建好，但没能沿用原会话的模型，它现在使用默认模型；需要时在输入框下方重新选择。',
  );
});
