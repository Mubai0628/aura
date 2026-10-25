import type { UserQuery } from '@aura/contracts/generated';
import { useState } from 'react';
import { userMessage } from '../user-message';

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
        setStatus(userMessage(result.error, '回答没有送达，请重试。'));
        return;
      }
      onSelected(result.value.file_reference_id);
      setStatus(`已选择：${result.value.relative_path}`);
    } catch {
      setStatus('无法确认文件，请检查连接。');
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
