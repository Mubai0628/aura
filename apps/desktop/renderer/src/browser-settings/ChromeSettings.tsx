import { useCallback, useEffect, useId, useState } from 'react';
import type {
  ChromeConnectionState,
  ChromeControlStatus,
  ChromeExtensionFolder,
  ChromePairingCode,
} from '@aura/contracts/generated';

import type { ResultEnvelope } from '../../../shared/aura-api';
import { ConfirmationDialog } from '../ConfirmationDialog';
import { userMessage } from '../agent-session/user-message';
import { remainingText } from '../browser/assist-time';
import { chromeControl } from '../chrome/bridge';
import { ToggleSwitch } from '../ui/ToggleSwitch';
import { usePolling } from '../ui/usePolling';
import { useReadOrder } from '../ui/useReadOrder';

/** How often the page reads Chrome control's state while it is open. */
export const STATUS_POLL_MS = 2_000;

/** Shown when the state cannot be read; the switch waits for it. */
const UNREADABLE = '读不到 Chrome 控制的状态，开关暂不可用。';

type Pending = 'enable' | 'pair' | 'forget' | 'folder';

const CONNECTION: Record<ChromeConnectionState, string> = {
  off: '已关闭',
  waiting: '等待扩展连接',
  pairing: '扩展正在配对',
  connected: '已连接',
};

/**
 * 设置 → 网络与浏览器 → Chrome: the switch, the connection, pairing with Aura's extension and
 * forgetting it, and the folder the extension is loaded from.
 */
