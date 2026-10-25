import type { AgentMessageAttachment } from '@aura/contracts/generated';
import { useCallback, useContext, useEffect, useState } from 'react';

import { AppIcon } from '../app-icons';
import { carriedMenuEntries, openCarried } from '../files/carried-menu';
import { FileLinkContext } from '../files/file-links';
import type { FileActionScope } from '../files/file-menu';
import { ToolIcon } from '../tool-icon';
import { useContextMenu } from '../ui/useContextMenu';
import { FileCard } from './AttachmentCards';
import { PicturePreview } from './PicturePreview';
import './composer-attachments.css';

/** A picture copied as a picture, or saved where the user chooses; Main reads it from Core. */
async function pictureAction(
  scope: FileActionScope,
  sessionId: string,
  artifactId: string,
  name: string,
  action: 'copy' | 'save',
) {
  const done = await window.aura.v1.messagePicture({
    action,
    session_id: sessionId,
    artifact_id: artifactId,
    name,
  });
  if (!done.completed && done.message) scope.notify(done.message);
}

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
  const menu = useContextMenu();
  // Stable while the viewer is open, so its focus is kept for its whole life.
  const close = useCallback(() => setPreviewing(false), []);
  useEffect(() => {
    let current = true;
    void window.aura.v1.browser
      ?.readScreenshot({ session_id: sessionId, artifact_id: artifactId })
      .then((result) => {
        if (!current) return;
        setUrl(
          result.ok ? `data:${result.value.media_type};base64,${result.value.data_base64}` : null,
        );
      });
    return () => {
      current = false;
    };
  }, [sessionId, artifactId]);
  return (
    <li className="attachment-picture" title={name}>
      {url ? (
        <button
          aria-label={`预览 ${name}`}
          className="attachment-picture-open"
          onClick={() => setPreviewing(true)}
          onContextMenu={(event) =>
            menu.open(event, `${name} 的操作`, [
              {
                label: '预览',
                icon: <AppIcon name="eye" size={14} />,
                run: () => setPreviewing(true),
              },
              ...(scope
                ? [
                    {
                      label: '复制图片',
                      icon: <ToolIcon name="copy" size={14} />,
                      run: () => void pictureAction(scope, sessionId, artifactId, name, 'copy'),
                    },
                    {
                      label: '另存为…',
                      icon: <AppIcon name="file" size={14} />,
                      run: () => void pictureAction(scope, sessionId, artifactId, name, 'save'),
                    },
                  ]
                : []),
            ])
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
