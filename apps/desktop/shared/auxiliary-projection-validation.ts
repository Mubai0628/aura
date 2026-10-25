import type { AuxiliaryProjectionKey } from '@aura/contracts/generated';
import { exactRecord, isAuraId, isRecord } from './projection-validation/common.js';

export const isAuxiliaryProjectionKey = (value: unknown): value is AuxiliaryProjectionKey => {
  if (
    !exactRecord(value, ['runtime_instance_id', 'projection_kind', 'projection_scope']) ||
    !isAuraId(value.runtime_instance_id, 'run')
  )
    return false;
  const scope = value.projection_scope;
  switch (value.projection_kind) {
    case 'pending_approval':
    case 'security_runtime':
      return exactRecord(scope, ['type']) && scope.type === 'global';
    case 'session_permission':
      return (
        exactRecord(scope, ['type', 'session_id']) &&
        scope.type === 'session' &&
        isAuraId(scope.session_id, 'ses')
      );
    case 'terminal_state':
      return (
        exactRecord(scope, ['type', 'session_id', 'terminal_id']) &&
        scope.type === 'terminal' &&
        isAuraId(scope.session_id, 'ses') &&
        typeof scope.terminal_id === 'string' &&
        /^term_[A-Za-z0-9_-]{1,128}$/u.test(scope.terminal_id)
      );
    default:
      return false;
  }
};

export function auxiliaryNotificationKey(
  method: string,
  value: unknown,
): AuxiliaryProjectionKey | undefined {
  if (!isRecord(value)) return undefined;
  const terminal = method === 'terminal.state.v1';
  const payload = terminal ? value.view : value.payload;
  const kind = terminal ? 'terminal_state' : value.projection_kind;
  const key = {
    runtime_instance_id: value.runtime_instance_id,
    projection_kind: kind,
    projection_scope:
      kind === 'terminal_state'
        ? {
            type: 'terminal',
            session_id: isRecord(payload) ? payload.session_id : undefined,
            terminal_id: isRecord(payload) ? payload.terminal_id : undefined,
          }
        : kind === 'session_permission'
          ? { type: 'session', session_id: isRecord(payload) ? payload.session_id : undefined }
          : { type: 'global' },
  };
  return isAuxiliaryProjectionKey(key) ? key : undefined;
}

export const isAuxiliaryRevision = (value: unknown): value is string =>
  typeof value === 'string' &&
  /^(0|[1-9][0-9]{0,19})$/u.test(value) &&
  BigInt(value) <= 18446744073709551615n;
