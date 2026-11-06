import { AppIcon } from '../../app-icons';

/**
 * Moving between the questions of one card: the one before, a dot for each (filled once
 * answered), and the one after.
 */
export function QueryPager({
  count,
  current,
  answered,
  disabled,
  onShow,
}: {
  count: number;
  current: number;
  answered: readonly boolean[];
  disabled: boolean;
  onShow: (index: number) => void;
}) {
  return (
    <div className="agent-query-pager" role="group" aria-label="切换问题">
      <button
        type="button"
        className="button is-ghost is-small"
        aria-label="上一题"
        disabled={disabled || current === 0}
        onClick={() => onShow(current - 1)}
      >
        <AppIcon name="chevron-left" size={14} />
      </button>
      <span className="agent-query-dots">
        {Array.from({ length: count }, (_, index) => (
          <button
            type="button"
            key={index}
            className="agent-query-dot"
            aria-label={`第 ${index + 1} 题${answered[index] ? '（已答）' : ''}`}
            aria-current={index === current ? 'step' : undefined}
            data-answered={answered[index] || undefined}
            disabled={disabled}
            onClick={() => onShow(index)}
          />
        ))}
      </span>
      <button
        type="button"
        className="button is-ghost is-small"
        aria-label="下一题"
        disabled={disabled || current === count - 1}
        onClick={() => onShow(current + 1)}
      >
        <AppIcon name="chevron-right" size={14} />
      </button>
    </div>
  );
}
