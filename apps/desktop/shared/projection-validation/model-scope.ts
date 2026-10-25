import { isRecord } from './common.js';

// Mirrors ProjectionChange::validate's existing Model payload/route identities.
const identities = new Map([
  ['endpoint_detail', 'endpoint_id'],
  ['model_profile', 'model_profile_id'],
  ['session_model_selector', 'session_id'],
  ['model_availability_test', 'model_availability_test_id'],
  ['model_discovery', 'model_discovery_run_id'],
  ['context_window_correction', 'session_id'],
  ['prompt_inspector', 'session_id'],
]);

export const projectionPayloadMatchesScope = (
  kind: unknown,
  scope: unknown,
  payload: unknown,
): boolean => {
  const field = typeof kind === 'string' ? identities.get(kind) : undefined;
  if (field === undefined) return true;
  return (
    isRecord(scope) &&
    isRecord(payload) &&
    typeof scope[field] === 'string' &&
    payload[field] === scope[field]
  );
};
