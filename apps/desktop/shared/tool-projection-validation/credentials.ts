import { isAuraId, isOneOf, isPositiveRevision } from './common.js';
import { exactRecord, isRecord } from './records.js';

/** Tool credential candidates and placements. */
export const isToolCredentialCandidate = (value: unknown): boolean =>
  exactRecord(value, [
    'compatibility',
    'credential_id',
    'credential_kind',
    'credential_revision',
    'display_name',
    'scope_summary',
    'state',
  ]) &&
  isAuraId(value.credential_id, 'crd') &&
  isPositiveRevision(value.credential_revision) &&
  typeof value.display_name === 'string' &&
  value.display_name.length > 0 &&
  value.display_name.length <= 120 &&
  value.credential_kind === 'api_key' &&
  value.state === 'configured' &&
  value.compatibility === 'compatible' &&
  exactRecord(value.scope_summary, ['scope']) &&
  isOneOf(value.scope_summary.scope, ['user', 'workspace', 'session']);

export const isCredentialSlotKey = (value: unknown): value is string =>
  typeof value === 'string' && value.length <= 64 && /^[a-z0-9]+(?:[-_][a-z0-9]+)*$/u.test(value);

export const isToolCredentialPlacement = (value: unknown): boolean => {
  if (!isRecord(value) || typeof value.placement !== 'string') return false;
  switch (value.placement) {
    case 'command_environment':
      return (
        exactRecord(value, ['environment_variable', 'placement']) &&
        typeof value.environment_variable === 'string' &&
        /^[A-Z_][A-Z0-9_]{0,63}$/u.test(value.environment_variable)
      );
    case 'http_bearer_authorization':
      return exactRecord(value, ['placement']);
    case 'http_fixed_header':
      return (
        exactRecord(value, ['header_name', 'placement']) &&
        typeof value.header_name === 'string' &&
        value.header_name.length <= 128 &&
        /^[!#$%&'*+.^_`|~0-9A-Za-z-]+$/u.test(value.header_name)
      );
    default:
      return false;
  }
};
