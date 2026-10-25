import type { ExecutionApprovalView } from '@aura/contracts/generated';
import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, test } from 'vitest';
import {
  externalCapabilities,
  externalTargets,
  workspaceCapabilities,
  workspaceTargets,
} from './approval-facts';
import { ApprovalPanel } from './ApprovalPanel';

/**
 * The approval the panel reads, exactly as Core freezes it: the external object it
 * classified, the reach of the authorization, and the capabilities the operation asks for.
 */
function view(resources: unknown[], capabilities: string[]): ExecutionApprovalView {
  return {
    approval: {
      approval_id: 'apr_1',
      revision: '1',
      state: 'pending',
      requested_at: '2026-09-25T00:00:00Z',
      expires_at: '2026-09-25T00:05:00Z',
      allowed_decisions: ['allow_once', 'deny'],
      scope: {
        lifetime: { lifetime: 'once', operation_id: 'op_1' },
        profile: 'balanced',
        operation_fingerprint: 'hash',
        backend_requirement: { allowed_classes: ['core_broker'], guarantees: [] },
      },
    },
    operation: {
      content: {
        action_kind: 'fs.list',
        action: { path: 'notes' },
        capabilities,
        resources,
      },
    },
    backend: { id: 'core-broker', class: 'core_broker', guarantees: [] },
    reasons: [],
  } as unknown as ExecutionApprovalView;
}

beforeEach(() => {
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        getAgentApproval: async () => ({ ok: true, value: view([], []) }),
        resolveAgentApproval: async () => ({ ok: true, value: {} }),
      },
    },
  });
});
afterEach(cleanup);

/** The deadline reads as the user's local time; the exact instant stays on the element. */
function deadlineShown() {
  const time = document.querySelector('.agent-approval time');
  expect(time?.getAttribute('datetime')).toBe('2026-09-25T00:05:00Z');
  expect(time?.textContent).not.toContain('T00:05');
  expect(time?.parentElement?.textContent).toMatch(/^请在 .+ 前确认/u);
}

const directory = {
  scope_kind: 'external_path',
  canonical_path: { display: 'C:\\outside\\notes' },
  identity: null,
  directory: true,
  recursive: false,
};
const file = {
  scope_kind: 'external_path',
  canonical_path: { display: 'C:\\outside\\notes\\a.txt' },
  identity: null,
  recursive: false,
};
const sensitive = {
  scope_kind: 'sensitive_user_path',
  canonical_path: { display: 'C:\\Users\\review\\.ssh' },
};

test('an external target is reported with the reach Core decided', () => {
  const targets = externalTargets(view([directory], ['external_file_read']) as never);
  expect(targets).toEqual([
    {
      display: 'C:\\outside\\notes',
      scope: '该目录及直接子项（不含更深层内容）',
      kind: 'ordinary',
    },
  ]);
  expect(externalTargets(view([file], ['external_file_read']) as never)[0]?.scope).toBe(
    '仅该对象（不含其下内容）',
  );
  const deeper = externalTargets(
    view([{ ...directory, recursive: true }], ['external_file_read']) as never,
  );
  expect(deeper[0]?.scope).toBe('该目录及其下内容');
});

test('a protected location is named as such, not as an ordinary file', () => {
  const targets = externalTargets(view([sensitive], ['external_file_read']) as never);
  expect(targets).toEqual([
    { display: 'C:\\Users\\review\\.ssh', scope: '敏感数据位置', kind: 'sensitive' },
  ]);
});

test('only the external capabilities the operation asked for are named', () => {
  expect(
    externalCapabilities(
      view(
        [directory],
        ['external_file_read', 'external_file_write', 'workspace_file_read'],
      ) as never,
    ),
  ).toEqual(['读取外部文件', '写入外部文件']);
  expect(externalCapabilities(view([directory], ['workspace_file_read']) as never)).toEqual([]);
});

test('the panel shows the target, the capabilities and the lifetime without an editor', async () => {
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        getAgentApproval: async () => ({
          ok: true,
          value: view([directory], ['external_file_write']),
        }),
        resolveAgentApproval: async () => ({ ok: true, value: {} }),
      },
    },
  });
  render(<ApprovalPanel sessionId="session" approvalId="apr_1" onChanged={() => {}} />);
  expect(await screen.findByText(/C:\\outside\\notes/)).toBeTruthy();
  expect(screen.getByText(/该目录及直接子项（不含更深层内容）/)).toBeTruthy();
  expect(screen.getByText('写入外部文件')).toBeTruthy();
  deadlineShown();
  // Core decides the scope; the dialog reports it and offers no field to widen it.
  expect(screen.queryByRole('textbox')).toBeNull();
  expect(screen.queryByRole('spinbutton')).toBeNull();
});

const workspaceFile = {
  scope_kind: 'workspace_path',
  workspace_id: 'ws_1',
  canonical_relative_path: 'notes/denied.txt',
};

/**
 * A workspace write approval is the ordinary case the lower modes now reach.
 *
 * The confirmed semantics let a ReadOnly session approve writing one named target without
 * changing its mode, which means this dialog is no longer only ever about external objects.
 * The counter-example this guards is the one the change could have introduced: an approval
 * that offers `allow_session` while naming only an internal spelling the user cannot judge.
 */
