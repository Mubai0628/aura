import { userMessage } from '../agent-session/user-message';
import { useEffect, useState, type CSSProperties } from 'react';
import type { CommandEntry, CommandSource, WorkspaceId } from '@aura/contracts/generated';
import { AppIcon } from '../app-icons';
import { PagedList } from '../ui/PagedList';
import { SkeletonRows } from '../ui/SkeletonRows';
import './command-settings.css';

interface Loaded {
  key: string;
  commands?: CommandEntry[];
  error?: string;
}

/** Settings → 命令: built-ins for reference, and the user's and the workspace's prompt commands. */
export function CommandSettings({
  coreReady,
  workspaceId,
}: {
  coreReady: boolean;
  workspaceId?: WorkspaceId | undefined;
}) {
  const [loaded, setLoaded] = useState<Loaded>();
  const [reload, setReload] = useState(0);
  const [busy, setBusy] = useState<string>();
  const [notice, setNotice] = useState<{ alert: boolean; text: string }>();
  const [previewing, setPreviewing] = useState<string>();
  const [removing, setRemoving] = useState<CommandEntry>();
  const key = `${workspaceId ?? '-'}:${reload}`;
  useEffect(() => {
    if (!coreReady) return;
    let live = true;
    void window.aura.v1
      .listCommands({ workspace_id: workspaceId ?? null })
      .then((result) => {
        if (live)
          setLoaded(
            result.ok
              ? { key, commands: result.value.commands }
              : { key, error: userMessage(result.error, '命令列表读取失败，请重试。') },
          );
      })
      .catch((reason: unknown) => {
        if (live) setLoaded({ key, error: userMessage(reason, '命令列表读取失败，请重试。') });
      });
    return () => {
      live = false;
    };
  }, [coreReady, workspaceId, key]);
  const current = loaded?.key === key ? loaded : undefined;
  const commands = current?.commands ?? [];
  const id = (command: CommandEntry) => `${command.source}:${command.name}`;

  async function act(
    label: string,
    work: () => Promise<{ ok: boolean; error?: { message: string } }>,
    done: string,
  ) {
    if (busy) return;
    setBusy(label);
    setNotice(undefined);
    try {
      const result = await work();
      setNotice(
        result.ok
          ? { alert: false, text: done }
          : { alert: true, text: userMessage(result.error, '操作未完成，请刷新后核对。') },
      );
      if (result.ok) setReload((value) => value + 1);
    } catch (reason) {
      setNotice({ alert: true, text: userMessage(reason, '操作结果未确认，请刷新后核对。') });
    } finally {
      setBusy(undefined);
    }
  }
  const toggle = (command: CommandEntry) =>
    act(
      id(command),
      () =>
        window.aura.v1.setCommandEnabled({
          workspace_id: workspaceId ?? null,
          name: command.name,
          source: command.source,
          enabled: !command.enabled,
        }),
      command.enabled ? `已停用 /${command.name}` : `已启用 /${command.name}`,
    );
  const open = (source: CommandSource, name: string | null) =>
    act(
      `open:${source}:${name ?? ''}`,
      async () => {
        const result = await window.aura.v1.openCommandDirectory({
          workspace_id: workspaceId ?? null,
          source,
          name,
        });
        return result.ok && !result.value.completed
          ? { ok: false, error: { message: result.value.message } }
          : result;
      },
      '已打开目录',
    );
  const remove = (command: CommandEntry) => {
    setRemoving(undefined);
    return act(
      id(command),
      () =>
        window.aura.v1.deleteCommand({
          workspace_id: workspaceId ?? null,
          name: command.name,
          revision: command.revision ?? '',
        }),
      `已删除 /${command.name}`,
    );
  };

  // A plugin's command is switched with its plugin.
  const row = (command: CommandEntry, index: number) => (
    <li
      key={id(command)}
      className="settings-card"
      data-tone={command.issue ? 'warning' : command.enabled ? undefined : 'muted'}
      style={{ '--item': index } as CSSProperties}
    >
      <div className="settings-card-heading command-row-heading">
        <code>/{command.name}</code>
        <span>{command.title}</span>
        {command.plugin ? (
          <span className="settings-tag command-row-source">插件 {command.plugin}</span>
        ) : null}
        <span
          className="status-pill command-row-state"
          data-tone={command.issue ? 'warning' : command.enabled ? 'success' : 'muted'}
        >
          {command.issue ? `未提供：${command.issue}` : command.enabled ? '已启用' : '已停用'}
        </span>
      </div>
      {command.description ? <p>{command.description}</p> : null}
      {command.location ? <p className="command-row-location">{command.location}</p> : null}
      <div className="settings-card-actions">
        {command.template != null ? (
          <button
            type="button"
            className="button is-small"
            aria-expanded={previewing === id(command)}
            onClick={() => setPreviewing(previewing === id(command) ? undefined : id(command))}
          >
            {previewing === id(command) ? '收起预览' : '预览'}
          </button>
        ) : null}
        {command.issue == null && command.source !== 'plugin' ? (
          <button
            type="button"
            className="button is-small"
            disabled={busy !== undefined}
            onClick={() => void toggle(command)}
          >
            {command.enabled ? '停用' : '启用'}
          </button>
        ) : null}
        {command.source !== 'plugin' ? (
          <button
            type="button"
            className="button is-ghost is-small"
            disabled={busy !== undefined}
            onClick={() => void open(command.source, command.name)}
          >
            <AppIcon name="folder" size={14} />
            打开目录
          </button>
        ) : null}
        {command.source === 'user' ? (
          <button
            type="button"
            className="button is-danger is-small command-remove"
            disabled={busy !== undefined}
            onClick={() => setRemoving(command)}
          >
            删除
          </button>
        ) : null}
      </div>
      {previewing === id(command) && command.template != null ? (
        <pre aria-label={`/${command.name} 的模板`}>{command.template}</pre>
      ) : null}
      {removing && id(removing) === id(command) ? (
        <div
          role="alertdialog"
          aria-label={`删除 /${command.name}`}
          className="settings-panel is-danger"
        >
          <p>
            删除 /{command.name} 会移除它的整个目录
            {command.location ? `（${command.location}）` : ''}
            ，不能撤销。
          </p>
          <div className="settings-card-actions">
            <button
              type="button"
              className="button is-danger is-solid is-small"
              onClick={() => void remove(command)}
            >
              删除
            </button>
            <button
              type="button"
              className="button is-small"
              onClick={() => setRemoving(undefined)}
            >
              取消
            </button>
          </div>
        </div>
      ) : null}
    </li>
  );
  const group = (source: CommandSource) => commands.filter((command) => command.source === source);

  return (
    <section className="command-settings settings-page" aria-label="命令设置">
      <header className="settings-content-heading">
        <div>
          <h2>命令</h2>
        </div>
        <div className="settings-page-actions">
          <button
            type="button"
            className="button"
            disabled={busy !== undefined}
            onClick={() => void open('user', null)}
          >
            <AppIcon name="folder" size={15} />
            打开我的命令目录
          </button>
          {workspaceId ? (
            <button
              type="button"
              className="button"
              disabled={busy !== undefined}
              onClick={() => void open('workspace', null)}
            >
              打开工作区命令目录
            </button>
          ) : null}
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
      {!coreReady ? (
        <p className="settings-status-line" role="status">
          Core 尚未就绪，命令管理暂不可用。
        </p>
      ) : current?.error ? (
        <p className="settings-status-line" role="alert">
          {current.error}{' '}
          <button
            type="button"
            className="button is-small"
            onClick={() => setReload((value) => value + 1)}
          >
            重试
          </button>
        </p>
      ) : !current ? (
        <SkeletonRows label="正在读取命令" />
      ) : null}
      <details className="command-format">
        <summary>命令文件格式</summary>
        <p>每个命令一个文件夹，文件夹名就是命令名（小写字母、数字和 -）：</p>
        <pre>
          {
            'command.toml\n  name = "review"\n  title = "审查改动"\n  description = "列出本次改动的风险"\n  argument_hint = "范围（可选）"\n  version = 1\ntemplate.md\n  请审查 $ARGUMENTS 的改动……'
          }
        </pre>
        <p>$ARGUMENTS 是插入后光标所在的位置。声明 runtime 的命令不会被提供。</p>
      </details>
      <h3>我的命令</h3>
      {group('user').length ? (
        <PagedList className="command-list settings-card-list" items={group('user')} row={row} />
      ) : (
        <p className="command-empty">还没有自己的命令。</p>
      )}
      <h3>工作区命令</h3>
      {!workspaceId ? (
        <p className="command-empty">选择工作区后可查看该项目的命令。</p>
      ) : group('workspace').length ? (
        <PagedList
          className="command-list settings-card-list"
          items={group('workspace')}
          row={row}
        />
      ) : (
        <p className="command-empty">此工作区没有命令（.aura/commands）。</p>
      )}
      {group('plugin').length ? (
        <>
          <h3>插件的命令</h3>
          <PagedList
            className="command-list settings-card-list"
            items={group('plugin')}
            row={row}
          />
        </>
      ) : null}
      <h3>内置命令</h3>
      <ul className="command-list settings-card-list is-builtin">
        {group('builtin').map((command, index) => (
          <li
            key={id(command)}
            className="settings-card"
            style={{ '--item': index } as CSSProperties}
          >
            <div className="command-row-heading">
              <code>/{command.name}</code>
              <span>{command.title}</span>
            </div>
            <p>{command.description}</p>
          </li>
        ))}
      </ul>
    </section>
  );
}
