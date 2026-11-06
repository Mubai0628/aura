import type {
  PluginCapability,
  PluginCounts,
  PluginPlatform,
  PluginServerItem,
  PluginServiceStatus,
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
    ['面板', counts.panels ?? 0],
    ['后台服务', counts.services ?? 0],
    ['渠道', counts.channels ?? 0],
    ['主题', counts.themes ?? 0],
  ]
    .filter(([, count]) => Number(count) > 0)
    .map(([label, count]) => `${label} ${count}`)
    .join(' · ') || '没有可用的内容';

/** What each capability lets the plugin's panel and background service do. */
export const CAPABILITY_LABELS: Record<PluginCapability, string> = {
  'session.read': '读会话的公开状态：标题、模型、用量、待办、上下文占用',
  'session.events': '收到会话事件：回合开始和结束、最终回复、等待审批、出错、压缩',
  'session.send': '替你在会话里发消息，消息标明来自这个插件',
  'session.control': '停止当前回合、新建会话、切换权限模式',
  notify: '显示提示',
  status: '在状态栏显示一条',
  commands: '运行插件自己的命令，照常走审批',
  settings: '读写插件自己的设置',
};

/** What a background service is doing, and why when it is not running. */
export const serviceStatusLabel = (
  status: PluginServiceStatus,
): { label: string; tone: StateTone; detail?: string } => {
  switch (status.state) {
    case 'off':
      return { label: '未开启', tone: 'muted' };
    case 'waiting':
      return { label: '未启动', tone: 'warning', detail: status.reason };
    case 'starting':
      return { label: '启动中', tone: 'active' };
    case 'running':
      return { label: '运行中', tone: 'success' };
    case 'restarting':
      return { label: `${status.seconds} 秒后重启`, tone: 'warning', detail: status.reason };
    case 'stopped':
      return { label: '已停止', tone: 'danger', detail: status.reason };
  }
};

export const PLATFORM_LABELS: Record<PluginPlatform, string> = {
  qq: 'QQ',
  feishu: '飞书',
  wechat: '微信',
  wecom: '企业微信',
  telegram: 'Telegram',
  dingtalk: '钉钉',
  slack: 'Slack',
  discord: 'Discord',
  onebot: 'OneBot',
  satori: 'Satori',
  other: '其他平台',
};

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

/** Whether a plugin can be used at all, and whether it waits for the user's trust first. */
export function pluginTrust(plugin: PluginView): { usable: boolean; needsTrust: boolean } {
  const usable = plugin.state !== 'invalid' && plugin.state !== 'unavailable';
  const needsTrust =
    usable && (plugin.trusted_fingerprint == null || plugin.state === 'content_changed');
  return { usable, needsTrust };
}
