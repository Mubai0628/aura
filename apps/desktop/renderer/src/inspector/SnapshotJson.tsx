import { useEffect, useState, type ReactNode } from 'react';

import { amount } from './inspector-labels';
import { SnapshotLines } from './json-lines';

/** One row's height, in pixels, as the stylesheet draws it. */
const ROW = 16;
/** Rows drawn from the first one on screen, and before it. */
const AHEAD = 40;
const BEHIND = 10;
/** Rows drawn while the lines are counted. */
const PLACEHOLDER = 10;
/** Characters a row shows of a very long line. */
const LONGEST = 2_000;
/** Milliseconds of work between two frames. */
const SLICE = 8;

const clip = (line: string) =>
  line.length <= LONGEST ? line : `${line.slice(0, LONGEST)} … 共 ${amount(line.length)} 字`;

/**
 * The snapshot's JSON opens at once. Its lines are counted first, the view takes their height,
 * and the rows on screen are drawn — skeletons until their text is laid out.
 */
export function SnapshotJson({ value }: { value: unknown }) {
  const [work] = useState(() => new SnapshotLines(value));
  const [, setReady] = useState(0);
  const [first, setFirst] = useState(0);
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const slice = () => {
      work.step(performance.now() + SLICE);
      setReady(work.total === undefined ? -1 : work.ready);
      if (!work.done) timer = setTimeout(slice, 0);
    };
    timer = setTimeout(slice, 0);
    return () => clearTimeout(timer);
  }, [work]);

  const total = work.total;
  const count = total ?? PLACEHOLDER;
  const rows: ReactNode[] = [];
  for (
    let index = Math.max(0, first - BEHIND);
    index < Math.min(count, first + AHEAD);
    index += 1
  ) {
    const line = total === undefined ? undefined : work.line(index);
    rows.push(
      line === undefined ? (
        <div
          aria-hidden="true"
          className="snapshot-json-row is-loading"
          key={index}
          style={{ top: index * ROW }}
        >
          <span
            className="snapshot-json-skeleton"
            style={{ width: `${28 + ((index * 37) % 52)}%` }}
          />
        </div>
      ) : (
        <div className="snapshot-json-row" key={index} style={{ top: index * ROW }}>
          {clip(line)}
        </div>
      ),
    );
  }
  return (
    <div className="snapshot-json">
      <p className="pixel-caption" role="status">
        {total === undefined ? '正在统计行数' : `${amount(total)} 行`}
      </p>
      <div
        aria-busy={!work.done}
        aria-label="快照 JSON"
        className="snapshot-json-view"
        onScroll={(event) => setFirst(Math.floor(event.currentTarget.scrollTop / ROW))}
        role="region"
        tabIndex={0}
      >
        <div className="snapshot-json-space" style={{ height: count * ROW }}>
          {rows}
        </div>
      </div>
    </div>
  );
}
