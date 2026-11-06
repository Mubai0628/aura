import type { ProviderAction, ProviderRequest } from '../s06-file-write-provider.mjs';
import { throughEntry } from '../tool-entry';

/** The latest tool result of the request, as the model read it. */
export function lastResult(request: ProviderRequest): string {
  const message = request.messages
    .filter((value) => (value as { role?: string }).role === 'tool')
    .at(-1) as { content: string } | undefined;
  if (message === undefined) throw new Error('the previous step left no tool result');
  return message.content;
}

/** The first value named `name` anywhere in a tool result. */
export function field(content: string, name: string): unknown {
  let found: unknown;
  const visit = (value: unknown): void => {
    if (found !== undefined || value === null || typeof value !== 'object') return;
    for (const [key, inner] of Object.entries(value)) {
      if (key === name) {
        found = inner;
        return;
      }
      visit(inner);
    }
  };
  visit(JSON.parse(content) as unknown);
  return found;
}

/** The element listing of the latest read (`computer.snapshot`). */
export function elementsOf(request: ProviderRequest): string {
  const content = lastResult(request);
  const found = field(content, 'elements');
  if (typeof found !== 'string')
    throw new Error(`no elements in the result: ${content.slice(0, 600)}`);
  return found;
}

/** The ref of the element with this role and name, from a line such as `e12 button "确定"`. */
export function refOf(elements: string, role: string, name: string): string {
  const wanted = `${role} ${JSON.stringify(name)}`;
  const line = elements.split('\n').find((text) => text.includes(wanted));
  const ref = line === undefined ? undefined : /^\s*(e\d+) /u.exec(line)?.[1];
  if (ref === undefined) throw new Error(`no ${wanted} in:\n${elements}`);
  return ref;
}

const call = (tool: string, args: Record<string, unknown>): ProviderAction =>
  throughEntry({ tool, arguments: args });

/**
 * The first turn on the test window: open it (it is running already), read it, click 确定, take
 * a screenshot with numbered boxes, minimize and restore it, then click 卡住, which keeps the
 * window from answering until the user stops the call; then answer. The refs come from the read.
 * Every later turn reads the window once and answers.
 */
export function computerScript(app: string) {
  let refs: { ok: string; hang: string } | undefined;
  let first: string | undefined;
  return (request: ProviderRequest): ProviderAction => {
    first ??= request.turn;
    if (request.turn !== first)
      return request.step === 0
        ? call('computer.snapshot', { app })
        : { tool: null, arguments: {} };
    switch (request.step) {
      case 0:
        return call('computer.open', { app });
      case 1:
        return call('computer.snapshot', { app });
      case 2: {
        const elements = elementsOf(request);
        refs = { ok: refOf(elements, 'button', '确定'), hang: refOf(elements, 'button', '卡住') };
        return call('computer.act', { app, steps: [{ action: 'click', ref: refs.ok }] });
      }
      case 3:
        return call('computer.screenshot', { app, marks: true });
      case 4:
        return call('computer.window', { app, action: 'minimize' });
      case 5:
        return call('computer.window', { app, action: 'restore' });
      case 6:
        if (refs === undefined) throw new Error('the read came first');
        return call('computer.act', { app, steps: [{ action: 'click', ref: refs.hang }] });
      default:
        return { tool: null, arguments: {} };
    }
  };
}
