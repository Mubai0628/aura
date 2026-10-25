import { userMessage } from './agent-session/user-message';
import type {
  AgentModeControlAction,
  AgentModeEpisodeView,
  TodoState,
} from '@aura/contracts/generated';
import type { ReactNode, RefObject } from 'react';
import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';

import { ActivitySummary, type ActivitySummaryProps } from './activity/ActivitySummary';
import { MAX_COMBINED_SIDEBAR_WIDTH_RATIO } from './activity/sidebar-ratio';
import { AppIcon } from './app-icons';
import { ToolIcon } from './tool-icon';
import { todoProgress } from './todo/todo-progress';

/** A full card needs room for its title, mode, activity, actions and errors. */
const FULL_CARD_MIN_WIDTH = 280;
const FULL_CARD_MAX_WIDTH = 320;
const CARD_RIGHT_INSET = 12;

const MODE_LABEL = {
  normal: 'Normal',
  plan: 'Plan',
  goal: 'Goal',
} as const;

interface ActivityNoteStackProps {
  combinedSidebarWidthRatio: number;
  conversationColumnRef: RefObject<HTMLElement | null>;
  viewportRef: RefObject<HTMLElement | null>;
  episode?: AgentModeEpisodeView;
  onControl?: (action: AgentModeControlAction) => Promise<void>;
  /** What the conversation is doing, what is uncommitted, what was done last. */
  summary?: ActivitySummaryProps | undefined;
  /**
   * The model's task
   * list, a part of this card after the conversation's own.
   */
  todo?: ReactNode;
  /**
   * The conversation's Goal - its state, what set it
   * and what it has used - a part of this card after the conversation's own.
   */
  goal?: ReactNode;
  /** The list whose progress the collapsed card shows. */
  taskList?: TodoState | undefined;
  /** Where the narrow window's entry goes — the conversation's header, beside its title. */
  entrySlot?: HTMLElement | null | undefined;
}

