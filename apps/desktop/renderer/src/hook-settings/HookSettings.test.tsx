import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type {
  HookDefinition,
  HookEntryView,
  HookInvocationView,
  HookPreview,
} from '@aura/contracts/generated';
import { HookSettings } from './HookSettings';

const definition = (name: string, extra: Partial<HookDefinition> = {}): HookDefinition => ({
  name,
  version: '1.0.0',
  description: `${name} 说明`,
  kind: 'user',
  failure_policy: 'fail_open',
  timeout_ms: 10_000,
  events: ['tool.before_call'],
  command: { type: 'executable', path: 'bin/hook.exe', arguments: [] },
  tools: ['fs.write'],
  ...extra,
});
const entry = (name: string, extra: Partial<HookEntryView> = {}): HookEntryView => ({
  path: `C:\\hooks\\${name}`,
  enabled: true,
  state: 'ready',
  definition: definition(name),
  fingerprint: `${name}-fingerprint-0123456789`,
  trusted_fingerprint: `${name}-fingerprint-0123456789`,
  problems: [],
  ...extra,
});
const registry = (entries: HookEntryView[], revision = 3) => ({
  ok: true,
  value: { revision, entries },
});
const preview = (name: string, extra: Partial<HookPreview> = {}): HookPreview => ({
  path: `C:\\hooks\\${name}`,
  definition: definition(name),
  files: [
    { path: 'hook.toml', bytes: 210 },
    { path: 'bin/hook.exe', bytes: 52_000 },
  ],
  fingerprint: `${name}-new-fingerprint-abcdef`,
  problems: [],
  registered: false,
  ...extra,
});

const api = {
  listHooks: vi.fn(),
  previewHook: vi.fn(),
  trustHook: vi.fn(),
  untrustHook: vi.fn(),
  setHookEnabled: vi.fn(),
  removeHook: vi.fn(),
  moveHook: vi.fn(),
  listHookInvocations: vi.fn(),
  selectHookDirectory: vi.fn(),
};
beforeEach(() => {
  for (const mock of Object.values(api)) mock.mockReset();
  api.listHooks.mockResolvedValue(
    registry([
      entry('guard'),
      entry('notes', { enabled: false, state: 'disabled' }),
      entry('changed', { state: 'content_changed', fingerprint: 'other-fingerprint' }),
    ]),
  );
  Object.defineProperty(window, 'aura', { configurable: true, value: { v1: api } });
});
afterEach(cleanup);

const rowOf = (name: string) =>
  screen.getByText(name, { selector: 'strong' }).closest('li') as HTMLElement;

test('lists Hooks in execution order with their state; the identity warning waits for trust', async () => {
  render(<HookSettings coreReady />);
  expect(await screen.findByText('guard', { selector: 'strong' })).toBeTruthy();
  const order = within(screen.getByRole('list', { name: 'Hook 执行顺序' }))
    .getAllByRole('listitem')
    .map((item) => within(item).getByText(/./u, { selector: 'strong' }).textContent);
  expect(order).toEqual(['guard', 'notes', 'changed']);
  expect(within(rowOf('guard')).getByText('已启用')).toBeTruthy();
  expect(within(rowOf('notes')).getByText('已停用')).toBeTruthy();
  expect(within(rowOf('changed')).getByText('内容已变化，需要重新信任')).toBeTruthy();
  expect(within(rowOf('guard')).getByText(/工具调用前；只对这些工具：fs\.write/u)).toBeTruthy();
  // The page has no warning; the review before trusting carries one sentence.
  expect(screen.queryAllByRole('note')).toHaveLength(0);
  // A changed Hook offers re-trust, never an on/off switch that would pretend it runs.
  expect(within(rowOf('changed')).queryByRole('button', { name: '启用' })).toBeNull();
  expect(within(rowOf('changed')).getByRole('button', { name: '查看变化并重新信任' })).toBeTruthy();
  expect(screen.getByRole('button', { name: '上移 guard' })).toHaveProperty('disabled', true);
  expect(screen.getByRole('button', { name: '下移 changed' })).toHaveProperty('disabled', true);
});

