import type { ProjectionChange } from '@aura/contracts/generated';
import type { P05MainCommand, P05MainParams, P05SecretMainCommand } from './generated/routes.js';
import { mainRoute } from './generated/routes.js';
import { validateManualModelProfileInput } from './generated/agent-validator.js';
import { REASONING_EFFORTS } from './projection-validation/common.js';
export type {
  P05MainCommand,
  P05MainParams,
  P05MainResult,
  P05NonSecretMainCommand,
  P05SecretMainCommand,
} from './generated/routes.js';

export type ModelProjectionChange = Extract<
  ProjectionChange,
  {
    projection_kind:
      | 'context_window_correction'
      | 'endpoint_detail'
      | 'model_availability_test'
      | 'model_discovery'
      | 'model_profile'
      | 'model_settings'
      | 'prompt_inspector'
      | 'provider_catalog'
      | 'session_model_selector';
  }
>;

export const isP05MainCommand = (value: string): value is P05MainCommand =>
  mainRoute(value)?.owner === 'model';

export const isP05SecretMainCommand = (value: string): value is P05SecretMainCommand =>
  mainRoute(value)?.auth === 'secret_input';

export const isValidApiKeyBytes = (value: unknown): value is Uint8Array => {
  if (!isUint8ArrayView(value) || value.byteLength < 1 || value.byteLength > 2_560) {
    return false;
  }
  let index = 0;
  while (index < value.byteLength) {
    const first = value[index];
    if (first === undefined) return false;
    if (first <= 0x7f) {
      if (first <= 0x1f || first === 0x7f) return false;
      index += 1;
      continue;
    }
    const second = value[index + 1];
    if (first >= 0xc2 && first <= 0xdf) {
      if (!isContinuationByte(second)) return false;
      index += 2;
      continue;
    }
    const third = value[index + 2];
    if (first >= 0xe0 && first <= 0xef) {
      if (!isContinuationByte(second) || !isContinuationByte(third)) return false;
      if (first === 0xe0 && second < 0xa0) return false;
      if (first === 0xed && second > 0x9f) return false;
      if (first === 0xe2 && second === 0x80 && (third === 0xa8 || third === 0xa9)) return false;
      index += 3;
      continue;
    }
    const fourth = value[index + 3];
    if (first >= 0xf0 && first <= 0xf4) {
      if (
        !isContinuationByte(second) ||
        !isContinuationByte(third) ||
        !isContinuationByte(fourth)
      ) {
        return false;
      }
      if (first === 0xf0 && second < 0x90) return false;
      if (first === 0xf4 && second > 0x8f) return false;
      index += 4;
      continue;
    }
    return false;
  }
  return true;
};

const isUint8ArrayView = (value: unknown): value is Uint8Array =>
  ArrayBuffer.isView(value) && Object.prototype.toString.call(value) === '[object Uint8Array]';

const isContinuationByte = (value: number | undefined): value is number =>
  value !== undefined && value >= 0x80 && value <= 0xbf;

