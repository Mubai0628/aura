import type { SecurityProjectionChange } from '@aura/contracts/generated';
import { readSecurityNotification } from './auxiliary-projections.js';
import type { ProjectionCompatibility } from './projection-compatibility.js';
import { terminalOutputNotification } from './terminal-output.js';
import { terminalOpeningNotification } from './terminal-opening-progress.js';
import { browserCommandNotification } from './browser-host/core-channel.js';
import { mediaCommandNotification } from './media-host/core-channel.js';
interface Context {
  owner: object;
  method: string;
  params: unknown;
  runtimeId: string;
  securitySupported: boolean;
  securityListeners: ReadonlySet<(change: SecurityProjectionChange) => void>;
  compatibility: ProjectionCompatibility;
}
export function auxiliaryRuntimeNotification(context: Context): boolean {
  const { owner, method, params, runtimeId, securitySupported, securityListeners, compatibility } =
    context;
  if (terminalOpeningNotification(owner, method, params, runtimeId)) return true;
  if (browserCommandNotification(owner, method, params, runtimeId)) return true;
  if (mediaCommandNotification(owner, method, params, runtimeId)) return true;
  if (terminalOutputNotification(owner, method, params, runtimeId, compatibility)) return true;
  if (method !== 'security.projection.change') return false;
  const change = readSecurityNotification(params, runtimeId, compatibility, securitySupported);
  if (change === undefined) return true;
  for (const listener of securityListeners) listener(structuredClone(change));
  return true;
}
