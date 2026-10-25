import type { ReactNode } from 'react';

import { ListSentinel } from './SkeletonRows';
import { useIncrementalList } from './useIncrementalList';

/** A list whose rows are drawn a page at a time (see `useIncrementalList`). */
export function PagedList<T>({
  className,
  items,
  row,
}: {
  className: string;
  items: readonly T[];
  row: (item: T, index: number) => ReactNode;
}) {
  const pages = useIncrementalList(items);
  return (
    <ul className={className}>
      {pages.shown.map((item, index) => row(item, index))}
      {pages.more ? <ListSentinel sentinel={pages.sentinel} /> : null}
    </ul>
  );
}
