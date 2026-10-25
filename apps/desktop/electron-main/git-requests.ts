import { CORE_NOT_READY_MESSAGE } from './core-unavailable.js';
import { shell } from 'electron';
import type { CoreSupervisor } from './core-supervisor.js';
import { parseGitResult } from '../shared/git-validation.js';
import type {
  GitCoreCommand,
  GitCoreParams,
  GitCoreResult,
  GitMainCommand,
  GitMainParams,
} from '../shared/generated/routes.js';
import type { ConfirmedRequest } from './secret-requests.js';
export function requestGitCommand<C extends GitCoreCommand>(
  command: C,
  params: GitCoreParams[C],
  confirm: ConfirmedRequest,
): Promise<GitCoreResult[C]> {
  return confirm({
    command,
    accept: (value) => parseGitResult(command, value),
    send: (transport) => transport.request(command, params, 1, 45_000),
  });
}
export async function dispatchGitCommand(
  command: GitMainCommand,
  params: GitMainParams[GitMainCommand],
  core: CoreSupervisor | undefined,
): Promise<unknown> {
  if (command === 'git.installation.open') {
    await shell.openExternal('https://git-scm.com/download/win');
    return { opened: true };
  }
  if (core === undefined) throw new Error(CORE_NOT_READY_MESSAGE);
  return core.domains.git(command, params);
}
