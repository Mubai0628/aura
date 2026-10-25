import { useId, type ReactNode } from 'react';
import type { PixelCell } from './pixel-cells';

/** A bar of squares, each the colour of the part it shows. */
export function PixelBar<K extends string>({
  cells,
  label,
}: {
  cells: PixelCell<K>[];
  label: string;
}) {
  return (
    <span aria-label={label} className="pixel-bar" role="img">
      {cells.map((cell, index) => (
        <span className={`pixel-cell is-${cell}`} key={index} />
      ))}
    </span>
  );
}

/** One square-cornered card of the inspector: a title, an optional figure, and its drawing. */
export function PixelCard({
  title,
  figure,
  children,
}: {
  title: string;
  figure?: ReactNode;
  children?: ReactNode;
}) {
  const id = useId();
  return (
    <section aria-labelledby={id} className="pixel-card">
      <h4>
        <span aria-hidden="true" className="pixel-mark" />
        <span id={id}>{title}</span>
        {figure !== undefined ? <span className="pixel-figure">{figure}</span> : null}
      </h4>
      {children}
    </section>
  );
}
