import { useState } from 'react';
import type { AppUpdatePhase } from '../../../shared/app-updates';
import { userMessage } from '../agent-session/user-message';
import { formatBytes } from '../files/file-reads';
import { ToggleSwitch } from '../ui/ToggleSwitch';
import { RollbackDialog } from './RollbackDialog';
import { RestartUpdateDialog } from './RestartUpdateDialog';
import { useAppUpdates } from './useAppUpdates';
import './about-updates.css';

const PHASE_LABELS: Record<AppUpdatePhase, string> = {
  idle: '尚未检查',
  checking: '正在检查更新',
  up_to_date: '已是最新版本',
  available: '发现新版本',
  downloading: '正在下载',
  paused: '下载已暂停',
  applying: '正在校验与安装',
  ready: '等待重启',
  error: '更新失败',
};

export function AboutUpdates() {
  const { state, error, pending, send } = useAppUpdates();
  const [confirmation, setConfirmation] = useState<{
    kind: 'restart' | 'rollback';
    version: string;
  }>();
  const waiting = state?.waiting;
  const previous = state?.previous;
  const supported = state?.available === true;
  const working =
    state !== undefined && ['checking', 'downloading', 'applying'].includes(state.phase);
  const blocked = !supported || working || pending !== undefined;
  const canPause =
    supported && state.phase === 'downloading' && (pending === undefined || pending === 'download');
  // Main words the network steps' failures itself; what is left is the setup program's.
  const failure =
    error ??
    (state?.error
      ? userMessage(state.error, '更新没有完成，请稍后重试；仍然失败时请重新运行安装程序修复。')
      : undefined);
  const status = !state
    ? '正在读取版本'
    : !supported
      ? '此版本不支持应用内更新'
      : state.phase === 'paused' && state.error
        ? '下载中断'
        : PHASE_LABELS[state.phase];
  const tone = failure
    ? 'danger'
    : state?.phase === 'up_to_date'
      ? 'success'
      : state?.phase === 'available' || state?.waiting
        ? 'active'
        : undefined;
  const notice = state?.notice;
  const progress = state?.progress;
  const confirmRestart = async () => {
    if (blocked || confirmation?.kind !== 'restart' || confirmation.version !== state?.waiting)
      return;
    if (await send({ command: 'restart' })) setConfirmation(undefined);
  };
  const confirmRollback = async (restoreData: boolean) => {
    if (blocked || confirmation?.kind !== 'rollback' || confirmation.version !== state?.previous)
      return false;
    const ok = await send({ command: 'roll_back', restore_data: restoreData });
    if (ok) setConfirmation(undefined);
    return ok;
  };

  return (
    <div className="settings-page about-updates">
      <div className="settings-content-heading">
        <h2>关于与更新</h2>
      </div>
      {notice ? (
        <section
          aria-label="版本恢复通知"
          className="settings-card update-notice"
          data-tone="danger"
          role="alert"
        >
          <h3>{notice.going_back ? '回退失败，已返回原版本' : '更新失败，已回到原版本'}</h3>
          <p>
            {userMessage(
              notice.detail,
              '版本未能正常启动，请检查安装状态；仍然失败时请重新运行安装程序修复。',
            )}
          </p>
          <dl className="update-facts">
            <div>
              <dt>未能启动</dt>
              <dd>{notice.version}</dd>
            </div>
            <div>
              <dt>已返回</dt>
              <dd>{notice.kept_version}</dd>
            </div>
          </dl>
          <div className="settings-card-actions">
            <button
              disabled={!supported || pending !== undefined}
              onClick={() => void send({ command: 'dismiss_notice' })}
              type="button"
            >
              知道了
            </button>
          </div>
        </section>
      ) : null}
      <section aria-label="Aura 版本" className="settings-card">
        <div className="settings-card-heading">
          <h3>Aura</h3>
          <span aria-live="polite" className="status-pill" data-tone={tone}>
            {status}
          </span>
        </div>
        <dl className="update-facts">
          <div>
            <dt>当前版本</dt>
            <dd>{state?.current || '—'}</dd>
          </div>
          {state?.previous ? (
            <div>
              <dt>上一版本</dt>
              <dd>{state.previous}</dd>
            </div>
          ) : null}
          {state?.latest ? (
            <div>
              <dt>最新版本</dt>
              <dd>{state.latest.version}</dd>
            </div>
          ) : null}
          {state?.waiting ? (
            <div>
              <dt>等待重启的版本</dt>
              <dd>{state.waiting}</dd>
            </div>
          ) : null}
          {state?.checked_at ? (
            <div>
              <dt>上次检查</dt>
              <dd>{new Date(state.checked_at).toLocaleString('zh-CN')}</dd>
            </div>
          ) : null}
        </dl>
        <div className="settings-card-actions">
          <button disabled={blocked} onClick={() => void send({ command: 'check' })} type="button">
            检查更新
          </button>
          {state?.latest &&
          state.waiting !== state.latest.version &&
          state.phase !== 'downloading' &&
          state.phase !== 'applying' ? (
            <button
              className="button is-primary"
              disabled={blocked}
              onClick={() => void send({ command: 'download' })}
              type="button"
            >
              {state.phase === 'paused' ? '继续下载' : '下载更新'}
            </button>
          ) : null}
          {state?.phase === 'downloading' ? (
            <button
              disabled={!canPause}
              onClick={() => void send({ command: 'pause' })}
              type="button"
            >
              {pending === 'pause' ? '正在暂停…' : '暂停下载'}
            </button>
          ) : null}
          {waiting ? (
            <button
              className="button is-primary"
              disabled={blocked}
              onClick={() => setConfirmation({ kind: 'restart', version: waiting })}
              type="button"
            >
              重启并更新
            </button>
          ) : null}
        </div>
        {progress && (state?.phase === 'downloading' || state?.phase === 'paused') ? (
          <div className="update-progress">
            <progress
              aria-label="更新下载进度"
              max={progress.total > 0 ? progress.total : undefined}
              value={progress.total > 0 ? Math.min(progress.done, progress.total) : undefined}
            />
            <span>
              {formatBytes(progress.done)}
              {progress.total > 0 ? ` / ${formatBytes(progress.total)}` : ''}
            </span>
          </div>
        ) : null}
        {failure ? (
          <p className="update-error" role="alert">
            {failure}
          </p>
        ) : null}
        {!state || error ? (
          <div className="settings-card-actions">
            <button
              disabled={pending !== undefined}
              onClick={() => void send({ command: 'state' })}
              type="button"
            >
              重新读取状态
            </button>
          </div>
        ) : null}
        {state?.latest?.notes ? (
          <details className="update-release-notes">
            <summary>更新内容</summary>
            <p>{state.latest.notes}</p>
          </details>
        ) : null}
      </section>
      <section aria-label="更新偏好" className="settings-card update-setting-row">
        <h3>自动检查更新</h3>
        <ToggleSwitch
          checked={state?.auto_check === true}
          disabled={blocked}
          label="自动检查更新"
          onCheckedChange={(enabled) => void send({ command: 'set_auto_check', enabled })}
        />
      </section>
      <section aria-label="版本回退" className="settings-card update-setting-row">
        <h3>版本回退</h3>
        <button
          disabled={blocked || !previous}
          onClick={() => {
            if (previous) setConfirmation({ kind: 'rollback', version: previous });
          }}
          type="button"
        >
          回到上一版本
        </button>
      </section>
      {confirmation?.kind === 'restart' ? (
        <RestartUpdateDialog
          busy={pending === 'restart'}
          error={failure}
          onCancel={() => setConfirmation(undefined)}
          onConfirm={() => void confirmRestart()}
          version={confirmation.version}
          valid={supported && !working && confirmation.version === waiting}
        />
      ) : null}
      {confirmation?.kind === 'rollback' ? (
        <RollbackDialog
          busy={pending === 'roll_back'}
          error={failure}
          onCancel={() => setConfirmation(undefined)}
          onConfirm={confirmRollback}
          target={confirmation.version}
          valid={supported && !working && confirmation.version === previous}
        />
      ) : null}
    </div>
  );
}
