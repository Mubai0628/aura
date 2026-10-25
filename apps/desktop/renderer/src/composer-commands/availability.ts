import type { CommandEntry } from '@aura/contracts/generated';

/** What the composer knows about its session when the menu opens. */
export interface CommandContext {
  /** A real Core session exists (not a new conversation's first draft). */
  session: boolean;
  activeTurn: boolean;
  queuePending: boolean;
  modelSelected: boolean;
}

/** Built-in commands this renderer can carry out. Others in the catalog are not offered. */
export const SUPPORTED_BUILTINS = new Set([
  'plan',
  'goal',
  'init',
  'model',
  'permission',
  'stop',
  'compact',
  'context',
  'tools',
  'memory',
  'files',
  'diff',
  'terminal',
  'process',
  'help',
]);

/** The commands the composer offers: supported built-ins and runnable prompt commands. */
export function offered(commands: readonly CommandEntry[]): CommandEntry[] {
  return commands.filter((command) =>
    command.source === 'builtin'
      ? SUPPORTED_BUILTINS.has(command.name)
      : command.kind === 'prompt' && command.enabled && command.issue == null,
  );
}

/** Why a command cannot run now, shown greyed in the menu; undefined when it can. */
export function unavailableReason(
  command: CommandEntry,
  context: CommandContext,
): string | undefined {
  if (command.source !== 'builtin') return undefined;
  switch (command.name) {
    case 'stop':
      return context.activeTurn ? undefined : '只在任务运行中可用';
    // A mode applies to the next task; the running one and queued messages keep theirs.
    case 'plan':
    case 'goal':
      return context.activeTurn || context.queuePending
        ? '当前任务结束、排队消息处理完之后可用'
        : undefined;
    // During a task it is registered for the
    // next safe point; otherwise it is a task of its own.
    case 'compact':
      return context.session ? undefined : '发送第一条消息后可用';
    case 'init':
      if (context.activeTurn || context.queuePending)
        return '当前任务结束、排队消息处理完之后才能开始';
      return context.modelSelected ? undefined : '请先选择模型';
    case 'context':
    case 'files':
    case 'diff':
    case 'terminal':
    case 'process':
      return context.session ? undefined : '发送第一条消息后可用';
    default:
      return undefined;
  }
}
