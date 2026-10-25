import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, expect, test } from 'vitest';

import { SnapshotJson } from './SnapshotJson';

// A large snapshot
// opens at once with skeleton rows, then says how many lines it has and fills them in.

afterEach(cleanup);

const large = {
  id: 'ctx_large',
  items: Array.from({ length: 40_000 }, (_, index) => ({ index, text: `第 ${index} 条` })),
};

test('a large snapshot opens at once, counts its lines, then fills them in', async () => {
  const started = performance.now();
  render(<SnapshotJson value={large} />);
  expect(performance.now() - started).toBeLessThan(200);
  const view = screen.getByRole('region', { name: '快照 JSON' });
  expect(view.getAttribute('aria-busy')).toBe('true');
  expect(view.querySelectorAll('.snapshot-json-row.is-loading').length).toBeGreaterThan(0);
  const lines = JSON.stringify(large, null, 2).split('\n');
  expect(
    await screen.findByText(`${lines.length.toLocaleString('en-US')} 行`, undefined, {
      timeout: 5_000,
    }),
  ).toBeTruthy();
  expect(await screen.findByText(lines[1]!.trim(), { exact: false })).toBeTruthy();
  // Only the rows on screen are drawn, however long the snapshot.
  expect(view.querySelectorAll('.snapshot-json-row').length).toBeLessThan(60);
});
