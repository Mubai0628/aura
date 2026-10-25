import type {
  HookDefinition,
  HookEffect,
  HookEntryState,
  HookEvent,
  HookFailureKind,
  HookOutcome,
} from '@aura/contracts/generated';

const EVENTS: Record<HookEvent, string> = {
  'session.started': '会话开始',
  'session.resumed': '会话恢复',
  'message.before_accept': '接收消息前',
  'turn.before_start': '轮次开始前',
  'context.before_build': '构建上下文前',
  'model.before_call': '调用模型前',
  'model.after_call': '模型返回后',
  'tool.before_call': '工具调用前',
  'tool.after_call': '工具调用后',
  'tool.on_error': '工具出错时',
  'context.before_compact': '压缩上下文前',
  'context.after_compact': '压缩上下文后',
  'turn.before_complete': '轮次完成前',
  'turn.completed': '轮次完成后',
};

export const hookEventLabel = (event: HookEvent) => EVENTS[event] ?? event;

export const hookStateLabel = (state: HookEntryState): string => {
  switch (state) {
    case 'ready':
      return '已启用';
    case 'disabled':
      return '已停用';
    case 'not_trusted':
      return '未信任，不会运行';
    case 'content_changed':
      return '内容已变化，需要重新信任';
    case 'invalid':
      return '无法读取，不会运行';
  }
};

/** The colour a Hook's state is shown in. */
export const hookStateTone = (state: HookEntryState): 'success' | 'warning' | 'danger' | 'muted' =>
  state === 'ready'
    ? 'success'
    : state === 'disabled'
      ? 'muted'
      : state === 'invalid'
        ? 'danger'
        : 'warning';

export const hookKindLabel = (definition: HookDefinition) =>
  `${definition.kind === 'security' ? '安全 Hook' : '用户 Hook'} · ${
    definition.failure_policy === 'fail_closed' ? '失败时阻止' : '失败时放行'
  }`;

export const hookCommandLabel = (definition: HookDefinition) => {
  const { command } = definition;
  const runs =
    command.type === 'executable'
      ? `运行包内程序 ${command.path}`
      : `用 Aura 自带的 PowerShell 运行包内脚本 ${command.path}`;
  return command.arguments.length ? `${runs}（参数：${command.arguments.join(' ')}）` : runs;
};

export const hookToolsLabel = (definition: HookDefinition) =>
  definition.events.some((event) => event.startsWith('tool.'))
    ? definition.tools.length
      ? `只对这些工具：${definition.tools.join('、')}`
      : '对所有工具'
    : undefined;

export const hookOutcomeLabel = (outcome: HookOutcome, failure?: HookFailureKind | null) => {
  switch (outcome) {
    case 'continued':
      return '继续';
    case 'transformed':
      return '变换';
    case 'blocked':
      return '阻止';
    case 'failed':
      return failure ? `失败（${hookFailureLabel(failure)}）` : '失败';
  }
};

export const hookFailureLabel = (failure: HookFailureKind): string => {
  switch (failure) {
    case 'content_changed':
      return '内容已变化';
    case 'not_started':
      return '未能启动';
    case 'timed_out':
      return '超时';
    case 'output_limit':
      return '输出超出上限';
    case 'exit_code':
      return '退出码不是 0';
    case 'invalid_result':
      return '结果格式无效';
    case 'cancelled':
      return '已取消';
  }
};

export const hookEffectLabel = (effect: HookEffect) =>
  effect === 'action_blocked'
    ? '动作已被阻止'
    : effect === 'later_hooks_stopped'
      ? '同一事件后面的 Hook 已停止'
      : undefined;

/**
 * Core refuses Hook changes as `hooks.<reason>: <sentence>`; the sentence is the user's.
 * Anything without a readable sentence becomes the fallback.
 */
export function hookError(value: unknown, fallback = '操作未完成，请刷新后重试。'): string {
  const text = value instanceof Error ? value.message : typeof value === 'string' ? value : '';
  const sentence = text.replace(/^[a-z_]+(?:\.[a-z_]+)+:\s*/u, '').trim();
  return sentence && sentence !== text.trim() && /[㐀-鿿]/u.test(sentence)
    ? sentence.slice(0, 400)
    : fallback;
}

export const shortFingerprint = (fingerprint?: string | null) =>
  fingerprint ? fingerprint.slice(0, 16) : '—';

export const folderName = (path: string) => path.split(/[\\/]/u).filter(Boolean).at(-1) ?? path;
