import type { ResultEnvelope } from '../aura-api.js';
import type { P04MainParams, P04MainResult } from '../p04-validation.js';

export interface SessionConsentsApi {
  listNetworkSessionConsents(
    request: P04MainParams['security.network_session_consent.list'],
  ): Promise<ResultEnvelope<P04MainResult['security.network_session_consent.list']>>;
  revokeNetworkSessionConsent(
    request: P04MainParams['security.network_session_consent.revoke'],
  ): Promise<ResultEnvelope<P04MainResult['security.network_session_consent.revoke']>>;
  listNativeSessionConsents(
    request: P04MainParams['security.native_session_consent.list'],
  ): Promise<ResultEnvelope<P04MainResult['security.native_session_consent.list']>>;
  revokeNativeSessionConsent(
    request: P04MainParams['security.native_session_consent.revoke'],
  ): Promise<ResultEnvelope<P04MainResult['security.native_session_consent.revoke']>>;
  listFileSessionConsents(
    request: P04MainParams['security.file_session_consent.list'],
  ): Promise<ResultEnvelope<P04MainResult['security.file_session_consent.list']>>;
  revokeFileSessionConsent(
    request: P04MainParams['security.file_session_consent.revoke'],
  ): Promise<ResultEnvelope<P04MainResult['security.file_session_consent.revoke']>>;
  listMemorySessionConsents(
    request: P04MainParams['security.memory_session_consent.list'],
  ): Promise<ResultEnvelope<P04MainResult['security.memory_session_consent.list']>>;
  revokeMemorySessionConsent(
    request: P04MainParams['security.memory_session_consent.revoke'],
  ): Promise<ResultEnvelope<P04MainResult['security.memory_session_consent.revoke']>>;
}
