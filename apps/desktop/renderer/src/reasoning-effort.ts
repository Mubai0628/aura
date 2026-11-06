import type { ReasoningEffort } from '@aura/contracts/generated';
import { sessionChoices, useSessionChoice } from './session-choices';

/** How each thinking level reads. `xhigh` and `max` are two levels. */
export const REASONING_LABELS: Record<ReasoningEffort, string> = {
  off: '关闭',
  minimal: '最少',
  low: '低',
  medium: '中',
  high: '高',
  xhigh: '超高',
  max: '最高',
};

/**
 * The level a model is sent for a stored one, as Core decides: the level itself when the
 * model offers it; for `xhigh`, which older versions stored for a model whose top level is
 * named `max`, that `max`; otherwise none, and the model's own default applies.
 */
export function levelToSend(
  level: ReasoningEffort,
  offered: readonly ReasoningEffort[],
): ReasoningEffort | null {
  if (offered.includes(level)) return level;
  return level === 'xhigh' && offered.includes('max') ? 'max' : null;
}

/**
 * The thinking level each conversation chose, per model. A conversation (or a new one) that chose
 * none takes the model's default from the model settings.
 */
const reasoningChoices = sessionChoices<ReasoningEffort>('aura.session-reasoning.v1');

/** The level this conversation chose for this model; none follows the model's default. */
export function sessionReasoning(sessionId: string, modelProfileId: string) {
  return reasoningChoices.get(sessionId, modelProfileId);
}

export function chooseSessionReasoning(
  sessionId: string,
  modelProfileId: string,
  level: ReasoningEffort | null,
): void {
  reasoningChoices.choose(sessionId, modelProfileId, level);
}

/** A new conversation takes its draft's choices; a branch takes its source's. */
export function copySessionReasoning(from: string, to: string): void {
  reasoningChoices.copy(from, to);
}

/** The conversation's choice for the model, kept current while the composer shows it. */
export function useSessionReasoning(sessionId: string, modelProfileId: string | undefined) {
  return useSessionChoice(reasoningChoices, sessionId, modelProfileId);
}

/** Tests start from what localStorage holds. */
export function forgetSessionReasoning(): void {
  reasoningChoices.forget();
}
