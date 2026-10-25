import { useState } from 'react';
import type { HookEntryView, McpServerView } from '@aura/contracts/generated';
import { HookInvocations } from '../hook-settings/HookInvocations';
import { hookStateLabel, hookStateTone } from '../hook-settings/hook-labels';
import { mcpStateLabel, mcpStateTone } from '../mcp-settings/mcp-labels';
import type { SettingsSection } from '../app-shell-types';

/**
 * How what a plugin brings is running: each of its MCP servers with
 * its state, running instances and latest error, and each of its Hooks with its state and its
 * recent calls. The same facts the MCP and Hook pages show, gathered under the plugin.
 */
export function PluginRuntime({
  servers,
  hooks,
  onOpenSection,
}: {
  servers: McpServerView[];
  hooks: HookEntryView[];
  onOpenSection?: ((section: SettingsSection) => void) | undefined;
}) {
  // A Hook's calls are read only once its list is opened, as on the Hook page.
  const [opened, setOpened] = useState<ReadonlySet<string>>(new Set());
  if (!servers.length && !hooks.length) return null;
  return (
    <section className="plugin-runtime" aria-label="运行情况">
      <h4>运行情况</h4>
      {servers.length ? (
        <ul className="plugin-runtime-list" aria-label="它的 MCP 服务器">
          {servers.map((server) => (
            <li key={server.config.id}>
              <div className="plugin-runtime-row">
                <span className="status-pill" data-tone={mcpStateTone(server.state)}>
                  {mcpStateLabel(server)}
                </span>
                <strong>{server.config.display_name}</strong>
                <code>{server.config.id}</code>
                {server.running_instances ? (
                  <span className="plugin-runtime-count">运行中 {server.running_instances}</span>
                ) : null}
                {onOpenSection ? (
                  <button
                    type="button"
                    className="button is-ghost is-small"
                    onClick={() => onOpenSection('mcp')}
                  >
                    在 MCP 页查看
                  </button>
                ) : null}
              </div>
              {server.last_error ? (
                <p className="plugin-runtime-error" role="alert">
                  最近的错误：{server.last_error}
                </p>
              ) : null}
            </li>
          ))}
        </ul>
      ) : null}
      {hooks.length ? (
        <ul className="plugin-runtime-list" aria-label="它的 Hook">
          {hooks.map((hook) => {
            const name = hook.definition?.name ?? hook.path;
            return (
              <li key={hook.path}>
                <div className="plugin-runtime-row">
                  <span className="status-pill" data-tone={hookStateTone(hook.state)}>
                    {hookStateLabel(hook.state)}
                  </span>
                  <strong>{name}</strong>
                  {hook.definition ? (
                    <span className="plugin-runtime-count">
                      {hook.definition.events.join('、')}
                    </span>
                  ) : null}
                </div>
                {hook.problems.length ? (
                  <p className="plugin-runtime-error" role="alert">
                    {hook.problems.join('；')}
                  </p>
                ) : null}
                <details
                  onToggle={(event) => {
                    const open = event.currentTarget.open;
                    setOpened((current) => {
                      const next = new Set(current);
                      if (open) next.add(hook.path);
                      else next.delete(hook.path);
                      return next;
                    });
                  }}
                >
                  <summary>最近调用</summary>
                  {opened.has(hook.path) ? <HookInvocations path={hook.path} name={name} /> : null}
                </details>
              </li>
            );
          })}
        </ul>
      ) : null}
    </section>
  );
}