test('adding a Hook previews the chosen folder and trusts exactly the shown fingerprint', async () => {
  const user = userEvent.setup();
  api.selectHookDirectory.mockResolvedValue({ ok: true, value: { path: 'C:\\hooks\\fresh' } });
  api.previewHook.mockResolvedValue({ ok: true, value: preview('fresh') });
  api.trustHook.mockResolvedValue(registry([entry('guard'), entry('fresh')], 4));
  render(<HookSettings coreReady />);
  await screen.findByText('guard', { selector: 'strong' });
  await user.click(screen.getByRole('button', { name: '添加 Hook…' }));
  const panel = await screen.findByRole('region', { name: '预览 Hook fresh' });
  expect(api.previewHook).toHaveBeenCalledWith({ path: 'C:\\hooks\\fresh' });
  expect(within(panel).getByText('运行包内程序 bin/hook.exe')).toBeTruthy();
  expect(within(panel).getByText('用户 Hook · 失败时放行')).toBeTruthy();
  expect(within(panel).getByText('包内文件（2）')).toBeTruthy();
  await user.click(within(panel).getByRole('button', { name: '信任并启用' }));
  expect(api.trustHook).toHaveBeenCalledWith({
    path: 'C:\\hooks\\fresh',
    expected_fingerprint: 'fresh-new-fingerprint-abcdef',
    enabled: true,
    expected_revision: 3,
  });
  expect(await screen.findByText('已信任并启用 fresh')).toBeTruthy();
  expect(screen.queryByRole('region', { name: '预览 Hook fresh' })).toBeNull();
  expect(screen.getByText('fresh', { selector: 'strong' })).toBeTruthy();
});

test('a cancelled folder dialog changes nothing, and a broken package cannot be trusted', async () => {
  const user = userEvent.setup();
  api.selectHookDirectory.mockResolvedValueOnce({ ok: true, value: { path: null } });
  render(<HookSettings coreReady />);
  await screen.findByText('guard', { selector: 'strong' });
  await user.click(screen.getByRole('button', { name: '添加 Hook…' }));
  expect(api.previewHook).not.toHaveBeenCalled();
  api.selectHookDirectory.mockResolvedValueOnce({ ok: true, value: { path: 'C:\\hooks\\bad' } });
  api.previewHook.mockResolvedValueOnce({
    ok: true,
    value: preview('bad', {
      definition: null,
      fingerprint: null,
      files: [],
      problems: ['缺少 hook.toml'],
    }),
  });
  await user.click(screen.getByRole('button', { name: '添加 Hook…' }));
  const panel = await screen.findByRole('region', { name: '预览 Hook bad' });
  expect(within(panel).getByRole('alert').textContent).toContain('缺少 hook.toml');
  expect(within(panel).queryByRole('button', { name: '信任并启用' })).toBeNull();
  expect(api.trustHook).not.toHaveBeenCalled();
});

test('re-trusting a changed Hook keeps its on/off choice and a stale revision reloads', async () => {
  const user = userEvent.setup();
  api.listHooks.mockResolvedValue(
    registry([entry('changed', { enabled: false, state: 'content_changed' })]),
  );
  api.previewHook.mockResolvedValue({
    ok: true,
    value: preview('changed', { registered: true }),
  });
  api.trustHook.mockResolvedValue({
    ok: false,
    error: { message: 'hooks.revision_changed: Hook 列表已被修改，请刷新后再操作' },
  });
  render(<HookSettings coreReady />);
  await user.click(await screen.findByRole('button', { name: '查看变化并重新信任' }));
  const panel = await screen.findByRole('region', { name: '预览 Hook changed' });
  await user.click(within(panel).getByRole('button', { name: '重新信任' }));
  expect(api.trustHook).toHaveBeenCalledWith(expect.objectContaining({ enabled: false }));
  expect((await screen.findByRole('alert')).textContent).toBe('Hook 列表已被修改，请刷新后再操作');
  await waitFor(() => expect(api.listHooks).toHaveBeenCalledTimes(2));
});

test('switching, untrusting, moving and removing name the revision shown', async () => {
  const user = userEvent.setup();
  const after = registry([entry('guard')], 4);
  for (const mock of [api.setHookEnabled, api.untrustHook, api.moveHook, api.removeHook])
    mock.mockResolvedValue(after);
  render(<HookSettings coreReady />);
  await screen.findByText('guard', { selector: 'strong' });
  await user.click(within(rowOf('notes')).getByRole('button', { name: '启用' }));
  expect(api.setHookEnabled).toHaveBeenCalledWith({
    path: 'C:\\hooks\\notes',
    enabled: true,
    expected_revision: 3,
  });
  await screen.findByText('已启用 notes');
  await user.click(within(rowOf('guard')).getByRole('button', { name: '撤销信任' }));
  expect(api.untrustHook).toHaveBeenCalledWith({ path: 'C:\\hooks\\guard', expected_revision: 4 });
  api.listHooks.mockResolvedValue(registry([entry('guard'), entry('notes')], 5));
  await user.click(screen.getByRole('button', { name: '刷新' }));
  await screen.findByText('notes', { selector: 'strong' });
  await user.click(screen.getByRole('button', { name: '下移 guard' }));
  expect(api.moveHook).toHaveBeenCalledWith({
    path: 'C:\\hooks\\guard',
    direction: 'down',
    expected_revision: 5,
  });
  await user.click(within(rowOf('guard')).getByRole('button', { name: '移除' }));
  const confirm = screen.getByRole('alertdialog', { name: '移除 guard' });
  expect(confirm.textContent).toContain('不会删除它的文件夹');
  await user.click(within(confirm).getByRole('button', { name: '移除' }));
  expect(api.removeHook).toHaveBeenCalledWith({ path: 'C:\\hooks\\guard', expected_revision: 4 });
  expect(await screen.findByText('已从列表移除 guard，文件夹未删除')).toBeTruthy();
});

