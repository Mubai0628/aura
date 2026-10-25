import { HttpFailure, TransportFailure } from './errors.ts';
import type { Method, Options } from './types.ts';

export interface RetryDecision {
  retry: boolean;
  delay_ms: number;
}

export function mutating(method: Method): boolean {
  return method !== 'GET';
}

export function retryDecision(
  method: Method,
  failure: unknown,
  attempt: number,
  settings: Options,
): RetryDecision {
  let retry = false;
  let requested: number | undefined;
  if (failure instanceof TransportFailure) {
    retry = failure.retryable && !(mutating(method) && failure.phase === 'after_send');
  } else if (failure instanceof HttpFailure) {
    retry = failure.status === 429 || (!mutating(method) && failure.status >= 500);
    requested = failure.retry_after_ms;
  }
  if (!retry || attempt >= settings.max_attempts) {
    return { retry: false, delay_ms: 0 };
  }
  const backoff = settings.base_delay_ms * 2 ** (attempt - 1);
  const delay = requested === undefined || !Number.isFinite(requested) ? backoff : Math.max(0, requested);
  return { retry: true, delay_ms: Math.min(delay, settings.max_delay_ms) };
}
