export type SendPhase = 'before_send' | 'after_send';

export class TransportFailure extends Error {
  readonly code = 'transport_failed';
  phase: SendPhase;
  retryable: boolean;

  constructor(phase: SendPhase, retryable = true) {
    super('The connection failed');
    this.name = 'TransportFailure';
    this.phase = phase;
    this.retryable = retryable;
  }
}

export class HttpFailure extends Error {
  readonly code = 'http_failed';
  status: number;
  retry_after_ms: number | undefined;

  constructor(status: number, retry_after_ms?: number) {
    super(`The service returned HTTP ${status}`);
    this.name = 'HttpFailure';
    this.status = status;
    this.retry_after_ms = retry_after_ms;
  }
}

export class OutcomeUnknown extends Error {
  readonly code = 'outcome_unknown';
  attempts: number;

  constructor(attempts: number) {
    super('The request may have taken effect. Inspect its result before trying again.');
    this.name = 'OutcomeUnknown';
    this.attempts = attempts;
  }
}
