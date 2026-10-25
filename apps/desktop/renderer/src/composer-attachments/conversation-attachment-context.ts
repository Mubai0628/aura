import { createContext } from 'react';
import type { useComposerAttachments } from './useComposerAttachments';

export const ConversationAttachmentContext = createContext<
  ReturnType<typeof useComposerAttachments> | undefined
>(undefined);
