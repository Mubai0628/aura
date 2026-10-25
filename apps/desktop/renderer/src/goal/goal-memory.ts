/**
 * Which completed Goals were closed in this window, kept like the task list's: a
 * preference of this window only; a storage failure keeps the choice until the window reloads.
 */
const CLOSED = 'aura.goal-closed.v1';
/** Enough for every recent Goal; the oldest are forgotten first. */
const LIMIT = 200;

let cache: string[] | undefined;

function read(): string[] {
  if (cache) return cache;
  let items: string[] = [];
  try {
    const value: unknown = JSON.parse(localStorage.getItem(CLOSED) ?? '[]');
    if (Array.isArray(value)) items = value.filter((v): v is string => typeof v === 'string');
  } catch {
    /* Nothing remembered. */
  }
  cache = items;
  return items;
}

/** One Goal: its turn and the revision it reached. */
export const goalKey = (turnTaskId: string, revision: string | number) =>
  `${turnTaskId}:${revision}`;

export const isGoalClosed = (goal: string) => read().includes(goal);

export function closeGoal(goal: string): void {
  const closed = read();
  if (closed.includes(goal)) return;
  cache = [...closed, goal].slice(-LIMIT);
  try {
    localStorage.setItem(CLOSED, JSON.stringify(cache));
  } catch {
    /* Kept in memory. */
  }
}

/** Tests start from what localStorage holds. */
export function forgetGoalMemory(): void {
  cache = undefined;
}
