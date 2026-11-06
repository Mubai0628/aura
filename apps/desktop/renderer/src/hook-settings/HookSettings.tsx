import { useEffect, useState } from 'react';
import type {
  HookEntryView,
  HookMoveDirection,
  HookPreview,
  HookRegistryView,
} from '@aura/contracts/generated';
import type { ResultEnvelope } from '../../../shared/aura-api';
import { AppIcon } from '../app-icons';
import { HookDetails, HookHeading, type HookChange } from './HookDetails';
import { HookPreviewPanel } from './HookPreviewPanel';
import {
  folderName,
  hookError,
  hookEvents,
  hookName,
  hookReview,
  hookStateTone,
} from './hook-labels';
import { SettingsItemDialog, SettingsRow, SettingsRowOpen } from '../ui/SettingsItem';
import { ListSentinel, SkeletonRows } from '../ui/SkeletonRows';
import { useIncrementalList } from '../ui/useIncrementalList';
import './hook-settings.css';

const NONE: readonly never[] = [];

type Notice = { alert: boolean; text: string };

/** Settings → Hook: add, trust, order and inspect the user's lifecycle Hooks. */
export function HookSettings({ coreReady }: { coreReady: boolean }) {
  const [registry, setRegistry] = useState<HookRegistryView>();
  const [loadError, setLoadError] = useState<string>();
  const [reload, setReload] = useState(0);
  const [busy, setBusy] = useState<string>();
  const [notice, setNotice] = useState<Notice>();
  const [preview, setPreview] = useState<HookPreview>();
  const [removing, setRemoving] = useState<string>();
  const [history, setHistory] = useState<string>();
  const [opened, setOpened] = useState<string>();
  useEffect(() => {
    if (!coreReady) return;
    let live = true;
    void window.aura.v1.listHooks({}).then(
      (result) => {
        if (!live) return;
        if (result.ok) {
          setRegistry(result.value);
          setLoadError(undefined);
        } else setLoadError(hookError(result.error.message, 'Hook 列表读取失败，请重试。'));
      },
      (reason: unknown) => {
        if (live) setLoadError(hookError(reason, 'Hook 列表读取失败，请重试。'));
      },
    );
    return () => {
      live = false;
    };
  }, [coreReady, reload]);

  /** One user action at a time; a refusal reloads the list so the page shows the truth. */
  async function run(label: string, work: () => Promise<Notice | undefined>) {
    if (busy) return;
    setBusy(label);
    setNotice(undefined);
    try {
      const outcome = await work();
      if (outcome) setNotice(outcome);
      if (outcome?.alert) setReload((value) => value + 1);
    } catch (reason) {
      setNotice({ alert: true, text: hookError(reason, '操作结果未确认，请刷新后核对。') });
      setReload((value) => value + 1);
    } finally {
      setBusy(undefined);
    }
  }
  const applied = (result: ResultEnvelope<HookRegistryView>, done: string): Notice => {
    if (!result.ok) return { alert: true, text: hookError(result.error.message) };
    setRegistry(result.value);
    return { alert: false, text: done };
  };
  const revision = registry?.revision ?? 0;

  const open = (path: string) =>
    run(`preview:${path}`, async () => {
      const result = await window.aura.v1.previewHook({ path });
      if (!result.ok) return { alert: true, text: hookError(result.error.message) };
      setPreview(result.value);
      return undefined;
    });
  const add = () =>
    run('add', async () => {
      const selected = await window.aura.v1.selectHookDirectory({});
      if (!selected.ok) return { alert: true, text: hookError(selected.error.message) };
      if (!selected.value.path) return undefined;
      const result = await window.aura.v1.previewHook({ path: selected.value.path });
      if (!result.ok) return { alert: true, text: hookError(result.error.message) };
      setPreview(result.value);
      return undefined;
    });
  const trust = (shown: HookPreview) =>
    run(`trust:${shown.path}`, async () => {
      const listed = registry?.entries.find((entry) => entry.path === shown.path);
      const name = shown.definition?.name ?? folderName(shown.path);
      const result = await window.aura.v1.trustHook({
        path: shown.path,
        expected_fingerprint: shown.fingerprint ?? '',
        // Trusting again keeps a stopped Hook stopped; a new one starts enabled.
        enabled: listed ? listed.enabled : true,
        expected_revision: revision,
      });
      if (result.ok) setPreview(undefined);
      return applied(result, listed ? `已重新信任 ${name}` : `已信任并启用 ${name}`);
    });
  const change = (entry: HookEntryView, action: HookChange | HookMoveDirection) =>
    run(`${action}:${entry.path}`, async () => {
      const name = hookName(entry);
      const request = { path: entry.path, expected_revision: revision };
      const api = window.aura.v1;
      switch (action) {
        case 'enable':
        case 'disable':
          return applied(
            await api.setHookEnabled({ ...request, enabled: action === 'enable' }),
            action === 'enable' ? `已启用 ${name}` : `已停用 ${name}`,
          );
        case 'untrust':
          return applied(await api.untrustHook(request), `已撤销对 ${name} 的信任`);
        case 'remove': {
          setRemoving(undefined);
          const done = applied(await api.removeHook(request), `已从列表移除 ${name}，文件夹未删除`);
          // A folder added again later does not reopen this dialog.
          if (!done.alert) close();
          return done;
        }
        default:
          return applied(
            await api.moveHook({ ...request, direction: action }),
            `已调整 ${name} 的顺序`,
          );
      }
    });

  const entries = registry?.entries ?? NONE;
  const pages = useIncrementalList(entries);
  // A Hook that leaves the list, removed here or no longer found, takes its dialog with it.
  const shown = opened === undefined ? undefined : entries.find((entry) => entry.path === opened);
  const close = () => {
    // A folder being added keeps its review on the page.
    if (preview?.path === opened) setPreview(undefined);
    setOpened(undefined);
    setRemoving(undefined);
    setHistory(undefined);
  };
  // A dialog shows what is done in it, never the outcome of an earlier action on another item.
  const openItem = (path: string) => {
    setNotice(undefined);
    setOpened(path);
  };
  const row = (entry: HookEntryView, index: number) => {
    const name = hookName(entry);
    const locked = busy !== undefined;
    // A plugin's Hook is trusted with its plugin and runs after the user's own.
    const plugin = entry.plugin ?? undefined;
    const tone = hookStateTone(entry.state);
    const review = plugin ? undefined : hookReview(entry);
    return (
      <SettingsRow
        key={entry.path}
        state={entry.state}
        tone={tone === 'success' ? undefined : tone}
        index={index}
        headingClassName="hook-row-heading"
        heading={<HookHeading entry={entry} order={index + 1} />}
        summary={entry.definition?.description || hookEvents(entry)}
        alerts={
          entry.problems.length ? (
            <p className="settings-inline-alert" role="alert">
              {entry.problems.join('；')}
            </p>
          ) : null
        }
        actions={
          <>
            {review ? (
              <button
                type="button"
                className="button is-primary is-small"
                disabled={locked}
                onClick={() => {
                  openItem(entry.path);
                  void open(entry.path);
                }}
              >
                {review}
              </button>
            ) : null}
            {entry.state === 'ready' || entry.state === 'disabled' ? (
              <button
                type="button"
                className="button is-small"
                disabled={locked}
                onClick={() => void change(entry, entry.enabled ? 'disable' : 'enable')}
              >
                {entry.enabled ? '停用' : '启用'}
              </button>
            ) : null}
            {plugin ? null : (
              <>
                <button
                  type="button"
                  className="button is-ghost is-small"
                  aria-label={`上移 ${name}`}
                  disabled={locked || index === 0}
                  onClick={() => void change(entry, 'up')}
                >
                  上移
                </button>
                <button
                  type="button"
                  className="button is-ghost is-small"
                  aria-label={`下移 ${name}`}
                  disabled={
                    locked || index === entries.length - 1 || entries[index + 1]?.plugin != null
                  }
                  onClick={() => void change(entry, 'down')}
                >
                  下移
                </button>
              </>
            )}
            <SettingsRowOpen name={name} onOpen={() => openItem(entry.path)} />
          </>
        }
      />
    );
  };

  return (
    <section className="hook-settings settings-page" aria-label="Hook 设置">
      <header className="settings-content-heading">
        <div>
          <h2>Hook</h2>
        </div>
        <div className="settings-page-actions">
          <button
            type="button"
            className="button is-primary"
            disabled={!coreReady || busy !== undefined}
            onClick={() => void add()}
          >
            <AppIcon name="plus" size={15} />
            添加 Hook…
          </button>
          <button
            type="button"
            className="button is-ghost"
            aria-label="刷新"
            title="刷新"
            onClick={() => setReload((value) => value + 1)}
          >
            <AppIcon name="refresh" size={15} />
          </button>
        </div>
      </header>
      {notice ? (
        <p
          className={notice.alert ? 'settings-status-line' : 'settings-status-line is-done'}
          role={notice.alert ? 'alert' : 'status'}
        >
          {notice.text}
        </p>
      ) : null}
      {preview && !entries.some((entry) => entry.path === preview.path) ? (
        // A folder being added has no row yet, so its review keeps a cancel button.
        <HookPreviewPanel
          preview={preview}
          busy={busy !== undefined}
          onTrust={() => void trust(preview)}
          onCancel={() => setPreview(undefined)}
        />
      ) : null}
      {!coreReady ? (
        <p className="settings-status-line" role="status">
          Core 尚未就绪，Hook 管理暂不可用。
        </p>
      ) : loadError ? (
        <p className="settings-status-line" role="alert">
          {loadError}{' '}
          <button
            type="button"
            className="button is-small"
            onClick={() => setReload((value) => value + 1)}
          >
            重试
          </button>
        </p>
      ) : !registry ? (
        <SkeletonRows label="正在读取 Hook" />
      ) : entries.length ? (
        <ol className="hook-list settings-card-list" aria-label="Hook 执行顺序">
          {pages.shown.map(row)}
          {pages.more ? <ListSentinel sentinel={pages.sentinel} /> : null}
        </ol>
      ) : (
        <div className="settings-empty">
          <span className="settings-empty-icon" aria-hidden="true">
            <AppIcon name="activity" size={22} />
          </span>
          <strong>还没有 Hook</strong>
          <p>点“添加 Hook…”选择一个包含 hook.toml 的文件夹。</p>
        </div>
      )}
      <details className="hook-format">
        <summary>Hook 包格式</summary>
        <p>一个 Hook 是一个文件夹，根目录放 hook.toml，程序或脚本放在包内：</p>
        <pre>
          {
            'schema_version = 1\nname = "guard"\nversion = "1.0.0"\ndescription = "阻止写入 .env"\nkind = "user"            # security：失败时阻止\nevents = ["tool.before_call"]\ntimeout_ms = 10000\n[command]\nexecutable = "bin/guard.exe"   # 或 powershell_script = "guard.ps1"\n[match]\ntools = ["fs.write", "fs.patch"]'
          }
        </pre>
        <p>
          Aura 通过标准输入发给 Hook 一个 JSON 调用信封，Hook 在标准输出回一个 JSON：decision 为
          continue、transform 或 block（block 时写 reason）。
        </p>
      </details>
      {shown ? (
        <SettingsItemDialog
          title={hookName(shown)}
          heading={<HookHeading entry={shown} named={false} />}
          headingClassName="hook-row-heading"
          notice={notice}
          onClose={close}
        >
          <HookDetails
            entry={shown}
            locked={busy !== undefined}
            preview={preview?.path === shown.path ? preview : undefined}
            removing={removing === shown.path}
            history={history === shown.path}
            onPreview={() => void open(shown.path)}
            onClosePreview={() => setPreview(undefined)}
            onTrust={(review) => void trust(review)}
            onChange={(action) => void change(shown, action)}
            onToggleHistory={() => setHistory(history === shown.path ? undefined : shown.path)}
            onAskRemove={() => setRemoving(shown.path)}
            onCancelRemove={() => setRemoving(undefined)}
          />
        </SettingsItemDialog>
      ) : null}
    </section>
  );
}
