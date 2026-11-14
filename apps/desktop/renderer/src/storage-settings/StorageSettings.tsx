import { useCallback, useId, useState } from 'react';
import type {
  StorageCheck,
  StorageMovedPart,
  StorageOverview,
  StoragePart,
} from '@aura/contracts/generated';

import type { ResultEnvelope } from '../../../shared/aura-api';
import { ConfirmationDialog } from '../ConfirmationDialog';
import { userMessage } from '../agent-session/user-message';
import { usePolling } from '../ui/usePolling';
import { useReadOrder } from '../ui/useReadOrder';
import { storage } from './bridge';
import { NOT_CLOSED, moveFailed, storageFailure } from './storage-words';
import { OldData } from './OldData';
import { WorktreeLocationRow } from './WorktreeLocationRow';
import { size } from './size';
import './storage-settings.css';

/** How often the page reads again while the last move's old copy is being removed. */
const OLD_COPY_POLL_MS = 5_000;

const UNREADABLE = '读不到数据的位置，请重试。';

type Pending = 'choose' | 'move' | 'withdraw' | 'dismiss' | 'open' | 'install';

const PART_NAMES: Record<StorageMovedPart, string> = {
  data: '数据',
  core_log: 'Core 日志',
  window_profile: '界面配置',
  global_memory: '全局记忆',
};

/**
 * 设置 → 系统 → 存储: the data's folder and the installation's with their sizes; moving the data to
 * a folder the user picks or back to the default places, which happens at the next start; the
 * move waiting for it, and why the last one did not happen.
 */
