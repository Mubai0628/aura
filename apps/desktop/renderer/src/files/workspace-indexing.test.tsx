import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import { FileTreeIndexing, WorkspaceIndexBadge } from './workspace-indexing';

// A workspace whose index is still being built is marked in the sidebar, and the file tree says
// how far the build got; an index that did not cover the tree, or failed, says so.

const WORKSPACE = 'wsp_01ARZ3NDEKTSV4RRFFQ69G5FAV';

const projection = (indexing: object | undefined) => ({
  payload: {
    lifecycle_operations: [],
    workspaces: [
      {
        workspace_id: WORKSPACE,
        workspace_revision: '1',
        catalog: {
          freshness: 'rebuilding',
          catalog_revision: '0',
          indexed_at: null,
          watcher_state: 'watching',
          partitions: [],
          ...(indexing ? { indexing } : {}),
        },
        logical_files: [],
        editor_reservations: [],
        deferred_mutations: [],
      },
    ],
  },
  projection_kind: 'workspace_files',
  projection_scope: { type: 'global' },
  revision: '1',
  runtime_instance_id: 'rti_01ARZ3NDEKTSV4RRFFQ69G5FAV',
});

const bridge = (indexing: object | undefined) =>
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        getWorkspaceFiles: vi.fn().mockResolvedValue({ ok: true, value: projection(indexing) }),
        onWorkspaceFiles: vi.fn(() => () => undefined),
      },
    },
  });

const shown = () =>
  render(
    <>
      <WorkspaceIndexBadge workspaceId={WORKSPACE} />
      <FileTreeIndexing workspaceId={WORKSPACE} />
    </>,
  );

afterEach(cleanup);

test('a workspace being indexed is marked and its tree says how far the build got', async () => {
  bridge({ state: 'building', listed_entries: 1234, unindexed_directories: 0 });
  shown();
  expect(await screen.findByText('正在建立文件索引（已处理 1234 项）')).toBeTruthy();
  expect(screen.getByText('索引中')).toBeTruthy();
});

test('an index left incomplete for memory says why, what it means and what happens next', async () => {
  bridge({
    state: 'incomplete',
    listed_entries: 60000,
    unindexed_directories: 3,
    incomplete_reason: 'memory_low',
  });
  const rebuild = vi.fn().mockResolvedValue({
    ok: true,
    value: { workspace_id: WORKSPACE, started: true },
  });
  Object.assign(window.aura.v1, { rebuildWorkspaceIndex: rebuild });
  shown();
  const badge = await screen.findByText('索引不完整');
  expect(screen.queryByRole('tooltip')).toBeNull();
  fireEvent.mouseEnter(badge);
  const bubble = screen.getByRole('tooltip');
  expect(bubble.textContent).toContain('建索引时可用内存不足，3 个文件夹暂未建索引');
  expect(bubble.textContent).toContain('模型查找、搜索文件时直接读取这些文件夹，结果不受影响');
  expect(bubble.textContent).toContain('内存空闲后自动补建');
  fireEvent.mouseLeave(badge);
  expect(screen.queryByRole('tooltip')).toBeNull();
  expect(screen.getByText('3 个文件夹暂未建索引')).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: '立即补建' }));
  await vi.waitFor(() => expect(rebuild).toHaveBeenCalledWith({ workspace_id: WORKSPACE }));
  expect(screen.queryByText(/正在建立文件索引/)).toBeNull();
});

test('an index past its entry limit says so and offers no rebuild', async () => {
  bridge({
    state: 'incomplete',
    listed_entries: 200000,
    unindexed_directories: 12,
    incomplete_reason: 'entry_limit',
  });
  shown();
  fireEvent.mouseEnter(await screen.findByText('索引不完整'));
  expect(screen.getByRole('tooltip').textContent).toContain(
    '文件太多，超过索引上限，12 个文件夹未建索引',
  );
  expect(screen.queryByRole('button', { name: '立即补建' })).toBeNull();
});

test('a walk that fills in the left-out folders shows how far it got', async () => {
  bridge({
    state: 'incomplete',
    listed_entries: 4321,
    unindexed_directories: 3,
    incomplete_reason: 'memory_low',
    walking: true,
  });
  shown();
  expect(await screen.findByText('正在补建索引（已处理 4321 项）')).toBeTruthy();
  expect(screen.queryByRole('button', { name: '立即补建' })).toBeNull();
});

test('a failed build says why, and a finished one shows nothing', async () => {
  bridge({
    state: 'failed',
    listed_entries: 10,
    unindexed_directories: 0,
    failure: 'workspace root identity changed',
  });
  const first = shown();
  expect(await screen.findByText('文件索引没有建立：workspace root identity changed')).toBeTruthy();
  expect(screen.getByText('索引失败')).toBeTruthy();
  first.unmount();
  bridge(undefined);
  shown();
  await vi.waitFor(() => expect(screen.queryByText(/索引/)).toBeNull());
});

test('a failed build offers to walk the folder again', async () => {
  bridge({ state: 'failed', listed_entries: 0, unindexed_directories: 0, failure: 'denied' });
  const rebuild = vi.fn().mockResolvedValue({
    ok: true,
    value: { workspace_id: WORKSPACE, started: true },
  });
  Object.assign(window.aura.v1, { rebuildWorkspaceIndex: rebuild });
  shown();
  fireEvent.click(await screen.findByRole('button', { name: '重建索引' }));
  await vi.waitFor(() => expect(rebuild).toHaveBeenCalledWith({ workspace_id: WORKSPACE }));
});
