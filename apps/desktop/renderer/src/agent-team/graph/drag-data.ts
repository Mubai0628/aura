export const SESSION_DRAG = 'application/x-aura-session';

export function draggedSession(data: DataTransfer): string | undefined {
  try {
    const value: unknown = JSON.parse(data.getData(SESSION_DRAG));
    if (
      typeof value === 'object' &&
      value !== null &&
      'session_id' in value &&
      typeof value.session_id === 'string'
    )
      return value.session_id;
  } catch {
    /* A different kind of drag is not a conversation. */
  }
  return undefined;
}
