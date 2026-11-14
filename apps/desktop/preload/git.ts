import type { IpcErrorEnvelope } from '@aura/contracts/generated';
import type { ResultEnvelope } from '../shared/aura-api.js';
import type { GitApiV1, GitMainCommand, GitMainParams, GitMainResult } from '../shared/git-api.js';
import { isGitParams, parseGitResult } from '../shared/git-validation.js';
export function gitBridge(
  send: (command: string, params: unknown) => Promise<unknown>,
  error: (code: string, message: string) => IpcErrorEnvelope,
  isError: (value: unknown) => value is IpcErrorEnvelope,
): GitApiV1 {
  async function invoke<C extends GitMainCommand>(
    command: C,
    params: GitMainParams[C],
  ): Promise<ResultEnvelope<GitMainResult[C]>> {
    if (!isGitParams(command, params))
      return { ok: false, error: error('git.invalid_request', 'Git 请求参数不合法') };
    let result: unknown;
    try {
      result = await send(command, params);
    } catch {
      return {
        ok: false,
        error: error('git.transport_failed', 'Git 请求未完成，请重试读取；未自动重放'),
      };
    }
    try {
      if (typeof result === 'object' && result !== null && 'ok' in result) {
        if (!result.ok && 'error' in result && isError(result.error))
          return { ok: false, error: result.error };
        if (result.ok && 'value' in result)
          return { ok: true, value: parseGitResult(command, result.value) };
      }
    } catch {
      return {
        ok: false,
        error: error(
          'git.invalid_response',
          'Aura 界面和 Core 的版本对不上，这次 Git 操作的结果读不懂；请重启 Aura，仍然如此请重新运行安装程序修复。',
        ),
      };
    }
    return {
      ok: false,
      error: error(
        'git.invalid_response',
        'Aura 界面和 Core 的版本对不上，这次 Git 操作的结果读不懂；请重启 Aura，仍然如此请重新运行安装程序修复。',
      ),
    };
  }
  return {
    runtime: () => invoke('git.runtime.get', {}),
    read: (request) => invoke('git.workspace.read', request),
    openInstallation: () => invoke('git.installation.open', {}),
  };
}
