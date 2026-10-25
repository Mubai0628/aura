import type { AgentPromptInspection } from '@aura/contracts/generated';
import { useCallback, useEffect, useState } from 'react';
import type { LatestInspection } from './useLatestInspection';
import { userMessage } from './user-message';

type Selection = { sessionId: string; ordinal: number } & (
  { mode: 'latest' } | { mode: 'step'; modelStepId: string }
);
type Record = {
  displayKey: string;
  requestKey: string;
  view: AgentPromptInspection | undefined;
  error: string | undefined;
};

/**
 * What
 * each conversation's details show. The conversation view is rebuilt when the user goes to another
 * conversation or to Settings and back, while its details tab stays open; the choice stays here.
 */
const chosen = new Map<string, Selection>();

/** Tests: a fresh start. */
export function resetPromptInspectionsForTests(): void {
  chosen.clear();
}

/**
 * Context details follow requests; a message's inspection keeps its explicit step. Nothing is read
 * while the details are not on screen (`shown`); shown again, they read what is current. Details
 * shown with nothing chosen follow the latest request.
 */
export function usePromptInspection(
  sessionId: string,
  runtimeInstanceId: string | undefined,
  latest: LatestInspection,
  shown: boolean,
  onOpen: () => void,
) {
  const [selection, setChoice] = useState<Selection | undefined>(() => chosen.get(sessionId));
  const setSelection = useCallback(
    (choose: (previous: Selection | undefined) => Selection) =>
      setChoice((previous) => {
        const next = choose(previous ?? chosen.get(sessionId));
        chosen.set(next.sessionId, next);
        return next;
      }),
    [sessionId],
  );
  const [record, setRecord] = useState<Record>();
  const kept = selection?.sessionId === sessionId ? selection : chosen.get(sessionId);
  const selected: Selection | undefined =
    kept ?? (shown ? { sessionId, ordinal: 0, mode: 'latest' } : undefined);
  const step = selected?.mode === 'latest' ? latest.modelStepId : selected?.modelStepId;
  const displayKey = selected
    ? JSON.stringify([
        sessionId,
        runtimeInstanceId,
        selected.mode,
        selected.mode === 'step' ? step : null,
      ])
    : undefined;
  // A projection poll or streamed answer alone does not change the request being inspected.
  const requestKey =
    selected && step
      ? JSON.stringify([
          displayKey,
          step,
          selected.ordinal,
          selected.mode === 'latest'
            ? [latest.contextSnapshotId, latest.usage, latest.compactionRunning]
            : null,
        ])
      : undefined;
  useEffect(() => {
    if (!shown || !displayKey || !requestKey || !step) return;
    let current = true;
    const keepFailure = (error: string) => {
      if (!current) return;
      setRecord((previous) => ({
        displayKey,
        requestKey,
        view: previous?.displayKey === displayKey ? previous.view : undefined,
        error,
      }));
    };
    void (async () => {
      try {
        const result = await window.aura.v1.readAgentInspector({
          session_id: sessionId,
          model_step_id: step,
        });
        if (!current) return;
        if (!result.ok) {
          keepFailure(userMessage(result.error, '无法读取这个步骤的快照。'));
        } else if (
          result.value.projection.session_id !== sessionId ||
          result.value.execution.model_step_id !== step
        ) {
          keepFailure('读取到的步骤与当前请求不一致，请重新打开详情。');
        } else {
          setRecord({ displayKey, requestKey, view: result.value, error: undefined });
        }
      } catch (error) {
        keepFailure(userMessage(error, '无法读取这个步骤的快照。'));
      }
    })();
    return () => {
      current = false;
    };
  }, [displayKey, requestKey, sessionId, step, shown]);
  const openLatest = useCallback(() => {
    setSelection((previous) => ({
      sessionId,
      mode: 'latest',
      ordinal: (previous?.ordinal ?? 0) + 1,
    }));
    onOpen();
  }, [sessionId, onOpen, setSelection]);
  return {
    inspection: record?.displayKey === displayKey ? record?.view : undefined,
    error: record?.requestKey === requestKey ? record?.error : undefined,
    openLatest,
  };
}
