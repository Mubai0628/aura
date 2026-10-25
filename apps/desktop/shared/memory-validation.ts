import { mainRoute } from './generated/routes.js';
import type {
  MemoryMainCommand,
  MemoryMainParams,
  MemoryMainResult,
  MemoryCoreCommand,
  MemoryCoreResult,
} from './generated/routes.js';
import { validateMemoryRequest, validateMemoryResponse } from './generated/memory-validator.js';
export const isMemoryCommand = (command: string): command is MemoryMainCommand =>
  mainRoute(command)?.owner === 'memory';
export const isMemoryParams = <C extends MemoryMainCommand>(
  command: C,
  value: unknown,
): value is MemoryMainParams[C] => validateMemoryRequest({ command, value });
export function parseMemoryResult<C extends MemoryMainCommand>(
  command: C,
  value: unknown,
): MemoryMainResult[C] {
  if (!validateMemoryResponse({ command, value }))
    throw new Error('Invalid memory response contract');
  return value as MemoryMainResult[C];
}
export function parseMemoryCoreResult<C extends MemoryCoreCommand>(
  command: C,
  value: unknown,
): MemoryCoreResult[C] {
  if (!validateMemoryResponse({ command, value }))
    throw new Error('Invalid memory response contract');
  return value as MemoryCoreResult[C];
}
