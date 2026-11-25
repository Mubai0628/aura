import { expect, test } from 'vitest';
import { graphView } from '../graph-fixtures.test-support';
import { fit, zoomAbout } from '../canvas/layout';
import { graphPositions, mergePositions } from './graph-layout';

test('two small temporary teams fit a narrow overview without unused four-column space', () => {
  const positions = graphPositions(
    graphView([
      ['ses_a', '规划'],
      ['ses_b', '开发'],
    ]),
  );
  expect(fit(positions, { width: 320, height: 500 }, 0.05).zoom).toBeGreaterThanOrEqual(0.5);
});

test('world positions use actual session ids even when each parent calls its member A1', () => {
  const view = graphView([
    ['ses_a', '规划'],
    ['ses_b', '开发'],
  ]);
  view.graph.positions = [
    { session_id: 'ses_a_child', x: -140, y: 60 },
    { session_id: 'ses_b_child', x: 900, y: -120 },
  ];
  const positions = graphPositions(view);
  expect(Object.keys(positions).sort()).toEqual(['ses_a', 'ses_a_child', 'ses_b', 'ses_b_child']);
  expect(positions.ses_a_child).toEqual({ x: -140, y: 60 });
  expect(positions.ses_b_child).toEqual({ x: 900, y: -120 });
  const before = structuredClone(positions);
  const narrow = fit(positions, { width: 320, height: 500 }, 0.05);
  const wide = fit(positions, { width: 1600, height: 900 }, 0.05);
  expect(narrow.zoom).toBeLessThan(wide.zoom);
  zoomAbout(wide, 1.2, { x: 200, y: 100 }, 0.05);
  expect(positions).toEqual(before);
});

test('merging leaves the destination fixed and translates the source without changing relative layout', () => {
  const destination = graphView([['ses_a', '规划']]);
  const source = graphView([['ses_b', '开发']], 'acg_other');
  destination.graph.positions = [
    { session_id: 'ses_a', x: -100, y: 0 },
    { session_id: 'ses_a_child', x: 200, y: 100 },
  ];
  source.graph.positions = [
    { session_id: 'ses_b', x: -100, y: 0 },
    { session_id: 'ses_b_child', x: 200, y: 100 },
  ];
  const merged = new Map(
    mergePositions(destination, source).map((position) => [position.session_id, position]),
  );
  expect(merged.get('ses_a')).toEqual({ session_id: 'ses_a', x: -100, y: 0 });
  expect(merged.get('ses_a_child')).toEqual({ session_id: 'ses_a_child', x: 200, y: 100 });
  expect(merged.get('ses_b')!.x).toBeGreaterThan(380);
  expect(merged.get('ses_b_child')!.x - merged.get('ses_b')!.x).toBe(300);
  expect(merged.get('ses_b_child')!.y - merged.get('ses_b')!.y).toBe(100);
  expect(source.graph.positions[0]!.x).toBe(-100);
});
