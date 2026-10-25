import { useRef } from 'react';
import { AppIcon } from './app-icons';
import { useDismissablePopover } from './useDismissablePopover';

/**
 * The mode /plan or /goal chose, with the × that goes back to a normal conversation as before.
 * Goal's budgets, which lived in the + menu, open from its chip.
 */
export function ComposerModeChip({
  mode,
  budgetTokens,
  budgetTurns,
  onBudgetTokens,
  onBudgetTurns,
  onLeave,
}: {
  mode: 'plan' | 'goal';
  budgetTokens: string;
  budgetTurns: string;
  onBudgetTokens: (value: string) => void;
  onBudgetTurns: (value: string) => void;
  onLeave: () => void;
}) {
  const anchor = useRef<HTMLSpanElement>(null);
  const [open, setOpen] = useDismissablePopover(anchor);
  const name = mode === 'plan' ? 'Plan' : 'Goal';
  return (
    <span className="composer-mode-chip" ref={anchor}>
      {mode === 'goal' ? (
        <button
          aria-expanded={open}
          aria-haspopup="dialog"
          aria-label="Goal 预算"
          className="composer-mode-chip-label"
          onClick={() => setOpen((value) => !value)}
          type="button"
        >
          Goal
        </button>
      ) : (
        <span>Plan</span>
      )}
      <button
        aria-label={`退出 ${name} 模式`}
        className="composer-mode-chip-close"
        onClick={onLeave}
        title="回到普通对话"
        type="button"
      >
        <AppIcon name="close" size={11} />
      </button>
      {open && mode === 'goal' ? (
        <div aria-label="Goal 预算" className="composer-popover goal-budget-popover" role="dialog">
          <div className="goal-budget-fields">
            <label>
              Token 预算
              <input
                aria-label="Goal Token 预算"
                min="1"
                onChange={(event) => onBudgetTokens(event.target.value)}
                placeholder="不限"
                type="number"
                value={budgetTokens}
              />
            </label>
            <label>
              模型步数
              <input
                aria-label="Goal 模型步数预算"
                min="1"
                onChange={(event) => onBudgetTurns(event.target.value)}
                placeholder="不限"
                type="number"
                value={budgetTurns}
              />
            </label>
          </div>
        </div>
      ) : null}
    </span>
  );
}
