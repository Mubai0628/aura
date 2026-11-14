import { useRef, useState } from 'react';
import type { WorkspaceNavigationItem } from '../app-shell-types';
import type { NotificationInput } from '../notification-center-store';
import { pickWorkspaceFolder, relocateMessage, relocateWorkspace } from './relocate';

/** A folder that is not the Workspace's own folder object, waiting for the user's answer. */
export interface RelocationQuestion {
  from: string;
  to: string;
}

/**
 * Moving a Workspace to a folder the user picks: Main's folder dialog, then Core. The same folder
 * object at a new path switches at once; any other folder waits in `question` until the user says
 * it is the same project. The outcome is the app's notice; a refusal of the answered question is
 * shown in the question itself. One move at a time.
 */
export function useWorkspaceRelocation(
  workspace: WorkspaceNavigationItem,
  notify: (notice: NotificationInput) => void,
) {
  const [busy, setBusy] = useState(false);
  const [question, setQuestion] = useState<RelocationQuestion>();
  const [questionError, setQuestionError] = useState<string>();
  const pending = useRef(false);
  const dedupeKey = `workspace-relocate:${workspace.id}`;
  const moved = (folder: string) =>
    notify({
      dedupeKey,
      title: '工作区位置已更改',
      message: `“${workspace.name}”现在在 ${folder}。`,
      tone: 'success',
    });
  const run = async (step: () => Promise<void>) => {
    if (pending.current) return;
    pending.current = true;
    setBusy(true);
    try {
      await step();
    } finally {
      pending.current = false;
      setBusy(false);
    }
  };
  const start = () =>
    run(async () => {
      try {
        const folder = await pickWorkspaceFolder();
        if (folder === undefined) return;
        if ((await relocateWorkspace(workspace.id, folder, false)) === 'moved') moved(folder);
        else {
          setQuestionError(undefined);
          setQuestion({ from: workspace.root, to: folder });
        }
      } catch (reason) {
        notify({
          dedupeKey,
          title: '没有更改位置',
          message: relocateMessage(reason),
          tone: 'danger',
        });
      }
    });
  const confirm = () =>
    run(async () => {
      if (question === undefined) return;
      try {
        await relocateWorkspace(workspace.id, question.to, true);
        setQuestion(undefined);
        moved(question.to);
      } catch (reason) {
        setQuestionError(relocateMessage(reason));
      }
    });
  const cancel = () => {
    if (!pending.current) setQuestion(undefined);
  };
  return { busy, question, questionError, start, confirm, cancel };
}
