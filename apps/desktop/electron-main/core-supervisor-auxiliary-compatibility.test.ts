// @vitest-environment node
import { expect, test, vi } from 'vitest';
import { appendFile } from 'node:fs/promises';
import { fixture } from './core-supervisor-tests/fixture.js';
import { registerTerminalOutput } from './terminal-output.js';

vi.mock('node:child_process', () => ({ spawn: vi.fn() }));
vi.mock('node:fs/promises', () => ({ mkdir: vi.fn(), appendFile: vi.fn() }));
vi.mock('./core-transport.js', () => ({ CoreTransport: { connect: vi.fn() } }));

const session = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const other = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAW';
const securityRuntime = {
  sandbox_available: false,
  sandbox_host_state: 'not_started',
  egress_proxy_state: 'not_started',
  elevated_helper_state: 'not_started',
  quarantine_count: 0,
  host_preparation: {
    appcontainer_available: false,
    nested_jobs_available: false,
    standard_user_token_available: false,
    system_drive_compatibility_ready: false,
    nul_compatibility_ready: false,
    residual_acl_count: 0,
    residual_firewall_rule_count: 0,
    residual_appcontainer_profile_count: 0,
    restart_required: false,
    last_error_code: null,
  },
};

test.each(['pending_approval', 'session_permission', 'security_runtime'] as const)(
  'malformed %s pauses only its own scope, retains data and recovers on valid data',
  async (kind) => {
    const f = await fixture();
    const receive = vi.fn();
    f.supervisor.subscribeSecurity(receive);
    const change = (revision: string, sessionId = session) => ({
      runtime_instance_id: f.supervisor.snapshot.runtime_instance_id,
      revision,
      projection_kind: kind,
      payload:
        kind === 'pending_approval'
          ? { approvals: [], revision: 1 }
          : kind === 'session_permission'
            ? {
                session_id: sessionId,
                mode: 'read_only',
                revision: 1,
                active_grant_count: 0,
                transition: 'settled',
              }
            : securityRuntime,
    });
    const push = (value: unknown) =>
      f.callbacks().onNotification('security.projection.change', value);
    push(change('1'));
    expect(receive).toHaveBeenCalledTimes(1);
    const invalid = {
      ...change('2'),
      payload: { session_id: session, private_canary: 'DO-NOT-LOG-AUXILIARY' },
    };
    expect(() => push(invalid)).not.toThrow();
    expect(receive).toHaveBeenCalledTimes(1);
    expect(f.supervisor.projectionCompatibilitySnapshot.paused_projections).toEqual([
      {
        runtime_instance_id: f.supervisor.snapshot.runtime_instance_id,
        projection_kind: kind,
        projection_scope:
          kind === 'session_permission'
            ? { type: 'session', session_id: session }
            : { type: 'global' },
      },
    ]);
    expect(() =>
      push({ ...change('3'), runtime_instance_id: 'run_01ARZ3NDEKTSV4RRFFQ69G5FAW' }),
    ).not.toThrow();
    if (kind === 'session_permission') push(change('3', other));
    f.push('workspace_files', 2);
    expect(f.supervisor.projectionCompatibilitySnapshot.paused_projections).toHaveLength(1);
    push(change('4'));
    expect(f.supervisor.projectionCompatibilitySnapshot.paused_projections).toEqual([]);
    expect(f.transport.close).not.toHaveBeenCalled();
    expect(f.child.kill).not.toHaveBeenCalled();
    expect(JSON.stringify(vi.mocked(appendFile).mock.calls)).not.toContain('DO-NOT-LOG-AUXILIARY');
  },
);

test('malformed terminal state preserves the last view while output and another terminal continue', async () => {
  vi.useFakeTimers();
  try {
    const f = await fixture();
    const send = vi.fn();
    registerTerminalOutput(f.supervisor, () => ({ isDestroyed: () => false, send }) as never);
    const runtime = f.supervisor.snapshot.runtime_instance_id;
    const view = {
      session_id: session,
      terminal_id: 'term_fixture',
      shell: 'cmd',
      process_id: 1,
      started_at: '2026-10-15T00:00:00Z',
      end: null,
      owner: 'agent',
      controller: 'user',
      title: 'fixture',
      tool_call_id: null,
      revision: '1',
    };
    const push = (next: unknown, runtime_instance_id = runtime) =>
      f.callbacks().onNotification('terminal.state.v1', { runtime_instance_id, view: next });
    push(view);
    expect(send).toHaveBeenCalledWith('aura:v1:terminal-state', view);
    expect(() => push({ ...view, revision: '2', controller: 'invalid' })).not.toThrow();
    expect(send).toHaveBeenCalledTimes(1);
    expect(f.supervisor.projectionCompatibilitySnapshot.paused_projections).toEqual([
      {
        runtime_instance_id: runtime,
        projection_kind: 'terminal_state',
        projection_scope: { type: 'terminal', session_id: session, terminal_id: view.terminal_id },
      },
    ]);
    push({ ...view, terminal_id: 'term_other' });
    expect(() => push(view, 'run_01ARZ3NDEKTSV4RRFFQ69G5FAW')).not.toThrow();
    f.callbacks().onNotification('terminal.output.v1', {
      runtime_instance_id: runtime,
      session_id: session,
      terminal_id: view.terminal_id,
      start_offset: 0,
      next_offset: 1,
      data_base64: 'YQ==',
      truncated: false,
      end: null,
    });
    vi.advanceTimersByTime(16);
    expect(send).toHaveBeenCalledWith('aura:v1:terminal-output', expect.any(Array));
    expect(f.supervisor.projectionCompatibilitySnapshot.paused_projections).toHaveLength(1);
    push({ ...view, revision: '3' });
    expect(f.supervisor.projectionCompatibilitySnapshot.paused_projections).toEqual([]);
    expect(f.transport.close).not.toHaveBeenCalled();
  } finally {
    vi.useRealTimers();
  }
});
