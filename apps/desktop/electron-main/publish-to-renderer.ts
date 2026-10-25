import type { WebContents } from 'electron';
/** Main owns the receiver; no renderer-selected channel or destination. */
export function publishToRenderer(
  contents: WebContents | undefined,
  channel: string,
  payload: unknown,
): void {
  if (contents && !contents.isDestroyed()) contents.send(channel, structuredClone(payload));
}
