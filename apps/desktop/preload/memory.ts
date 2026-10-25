import type { IpcErrorEnvelope } from '@aura/contracts/generated';
import type { ResultEnvelope } from '../shared/aura-api.js';
import type { MemoryApiV1, MemoryMainCommand, MemoryMainResult } from '../shared/memory-api.js';
import { isMemoryCommand, isMemoryParams, parseMemoryResult } from '../shared/memory-validation.js';
export function memoryBridge(
  send: (command: string, params: unknown) => Promise<unknown>,
  error: (code: string, message: string) => IpcErrorEnvelope,
  isError: (value: unknown) => value is IpcErrorEnvelope,
): MemoryApiV1 {
  return {
    async invoke(command, params) {
      if (!isMemoryCommand(command) || !isMemoryParams(command, params))
        return { ok: false, error: error('memory.invalid_request', '记忆请求参数不合法') };
      let result: unknown;
      try {
        result = await send(command, params);
      } catch {
        return {
          ok: false,
          error: error('memory.transport_failed', '记忆请求未完成，请刷新查看结果；未自动重放'),
        };
      }
      return parse(command, result, error, isError);
    },
  };
}
function parse<C extends MemoryMainCommand>(
  command: C,
  result: unknown,
  error: (code: string, message: string) => IpcErrorEnvelope,
  isError: (value: unknown) => value is IpcErrorEnvelope,
): ResultEnvelope<MemoryMainResult[C]> {
  try {
    if (typeof result === 'object' && result !== null && 'ok' in result) {
      if (!result.ok && 'error' in result && isError(result.error))
        return { ok: false, error: result.error };
      if (result.ok && 'value' in result)
        return { ok: true, value: parseMemoryResult(command, result.value) };
    }
  } catch {
    /* Contract failures share the same bounded diagnostic below. */
  }
  return {
    ok: false,
    error: error('memory.invalid_response', '记忆回执格式不兼容，请更新或重启 Aura'),
  };
}
