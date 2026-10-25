import { useCallback, useRef, useState } from 'react';
import { userMessage } from './agent-session/user-message';
import type { NotificationInput } from './notification-center-store';

/** What adding a folder came to, as the app's notice says it. */
function addedNotice(disposition: string, name: string, workspace: string): NotificationInput {
  if (disposition === 'existing')
    return {
      dedupeKey: `workspace-add:${workspace}`,
      title: '已定位到这个工作区',
      message: '这个文件夹已在工作区中，已为你定位。',
      tone: 'info',
    };
  if (disposition === 'restored_archived')
    return {
      dedupeKey: `workspace-add:${workspace}`,
      title: '工作区已恢复',
      message: `已从归档中恢复“${name}”。`,
      tone: 'success',
    };
  return {
    dedupeKey: `workspace-add:${workspace}`,
    title: '工作区已添加',
    message: `“${name}”已加入工作区列表。`,
    tone: 'success',
  };
}

/**
 * Adding a workspace: Main's folder dialog, then Core registers the folder. Shared by the sidebar's
 * `+` and the empty state's button. With `notify` the outcome is the app's own notice
 *; without it the caller
 * shows `notice` in place.
 */
export function useAddWorkspace(
  onCreated: (workspace: string) => void,
  notify?: (notice: NotificationInput) => void,
) {
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string>();
  const [failed, setFailed] = useState(false);
  const pending = useRef(false);
  const add = useCallback(async () => {
    if (pending.current) return;
    pending.current = true;
    setBusy(true);
    setNotice(undefined);
    setFailed(false);
    try {
      const selected = await window.aura.v1.selectWorkspaceDirectory();
      if (!selected.ok)
        throw new Error(userMessage(selected.error.message, '未能打开文件夹选择器，请重试。'));
      if (selected.value.status === 'cancelled') return;
      const result = await window.aura.v1.createWorkspace({
        selected_root: selected.value.selected_root,
        display_name: selected.value.suggested_display_name,
      });
      if (!result.ok)
        throw new Error(userMessage(result.error.message, '添加未确认，请核对工作区列表。'));
      onCreated(result.value.workspace.workspace_id);
      const added = addedNotice(
        result.value.disposition,
        selected.value.suggested_display_name,
        result.value.workspace.workspace_id,
      );
      if (notify) notify(added);
      else
        setNotice(
          result.value.disposition === 'existing'
            ? added.message
            : result.value.disposition === 'restored_archived'
              ? '已恢复这个工作区。'
              : '工作区已添加。',
        );
    } catch (reason) {
      if (notify)
        notify({
          dedupeKey: 'workspace-add:failed',
          title: '没有添加工作区',
          message: userMessage(reason),
          tone: 'danger',
        });
      else {
        setFailed(true);
        setNotice(userMessage(reason));
      }
    } finally {
      pending.current = false;
      setBusy(false);
    }
  }, [notify, onCreated]);
  return { add, busy, notice, failed, dismiss: () => setNotice(undefined) };
}
