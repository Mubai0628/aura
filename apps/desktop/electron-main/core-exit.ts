/** How long a quitting Main waits for Core to record a normal exit before it stops Core anyway. */
export const PREPARE_EXIT_TIMEOUT_MS = 3_000;

interface StoppableCore {
  requestWithBudget(method: string, params: unknown, timeoutMilliseconds: number): Promise<unknown>;
  stop(): void;
}

/**
 * Before Main stops Core on quit, Core records a
 * normal exit, so the next start checks only its Store's structure before serving. A Core that
 * does not answer in time is stopped all the same; the next start then checks every page.
 */
export async function prepareExitThenStop(core: StoppableCore | undefined): Promise<void> {
  if (core === undefined) return;
  try {
    await core.requestWithBudget('runtime.prepare_exit', {}, PREPARE_EXIT_TIMEOUT_MS);
  } catch {
    // Not recorded: the next start checks the whole Store.
  } finally {
    core.stop();
  }
}
