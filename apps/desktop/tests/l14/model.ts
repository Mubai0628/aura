import type { ProviderAction, ProviderRequest } from '../s06-file-write-provider.mjs';
import { throughEntry } from '../tool-entry';

/** The page snapshot in the latest tool result. */
export function snapshotOf(request: ProviderRequest): string {
  const message = request.messages
    .filter((value) => (value as { role?: string }).role === 'tool')
    .at(-1) as { content: string } | undefined;
  if (message === undefined) throw new Error('the previous step left no tool result');
  let found: string | undefined;
  const visit = (value: unknown): void => {
    if (found !== undefined || value === null || typeof value !== 'object') return;
    for (const [key, inner] of Object.entries(value)) {
      if (key === 'snapshot' && typeof inner === 'string') {
        found = inner;
        return;
      }
      visit(inner);
    }
  };
  visit(JSON.parse(message.content) as unknown);
  if (found === undefined)
    throw new Error(`no page in the result: ${message.content.slice(0, 600)}`);
  return found;
}

/** The ref of the element with this role and accessible name. */
export function refOf(snapshot: string, role: string, name: string): string {
  const line = snapshot
    .split('\n')
    .find((text) => text.includes(`${role} ${JSON.stringify(name)}`) && text.includes('[ref='));
  const ref = line === undefined ? undefined : /\[ref=(e\d+)\]/u.exec(line)?.[1];
  if (ref === undefined) throw new Error(`no ${role} ${JSON.stringify(name)} in:\n${snapshot}`);
  return ref;
}

const call = (tool: string, args: Record<string, unknown>): ProviderAction =>
  throughEntry({ tool, arguments: args });

/**
 * One turn in a shared tab: open the first site, click and type there, go to the second site and
 * press its delete button, then answer. Every call goes through the unified entry, so it does not
 * matter which Chrome tools the request lists.
 */
export function chromeScript(sites: { a: string; b: string }, typed: string) {
  return (request: ProviderRequest): ProviderAction => {
    switch (request.step) {
      case 0:
        return call('chrome.open', { url: sites.a });
      case 1:
        return call('chrome.click', { ref: refOf(snapshotOf(request), 'button', '加一') });
      case 2:
        return call('chrome.type', {
          ref: refOf(snapshotOf(request), 'textbox', '名字'),
          text: typed,
        });
      case 3:
        return call('chrome.navigate', { url: sites.b });
      case 4:
        return call('chrome.click', { ref: refOf(snapshotOf(request), 'button', '删除全部记录') });
      default:
        return { tool: null, arguments: {} };
    }
  };
}
