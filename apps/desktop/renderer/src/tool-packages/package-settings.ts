import type { CanonicalToolPackageToolV1 } from '@aura/contracts/generated';

export function endpointSlot(tool: CanonicalToolPackageToolV1): string | undefined {
  return tool.runtime.type === 'declarative_http' &&
    tool.runtime.request.target.type === 'user_bound_endpoint'
    ? tool.runtime.request.target.endpoint_slot_key
    : undefined;
}

export function settingsNeeded(tools: CanonicalToolPackageToolV1[]): boolean {
  return tools.some(
    (tool) => tool.instance_configuration != null || endpointSlot(tool) !== undefined,
  );
}

/** Splits `https://host[:port]/base` into the origin and base path Core checks again. */
export function splitServiceUrl(text: string): { origin: string; base_path: string } | undefined {
  try {
    const url = new URL(text.trim());
    if (url.protocol !== 'https:' || url.username || url.password || url.search || url.hash)
      return undefined;
    return { origin: url.origin, base_path: url.pathname || '/' };
  } catch {
    return undefined;
  }
}
