import type { SessionNavigationItem } from '../app-shell-types';
import type { ConversationOption } from './useReferenceMenu';

/**
 * The conversations of a workspace a message of `current` may refer to: the sidebar's, which
 * leaves out archived ones, without drafts not sent yet and without `current` itself.
 */
export function otherConversations(
  sessions: readonly SessionNavigationItem[] | undefined,
  current: string | undefined,
): ConversationOption[] {
  return (sessions ?? [])
    .filter((session) => session.state !== 'draft' && session.id !== current)
    .map((session) => ({ id: session.id, title: session.title.trim() || '新会话' }));
}