export function StorageSettings({ coreReady }: { coreReady: boolean }) {
  const [overview, setOverview] = useState<StorageOverview>();
  const [readFailure, setReadFailure] = useState<string>();
  const [error, setError] = useState<string>();
  const [refused, setRefused] = useState<StorageCheck>();
  const [confirming, setConfirming] = useState<{ folder: string | null; check: StorageCheck }>();
  const [movingInstall, setMovingInstall] = useState(false);
  const [pending, setPending] = useState<Pending>();
  const busy = pending !== undefined;
  const titleId = useId();

  const reads = useReadOrder();
  const read = useCallback(
    async (live: () => boolean = () => true) => {
      const asked = reads.begin();
      try {
        const result = await storage()?.overview();
        if (!live() || result === undefined || !reads.accept(asked)) return;
        if (result.ok) {
          setOverview(result.value);
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
  // Read once, and again while the old copy is still being removed.
  const polled = coreReady && storage() !== undefined;
  usePolling(polled ? read : undefined, overview?.old_copies ? OLD_COPY_POLL_MS : 3_600_000);

  const target = (destination: string | null | undefined) => destination ?? '默认位置';
  const act = async <T,>(
    kind: Pending,
    work: () => Promise<ResultEnvelope<T>> | undefined,
    failed: (error: unknown) => string,
  ): Promise<T | undefined> => {
    setPending(kind);
    try {
      const result = await work();
      if (result === undefined) return undefined;
      if (!result.ok) {
        setError(failed(result.error));
        // What is in force is read again, so the page shows it next to the failure.
        void read();
        return undefined;
      }
      setError(undefined);
      reads.settle();
      return result.value;
    } catch (reason) {
      setError(failed(reason));
      return undefined;
    } finally {
      setPending(undefined);
    }
  };

  /** Checks where the data would go; a refused folder says why under the data's row. */
  const ask = async (folder: string | null) => {
    setRefused(undefined);
    const check = await act(
      'choose',
      () => storage()?.check({ folder }),
      storageFailure('没有检查这个位置', '数据还在原处；请重试。'),
    );
    if (check === undefined) return;
    if (check.refusals.length > 0) setRefused(check);
    else setConfirming({ folder, check });
  };
  const choose = async () => {
    setRefused(undefined);
    const picked = await act(
      'choose',
      () => storage()?.selectFolder(),
      storageFailure('没有打开选择框', '数据还在原处；请重试。'),
    );
    if (picked?.path) await ask(picked.path);
  };
  const move = async () => {
    if (confirming === undefined) return;
    const next = await act(
      'move',
      () => storage()?.requestMove({ folder: confirming.folder }),
      storageFailure(
        '没有记下要搬的位置',
        '数据还在原处，下次启动不会搬；请重试。',
        '可能已经记下要搬的位置',
      ),
    );
    if (next === undefined) return;
    setOverview(next);
    setConfirming(undefined);
    if (next.install)
      await act(
        'move',
        () => storage()?.restart(),
        () => NOT_CLOSED,
      );
  };
  const withdraw = async () => {
    const next = await act(
      'withdraw',
      () => storage()?.withdrawMove(),
      storageFailure(
        '没有取消',
        `下次启动时仍会搬到${target(overview?.requested?.destination)}；请重试。`,
        '可能已经取消',
      ),
    );
    if (next !== undefined) setOverview(next);
  };
  const dismiss = async () => {
    const next = await act(
      'dismiss',
      () => storage()?.dismissFailure(),
      storageFailure('没有清掉这条记录', '请重试。'),
    );
    if (next !== undefined) setOverview(next);
  };
  /** The installed setup program moves the installation; Aura closes for it. */
  const moveInstall = async () => {
    const opened = await act(
      'install',
      () => storage()?.moveInstall(),
      storageFailure('没有打开安装程序', '安装位置没有移动；请重试。'),
    );
    if (opened === undefined) return;
    setMovingInstall(false);
    if (!opened.restarting) setError('没有找到安装程序，请重新安装 Aura 后再试。');
  };
  const open = async (part: StoragePart) => {
    const opened = await act(
      'open',
      () => storage()?.openFolder({ part }),
      storageFailure('没有打开文件夹', '请重试。'),
    );
    if (opened && !opened.completed) setError(opened.message);
  };

  const installed = overview?.install != null;
  return (
    <div className="settings-page storage-settings">
      <div className="settings-content-heading">
        <h2>存储</h2>
      </div>
      {readFailure === undefined ? null : (
        <p className="settings-status-line" role="alert">
          {readFailure}
          <button className="button is-small" onClick={() => void read()} type="button">
            重试
          </button>
        </p>
      )}
      {overview?.failure ? (
        <p className="settings-status-line" role="alert">
          {moveFailed(target(overview.failure.destination), overview.failure.reason)}
          <button
            className="button is-small"
            disabled={busy}
            onClick={() => void dismiss()}
            type="button"
          >
            知道了
          </button>
        </p>
      ) : null}
      {overview?.requested ? (
        <p className="settings-status-line" role="status">
          下次启动时搬到{target(overview.requested.destination)}
          <button
            className="button is-small"
            disabled={busy}
            onClick={() => void withdraw()}
            type="button"
          >
            取消
          </button>
        </p>
      ) : null}
      {overview ? (
        <>
          <section aria-label="数据" className="settings-card storage-place">
            <div className="settings-card-heading">
              <h3>数据</h3>
              <span className="settings-tag">{size(overview.data.bytes)}</span>
            </div>
            <p className="storage-path">{overview.data.path}</p>
            {overview.old_copies ? (
              <p className="settings-status-line" role="status">
                旧位置的副本正在删除
              </p>
            ) : null}
            <div className="settings-card-actions">
              <button
                className="button is-small"
                disabled={busy || overview.requested != null}
                onClick={() => void choose()}
                type="button"
              >
                移动到…
              </button>
              {overview.data_moved ? (
                <button
                  className="button is-small"
                  disabled={busy || overview.requested != null}
                  onClick={() => void ask(null)}
                  type="button"
                >
                  移回默认位置
                </button>
              ) : null}
              <button
                className="button is-small"
                disabled={busy}
                onClick={() => void open('data')}
                type="button"
              >
                打开文件夹
              </button>
            </div>
            {refused ? (
              <div className="settings-inline-alert storage-refusals" role="alert">
                <p>不能搬到{target(refused.destination)}：</p>
                <ul>
                  {refused.refusals.map((refusal) => (
                    <li key={refusal}>{refusal}</li>
                  ))}
                </ul>
              </div>
            ) : null}
          </section>
          {overview.install ? (
            <section aria-label="安装" className="settings-card storage-place">
              <div className="settings-card-heading">
                <h3>安装</h3>
                <span className="settings-tag">{size(overview.install.bytes)}</span>
              </div>
              <p className="storage-path">{overview.install.path}</p>
              <div className="settings-card-actions">
                <button
                  className="button is-small"
                  disabled={busy || overview.requested != null}
                  onClick={() => setMovingInstall(true)}
                  type="button"
                >
                  移动到…
                </button>
                <button
                  className="button is-small"
                  disabled={busy}
                  onClick={() => void open('install')}
                  type="button"
                >
                  打开文件夹
                </button>
              </div>
            </section>
          ) : null}
          <WorktreeLocationRow />
          <OldData coreReady={coreReady} />
        </>
      ) : !coreReady ? (
        <p className="settings-status-line" role="status">
          Core 尚未就绪，存储设置暂不可用。
        </p>
      ) : readFailure === undefined ? (
        <p className="settings-status-line" role="status">
          读取中…
        </p>
      ) : null}
      {error === undefined ? null : (
        <p className="settings-status-line" role="alert">
          {error}
        </p>
      )}
      {confirming ? (
        <ConfirmationDialog
          busy={pending === 'move'}
          confirmLabel={installed ? '关闭并搬' : '下次启动时搬'}
          details={
            <ul className="storage-move-parts">
              {confirming.check.parts.map((part) => (
                <li key={part.part}>
                  <span>{PART_NAMES[part.part]}</span>
                  <span>{size(part.bytes)}</span>
                </li>
              ))}
            </ul>
          }
          description={[
            `要复制 ${size(confirming.check.bytes)}，这个磁盘还有 ${size(confirming.check.free)}。`,
            installed ? 'Aura 会关闭，搬完后从新位置打开。' : '',
          ].join('')}
          onCancel={() => setConfirming(undefined)}
          onConfirm={() => void move()}
          title={`搬到${target(confirming.check.destination)}？`}
          titleId={titleId}
        />
      ) : null}
      {movingInstall ? (
        <ConfirmationDialog
          busy={pending === 'install'}
          confirmLabel="关闭并打开"
          description="Aura 会关闭，在安装程序里选好新位置后移动，完成后从新位置打开。"
          onCancel={() => setMovingInstall(false)}
          onConfirm={() => void moveInstall()}
          title="打开安装程序移动 Aura？"
          titleId={titleId}
        />
      ) : null}
    </div>
  );
}
