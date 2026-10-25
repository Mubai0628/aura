export { execute } from './client.ts';
export { HttpFailure, OutcomeUnknown, TransportFailure } from './errors.ts';
export { mutating, retryDecision } from './policy.ts';
export { defaults, options, validate } from './types.ts';
export type { Dependencies, Method, Options, Request, Response, Result } from './types.ts';

import { execute } from './client.ts';
import type { Dependencies, Result } from './types.ts';

/** A convenience operation used by the order application. */
export function submitOrder(jsonBody: string, dependencies: Dependencies): Promise<Result> {
  return execute({ method: 'POST', path: '/orders', body: jsonBody }, dependencies);
}

/** Reads use the same retry and attempt reporting machinery. */
export function readOrders(dependencies: Dependencies): Promise<Result> {
  return execute({ method: 'GET', path: '/orders' }, dependencies);
}
