import { AppIcon } from '../app-icons';
import type { FileActionScope } from '../files/file-menu';
import { useContextMenu } from '../ui/useContextMenu';
import type { ComposerAttachment } from './attachment-files';
import { FileCard } from './AttachmentCards';
import { composerFileEntries } from './composer-file-menu';
import { PreviewPicture } from './PicturePreview';
import './composer-attachments.css';

/**
 * The composer's pictures in a row of thumbnails, and
 * its files below them as cards with their type's icon.
 */
export function ComposerAttachments({
  attachments,
  error,
  onRemove,
  scope,
}: {
  attachments: readonly ComposerAttachment[];
  error?: string | undefined;
  onRemove: (id: string) => void;
  /** The conversation's workspace files, for a card's menu; none before a conversation exists. */
  scope?: FileActionScope | undefined;
}) {
  const pictures = attachments.filter((attachment) => attachment.kind === 'image');
  const files = attachments.filter((attachment) => attachment.kind !== 'image');
  if (!attachments.length && !error) return null;
  return (
    <div className="composer-attachments">
      {pictures.length ? (
        <ul aria-label="附加的图片" className="attachment-pictures">
          {pictures.map((picture) => (
            <li className="attachment-picture" key={picture.id} title={picture.name}>
              <PreviewPicture
                name={picture.name}
                src={`data:${picture.mediaType};base64,${picture.dataBase64}`}
              />
              <button
                aria-label={`移除 ${picture.name}`}
                className="attachment-remove"
                onClick={() => onRemove(picture.id)}
                type="button"
              >
                <AppIcon name="close" size={12} />
              </button>
            </li>
          ))}
        </ul>
      ) : null}
      {files.length ? (
        <ul aria-label="附加的文件" className="attachment-files">
          {files.map((file) => (
            <ComposerFile
              file={file}
              key={file.id}
              onRemove={() => onRemove(file.id)}
              scope={scope}
            />
          ))}
        </ul>
      ) : null}
      {error ? (
        <p className="attachment-error" role="alert">
          {error}
        </p>
      ) : null}
    </div>
  );
}

/** A file in the composer, with its menu. */
function ComposerFile({
  file,
  onRemove,
  scope,
}: {
  file: Exclude<ComposerAttachment, { kind: 'image' }>;
  onRemove: () => void;
  scope: FileActionScope | undefined;
}) {
  const menu = useContextMenu();
  return (
    <>
      <FileCard
        bytes={file.bytes}
        name={file.name}
        title={file.kind === 'file' ? file.path : file.name}
        onMenu={(event) =>
          menu.open(event, `${file.name} 的操作`, composerFileEntries(scope, file, onRemove))
        }
        onRemove={onRemove}
      />
      {menu.menu}
    </>
  );
}
