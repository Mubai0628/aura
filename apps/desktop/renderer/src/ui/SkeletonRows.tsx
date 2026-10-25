import './skeleton.css';

/**
 * What a list looks like while it is read for the first
 * time — grey rows in its own shape, no words.
 */
export function SkeletonRows({ rows = 4, label }: { rows?: number; label: string }) {
  return (
    <ul aria-busy="true" aria-label={label} className="skeleton-rows">
      {Array.from({ length: rows }, (_, index) => (
        <li className="skeleton-row" key={index}>
          <span className="skeleton-block skeleton-icon" />
          <span className="skeleton-lines">
            <span className="skeleton-block skeleton-title" />
            <span className="skeleton-block skeleton-text" />
          </span>
        </li>
      ))}
    </ul>
  );
}

/** The last row of a list drawn in pages: when it comes into view, the next page is drawn. */
export function ListSentinel({ sentinel }: { sentinel: React.RefObject<HTMLLIElement | null> }) {
  return <li aria-hidden="true" className="list-sentinel" ref={sentinel} />;
}
