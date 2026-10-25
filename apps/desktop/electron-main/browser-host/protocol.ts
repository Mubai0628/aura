/**
 * The `browser.command.v1` notification from Core, accepted only in its exact shape, and the
 * result Main sends back on the Core-only route `browser.host.result`; `browser.cancel.v1`
 * withdraws a command Core no longer waits for.
 */
import type {
  BrowserHostActionV1,
  BrowserHostCancelV1,
  BrowserHostCommandV1,
  BrowserHostResultV1,
} from '@aura/contracts/generated';

type Fields = Record<string, unknown>;
const isRecord = (value: unknown): value is Fields =>
  typeof value === 'object' && value !== null && !Array.isArray(value);
const isText = (value: unknown): value is string => typeof value === 'string';
const optionalText = (value: unknown) => value === undefined || value === null || isText(value);
const count = (value: unknown, max: number) =>
  typeof value === 'number' && Number.isInteger(value) && value >= 0 && value <= max;
const flag = (value: unknown) => typeof value === 'boolean';
const point = (value: unknown) =>
  value === undefined || value === null || (typeof value === 'number' && Number.isFinite(value));

const ACTIONS: Readonly<Record<string, (action: Fields) => boolean>> = {
  open: (a) => isText(a.url),
  navigate: (a) => ['back', 'forward', 'reload'].includes(String(a.action)),
  snapshot: (a) => count(a.max_chars, 100_000),
  screenshot: (a) => optionalText(a.element) && flag(a.full_page),
  click: (a) =>
    optionalText(a.element) &&
    point(a.x) &&
    point(a.y) &&
    ['left', 'right', 'middle'].includes(String(a.button)) &&
    flag(a.double),
  type: (a) => optionalText(a.element) && isText(a.text) && flag(a.clear) && flag(a.submit),
  press: (a) => isText(a.key) && count(a.repeat, 20) && count(a.hold_ms, 10_000),
  scroll: (a) =>
    optionalText(a.element) &&
    ['up', 'down', 'left', 'right'].includes(String(a.direction)) &&
    count(a.amount, 100_000),
  hover: (a) => isText(a.element),
  select: (a) => isText(a.element) && Array.isArray(a.values) && a.values.every(isText),
  wait: (a) =>
    optionalText(a.text) &&
    optionalText(a.text_gone) &&
    (a.time_ms === undefined || a.time_ms === null || count(a.time_ms, 60_000)) &&
    count(a.timeout_ms, 60_000),
  console: (a) =>
    ['error', 'warning', 'all'].includes(String(a.level)) && count(a.max_entries, 1_000),
  close: () => true,
  request_user: (a) => isText(a.reason) && a.reason.length <= 1_000 && count(a.timeout_ms, 600_000),
  evaluate: (a) => isText(a.expression) && a.expression.length <= 20_000,
  network: (a) => flag(a.failed_only) && count(a.max_entries, 1_000),
  // Read through DevTools.
  inspect: (a) => isText(a.element) && a.element.length <= 64,
  performance: () => true,
};

/** The command, or undefined when the notification is not one Main can carry out. */
export function readCommand(value: unknown, runtimeId: string): BrowserHostCommandV1 | undefined {
  if (!isRecord(value) || !isRecord(value.access) || !isRecord(value.action)) return undefined;
  const {
    command_id: id,
    runtime_instance_id: runtime,
    session_id: session,
    access,
    action,
  } = value;
  const check = ACTIONS[String(action.kind)];
  if (
    !isText(id) ||
    !id.startsWith('bcm-') ||
    runtime !== runtimeId ||
    !isText(session) ||
    !optionalText(value.expected_host) ||
    !count(value.timeout_ms, 600_000) ||
    !flag(access.read_only) ||
    !Array.isArray(access.approved_hosts) ||
    !access.approved_hosts.every(isText) ||
    !optionalText(access.file_root) ||
    check === undefined ||
    !check(action)
  )
    return undefined;
  return value as unknown as BrowserHostCommandV1;
}

/** The withdrawal, or undefined when the notification is not one for this runtime. */
export function readCancel(value: unknown, runtimeId: string): BrowserHostCancelV1 | undefined {
  if (!isRecord(value)) return undefined;
  const { command_id: id, runtime_instance_id: runtime } = value;
  return isText(id) &&
    id.startsWith('bcm-') &&
    runtime === runtimeId &&
    Object.keys(value).length === 2
    ? { command_id: id, runtime_instance_id: runtime }
    : undefined;
}

/** The model's own input on the page (a script in it counts): refused while the user has it. */
export const isInput = (action: BrowserHostActionV1): boolean =>
  ['click', 'type', 'press', 'scroll', 'hover', 'select', 'evaluate'].includes(action.kind);

export const refused = (
  command: BrowserHostCommandV1,
  pageUrl: string,
  message: string,
): BrowserHostResultV1 => ({
  command_id: command.command_id,
  page_url: pageUrl,
  outcome: { outcome: 'refused', message },
});
