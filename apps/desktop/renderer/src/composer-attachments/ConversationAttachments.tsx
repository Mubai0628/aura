import { useContext, useRef, useState, type ReactNode } from 'react';

import { useComposerAttachments } from './useComposerAttachments';
import { ConversationAttachmentContext } from './conversation-attachment-context';
import './composer-attachments.css';

export function ConversationAttachmentState({ children }: { children: ReactNode }) {
  const attached = useComposerAttachments();
  return (
    <ConversationAttachmentContext.Provider value={attached}>
      {children}
    </ConversationAttachmentContext.Provider>
  );
}

/** The conversation column owns one attachment state for its list, composer and empty page. */
export function ConversationAttachments({
  className,
  children,
}: {
  className?: string;
  children: ReactNode;
}) {
  return (
    <ConversationAttachmentState>
      <ConversationDropArea className={className}>{children}</ConversationDropArea>
    </ConversationAttachmentState>
  );
}

function ConversationDropArea({
  className,
  children,
}: {
  className: string | undefined;
  children: ReactNode;
}) {
  const attached = useContext(ConversationAttachmentContext)!;
  const depth = useRef(0);
  const [over, setOver] = useState(false);
  return (
    <section
      aria-label="当前会话"
      className={`${className ?? ''}${over ? ' is-file-drop-target' : ''}`}
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
