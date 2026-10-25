import type { McpServerConfig, McpServerState, McpServerView } from '@aura/contracts/generated';

export const mcpStateLabel = (server: McpServerView): string => {
  const state: McpServerState = server.state;
  switch (state) {
    case 'ready':
      return server.tool_count == null ? '可用' : `可用 · ${server.tool_count} 个工具`;
    case 'disabled':
      return '已停用';
    case 'not_trusted':
      return '未信任，不会启动';
    case 'config_changed':
      return '配置已变化，需要重新信任';
    case 'invalid':
      return '配置有问题';
    case 'starting':
      return '正在启动并列出工具…';
    case 'failed':
      return '启动失败';
    case 'sign_in_required':
      return '需要登录';
  }
};

/** The colour a server's state is shown in. */
export const mcpStateTone = (
  state: McpServerState,
): 'success' | 'warning' | 'danger' | 'muted' | 'active' => {
  switch (state) {
    case 'ready':
      return 'success';
    case 'starting':
      return 'active';
    case 'disabled':
      return 'muted';
    case 'failed':
    case 'invalid':
      return 'danger';
    case 'not_trusted':
    case 'config_changed':
    case 'sign_in_required':
      return 'warning';
  }
};

export const transportSummary = (config: McpServerConfig): string =>
  config.transport.type === 'stdio'
    ? [config.transport.command, ...(config.transport.arguments ?? [])].join(' ')
    : config.transport.url;

/** How a server signs in, as trust covers it: where a secret goes or the OAuth client and
 * scopes, or a program's secret variable names. Never a secret. */
export function authSummary(config: McpServerConfig): string | undefined {
  const transport = config.transport;
  if (transport.type === 'stdio') {
    const names = (transport.secret_environment ?? []).map((entry) => entry.name);
    return names.length ? `带密钥的环境变量：${names.join('、')}` : undefined;
  }
  const auth = transport.auth;
  if (!auth) return undefined;
  if (auth.type === 'secret')
    return `认证：密钥（${auth.header ? `请求头 ${auth.header}` : 'Authorization: Bearer'}），由 Aura 的网络网关附加`;
  const scopes = auth.scopes?.length ? auth.scopes.join(' ') : '按服务器的要求';
  return `认证：OAuth 登录（客户端 ID：${auth.client_id ?? '自动注册'}；权限：${scopes}）`;
}

/** What trusting a server covers: its program and arguments, or its address and headers;
 * and how it signs in. */
export const trustSummary = (config: McpServerConfig): string =>
  [
    ...(config.transport.type === 'stdio'
      ? [transportSummary(config)]
      : [
          config.transport.url,
          ...(config.transport.headers ?? []).map(({ name, value }) => `请求头 ${name}: ${value}`),
        ]),
    authSummary(config),
  ]
    .filter((line): line is string => line !== undefined)
    .join('\n');

/** Core refuses MCP changes as `mcp.<reason>: <sentence>`; the sentence is the user's. */
export function mcpError(value: unknown, fallback = '操作未完成，请刷新后重试。'): string {
  const text = value instanceof Error ? value.message : typeof value === 'string' ? value : '';
  const sentence = text.replace(/^[a-z_]+(?:\.[a-z_]+)+:\s*/u, '').trim();
  return sentence && sentence !== text.trim() && /[㐀-鿿]/u.test(sentence)
    ? sentence.slice(0, 600)
    : fallback;
}

/** `NAME=value` lines ↔ entries; blank lines are ignored. */
export const parsePairs = (text: string, separator: '=' | ':') =>
  text
    .split(/\r?\n/u)
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => {
      const at = line.indexOf(separator);
      return at < 0
        ? { name: line, value: '' }
        : { name: line.slice(0, at).trim(), value: line.slice(at + 1).trim() };
    });

export const formatPairs = (
  entries: ReadonlyArray<{ name: string; value: string }> | undefined,
  separator: '=' | ':',
) =>
  (entries ?? [])
    .map(({ name, value }) => `${name}${separator}${separator === ':' ? ' ' : ''}${value}`)
    .join('\n');
