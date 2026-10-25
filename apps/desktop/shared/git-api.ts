import type { GitMainCommand, GitMainParams, GitMainResult } from './generated/routes.js';
import type { ResultEnvelope } from './aura-api.js';
export interface GitApiV1 {
  runtime: () => Promise<ResultEnvelope<GitMainResult['git.runtime.get']>>;
  read: (
    request: GitMainParams['git.workspace.read'],
  ) => Promise<ResultEnvelope<GitMainResult['git.workspace.read']>>;
  openInstallation: () => Promise<ResultEnvelope<GitMainResult['git.installation.open']>>;
}
export type { GitMainCommand, GitMainParams, GitMainResult };
