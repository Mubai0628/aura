import type { PluginPreview } from '@aura/contracts/generated';
import { AppIcon } from '../app-icons';
import { bytesLabel, limitsLabel, serverTargetLabel, signInLabel } from './plugin-labels';

/** Everything a plugin (or its waiting update) brings, read before the user trusts it. The
 * button that showed it hides it, so it has no close button of its own. */
export function PluginPreviewPanel({
  preview,
  busy,
  onTrust,
}: {
  preview: PluginPreview;
  busy: boolean;
  onTrust?: (() => void) | undefined;
}) {
  const { plugin } = preview;
  const pendingVersion = plugin.pending_update?.version;
  const bytes = preview.files.reduce((sum, file) => sum + file.bytes, 0);
  return (
    <div
      className="plugin-preview settings-panel"
      role="region"
      aria-label={`${plugin.name} ${preview.pending ? '的新版本' : ''}的内容`}
    >
      {plugin.description ? (
        <p className="plugin-preview-description">{plugin.description}</p>
      ) : null}
      <p className="plugin-preview-lead">
        {preview.pending
          ? `新版本 ${pendingVersion ?? ''}（当前 ${plugin.version ?? '未标明'}）带来：`
          : '这个插件带来：'}
      </p>
      {preview.skills.length ? (
        <section aria-label="Skill">
          <h4>Skill</h4>
          <ul>
            {preview.skills.map((skill) => (
              <li key={skill.relative}>
                <strong>{skill.name ?? skill.relative}</strong>
                {skill.description ? <span> — {skill.description}</span> : null}
                {skill.problems.length ? <p role="alert">{skill.problems.join('；')}</p> : null}
              </li>
            ))}
          </ul>
        </section>
      ) : null}
      {preview.commands.length ? (
        <section aria-label="命令">
          <h4>命令</h4>
          <ul>
            {preview.commands.map((command) => (
              <li key={command.name}>
                <code>/{command.name}</code>
                {command.description ? <span> — {command.description}</span> : null}
                {command.problem ? <p role="alert">{command.problem}</p> : null}
                {command.warnings.length ? (
                  <p className="plugin-warning">{command.warnings.join('；')}</p>
                ) : null}
              </li>
            ))}
          </ul>
        </section>
      ) : null}
      {preview.servers.length ? (
        <section aria-label="MCP 服务器">
          <h4>MCP 服务器</h4>
          <ul>
            {preview.servers.map((server) => (
              <li key={server.id}>
                <strong>{server.display_name}</strong> <code>{server.id}</code>
                <p className="plugin-mono">{serverTargetLabel(server)}</p>
                {signInLabel(server) ? <p>{signInLabel(server)}</p> : null}
                {server.problems.length ? <p role="alert">{server.problems.join('；')}</p> : null}
              </li>
            ))}
          </ul>
        </section>
      ) : null}
      {preview.hooks.length ? (
        <section aria-label="Hook">
          <h4>Hook</h4>
          <ul>
            {preview.hooks.map((hook) => (
              <li key={hook.folder}>
                <strong>{hook.definition?.name ?? hook.folder}</strong>
                {hook.definition ? (
                  <span>
                    {' '}
                    — 事件：{hook.definition.events.join('、')}
                    {hook.definition.kind === 'security' ? '（安全 Hook，失败即阻断）' : ''}
                  </span>
                ) : null}
                {hook.problems.length ? <p role="alert">{hook.problems.join('；')}</p> : null}
              </li>
            ))}
          </ul>
        </section>
      ) : null}
      {plugin.unsupported.length && !preview.pending ? (
        <section aria-label="不支持的部分">
          <h4>Aura 不使用的部分</h4>
          <ul>
            {plugin.unsupported.map((part) => (
              <li key={part}>{part}</li>
            ))}
          </ul>
        </section>
      ) : null}
      <div className="settings-callout is-warning plugin-note">
        <AppIcon name="shield" size={15} />
        <p>
          {limitsLabel(plugin)}。共 {preview.files.length} 个文件，{bytesLabel(bytes)}。
          插件里的程序以你的身份运行，能做你能做的事；只信任你了解的插件。
        </p>
      </div>
      {onTrust ? (
        <div className="settings-card-actions">
          <button type="button" className="button is-primary" disabled={busy} onClick={onTrust}>
            <AppIcon name="check" size={15} />
            {preview.pending ? '信任并更新' : '信任并启用'}
          </button>
        </div>
      ) : null}
    </div>
  );
}
