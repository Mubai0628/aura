import type { PeerEdgeActivity } from '@aura/contracts/generated';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import { graphView } from '../graph-fixtures.test-support';
import { GraphPeerLines } from './GraphPeerLines';
import { graphPositions } from './graph-layout';

afterEach(cleanup);

test('the three edges of a staggered ring do not intersect or share a segment', () => {
  const view = graphView([
    ['ses_a', '规划'],
    ['ses_b', '开发'],
    ['ses_c', '验收'],
  ]);
  view.graph.edges = [
    { source_session_id: 'ses_a', target_session_id: 'ses_b' },
    { source_session_id: 'ses_b', target_session_id: 'ses_c' },
    { source_session_id: 'ses_c', target_session_id: 'ses_a' },
  ];
  const { container } = render(
    <GraphPeerLines
      view={view}
      positions={{ ses_a: { x: 40, y: 340 }, ses_b: { x: 424, y: 340 }, ses_c: { x: 124, y: 50 } }}
      disabled={false}
      onDisconnect={vi.fn()}
    />,
  );
  const lines = Array.from(container.querySelectorAll('.agent-graph-peer-line'), (line) => {
    const points = Array.from(line.getAttribute('d')!.matchAll(/[ML](-?\d+) (-?\d+)/gu), (m) => ({
      x: Number(m[1]),
      y: Number(m[2]),
    }));
    return points.slice(1).map((end, index) => {
      const begin = points[index]!;
      return {
        left: Math.min(begin.x, end.x),
        right: Math.max(begin.x, end.x),
        top: Math.min(begin.y, end.y),
        bottom: Math.max(begin.y, end.y),
      };
    });
  });
  expect(lines).toHaveLength(3);
  for (const [index, line] of lines.entries()) {
    for (const other of lines.slice(index + 1)) {
      expect(
        line.some((a) =>
          other.some(
            (b) => a.left <= b.right && a.right >= b.left && a.top <= b.bottom && a.bottom >= b.top,
          ),
        ),
      ).toBe(false);
    }
  }
});

test('only an actual running delivery highlights the line; unknown remains readable', () => {
  const graph = graphView([
    ['ses_a', '规划'],
    ['ses_b', '开发'],
  ]);
  const edge = { source_session_id: 'ses_a', target_session_id: 'ses_b' };
  graph.graph.edges = [edge];
  const positions = graphPositions(graph);
  const disconnect = vi.fn();
  const show = (state?: PeerEdgeActivity['state'], paused = false) => {
    const view = {
      ...graph,
      peer_activity: state ? [{ edge, state, paused_by_global_switch: paused }] : [],
    };
    return (
      <GraphPeerLines
        view={view}
        positions={positions}
        disabled={false}
        onDisconnect={disconnect}
      />
    );
  };
  const { container, rerender } = render(show());
  expect(screen.getByRole('button', { name: /规划 → 开发 · 可协作/ })).toBeTruthy();
  expect(container.querySelector('.agent-graph-peer-line[data-active]')).toBeNull();
  rerender(show('started'));
  expect(container.querySelector('.agent-graph-peer-line[data-active]')).not.toBeNull();
  expect(container.querySelector('.agent-canvas-flow')).not.toBeNull();
  rerender(show('unknown'));
  expect(container.querySelector('.agent-graph-peer-line[data-active]')).toBeNull();
  expect(container.querySelector('.agent-canvas-flow')).toBeNull();
  expect(screen.getByRole('button', { name: /规划 → 开发 · 结果未知，先对账/ })).toBeTruthy();
  rerender(show('queued', true));
  expect(screen.getByRole('button', { name: /规划 → 开发 · 已排队/ })).toBeTruthy();
  expect(disconnect).not.toHaveBeenCalled();
});

test('each direction of a mutual connection has its own hit area and delete action', () => {
  const view = graphView([
    ['ses_a', '规划'],
    ['ses_b', '开发'],
  ]);
  const outbound = { source_session_id: 'ses_a', target_session_id: 'ses_b' };
  const inbound = { source_session_id: 'ses_b', target_session_id: 'ses_a' };
  view.graph.edges = [outbound, inbound];
  const disconnect = vi.fn();
  const { container } = render(
    <GraphPeerLines
      view={view}
      positions={graphPositions(view)}
      disabled={false}
      onDisconnect={disconnect}
    />,
  );
  const hits = [...container.querySelectorAll('.agent-graph-peer-hit')];
  expect(hits).toHaveLength(2);
  expect(hits[0]?.getAttribute('d')).not.toBe(hits[1]?.getAttribute('d'));
  for (const [index, edge] of [outbound, inbound].entries()) {
    fireEvent.contextMenu(hits[index]!);
    fireEvent.click(screen.getByRole('menuitem', { name: '删除连线' }));
    expect(disconnect).toHaveBeenNthCalledWith(index + 1, edge);
  }
});

test('a reverse question flows toward its receiver on a one-way connection', () => {
  const view = graphView([
    ['ses_a', '规划'],
    ['ses_b', '开发'],
  ]);
  const edge = { source_session_id: 'ses_a', target_session_id: 'ses_b' };
  const question = { source_session_id: 'ses_b', target_session_id: 'ses_a' };
  view.graph.edges = [edge];
  view.peer_activity = [{ edge: question, state: 'started', paused_by_global_switch: false }];
  const { container } = render(
    <GraphPeerLines
      view={view}
      positions={graphPositions(view)}
      disabled={false}
      onDisconnect={vi.fn()}
    />,
  );
  const original = container.querySelector('.agent-graph-peer-line')!.getAttribute('d')!;
  const flow = container.querySelector('.agent-canvas-flow[data-kind="question"]');
  expect(flow?.getAttribute('d')?.match(/-?\d+ -?\d+/gu)).toEqual(
    original.match(/-?\d+ -?\d+/gu)?.reverse(),
  );
  expect(flow?.hasAttribute('marker-end')).toBe(true);
  expect(screen.getByRole('button', { name: /开发 → 规划 · 提问执行中/ })).toBeTruthy();
});
