import { mkdir } from 'node:fs/promises';
import { shell, type BrowserWindow, type IpcMainInvokeEvent } from 'electron';
import type {
  AgentMainCommand,
  AgentMainParams,
  AgentMainResult,
} from '../shared/generated/routes.js';
import type { CoreSupervisor } from './core-supervisor.js';
import { isDirectory, ordinaryPath } from './folder-dialog.js';
import { selectHookDirectory } from './hook-directory.js';
import { openSkillDirectory, selectSkillArchive, selectSkillDirectory } from './skill-directory.js';
import {
  openPluginDirectory,
  selectPluginArchive,
  selectPluginDirectory,
} from './plugin-directory.js';
import { signInMcpServer } from './mcp-sign-in.js';
import { writeClipboard } from './clipboard.js';
import { openExternalLink, openWorkspaceFileOutside } from './open-outside.js';

type Request = AgentMainParams['agent.commands.open_directory'];
type Opened = AgentMainResult['agent.commands.open_directory'];

/**
 * Opens a folder that Core's command catalog names; the renderer never supplies a path.
 * Aura creates its own commands folder on demand, but never creates folders in a project.
 */
export async function openCommandDirectory(
  request: Request,
  supervisor: CoreSupervisor,
): Promise<Opened> {
  const catalog = await supervisor.requestAgent('agent.commands.catalog', {
    workspace_id: request.workspace_id ?? null,
  });
  const name = request.name ?? null;
  let directory: string | null | undefined;
  let missing = '命令目录已不存在，请刷新后再试。';
  if (name !== null) {
    directory = catalog.commands.find(
      (command) => command.source === request.source && command.name === name,
    )?.location;
    if (!directory) return { completed: false, message: '没有找到这个命令，请刷新后再试。' };
  } else if (request.source === 'user') {
    directory = catalog.user_directory;
    await mkdir(directory, { recursive: true });
  } else if (request.source === 'workspace') {
    directory = catalog.workspace_directory;
    if (!directory) return { completed: false, message: '请先选择工作区。' };
    missing = '此工作区还没有 .aura/commands 目录；在项目里新建该目录后即可添加命令。';
  } else {
    return { completed: false, message: '内置命令没有目录。' };
  }
  const folder = ordinaryPath(directory);
  if (!(await isDirectory(folder))) return { completed: false, message: missing };
  const failure = await shell.openPath(folder);
  return failure
    ? { completed: false, message: '无法打开命令目录，请检查目录是否仍存在与文件权限。' }
    : { completed: true, message: '已打开命令目录' };
}

/**
 * Agent routes: Main serves `agent.commands.open_directory`, the Hook, Skill and plugin dialogs,
 * opening a listed Skill's or plugin's folder, starting an MCP sign-in and opening a
 * workspace file or a web link outside Aura itself; Core serves the rest.
 */
export function agentRouteHandler(
  supervisor: CoreSupervisor,
  owner: (event: IpcMainInvokeEvent) => BrowserWindow,
  /** The browser host reads the settings again from every reply that carries them. */
  browserSettings?: (snapshot: unknown) => void,
) {
  type Served = (
    event: IpcMainInvokeEvent,
    params: AgentMainParams[AgentMainCommand],
  ) => Promise<unknown>;
  const served: Partial<Record<AgentMainCommand, Served>> = {
    'agent.commands.open_directory': (_event, params) =>
      openCommandDirectory(params as Request, supervisor),
    'agent.hooks.select_directory': (event) => selectHookDirectory(owner(event)),
    'agent.skills.select_directory': (event) => selectSkillDirectory(owner(event)),
    'agent.skills.select_archive': (event) => selectSkillArchive(owner(event)),
    'agent.skills.open_directory': (_event, params) =>
      openSkillDirectory(params as AgentMainParams['agent.skills.open_directory'], supervisor),
    'agent.plugins.select_directory': (event) => selectPluginDirectory(owner(event)),
    'agent.plugins.select_archive': (event) => selectPluginArchive(owner(event)),
    'agent.plugins.open_directory': (_event, params) =>
      openPluginDirectory(params as AgentMainParams['agent.plugins.open_directory'], supervisor),
    'agent.mcp.oauth.sign_in': (_event, params) =>
      signInMcpServer(params as AgentMainParams['agent.mcp.oauth.sign_in'], supervisor),
    'agent.files.open_outside': (_event, params) =>
      openWorkspaceFileOutside(params as AgentMainParams['agent.files.open_outside'], supervisor),
    'agent.links.open': (_event, params) =>
      openExternalLink(params as AgentMainParams['agent.links.open']),
    'agent.clipboard.write': async (_event, params) =>
      writeClipboard(params as AgentMainParams['agent.clipboard.write']),
    'browser.settings.get': async (_event, params) => {
      const snapshot = await supervisor.requestAgent('browser.settings.get', params as never);
      browserSettings?.(snapshot);
      return snapshot;
    },
    'browser.settings.update': async (_event, params) => {
      const snapshot = await supervisor.requestAgent('browser.settings.update', params as never);
      browserSettings?.(snapshot);
      return snapshot;
    },
  };
  return (
    event: IpcMainInvokeEvent,
    command: AgentMainCommand,
    params: AgentMainParams[AgentMainCommand],
  ): Promise<unknown> => {
    const serve = served[command];
    return serve ? serve(event, params) : supervisor.requestAgent(command, params);
  };
}
