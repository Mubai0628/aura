import type { SessionConsentsApi } from './api/session-consents.js';
import type { GitApiV1 } from './git-api.js';
import type { AttachmentFileRequest, AttachmentFileResult } from './attachment-files.js';
import type { MessagePictureRequest, MessagePictureResult } from './message-pictures.js';
import type { WorkspaceMediaRequest, WorkspaceMediaResult } from './workspace-media.js';
import type { ConfigRecoveryMainParams, ConfigRecoveryMainResult } from './generated/routes.js';
import type { ToolCredentialMainParams, ToolCredentialMainResult } from './generated/routes.js';
import type { RuntimeReadiness } from '@aura/contracts/generated';
import type {
  AgentMainParams,
  AgentMainResult,
  P03MainParams,
  P03MainResult,
} from './generated/routes.js';
import type {
  BeginWorkspaceLifecycleResult,
  ApprovalRequest,
  ApprovalResolution,
  ApprovalView,
  BufferEditCommand,
  BufferEditResult,
  BufferOpenCommand,
  BufferSaveCommand,
  DiscardConfirmation,
  FsApplyChangesetRequest,
  FsApplyChangesetResult,
  FsApplyPatchRequest,
  FsCopyRequest,
  FsCreateDirectoryRequest,
  FsDeleteRequest,
  FsFindPathsRequest,
  FsGlobRequest,
  FsGlobResult,
  FsHashRequest,
  FsHashResult,
  FsInspectRequest,
  FsInspectResult,
  FsListRequest,
  FsListResult,
  FsMoveRequest,
  FsMutationResult,
  FsReadBinaryRangeRequest,
  FsReadBinaryRangeResult,
  FsReadTextRequest,
  FsReadTextResult,
  FsSearchTextRequest,
  FsSearchTextResult,
  FsStatRequest,
  FsStatResult,
  FsWriteFileRequest,
  IpcErrorEnvelope,
  HostPreparationStatus,
  LogicalFileStatus,
  PathSearchResult,
  PrepareEditorLifecycleTransition,
  PermissionGrant,
  ProjectionChange,
  QuarantineRecordDetails,
  SecurityProjectionChange,
  SessionPermissionProjection,
  ToolActivityPageRequestV1,
  ToolActivityPageV1,
  ToolCredentialCandidatesProjectionV1,
  WorkspaceArchiveCommand,
  WorkspaceCreateCommand,
  WorkspaceCreateResult,
  WorkspaceRelocateCommand,
  WorkspaceRestoreCommand,
  WorkspaceLifecycleSummary,
  WorkspaceSummary,
} from '@aura/contracts/generated';

import type {
  RendererIssueDiscardConfirmation,
  RendererLifecycleAck,
  RendererResolveWorkspaceLifecycle,
} from './p03-validation.js';
import type { AppUpdateCommand, AppUpdateState } from './app-updates.js';
import type { P04MainParams, P04MainResult } from './p04-validation.js';
import type { ModelProjectionChange, P05MainParams, P05MainResult } from './p05-validation.js';
import type { P07MainParams, P07MainResult, P07ReadParams } from './p07-validation.js';
import type {
  ModelProjectionRouteRequest,
  ToolProjectionChange,
  ToolProjectionRouteRequest,
} from './projection-validation.js';

export type { ModelProjectionChange } from './p05-validation.js';
export type { P07MainCommand, P07MainParams, P07MainResult } from './p07-validation.js';
export type {
  ModelProjectionRouteRequest,
  ToolProjectionChange,
  ToolProjectionRouteRequest,
} from './projection-validation.js';

export type CoreHealthProjectionChange = Extract<
  ProjectionChange,
  { projection_kind: 'core_health' }
>;
export type WorkspaceSessionProjectionChange = Extract<
  ProjectionChange,
  { projection_kind: 'workspace_session' }
>;
export type WorkspaceFilesProjectionChange = Extract<
  ProjectionChange,
  { projection_kind: 'workspace_files' }
>;
export type SecurityControlProjectionChange = SecurityProjectionChange;

export type ResultEnvelope<Value> =
  { ok: true; value: Value } | { error: IpcErrorEnvelope; ok: false };

export interface BinaryStreamOpenRequest {
  streamId: string;
}

export interface BinaryStreamChunk {
  chunkSequence: bigint;
  data: ArrayBuffer;
  endOfMessage: boolean;
}

export interface AuraBinaryStream {
  cancel(): void;
  readonly closed: Promise<ResultEnvelope<void>>;
  onChunk(listener: (chunk: BinaryStreamChunk) => void): () => void;
}

