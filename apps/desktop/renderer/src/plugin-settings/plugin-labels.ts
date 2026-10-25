import type {
  PluginCounts,
  PluginServerItem,
  PluginState,
  PluginView,
} from '@aura/contracts/generated';

export { bytesLabel, skillError as pluginError } from '../skill-settings/skill-labels';

/** The colour a state is shown in: usable, needs the user, or cannot be used. */
export type StateTone = 'success' | 'warning' | 'danger' | 'muted' | 'active';

export const pluginStateTone = (state: PluginState): StateTone =>
  state === 'ready'
    ? 'success'
    : state === 'disabled'
      ? 'muted'
      : state === 'invalid' || state === 'unavailable'
        ? 'danger'
        : 'warning';

export const pluginStateLabel = (plugin: PluginView): string => {
  const state: PluginState = plugin.state;
  switch (state) {
    case 'ready':
      return '可用';
    case 'disabled':
      return '已停用';
    case 'not_trusted':
      return '未信任，内容都不提供';
    case 'content_changed':
      return '内容已变化，需要重新信任';
    case 'invalid':
      return '无法使用';
    case 'unavailable':
      return '无法读取';
  }
};

/** “Skill 2 · 命令 3 · MCP 1 · Hook 1”, leaving out what the plugin does not bring. */
export const countsLabel = (counts: PluginCounts): string =>
  [
    ['Skill', counts.skills],
    ['命令', counts.commands],
    ['MCP', counts.servers],
    ['Hook', counts.hooks],
  ]
    .filter(([, count]) => Number(count) > 0)
    .map(([label, count]) => `${label} ${count}`)
    .join(' · ') || '没有可用的内容';

export const limitsLabel = (plugin: PluginView): string =>
  `每个进程树最多 ${plugin.limits.memory_mb} MB 内存、${plugin.limits.processes} 个进程`;

export const serverTargetLabel = (server: PluginServerItem): string =>
  server.kind === 'http'
    ? `连接 ${server.target}`
    : `运行 ${[server.target, ...server.arguments].join(' ')}`;

export const signInLabel = (server: PluginServerItem): string | undefined => {
  if (server.sign_in === 'oauth') return '在浏览器中用 OAuth 登录';
  if (server.sign_in === 'secret') return '需要你在 Aura 里输入一个密钥';
  if (server.secret_names.length) return `需要你在 Aura 里输入：${server.secret_names.join('、')}`;
  return undefined;
};
