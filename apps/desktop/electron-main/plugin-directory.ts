import { shell, type BrowserWindow } from 'electron';
import type { AgentMainParams, AgentMainResult } from '../shared/generated/routes.js';
import type { CoreSupervisor } from './core-supervisor.js';
import { isDirectory, ordinaryPath, selectFile, selectFolder } from './folder-dialog.js';

type Selected = AgentMainResult['agent.plugins.select_directory'];
type SelectedArchive = AgentMainResult['agent.plugins.select_archive'];
type OpenRequest = AgentMainParams['agent.plugins.open_directory'];
type Opened = AgentMainResult['agent.plugins.open_directory'];

/**
 * Settings → 插件: Main's own folder dialog for importing a plugin. Core checks the folder,
 * copies it into Aura's own and trusts nothing until the user confirms what it brings.
 */
export function selectPluginDirectory(owner: BrowserWindow): Promise<Selected> {
  return selectFolder(owner, '选择插件文件夹（含 aura-plugin.toml 或 .claude-plugin）');
}

/** Settings → 插件: Main's own file dialog for a plugin packed as a ZIP file. */
export function selectPluginArchive(owner: BrowserWindow): Promise<SelectedArchive> {
  return selectFile(owner, '选择插件的 ZIP 文件', [{ name: '插件（ZIP）', extensions: ['zip'] }]);
}

/**
 * Opens the folder of an installed plugin that Core lists, named by the plugin; the renderer
 * never supplies a path.
 */
export async function openPluginDirectory(
  request: OpenRequest,
  supervisor: CoreSupervisor,
): Promise<Opened> {
  const listed = await supervisor.requestAgent('agent.plugins.list', {});
  const listedPath = listed.plugins.find((plugin) => plugin.name === request.name)?.path;
  if (!listedPath) return { completed: false, message: '没有找到这个插件，请刷新后再试。' };
  const directory = ordinaryPath(listedPath);
  if (!(await isDirectory(directory)))
    return { completed: false, message: '这个插件的文件夹已不存在，请刷新后再试。' };
  const failure = await shell.openPath(directory);
  return failure
    ? { completed: false, message: '无法打开插件文件夹，请检查它是否仍存在与文件权限。' }
    : { completed: true, message: '已打开插件文件夹' };
}
