import type { BrowserWindow } from 'electron';
import type { AgentMainResult } from '../shared/generated/routes.js';
import { selectFolder } from './folder-dialog.js';

type Selected = AgentMainResult['agent.hooks.select_directory'];

/**
 * Settings → Hook: Main's own folder dialog. Nothing is read or trusted here; the Core
 * previews the folder and trusts only the fingerprint the user then confirms.
 */
export function selectHookDirectory(owner: BrowserWindow): Promise<Selected> {
  return selectFolder(owner, '选择 Hook 文件夹');
}
