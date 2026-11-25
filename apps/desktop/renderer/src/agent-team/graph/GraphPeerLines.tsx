import type { AgentGraphEdge, AgentGraphView } from '@aura/contracts/generated';
import { useId, useMemo } from 'react';
import { ToolIcon } from '../../tool-icon';
import { useContextMenu } from '../../ui/useContextMenu';
import { CanvasFlow } from '../canvas/CanvasFlow';
import { pathOf, type Positions } from '../canvas/layout';
import { peerEdgeActivity } from './peer-activity';
import { peerRoutes } from './peer-routes';

export function GraphPeerLines({
  view,
  positions,
  disabled,
  onDisconnect,
}: {
  view: AgentGraphView;
  positions: Positions;
  disabled: boolean;
  onDisconnect: (edge: AgentGraphEdge) => void;
}) {
  const marker = `peer-arrow-${useId().replaceAll(':', '')}`;
  const names = new Map(view.profiles.map((profile) => [profile.session_id, profile.name]));
  const routes = useMemo(
    () => peerRoutes(positions, view.graph.sessions, view.graph.edges),
    [positions, view.graph.sessions, view.graph.edges],
  );
  const menu = useContextMenu();
  return (
    <>
      <svg aria-label="同级连线" className="agent-canvas-lines" width="1" height="1">
        <defs>
          <marker
            id={marker}
            viewBox="0 0 10 10"
            refX="9"
            refY="5"
            markerWidth="6"
            markerHeight="6"
            orient="auto-start-reverse"
          >
            <path d="M0 0 L10 5 L0 10z" fill="currentColor" />
          </marker>
        </defs>
        {view.graph.edges.map((edge) => {
          const { source_session_id: source, target_session_id: target } = edge;
          const route = routes.get(`${source}/${target}`);
          if (!route) return null;
          const path = pathOf(route);
          const activity = peerEdgeActivity(view, edge);
          const label = `${names.get(source)} → ${names.get(target)}`;
          const reverseQuestion =
            !routes.has(`${target}/${source}`) &&
            peerEdgeActivity(view, { source_session_id: target, target_session_id: source }).active;
          const activityLabel = `${label} · ${activity.text}${
            reverseQuestion ? ` · ${names.get(target)} → ${names.get(source)} · 提问执行中` : ''
          }`;
          const actions = [
            {
              label: '删除连线',
              icon: <ToolIcon name="trash" />,
              disabled,
              run: () => onDisconnect(edge),
            },
          ];
          return (
            <g key={`${source}/${target}`}>
              <title>{activityLabel}</title>
              <path
                className="agent-canvas-line-path agent-graph-peer-line"
                data-active={activity.active || undefined}
                markerEnd={`url(#${marker})`}
                d={path}
              />
              {activity.active ? <CanvasFlow kind="task" path={path} /> : null}
              {reverseQuestion ? (
                <CanvasFlow
                  kind="question"
                  path={pathOf([...route].reverse())}
                  markerEnd={`url(#${marker})`}
                />
              ) : null}
              <path
                className="agent-graph-peer-hit"
                role="button"
                tabIndex={0}
                aria-label={activityLabel}
                aria-keyshortcuts="Delete"
                d={path}
                onContextMenu={(event) => menu.open(event, label, actions)}
                onKeyDown={(event) => {
                  if (event.key === 'Delete' && !disabled) {
                    event.preventDefault();
                    onDisconnect(edge);
                  } else if (event.key === 'Enter' || event.key === ' ') {
                    event.preventDefault();
                    const bounds = event.currentTarget.getBoundingClientRect();
                    menu.openAt(bounds.left, bounds.top, label, actions);
                  }
                }}
              />
            </g>
          );
        })}
      </svg>
      {menu.menu}
    </>
  );
}