export const isP05MainParams = <Command extends P05MainCommand>(
  command: Command,
  value: unknown,
): value is P05MainParams[Command] => {
  switch (command) {
    case 'provider_icon.read':
      return (
        exactRecord(value, ['artifact_id', 'content_hash']) &&
        isAuraId(value.artifact_id, 'art') &&
        isContentHash(value.content_hash)
      );
    case 'models.catalog.get':
    case 'models.catalog.ensure_fresh':
    case 'models.catalog.refresh':
    case 'models.settings.get_snapshot':
      return exactRecord(value, []);
    case 'models.catalog.provider.get':
      return (
        recordWithOptional(
          value,
          ['catalog_provider_id'],
          ['after_model_id', 'expected_catalog_revision', 'limit', 'query'],
        ) &&
        typeof value.catalog_provider_id === 'string' &&
        isOptionalNullableString(value.after_model_id) &&
        (value.expected_catalog_revision === undefined ||
          value.expected_catalog_revision === null ||
          isRevision(value.expected_catalog_revision)) &&
        (value.limit === undefined ||
          value.limit === null ||
          (isSafeUnsigned(value.limit) && value.limit >= 1 && value.limit <= 100)) &&
        isOptionalNullableString(value.query)
      );
    case 'models.provider.create':
      return (
        exactRecord(value, ['expected_revision', 'input']) &&
        isRevision(value.expected_revision) &&
        isProviderCreateInput(value.input)
      );
    case 'models.provider.rename':
      return (
        exactRecord(value, ['display_name', 'expected_revision', 'provider_id']) &&
        typeof value.display_name === 'string' &&
        isRevision(value.expected_revision) &&
        isAuraId(value.provider_id, 'prv')
      );
    // A provider's default connection and its backups, each named once.
    case 'models.provider.set_connections':
      return (
        exactRecord(value, [
          'backup_endpoint_ids',
          'default_endpoint_id',
          'expected_revision',
          'provider_id',
        ]) &&
        isRevision(value.expected_revision) &&
        isAuraId(value.provider_id, 'prv') &&
        (value.default_endpoint_id === null || isAuraId(value.default_endpoint_id, 'end')) &&
        Array.isArray(value.backup_endpoint_ids) &&
        value.backup_endpoint_ids.length <= 64 &&
        value.backup_endpoint_ids.every((id) => isAuraId(id, 'end')) &&
        new Set(value.backup_endpoint_ids).size === value.backup_endpoint_ids.length
      );
    case 'models.provider.delete':
      return isRevisionIdRequest(value, 'provider_id', 'prv');
    case 'models.endpoint.create':
    case 'models.endpoint.create_with_credential':
      return isEndpointCreateRequest(value);
    case 'models.endpoint.update':
      return isEndpointUpdateRequest(value);
    case 'models.endpoint.delete':
      return isRevisionIdRequest(value, 'endpoint_id', 'end');
    case 'models.discovery.start':
      return (
        exactRecord(value, [
          'endpoint_id',
          'expected_endpoint_revision',
          'expected_models_revision',
        ]) &&
        isAuraId(value.endpoint_id, 'end') &&
        isRevision(value.expected_endpoint_revision) &&
        isRevision(value.expected_models_revision)
      );
    case 'models.discovery.cancel':
      return (
        exactRecord(value, ['expected_operational_revision', 'model_discovery_run_id']) &&
        isRevision(value.expected_operational_revision) &&
        isAuraId(value.model_discovery_run_id, 'mdr')
      );
    case 'models.endpoint.configure_credential':
    case 'models.endpoint.replace_credential':
    case 'models.endpoint.clear_credential':
      return isCredentialMutationRequest(value);
    case 'models.profile.test_availability':
      return (
        exactRecord(value, [
          'expected_endpoint_revision',
          'expected_model_profile_revision',
          'model_profile_id',
        ]) &&
        isRevision(value.expected_endpoint_revision) &&
        isRevision(value.expected_model_profile_revision) &&
        isAuraId(value.model_profile_id, 'mdl')
      );
    case 'models.availability_test.cancel':
      return (
        exactRecord(value, ['expected_operational_revision', 'model_availability_test_id']) &&
        isRevision(value.expected_operational_revision) &&
        isAuraId(value.model_availability_test_id, 'mav')
      );
    case 'models.profile.create_manual':
      return (
        exactRecord(value, ['expected_revision', 'input']) &&
        isRevision(value.expected_revision) &&
        isManualProfileInput(value.input)
      );
    case 'models.profile.create_from_catalog':
      return (
        exactRecord(value, [
          'catalog_model_id',
          'catalog_provider_id',
          'endpoint_id',
          'expected_catalog_object_hash',
          'expected_catalog_revision',
          'expected_endpoint_revision',
          'expected_models_revision',
        ]) &&
        typeof value.catalog_model_id === 'string' &&
        typeof value.catalog_provider_id === 'string' &&
        isAuraId(value.endpoint_id, 'end') &&
        isContentHash(value.expected_catalog_object_hash) &&
        isRevision(value.expected_catalog_revision) &&
        isRevision(value.expected_endpoint_revision) &&
        isRevision(value.expected_models_revision)
      );
    case 'models.profile.apply_catalog_update':
      return (
        exactRecord(value, [
          'expected_catalog_object_hash',
          'expected_catalog_revision',
          'expected_model_profile_revision',
          'expected_models_revision',
          'model_profile_id',
        ]) &&
        isContentHash(value.expected_catalog_object_hash) &&
        isRevision(value.expected_catalog_revision) &&
        isRevision(value.expected_model_profile_revision) &&
        isRevision(value.expected_models_revision) &&
        isAuraId(value.model_profile_id, 'mdl')
      );
    case 'models.profile.update':
      return isProfileUpdateRequest(value);
    case 'models.profile.delete':
      return isRevisionIdRequest(value, 'model_profile_id', 'mdl');
    case 'models.profile.set_subagent_availability':
      return (
        exactRecord(value, ['available_for_subagents', 'expected_revision', 'model_profile_id']) &&
        typeof value.available_for_subagents === 'boolean' &&
        isRevision(value.expected_revision) &&
        isAuraId(value.model_profile_id, 'mdl')
      );
    case 'models.provider.set_all_current_models_subagent_availability':
      return (
        exactRecord(value, ['available_for_subagents', 'expected_revision', 'provider_id']) &&
        typeof value.available_for_subagents === 'boolean' &&
        isRevision(value.expected_revision) &&
        isAuraId(value.provider_id, 'prv')
      );
    case 'models.default.set':
      return (
        exactRecord(value, ['expected_revision', 'model_profile_id']) &&
        isRevision(value.expected_revision) &&
        (value.model_profile_id === null || isAuraId(value.model_profile_id, 'mdl'))
      );
    case 'models.session.set_binding':
      return (
        exactRecord(value, ['expected_revision', 'model_profile_id', 'session_id']) &&
        isRevision(value.expected_revision) &&
        (value.model_profile_id === null || isAuraId(value.model_profile_id, 'mdl')) &&
        isAuraId(value.session_id, 'ses')
      );
    case 'models.context_window.correct':
      return (
        exactRecord(value, ['expected_revision', 'model_profile_id', 'window_tokens']) &&
        isRevision(value.expected_revision) &&
        isAuraId(value.model_profile_id, 'mdl') &&
        isSafeUnsigned(value.window_tokens)
      );
    case 'models.context_window.clear_user_override':
    case 'models.context_window.reset_runtime_correction':
      return isRevisionIdRequest(value, 'model_profile_id', 'mdl');
    case 'models.compatibility.get':
      return exactRecord(value, ['endpoint_id']) && isAuraId(value.endpoint_id, 'end');
    case 'models.compatibility.reset':
      return isRevisionIdRequest(value, 'endpoint_id', 'end');
  }
};

