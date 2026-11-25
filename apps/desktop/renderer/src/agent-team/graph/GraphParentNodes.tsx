import type { AgentGraphTeam, AgentGraphView } from '@aura/contracts/generated';
import type { KeyboardEvent, MouseEvent, PointerEvent as ReactPointerEvent } from 'react';
import { AppIcon } from '../../app-icons';
import { ToolIcon } from '../../tool-icon';
import { CanvasNode } from '../canvas/CanvasNode';
import { LEAD_KEY, labelPoint, pathOf, route, type Positions } from '../canvas/layout';
import { leadFacts, memberFacts } from '../canvas/node-facts';
import { CanvasFlow } from '../canvas/CanvasFlow';
import { teamLineFlow, useTeamFlows } from '../canvas/flows';
import { useMemberMenu } from '../member-menu';
import { useContextMenu, type MenuEntry } from '../../ui/useContextMenu';
import { useMemberLayout } from './member-layout';

export function GraphParentNodes({
  entry,
  view,
  positions,
  selected,
  selectedForMove,
  chosen,
  disabled,
  linking,
  onSelect,
  onOpen,
  onGrab,
  onNodeKey,
  onRemove,
  onEditProfile,
  onStartLink,
}: {
  entry: AgentGraphTeam;
  view: AgentGraphView;
  positions: Positions;
  selected: string;
  selectedForMove: ReadonlySet<string>;
  chosen: string | undefined;
  disabled: boolean;
  linking: string | undefined;
  onSelect: (node: string) => void;
  onOpen: (parent: string, agent?: string) => void;
  onGrab: (event: ReactPointerEvent<HTMLButtonElement>, id: string) => void;
  onNodeKey: (event: KeyboardEvent<HTMLButtonElement>, id: string) => void;
  onRemove: (parent: string) => void;
  onEditProfile: (parent: string) => void;
  onStartLink: (parent: string) => void;
}) {
  const parent = entry.parent_session_id;
  const profile = view.profiles.find((profile) => profile.session_id === parent);
  const name = profile?.name ?? '未命名会话';
  const at = positions[parent] ?? { x: 0, y: 0 };
  const menu = useContextMenu();
  const actions = useMemberMenu(parent);
  const layout = useMemberLayout(parent, entry.team.members);
  const flows = useTeamFlows(entry.team);
  const local = {
    ...Object.fromEntries(Object.entries(positions).filter(([id]) => id !== parent)),
    [LEAD_KEY]: at,
  };
  const expandOne = (member: (typeof layout.folded)[number]) => ({
    label: `展开 ${member.agent} ${member.name}`,
    icon: <AppIcon name="expand" />,
    run: () => layout.set([member], false),
  });
  const openParentMenu = (event: MouseEvent<HTMLButtonElement>) => {
    event.currentTarget.focus();
    const folds: MenuEntry[] = layout.folded.length
      ? [
          {
            label: '展开子 Agent',
            icon: <AppIcon name="expand" />,
            entries: [
              ...layout.folded.slice(0, 3).map(expandOne),
              'separator',
              ...Array.from(layout.byPhase, ([phase, members]) => ({
                label: `${phase}（${members.length}）`,
                entries: [
                  { label: '展开全部', run: () => layout.set(members, false) },
                  ...members.map(expandOne),
                ],
              })),
            ],
          },
        ]
      : [];
    menu.open(event, name, [
      ...folds,
      ...(folds.length ? ['separator' as const] : []),
      { label: '打开会话', icon: <AppIcon name="chat" />, run: () => onOpen(parent) },
      { label: '编辑简介', icon: <ToolIcon name="pencil" />, run: () => onEditProfile(parent) },
      {
        label: '发起连线',
        icon: <AppIcon name="agents" />,
        disabled: disabled || view.graph.sessions.length < 2,
        run: () => onStartLink(parent),
      },
      {
        label: '移出画布',
        icon: <ToolIcon name="trash" />,
        disabled: disabled || view.graph.sessions.length < 2,
        run: () => onRemove(parent),
      },
    ]);
  };
  return (
    <>
      <svg aria-hidden="true" className="agent-canvas-lines" width="1" height="1">
        {layout.visible.map((member) => {
          const corners = route(local, member.session_id);
          const flow = teamLineFlow(member, flows.get(member.agent));
          return (
            <g key={member.session_id}>
              <path
                className="agent-canvas-line-path"
                data-agent={member.agent}
                data-chosen={selected === parent && chosen === member.agent ? 'true' : undefined}
                data-kind="temporary"
                data-ended={memberFacts(member).ended ? 'true' : undefined}
                d={pathOf(corners)}
              />
              {flow ? (
                <CanvasFlow
                  agent={member.agent}
                  kind={flow.kind}
                  path={pathOf(flow.up ? [...corners].reverse() : corners)}
                />
              ) : null}
            </g>
          );
        })}
      </svg>
      <div role="group" aria-label={`${name}会话节点`} className="agent-graph-root">
        <CanvasNode
          at={at}
          lead
          facts={{ ...leadFacts(entry.team.lead), name }}
          ariaLabel={`${name}${leadFacts(entry.team.lead).model}，主 Agent`}
          chosen={selected === parent && chosen === undefined}
          graphNode={parent}
          selectedForMove={selectedForMove.has(parent)}
          linkSource={linking === parent}
          onKeyDown={(event) => onNodeKey(event, parent)}
          onPointerDown={(event) => onGrab(event, parent)}
          onPick={() => onSelect(parent)}
          onOpen={linking ? undefined : () => onOpen(parent)}
          onMenu={openParentMenu}
        />
      </div>
      {layout.visible.map((member) => (
        <CanvasNode
          key={member.session_id}
          context={name}
          at={positions[member.session_id] ?? { x: 0, y: 0 }}
          facts={memberFacts(member)}
          chosen={selected === parent && chosen === member.agent}
          graphNode={member.session_id}
          selectedForMove={selectedForMove.has(member.session_id)}
          onKeyDown={(event) => onNodeKey(event, member.session_id)}
          onPointerDown={(event) => onGrab(event, member.session_id)}
          onPick={() => onSelect(member.session_id)}
          onOpen={() => {
            if (!linking) {
              layout.set([member], false);
              onOpen(parent, member.agent);
            }
          }}
          onMenu={(event) => {
            event.currentTarget.focus();
            menu.open(event, `${name}的 ${member.agent}`, [
              {
                label: '打开会话',
                icon: <AppIcon name="chat" />,
                run: () => onOpen(parent, member.agent),
              },
              {
                label: '收起子 Agent',
                icon: <AppIcon name="shrink" />,
                run: () => layout.set([member], true),
              },
              'separator',
              ...actions.entries(member),
            ]);
          }}
        />
      ))}
      {layout.visible.map((member) => {
        const flow = flows.get(member.agent);
        if (!flow) return null;
        const corners = route(local, member.session_id);
        const at = labelPoint(corners);
        return (
          <span
            className="agent-canvas-chip"
            data-kind={flow.kind}
            key={`chip:${member.session_id}`}
            style={{ left: at.x, top: at.y }}
          >
            {flow.label}
          </span>
        );
      })}
      {actions.problem ? (
        <p className="agent-canvas-problem" role="alert">
          {actions.problem}
          <button type="button" onClick={actions.clearProblem}>
            关闭提示
          </button>
        </p>
      ) : null}
      {menu.menu}
      {actions.dialog}
    </>
  );
}