export function ActivityNoteStack({
  combinedSidebarWidthRatio,
  conversationColumnRef,
  viewportRef,
  episode,
  onControl,
  summary,
  todo,
  goal,
  taskList,
  entrySlot,
}: ActivityNoteStackProps) {
  const activityRef = useRef<HTMLElement>(null);
  const entryRef = useRef<HTMLButtonElement>(null);
  const [display, setDisplay] = useState<'full' | 'compact'>(() =>
    combinedSidebarWidthRatio >= MAX_COMBINED_SIDEBAR_WIDTH_RATIO ? 'compact' : 'full',
  );
  const [cardWidth, setCardWidth] = useState(FULL_CARD_MAX_WIDTH);
  const [expanded, setExpanded] = useState(false);
  /** Set while the card is closing, so the re-rendered entry takes the focus. */
  const returnFocus = useRef(false);
  const [busy, setBusy] = useState(false);
  const [draft, setDraft] = useState<{
    revision: string;
    tokenBudget: string;
    turnBudget: string;
    editing: boolean;
    error: string | null;
  } | null>(null);
  // A card is only ever shown for a real mode episode: the layout is measured
  // from the actual window, never padded with placeholder activity.
  const liveCard = episode !== undefined;

  useEffect(() => {
    if (!liveCard) return;
    const viewport = viewportRef.current;
    const conversationColumn = conversationColumnRef.current;
    if (viewport === null || conversationColumn === null) return;
    const updateVisibility = () => {
      const viewportRect = viewport.getBoundingClientRect();
      if (viewportRect.width <= 0) return;
      // The conversation column is centered between both sidebars, so the space
      // right of it already accounts for the left sidebar, the right panel and
      // any card the panel itself occupies.
      const available =
        viewportRect.right - CARD_RIGHT_INSET - conversationColumn.getBoundingClientRect().right;
      if (
        available >= FULL_CARD_MIN_WIDTH &&
        combinedSidebarWidthRatio < MAX_COMBINED_SIDEBAR_WIDTH_RATIO
      ) {
        setDisplay('full');
        setCardWidth(Math.min(FULL_CARD_MAX_WIDTH, Math.floor(available)));
        return;
      }
      setDisplay('compact');
      setCardWidth(FULL_CARD_MAX_WIDTH);
    };
    updateVisibility();
    if (typeof ResizeObserver === 'undefined') {
      window.addEventListener('resize', updateVisibility);
      return () => window.removeEventListener('resize', updateVisibility);
    }
    const observer = new ResizeObserver(updateVisibility);
    observer.observe(viewport);
    observer.observe(conversationColumn);
    return () => observer.disconnect();
  }, [combinedSidebarWidthRatio, conversationColumnRef, liveCard, viewportRef]);

  // A compact entry expands into the full card constrained to the window; Escape
  // closes it and returns focus to the entry that opened it.
  useEffect(() => {
    if (!expanded) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.stopPropagation();
      returnFocus.current = true;
      setExpanded(false);
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [expanded]);

  // An open card drops down from the header over the conversation; a click anywhere else
  // closes it, as Escape does.
  useEffect(() => {
    if (!expanded) return;
    const outside = (event: PointerEvent) => {
      if (event.target instanceof Node && !activityRef.current?.contains(event.target))
        setExpanded(false);
    };
    document.addEventListener('pointerdown', outside, true);
    return () => document.removeEventListener('pointerdown', outside, true);
  }, [expanded]);

  // The entry only exists while the card is collapsed, so focus can only return
  // after React rendered it again.
  useLayoutEffect(() => {
    if (expanded || !returnFocus.current) return;
    returnFocus.current = false;
    entryRef.current?.focus();
  }, [expanded]);

  const budgetText = (value: number | null | undefined) => (value != null ? String(value) : '');
  const revision = episode?.revision ?? '';
  const current = draft?.revision === revision ? draft : null;
  const tokenBudget = current?.tokenBudget ?? budgetText(episode?.goal_budget?.max_tokens);
  const turnBudget = current?.turnBudget ?? budgetText(episode?.goal_budget?.max_turns);
  const editing = current?.editing ?? false;
  const error = current?.error;
  const updateDraft = (patch: {
    tokenBudget?: string;
    turnBudget?: string;
    editing?: boolean;
    error?: string | null;
  }) => {
    setDraft((previous) => {
      const base =
        previous?.revision === revision
          ? previous
          : {
              revision,
              tokenBudget: budgetText(episode?.goal_budget?.max_tokens),
              turnBudget: budgetText(episode?.goal_budget?.max_turns),
              editing: false,
              error: null,
            };
      return { ...base, ...patch, revision };
    });
  };

  const run = async (action: AgentModeControlAction) => {
    if (!onControl || busy) return;
    setBusy(true);
    updateDraft({ error: null });
    try {
      await onControl(action);
    } catch (reason) {
      updateDraft({
        error: userMessage(reason, '操作未完成。'),
      });
    } finally {
      setBusy(false);
    }
  };

  const saveBudget = () => {
    const number = (text: string) => (text.trim() === '' ? null : Number(text));
    const goal_budget = {
      max_tokens: number(tokenBudget),
      max_turns: number(turnBudget),
    };
    if (
      Object.values(goal_budget).some(
        (value) => value !== null && (!Number.isSafeInteger(value) || value <= 0),
      )
    ) {
      updateDraft({ error: '预算必须是正整数；留空表示不设置该项上限。' });
      return;
    }
    updateDraft({ editing: false, error: null });
    void run({ action: 'edit_goal_budget', goal_budget });
  };

  const mode = episode?.mode ?? 'normal';
  /** One control at a time: the entry while collapsed, the card's own close while
   *  expanded. Expanding never leaves two overlapping controls on screen. */
  const collapse = () => {
    returnFocus.current = true;
    setExpanded(false);
  };
  if (!liveCard) return null;
  // The mode and its controls: part of the conversation's state in the card.
  const modePart = (
    <div className="activity-mode">
      <div className="activity-row activity-mode-name">
        <span className="activity-label">模式</span>
        <span className="activity-value">
          {MODE_LABEL[mode]}
          {episode.paused ? ' · 已暂停' : ''}
          {episode.disposition_required ? ' · 待裁定' : ''}
          {episode.end_reason === 'skipped' ? ' · 已跳过' : ''}
        </span>
      </div>
      {/* What the Goal has used is live in its own part of the card (GoalDock). */}
      {mode === 'plan' || mode === 'goal' ? (
        <div className="activity-mode-actions">
          <button type="button" disabled={busy} onClick={() => void run({ action: 'terminate' })}>
            终止
          </button>
          {mode === 'goal' && !episode.paused ? (
            <button
              type="button"
              disabled={busy}
              onClick={() => void run({ action: 'pause_goal' })}
            >
              暂停
            </button>
          ) : null}
          {mode === 'goal' ? (
            <button
              type="button"
              disabled={busy}
              onClick={() => updateDraft({ editing: !editing, error: null })}
            >
              {editing ? '取消编辑' : '编辑额度'}
            </button>
          ) : null}
        </div>
      ) : null}
      {mode === 'goal' && editing ? (
        <form
          className="activity-mode-edit"
          onSubmit={(event) => {
            event.preventDefault();
            saveBudget();
          }}
        >
          <label>
            Token 上限
            <input
              type="number"
              min="1"
              step="1"
              value={tokenBudget}
              onChange={(event) => updateDraft({ tokenBudget: event.target.value })}
            />
          </label>
          <label>
            模型步数上限
            <input
              type="number"
              min="1"
              step="1"
              value={turnBudget}
              onChange={(event) => updateDraft({ turnBudget: event.target.value })}
            />
          </label>
          <button type="submit" disabled={busy}>
            保存额度
          </button>
        </form>
      ) : null}
      {error ? <p role="alert">{error}</p> : null}
    </div>
  );
  const collapsed = display === 'compact' && !expanded;
  const progress = todoProgress(taskList);
  const entry =
    display === 'compact' && !expanded ? (
      <button
        aria-expanded="false"
        aria-label={`展开近期活动（当前模式 ${MODE_LABEL[mode]}${progress ? `，待办 ${progress.done}/${progress.total}` : ''}）`}
        className="activity-compact-entry icon-button"
        onClick={() => setExpanded(true)}
        ref={entryRef}
        title="近期活动"
        type="button"
      >
        <ToolIcon name="panel-right-open" />
        <span className="activity-compact-mode">{MODE_LABEL[mode]}</span>
        {progress ? (
          <span className="activity-compact-todo">{`待办 ${progress.done}/${progress.total}`}</span>
        ) : null}
      </button>
    ) : null;

  return (
    <>
      {entrySlot ? createPortal(entry, entrySlot) : entry}
      <aside
        aria-label="近期活动"
        className="activity-stack"
        data-display={collapsed ? 'compact' : 'full'}
        data-expanded={expanded ? 'true' : 'false'}
        data-mode={mode}
        hidden={collapsed}
        ref={activityRef}
        style={{ width: `${cardWidth}px` }}
      >
        <div className="activity-heading">
          <span>近期活动</span>
          {display === 'compact' ? (
            <button
              aria-expanded="true"
              aria-label="收起卡片"
              className="icon-button activity-collapse"
              onClick={collapse}
              type="button"
            >
              <AppIcon name="close" size={13} />
            </button>
          ) : null}
        </div>
        {summary ? (
          <ActivitySummary
            {...summary}
            goal={goal}
            todo={todo}
            onOpenReview={() => {
              // A card dropped down over the conversation goes away once the review is open.
              setExpanded(false);
              summary.onOpenReview();
            }}
          >
            {modePart}
          </ActivitySummary>
        ) : (
          <>
            <section aria-label="会话" className="activity-section">
              {modePart}
            </section>
            {goal}
            {todo}
          </>
        )}
      </aside>
    </>
  );
}
