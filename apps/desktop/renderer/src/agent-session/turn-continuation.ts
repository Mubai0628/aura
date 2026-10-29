import type { AgentTurnView } from '@aura/contracts/generated';

const twoDigits = (value: number) => String(value).padStart(2, '0');

/** "21:17" today, "10月27日 21:17" on another day; 24-hour local time. */
export function restartTime(at: string, now = new Date()): string {
  const time = new Date(at);
  const clock = `${twoDigits(time.getHours())}:${twoDigits(time.getMinutes())}`;
  return time.toDateString() === now.toDateString()
    ? clock
    : `${time.getMonth() + 1}月${time.getDate()}日 ${clock}`;
}

/** Whether `item` is a turn a restart cut off that the user can continue now. */
export const continuable = (item: AgentTurnView, last: boolean, idle: boolean): boolean =>
  last &&
  idle &&
  !item.imported &&
  item.turn.state.state === 'recovery_required' &&
  item.turn.state.reason === 'agent.core_restarted';
