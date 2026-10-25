import type { RuntimeReadiness } from '@aura/contracts/generated';
import validate from './generated/readiness-validator.js';

export function parseRuntimeReadiness(value: unknown): RuntimeReadiness {
  if (!validate(value)) throw new Error('Invalid runtime readiness contract');
  return value as RuntimeReadiness;
}
