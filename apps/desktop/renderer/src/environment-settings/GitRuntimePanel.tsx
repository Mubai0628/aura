import { userMessage } from '../agent-session/user-message';
import { useCallback, useEffect, useRef, useState } from 'react';
import type { GitRuntimeStatusV1 } from '@aura/contracts/generated';
export function GitRuntimePanel() {
  const [status, setStatus] = useState<GitRuntimeStatusV1>();
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(true);
  const sequence = useRef(0);
  const load = useCallback(async (request: number) => {
    try {
      const result = await window.aura.v1.git.runtime();
      if (request !== sequence.current) return;
      if (result.ok) setStatus(result.value);
      else setError(userMessage(result.error, 'Git 状态读取失败，请重试。'));
    } catch {
      if (request === sequence.current) setError('Git 检测未完成，请重试。');
    } finally {
      if (request === sequence.current) setBusy(false);
    }
  }, []);
  const reload = () => {
    setBusy(true);
    setError(undefined);
    void load(++sequence.current);
  };
  useEffect(() => {
    void load(++sequence.current);
    return () => {
      sequence.current += 1;
    };
  }, [load]);
  const install = async () => {
    const request = sequence.current;
    try {
      const result = await window.aura.v1.git.openInstallation();
      if (request === sequence.current && !result.ok)
        setError(userMessage(result.error, 'Git 重新检测没有完成，请重试。'));
    } catch {
      if (request === sequence.current)
        setError('无法打开 Git 官网，请访问 https://git-scm.com/download/win');
    }
  };
  return (
    <section aria-label="Git 运行环境" className="settings-surface">
      <header className="tool-section-heading">
        <div>
          <h3>本机 Git</h3>
        </div>
        <button type="button" disabled={busy} onClick={() => void reload()}>
          {busy ? '正在检测…' : '重新检测'}
        </button>
      </header>
      {busy && status === undefined ? <p role="status">正在检测 Git…</p> : null}
      {error === undefined ? null : <p role="alert">{error}</p>}
      {status === undefined ? null : (
        <>
          <p role="status">{status.message}</p>
          {status.version ? <p>{status.version}</p> : null}
          {status.executable ? (
            <p>
              <code>{status.executable}</code>
            </p>
          ) : null}
          {status.state === 'available' ? null : (
            <>
              <p>{status.next_step}</p>
              <button type="button" onClick={() => void install()}>
                前往 Git 官网安装或升级
              </button>
            </>
          )}
        </>
      )}
    </section>
  );
}
