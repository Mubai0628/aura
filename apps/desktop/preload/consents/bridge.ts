import type { SessionConsentsApi } from '../../shared/api/session-consents.js';
import type { ResultEnvelope } from '../../shared/aura-api.js';
import type { P04MainCommand, P04MainParams, P04MainResult } from '../../shared/p04-validation.js';

type InvokeSecurity = <C extends P04MainCommand>(
  command: C,
  request: P04MainParams[C],
) => Promise<ResultEnvelope<P04MainResult[C]>>;

/** Consent management keeps the same registered and validated P04 transport. */
export function sessionConsentsBridge(invoke: InvokeSecurity): SessionConsentsApi {
  return {
    listNetworkSessionConsents: (request) =>
      invoke('security.network_session_consent.list', request),
    revokeNetworkSessionConsent: (request) =>
      invoke('security.network_session_consent.revoke', request),
    listNativeSessionConsents: (request) => invoke('security.native_session_consent.list', request),
    revokeNativeSessionConsent: (request) =>
      invoke('security.native_session_consent.revoke', request),
    listFileSessionConsents: (request) => invoke('security.file_session_consent.list', request),
    revokeFileSessionConsent: (request) => invoke('security.file_session_consent.revoke', request),
    listMemorySessionConsents: (request) => invoke('security.memory_session_consent.list', request),
    revokeMemorySessionConsent: (request) =>
      invoke('security.memory_session_consent.revoke', request),
  };
}