const isProviderCreateInput = (value: unknown): boolean => {
  if (!exactRecord(value, ['display_name', 'source'])) return false;
  if (typeof value.display_name !== 'string') return false;
  if (!isRecord(value.source) || typeof value.source.type !== 'string') return false;
  if (value.source.type === 'models_dev') {
    return (
      exactRecord(value.source, ['catalog_provider_id', 'type']) &&
      typeof value.source.catalog_provider_id === 'string'
    );
  }
  return (
    value.source.type === 'custom' &&
    recordWithOptional(value.source, ['type'], ['catalog_provider_id']) &&
    isOptionalNullableString(value.source.catalog_provider_id)
  );
};

const isEndpointCreateRequest = (value: unknown): boolean =>
  recordWithOptional(
    value,
    [
      'api_format',
      'auth_scheme',
      'base_url',
      'discovery_mode',
      'display_name',
      'expected_revision',
      'max_parallel_model_requests',
      'provider_id',
      'timeouts',
    ],
    [
      'anthropic_workspace_id',
      'chat_instruction_role',
      'chat_output_token_field',
      'openai_organization_id',
      'openai_project_id',
    ],
  ) &&
  isEndpointFields(value) &&
  isRevision(value.expected_revision) &&
  isAuraId(value.provider_id, 'prv');

