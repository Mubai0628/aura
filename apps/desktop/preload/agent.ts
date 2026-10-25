import type { IpcErrorEnvelope } from '@aura/contracts/generated';
import { terminalBridge } from './terminal.js';
import { browserBridge } from './browser.js';
import { attachmentFile, attachmentPath, messagePicture, workspaceMediaUrl } from './media.js';
import { setMcpSecret } from './mcp-secret.js';
import type { ResultEnvelope } from '../shared/aura-api.js';
import {
  isAgentParams,
  parseAgentResult,
  type AgentMainCommand,
  type AgentMainParams,
  type AgentMainResult,
} from '../shared/agent-validation.js';

/** Fixed bridge surface; no generic invoke, actor or network-channel setter. */
export function agentBridge(
  send: (command: string, params: unknown) => Promise<unknown>,
  error: (code: string, message: string) => IpcErrorEnvelope,
  isError: (value: unknown) => value is IpcErrorEnvelope,
) {
  const invoke = async <C extends AgentMainCommand>(
    command: C,
    params: AgentMainParams[C],
  ): Promise<ResultEnvelope<AgentMainResult[C]>> => {
    if (!isAgentParams(command, params))
      return {
        ok: false,
        error: error('invalid_agent_request', `Invalid parameters for ${command}`),
      };
    let result: unknown;
    try {
      result = await send(command, params);
    } catch {
      return {
        ok: false,
        error: error('agent_transport_failed', 'Agent transport failed; request was not retried'),
      };
    }
    if (typeof result === 'object' && result !== null && 'ok' in result) {
      if (result.ok === false && 'error' in result && isError(result.error))
        return { ok: false, error: result.error };
      if (result.ok === true && 'value' in result) {
        try {
          return { ok: true, value: parseAgentResult(command, result.value) };
        } catch {
          /* Invalid projection never reaches the Renderer. */
        }
      }
    }
    return {
      ok: false,
      error: error('invalid_agent_result', 'Main returned an invalid Agent response'),
    };
  };
  return {
    terminal: terminalBridge(invoke, error, isError),
    browser: browserBridge(invoke),
    workspaceMediaUrl,
    attachmentPath,
    attachmentFile,
    messagePicture,
    readAttachmentText: (request: AgentMainParams['agent.attachments.text']) =>
      invoke('agent.attachments.text', request),
    readAgentToolSettings: (request: AgentMainParams['agent.tools.settings']) =>
      invoke('agent.tools.settings', request),
    configureAgentTools: (request: AgentMainParams['agent.tools.configure']) =>
      invoke('agent.tools.configure', request),
    testAgentNetworkProxy: (request: AgentMainParams['agent.tools.network_proxy.test']) =>
      invoke('agent.tools.network_proxy.test', request),
    readAgentContextRemediation: (request: AgentMainParams['agent.context.remediation']) =>
      invoke('agent.context.remediation', request),
    recheckAgentContext: (request: AgentMainParams['agent.context.recheck']) =>
      invoke('agent.context.recheck', request),
    resolveAgentContextWindow: (request: AgentMainParams['agent.context.window.resolve']) =>
      invoke('agent.context.window.resolve', request),
    requestAgentCompaction: (request: AgentMainParams['agent.compaction.request']) =>
      invoke('agent.compaction.request', request),
    listCommands: (request: AgentMainParams['agent.commands.catalog']) =>
      invoke('agent.commands.catalog', request),
    setCommandEnabled: (request: AgentMainParams['agent.commands.set_enabled']) =>
      invoke('agent.commands.set_enabled', request),
    deleteCommand: (request: AgentMainParams['agent.commands.delete']) =>
      invoke('agent.commands.delete', request),
    openCommandDirectory: (request: AgentMainParams['agent.commands.open_directory']) =>
      invoke('agent.commands.open_directory', request),
    openWorkspaceFileOutside: (request: AgentMainParams['agent.files.open_outside']) =>
      invoke('agent.files.open_outside', request),
    openExternalLink: (request: AgentMainParams['agent.links.open']) =>
      invoke('agent.links.open', request),
    writeClipboard: (request: AgentMainParams['agent.clipboard.write']) =>
      invoke('agent.clipboard.write', request),
    listHooks: (request: AgentMainParams['agent.hooks.list']) =>
      invoke('agent.hooks.list', request),
    previewHook: (request: AgentMainParams['agent.hooks.preview']) =>
      invoke('agent.hooks.preview', request),
    trustHook: (request: AgentMainParams['agent.hooks.trust']) =>
      invoke('agent.hooks.trust', request),
    untrustHook: (request: AgentMainParams['agent.hooks.untrust']) =>
      invoke('agent.hooks.untrust', request),
    setHookEnabled: (request: AgentMainParams['agent.hooks.set_enabled']) =>
      invoke('agent.hooks.set_enabled', request),
    removeHook: (request: AgentMainParams['agent.hooks.remove']) =>
      invoke('agent.hooks.remove', request),
    moveHook: (request: AgentMainParams['agent.hooks.move']) => invoke('agent.hooks.move', request),
    listHookInvocations: (request: AgentMainParams['agent.hooks.invocations']) =>
      invoke('agent.hooks.invocations', request),
    selectHookDirectory: (request: AgentMainParams['agent.hooks.select_directory']) =>
      invoke('agent.hooks.select_directory', request),
    listMcpServers: (request: AgentMainParams['agent.mcp.list']) =>
      invoke('agent.mcp.list', request),
    previewMcpServer: (request: AgentMainParams['agent.mcp.preview']) =>
      invoke('agent.mcp.preview', request),
    describeMcpServer: (request: AgentMainParams['agent.mcp.describe']) =>
      invoke('agent.mcp.describe', request),
    saveMcpServer: (request: AgentMainParams['agent.mcp.save']) =>
      invoke('agent.mcp.save', request),
    trustMcpServer: (request: AgentMainParams['agent.mcp.trust']) =>
      invoke('agent.mcp.trust', request),
    untrustMcpServer: (request: AgentMainParams['agent.mcp.untrust']) =>
      invoke('agent.mcp.untrust', request),
    setMcpServerEnabled: (request: AgentMainParams['agent.mcp.set_enabled']) =>
      invoke('agent.mcp.set_enabled', request),
    removeMcpServer: (request: AgentMainParams['agent.mcp.remove']) =>
      invoke('agent.mcp.remove', request),
    refreshMcpServer: (request: AgentMainParams['agent.mcp.refresh']) =>
      invoke('agent.mcp.refresh', request),
    setMcpSecret: setMcpSecret(isError, error),
    signInMcpServer: (request: AgentMainParams['agent.mcp.oauth.sign_in']) =>
      invoke('agent.mcp.oauth.sign_in', request),
    signOutMcpServer: (request: AgentMainParams['agent.mcp.oauth.sign_out']) =>
      invoke('agent.mcp.oauth.sign_out', request),
    getUiSettings: () => invoke('ui.settings.get', {}),
    updateUiSettings: (request: AgentMainParams['ui.settings.update']) =>
      invoke('ui.settings.update', request),
    listSkills: (request: AgentMainParams['agent.skills.list']) =>
      invoke('agent.skills.list', request),
    previewSkill: (request: AgentMainParams['agent.skills.preview']) =>
      invoke('agent.skills.preview', request),
    trustSkill: (request: AgentMainParams['agent.skills.trust']) =>
      invoke('agent.skills.trust', request),
    untrustSkill: (request: AgentMainParams['agent.skills.untrust']) =>
      invoke('agent.skills.untrust', request),
    setSkillEnabled: (request: AgentMainParams['agent.skills.set_enabled']) =>
      invoke('agent.skills.set_enabled', request),
    importSkill: (request: AgentMainParams['agent.skills.import']) =>
      invoke('agent.skills.import', request),
    removeSkill: (request: AgentMainParams['agent.skills.remove']) =>
      invoke('agent.skills.remove', request),
    selectSkillDirectory: (request: AgentMainParams['agent.skills.select_directory']) =>
      invoke('agent.skills.select_directory', request),
    selectSkillArchive: (request: AgentMainParams['agent.skills.select_archive']) =>
      invoke('agent.skills.select_archive', request),
    openSkillDirectory: (request: AgentMainParams['agent.skills.open_directory']) =>
      invoke('agent.skills.open_directory', request),
    listPlugins: (request: AgentMainParams['agent.plugins.list']) =>
      invoke('agent.plugins.list', request),
    previewPlugin: (request: AgentMainParams['agent.plugins.preview']) =>
      invoke('agent.plugins.preview', request),
    importPlugin: (request: AgentMainParams['agent.plugins.import']) =>
      invoke('agent.plugins.import', request),
    trustPlugin: (request: AgentMainParams['agent.plugins.trust']) =>
      invoke('agent.plugins.trust', request),
    untrustPlugin: (request: AgentMainParams['agent.plugins.untrust']) =>
      invoke('agent.plugins.untrust', request),
    setPluginEnabled: (request: AgentMainParams['agent.plugins.set_enabled']) =>
      invoke('agent.plugins.set_enabled', request),
    discardPluginUpdate: (request: AgentMainParams['agent.plugins.discard_update']) =>
      invoke('agent.plugins.discard_update', request),
    uninstallPlugin: (request: AgentMainParams['agent.plugins.uninstall']) =>
      invoke('agent.plugins.uninstall', request),
    selectPluginDirectory: (request: AgentMainParams['agent.plugins.select_directory']) =>
      invoke('agent.plugins.select_directory', request),
    selectPluginArchive: (request: AgentMainParams['agent.plugins.select_archive']) =>
      invoke('agent.plugins.select_archive', request),
    openPluginDirectory: (request: AgentMainParams['agent.plugins.open_directory']) =>
      invoke('agent.plugins.open_directory', request),
    readAgentCompaction: (request: AgentMainParams['agent.compaction.status']) =>
      invoke('agent.compaction.status', request),
    selectAgentQueryFile: (request: AgentMainParams['agent.query.select_file']) =>
      invoke('agent.query.select_file', request),
    readAgentHistory: (request: AgentMainParams['agent.history']) =>
      invoke('agent.history', request),
    branchSession: (request: AgentMainParams['agent.session.branch']) =>
      invoke('agent.session.branch', request),
    readAgentMessageText: (request: AgentMainParams['agent.message.text']) =>
      invoke('agent.message.text', request),
    readAgentInbox: (request: AgentMainParams['agent.inbox']) => invoke('agent.inbox', request),
    submitAgentMessage: (request: AgentMainParams['agent.submit']) =>
      invoke('agent.submit', request),
    mutateAgentInbox: (request: AgentMainParams['agent.inbox.mutate']) =>
      invoke('agent.inbox.mutate', request),
    readAgentResult: (request: AgentMainParams['agent.result']) => invoke('agent.result', request),
    readAgentInspector: (request: AgentMainParams['agent.inspector']) =>
      invoke('agent.inspector', request),
    startRelatedAgentTurn: (request: AgentMainParams['agent.related.start']) =>
      invoke('agent.related.start', request),
    readShellExecution: (request: AgentMainParams['agent.shell.query']) =>
      invoke('agent.shell.query', request),
    stopShellExecution: (request: AgentMainParams['agent.shell.stop']) =>
      invoke('agent.shell.stop', request),
    readAgentToolActivity: (request: AgentMainParams['agent.tool_activity']) =>
      invoke('agent.tool_activity', request),
    startAgent: (request: AgentMainParams['agent.start']) => invoke('agent.start', request),
    startWorkspaceInit: (request: AgentMainParams['agent.init.start']) =>
      invoke('agent.init.start', request),
    startAgentCompaction: (request: AgentMainParams['agent.compaction.start']) =>
      invoke('agent.compaction.start', request),
    getAgentRun: (request: AgentMainParams['agent.status']) => invoke('agent.status', request),
    cancelAgentRun: (request: AgentMainParams['agent.cancel']) => invoke('agent.cancel', request),
    controlAgentMode: (request: AgentMainParams['agent.mode.control']) =>
      invoke('agent.mode.control', request),
    answerAgentQuery: (request: AgentMainParams['agent.query.answer']) =>
      invoke('agent.query.answer', request),
    touchAgentQuery: (request: AgentMainParams['agent.query.touch']) =>
      invoke('agent.query.touch', request),
    readAgentSessionsActivity: (request: AgentMainParams['agent.sessions.activity']) =>
      invoke('agent.sessions.activity', request),
    getAgentApproval: (request: AgentMainParams['agent.approval.get']) =>
      invoke('agent.approval.get', request),
    resolveAgentApproval: (request: AgentMainParams['agent.approval.resolve']) =>
      invoke('agent.approval.resolve', request),
  };
}
