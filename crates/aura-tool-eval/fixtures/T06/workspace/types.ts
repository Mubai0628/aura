export type Method = 'GET' | 'POST' | 'PUT' | 'DELETE';

export interface Request {
  method: Method;
  path: string;
  body?: string;
}

export interface Response {
  status: number;
  value: string;
  retry_after_ms?: number;
}

export interface Options {
  max_attempts: number;
  base_delay_ms: number;
  max_delay_ms: number;
}

export interface Dependencies {
  send: (request: Request) => Promise<Response>;
  sleep: (milliseconds: number) => Promise<void>;
}

export interface Result {
  value: string;
  attempts: number;
}

export const defaults: Options = {
  max_attempts: 3,
  base_delay_ms: 5,
  max_delay_ms: 1000,
};

export function options(overrides: Partial<Options> = {}): Options {
  const value = { ...defaults, ...overrides };
  if (!Number.isInteger(value.max_attempts) || value.max_attempts < 1 || value.max_attempts > 5) {
    throw new RangeError('max_attempts must be an integer between 1 and 5');
  }
  for (const name of ['base_delay_ms', 'max_delay_ms'] as const) {
    if (!Number.isFinite(value[name]) || value[name] < 0) {
      throw new RangeError(`${name} must be nonnegative`);
    }
  }
  return value;
}

export function validate(request: Request): void {
  if (!['GET', 'POST', 'PUT', 'DELETE'].includes(request.method)) {
    throw new TypeError('unsupported method');
  }
  if (!request.path.startsWith('/') || request.path.startsWith('//')) {
    throw new TypeError('a service-relative path is required');
  }
}
