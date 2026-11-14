import type { AgentSessionBranchResult } from '@aura/contracts/generated';
import { readSessionModelSelector } from '../model-projection-store';
import { copySessionReasoning } from '../reasoning-effort';
import { copySessionSpeed } from '../speed-mode';

/** A branch made; `modelNotKept` when it answers with the default model instead. */
export type BranchMade = AgentSessionBranchResult & { modelNotKept?: true };

/**
 * Makes a branch of a conversation at one finished turn, then gives it the conversation's
 * model choice (the design's "模型选择 → 继承"). Core carries the history and the permission mode;
 * the model is set through the same command the composer uses, so its checks apply. If that step
 * fails the branch keeps the default model, which the composer shows, and the result says so.
 */
export async function branchFrom(sessionId: string, turnTaskId: string): Promise<BranchMade> {
  const made = await window.aura.v1.branchSession({
    session_id: sessionId,
    turn_task_id: turnTaskId,
  });
  if (!made.ok) throw made.error;
  // The branch keeps the conversation's thinking levels too.
  copySessionReasoning(sessionId, made.value.session_id);
  copySessionSpeed(sessionId, made.value.session_id);
  return (await keepModel(sessionId, made.value.session_id))
    ? made.value
    : { ...made.value, modelNotKept: true };
}

/** Whether the branch answers with the conversation's model (or there was none to keep). */
async function keepModel(source: string, branch: string): Promise<boolean> {
  try {
    const model = (await readSessionModelSelector(source)).payload.selected_model_profile_id;
    if (!model) return true;
    // The new conversation's own binding revision, as a new conversation is bound.
    const selector = (await readSessionModelSelector(branch)).payload;
    if (selector.selected_model_profile_id === model) return true;
    const bound = await window.aura.v1.setSessionModelBinding({
      session_id: branch,
      expected_revision: selector.binding_revision,
      model_profile_id: model,
    });
    return bound.ok;
  } catch {
    // The branch is made; it answers with the default model until another is chosen.
    return false;
  }
}
