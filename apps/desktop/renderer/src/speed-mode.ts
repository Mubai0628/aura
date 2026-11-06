import type { ModelSettingsProjection, ModelSpeedModesView } from '@aura/contracts/generated';
import { sessionChoices, useSessionChoice } from './session-choices';

/** How the modes models.dev lists read; any other mode reads by its own name. */
const SPEED_LABELS: Record<string, string> = {
  fast: '快速',
  flex: '弹性',
  priority: '优先',
  ultrafast: '极速',
};

export const speedLabel = (name: string): string => SPEED_LABELS[name] ?? name;

/** The speed modes Core lists for a model, when it offers any. */
export const speedModesOf = (
  settings: ModelSettingsProjection,
  modelProfileId: string | undefined,
): ModelSpeedModesView | undefined =>
  settings.speed_modes?.find((entry) => entry.model_profile_id === modelProfileId);

/**
 * The speed mode each conversation chose, per model: one the model's settings allow. None is the
 * standard mode; a conversation that chose a mode the model no longer offers is sent the standard
 * mode, as Core decides.
 */
const speedChoices = sessionChoices<string>('aura.session-speed.v1');

export const sessionSpeed = (sessionId: string, modelProfileId: string) =>
  speedChoices.get(sessionId, modelProfileId);

export const chooseSessionSpeed = (
  sessionId: string,
  modelProfileId: string,
  mode: string | null,
): void => speedChoices.choose(sessionId, modelProfileId, mode);

/** A new conversation takes its draft's choices; a branch takes its source's. */
export const copySessionSpeed = (from: string, to: string): void => speedChoices.copy(from, to);

export const useSessionSpeed = (sessionId: string, modelProfileId: string | undefined) =>
  useSessionChoice(speedChoices, sessionId, modelProfileId);

/** Tests start from what localStorage holds. */
export const forgetSessionSpeed = (): void => speedChoices.forget();
