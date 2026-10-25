import type { MouseEvent, ReactElement } from 'react';

import { AppIcon } from '../app-icons';
import { fileKind, sizeLabel, typeLabel, type FileKind } from './attachment-files';

const GLYPHS: Record<FileKind, ReactElement> = {
  pdf: <path d="M8 13h1.5a1.5 1.5 0 0 0 0-3H8v6M13 10v6h1a2 2 0 0 0 2-2v-2a2 2 0 0 0-2-2h-1" />,
  doc: <path d="M8 10l1.2 6 1.8-4 1.8 4L14 10M16 10h0" />,
  sheet: <path d="M8 10h8M8 13h8M8 16h8M12 10v6" />,
  slides: <path d="M8 10h8v5H8zM12 15v2" />,
  archive: <path d="M12 6v2M12 10v2M12 14v2M10.5 17h3" />,
  code: <path d="m10 10-2.5 2.5L10 15M14 10l2.5 2.5L14 15" />,
  text: <path d="M8 10h8M8 13h8M8 16h5" />,
  image: <path d="m7.5 16 3-3.5 2 2.2 1.5-1.7 2.5 3M14.5 10h0" />,
  audio: <path d="M10 16.5a1.5 1.5 0 1 1 0-3 1.5 1.5 0 0 1 0 3Zm1.5-1.5V9l4-1v5.5" />,
  video: <path d="M8 10h6v5H8zM14 11.5l2.5-1.5v5l-2.5-1.5" />,
  other: <path d="M8 12h8M8 15h6" />,
};

/** A file's icon: a page in its kind's colour with that kind's mark. */
export function FileIcon({ name, size = 32 }: { name: string; size?: number }) {
  const kind = fileKind(name);
  return (
    <span aria-hidden="true" className="attachment-file-icon" data-kind={kind}>
      <svg fill="none" height={size} viewBox="0 0 24 24" width={size}>
        <path
          className="attachment-file-page"
          d="M6 3h8.5L19 7.5V20a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1Z"
        />
        <path className="attachment-file-fold" d="M14.5 3v4.5H19" />
        <g className="attachment-file-mark">{GLYPHS[kind]}</g>
      </svg>
    </span>
  );
}

/**
 * A file's card: its icon, name, type and size, and a button to take it out (in the composer).
 * In a message the card opens its file, and has a menu. So has a composer's card.
 */
export function FileCard({
  name,
  bytes,
  title,
  onRemove,
  onOpen,
  onMenu,
}: {
  name: string;
  bytes: number;
  title?: string | undefined;
  onRemove?: (() => void) | undefined;
  onOpen?: (() => void) | undefined;
  onMenu?: ((event: MouseEvent<HTMLElement>) => void) | undefined;
}) {
  const facts = (
    <>
      <FileIcon name={name} />
      <span className="attachment-file-text">
        <span className="attachment-file-name">{name}</span>
        <span className="attachment-file-meta">
          {typeLabel(name)} · {sizeLabel(bytes)}
        </span>
      </span>
    </>
  );
  return (
    <li className="attachment-file-card" onContextMenu={onMenu} title={title ?? name}>
      {onOpen ? (
        <button
          aria-label={`打开 ${name}`}
          className="attachment-file-open"
          onClick={onOpen}
          type="button"
        >
          {facts}
        </button>
      ) : (
        facts
      )}
      {onRemove ? (
        <button
          aria-label={`移除 ${name}`}
          className="attachment-remove"
          onClick={onRemove}
          type="button"
        >
          <AppIcon name="close" size={12} />
        </button>
      ) : null}
    </li>
  );
}
