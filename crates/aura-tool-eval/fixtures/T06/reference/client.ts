import { HttpFailure, OutcomeUnknown, TransportFailure } from './errors.ts';
import { mutating, retryDecision } from './policy.ts';
import { options, validate } from './types.ts';
import type { Dependencies, Options, Request, Result } from './types.ts';

/** A transport is injected so callers can test the client without a network. */
export async function execute(
  request: Request,
  dependencies: Dependencies,
  overrides: Partial<Options> = {},
): Promise<Result> {
  validate(request);
  const settings = options(overrides);
  for (let attempt = 1; attempt <= settings.max_attempts; attempt += 1) {
    try {
      const response = await dependencies.send({ ...request });
      if (response.status < 200 || response.status >= 300) {
        throw new HttpFailure(response.status, response.retry_after_ms);
      }
      return { value: response.value, attempts: attempt };
    } catch (failure) {
      if (mutating(request.method) && (
        (failure instanceof TransportFailure && failure.phase === 'after_send') ||
        (failure instanceof HttpFailure && failure.status >= 500)
      )) {
        throw new OutcomeUnknown(attempt);
      }
      const decision = retryDecision(request.method, failure, attempt, settings);
      if (!decision.retry) {
        throw failure;
      }
      await dependencies.sleep(decision.delay_ms);
    }
  }
  throw new Error('unreachable attempt limit');
}
