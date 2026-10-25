/**
 * A snapshot's JSON,
 * laid out as `JSON.stringify(value, null, 2)` lays it out, a slice of work at a time: first how
 * many lines it has, then the lines themselves, so the view opens at once and fills in.
 */

/** A container's members as JSON writes them: an object's defined fields, an array's items. */
function members(value: unknown): [string | null, unknown][] | undefined {
  if (typeof value !== 'object' || value === null) return undefined;
  if (Array.isArray(value))
    return value.map((item) => [
      null,
      item === undefined || typeof item === 'function' ? null : item,
    ]);
  return Object.entries(value).filter(
    ([, item]) => item !== undefined && typeof item !== 'function',
  );
}

const primitive = (value: unknown) => JSON.stringify(value) ?? 'null';

/** Counts the lines a few values at a time. */
export class LineCounter {
  readonly #pending: unknown[];
  count = 0;

  constructor(value: unknown) {
    this.#pending = [value];
  }

  /** Counts up to `values` more values; true once every one is counted. */
  run(values: number): boolean {
    for (let left = values; left > 0; left -= 1) {
      if (this.#pending.length === 0) return true;
      const children = members(this.#pending.pop());
      if (children === undefined || children.length === 0) this.count += 1;
      else {
        // The opening line and the closing one; each member brings its own.
        this.count += 2;
        for (const [, child] of children) this.#pending.push(child);
      }
    }
    return this.#pending.length === 0;
  }
}

interface Open {
  members: [string | null, unknown][];
  next: number;
  indent: number;
  close: string;
}

/** The lines, one at a time, in order. */
export function* jsonLines(value: unknown): Generator<string, void, undefined> {
  const open: Open[] = [];
  let pending: { value: unknown; indent: number; prefix: string; suffix: string } | undefined = {
    value,
    indent: 0,
    prefix: '',
    suffix: '',
  };
  while (pending !== undefined || open.length > 0) {
    if (pending !== undefined) {
      const { value: item, indent, prefix, suffix } = pending;
      pending = undefined;
      const pad = '  '.repeat(indent);
      const children = members(item);
      if (children === undefined) {
        yield `${pad}${prefix}${primitive(item)}${suffix}`;
        continue;
      }
      const [start, end] = Array.isArray(item) ? ['[', ']'] : ['{', '}'];
      if (children.length === 0) {
        yield `${pad}${prefix}${start}${end}${suffix}`;
        continue;
      }
      yield `${pad}${prefix}${start}`;
      open.push({ members: children, next: 0, indent: indent + 1, close: `${pad}${end}${suffix}` });
      continue;
    }
    const top = open[open.length - 1]!;
    if (top.next >= top.members.length) {
      open.pop();
      yield top.close;
      continue;
    }
    const [key, child] = top.members[top.next]!;
    top.next += 1;
    pending = {
      value: child,
      indent: top.indent,
      prefix: key === null ? '' : `${JSON.stringify(key)}: `,
      suffix: top.next < top.members.length ? ',' : '',
    };
  }
}

/**
 * The work of one snapshot, done in slices: counted first, then laid out. Each `step` works until
 * `now()` reaches `until`, then says what is known.
 */
export class SnapshotLines {
  readonly #counter: LineCounter;
  readonly #produce: Generator<string, void, undefined>;
  readonly #lines: string[] = [];
  #total: number | undefined;
  #done = false;

  constructor(value: unknown) {
    this.#counter = new LineCounter(value);
    this.#produce = jsonLines(value);
  }

  get total(): number | undefined {
    return this.#total;
  }

  get ready(): number {
    return this.#lines.length;
  }

  get done(): boolean {
    return this.#done;
  }

  line(index: number): string | undefined {
    return this.#lines[index];
  }

  step(until: number, now: () => number = () => performance.now()): void {
    while (this.#total === undefined) {
      if (this.#counter.run(2_000)) this.#total = this.#counter.count;
      else if (now() >= until) return;
    }
    while (!this.#done && now() < until)
      for (let left = 256; left > 0; left -= 1) {
        const next = this.#produce.next();
        if (next.done === true) {
          this.#done = true;
          break;
        }
        this.#lines.push(next.value);
      }
  }
}
