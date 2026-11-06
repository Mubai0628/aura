import { AppIcon } from '../app-icons';
import type { FileActionScope } from '../files/file-menu';
import { ToolIcon } from '../tool-icon';
import type { MenuEntry } from '../ui/ContextMenu';

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

/**
 * The right-click menu of a picture the conversation keeps: one sent with a message or one a
 * call took. Copying and saving need the conversation, which says what went wrong.
 */
export function pictureMenuEntries(
  scope: FileActionScope | undefined,
  sessionId: string,
  artifactId: string,
  name: string,
  preview: () => void,
): MenuEntry[] {
  return [
    { label: '预览', icon: <AppIcon name="eye" size={14} />, run: preview },
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
  ];
}
