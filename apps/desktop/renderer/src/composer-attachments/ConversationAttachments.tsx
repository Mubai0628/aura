import { useContext, useRef, useState, type ReactNode } from 'react';

import { useComposerAttachments } from './useComposerAttachments';
import { ConversationAttachmentContext } from './conversation-attachment-context';
import './composer-attachments.css';

/** `draftKey` names the conversation whose pictures and files are saved. */
export function ConversationAttachmentState({
  draftKey,
  draftWorkspaceId,
  children,
}: {
  draftKey?: string | undefined;
  draftWorkspaceId?: string | undefined;
  children: ReactNode;
}) {
  const attached = useComposerAttachments(undefined, draftKey, draftWorkspaceId);
  return (
    <ConversationAttachmentContext.Provider value={attached}>
      {children}
    </ConversationAttachmentContext.Provider>
  );
}

/** The conversation column owns one attachment state for its list, composer and empty page. */
export function ConversationAttachments({
  className,
  draftKey,
  draftWorkspaceId,
  inert,
  children,
}: {
  className?: string;
  draftKey?: string | undefined;
  draftWorkspaceId?: string | undefined;
  inert?: boolean | undefined;
  children: ReactNode;
}) {
  return (
    <ConversationAttachmentState draftKey={draftKey} draftWorkspaceId={draftWorkspaceId}>
      <ConversationDropArea className={className} inert={inert}>
        {children}
      </ConversationDropArea>
    </ConversationAttachmentState>
  );
}

function ConversationDropArea({
  className,
  children,
  inert,
}: {
  className: string | undefined;
  children: ReactNode;
  inert: boolean | undefined;
}) {
  const attached = useContext(ConversationAttachmentContext)!;
  const depth = useRef(0);
  const [over, setOver] = useState(false);
  return (
    <section
      aria-label="当前会话"
      className={`${className ?? ''}${over ? ' is-file-drop-target' : ''}`}
      inert={inert}
      onDragEnter={(event) => {
        if (!event.dataTransfer.types.includes('Files')) return;
        event.preventDefault();
        depth.current += 1;
        setOver(true);
      }}
      onDragOver={(event) => {
        if (!event.dataTransfer.types.includes('Files')) return;
        event.preventDefault();
        event.dataTransfer.dropEffect = 'copy';
      }}
      onDragLeave={() => {
        depth.current = Math.max(0, depth.current - 1);
        if (depth.current === 0) setOver(false);
      }}
      onDrop={(event) => {
        depth.current = 0;
        setOver(false);
        if (!event.dataTransfer.files.length) return;
        event.preventDefault();
        void attached.add([...event.dataTransfer.files]);
      }}
    >
      {children}
    </section>
  );
}
