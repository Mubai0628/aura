import type { MemoryMainCommand, MemoryMainParams, MemoryMainResult } from './generated/routes.js';
import type { ResultEnvelope } from './aura-api.js';
/** Vault management accepts only a scope and Core-known workspace ID. */
export interface MemoryApiV1 {
  invoke<C extends MemoryMainCommand>(
    command: C,
    params: MemoryMainParams[C],
  ): Promise<ResultEnvelope<MemoryMainResult[C]>>;
}
export type { MemoryMainCommand, MemoryMainParams, MemoryMainResult };
