import type {
  TerminalOpeningStartV1,
  TerminalOpeningRefV1,
  TerminalOpeningChangeV1,
  TerminalOpeningProgressV1,
  CancelDisposition,
} from '@aura/contracts/generated';
import { isAgentParams } from './agent-validation.js';
import { exactRecord, isAuraId } from './projection-validation/common.js';
import {
  isUnconfirmedCommandOutcome,
  REQUEST_TIMEOUT_CODE,
  type IpcFailureLike,
} from './ipc-request-failure.js';
const token = (value: unknown): value is string =>
  typeof value === 'string' &&
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/u.test(value);
const phase = (value: unknown) =>
  value === 'waiting_runtime' ||
  value === 'waiting_launch' ||
  value === 'launching' ||
  value === 'cancelled';
export const isTerminalOpeningStart = (value: unknown): value is TerminalOpeningStartV1 =>
  exactRecord(value, ['contract_version', 'opening_id', 'request']) &&
  value.contract_version === 1 &&
  token(value.opening_id) &&
  isAgentParams('terminal.user.open', value.request);
export const isTerminalOpeningRef = (value: unknown): value is TerminalOpeningRefV1 =>
  exactRecord(value, ['contract_version', 'opening_id']) &&
  value.contract_version === 1 &&
  token(value.opening_id);
export const isTerminalOpeningChange = (value: unknown): value is TerminalOpeningChangeV1 =>
  exactRecord(value, [
    'contract_version',
    'opening_id',
    'runtime_instance_id',
    'session_id',
    'phase',
  ]) &&
  value.contract_version === 1 &&
  token(value.opening_id) &&
  isAuraId(value.runtime_instance_id, 'run') &&
  isAuraId(value.session_id, 'ses') &&
  phase(value.phase);
export const isTerminalOpeningProgress = (value: unknown): value is TerminalOpeningProgressV1 =>
  exactRecord(value, [
    'contract_version',
    'request_id',
    'runtime_instance_id',
    'session_id',
    'phase',
  ]) &&
  value.contract_version === 1 &&
  typeof value.request_id === 'string' &&
  /^cor_[0-7][0-9A-HJKMNP-TV-Z]{25}$/u.test(value.request_id) &&
  isAuraId(value.runtime_instance_id, 'run') &&
  isAuraId(value.session_id, 'ses') &&
  phase(value.phase);
export const isCancelDisposition = (value: unknown): value is CancelDisposition =>
  value === 'accepted' || value === 'too_late_to_cancel' || value === 'unknown_request';
/** A Core cancelled terminal or locally withdrawn opening is proof; a caller timeout is not. */
export const isConfirmedOpeningCancellation = (error: IpcFailureLike): boolean =>
  error.terminal === 'cancelled' &&
  error.code !== REQUEST_TIMEOUT_CODE &&
  !isUnconfirmedCommandOutcome(error);
