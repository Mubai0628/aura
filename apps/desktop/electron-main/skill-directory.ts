import { shell, type BrowserWindow } from 'electron';
import type { AgentMainParams, AgentMainResult } from '../shared/generated/routes.js';
import type { CoreSupervisor } from './core-supervisor.js';
import { isDirectory, ordinaryPath, selectFile, selectFolder } from './folder-dialog.js';

type Selected = AgentMainResult['agent.skills.select_directory'];
type SelectedArchive = AgentMainResult['agent.skills.select_archive'];
type OpenRequest = AgentMainParams['agent.skills.open_directory'];
type Opened = AgentMainResult['agent.skills.open_directory'];

/**
 * Settings → Skill: Main's own folder dialog for importing a Skill. Core checks the folder,
 * copies it into Aura's own and trusts nothing until the user confirms the shown content.
 */
export function selectSkillDirectory(owner: BrowserWindow): Promise<Selected> {
  return selectFolder(owner, '选择 Skill 文件夹（含 SKILL.md）');
}

/** Settings → Skill: Main's own file dialog for a Skill packed as a ZIP file. */
export function selectSkillArchive(owner: BrowserWindow): Promise<SelectedArchive> {
  return selectFile(owner, '选择 Skill 的 ZIP 文件', [
    { name: 'Skill（ZIP）', extensions: ['zip', 'skill'] },
  ]);
}

/**
 * Opens the folder of a Skill that Core lists, named by its id; the renderer never supplies a
 * path. A workspace Skill is found only with its workspace.
 */
export async function openSkillDirectory(
  request: OpenRequest,
  supervisor: CoreSupervisor,
): Promise<Opened> {
  const listed = await supervisor.requestAgent('agent.skills.list', {
    workspace_id: request.workspace_id ?? null,
  });
  const listedPath = listed.skills.find((skill) => skill.id === request.id)?.path;
  if (!listedPath) return { completed: false, message: '没有找到这个 Skill，请刷新后再试。' };
  const directory = ordinaryPath(listedPath);
  if (!(await isDirectory(directory)))
    return { completed: false, message: '这个 Skill 的文件夹已不存在，请刷新后再试。' };
  const failure = await shell.openPath(directory);
  return failure
    ? { completed: false, message: '无法打开 Skill 文件夹，请检查它是否仍存在与文件权限。' }
    : { completed: true, message: '已打开 Skill 文件夹' };
}