export function ChromeSettings({ coreReady }: { coreReady: boolean }) {
  const [status, setStatus] = useState<ChromeControlStatus>();
  const [code, setCode] = useState<ChromePairingCode>();
  const [folder, setFolder] = useState<ChromeExtensionFolder>();
  const [error, setError] = useState<string>();
  const [readFailure, setReadFailure] = useState<string>();
  const [pending, setPending] = useState<Pending>();
  const busy = pending !== undefined;
  const [confirming, setConfirming] = useState<'off' | 'forget'>();
  const [now, setNow] = useState(Date.now);
  const offTitle = useId();
  const forgetTitle = useId();

  const reads = useReadOrder();
  const read = useCallback(
    async (live: () => boolean = () => true) => {
      const asked = reads.begin();
      try {
        const result = await chromeControl()?.status();
        if (!live() || result === undefined || !reads.accept(asked)) return;
        if (result.ok) {
          setStatus(result.value);
          setReadFailure(undefined);
        } else {
          setReadFailure(userMessage(result.error, UNREADABLE));
        }
      } catch {
        if (live() && reads.accept(asked)) setReadFailure(UNREADABLE);
      }
    },
    [reads],
  );
  usePolling(coreReady && chromeControl() !== undefined ? read : undefined, STATUS_POLL_MS);
  useEffect(() => {
    if (code === undefined) return;
    const timer = setInterval(() => setNow(Date.now()), 1_000);
    return () => clearInterval(timer);
  }, [code]);
  // A code is shown until it pairs or runs out.
  const expired = code !== undefined && Date.parse(code.expires_at) <= now;
  const shownCode = expired || status?.paired ? undefined : code;

  const act = async <T,>(
    kind: Pending,
    work: () => Promise<ResultEnvelope<T>> | undefined,
    fallback: string,
  ): Promise<T | undefined> => {
    setPending(kind);
    try {
      const result = await work();
      if (result === undefined) return undefined;
      if (!result.ok) {
        setError(userMessage(result.error, fallback));
        return undefined;
      }
      setError(undefined);
      // What the action returned is newer than any read still out.
      reads.settle();
      return result.value;
    } catch {
      setError(fallback);
      return undefined;
    } finally {
      setPending(undefined);
    }
  };
  const setEnabled = async (enabled: boolean) => {
    if (status?.revision == null) return;
    const revision = status.revision;
    const next = await act(
      'enable',
      () => chromeControl()?.setEnabled({ expected_revision: revision, enabled }),
      'Chrome 设置没有保存，请重试。',
    );
    if (next !== undefined) setStatus(next);
    if (!enabled) setCode(undefined);
  };
  const pair = async () => {
    const next = await act(
      'pair',
      () => chromeControl()?.beginPairing(),
      '没有拿到配对码，请重试。',
    );
    if (next !== undefined) {
      setNow(Date.now());
      setCode(next);
    }
  };
  const forget = async () => {
    const next = await act('forget', () => chromeControl()?.revoke(), '没有断开扩展，请重试。');
    if (next !== undefined) setStatus(next);
  };
  const openFolder = async () => {
    const next = await act(
      'folder',
      () => chromeControl()?.openExtensionFolder(),
      '没有打开扩展文件夹，请重试。',
    );
    if (next !== undefined) setFolder(next);
  };

  const ready = status?.revision != null && !busy;
  const enabled = status?.enabled ?? false;
  return (
    <section aria-label="Chrome" className="settings-surface browser-settings-chrome">
      <div className="browser-settings-row-inline">
        <label className="browser-settings-toggle">
          <ToggleSwitch
            checked={enabled}
            disabled={!ready}
            label="Chrome 控制"
            onCheckedChange={(checked) => (checked ? void setEnabled(true) : setConfirming('off'))}
          />
          <span>Chrome 控制</span>
        </label>
        {status ? (
          <span className={`chrome-connection is-${status.connection}`} role="status">
            {CONNECTION[status.connection]}
          </span>
        ) : coreReady && readFailure === undefined ? (
          <span className="chrome-connection" role="status">
            读取中…
          </span>
        ) : null}
      </div>
      {readFailure === undefined ? null : (
        <p className="settings-status-line" role="alert">
          {readFailure}
          <button className="button is-small" onClick={() => void read()} type="button">
            重试
          </button>
        </p>
      )}
      {enabled && status && !status.registered ? (
        <p className="settings-status-line" role="alert">
          Chrome 还找不到 Aura：请重新打开 Chrome 控制。
        </p>
      ) : null}
      {enabled && status?.paired ? (
        <div className="browser-settings-row-inline">
          <span>
            已配对的扩展 <code>{status.paired.extension_id.slice(0, 8)}…</code>
          </span>
          <button
            className="button is-small is-danger"
            disabled={!ready}
            onClick={() => setConfirming('forget')}
            type="button"
          >
            {pending === 'forget' ? '正在断开…' : '断开并忘记'}
          </button>
        </div>
      ) : null}
      {enabled && !status?.paired ? (
        <div className="browser-settings-row-inline">
          {shownCode ? (
            <>
              <span className="chrome-pairing-code" aria-label="配对码">
                {shownCode.code.slice(0, 4)} {shownCode.code.slice(4)}
              </span>
              <span className="chrome-pairing-time" role="timer">
                剩余 {remainingText(Date.parse(shownCode.expires_at) - now)}
              </span>
            </>
          ) : (
            <button
              className="button is-small"
              disabled={!ready}
              onClick={() => void pair()}
              type="button"
            >
              {pending === 'pair' ? '正在获取配对码…' : '配对'}
            </button>
          )}
        </div>
      ) : null}
      <div className="browser-settings-row-inline">
        <button
          className="button is-small"
          disabled={busy}
          onClick={() => void openFolder()}
          type="button"
        >
          {pending === 'folder' ? '正在打开…' : '打开扩展文件夹'}
        </button>
        {folder ? (
          <span className="chrome-folder">
            {folder.opened ? null : '没有找到扩展文件夹：'}
            <code>{folder.path}</code>
          </span>
        ) : null}
      </div>
      {error !== undefined || status?.issue ? (
        <p className="settings-status-line" role="alert">
          {error ?? status?.issue}
        </p>
      ) : null}
      {confirming === 'off' ? (
        <ConfirmationDialog
          busy={busy}
          confirmLabel="关闭"
          description="已配对的扩展会被忘记，共享给 Aura 的标签页都会停止。"
          onCancel={() => setConfirming(undefined)}
          onConfirm={() => {
            setConfirming(undefined);
            void setEnabled(false);
          }}
          title="关闭 Chrome 控制？"
          titleId={offTitle}
        />
      ) : null}
      {confirming === 'forget' ? (
        <ConfirmationDialog
          busy={busy}
          confirmLabel="断开"
          description="之后要在扩展里重新输入配对码才能再用。"
          onCancel={() => setConfirming(undefined)}
          onConfirm={() => {
            setConfirming(undefined);
            void forget();
          }}
          title="断开并忘记扩展？"
          titleId={forgetTitle}
        />
      ) : null}
    </section>
  );
}