const isEndpointUpdateRequest = (value: unknown): boolean =>
  recordWithOptional(
    value,
    [
      'api_format',
      'auth_scheme',
      'base_url',
      'discovery_mode',
      'display_name',
      'endpoint_id',
      'expected_endpoint_revision',
      'expected_models_revision',
      'max_parallel_model_requests',
      'timeouts',
    ],
    [
      'anthropic_workspace_id',
      'chat_instruction_role',
      'chat_output_token_field',
      'openai_organization_id',
      'openai_project_id',
    ],
  ) &&
  isEndpointFields(value) &&
  isRevision(value.expected_endpoint_revision) &&
  isRevision(value.expected_models_revision) &&
  isAuraId(value.endpoint_id, 'end');

const isEndpointFields = (value: Record<string, unknown>): boolean =>
  (value.api_format === 'open_ai_responses' ||
    value.api_format === 'open_ai_chat_completions' ||
    value.api_format === 'anthropic_messages') &&
  (value.auth_scheme === 'none' ||
    value.auth_scheme === 'open_ai_bearer_api_key' ||
    value.auth_scheme === 'anthropic_api_key') &&
  typeof value.base_url === 'string' &&
  isDiscoveryMode(value.discovery_mode) &&
  typeof value.display_name === 'string' &&
  isSafeUnsigned(value.max_parallel_model_requests) &&
  isTimeouts(value.timeouts) &&
  isOptionalNullableString(value.anthropic_workspace_id) &&
  (value.chat_instruction_role === undefined ||
    value.chat_instruction_role === null ||
    value.chat_instruction_role === 'system' ||
    value.chat_instruction_role === 'developer') &&
  (value.chat_output_token_field === undefined ||
    value.chat_output_token_field === null ||
    value.chat_output_token_field === 'max_completion_tokens' ||
    value.chat_output_token_field === 'max_tokens') &&
  isOptionalNullableString(value.openai_organization_id) &&
  isOptionalNullableString(value.openai_project_id);

const isDiscoveryMode = (value: unknown): boolean =>
  isRecord(value) &&
  ((value.type === 'automatic' && exactRecord(value, ['type'])) ||
    (value.type === 'disabled' && exactRecord(value, ['type'])) ||
    (value.type === 'custom' &&
      exactRecord(value, ['path_and_query', 'type']) &&
      typeof value.path_and_query === 'string'));

const isTimeouts = (value: unknown): boolean =>
  recordWithOptional(value, ['connect_ms', 'first_event_ms', 'stream_idle_ms'], ['total_ms']) &&
  isSafeUnsigned(value.connect_ms) &&
  isSafeUnsigned(value.first_event_ms) &&
  isSafeUnsigned(value.stream_idle_ms) &&
  (value.total_ms === undefined || value.total_ms === null || isSafeUnsigned(value.total_ms));

const isCredentialMutationRequest = (value: unknown): boolean =>
  exactRecord(value, [
    'credential_id',
    'credential_slot',
    'endpoint_id',
    'expected_models_revision',
    'expected_row_revision',
  ]) &&
  isAuraId(value.credential_id, 'crd') &&
  value.credential_slot === 'api-key' &&
  isAuraId(value.endpoint_id, 'end') &&
  isRevision(value.expected_models_revision) &&
  isRevision(value.expected_row_revision);

const isManualProfileInput = (value: unknown): boolean => validateManualModelProfileInput(value);

const isProfileUpdateRequest = (value: unknown): boolean =>
  recordWithOptional(
    value,
    [
      'display_name',
      'enabled',
      'expected_revision',
      'historical_snapshot_max_tokens',
      'model_profile_id',
      'request_defaults',
    ],
    ['notes'],
  ) &&
  typeof value.display_name === 'string' &&
  typeof value.enabled === 'boolean' &&
  isRevision(value.expected_revision) &&
  isSafeUnsigned(value.historical_snapshot_max_tokens) &&
  isAuraId(value.model_profile_id, 'mdl') &&
  isOptionalNullableString(value.notes) &&
  isRequestDefaults(value.request_defaults);

