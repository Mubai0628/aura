import type { AgentTurnView } from '@aura/contracts/generated';
import { terminalTurn } from './turn-state';

/** How long a turn has taken: until it ended, or until `now` while it runs. */
export function turnDuration(item: AgentTurnView, now?: number): string | undefined {
  const end =
    terminalTurn(item.turn.state.state) || now === undefined
      ? Date.parse(item.turn.updated_at)
      : now;
  const seconds = Math.max(0, Math.floor((end - Date.parse(item.turn.created_at)) / 1000));
  if (!Number.isFinite(seconds)) return undefined;
  return seconds < 60 ? `${seconds} 秒` : `${Math.floor(seconds / 60)} 分 ${seconds % 60} 秒`;
}
