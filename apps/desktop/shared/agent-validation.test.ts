import { describe, expect, it, vi } from 'vitest';
import type { IpcMainInvokeEvent } from 'electron';
import type {
  AgentRunProjection,
  AgentStartRequest,
  IpcErrorEnvelope,
} from '@aura/contracts/generated';
import { agentBridge } from '../preload/agent.js';
import { dispatchDomainRoute } from '../electron-main/domain-route.js';
import {
  isAgentParams,
  parseAgentResult,
  validateAgentRequest,
  validateAgentResponse,
} from './agent-validation.js';

const id = '01ARZ3NDEKTSV4RRFFQ69G5FAV';
const start: AgentStartRequest = {
  session_id: `ses_${id}`,
  model_profile_id: `mdl_${id}`,
  text: 'Read the workspace',
  mode: 'normal',
  goal_budget: null,
};
const at = '2026-09-09T00:00:00Z';
const toolSettings = {
  revision: '7',
  max_dynamic_tool_schemas: 50,
  max_concurrent_tool_calls: 4,
  default_timeout_ms: 600_000,
  max_retries: 3,
  fixed_tool_ids: [],
  tools: [],
};
const projection: AgentRunProjection = {
  turn: {
    turn_task_id: `trn_${id}`,
    session_id: start.session_id,
    workspace_id: `wsp_${id}`,
    root_user_event_id: `evt_${id}`,
    lineage_id: `lin_${id}`,
    lineage_resolution_required: false,
    agent_run_id: `agt_${id}`,
    mode: 'normal',
    state: { state: 'running' },
    revision: '1',
    created_at: at,
    updated_at: at,
  },
  run: {
    agent_run_id: `agt_${id}`,
    turn_task_id: `trn_${id}`,
    parent_agent_run_id: null,
    state: { state: 'preparing_context' },
    revision: '1',
    next_model_ordinal: 1,
    prompt_bundle_snapshot_id: `pbs_${id}`,
    created_at: at,
    updated_at: at,
  },
  model_steps: [],
  pending_query: null,
  cancellation_requested: false,
  pending_approval_ids: [],
  recovery_episodes: [],
};
const error = (code: string, message: string): IpcErrorEnvelope => ({
  code,
  message,
  category: 'protocol',
  terminal: 'protocol_violation',
  correlation_id: `cor_${id}`,
  details: {},
  retryable: false,
});

