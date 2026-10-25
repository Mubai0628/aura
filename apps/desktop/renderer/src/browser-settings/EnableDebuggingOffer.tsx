import { useId, useState } from 'react';
import { ConfirmationDialog } from '../ConfirmationDialog';
import { userMessage } from '../agent-session/user-message';
import { browserApi } from '../browser/browser-store';
import { DEBUGGING_RISK } from './BrowserSettings';

/**
 * Under the status line of a turn with a call the switch stopped, the user
 * can turn it on — after the same confirmation as in 设置 → 网络与浏览器. The model cannot.
 */
export function EnableDebuggingOffer() {
  const titleId = useId();
  const [confirming, setConfirming] = useState(false);
  const [state, setState] = useState<{ busy: boolean; done?: boolean; error?: string }>({
    busy: false,
  });
  const enable = async () => {
    const api = browserApi();
    if (api === undefined) return;
    setState({ busy: true });
    const current = await api.getSettings();
    if (!current.ok || current.value.revision == null) {
      setState({ busy: false, error: '浏览器设置读取失败，请在设置中打开。' });
      return;
    }
    const saved = await api.updateSettings({
      expected_revision: current.value.revision,
      settings: { ...current.value.settings, debugging: true },
    });
    setState(
      saved.ok
        ? { busy: false, done: true }
        : { busy: false, error: userMessage(saved.error, '浏览器设置没有保存，请重试。') },
    );
  };
  if (state.done)
    return (
      <p className="tool-offer-status" role="status">
        已允许模型调试网页
      </p>
    );
  return (
    <div className="tool-offer">
      <button
        className="secondary-button"
        disabled={state.busy}
        onClick={() => setConfirming(true)}
        type="button"
      >
        允许模型调试网页
      </button>
      {state.error ? <p role="alert">{state.error}</p> : null}
      {confirming ? (
        <ConfirmationDialog
          busy={state.busy}
          confirmLabel="允许"
          description={DEBUGGING_RISK}
          onCancel={() => setConfirming(false)}
          onConfirm={() => {
            setConfirming(false);
            void enable();
          }}
          title="允许模型调试网页？"
          titleId={titleId}
        />
      ) : null}
    </div>
  );
}
