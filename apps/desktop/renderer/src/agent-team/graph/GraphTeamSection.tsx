import type { AgentGraphTeam, AgentProfile } from '@aura/contracts/generated';
import { AppIcon } from '../../app-icons';
import { ToolIcon } from '../../tool-icon';
import { AgentTeamAttention } from '../AgentTeamAttention';
import { AgentTeamList } from '../AgentTeamList';
import { useMemberMenu } from '../member-menu';
import { useContextMenu } from '../../ui/useContextMenu';

export function GraphTeamSection({
  entry,
  profile,
  currentSession,
  chosen,
  onPick,
  onEditProfile,
  showAttention = true,
}: {
  entry: AgentGraphTeam;
  profile: AgentProfile | undefined;
  currentSession: string;
  chosen: string | undefined;
  onPick: (parent: string, agent?: string) => void;
  onEditProfile: (session: string) => void;
  showAttention?: boolean;
}) {
  const parent = entry.parent_session_id;
  const name = profile?.name ?? '未命名会话';
  const actions = useMemberMenu(parent);
  const menu = useContextMenu();
  const pick = (agent: string | undefined) => onPick(parent, agent);
  return (
    <section className="agent-graph-team" aria-label={`${name}的临时团队`}>
      <header>
        <h3>{name}</h3>
      </header>
      {showAttention ? (
        <AgentTeamAttention sessionId={parent} members={entry.team.members} onPick={pick} />
      ) : null}
      <AgentTeamList
        lead={entry.team.lead}
        members={entry.team.members}
        chosen={parent === currentSession ? chosen : ''}
        onPick={pick}
        onMenu={(event, member) =>
          menu.open(
            event,
            member ? `${name}的 ${member.agent}` : name,
            member
              ? [
                  {
                    label: '打开会话',
                    icon: <AppIcon name="chat" />,
                    run: () => pick(member.agent),
                  },
                  ...actions.entries(member),
                ]
              : [
                  { label: '打开会话', icon: <AppIcon name="chat" />, run: () => pick(undefined) },
                  {
                    label: '编辑简介',
                    icon: <ToolIcon name="pencil" />,
                    run: () => onEditProfile(parent),
                  },
                ],
          )
        }
      />
      {actions.problem ? (
        <p role="alert">
          {actions.problem}
          <button type="button" onClick={actions.clearProblem}>
            关闭提示
          </button>
        </p>
      ) : null}
      {menu.menu}
      {actions.dialog}
    </section>
  );
}
