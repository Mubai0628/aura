import { isAuraId, recordWithOptional } from './common.js';

// Mirrors CatalogDecimal: no sign, exponent, leading zeroes or trailing fractional zeroes.
const DECIMAL = /^(?:0|[1-9][0-9]*)(?:\.[0-9]*[1-9])?$/u;
const WORD = /^[A-Za-z0-9._-]{1,64}$/u;

/** Whether `value` may name a speed mode; mirrors is_speed_mode_word. */
export const isSpeedModeName = (value: unknown): value is string =>
  typeof value === 'string' && WORD.test(value);

const isDecimal = (value: unknown): boolean =>
  value === undefined || value === null || (typeof value === 'string' && DECIMAL.test(value));

const isPrice = (value: unknown): boolean =>
  value === undefined ||
  value === null ||
  (recordWithOptional(
    value,
    ['currency', 'unit'],
    ['cache_read', 'cache_write', 'input', 'output'],
  ) &&
    value.currency === 'usd' &&
    value.unit === 'per_million_tokens' &&
    isDecimal(value.input) &&
    isDecimal(value.output) &&
    isDecimal(value.cache_read) &&
    isDecimal(value.cache_write));

const unique = (values: unknown[]): boolean => new Set(values).size === values.length;

const isMode = (value: unknown): boolean =>
  recordWithOptional(value, ['name'], ['price']) &&
  isSpeedModeName(value.name) &&
  isPrice(value.price);

const isModelSpeedModes = (value: unknown): boolean =>
  recordWithOptional(value, ['model_profile_id', 'modes'], ['standard_price']) &&
  isAuraId(value.model_profile_id, 'mdl') &&
  isPrice(value.standard_price) &&
  Array.isArray(value.modes) &&
  value.modes.every(isMode) &&
  unique(value.modes.map((mode) => mode.name));

/** The speed modes each model offers, one entry per model, or none listed. */
export const areModelSpeedModes = (value: unknown): boolean =>
  value === undefined ||
  (Array.isArray(value) &&
    value.every(isModelSpeedModes) &&
    unique(value.map((model) => model.model_profile_id)));