test('a workspace approval names its target and capability in the user words', () => {
  expect(workspaceTargets(view([workspaceFile], ['workspace_file_write']) as never)).toEqual([
    { display: 'notes/denied.txt', scope: '工作区内该目标' },
  ]);
  expect(workspaceCapabilities(view([workspaceFile], ['workspace_file_write']) as never)).toEqual([
    '改写工作区文件',
  ]);
  // An external capability is not reported as a workspace one, and the root is the workspace.
  expect(workspaceCapabilities(view([directory], ['external_file_write']) as never)).toEqual([]);
  expect(
    workspaceTargets(
      view(
        [{ scope_kind: 'workspace_root', workspace_id: 'ws_1' }],
        ['workspace_file_read'],
      ) as never,
    ),
  ).toEqual([{ display: '工作区根目录', scope: '具体操作范围见本次参数' }]);
});

test('the panel renders a workspace approval as a real request, not a JSON blob', async () => {
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        getAgentApproval: async () => ({
          ok: true,
          value: view([workspaceFile], ['workspace_file_write']),
        }),
        resolveAgentApproval: async () => ({ ok: true, value: {} }),
      },
    },
  });
  render(<ApprovalPanel sessionId="session" approvalId="apr_1" onChanged={() => {}} />);
  expect(await screen.findByText(/notes\/denied\.txt/)).toBeTruthy();
  expect(screen.getByText(/工作区内该目标/)).toBeTruthy();
  expect(screen.getByText('改写工作区文件')).toBeTruthy();
  deadlineShown();
  expect(screen.getByText(/工作区目标/)).toBeTruthy();
  expect(screen.getByText('默认权限模式')).toBeTruthy();
});

test('S06 F1 file Session consent describes persistence and the full write scope', async () => {
  Object.assign(window.aura.v1, {
    getAgentApproval: async () => ({
      ok: true,
      value: view([workspaceFile], ['workspace_file_write']),
    }),
  });
  render(<ApprovalPanel sessionId="session" approvalId="apr_1" onChanged={() => {}} />);
  expect(await screen.findByText(/模式变化和 Core 重启后仍有效/)).toBeTruthy();
  expect(screen.getByText(/覆盖、删除和重命名/)).toBeTruthy();
  expect(screen.getByText(/目标对象被替换须重新批准/)).toBeTruthy();
  expect(
    screen.getByText(/在 Aura 编辑器中保存不会使批准失效；被其他程序替换时须重新批准/),
  ).toBeTruthy();
});

test('S07 a download asking only for network does not promise a file session answer', async () => {
  const value = view([workspaceFile], ['ordinary_network', 'workspace_file_create']);
  Object.assign(value, { requested_capabilities: ['ordinary_network'] });
  Object.assign(value.operation.content, {
    action_kind: 'http.download',
    action: { url: 'http://127.0.0.1:8080/file', path: 'notes/denied.txt' },
  });
  Object.assign(window.aura.v1, { getAgentApproval: async () => ({ ok: true, value }) });
  render(<ApprovalPanel sessionId="session" approvalId="apr_1" onChanged={() => {}} />);
  expect(await screen.findByText(/覆盖本会话普通网络工具的全部目标/)).toBeTruthy();
  expect(screen.queryByText(/模式变化和 Core 重启后仍有效/)).toBeNull();
  expect(screen.queryByText(/只覆盖这一类操作/)).toBeNull();
});

test('the model explains first; technical facts wait under 查看详情; the answers stay in view', async () => {
  const explained = view([], []) as unknown as {
    operation: { content: Record<string, unknown> };
  };
  explained.operation.content.purpose = '写入项目说明';
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        getAgentApproval: async () => ({ ok: true, value: explained }),
        resolveAgentApproval: async () => ({ ok: true, value: {} }),
      },
    },
  });
  render(<ApprovalPanel sessionId="session" approvalId="apr_1" onChanged={() => {}} />);
  const purpose = await screen.findByText('写入项目说明');
  expect(purpose.closest('.agent-approval-purpose')).toBeTruthy();
  const details = screen.getByText('默认权限模式').closest('details');
  expect(details?.querySelector('summary')?.textContent).toBe('查看详情');
  expect(details?.open).toBe(false);
  expect(details?.textContent).toContain('"path": "notes"');
  // Only the model's explanation is in view; the lifetime and what an answer covers are
  // under 查看详情 too, and the header names the action in words, not by its code.
  const body = document.querySelector('.agent-approval-body');
  expect([...(body?.children ?? [])].map((child) => child.tagName)).toEqual(['P', 'DETAILS']);
  expect(details?.contains(document.querySelector('.agent-approval time'))).toBe(true);
  expect(details?.contains(screen.getByText(/这类操作每次都会询问/u))).toBe(true);
  expect(document.querySelector('.agent-approval-kind')?.textContent).toBe('列出目录');
  const actions = document.querySelector('.agent-approval-actions');
  expect(actions?.contains(screen.getByRole('button', { name: '仅此一次' }))).toBe(true);
  expect(screen.getByRole('button', { name: '仅此一次' }).className).toContain('is-primary');
  expect(screen.getByRole('button', { name: '拒绝' }).className).toContain('is-danger');
});
