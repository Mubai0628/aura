import { useState } from 'react';
import type {
  PluginAllowedUser,
  PluginRemoteView,
  PluginScope,
  PluginView,
} from '@aura/contracts/generated';
import { restartTime } from '../agent-session/turn-continuation';
import { ToggleSwitch } from '../ui/ToggleSwitch';
import { useWorkspaceSessions } from '../workspace-session-store';
import type { PluginChange } from './PluginDetails';
import { PLATFORM_LABELS } from './plugin-labels';

const toggled = <T,>(list: readonly T[] | undefined, value: T, on: boolean): T[] => {
  const rest = (list ?? []).filter((item) => item !== value);
  return on ? [...rest, value] : rest;
};

const sameScope = (left: PluginScope, right: PluginScope) => {
  const key = (scope: PluginScope) =>
    JSON.stringify([[...(scope.workspaces ?? [])].sort(), [...(scope.sessions ?? [])].sort()]);
  return key(left) === key(right);
};

const listed = (allowed: readonly PluginAllowedUser[], channel: string, id: string) =>
  allowed.some((user) => user.channel === channel && user.id === id);

interface RemoteProps {
  plugin: PluginView;
  busy: boolean;
  onConfigure: (change: PluginChange, done: string) => void;
}

/**
 * What the plugin's background service may reach: the conversations in its scope, who on each
 * of its channels may write to Aura, who wrote without being on the list, and remote approval.
 */
export function PluginRemote(props: RemoteProps) {
  const remote = props.plugin.remote;
  return remote ? <RemoteSettings {...props} remote={remote} /> : null;
}

