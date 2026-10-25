/**
 * What this window remembers about the task list: which lists were closed once they were
 * finished, and in which conversations the card is folded. A preference of this window only,
 * kept like the composer drafts; a storage failure keeps the choice until the window reloads.
 */
const CLOSED = 'aura.todo-closed.v1';
const FOLDED = 'aura.todo-folded.v1';
/** Enough for every recent list; the oldest are forgotten first. */
const LIMIT = 200;

const cache = new Map<string, string[]>();

function read(key: string): string[] {
  const cached = cache.get(key);
  if (cached) return cached;
  let items: string[] = [];
  try {
    const value: unknown = JSON.parse(localStorage.getItem(key) ?? '[]');
    if (Array.isArray(value)) items = value.filter((v): v is string => typeof v === 'string');
  } catch {
    /* Nothing remembered. */
  }
  cache.set(key, items);
  return items;
}

function save(key: string, items: string[]): void {
  const kept = items.slice(-LIMIT);
  cache.set(key, kept);
  try {
    localStorage.setItem(key, JSON.stringify(kept));
  } catch {
    /* Kept in memory. */
  }
}

/** One list: its turn and the revision the model wrote it at. */
export const todoListKey = (turnTaskId: string, revision: string | number) =>
  `${turnTaskId}:${revision}`;

export const isTodoClosed = (list: string) => read(CLOSED).includes(list);

export function closeTodo(list: string): void {
  const closed = read(CLOSED);
  if (!closed.includes(list)) save(CLOSED, [...closed, list]);
}

export const isTodoFolded = (sessionId: string) => read(FOLDED).includes(sessionId);

export function foldTodo(sessionId: string, folded: boolean): void {
  const others = read(FOLDED).filter((id) => id !== sessionId);
  save(FOLDED, folded ? [...others, sessionId] : others);
}

/** Tests start from what localStorage holds. */
export function forgetTodoMemory(): void {
  cache.clear();
}