test('recent calls show outcome, reason and stderr for one Hook', async () => {
  const user = userEvent.setup();
  const call = (extra: Partial<HookInvocationView>): HookInvocationView => ({
    invocation_id: 'hki_1',
    path: 'C:\\hooks\\guard',
    hook_name: 'guard',
    hook_version: '1.0.0',
    fingerprint: 'guard-fingerprint-0123456789',
    event: 'tool.before_call',
    started_at: '2026-10-17T08:00:00Z',
    duration_ms: 42,
    outcome: 'blocked',
    effect: 'action_blocked',
    reason: '不许写 .env',
    ...extra,
  });
  api.listHookInvocations.mockResolvedValue({
    ok: true,
    value: {
      invocations: [
        call({}),
        call({
          invocation_id: 'hki_2',
          outcome: 'failed',
          failure: 'timed_out',
          effect: 'none',
          reason: '超过 10 秒',
          stderr: 'still waiting',
        }),
      ],
      statistics: null,
    },
  });
  render(<HookSettings coreReady />);
  await screen.findByText('guard', { selector: 'strong' });
  await user.click(within(rowOf('guard')).getByRole('button', { name: '最近调用' }));
  const calls = await screen.findByRole('region', { name: 'guard 的最近调用' });
  expect(api.listHookInvocations).toHaveBeenCalledWith({
    path: 'C:\\hooks\\guard',
    turn_task_id: null,
    limit: 20,
  });
  expect(await within(calls).findByText('原因：不许写 .env')).toBeTruthy();
  expect(within(calls).getByText('动作已被阻止')).toBeTruthy();
  expect(within(calls).getByText('失败（超时）')).toBeTruthy();
  expect(within(calls).getByText('still waiting')).toBeTruthy();
});

test('nothing is requested before Core is ready', () => {
  render(<HookSettings coreReady={false} />);
  expect(screen.getByRole('status').textContent).toContain('Core 尚未就绪');
  expect(api.listHooks).not.toHaveBeenCalled();
  expect(screen.getByRole('button', { name: '添加 Hook…' })).toHaveProperty('disabled', true);
});

test('a plugin’s Hook is listed after the user’s own and is managed with its plugin', async () => {
  api.listHooks.mockResolvedValue(
    registry([
      entry('guard'),
      entry('checker', { path: 'C:\\Aura\\plugins\\packages\\kit\\hooks\\checker', plugin: 'kit' }),
    ]),
  );
  render(<HookSettings coreReady />);
  await screen.findByText('checker', { selector: 'strong' });
  const row = rowOf('checker');
  expect(within(row).getByText('插件 kit')).toBeTruthy();
  for (const name of ['预览', '停用', '撤销信任', '上移 checker', '下移 checker', '移除'])
    expect(within(row).queryByRole('button', { name })).toBeNull();
  expect(within(row).getByText(/在你自己的 Hook 之后运行/u)).toBeTruthy();
  // The user's own Hook cannot be moved below a plugin's.
  expect(
    (within(rowOf('guard')).getByRole('button', { name: '下移 guard' }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
});

// A listed Hook's preview opens in its own row and the button that opened it
// closes it; only a folder being added, which has no row yet, keeps a cancel button.
test('a listed Hook’s preview opens in its row and the same button closes it', async () => {
  const user = userEvent.setup();
  api.previewHook.mockResolvedValue({
    ok: true,
    value: preview('guard', { registered: true }),
  });
  render(<HookSettings coreReady />);
  await screen.findByText('guard', { selector: 'strong' });
  await user.click(within(rowOf('guard')).getByRole('button', { name: '预览' }));
  const panel = await within(rowOf('guard')).findByRole('region', { name: '预览 Hook guard' });
  expect(within(panel).queryByRole('button', { name: /^(关闭|取消)$/u })).toBeNull();
  await user.click(within(rowOf('guard')).getByRole('button', { name: '收起' }));
  expect(screen.queryByRole('region', { name: '预览 Hook guard' })).toBeNull();
  expect(within(rowOf('guard')).getByRole('button', { name: '预览' })).toBeTruthy();
});
