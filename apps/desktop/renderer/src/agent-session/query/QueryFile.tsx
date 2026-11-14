import type { UserQuery } from '@aura/contracts/generated';
import { useState } from 'react';
import { failedAction } from '../failure-text';

/** What a file that was not taken as the answer says; the answer has not gone. */
const NOT_TAKEN = '这个文件没有选上';
const NOT_TAKEN_NEXT = '回答还没有发出；请重新选择。';

/** A file or folder in the workspace as the answer to one question. */
export function QueryFile({
  sessionId,
  query,
  questionId,
  onSelected,
}: {
  sessionId: string;
  query: UserQuery;
  questionId: string;
  onSelected: (id: string | undefined) => void;
}) {
  const [path, setPath] = useState('');
  const [status, setStatus] = useState('');
  const [busy, setBusy] = useState(false);
  const select = async () => {
    setBusy(true);
    try {
      const result = await window.aura.v1.selectAgentQueryFile({
        session_id: sessionId,
        agent_run_id: query.agent_run_id,
        query_id: query.query_id,
        expected_revision: query.revision,
        question_id: questionId,
        relative_path: path,
      });
      if (!result.ok) {
        setStatus(
          `${result.error.code ?? ''} ${result.error.message}`.includes('agent.query_file_scope')
            ? '这个文件不在工作区里，不能用来回答；请选工作区里的文件或目录。'
            : failedAction(result.error, NOT_TAKEN, NOT_TAKEN_NEXT),
        );
        return;
      }
      onSelected(result.value.file_reference_id);
      setStatus(`已选择：${result.value.relative_path}`);
    } catch (reason) {
      setStatus(failedAction(reason, NOT_TAKEN, NOT_TAKEN_NEXT));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="agent-query-file">
      <label className="agent-query-field">
        <span>工作区内路径</span>
        <input
          value={path}
          disabled={busy}
          onChange={(event) => {
            setPath(event.target.value);
            onSelected(undefined);
            setStatus('');
          }}
          placeholder="相对工作区的文件或目录"
        />
      </label>
      <button
        className="button is-small"
        type="button"
        disabled={busy || !path.trim()}
        onClick={() => void select()}
      >
        确认文件或目录
      </button>
      <p className="agent-query-file-status" role="status">
        {status}
      </p>
    </div>
  );
}
