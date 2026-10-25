import { createContext, useContext } from 'react';
import { fileReference, type FileReference } from './file-paths';
import { openFile, type FileActionScope } from './file-menu';

/**
 * The conversation a path belongs to. Inside it, a path in a message or the
 * file a file tool names opens in a 文件 tab of the right sidebar; elsewhere (a Skill preview, a
 * settings page) the same text stays text.
 */
export type FileLinkScope = FileActionScope;

export const FileLinkContext = createContext<FileLinkScope | undefined>(undefined);

/** The file the text names in this conversation, and how to open it; nothing outside one. */
export function useFileLink(
  text: string,
  options: { file?: boolean } = {},
): { reference: FileReference; scope: FileLinkScope; open: () => void } | undefined {
  const scope = useContext(FileLinkContext);
  const reference = scope ? fileReference(text, scope.root, options) : undefined;
  if (!scope || !reference) return undefined;
  return {
    reference,
    scope,
    open: () => openFile(scope, reference.path, reference.line),
  };
}
