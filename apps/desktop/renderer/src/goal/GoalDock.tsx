import type { GoalState } from '@aura/contracts/generated';
import { useId, useReducer } from 'react';
import { AppIcon } from '../app-icons';
import { closeGoal, goalKey, isGoalClosed } from './goal-memory';
import { GOAL_WORDS, goalPhase } from './goal-state';
import './goal-dock.css';

const amount = (value: number) => value.toLocaleString('zh-CN');

/**
 * The conversation's Goal in the 近期活动 card: its state - moving while
 * its turn works on it - what set it, and what it has used so far, live. Once complete it can be
 * closed, and that Goal stays closed; the next Goal shows again.
 */
export function GoalDock({
  goal,
  objective,
  paused,
  running,
}: {
  goal: GoalState | undefined;
  /** The user's request that set the Goal. */
  objective?: string | undefined;
  paused: boolean;
  /** The Goal's turn is running. */
  running: boolean;
}) {
  const headingId = useId();
  const [, remembered] = useReducer((count: number) => count + 1, 0);
  if (!goal) return null;
  const key = goalKey(goal.turn_task_id, goal.revision);
  if (goal.completed && isGoalClosed(key)) return null;
  const phase = goalPhase(goal, running, paused);
  return (
    <section aria-labelledby={headingId} className="activity-section goal-dock" data-state={phase}>
      <div className="goal-head">
        <h4 className="activity-section-title" id={headingId}>
          Goal
        </h4>
        <span className={`goal-status is-${phase}`}>
          <span aria-hidden="true" className="goal-mark">
            {phase === 'completed' ? <AppIcon name="check" size={10} /> : null}
          </span>
          {GOAL_WORDS[phase]}
        </span>
        {goal.completed ? (
          <button
            aria-label="关闭 Goal"
            className="goal-icon-button"
            onClick={() => {
              closeGoal(key);
              remembered();
            }}
            type="button"
          >
            <AppIcon name="close" size={13} />
          </button>
        ) : null}
      </div>
      {phase === 'running' ? <span aria-hidden="true" className="goal-progress" /> : null}
      {objective ? (
        <p className="goal-objective" title={objective}>
          {objective}
        </p>
      ) : null}
      <p className="goal-usage">
        Token {amount(goal.consumed_tokens)} /{' '}
        {goal.budget.max_tokens == null ? '未设上限' : amount(goal.budget.max_tokens)} · 模型步数{' '}
        {goal.consumed_turns} / {goal.budget.max_turns ?? '未设上限'}
      </p>
    </section>
  );
}
