import { createContext } from 'react';
import type { AgentMessageTextSource } from '@aura/contracts/generated';
import type { MentionSource } from '../../../shared/attachment-files';

/**
 * The message the text on screen
 * belongs to, so a full path outside the workspace in it can be opened — Core finds the path in
 * that message before saying where the file is.
 */
export const MentionContext = createContext<MentionSource | undefined>(undefined);

/** The message a body's text comes from, as a mention; nothing for reasoning. */
export function mentionOf(source: AgentMessageTextSource): MentionSource | undefined {
  switch (source.kind) {
    case 'user':
      return { kind: 'user', event_id: source.event_id };
    case 'assistant':
      return { kind: 'assistant', model_step_id: source.model_step_id };
    case 'completion':
      return { kind: 'completion', turn_task_id: source.turn_task_id };
    default:
      return undefined;
  }
}
