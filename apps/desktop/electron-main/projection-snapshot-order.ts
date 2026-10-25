/** A reply may be overtaken by a notification or another snapshot on the same
 * authenticated Core. Only a revision older than the request's starting point
 * proves a regression. Returning undefined preserves an already newer view. */
export function selectSnapshotRevision(
  revision: string,
  revisionAtRequest: number | undefined,
  currentRevision: number | undefined,
  projectionName: string,
): number | undefined {
  const incoming = Number(revision);
  if (!Number.isSafeInteger(incoming)) {
    throw new Error(`${projectionName} snapshot revision was not a safe integer`);
  }
  if (revisionAtRequest !== undefined && incoming < revisionAtRequest) {
    throw new Error(`${projectionName} snapshot revision moved backwards`);
  }
  if (currentRevision !== undefined && incoming < currentRevision) return undefined;
  return incoming;
}