function RemoteSettings({
  plugin,
  busy,
  onConfigure,
  remote,
}: RemoteProps & { remote: PluginRemoteView }) {
  const projection = useWorkspaceSessions().projection?.payload;
  const [scope, setScope] = useState<PluginScope>();
  const [adding, setAdding] = useState<Record<string, { id: string; name: string }>>({});
  const shown = scope ?? remote.scope;
  const allowed = remote.allowed ?? [];
  const channels = remote.channels ?? [];
  const workspaces = (projection?.workspaces ?? []).filter((item) => item.state !== 'archived');
  const sessions = (projection?.sessions ?? []).filter((item) => item.state !== 'archived');
  const saveAllowed = (next: PluginAllowedUser[]) =>
    onConfigure({ allowed: next }, `已更新 ${plugin.name} 的白名单`);
  const channelTitle = (id: string) => channels.find((item) => item.id === id)?.title ?? id;
  return (
    <section className="plugin-remote" aria-label="远程">
      <strong>会话范围</strong>
      <ul className="plugin-scope">
        {workspaces.map((workspace) => {
          const whole = (shown.workspaces ?? []).includes(workspace.workspace_id);
          const own = sessions.filter((item) => item.workspace_id === workspace.workspace_id);
          return (
            <li key={workspace.workspace_id}>
              <label>
                <input
                  type="checkbox"
                  checked={whole}
                  disabled={busy}
                  onChange={(event) => {
                    const on = event.currentTarget.checked;
                    setScope({
                      ...shown,
                      workspaces: toggled(shown.workspaces, workspace.workspace_id, on),
                    });
                  }}
                />
                {workspace.display_name}
              </label>
              {own.length && !whole ? (
                <ul aria-label={`${workspace.display_name} 的会话`}>
                  {own.map((session) => (
                    <li key={session.session_id}>
                      <label>
                        <input
                          type="checkbox"
                          checked={(shown.sessions ?? []).includes(session.session_id)}
                          disabled={busy}
                          onChange={(event) => {
                            const on = event.currentTarget.checked;
                            setScope({
                              ...shown,
                              sessions: toggled(shown.sessions, session.session_id, on),
                            });
                          }}
                        />
                        {session.title}
                      </label>
                    </li>
                  ))}
                </ul>
              ) : null}
            </li>
          );
        })}
      </ul>
      <div className="settings-card-actions">
        <button
          type="button"
          className="button is-small"
          disabled={busy || !scope || sameScope(scope, remote.scope)}
          onClick={() => onConfigure({ scope: shown }, `已保存 ${plugin.name} 的会话范围`)}
        >
          保存会话范围
        </button>
      </div>
      {channels.map((channel) => {
        const draft = adding[channel.id] ?? { id: '', name: '' };
        const id = draft.id.trim();
        const setDraft = (next: { id: string; name: string }) =>
          setAdding((known) => ({ ...known, [channel.id]: next }));
        return (
          <div
            key={channel.id}
            role="group"
            className="plugin-allowed"
            aria-label={`${channel.title} 的白名单`}
          >
            <strong>
              {channel.title} · {PLATFORM_LABELS[channel.platform]}
            </strong>
            <ul>
              {allowed
                .filter((user) => user.channel === channel.id)
                .map((user) => (
                  <li key={user.id}>
                    <span>{user.name || user.id}</span>
                    {user.name ? <span className="plugin-allowed-id">{user.id}</span> : null}
                    <button
                      type="button"
                      className="button is-ghost is-small"
                      aria-label={`移除 ${user.name || user.id}`}
                      disabled={busy}
                      onClick={() =>
                        saveAllowed(
                          allowed.filter(
                            (item) => item.channel !== channel.id || item.id !== user.id,
                          ),
                        )
                      }
                    >
                      移除
                    </button>
                  </li>
                ))}
            </ul>
            <div className="plugin-allowed-add">
              <label className="plugin-option">
                用户编号
                <input
                  type="text"
                  value={draft.id}
                  maxLength={128}
                  disabled={busy}
                  onChange={(event) => setDraft({ ...draft, id: event.currentTarget.value })}
                />
              </label>
              <label className="plugin-option">
                名字
                <input
                  type="text"
                  value={draft.name}
                  maxLength={64}
                  disabled={busy}
                  onChange={(event) => setDraft({ ...draft, name: event.currentTarget.value })}
                />
              </label>
              <button
                type="button"
                className="button is-small"
                disabled={busy || !id || listed(allowed, channel.id, id)}
                onClick={() => {
                  saveAllowed([...allowed, { channel: channel.id, id, name: draft.name.trim() }]);
                  setDraft({ id: '', name: '' });
                }}
              >
                添加
              </button>
            </div>
          </div>
        );
      })}
      {remote.unknown?.length ? (
        <div role="group" className="plugin-unknown" aria-label="不在名单里的用户">
          <strong>不在名单里的用户</strong>
          <ul>
            {remote.unknown.map((user) => (
              <li key={`${user.channel}:${user.id}`}>
                <span>
                  {channelTitle(user.channel)} · {user.name || user.id} ·{' '}
                  {user.chat === 'group' ? '群聊' : '私聊'} · {restartTime(user.at)}
                </span>
                <button
                  type="button"
                  className="button is-small"
                  aria-label={`把 ${user.name || user.id} 加入白名单`}
                  disabled={busy || listed(allowed, user.channel, user.id)}
                  onClick={() =>
                    saveAllowed([
                      ...allowed,
                      { channel: user.channel, id: user.id, name: user.name ?? '' },
                    ])
                  }
                >
                  加入白名单
                </button>
              </li>
            ))}
          </ul>
        </div>
      ) : null}
      <div className="plugin-service-heading">
        <strong>远程批准</strong>
        <ToggleSwitch
          checked={remote.remote_approval}
          disabled={busy}
          label={`${plugin.name} 的远程批准`}
          onCheckedChange={(on) =>
            onConfigure(
              { remote_approval: on },
              on ? `已开启 ${plugin.name} 的远程批准` : `已关闭 ${plugin.name} 的远程批准`,
            )
          }
        />
      </div>
    </section>
  );
}
