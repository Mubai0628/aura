import type { PluginPreview } from '@aura/contracts/generated';
import { CAPABILITY_LABELS, PLATFORM_LABELS } from './plugin-labels';

/** What a version 2 plugin declares besides its Skills, commands, servers and Hooks: the
 * capabilities its panel and background service ask for, the panel, the service, its own
 * settings and its theme. All of it is what trusting the plugin trusts. */
export function PluginDeclarations({ preview }: { preview: PluginPreview }) {
  const capabilities = preview.capabilities ?? [];
  const options = preview.options ?? [];
  const channels = preview.channels ?? [];
  return (
    <>
      {capabilities.length ? (
        <section aria-label="会话能力">
          <h4>会话能力</h4>
          <ul>
            {capabilities.map((capability) => (
              <li key={capability}>
                {CAPABILITY_LABELS[capability]} <code>{capability}</code>
              </li>
            ))}
          </ul>
        </section>
      ) : null}
      {preview.panel ? (
        <section aria-label="面板">
          <h4>面板</h4>
          <ul>
            <li>
              <strong>{preview.panel.panel.title}</strong> <code>{preview.panel.panel.entry}</code>
              {preview.panel.panel.network.length ? (
                <p>可连接：{preview.panel.panel.network.join('、')}</p>
              ) : (
                <p>不联网</p>
              )}
              {preview.panel.problems.length ? (
                <p role="alert">{preview.panel.problems.join('；')}</p>
              ) : null}
            </li>
          </ul>
        </section>
      ) : null}
      {preview.service ? (
        <section aria-label="后台服务">
          <h4>后台服务</h4>
          <ul>
            <li>
              <p className="plugin-mono">
                运行{' '}
                {[preview.service.service.command, ...preview.service.service.arguments].join(' ')}
              </p>
              {preview.service.service.network.length ? (
                <p>声明连接：{preview.service.service.network.join('、')}</p>
              ) : null}
              {preview.service.problems.length ? (
                <p role="alert">{preview.service.problems.join('；')}</p>
              ) : null}
            </li>
          </ul>
        </section>
      ) : null}
      {channels.length ? (
        <section aria-label="渠道">
          <h4>渠道</h4>
          <ul>
            {channels.map((channel) => (
              <li key={channel.id}>
                <strong>{channel.title}</strong> {PLATFORM_LABELS[channel.platform]}{' '}
                <code>{channel.id}</code>
              </li>
            ))}
          </ul>
        </section>
      ) : null}
      {preview.theme ? (
        <section aria-label="主题">
          <h4>主题</h4>
          <ul>
            <li>
              {preview.theme.title ? <strong>{preview.theme.title}</strong> : null}{' '}
              <code>theme.toml</code>
              {preview.theme.problems.length ? (
                <p role="alert">{preview.theme.problems.join('；')}</p>
              ) : null}
            </li>
          </ul>
        </section>
      ) : null}
      {options.length ? (
        <section aria-label="插件设置">
          <h4>插件设置</h4>
          <ul>
            {options.map((option) => (
              <li key={option.key}>
                <strong>{option.title}</strong> <code>{option.key}</code>
                {option.kind === 'secret' ? <span>（保密）</span> : null}
                {option.required ? <span>（必填）</span> : null}
                {option.description ? <p>{option.description}</p> : null}
              </li>
            ))}
          </ul>
        </section>
      ) : null}
    </>
  );
}
