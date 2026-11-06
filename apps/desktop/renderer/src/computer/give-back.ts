import type { ComputerControlledV1 } from '@aura/contracts/generated';

import { acceptSending, endSending, startSending } from '../agent-session/sending';
import { userMessage } from '../agent-session/user-message';
import { sessionReasoning } from '../reasoning-effort';
import { sessionSpeed } from '../speed-mode';

/** The conversation a desktop is given back in. */
export interface GiveBackConversation {
  /** The turn that runs now: the message joins it. Without one, the message starts the next. */
  activeTurn?: string | undefined;
  /** The conversation's model, whose choices go with a message that starts a turn. */
  modelId?: string | undefined;
  /** The conversation changed: it reads again. */
  onChanged?: (() => void) | undefined;
}

/** What the model is told once the user gives the desktop back. */
export const givenBack = (app: ComputerControlledV1): string =>
  app.mode === 'child_session' ? '我已交还小窗的操作权，继续。' : '我已交还电脑的操作权，继续。';

/**
 * Tells the model the desktop is given back, as the user's message: in the running turn, or as
 * the next one with the conversation's thinking level and speed as the composer would send them,
 * on the desktop of the row. It shows at once, as one sent from the composer does. Answers what
 * went wrong, or nothing.
 */
export async function tellModel(
  sessionId: string,
  app: ComputerControlledV1,
  conversation: GiveBackConversation,
): Promise<string | undefined> {
  const { activeTurn, modelId, onChanged } = conversation;
  const text = givenBack(app);
  const level = modelId ? sessionReasoning(sessionId, modelId) : undefined;
  const speed = modelId ? sessionSpeed(sessionId, modelId) : undefined;
  startSending(sessionId, { text, files: [] });
  try {
    const result = await window.aura.v1.submitAgentMessage({
      session_id: sessionId,
      text,
      destination: activeTurn
        ? { lane: 'addition', turn_task_id: activeTurn }
        : { lane: 'next_turn' },
      computer_mode: app.mode ?? 'foreground',
      ...(level ? { reasoning_effort: level } : {}),
      ...(speed ? { speed_mode: speed } : {}),
    });
    if (result.ok) {
      acceptSending(sessionId);
      return undefined;
    }
    endSending(sessionId);
    return userMessage(result.error, '操作权已交还，但消息没有发出。');
  } catch (reason) {
    endSending(sessionId);
    return userMessage(reason, '操作权已交还，消息结果未确认，请刷新后核对。');
  } finally {
    onChanged?.();
  }
}
