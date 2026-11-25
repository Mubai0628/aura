import type { FlowKind } from './flows';

export function CanvasFlow({
  path,
  kind,
  agent,
  markerEnd,
}: {
  path: string;
  kind: FlowKind;
  agent?: string;
  markerEnd?: string;
}) {
  return (
    <g aria-hidden="true">
      <path className="agent-canvas-glow" data-kind={kind} d={path} />
      <path
        className="agent-canvas-flow"
        data-kind={kind}
        data-agent={agent}
        d={path}
        markerEnd={markerEnd}
      />
    </g>
  );
}