const isRequestDefaults = (value: unknown): boolean =>
  recordWithOptional(
    value,
    [],
    [
      'chat_instruction_role',
      'chat_output_token_field',
      'image_input',
      'max_output_tokens',
      'reasoning_effort',
      'reasoning_efforts',
      'temperature',
      'top_p',
    ],
  ) &&
  // Whether the model is sent images; none takes the catalog's.
  (value.image_input === undefined ||
    value.image_input === null ||
    typeof value.image_input === 'boolean') &&
  (value.reasoning_effort === undefined ||
    value.reasoning_effort === null ||
    REASONING_EFFORTS.has(value.reasoning_effort)) &&
  // The levels the model offers, when its user listed them.
  (value.reasoning_efforts === undefined ||
    value.reasoning_efforts === null ||
    (Array.isArray(value.reasoning_efforts) &&
      value.reasoning_efforts.length <= 7 &&
      value.reasoning_efforts.every((level) => REASONING_EFFORTS.has(level)))) &&
  (value.chat_instruction_role === undefined ||
    value.chat_instruction_role === null ||
    value.chat_instruction_role === 'system' ||
    value.chat_instruction_role === 'developer') &&
  (value.chat_output_token_field === undefined ||
    value.chat_output_token_field === null ||
    value.chat_output_token_field === 'max_completion_tokens' ||
    value.chat_output_token_field === 'max_tokens') &&
  (value.max_output_tokens === undefined ||
    value.max_output_tokens === null ||
    isSafeUnsigned(value.max_output_tokens)) &&
  isOptionalNullableFiniteNumber(value.temperature) &&
  isOptionalNullableFiniteNumber(value.top_p);

const isRevisionIdRequest = (
  value: unknown,
  idKey: 'endpoint_id' | 'model_profile_id' | 'provider_id',
  prefix: 'end' | 'mdl' | 'prv',
): boolean =>
  exactRecord(value, ['expected_revision', idKey]) &&
  isRevision(value.expected_revision) &&
  isAuraId(value[idKey], prefix);

const isOptionalNullableString = (value: unknown): boolean =>
  value === undefined || value === null || typeof value === 'string';

const isOptionalNullableFiniteNumber = (value: unknown): boolean =>
  value === undefined || value === null || (typeof value === 'number' && Number.isFinite(value));

const isRevision = (value: unknown): value is string =>
  typeof value === 'string' && /^(0|[1-9][0-9]*)$/u.test(value);

const isSafeUnsigned = (value: unknown): value is number =>
  typeof value === 'number' && Number.isSafeInteger(value) && value >= 0;

const isContentHash = (value: unknown): boolean =>
  exactRecord(value, ['algorithm', 'hex']) &&
  (value.algorithm === 'blake3' || value.algorithm === 'sha256') &&
  typeof value.hex === 'string' &&
  /^[0-9a-f]{64}$/u.test(value.hex);

const isAuraId = (
  value: unknown,
  prefix: 'art' | 'crd' | 'end' | 'mav' | 'mdl' | 'mdr' | 'prv' | 'ses',
): value is string =>
  typeof value === 'string' &&
  new RegExp(`^${prefix}_[0-7][0-9A-HJKMNP-TV-Z]{25}$`, 'u').test(value);

const exactRecord = <Keys extends readonly string[]>(
  value: unknown,
  keys: Keys,
): value is Record<Keys[number], unknown> => recordWithOptional(value, keys, []);

const recordWithOptional = <Required extends readonly string[], Optional extends readonly string[]>(
  value: unknown,
  required: Required,
  optional: Optional,
): value is Record<Required[number], unknown> & Partial<Record<Optional[number], unknown>> => {
  if (!isRecord(value)) return false;
  const keys = Object.keys(value);
  if (!required.every((key) => keys.includes(key))) return false;
  const allowed = new Set<string>([...required, ...optional]);
  return keys.every((key) => allowed.has(key));
};

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);
