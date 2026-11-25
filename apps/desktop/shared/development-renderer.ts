export const DEVELOPMENT_RENDERER_URL = 'http://127.0.0.1:5175';
export const DEVELOPMENT_RENDERER_HEADER = 'x-aura-dev-renderer';

export function isDevelopmentRendererInstance(value: unknown): value is string {
  return typeof value === 'string' && /^[a-f0-9]{32}$/u.test(value);
}
