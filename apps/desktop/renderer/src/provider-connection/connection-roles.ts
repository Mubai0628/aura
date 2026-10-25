/**
 * A
 * provider's connections as Core routes them (crates/aura-model-config/src/routing.rs). Among the
 * connections of one API format, the default is the one the user chose, else the first created;
 * every other is a backup, in the order they were created.
 */
export type ConnectionRole = { kind: 'default' } | { kind: 'backup'; place: number };

interface Connection {
  id: string;
  api_format: string;
}

/** `connections` in the order they were created. */
export function connectionRoles(
  chosen: string | null | undefined,
  connections: readonly Connection[],
): Map<string, ConnectionRole> {
  const roles = new Map<string, ConnectionRole>();
  for (const format of new Set(connections.map((connection) => connection.api_format))) {
    const own = connections.filter((connection) => connection.api_format === format);
    const first = own.find((connection) => connection.id === chosen) ?? own[0];
    if (first === undefined) continue;
    roles.set(first.id, { kind: 'default' });
    own
      .filter((connection) => connection !== first)
      .forEach((connection, index) =>
        roles.set(connection.id, { kind: 'backup', place: index + 1 }),
      );
  }
  return roles;
}

/** When a connection was last changed, to the minute. */
export function changedAt(at: string): string {
  const date = new Date(at);
  return Number.isNaN(date.getTime())
    ? at
    : date.toLocaleString('zh-CN', {
        year: 'numeric',
        month: '2-digit',
        day: '2-digit',
        hour: '2-digit',
        minute: '2-digit',
        hour12: false,
      });
}
