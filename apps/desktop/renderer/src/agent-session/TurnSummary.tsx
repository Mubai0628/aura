import type { AgentTurnView } from '@aura/contracts/generated';
import { GOAL_WORDS, goalPhase } from '../goal/goal-state';
import { turnDuration } from './turn-duration';

const amount = (value: number) => value.toLocaleString('zh-CN');

/**
 * What a finished turn took, at the right of the line under its answer: its Goal's outcome and usage, then its time.
 */
export function TurnSummary({ item }: { item: AgentTurnView }) {
  const duration = turnDuration(item);
  const goal = item.goal;
  if (!duration && !goal) return null;
  return (
    <p className="agent-turn-summary">
      {goal ? (
        <span className="agent-turn-goal" role="status">
          Goal {GOAL_WORDS[goalPhase(goal, false, false)]} · Token {amount(goal.consumed_tokens)} /{' '}
          {goal.budget.max_tokens == null ? '未设预算' : amount(goal.budget.max_tokens)} · 模型步数{' '}
          {goal.consumed_turns} / {goal.budget.max_turns ?? '未设预算'}
          {goal.latest_check_model_step ? ' · 已检查进度' : ''}
        </span>
      ) : null}
      {duration ? (
        <span className="agent-elapsed" aria-label={`耗时 ${duration}`}>
          {duration}
        </span>
      ) : null}
    </p>
  );
}
