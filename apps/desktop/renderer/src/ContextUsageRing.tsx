import type { ContextUsageSummaryView } from '@aura/contracts/generated';
import type { CSSProperties } from 'react';
import { useId, useRef } from 'react';
import type { InspectionSource } from './agent-session/useLatestInspection';
import { PixelBar } from './inspector/PixelBar';
import { pixelCells } from './inspector/pixel-cells';
import { ToolIcon } from './tool-icon';
import { useDismissablePopover } from './useDismissablePopover';
import './inspector/pixel-inspector.css';
import './context-usage.css';

/** Squares in a row: each is one thirty-second of the window. */
const CELLS = 32;
const count = (value: number) =>
  new Intl.NumberFormat('zh-CN', { notation: 'compact', maximumFractionDigits: 1 }).format(value);
/** Every share with two decimals. */
const share = (part: number, whole: number) => `${((part / Math.max(1, whole)) * 100).toFixed(2)}%`;

/**
 * The context the conversation's requests use, as a ring in the composer; opened, each kind of use is a row of squares in the inspector's colours, as much of the window as
 * it takes, and the last row is the whole use.
 */
export function ContextUsageRing({
  usage,
  source,
  running = false,
  onOpenInspector,
}: {
  usage: ContextUsageSummaryView | undefined;
  source?: InspectionSource | undefined;
  running?: boolean;
  onOpenInspector: () => void;
}) {
  const anchor = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useDismissablePopover(anchor);
  const id = useId();
  const limit = Math.max(1, usage?.runtime_effective_window_tokens ?? 1);
  const used = Math.max(0, usage?.used_input_tokens ?? 0);
  const ratio = Math.min(1, used / limit);
  const percentage = share(ratio, 1);
  const label = usage
    ? `${usage.estimated ? '约 ' : ''}${percentage} · ${count(used)} / ${count(limit)} tokens`
    : source?.kind === 'failed'
      ? '上下文读取失败'
      : source?.kind === 'loading'
        ? '正在读取上下文用量'
        : '本会话尚无模型请求';
  const segments = usage?.segments.filter((segment) => segment.estimated_tokens > 0) ?? [];
  const total = segments.reduce((sum, part) => sum + part.estimated_tokens, 0);
  // An older request must never pass for the current step's own usage.
  const provenance =
    usage === undefined
      ? source?.kind === 'failed'
        ? '读取失败：未能读取最近一次请求的上下文记录，可稍后重试。'
        : source?.kind === 'loading'
          ? '正在读取最近一次模型请求的上下文记录。'
          : '本会话还没有形成模型请求，因此没有可展示的上下文用量。'
      : source?.kind === 'recent'
        ? '数据来自本会话最近一次真实模型请求（当前步骤尚未形成请求）。'
        : undefined;
  return (
    <div className="context-ring-anchor" ref={anchor}>
      <button
        className="context-ring-button"
        type="button"
        aria-label={`上下文占用：${label}`}
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        onClick={() => setOpen(!open)}
        title={label}
      >
        <span
          className="context-ring"
          role="progressbar"
          aria-label="上下文使用情况"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={Number((ratio * 100).toFixed(2))}
          aria-valuetext={label}
          style={{ '--context-angle': `${ratio * 360}deg` } as CSSProperties}
        />
      </button>
      {open && (
        <section className="context-pixel" id={id} aria-label="上下文占用详情">
          <header className="context-pixel-head">
            <span aria-hidden="true" className="pixel-mark" />
            <span>上下文</span>
            {running ? (
              <span className="context-pixel-badge" role="status">
                压缩中
              </span>
            ) : null}
            {usage && (
              <button
                aria-label="检查上下文详情"
                className="icon-button context-pixel-link"
                onClick={() => {
                  setOpen(false);
                  onOpenInspector();
                }}
                title="上下文详情"
                type="button"
              >
                <ToolIcon name="external-link" size={13} />
              </button>
            )}
          </header>
          {usage ? (
            <ol className="pixel-rows context-pixel-rows">
              {segments.map((segment) => (
                <li
                  key={segment.kind}
                  title={`${segment.label} · ${count(segment.estimated_tokens)} · 占已用 ${share(
                    segment.estimated_tokens,
                    total,
                  )}`}
                >
                  <span className="pixel-row-name">{segment.label}</span>
                  <PixelBar
                    cells={pixelCells(
                      [{ key: segment.kind, value: segment.estimated_tokens }],
                      limit,
                      CELLS,
                    )}
                    label={`${segment.label} ${count(segment.estimated_tokens)}`}
                  />
                  <b>{share(segment.estimated_tokens, limit)}</b>
                </li>
              ))}
              <li
                className="is-total"
                title={`${usage.estimated ? '估算' : '实测'}：约 ${count(used)} / ${count(limit)} tokens`}
              >
                <span className="pixel-row-name">总占用</span>
                <PixelBar
                  cells={pixelCells(
                    segments.map((segment) => ({
                      key: segment.kind,
                      value: segment.estimated_tokens,
                    })),
                    limit,
                    CELLS,
                  )}
                  label={`总占用 ${count(used)} / ${count(limit)}`}
                />
                <b>{percentage}</b>
              </li>
            </ol>
          ) : null}
          {provenance === undefined ? null : (
            <p className={usage ? 'sr-only' : 'pixel-caption'} role="note">
              {provenance}
            </p>
          )}
        </section>
      )}
    </div>
  );
}
