import type { PromptInspectorProjection } from '@aura/contracts/generated';
import type { ReactNode } from 'react';
import { PixelBar, PixelCard } from './inspector/PixelBar';
import { COMPACTION_LABELS, amount, share } from './inspector/inspector-labels';
import { pixelCells } from './inspector/pixel-cells';
import './inspector/pixel-inspector.css';

/**
 * The step's request at a glance: the context it used, part by part, and how its
 * window was budgeted, as pixel bars with their numbers.
 */
export function PromptInspectorShell({
  projection,
  chips,
}: {
  projection: PromptInspectorProjection | undefined;
  /** What the step ran on, beside the title. */
  chips?: ReactNode;
}) {
  if (projection === undefined) {
    return (
      <section aria-label="提示词监测" className="pixel-inspector is-empty">
        <header className="pixel-head">
          <h3>提示词监测</h3>
        </header>
        <p className="pixel-muted">尚无已提交的上下文快照</p>
      </section>
    );
  }
  const usage = projection.context_usage;
  const budget = projection.budget;
  return (
    <section aria-label="提示词监测" className="pixel-inspector">
      <header className="pixel-head">
        <h3>提示词监测</h3>
        {chips}
      </header>
      <PixelCard
        title="上下文"
        figure={
          usage ? share(usage.used_input_tokens, usage.runtime_effective_window_tokens) : undefined
        }
      >
        {usage ? (
          <>
            <p className="pixel-total">
              <strong>{amount(usage.used_input_tokens)}</strong>
              <span>/ {amount(usage.runtime_effective_window_tokens)}</span>
            </p>
            <PixelBar
              cells={pixelCells(
                usage.segments.map((segment) => ({
                  key: segment.kind,
                  value: segment.estimated_tokens,
                })),
                usage.runtime_effective_window_tokens,
                32,
              )}
              label={`上下文 ${amount(usage.used_input_tokens)} / ${amount(
                usage.runtime_effective_window_tokens,
              )}`}
            />
            <ul className="pixel-legend">
              {usage.segments.map((segment) => (
                <li className={`is-${segment.kind}`} key={segment.kind}>
                  <span aria-hidden="true" className="pixel-swatch" />
                  <span>{segment.label}</span>
                  <b>{amount(segment.estimated_tokens)}</b>
                </li>
              ))}
            </ul>
          </>
        ) : (
          <p className="pixel-muted">无用量</p>
        )}
      </PixelCard>
      <PixelCard title="预算" figure={COMPACTION_LABELS[projection.compaction.state]}>
        {budget ? (
          <>
            <PixelBar
              cells={pixelCells(
                [
                  {
                    key: 'used',
                    value: Math.min(
                      budget.estimated_input_tokens,
                      budget.request_input_budget_tokens,
                    ),
                  },
                  {
                    key: 'input',
                    value: Math.max(
                      0,
                      budget.request_input_budget_tokens - budget.estimated_input_tokens,
                    ),
                  },
                  { key: 'output', value: budget.output_reserve_tokens },
                  { key: 'safety', value: budget.safety_reserve_tokens },
                ],
                budget.runtime_effective_window_tokens,
                32,
              )}
              label={`预算 输入 ${amount(budget.request_input_budget_tokens)} / 输出 ${amount(
                budget.output_reserve_tokens,
              )} / 安全 ${amount(budget.safety_reserve_tokens)}`}
            />
            <ul className="pixel-legend">
              <li className="is-input">
                <span aria-hidden="true" className="pixel-swatch" />
                <span>输入</span>
                <b>{amount(budget.request_input_budget_tokens)}</b>
              </li>
              <li className="is-output">
                <span aria-hidden="true" className="pixel-swatch" />
                <span>输出</span>
                <b>{amount(budget.output_reserve_tokens)}</b>
              </li>
              <li className="is-safety">
                <span aria-hidden="true" className="pixel-swatch" />
                <span>安全</span>
                <b>{amount(budget.safety_reserve_tokens)}</b>
              </li>
            </ul>
          </>
        ) : (
          <p className="pixel-muted">尚未生成</p>
        )}
      </PixelCard>
    </section>
  );
}
