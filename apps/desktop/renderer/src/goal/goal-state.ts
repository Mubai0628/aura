import type { GoalState } from '@aura/contracts/generated';

/** Where a Goal is, as the conversation and the 近期活动 card both say it. */
export type GoalPhase = 'running' | 'completed' | 'paused' | 'open';

export const GOAL_WORDS: Record<GoalPhase, string> = {
  running: '进行中',
  completed: '已完成',
  paused: '已暂停',
  open: '未完成',
};

/** A Goal its turn is still working on is under way, never "not finished". */
export function goalPhase(goal: GoalState, running: boolean, paused: boolean): GoalPhase {
  if (goal.completed) return 'completed';
  if (running) return 'running';
  return paused ? 'paused' : 'open';
}
