import type { ReasoningEffort } from '@aura/contracts/generated';
import { useRef, useState } from 'react';
import { AppIcon } from './app-icons';
import { userMessage } from './agent-session/user-message';
import { useDismissablePopover } from './useDismissablePopover';
import {
  REASONING_LABELS,
  chooseSessionReasoning,
  levelToSend,
  useSessionReasoning,
} from './reasoning-effort';
import './styles/model-picker.css';

/**
 * The model's
 * thinking level, beside the model. "默认" is the provider's own default and sends nothing; the
 * other choices are the levels this model offers, so a model that takes none shows no choice. The
 * level belongs to the model profile, as its other request settings do. In the composer's row its
 * words give way first, so the permission's and the model's stay where they fit before.
 *
 * In the composer the level is the conversation's own (`SessionReasoningPicker`); the
 * model settings keep the default a conversation that chose none follows.
 */
export function ReasoningPicker({
  levels,
  level,
  inherited,
  onSelect,
}: {
  levels: readonly ReasoningEffort[];
  level: ReasoningEffort | undefined;
  /** The level "默认" stands for here — the model settings' — named after it. */
  inherited?: ReasoningEffort | null | undefined;
  onSelect: (level: ReasoningEffort | null) => Promise<void>;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useDismissablePopover(ref);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string>();
  if (!levels.length) return null;
  const followed = inherited ? levelToSend(inherited, levels) : null;
  const byDefault = followed ? `默认（${REASONING_LABELS[followed]}）` : '默认';
  const current = level && levels.includes(level) ? REASONING_LABELS[level] : byDefault;
  const choose = async (next: ReasoningEffort | null) => {
    if (pending) return;
    setPending(true);
    setError(undefined);
    try {
      await onSelect(next);
      setOpen(false);
    } catch (reason) {
      setError(userMessage(reason, '思考程度没有保存，请重试。'));
    } finally {
      setPending(false);
    }
  };
  const choices: { value: ReasoningEffort | null; label: string }[] = [
    { value: null, label: byDefault },
    ...levels.map((value) => ({ value, label: REASONING_LABELS[value] })),
  ];
  return (
    <div className="composer-menu-anchor reasoning-menu-anchor" ref={ref}>
      <button
        aria-expanded={open}
        aria-haspopup="menu"
        aria-label={`思考程度：${current}`}
        className="composer-select reasoning-select"
        data-adaptive
        data-adaptive-first
        onClick={() => setOpen(!open)}
        title={`思考程度：${current}`}
        type="button"
      >
        <AppIcon name="spark" size={14} />
        <span data-adaptive-label>{current}</span>
        <AppIcon className="adaptive-chevron" name="chevron-down" size={10} />
      </button>
      {open && (
        <div aria-label="思考程度" className="composer-popover reasoning-popover" role="menu">
          {choices.map((choice) => {
            const checked =
              (choice.value ?? undefined) === (level && levels.includes(level) ? level : undefined);
            return (
              <button
                aria-checked={checked}
                disabled={pending}
                key={choice.label}
                onClick={() => void choose(choice.value)}
                role="menuitemradio"
                type="button"
              >
                <span>{choice.label}</span>
                {checked && <AppIcon name="check" size={14} />}
              </button>
            );
          })}
          {error && <p role="alert">{error}</p>}
        </div>
      )}
    </div>
  );
}

/** The composer's thinking level, the conversation's own choice for its model. */
export function SessionReasoningPicker({
  sessionId,
  modelProfileId,
  levels,
  fallback,
}: {
  sessionId: string;
  modelProfileId: string;
  levels: readonly ReasoningEffort[];
  /** The model settings' default, which a conversation that chose none follows. */
  fallback?: ReasoningEffort | undefined;
}) {
  const level = useSessionReasoning(sessionId, modelProfileId);
  return (
    <ReasoningPicker
      key={modelProfileId}
      inherited={fallback}
      level={level}
      levels={levels}
      onSelect={async (next) => chooseSessionReasoning(sessionId, modelProfileId, next)}
    />
  );
}
