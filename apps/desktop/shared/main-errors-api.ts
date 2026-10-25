import type { ResultEnvelope } from './aura-api.js';
import { isRecord } from './projection-validation/common.js';

export const MAIN_ERROR_EVENT_CHANNEL = 'aura:v1:main-error';
export const MAIN_ERROR_SNAPSHOT_CHANNEL = 'aura:v1:main-error-get';
export type MainErrorKind = 'uncaught_exception' | 'unhandled_rejection';
/** Main-owned diagnostics only: no error text, stack, command or mutable authority. */
export interface MainErrorNoticeV1 {
  version: 1;
  id: string;
  kind: MainErrorKind;
  logged: boolean;
  log_path: string;
}
export interface MainErrorsApiV1 {
  snapshot(): Promise<ResultEnvelope<MainErrorNoticeV1 | null>>;
  onChange(listener: (notice: MainErrorNoticeV1) => void): () => void;
}
export function isMainErrorNotice(value: unknown): value is MainErrorNoticeV1 {
  return (
    isRecord(value) &&
    Object.keys(value).sort().join(',') === 'id,kind,log_path,logged,version' &&
    value.version === 1 &&
    typeof value.id === 'string' &&
    /^main_error_[1-9][0-9]{0,14}$/u.test(value.id) &&
    (value.kind === 'uncaught_exception' || value.kind === 'unhandled_rejection') &&
    typeof value.logged === 'boolean' &&
    typeof value.log_path === 'string' &&
    value.log_path.length > 0 &&
    value.log_path.length <= 32768 &&
    !/[\0\r\n]/u.test(value.log_path)
  );
}