describe('generated Agent boundary and fixed desktop bridge', () => {
  it('validates bounded user-terminal output and refuses caller-selected authority', () => {
    const output = {
      runtime_instance_id: `run_${id}`,
      session_id: `ses_${id}`,
      terminal_id: 'term_test',
      start_offset: 0,
      next_offset: 1,
      data_base64: 'YQ==',
      truncated: false,
      end: null,
    };
    expect(parseAgentResult('terminal.user.subscribe', output)).toEqual(output);
    expect(() =>
      parseAgentResult('terminal.user.subscribe', { ...output, data_base64: '%' }),
    ).toThrow();
    expect(
      isAgentParams('terminal.user.open', {
        session_id: `ses_${id}`,
        shell: 'cmd',
        columns: 80,
        rows: 24,
        actor: 'parent_agent',
      }),
    ).toBe(false);
  });
  it('accepts old settings and validates field-scoped recovery revisions and loopback targets', () => {
    expect(parseAgentResult('agent.tools.settings', toolSettings)).toEqual(toolSettings);
    const recovery = {
      expected_revision: '7',
      source_revision: '5',
      proxy: { enabled: true, protocol: 'http', host: '127.0.0.1', port: 10809 },
    };
    const value = { ...toolSettings, network_proxy_recovery: recovery };
    expect(parseAgentResult('agent.tools.settings', value)).toEqual(value);
    expect(() =>
      parseAgentResult('agent.tools.settings', {
        ...value,
        network_proxy_recovery: { ...recovery, expected_revision: 7 },
      }),
    ).toThrow();
    expect(() =>
      parseAgentResult('agent.tools.settings', {
        ...value,
        network_proxy_recovery: {
          ...recovery,
          proxy: { ...recovery.proxy, host: 'example.com' },
        },
      }),
    ).toThrow();
  });
  it('rejects caller-selected authority and forged identities before Main dispatch', async () => {
    const handlers = {
      agent: vi.fn().mockResolvedValue(projection),
      workspace: vi.fn(),
      security: vi.fn(),
      model: vi.fn(),
      tool: vi.fn(),
      git: vi.fn(),
      package: vi.fn(),
      memory: vi.fn(),
    };
    const event = {} as IpcMainInvokeEvent;
    expect(isAgentParams('agent.start', start)).toBe(true);
    await expect(dispatchDomainRoute(event, 'agent.start', start, handlers)).resolves.toEqual(
      projection,
    );
    for (const bad of [
      { ...start, channel: 'model' },
      { ...start, permission_mode: 'full_access' },
      { ...start, actor: 'authenticated_main' },
      { ...start, session_id: `agt_${id}` },
      { ...start, session_id: `ses_${id}\n` },
    ]) {
      expect(isAgentParams('agent.start', bad)).toBe(false);
      await expect(dispatchDomainRoute(event, 'agent.start', bad, handlers)).rejects.toThrow();
    }
    expect(handlers.agent).toHaveBeenCalledTimes(1);
    await expect(
      dispatchDomainRoute(event, 'internal.e2e.agent', start, handlers),
    ).rejects.toThrow();
    for (const name of ['workspace', 'security', 'model', 'tool'] as const)
      expect(handlers[name]).not.toHaveBeenCalled();
  });
  it('keeps numeric limits and the Session authority out of caller control', () => {
    expect(
      isAgentParams('agent.start', {
        ...start,
        goal_budget: { max_tokens: Number.MAX_SAFE_INTEGER },
      }),
    ).toBe(true);
    for (const n of [-1, 0.5, Number.MAX_SAFE_INTEGER + 1, Infinity, NaN])
      expect(isAgentParams('agent.start', { ...start, goal_budget: { max_tokens: n } })).toBe(
        false,
      );
    const value = { workspace_id: `wsp_${id}`, title: 'New task' };
    expect(validateAgentRequest({ command: 'session.create', value })).toBe(true);
    expect(
      validateAgentRequest({ command: 'session.create', value: { ...value, mode: 'full_access' } }),
    ).toBe(false);
    expect(
      validateAgentResponse({
        command: 'session.create',
        value: { ...value, mode: 'full_access' },
      }),
    ).toBe(false);
  });
  it('validates the real projection and rejects incomplete or foreign DTOs', () => {
    expect(parseAgentResult('agent.start', projection)).toEqual(projection);
    for (const value of [
      {},
      { ...projection, side_effect: 'verified' },
      { ...projection, run: { ...projection.run, state: { state: 'completed', replay: true } } },
      { ...projection, turn: { ...projection.turn, session_id: 'forged' } },
    ])
      expect(() => parseAgentResult('agent.status', value)).toThrow();
  });
  it('exposes only the fixed whitelisted actions and preserves validated success/refusal', async () => {
    const refusal = error('permission.denied', 'Denied');
    const send = vi
      .fn()
      .mockResolvedValueOnce({ ok: true, value: projection })
      .mockResolvedValueOnce({ ok: false, error: refusal });
    const bridge = agentBridge(send, error, (v): v is IpcErrorEnvelope => v === refusal);
    expect(Object.keys(bridge).sort()).toEqual([
      'answerAgentQuery',
      // What a message carried from outside the workspace, through its authorization.
      'attachmentFile',
      // Where a file added to a message is (never its contents).
      'attachmentPath',
      'branchSession',
      // Aura's built-in browser (its own validated channel).
      'browser',
      'cancelAgentRun',
      // Chrome control: settings, a conversation's shared tabs, Stop.
      'chrome',
      'configureAgentTools',
      // A turn a restart of Core cut off, continued with "继续".
      'continueAgent',
      'controlAgentMode',
      'deleteCommand',
      'describeMcpServer',
      'discardPluginUpdate',
      'getAgentApproval',
      'getAgentRun',
      'getUiSettings',
      'importPlugin',
      'importSkill',
      'listCommands',
      'listHookInvocations',
      'listHooks',
      'listMcpServers',
      'listPlugins',
      'listSkills',
      // A sent picture copied or saved by the user.
      'messagePicture',
      'moveHook',
      'mutateAgentInbox',
      'openCommandDirectory',
      'openExternalLink',
      'openPluginDirectory',
      'openSkillDirectory',
      'openWorkspaceFileOutside',
      'previewHook',
      'previewMcpServer',
      'previewPlugin',
      'previewSkill',
      'readAgentCompaction',
      'readAgentContextRemediation',
      'readAgentHistory',
      'readAgentInbox',
      'readAgentInspector',
      'readAgentMessageText',
      'readAgentResult',
      'readAgentSessionsActivity',
      'readAgentToolActivity',
      'readAgentToolSettings',
      // The text of a text file a message carried.
      'readAttachmentText',
      'readShellExecution',
      'rebuildWorkspaceIndex',
      'recheckAgentContext',
      'refreshMcpServer',
      'removeHook',
      'removeMcpServer',
      'removeSkill',
      'requestAgentCompaction',
      'resolveAgentApproval',
      'resolveAgentContextWindow',
      'restoreMcpPluginDefault',
      'saveMcpServer',
      'selectAgentQueryFile',
      'selectHookDirectory',
      'selectPluginArchive',
      'selectPluginDirectory',
      'selectSkillArchive',
      'selectSkillDirectory',
      'setCommandEnabled',
      'setHookEnabled',
      'setMcpSecret',
      'setMcpServerEnabled',
      'setPluginEnabled',
      'setSkillEnabled',
      'signInMcpServer',
      'signOutMcpServer',
      'startAgent',
      // `/compact` while nothing runs.
      'startAgentCompaction',
      'startRelatedAgentTurn',
      'startWorkspaceInit',
      'stopShellExecution',
      'submitAgentMessage',
      'terminal',
      'testAgentNetworkProxy',
      'touchAgentQuery',
      'trustHook',
      'trustMcpServer',
      'trustPlugin',
      'trustSkill',
      'uninstallPlugin',
      'untrustHook',
      'untrustMcpServer',
      'untrustPlugin',
      'untrustSkill',
      'updateUiSettings',
      // A workspace's media for the 文件 tab (its own validated channel).
      'workspaceMediaUrl',
      'writeClipboard',
    ]);
    await expect(bridge.startAgent(start)).resolves.toEqual({ ok: true, value: projection });
    await expect(
      bridge.cancelAgentRun({
        session_id: start.session_id,
        agent_run_id: projection.run.agent_run_id,
      }),
    ).resolves.toEqual({ ok: false, error: refusal });
    expect(send.mock.calls.map(([command]) => command)).toEqual(['agent.start', 'agent.cancel']);
  });
  it('refuses malformed Main replies and does not retry ambiguous transport failures', async () => {
    const send = vi
      .fn()
      .mockResolvedValueOnce({ ok: true, value: {} })
      .mockRejectedValueOnce(new Error('private transport detail'));
    const knownRefusal = error('permission.denied', 'Denied');
    const bridge = agentBridge(send, error, (v): v is IpcErrorEnvelope => v === knownRefusal);
    const malformed = await bridge.startAgent(start);
    expect(malformed.ok).toBe(false);
    const failed = await bridge.startAgent(start);
    expect(failed.ok).toBe(false);
    expect(JSON.stringify(failed)).not.toContain('private transport detail');
    expect(send).toHaveBeenCalledTimes(2);
    await bridge.startAgent({ ...start, channel: 'general_tool' } as AgentStartRequest);
    expect(send).toHaveBeenCalledTimes(2);
  });
});