export interface AuraApiV1 extends SessionConsentsApi {
  mainErrors?: import('./main-errors-api.js').MainErrorsApiV1;
  git: GitApiV1;
  packages: import('./package-api.js').PackageApiV1;
  memory: import('./memory-api.js').MemoryApiV1;
  projectionCompatibility?: import('./projection-compatibility-api.js').ProjectionCompatibilityApiV1;
  terminal?: import('./terminal-api.js').TerminalApiV1;
  browser?: import('./browser-api.js').BrowserApiV1;
  chrome?: import('./chrome-control-api.js').ChromeControlApiV1;
  updateToolCredentialOrigins(
    request: ToolCredentialMainParams['tools.credential.origins'],
  ): Promise<ResultEnvelope<ToolCredentialMainResult['tools.credential.origins']>>;
  listToolCredentials(
    request: ToolCredentialMainParams['tools.credential.list'],
  ): Promise<ResultEnvelope<ToolCredentialMainResult['tools.credential.list']>>;
  readToolCredentialOutcome(
    request: ToolCredentialMainParams['tools.credential.outcome'],
  ): Promise<ResultEnvelope<ToolCredentialMainResult['tools.credential.outcome']>>;
  createToolCredential(
    request: ToolCredentialMainParams['tools.credential.create'],
    secret: Uint8Array,
  ): Promise<ResultEnvelope<ToolCredentialMainResult['tools.credential.create']>>;
  rotateToolCredential(
    request: ToolCredentialMainParams['tools.credential.rotate'],
    secret: Uint8Array,
  ): Promise<ResultEnvelope<ToolCredentialMainResult['tools.credential.rotate']>>;
  clearToolCredential(
    request: ToolCredentialMainParams['tools.credential.clear'],
  ): Promise<ResultEnvelope<ToolCredentialMainResult['tools.credential.clear']>>;
  recoverToolCredential(
    request: ToolCredentialMainParams['tools.credential.recover'],
  ): Promise<ResultEnvelope<ToolCredentialMainResult['tools.credential.recover']>>;
  getConfigRecoveryNotice(): Promise<
    ResultEnvelope<ConfigRecoveryMainResult['config.recovery.notice']>
  >;
  planConfigRecovery(
    request: ConfigRecoveryMainParams['config.recovery.plan'],
  ): Promise<ResultEnvelope<ConfigRecoveryMainResult['config.recovery.plan']>>;
  applyConfigRecovery(
    request: ConfigRecoveryMainParams['config.recovery.apply'],
  ): Promise<ResultEnvelope<ConfigRecoveryMainResult['config.recovery.apply']>>;
  readShellExecution(
    request: AgentMainParams['agent.shell.query'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.shell.query']>>;
  stopShellExecution(
    request: AgentMainParams['agent.shell.stop'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.shell.stop']>>;
  readAgentToolActivity(
    request: AgentMainParams['agent.tool_activity'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.tool_activity']>>;
  readAgentToolSettings(
    request: AgentMainParams['agent.tools.settings'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.tools.settings']>>;
  configureAgentTools(
    request: AgentMainParams['agent.tools.configure'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.tools.configure']>>;
  testAgentNetworkProxy(
    request: AgentMainParams['agent.tools.network_proxy.test'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.tools.network_proxy.test']>>;

  createSession(
    request: P03MainParams['session.create'],
  ): Promise<ResultEnvelope<P03MainResult['session.create']>>;
  renameSession(
    request: P03MainParams['session.rename'],
  ): Promise<ResultEnvelope<P03MainResult['session.rename']>>;
  archiveSession(
    request: P03MainParams['session.archive'],
  ): Promise<ResultEnvelope<P03MainResult['session.archive']>>;
  restoreSession(
    request: P03MainParams['session.restore'],
  ): Promise<ResultEnvelope<P03MainResult['session.restore']>>;
  deleteSession(
    request: P03MainParams['session.delete'],
  ): Promise<ResultEnvelope<P03MainResult['session.delete']>>;
  readAgentHistory(
    request: AgentMainParams['agent.history'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.history']>>;
  /** A new conversation from one finished turn of this one. */
  branchSession(
    request: AgentMainParams['agent.session.branch'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.session.branch']>>;
  readAgentMessageText(
    request: AgentMainParams['agent.message.text'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.message.text']>>;
  selectAgentQueryFile(
    request: AgentMainParams['agent.query.select_file'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.query.select_file']>>;
  readAgentInbox(
    request: AgentMainParams['agent.inbox'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.inbox']>>;
  submitAgentMessage(
    request: AgentMainParams['agent.submit'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.submit']>>;
  mutateAgentInbox(
    request: AgentMainParams['agent.inbox.mutate'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.inbox.mutate']>>;
  readAgentResult(
    request: AgentMainParams['agent.result'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.result']>>;
  readAgentInspector(
    request: AgentMainParams['agent.inspector'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.inspector']>>;
  startRelatedAgentTurn(
    request: AgentMainParams['agent.related.start'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.related.start']>>;
  requestAgentCompaction(
    request: AgentMainParams['agent.compaction.request'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.compaction.request']>>;
  /** The user's Slash command menu (built-in and prompt commands). */
  listCommands(
    request: AgentMainParams['agent.commands.catalog'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.commands.catalog']>>;
  /** Settings → 命令: turn a user or workspace command on or off. */
  setCommandEnabled(
    request: AgentMainParams['agent.commands.set_enabled'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.commands.set_enabled']>>;
  /** Settings → 命令: remove one of the user's own commands at the revision shown. */
  deleteCommand(
    request: AgentMainParams['agent.commands.delete'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.commands.delete']>>;
  /** Opens a command folder that Core's catalog names. */
  openCommandDirectory(
    request: AgentMainParams['agent.commands.open_directory'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.commands.open_directory']>>;
  /**
   * A workspace file with its default application (never a program or a script) or in
   * the file manager. Main checks the path stays in the workspace.
   */
  openWorkspaceFileOutside(
    request: AgentMainParams['agent.files.open_outside'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.files.open_outside']>>;
  /**
   * The address of Aura's own scheme a workspace picture, video or sound streams from, for
   * the 文件 tab. Main checks the path stays in the workspace and names media.
   */
  workspaceMediaUrl(request: WorkspaceMediaRequest): Promise<WorkspaceMediaResult>;
  /**
   * Where a file the user added to a message is on this computer; empty when it has no
   * place (a pasted picture). Core checks a file named this way is in the workspace.
   */
  attachmentPath(file: File): string;
  /**
   * An attached file outside the workspace — streamed, its beginning read, or opened —
   * found again by Core through the conversation's read-only authorization.
   */
  attachmentFile(request: AttachmentFileRequest): Promise<AttachmentFileResult>;
  /** Aura's own updates: check, download, apply, restart or go back a version (Main does it). */
  appUpdate(command: AppUpdateCommand): Promise<AppUpdateState>;
  /** The update state as Main reports it; returns a function that stops listening. */
  onAppUpdateState(listener: (state: AppUpdateState) => void): () => void;
  /** A message's picture copied as a picture, or saved where the user chooses. */
  messagePicture(request: MessagePictureRequest): Promise<MessagePictureResult>;
  /** A text file or a zip's listing one of the conversation's messages carries. */
  readAttachmentText(
    request: AgentMainParams['agent.attachments.text'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.attachments.text']>>;
  /** An http(s) page the conversation links to, in the default browser. */
  openExternalLink(
    request: AgentMainParams['agent.links.open'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.links.open']>>;
  /**
   * Text the user copies — a reply, a message, a path, a link. Main writes it, since it
   * refuses the browser's clipboard permissions to every page.
   */
  writeClipboard(
    request: AgentMainParams['agent.clipboard.write'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.clipboard.write']>>;
  /** Walks a workspace's folder again after its file index failed to build. */
  rebuildWorkspaceIndex(
    request: AgentMainParams['agent.workspace.rebuild_index'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.workspace.rebuild_index']>>;
  /** Settings → Hook: the registry in execution order, each Hook with its live state. */
  listHooks(
    request: AgentMainParams['agent.hooks.list'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.hooks.list']>>;
  /** A Hook folder's manifest, files and fingerprint, read before the user trusts it. */
  previewHook(
    request: AgentMainParams['agent.hooks.preview'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.hooks.preview']>>;
  /** Trusts exactly the previewed fingerprint; adds the Hook when it is not listed yet. */
  trustHook(
    request: AgentMainParams['agent.hooks.trust'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.hooks.trust']>>;
  /** Withdraws trust; the Hook stays listed and no longer runs. */
  untrustHook(
    request: AgentMainParams['agent.hooks.untrust'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.hooks.untrust']>>;
  /** Turns a listed Hook on or off at the revision shown. */
  setHookEnabled(
    request: AgentMainParams['agent.hooks.set_enabled'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.hooks.set_enabled']>>;
  /** Removes a Hook from the list; its folder is left untouched. */
  removeHook(
    request: AgentMainParams['agent.hooks.remove'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.hooks.remove']>>;
  /** Moves a Hook one place up or down in the execution order. */
  moveHook(
    request: AgentMainParams['agent.hooks.move'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.hooks.move']>>;
  /** Recent Hook calls, newest first; naming a turn also returns its totals. */
  listHookInvocations(
    request: AgentMainParams['agent.hooks.invocations'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.hooks.invocations']>>;
  /** Main's own folder dialog for adding a Hook. */
  selectHookDirectory(
    request: AgentMainParams['agent.hooks.select_directory'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.hooks.select_directory']>>;
  /** Settings → MCP: every configured server with its live state. */
  listMcpServers(
    request: AgentMainParams['agent.mcp.list'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.mcp.list']>>;
  /** What a configuration would run and the fingerprint trust would record. */
  previewMcpServer(
    request: AgentMainParams['agent.mcp.preview'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.mcp.preview']>>;
  /** One server's listed tools, skipped tools and recent error output. */
  describeMcpServer(
    request: AgentMainParams['agent.mcp.describe'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.mcp.describe']>>;
  /** Adds a server (untrusted and off) or edits one at the revision shown. */
  saveMcpServer(
    request: AgentMainParams['agent.mcp.save'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.mcp.save']>>;
  /** Trusts exactly the shown fingerprint, switches the server on and lists its tools. */
  trustMcpServer(
    request: AgentMainParams['agent.mcp.trust'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.mcp.trust']>>;
  /** Withdraws trust; the server stops and its tools leave the registry. */
  untrustMcpServer(
    request: AgentMainParams['agent.mcp.untrust'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.mcp.untrust']>>;
  /** Switches a trusted server on or off at the revision shown. */
  setMcpServerEnabled(
    request: AgentMainParams['agent.mcp.set_enabled'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.mcp.set_enabled']>>;
  restoreMcpPluginDefault(
    request: AgentMainParams['agent.mcp.restore_plugin_default'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.mcp.restore_plugin_default']>>;
  /** Removes a server from the list; its program is left untouched. */
  removeMcpServer(
    request: AgentMainParams['agent.mcp.remove'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.mcp.remove']>>;
  /** Starts the server once to list its tools again (测试连接). */
  refreshMcpServer(
    request: AgentMainParams['agent.mcp.refresh'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.mcp.refresh']>>;
  /**
   * One server's secret, sent only as SecretInput bytes (zeroed after sending). The
   * result names the secret's ID for the configuration; the value is never returned.
   */
  setMcpSecret(
    request: AgentMainParams['agent.mcp.secret.set'],
    secret: Uint8Array,
  ): Promise<ResultEnvelope<AgentMainResult['agent.mcp.secret.set']>>;
  /**
   * Starts an OAuth sign-in. Main opens the authorization address in the system
   * browser (https, or this computer only); the result names it as a link to copy too.
   */
  signInMcpServer(
    request: AgentMainParams['agent.mcp.oauth.sign_in'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.mcp.oauth.sign_in']>>;
  /** Forgets a server's sign-in; its running instances stop. */
  signOutMcpServer(
    request: AgentMainParams['agent.mcp.oauth.sign_out'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.mcp.oauth.sign_out']>>;
  /** 设置 → 环境: whether the session shows its debugging information, the default terminal. */
  getUiSettings(): Promise<ResultEnvelope<AgentMainResult['ui.settings.get']>>;
  /** Saves the interface settings at the revision the page showed. */
  updateUiSettings(
    request: AgentMainParams['ui.settings.update'],
  ): Promise<ResultEnvelope<AgentMainResult['ui.settings.update']>>;
  /** Settings → Skill: every Skill found, with the user's choices; a workspace adds its own. */
  listSkills(
    request: AgentMainParams['agent.skills.list'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.skills.list']>>;
  /** One Skill's SKILL.md, files and fingerprint, read before the user trusts it. */
  previewSkill(
    request: AgentMainParams['agent.skills.preview'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.skills.preview']>>;
  /** Trusts exactly the shown fingerprint and switches the Skill on. */
  trustSkill(
    request: AgentMainParams['agent.skills.trust'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.skills.trust']>>;
  /** Withdraws trust; the Skill is no longer offered to the model. */
  untrustSkill(
    request: AgentMainParams['agent.skills.untrust'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.skills.untrust']>>;
  /** Switches a Skill on or off at the revision shown. */
  setSkillEnabled(
    request: AgentMainParams['agent.skills.set_enabled'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.skills.set_enabled']>>;
  /** Copies a Skill folder into Aura's own; the original is not changed. */
  importSkill(
    request: AgentMainParams['agent.skills.import'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.skills.import']>>;
  /** Removes a Skill imported into Aura's own folder; other Skills can only be switched off. */
  removeSkill(
    request: AgentMainParams['agent.skills.remove'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.skills.remove']>>;
  /** Main's own folder dialog for importing a Skill. */
  selectSkillDirectory(
    request: AgentMainParams['agent.skills.select_directory'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.skills.select_directory']>>;
  /** Main's own file dialog for importing a Skill packed as a ZIP file. */
  selectSkillArchive(
    request: AgentMainParams['agent.skills.select_archive'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.skills.select_archive']>>;
  /** Opens a listed Skill's folder in the file manager; only a folder named by its id. */
  openSkillDirectory(
    request: AgentMainParams['agent.skills.open_directory'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.skills.open_directory']>>;
  /** Settings → 插件: every installed plugin, with the user's choices and a waiting update. */
  listPlugins(
    request: AgentMainParams['agent.plugins.list'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.plugins.list']>>;
  /** Everything an installed plugin (or its waiting update) brings, read before the user trusts it. */
  previewPlugin(
    request: AgentMainParams['agent.plugins.preview'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.plugins.preview']>>;
  /** Copies a plugin folder or ZIP file into Aura's own; a trusted plugin's newer copy waits as its update. */
  importPlugin(
    request: AgentMainParams['agent.plugins.import'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.plugins.import']>>;
  /** Trusts exactly the shown fingerprint and switches the plugin on; for an update, it replaces the installed copy. */
  trustPlugin(
    request: AgentMainParams['agent.plugins.trust'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.plugins.trust']>>;
  /** Withdraws trust; nothing the plugin brings is offered. */
  untrustPlugin(
    request: AgentMainParams['agent.plugins.untrust'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.plugins.untrust']>>;
  /** Switches a plugin on or off at the revision shown. */
  setPluginEnabled(
    request: AgentMainParams['agent.plugins.set_enabled'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.plugins.set_enabled']>>;
  /** Removes a waiting update; the installed copy is unchanged. */
  discardPluginUpdate(
    request: AgentMainParams['agent.plugins.discard_update'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.plugins.discard_update']>>;
  /** Removes the plugin, its folder, and its servers' secrets and sign-ins. */
  uninstallPlugin(
    request: AgentMainParams['agent.plugins.uninstall'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.plugins.uninstall']>>;
  /** Main's own folder dialog for importing a plugin. */
  selectPluginDirectory(
    request: AgentMainParams['agent.plugins.select_directory'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.plugins.select_directory']>>;
  /** Main's own file dialog for importing a plugin packed as a ZIP file. */
  selectPluginArchive(
    request: AgentMainParams['agent.plugins.select_archive'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.plugins.select_archive']>>;
  /** Opens an installed plugin's folder in the file manager; only a folder named by the plugin. */
  openPluginDirectory(
    request: AgentMainParams['agent.plugins.open_directory'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.plugins.open_directory']>>;
  readAgentContextRemediation(
    request: AgentMainParams['agent.context.remediation'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.context.remediation']>>;
  recheckAgentContext(
    request: AgentMainParams['agent.context.recheck'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.context.recheck']>>;
  resolveAgentContextWindow(
    request: AgentMainParams['agent.context.window.resolve'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.context.window.resolve']>>;
  readAgentCompaction(
    request: AgentMainParams['agent.compaction.status'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.compaction.status']>>;
  startAgent(
    request: AgentMainParams['agent.start'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.start']>>;
  /** `/init`: a one-off initialization task in an idle session. */
  startWorkspaceInit(
    request: AgentMainParams['agent.init.start'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.init.start']>>;
  /** `/compact` while nothing runs: a one-off compaction task. */
  startAgentCompaction(
    request: AgentMainParams['agent.compaction.start'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.compaction.start']>>;
  getAgentRun(
    request: AgentMainParams['agent.status'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.status']>>;
  cancelAgentRun(
    request: AgentMainParams['agent.cancel'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.cancel']>>;
  controlAgentMode(
    request: AgentMainParams['agent.mode.control'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.mode.control']>>;
  answerAgentQuery(
    request: AgentMainParams['agent.query.answer'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.query.answer']>>;
  /** The user touched the waiting question; its countdown starts again. */
  touchAgentQuery(
    request: AgentMainParams['agent.query.touch'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.query.touch']>>;
  /** Which conversations have a turn running or waiting for the user, for the sidebar. */
  readAgentSessionsActivity(
    request: AgentMainParams['agent.sessions.activity'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.sessions.activity']>>;
  getAgentApproval(
    request: AgentMainParams['agent.approval.get'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.approval.get']>>;
  resolveAgentApproval(
    request: AgentMainParams['agent.approval.resolve'],
  ): Promise<ResultEnvelope<AgentMainResult['agent.approval.resolve']>>;
  attachExternalReadOnly(
    request: P04MainParams['security.external_grant.attach_read_only'],
  ): Promise<ResultEnvelope<P04MainResult['security.external_grant.attach_read_only']>>;
  deriveWorkspaceExternalGrants(
    request: P04MainParams['security.external_grant.derive_workspace'],
  ): Promise<ResultEnvelope<P04MainResult['security.external_grant.derive_workspace']>>;
  discardQuarantine(
    request: P04MainParams['security.quarantine.discard'],
  ): Promise<ResultEnvelope<P04MainResult['security.quarantine.discard']>>;
  exportArtifact(
    request: P04MainParams['artifact.export'],
  ): Promise<ResultEnvelope<P04MainResult['artifact.export']>>;
  applyWorkspaceChangeset(
    request: FsApplyChangesetRequest,
  ): Promise<ResultEnvelope<FsApplyChangesetResult>>;
  applyWorkspacePatch(request: FsApplyPatchRequest): Promise<ResultEnvelope<FsMutationResult>>;
  bindToolInstanceCredential(
    request: P07MainParams['tools.instance.bind_credential'],
  ): Promise<ResultEnvelope<P07MainResult['tools.instance.bind_credential']>>;
  clearSessionToolPin(
    request: P07MainParams['tools.pin.session.clear'],
  ): Promise<ResultEnvelope<P07MainResult['tools.pin.session.clear']>>;
  clearToolInstanceCredential(
    request: P07MainParams['tools.instance.clear_credential'],
  ): Promise<ResultEnvelope<P07MainResult['tools.instance.clear_credential']>>;
  clearWorkspaceToolPin(
    request: P07MainParams['tools.pin.workspace.clear'],
  ): Promise<ResultEnvelope<P07MainResult['tools.pin.workspace.clear']>>;
  setSessionToolPin(
    request: P07MainParams['tools.pin.session.set'],
  ): Promise<ResultEnvelope<P07MainResult['tools.pin.session.set']>>;
  setToolEnabled(
    request: P07MainParams['tools.tool.set_enabled'],
  ): Promise<ResultEnvelope<P07MainResult['tools.tool.set_enabled']>>;
  setWorkspaceToolPin(
    request: P07MainParams['tools.pin.workspace.set'],
  ): Promise<ResultEnvelope<P07MainResult['tools.pin.workspace.set']>>;
  updateToolInstanceConfiguration(
    request: P07MainParams['tools.instance.update_configuration'],
  ): Promise<ResultEnvelope<P07MainResult['tools.instance.update_configuration']>>;
  clearModelCredential(
    request: P05MainParams['models.endpoint.clear_credential'],
  ): Promise<ResultEnvelope<P05MainResult['models.endpoint.clear_credential']>>;
  clearModelContextWindowOverride(
    request: P05MainParams['models.context_window.clear_user_override'],
  ): Promise<ResultEnvelope<P05MainResult['models.context_window.clear_user_override']>>;
  configureModelCredential(
    request: P05MainParams['models.endpoint.configure_credential'],
    secret: Uint8Array,
  ): Promise<ResultEnvelope<P05MainResult['models.endpoint.configure_credential']>>;
  correctModelContextWindow(
    request: P05MainParams['models.context_window.correct'],
  ): Promise<ResultEnvelope<P05MainResult['models.context_window.correct']>>;
  createModelEndpoint(
    request: P05MainParams['models.endpoint.create'],
  ): Promise<ResultEnvelope<P05MainResult['models.endpoint.create']>>;
  createModelEndpointWithCredential(
    request: P05MainParams['models.endpoint.create_with_credential'],
    secret: Uint8Array,
  ): Promise<ResultEnvelope<P05MainResult['models.endpoint.create_with_credential']>>;
  createModelProfile(
    request: P05MainParams['models.profile.create_manual'],
  ): Promise<ResultEnvelope<P05MainResult['models.profile.create_manual']>>;
  createModelProvider(
    request: P05MainParams['models.provider.create'],
  ): Promise<ResultEnvelope<P05MainResult['models.provider.create']>>;
  deleteModelEndpoint(
    request: P05MainParams['models.endpoint.delete'],
  ): Promise<ResultEnvelope<P05MainResult['models.endpoint.delete']>>;
  deleteModelProfile(
    request: P05MainParams['models.profile.delete'],
  ): Promise<ResultEnvelope<P05MainResult['models.profile.delete']>>;
  deleteModelProvider(
    request: P05MainParams['models.provider.delete'],
  ): Promise<ResultEnvelope<P05MainResult['models.provider.delete']>>;
  startModelDiscovery(
    request: P05MainParams['models.discovery.start'],
  ): Promise<ResultEnvelope<P05MainResult['models.discovery.start']>>;
  cancelModelDiscovery(
    request: P05MainParams['models.discovery.cancel'],
  ): Promise<ResultEnvelope<P05MainResult['models.discovery.cancel']>>;
  cancelModelAvailabilityTest(
    request: P05MainParams['models.availability_test.cancel'],
  ): Promise<ResultEnvelope<P05MainResult['models.availability_test.cancel']>>;
  getModelCatalogProvider(
    request: P05MainParams['models.catalog.provider.get'],
  ): Promise<ResultEnvelope<P05MainResult['models.catalog.provider.get']>>;
  createModelProfileFromCatalog(
    request: P05MainParams['models.profile.create_from_catalog'],
  ): Promise<ResultEnvelope<P05MainResult['models.profile.create_from_catalog']>>;
  applyModelProfileCatalogUpdate(
    request: P05MainParams['models.profile.apply_catalog_update'],
  ): Promise<ResultEnvelope<P05MainResult['models.profile.apply_catalog_update']>>;
  getApprovalDetails(
    request: P04MainParams['approval.get_details'],
  ): Promise<ResultEnvelope<ApprovalRequest>>;
  getHostPreparationStatus(): Promise<ResultEnvelope<HostPreparationStatus>>;
  getModelCompatibility(
    request: P05MainParams['models.compatibility.get'],
  ): Promise<ResultEnvelope<P05MainResult['models.compatibility.get']>>;
  getModelProviderCatalog(): Promise<ResultEnvelope<P05MainResult['models.catalog.get']>>;
  ensureModelProviderCatalogFresh(): Promise<
    ResultEnvelope<P05MainResult['models.catalog.ensure_fresh']>
  >;
  getModelSettings(): Promise<ResultEnvelope<P05MainResult['models.settings.get_snapshot']>>;
  getPermissionDefaults(): Promise<ResultEnvelope<P04MainResult['permission.get_defaults']>>;
  setPermissionDefaults(
    request: P04MainParams['permission.set_defaults'],
  ): Promise<ResultEnvelope<P04MainResult['permission.set_defaults']>>;
  getSessionPermissionState(
    request: P04MainParams['permission.get_session_state'],
  ): Promise<ResultEnvelope<SessionPermissionProjection>>;
  archiveWorkspace(
    request: WorkspaceArchiveCommand,
  ): Promise<ResultEnvelope<BeginWorkspaceLifecycleResult>>;
  deleteWorkspace(
    request: P03MainParams['workspace.delete'],
  ): Promise<ResultEnvelope<BeginWorkspaceLifecycleResult>>;
  acknowledgeWorkspaceLifecycle(
    request: RendererLifecycleAck,
  ): Promise<ResultEnvelope<WorkspaceLifecycleSummary>>;
  createWorkspace(request: WorkspaceCreateCommand): Promise<ResultEnvelope<WorkspaceCreateResult>>;
  selectWorkspaceDirectory(): Promise<ResultEnvelope<P03MainResult['workspace.select_directory']>>;
  copyWorkspacePath(request: FsCopyRequest): Promise<ResultEnvelope<FsMutationResult>>;
  createWorkspaceDirectory(
    request: FsCreateDirectoryRequest,
  ): Promise<ResultEnvelope<FsMutationResult>>;
  deleteWorkspacePath(request: FsDeleteRequest): Promise<ResultEnvelope<FsMutationResult>>;
  editWorkspaceBuffer(request: BufferEditCommand): Promise<ResultEnvelope<BufferEditResult>>;
  findWorkspacePaths(request: FsFindPathsRequest): Promise<ResultEnvelope<PathSearchResult>>;
  globWorkspacePaths(request: FsGlobRequest): Promise<ResultEnvelope<FsGlobResult>>;
  getRuntimeReadiness(): Promise<ResultEnvelope<RuntimeReadiness>>;
  getCoreHealth(): Promise<ResultEnvelope<CoreHealthProjectionChange>>;
  getWorkspaceFiles(): Promise<ResultEnvelope<WorkspaceFilesProjectionChange>>;
  getWorkspaceSessions(): Promise<ResultEnvelope<WorkspaceSessionProjectionChange>>;
  listPendingApprovals(): Promise<ResultEnvelope<ApprovalView[]>>;
  listPermissionGrants(
    request: P04MainParams['permission.list_grants'],
  ): Promise<ResultEnvelope<PermissionGrant[]>>;
  listQuarantine(): Promise<ResultEnvelope<QuarantineRecordDetails[]>>;
  listExternalGrants(
    request: P04MainParams['security.external_grant.list'],
  ): Promise<ResultEnvelope<P04MainResult['security.external_grant.list']>>;
  /**
   * The reusable Native session consent the user approved for one session.
   *
   * This is the authorization that outlives the Operation it was approved for, so
   * it has its own list and its own withdrawal rather than being folded into the
   * directory grants: the two have different scope, different lifetime rules and
   * different explanations.
   */
  hashWorkspaceFile(request: FsHashRequest): Promise<ResultEnvelope<FsHashResult>>;
  inspectWorkspaceFile(request: FsInspectRequest): Promise<ResultEnvelope<FsInspectResult>>;
  issueWorkspaceDiscardConfirmation(
    request: RendererIssueDiscardConfirmation,
  ): Promise<ResultEnvelope<DiscardConfirmation>>;
  listWorkspaceFiles(request: FsListRequest): Promise<ResultEnvelope<FsListResult>>;
  moveWorkspacePath(request: FsMoveRequest): Promise<ResultEnvelope<FsMutationResult>>;
  onCoreHealth(listener: (change: CoreHealthProjectionChange) => void): () => void;
  onModelProjection(listener: (change: ModelProjectionChange) => void): () => void;
  onToolProjection(listener: (change: ToolProjectionChange) => void): () => void;
  refreshToolProjection(
    request: ToolProjectionRouteRequest,
  ): Promise<ResultEnvelope<ToolProjectionChange>>;
  readToolActivityPage(
    request: ToolActivityPageRequestV1,
  ): Promise<ResultEnvelope<ToolActivityPageV1>>;
  getToolCredentialCandidates(
    request: P07ReadParams['tools.credential.candidates'],
  ): Promise<ResultEnvelope<ToolCredentialCandidatesProjectionV1>>;
  subscribeModelProjection(request: ModelProjectionRouteRequest): Promise<ResultEnvelope<void>>;
  subscribeToolProjection(request: ToolProjectionRouteRequest): Promise<ResultEnvelope<void>>;
  unsubscribeModelProjection(request: ModelProjectionRouteRequest): Promise<ResultEnvelope<void>>;
  unsubscribeToolProjection(request: ToolProjectionRouteRequest): Promise<ResultEnvelope<void>>;
  onSecurityControl(listener: (change: SecurityControlProjectionChange) => void): () => void;
  onWorkspaceFiles(listener: (change: WorkspaceFilesProjectionChange) => void): () => void;
  onWorkspaceLifecyclePrepare(
    listener: (prepare: PrepareEditorLifecycleTransition) => void,
  ): () => void;
  onWorkspaceSessions(listener: (change: WorkspaceSessionProjectionChange) => void): () => void;
  openWorkspaceBuffer(request: BufferOpenCommand): Promise<ResultEnvelope<LogicalFileStatus>>;
  openBinaryStream(request: BinaryStreamOpenRequest): Promise<ResultEnvelope<AuraBinaryStream>>;
  readWorkspaceBinaryRange(
    request: FsReadBinaryRangeRequest,
  ): Promise<ResultEnvelope<FsReadBinaryRangeResult>>;
  readWorkspaceText(request: FsReadTextRequest): Promise<ResultEnvelope<FsReadTextResult>>;
  refreshModelProviderCatalog(): Promise<ResultEnvelope<P05MainResult['models.catalog.refresh']>>;
  renameModelProvider(
    request: P05MainParams['models.provider.rename'],
  ): Promise<ResultEnvelope<P05MainResult['models.provider.rename']>>;
  /** A provider's default connection and its backups, in order. */
  setModelProviderConnections(
    request: P05MainParams['models.provider.set_connections'],
  ): Promise<ResultEnvelope<P05MainResult['models.provider.set_connections']>>;
  replaceModelCredential(
    request: P05MainParams['models.endpoint.replace_credential'],
    secret: Uint8Array,
  ): Promise<ResultEnvelope<P05MainResult['models.endpoint.replace_credential']>>;
  resetModelCompatibility(
    request: P05MainParams['models.compatibility.reset'],
  ): Promise<ResultEnvelope<P05MainResult['models.compatibility.reset']>>;
  resetModelRuntimeContextWindow(
    request: P05MainParams['models.context_window.reset_runtime_correction'],
  ): Promise<ResultEnvelope<P05MainResult['models.context_window.reset_runtime_correction']>>;
  prepareHostPreparation(
    request: P04MainParams['security.host_preparation.prepare'],
  ): Promise<ResultEnvelope<HostPreparationStatus>>;
  prepareQuarantineRelease(
    request: P04MainParams['security.quarantine.prepare_release'],
  ): Promise<ResultEnvelope<P04MainResult['security.quarantine.prepare_release']>>;
  relocateWorkspace(
    request: WorkspaceRelocateCommand,
  ): Promise<ResultEnvelope<BeginWorkspaceLifecycleResult>>;
  resolveWorkspaceLifecycle(
    request: RendererResolveWorkspaceLifecycle,
  ): Promise<ResultEnvelope<WorkspaceLifecycleSummary>>;
  restoreWorkspace(request: WorkspaceRestoreCommand): Promise<ResultEnvelope<WorkspaceSummary>>;
  saveWorkspaceBuffer(request: BufferSaveCommand): Promise<ResultEnvelope<LogicalFileStatus>>;
  resolveApproval(
    request: P04MainParams['approval.resolve'],
  ): Promise<ResultEnvelope<ApprovalResolution>>;
  removeHostPreparation(
    request: P04MainParams['security.host_preparation.remove'],
  ): Promise<ResultEnvelope<HostPreparationStatus>>;
  repairHostPreparation(
    request: P04MainParams['security.host_preparation.repair'],
  ): Promise<ResultEnvelope<HostPreparationStatus>>;
  releaseQuarantine(
    request: P04MainParams['security.quarantine.release'],
  ): Promise<ResultEnvelope<P04MainResult['security.quarantine.release']>>;
  revalidateExternalGrant(
    request: P04MainParams['security.external_grant.revalidate'],
  ): Promise<ResultEnvelope<P04MainResult['security.external_grant.revalidate']>>;
  revokeExternalGrant(
    request: P04MainParams['security.external_grant.revoke'],
  ): Promise<ResultEnvelope<P04MainResult['security.external_grant.revoke']>>;
  revokePermissionGrant(
    request: P04MainParams['permission.revoke_grant'],
  ): Promise<ResultEnvelope<PermissionGrant>>;
  issueQuarantineDiscardConfirmation(
    request: P04MainParams['security.quarantine.issue_discard_confirmation'],
  ): Promise<ResultEnvelope<P04MainResult['security.quarantine.issue_discard_confirmation']>>;
  searchWorkspaceText(request: FsSearchTextRequest): Promise<ResultEnvelope<FsSearchTextResult>>;
  statWorkspaceFile(request: FsStatRequest): Promise<ResultEnvelope<FsStatResult>>;
  setSessionPermissionMode(
    request: P04MainParams['permission.set_session_mode'],
  ): Promise<ResultEnvelope<SessionPermissionProjection>>;
  setDefaultModel(
    request: P05MainParams['models.default.set'],
  ): Promise<ResultEnvelope<P05MainResult['models.default.set']>>;
  setModelProfileSubagentAvailability(
    request: P05MainParams['models.profile.set_subagent_availability'],
  ): Promise<ResultEnvelope<P05MainResult['models.profile.set_subagent_availability']>>;
  setProviderModelsSubagentAvailability(
    request: P05MainParams['models.provider.set_all_current_models_subagent_availability'],
  ): Promise<
    ResultEnvelope<P05MainResult['models.provider.set_all_current_models_subagent_availability']>
  >;
  setSessionModelBinding(
    request: P05MainParams['models.session.set_binding'],
  ): Promise<ResultEnvelope<P05MainResult['models.session.set_binding']>>;
  testModelAvailability(
    request: P05MainParams['models.profile.test_availability'],
  ): Promise<ResultEnvelope<P05MainResult['models.profile.test_availability']>>;
  updateModelEndpoint(
    request: P05MainParams['models.endpoint.update'],
  ): Promise<ResultEnvelope<P05MainResult['models.endpoint.update']>>;
  updateModelProfile(
    request: P05MainParams['models.profile.update'],
  ): Promise<ResultEnvelope<P05MainResult['models.profile.update']>>;
  writeWorkspaceFile(request: FsWriteFileRequest): Promise<ResultEnvelope<FsMutationResult>>;
}

export interface AuraWindowApi {
  readonly v1: AuraApiV1;
}
