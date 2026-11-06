import { AppIcon } from '../app-icons';
import type { Controlling } from './useComputerControlling';
import './computer-controlling.css';

/**
 * 近期活动's 正在操控: each program the model is controlling now, whether it has the mouse and
 * keyboard, and 停止; 全部停止 for two or more. A program the user took the desktop over from
 * says so, with 交还 to give the desktop back, until the conversation's next turn runs.
 * Nothing while there is none.
 */
export function ComputerControlling({ controlling }: { controlling: Controlling }) {
  const { apps, pip, pending, givingBack, failure, stop, giveBack } = controlling;
  const pipLabel =
    pip?.phase === 'waiting_for_login'
      ? '等待登录小窗'
      : pip?.phase === 'failed' || pip?.phase === 'disconnected'
        ? '小窗未连接'
        : undefined;
  if (apps.length === 0 && failure === undefined && !pipLabel && !pip?.notice) return null;
  const stoppable = apps.filter((app) => app.taken_over !== true);
  const busy = pending !== undefined || givingBack !== undefined;
  return (
    <section aria-label="正在操控" className="activity-section computer-controlling">
      <h4 className="activity-section-title">
        {apps.length === 0 && (pipLabel || pip?.notice) ? '小窗' : '正在操控'}
      </h4>
      {pipLabel ? <p role="status">{pipLabel}</p> : null}
      {pip?.notice ? <p role="alert">{pip.notice}</p> : null}
      <ul>
        {apps.map((app) => (
          <li key={`${app.mode ?? 'foreground'}:${app.app_id}`}>
            <AppIcon name="desktop" size={13} />
            <span className="computer-controlling-name" title={app.app_id}>
              {app.name}
            </span>
            <span className="computer-controlling-state" role="status">
              {app.taken_over === true
                ? app.mode === 'child_session'
                  ? '你已接管小窗'
                  : '你已接管'
                : app.mode === 'child_session'
                  ? '小窗'
                  : app.foreground
                    ? '前台'
                    : '后台'}
            </span>
            {app.taken_over === true ? (
              <button
                aria-label={`交还操控 ${app.name}`}
                className="activity-link"
                disabled={busy}
                onClick={() => void giveBack(app)}
                type="button"
              >
                {givingBack === app.app_id ? '正在交还…' : '交还'}
              </button>
            ) : (
              <button
                aria-label={`停止操控 ${app.name}`}
                className="activity-link"
                disabled={busy}
                onClick={() => void stop(app.app_id)}
                type="button"
              >
                {pending === app.app_id ? '正在停止…' : '停止'}
              </button>
            )}
          </li>
        ))}
      </ul>
      {stoppable.length > 1 ? (
        <button
          className="button is-small is-danger computer-controlling-all"
          disabled={busy}
          onClick={() => void stop()}
          type="button"
        >
          {pending === '*' ? '正在停止…' : '全部停止'}
        </button>
      ) : null}
      {failure === undefined ? null : <p role="alert">{failure}</p>}
    </section>
  );
}
