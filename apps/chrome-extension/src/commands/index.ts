/**
 * One command from Core: checked against its shape, held to the site Core authorized it for, and
 * carried out in its tab.
 */
import { inspect, originOf } from '../page/inspect.js';
import { CommandError, type ChromeCommand } from '../protocol.js';
import { act } from './act.js';
import { refused, type CommandContext } from './context.js';
import { consoleOf, report, screenshot, wait } from './look.js';
import { assist, close, navigate, open } from './tabs.js';

type Check = (value: unknown) => boolean;
const count: Check = (value) => Number.isInteger(value) && (value as number) >= 0;
const text: Check = (value) => typeof value === 'string';
const number: Check = (value) => typeof value === 'number' && Number.isFinite(value);
const flag: Check = (value) => typeof value === 'boolean';
const maybe =
  (check: Check): Check =>
  (value) =>
    value === null || check(value);
const oneOf =
  (...allowed: string[]): Check =>
  (value) =>
    typeof value === 'string' && allowed.includes(value);
const list =
  (check: Check): Check =>
  (value) =>
    Array.isArray(value) && value.every(check);

const INTENTS = oneOf('click', 'type', 'press', 'select', 'hover', 'scroll');
const SHAPES: Record<ChromeCommand['op'], Record<string, Check>> = {
  open: { url: text },
  navigate: { tab_id: count, url: maybe(text), action: maybe(oneOf('back', 'forward', 'reload')) },
  snapshot: { tab_id: count, max_chars: count },
  screenshot: { tab_id: count, element: maybe(text), full_page: flag },
  inspect: {
    tab_id: count,
    element: maybe(text),
    x: maybe(number),
    y: maybe(number),
    intent: INTENTS,
  },
  click: {
    tab_id: count,
    element: maybe(text),
    x: maybe(number),
    y: maybe(number),
    button: oneOf('left', 'right', 'middle'),
    double: flag,
  },
  type: { tab_id: count, element: maybe(text), text, clear: flag, submit: flag },
  press: { tab_id: count, key: text, repeat: count, hold_ms: count },
  scroll: {
    tab_id: count,
    direction: oneOf('up', 'down', 'left', 'right'),
    element: maybe(text),
    amount: count,
  },
  hover: { tab_id: count, element: text },
  select: { tab_id: count, element: text, values: list(text) },
  wait: {
    tab_id: count,
    text: maybe(text),
    text_gone: maybe(text),
    time_ms: maybe(count),
    timeout_ms: count,
  },
  console: { tab_id: count, level: maybe(oneOf('error', 'warning', 'all')), max_entries: count },
  close: { tab_id: count },
  assist: { tab_id: count, reason: text, timeout_seconds: count },
  release: { tab_ids: list(count) },
};

/** The command in a message from Core, in exactly one of the shapes Core sends. */
export function parseCommand(value: unknown): ChromeCommand {
  const record =
    typeof value === 'object' && value !== null ? (value as Record<string, unknown>) : {};
  const op = typeof record.op === 'string' ? record.op : '';
  const shape = Object.hasOwn(SHAPES, op) ? SHAPES[op as ChromeCommand['op']] : undefined;
  const fields = Object.keys(record).filter((key) => key !== 'op');
  const matches =
    shape !== undefined &&
    fields.length === Object.keys(shape).length &&
    Object.entries(shape).every(([key, check]) => Object.hasOwn(record, key) && check(record[key]));
  if (!matches)
    throw new CommandError('chrome.unsupported', 'Aura 发来的命令这个扩展版本不认识；请更新扩展。');
  return record as unknown as ChromeCommand;
}

/** Carries out `command`; `origin` is the site Core authorized it for. */
export async function run(
  context: CommandContext,
  command: ChromeCommand,
  origin: string | undefined,
): Promise<unknown> {
  if (origin !== undefined && 'tab_id' in command) {
    const tab = await context.tabs.get(command.tab_id);
    const now = originOf(tab.url ?? '');
    if (now !== origin)
      throw refused(
        `页面已经不在批准这一步时的 ${origin}（现在是 ${now}），这一步没有执行；请先用 chrome.snapshot 查看。`,
      );
  }
  switch (command.op) {
    case 'open':
      return open(context, command.url);
    case 'navigate':
      return navigate(context, command.tab_id, command.url, command.action);
    case 'snapshot':
      return report(context, command.tab_id, command.max_chars);
    case 'screenshot':
      return screenshot(context, command.tab_id, command.element, command.full_page);
    case 'inspect': {
      const page = await context.pages.page(command.tab_id);
      const tab = await context.tabs.get(command.tab_id);
      const point =
        command.x !== null && command.y !== null ? { x: command.x, y: command.y } : null;
      return inspect(page, { ref: command.element, point }, command.intent, tab.url ?? '');
    }
    case 'click':
    case 'type':
    case 'press':
    case 'scroll':
    case 'hover':
    case 'select':
      return act(context, command);
    case 'wait':
      return wait(
        context,
        command.tab_id,
        command.text,
        command.text_gone,
        command.time_ms,
        command.timeout_ms,
      );
    case 'console':
      return consoleOf(context, command.tab_id, command.level, command.max_entries);
    case 'close':
      return close(context, command.tab_id);
    case 'assist':
      return assist(context, command.tab_id, command.reason, command.timeout_seconds);
    case 'release':
      for (const tabId of command.tab_ids) await context.pages.forget(tabId);
      return {};
  }
}
