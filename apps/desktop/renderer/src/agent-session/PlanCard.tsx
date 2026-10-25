import type { PlanRecord } from '@aura/contracts/generated';
import { MarkdownBody } from '../markdown-body';
import { ToolIcon } from '../tool-icon';
import './plan-card.css';

/**
 * The plan the model
 * wrote is Markdown, and was shown as plain text — `**` and backticks and all, every step in bold.
 * Now its goal, the project as it stands, what it must do and its steps read as written; each step
 * shows its id (which the others name when they wait for it), how it is checked and what it waits
 * for; 实施计划 is the card's main action.
 */
export function PlanCard({
  plan,
  disabled,
  onImplement,
  carried = false,
}: {
  plan: PlanRecord;
  disabled: boolean;
  onImplement: () => void;
  /** A plan a branch carried is history; it is implemented where it was made. */
  carried?: boolean;
}) {
  const title = `实施计划 · 版本 ${plan.revision}`;
  const state = plan.plan.current_project_state;
  const requirements = plan.plan.requirements ?? [];
  const steps = plan.plan.steps ?? [];
  return (
    <section aria-label={title} className="agent-plan">
      <header className="agent-plan-header">
        <ToolIcon name="list-checks" size={16} />
        <h3>{title}</h3>
      </header>
      <p className="agent-plan-goal">
        <span className="agent-plan-label">目标</span>
        {plan.original_user_goal}
      </p>
      {state ? (
        <div className="agent-plan-part">
          <span className="agent-plan-label">现状</span>
          <MarkdownBody className="agent-plan-text" text={state} />
        </div>
      ) : null}
      {requirements.length ? (
        <div className="agent-plan-part">
          <span className="agent-plan-label">要求</span>
          <ul aria-label="要求" className="agent-plan-requirements">
            {requirements.map((requirement, index) => (
              <li key={`${index}:${requirement}`}>
                <MarkdownBody className="agent-plan-text" text={requirement} />
              </li>
            ))}
          </ul>
        </div>
      ) : null}
      {steps.length ? (
        <div className="agent-plan-part">
          <span className="agent-plan-label">步骤</span>
          <ol aria-label="步骤" className="agent-plan-steps">
            {steps.map((step) => (
              <li key={step.id}>
                <span className="agent-plan-step-id">{step.id}</span>
                <div className="agent-plan-step-body">
                  <MarkdownBody className="agent-plan-text" text={step.description} />
                  {step.validation ? (
                    <div className="agent-plan-check">
                      <span className="agent-plan-label">验证</span>
                      <MarkdownBody className="agent-plan-text" text={step.validation} />
                    </div>
                  ) : null}
                  {step.depends_on?.length ? (
                    <div className="agent-plan-deps">
                      <span className="agent-plan-label">依赖</span>
                      <span>{step.depends_on.join('、')}</span>
                    </div>
                  ) : null}
                </div>
              </li>
            ))}
          </ol>
        </div>
      ) : null}
      <footer className="agent-plan-footer">
        {carried ? (
          <p className="agent-plan-carried">这是分支前的历史，不能在这里实施。</p>
        ) : (
          <button
            className="agent-plan-implement"
            disabled={disabled}
            onClick={onImplement}
            type="button"
          >
            <ToolIcon name="play" size={14} />
            实施计划
          </button>
        )}
      </footer>
    </section>
  );
}
