import type { AgentTurnView } from '@aura/contracts/generated';
import { localTime } from '../../../shared/local-time';

/** "21:17" today, "10月27日 21:17" on another day; 24-hour local time. */
export function restartTime(at: string, now = new Date()): string {
  return localTime(at, now);
}

/** Whether `item` is a turn a restart cut off that the user can continue now. */
export const continuable = (item: AgentTurnView, last: boolean, idle: boolean): boolean =>
  last &&
  idle &&
  !item.imported &&
  item.turn.state.state === 'recovery_required' &&
  item.turn.state.reason === 'agent.core_restarted';
