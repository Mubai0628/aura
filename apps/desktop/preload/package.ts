import type { IpcErrorEnvelope } from '@aura/contracts/generated';
import type { ResultEnvelope } from '../shared/aura-api.js';
import type { PackageApiV1, PackageMainCommand, PackageMainResult } from '../shared/package-api.js';
import {
  isPackageCommand,
  isPackageParams,
  parsePackageResult,
} from '../shared/package-validation.js';
export function packageBridge(
  send: (command: string, params: unknown) => Promise<unknown>,
  error: (code: string, message: string) => IpcErrorEnvelope,
  isError: (value: unknown) => value is IpcErrorEnvelope,
): PackageApiV1 {
  return {
    async invoke(command, params) {
      if (!isPackageCommand(command) || !isPackageParams(command, params))
        return { ok: false, error: error('package.invalid_request', '工具包请求参数不合法') };
      let result: unknown;
      try {
        result = await send(command, params);
      } catch {
        return {
          ok: false,
          error: error(
            'package.transport_failed',
            '工具包请求未完成，请刷新后查看结果；未自动重放',
          ),
        };
      }
      return parse(command, result, error, isError);
    },
  };
}
function parse<C extends PackageMainCommand>(
  command: C,
  result: unknown,
  error: (code: string, message: string) => IpcErrorEnvelope,
  isError: (value: unknown) => value is IpcErrorEnvelope,
): ResultEnvelope<PackageMainResult[C]> {
  try {
    if (typeof result === 'object' && result !== null && 'ok' in result) {
      if (!result.ok && 'error' in result && isError(result.error))
        return { ok: false, error: result.error };
      if (result.ok && 'value' in result)
        return { ok: true, value: parsePackageResult(command, result.value) };
    }
  } catch {
    return { ok: false, error: error('package.invalid_response', '工具包回执格式不兼容') };
  }
  return { ok: false, error: error('package.invalid_response', '工具包回执格式不兼容') };
}
