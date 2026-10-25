import { mainRoute } from './generated/routes.js';
import { validateGitRequest, validateGitResponse } from './generated/git-validator.js';
import type { GitMainCommand, GitMainParams, GitMainResult } from './git-api.js';
export const isGitCommand = (command: string): command is GitMainCommand =>
  mainRoute(command)?.owner === 'git';
export const isGitParams = <C extends GitMainCommand>(
  command: C,
  value: unknown,
): value is GitMainParams[C] => validateGitRequest({ command, value });
export function parseGitResult<C extends GitMainCommand>(
  command: C,
  value: unknown,
): GitMainResult[C] {
  if (!validateGitResponse({ command, value })) throw new Error('Invalid Git result contract');
  return value as GitMainResult[C];
}
