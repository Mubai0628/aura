import type { AgentMessageAttachment } from '@aura/contracts/generated';
import { runsCode } from '../../../shared/outside-files';
import { userMessage } from '../agent-session/user-message';
import { copyText } from '../clipboard';
import { toolTabs } from '../right-workspace/tool-tabs';
import { ToolIcon } from '../tool-icon';
import type { MenuEntry } from '../ui/ContextMenu';
import { fileMenuEntries, openFile, type FileActionScope } from './file-menu';
import type { AttachmentFileTarget } from '../../../shared/attachment-files';
import { sidebarShows } from './file-paths';
import { attachmentEntry, fileSource, outsideTarget, recordEntry } from './file-sources';

/**
 * What can be done with a file a message carried — the
 * user's message now, a model's message later. A workspace file keeps the menu of a workspace
 * path; a file outside the workspace opens in the sidebar, outside Aura, or in the file manager;
 * a text file or a zip's listing kept with the message opens in the sidebar or is copied.
 */

/** The 文件 tab entry that shows what a message carried; none for a picture. */
export function carriedEntry(attachment: AgentMessageAttachment): string | undefined {
  switch (attachment.kind) {
    case 'image':
      return undefined;
    case 'text':
      return recordEntry(attachment.artifact_id, attachment.name);
    case 'file':
      return attachment.path;
    case 'external_file':
      return attachment.listing_artifact_id
        ? recordEntry(attachment.listing_artifact_id, attachment.name)
        : attachmentEntry(attachment.external_grant_id, attachment.name);
  }
}

/** Opens what a message carried where it is shown: the sidebar, or outside Aura. */
export function openCarried(scope: FileActionScope, attachment: AgentMessageAttachment): void {
  const entry = carriedEntry(attachment);
  if (entry === undefined) return;
  if (attachment.kind === 'file') openFile(scope, entry);
  else toolTabs.revealFile(scope.session, entry);
}

const icon = (
  name: 'panel-right-open' | 'copy' | 'external-link' | 'folder-open' | 'file-text',
) => <ToolIcon name={name} size={14} />;

/** Opens a file outside the workspace's own files outside Aura, through Main. */
export async function openOutside(
  scope: Pick<FileActionScope, 'notify'>,
  target: AttachmentFileTarget,
  action: 'default_application' | 'file_manager',
): Promise<void> {
  const result = await window.aura.v1.attachmentFile({ ...target, action });
  if (!result.ok) scope.notify(result.message);
}

async function copyKept(scope: FileActionScope, artifact: string): Promise<void> {
  const read = await window.aura.v1.readAttachmentText({
    session_id: scope.session,
    artifact_id: artifact,
  });
  if (!read.ok) {
    scope.notify(userMessage(read.error, '这个附件已不可用。'));
    return;
  }
  const refused = await copyText(read.value.text, '没能复制内容，请重试。');
  if (refused) scope.notify(refused);
}

async function copyPath(scope: FileActionScope, path: string): Promise<void> {
  const refused = await copyText(path, '没能复制路径，请重试。');
  if (refused) scope.notify(refused);
}

/** Opening outside Aura, and showing in the file manager, for a file seen through Main. */
export function outsideEntries(
  scope: Pick<FileActionScope, 'notify'>,
  target: AttachmentFileTarget,
  name: string,
): MenuEntry[] {
  const program = runsCode(name);
  return [
    {
      label: '用默认应用打开',
      icon: icon('external-link'),
      disabled: program,
      ...(program ? { note: 'Aura 不直接运行可执行文件或脚本' } : {}),
      run: () => void openOutside(scope, target, 'default_application'),
    },
    {
      label: '在文件资源管理器中显示',
      icon: icon('folder-open'),
      run: () => void openOutside(scope, target, 'file_manager'),
    },
  ];
}

/**
 * A file outside the workspace named by its full path — in a message, or picked for one —
 * in the sidebar when it shows that kind of file, outside Aura, or in the file manager; its path
 * is copied.
 */
export function placedEntries(
  scope: FileActionScope,
  entry: string,
  target: AttachmentFileTarget,
  path: string,
): MenuEntry[] {
  return [
    ...(sidebarShows(path)
      ? [
          {
            label: '在侧边栏打开',
            icon: icon('panel-right-open'),
            run: () => toolTabs.revealFile(scope.session, entry),
          },
        ]
      : []),
    ...outsideEntries(scope, target, path),
    'separator',
    { label: '复制路径', icon: icon('copy'), run: () => void copyPath(scope, path) },
  ];
}

/** Opens such a file as a click on it should: in the sidebar, or outside Aura. */
export function openPlaced(
  scope: FileActionScope,
  entry: string,
  target: AttachmentFileTarget,
  path: string,
): void {
  if (sidebarShows(path)) toolTabs.revealFile(scope.session, entry);
  else void openOutside(scope, target, runsCode(path) ? 'file_manager' : 'default_application');
}

/** The menu of a file card in a message. */
export function carriedMenuEntries(
  scope: FileActionScope,
  attachment: AgentMessageAttachment,
): MenuEntry[] {
  if (attachment.kind === 'file') return fileMenuEntries(scope, attachment.path, 'conversation');
  const entry = carriedEntry(attachment);
  const entries: MenuEntry[] = [];
  if (entry !== undefined)
    entries.push({
      label: '在侧边栏打开',
      icon: icon('panel-right-open'),
      run: () => toolTabs.revealFile(scope.session, entry),
    });
  if (attachment.kind === 'external_file') {
    entries.push(
      ...outsideEntries(
        scope,
        { session_id: scope.session, external_grant_id: attachment.external_grant_id },
        attachment.name,
      ),
    );
    entries.push('separator', {
      label: '复制路径',
      icon: icon('copy'),
      run: () => void copyPath(scope, attachment.path),
    });
  } else if (attachment.kind === 'text') {
    entries.push('separator', {
      label: '复制内容',
      icon: icon('copy'),
      run: () => void copyKept(scope, attachment.artifact_id),
    });
  }
  return entries;
}

/** The menu of a 文件 tab's file that a message carried. */
export function carriedTabEntries(scope: FileActionScope, entry: string): MenuEntry[] {
  const source = fileSource(entry);
  const target = outsideTarget(source, scope.session);
  if (target && (source.kind === 'mentioned' || source.kind === 'picked'))
    return [
      ...outsideEntries(scope, target, source.name),
      'separator',
      { label: '复制路径', icon: icon('copy'), run: () => void copyPath(scope, source.path) },
    ];
  if (target && source.kind !== 'workspace' && source.kind !== 'record')
    return outsideEntries(scope, target, source.name);
  if (source.kind === 'record')
    return [
      {
        label: '复制内容',
        icon: icon('copy'),
        run: () => void copyKept(scope, source.artifact),
      },
    ];
  return [];
}

/** The latest file a message of the conversation carried under each name, as its tab entry. */
export function carriedByName(
  messages: readonly ({ attachments?: AgentMessageAttachment[] | null } | undefined)[],
): Map<string, string> {
  const named = new Map<string, string>();
  for (const message of messages)
    for (const attachment of message?.attachments ?? []) {
      if (attachment.kind !== 'external_file' && attachment.kind !== 'text') continue;
      const entry = carriedEntry(attachment);
      if (entry !== undefined) named.set(attachment.name, entry);
    }
  return named;
}
