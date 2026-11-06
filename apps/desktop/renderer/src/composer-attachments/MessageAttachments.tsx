import type { AgentMessageAttachment } from '@aura/contracts/generated';
import { useCallback, useContext, useEffect, useState } from 'react';

import { carriedMenuEntries, openCarried } from '../files/carried-menu';
import { FileLinkContext } from '../files/file-links';
import type { FileActionScope } from '../files/file-menu';
import { useContextMenu } from '../ui/useContextMenu';
import { useNearView } from '../ui/useNearView';
import { FileCard } from './AttachmentCards';
import { PicturePreview } from './PicturePreview';
import { pictureMenuEntries } from './picture-menu';
import { readPicture } from './picture-reads';
import './composer-attachments.css';

/** One picture of a message, read back from the conversation's own record. */
function SentPicture({
  sessionId,
  artifactId,
  name,
  scope,
}: {
  sessionId: string;
  artifactId: string;
  name: string;
  scope: FileActionScope | undefined;
}) {
  const [url, setUrl] = useState<string | null>();
  const [previewing, setPreviewing] = useState(false);
  const [spot, near] = useNearView<HTMLLIElement>();
  const menu = useContextMenu();
  // Stable while the viewer is open, so its focus is kept for its whole life.
  const close = useCallback(() => setPreviewing(false), []);
  useEffect(() => {
    if (!near) return;
    let current = true;
    void readPicture(sessionId, artifactId).then((read) => {
      if (current && read !== undefined) setUrl(read);
    });
    return () => {
      current = false;
    };
  }, [near, sessionId, artifactId]);
  return (
    <li className="attachment-picture" ref={spot} title={name}>
      {url ? (
        <button
          aria-label={`预览 ${name}`}
          className="attachment-picture-open"
          onClick={() => setPreviewing(true)}
          onContextMenu={(event) =>
            menu.open(
              event,
              `${name} 的操作`,
              pictureMenuEntries(scope, sessionId, artifactId, name, () => setPreviewing(true)),
            )
          }
          type="button"
        >
          <img alt={name} src={url} />
        </button>
      ) : url === null ? (
        <span className="attachment-picture-missing" role="status">
          图片不可用
        </span>
      ) : null}
      {previewing && url ? <PicturePreview name={name} onClose={close} src={url} /> : null}
      {menu.menu}
    </li>
  );
}

/** One file of a message: a click opens it, its menu has what can be done with it. */
function SentFile({
  attachment,
  scope,
}: {
  attachment: Exclude<AgentMessageAttachment, { kind: 'image' }>;
  scope: FileActionScope | undefined;
}) {
  const menu = useContextMenu();
  return (
    <>
      <FileCard
        bytes={attachment.byte_length}
        name={attachment.name}
        title={attachment.kind === 'text' ? attachment.name : attachment.path}
        {...(scope
          ? {
              onOpen: () => openCarried(scope, attachment),
              onMenu: (event) =>
                menu.open(
                  event,
                  `${attachment.name} 的操作`,
                  carriedMenuEntries(scope, attachment),
                ),
            }
          : {})}
      />
      {menu.menu}
    </>
  );
}

/**
 * The pictures and files sent with a message, under what the user typed: the pictures in
 * a row, the files as cards. A card opens its file in the sidebar, and every
 * picture and file has a menu; a model's message showing pictures and files will use the same.
 */
export function MessageAttachments({
  sessionId,
  attachments,
}: {
  sessionId: string;
  attachments: readonly AgentMessageAttachment[];
}) {
  const scope = useContext(FileLinkContext);
  if (!attachments.length) return null;
  const pictures = attachments.filter((attachment) => attachment.kind === 'image');
  const files = attachments.filter((attachment) => attachment.kind !== 'image');
  return (
    <div className="message-attachments">
      {pictures.length ? (
        <ul aria-label="发送的图片" className="attachment-pictures">
          {pictures.map((picture) => (
            <SentPicture
              artifactId={picture.artifact_id}
              key={picture.artifact_id}
              name={picture.name}
              scope={scope}
              sessionId={sessionId}
            />
          ))}
        </ul>
      ) : null}
      {files.length ? (
        <ul aria-label="发送的文件" className="attachment-files">
          {files.map((file) => (
            <SentFile
              attachment={file}
              key={
                file.kind === 'text'
                  ? file.artifact_id
                  : file.kind === 'external_file'
                    ? `external:${file.external_grant_id}`
                    : file.path
              }
              scope={scope}
            />
          ))}
        </ul>
      ) : null}
    </div>
  );
}
