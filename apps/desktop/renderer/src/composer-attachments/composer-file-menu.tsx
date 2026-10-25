import { copyText } from '../clipboard';
import { placedEntries } from '../files/carried-menu';
import { fileMenuEntries, type FileActionScope } from '../files/file-menu';
import { pickedEntry } from '../files/file-sources';
import { fileReference } from '../files/file-paths';
import { ToolIcon } from '../tool-icon';
import type { MenuEntry } from '../ui/ContextMenu';
import type { ComposerAttachment } from './attachment-files';

/**
 * What can be done with a
 * file in the composer before it is sent. A workspace file has the menu of a workspace path; text
 * added as a file is copied whole. Each can be taken out again. A file from outside the workspace opens too — Main knows
 * it as one the user picked.
 */
export function composerFileEntries(
  scope: FileActionScope | undefined,
  file: Exclude<ComposerAttachment, { kind: 'image' }>,
  remove: () => void,
): MenuEntry[] {
  const copy = async (text: string, failed: string) => {
    const refused = await copyText(text, failed);
    if (refused) scope?.notify(refused);
  };
  const removing: MenuEntry = {
    label: '移除',
    icon: <ToolIcon name="trash" size={14} />,
    run: remove,
  };
  const copied = <ToolIcon name="copy" size={14} />;
  if (file.kind === 'text') {
    const content: MenuEntry = {
      label: '复制内容',
      icon: copied,
      run: () => void copy(file.text, '没能复制内容，请重试。'),
    };
    return scope && file.path
      ? [
          ...placedEntries(scope, pickedEntry(file.path), { picked_path: file.path }, file.path),
          content,
          'separator',
          removing,
        ]
      : [content, 'separator', removing];
  }
  const inside = scope ? fileReference(file.path, scope.root, { file: true }) : undefined;
  if (scope && inside !== undefined && /^[a-z]:[\\/]/iu.test(file.path))
    return [...fileMenuEntries(scope, inside.path, 'conversation'), 'separator', removing];
  if (scope)
    return [
      ...placedEntries(scope, pickedEntry(file.path), { picked_path: file.path }, file.path),
      'separator',
      removing,
    ];
  return [
    { label: '复制路径', icon: copied, run: () => void copy(file.path, '没能复制路径，请重试。') },
    'separator',
    removing,
  ];
}
