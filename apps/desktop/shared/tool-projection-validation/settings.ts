import {
  exactRecord,
  isRevision,
  isSafeUnsigned,
  recordWithOptional,
} from '../projection-validation/common.js';
import { isCanonicalHash, isOptionalNullable, isToolProjectionError } from './common.js';

// `ToolSettingsSnapshotV1` (crates/aura-tool-contract/src/registry/settings.rs). `network_proxy`
// is absent until a proxy is first saved, then present whether or not it is enabled.
export const isToolSettingsProjection = (value: unknown): boolean =>
  recordWithOptional(value, ['settings'], ['configuration_issue']) &&
  recordWithOptional(
    value.settings,
    [
      'content_hash',
      'default_timeout_ms',
      'max_concurrent_tool_calls',
      'max_dynamic_tool_schemas',
      'max_retries',
      'revision',
    ],
    ['network_proxy'],
  ) &&
  isCanonicalHash(value.settings.content_hash, 'sha256') &&
  isSafeUnsigned(value.settings.default_timeout_ms) &&
  isSafeUnsigned(value.settings.max_concurrent_tool_calls) &&
  isSafeUnsigned(value.settings.max_dynamic_tool_schemas) &&
  isSafeUnsigned(value.settings.max_retries) &&
  isRevision(value.settings.revision) &&
  (value.settings.network_proxy === undefined ||
    isToolNetworkProxy(value.settings.network_proxy)) &&
  isOptionalNullable(value.configuration_issue, isToolProjectionError);

// `ToolNetworkProxyV1` (crates/aura-tool-contract/src/network_proxy.rs): a literal loopback
// address or `localhost`, and a port in 1–65535.
const LOOPBACK_HOST = /^(localhost|127(\.(25[0-5]|2[0-4][0-9]|1[0-9]{2}|[1-9]?[0-9])){3}|::1)$/u;

const isToolNetworkProxy = (value: unknown): boolean =>
  exactRecord(value, ['enabled', 'host', 'port', 'protocol']) &&
  typeof value.enabled === 'boolean' &&
  typeof value.host === 'string' &&
  LOOPBACK_HOST.test(value.host) &&
  typeof value.port === 'number' &&
  Number.isInteger(value.port) &&
  value.port >= 1 &&
  value.port <= 65_535 &&
  (value.protocol === 'http' || value.protocol === 'socks5');
