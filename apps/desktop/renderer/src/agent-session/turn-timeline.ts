import type { AgentToolActivityEntry, AgentTurnView } from '@aura/contracts/generated';

type AssistantMessage = AgentTurnView['assistant_messages'][number];
type UserMessage = AgentTurnView['user_messages'][number];

/** One part of a model step inside a process section: its reasoning, or its visible calls. */
export type ProcessPart =
  | { key: string; kind: 'reasoning'; message: AssistantMessage }
  | { key: string; kind: 'tools'; message: AssistantMessage; callIds: string[] };

/**
 * A turn read in order. What the model writes between its tools is a reply in the
 * conversation; a message the user adds mid-turn is theirs; reasoning and calls in between are
 * process sections. The step that answers the turn keeps its text for the final area.
 */
export type TimelineBlock =
  | { kind: 'process'; key: string; parts: ProcessPart[] }
  | { kind: 'reply'; key: string; message: AssistantMessage }
  | { kind: 'user'; key: string; message: UserMessage }
  /**
   * The context was compacted before this step: older history became a summary. Or older tool results were cleared.
   */
  | { kind: 'compacted'; key: string; message: AssistantMessage }
  /** This step went through the provider's backup connection. */
  | { kind: 'connection'; key: string; message: AssistantMessage };

export function turnTimeline(
  item: AgentTurnView,
  calls: ReadonlyMap<string, AgentToolActivityEntry>,
  finalStepId: string | undefined,
): TimelineBlock[] {
  const root = item.turn.root_user_event_id;
  const events = [
    ...item.user_messages
      .filter((message) => message.event_id !== root)
      .map((user) => ({ at: user.at, user, assistant: undefined })),
    ...item.assistant_messages.map((assistant) => ({
      at: assistant.at,
      assistant,
      user: undefined,
    })),
  ].sort((a, b) => a.at.localeCompare(b.at));
  const blocks: TimelineBlock[] = [];
  let parts: ProcessPart[] = [];
  const close = () => {
    if (parts.length) blocks.push({ kind: 'process', key: parts[0]!.key, parts });
    parts = [];
  };
  for (const event of events) {
    if (event.user) {
      close();
      blocks.push({ kind: 'user', key: event.user.event_id, message: event.user });
      continue;
    }
    const message = event.assistant;
    if (!message) continue;
    const id = message.model_step_id;
    if (message.context_compacted || message.context_cleared) {
      close();
      blocks.push({ kind: 'compacted', key: `${id}:compacted`, message });
    }
    if (message.backup_connection) {
      close();
      blocks.push({ kind: 'connection', key: `${id}:connection`, message });
    }
    if (message.reasoning) parts.push({ key: `${id}:reasoning`, kind: 'reasoning', message });
    if (message.text && id !== finalStepId) {
      close();
      blocks.push({ kind: 'reply', key: id, message });
    }
    const callIds = message.tool_call_ids.filter((call) => !calls.get(call)?.display.internal);
    if (callIds.length) parts.push({ key: `${id}:tools`, kind: 'tools', message, callIds });
  }
  close();
  return blocks;
}

export const sectionCallIds = (parts: readonly ProcessPart[]): string[] =>
  parts.flatMap((part) => (part.kind === 'tools' ? part.callIds : []));

/** What a closed section holds: 思考, then the calls' own titles. */
export function sectionLabel(
  parts: readonly ProcessPart[],
  calls: ReadonlyMap<string, AgentToolActivityEntry>,
): string {
  const ids = sectionCallIds(parts);
  const titles = [
    ...new Set(ids.map((id) => calls.get(id)?.display.title).filter((title) => !!title)),
  ] as string[];
  const named =
    titles.length > 3 ? `${titles.slice(0, 3).join('、')} 等` : titles.join('、') || '工具调用';
  return [parts.some((part) => part.kind === 'reasoning') ? '思考' : '', ids.length ? named : '']
    .filter(Boolean)
    .join(' · ');
}

/** A failed, refused or unconfirmed call keeps its section open. */
export function sectionFailed(
  parts: readonly ProcessPart[],
  calls: ReadonlyMap<string, AgentToolActivityEntry>,
): boolean {
  return sectionCallIds(parts).some((id) => {
    const summary = calls.get(id)?.summary;
    const outcome = summary?.outcome?.type;
    return (
      summary?.side_effect_state === 'unknown' ||
      outcome === 'failed' ||
      outcome === 'operation_rejected' ||
      outcome === 'indeterminate'
    );
  });
}
