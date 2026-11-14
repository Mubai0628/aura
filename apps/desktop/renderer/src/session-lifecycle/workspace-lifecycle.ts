import type { WorkspaceLifecycleSummary } from '@aura/contracts/generated';
import { LifecycleRefused, lifecycleNotCompleted } from './failure-words';

/**
 * Archiving, moving or removing a Workspace runs Main's lifecycle barrier, which asks this window
 * for its unsaved editor buffers before Core commits. Aura's editor keeps none, so every
 * preparation is answered at once with none. The answer is kept for whoever started the
 * operation: it can arrive before the call that started it returns.
 */
type Outcome = { summary: WorkspaceLifecycleSummary } | { error: unknown };

const outcomes = new Map<string, Outcome>();
const waiters = new Map<string, (outcome: Outcome) => void>();

const settle = (operationId: string, outcome: Outcome) => {
  const waiter = waiters.get(operationId);
  if (waiter === undefined) outcomes.set(operationId, outcome);
  else {
    waiters.delete(operationId);
    waiter(outcome);
  }
};

/** Answers every lifecycle preparation of this window. Returns the unsubscribe function. */
export function answerWorkspaceLifecycle(): () => void {
  // A window without the bridge (a Renderer test that does not need it) has nothing to answer.
  if (typeof window.aura?.v1?.onWorkspaceLifecyclePrepare !== 'function') return () => undefined;
  return window.aura.v1.onWorkspaceLifecyclePrepare((prepare) => {
    void window.aura.v1
      .acknowledgeWorkspaceLifecycle({
        buffers: [],
        lifecycle_revision: prepare.lifecycle_revision,
        operation_id: prepare.operation_id,
        workspace_id: prepare.workspace_id,
      })
      .then(
        (result) =>
          settle(
            prepare.operation_id,
            result.ok ? { summary: result.value } : { error: result.error },
          ),
        (error: unknown) => settle(prepare.operation_id, { error }),
      );
  });
}

/** The end of a lifecycle operation this window started. */
export function lifecycleOutcome(
  operationId: string,
  timeoutMs = 60_000,
): Promise<WorkspaceLifecycleSummary> {
  return new Promise((resolve, reject) => {
    const deliver = (outcome: Outcome) => {
      window.clearTimeout(timer);
      if ('summary' in outcome) resolve(outcome.summary);
      else reject(outcome.error);
    };
    const timer = window.setTimeout(() => {
      waiters.delete(operationId);
      reject(
        new Error(
          '工作区操作一分钟内没有结束，可能仍在进行；请稍后看工作区的状态，再决定是否重试。',
        ),
      );
    }, timeoutMs);
    const early = outcomes.get(operationId);
    if (early !== undefined) {
      outcomes.delete(operationId);
      deliver(early);
    } else waiters.set(operationId, deliver);
  });
}

const completed = async (begun: Promise<unknown>) => {
  const result = (await begun) as
    { ok: true; value: { operation_id: string } } | { ok: false; error: unknown };
  if (!result.ok) throw result.error;
  const summary = await lifecycleOutcome(result.value.operation_id);
  if (summary.state === 'completed') return;
  // A blocked operation holds the Workspace until it is resolved: it is withdrawn, so the
  // Workspace can be acted on again once what blocked it has ended.
  if (summary.state === 'blocked') await withdrawLifecycle(summary);
  throw new LifecycleRefused(
    lifecycleNotCompleted(summary, summary.operation === 'permanent_delete' ? '删除' : '归档'),
  );
};

/** Gives up an operation stopped by work in the Workspace, leaving the Workspace as it was. */
export async function withdrawLifecycle(summary: WorkspaceLifecycleSummary): Promise<void> {
  await window.aura.v1
    .resolveWorkspaceLifecycle({
      operation_id: summary.operation_id,
      expected_lifecycle_revision: summary.lifecycle_revision,
      action: 'return_to_editing',
    })
    .catch(() => undefined);
}

/** Archives a Workspace: it leaves the sidebar and can be restored in 设置 → 归档. */
export async function archiveWorkspace(workspaceId: string, revision: string): Promise<void> {
  await completed(
    window.aura.v1.archiveWorkspace({
      workspace_id: workspaceId,
      expected_workspace_revision: revision,
    }),
  );
}

/** Archives a Workspace at its current revision. */
export async function archiveWorkspaceById(workspaceId: string): Promise<void> {
  const current = await currentWorkspace(workspaceId);
  if (current.state !== 'archived') await archiveWorkspace(workspaceId, current.revision);
}

/**
 * Deletes what Aura kept for an archived Workspace. Only 设置 → 归档 offers it, and it never archives on the
 * way; Core refuses a Workspace that is not archived.
 */
export async function deleteArchivedWorkspace(workspaceId: string): Promise<void> {
  await completed(window.aura.v1.deleteWorkspace({ workspace_id: workspaceId }));
}

/** The Workspace as Core has it now, so a lifecycle call names its current revision. */
export async function currentWorkspace(workspaceId: string) {
  const projection = await window.aura.v1.getWorkspaceSessions();
  if (!projection.ok) throw projection.error;
  const workspace = projection.value.payload.workspaces.find(
    (candidate) => candidate.workspace_id === workspaceId,
  );
  if (workspace === undefined) throw new Error('这个工作区已不存在。');
  return workspace;
}
