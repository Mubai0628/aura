import type { PageDebugger } from './cdp.js';

/** Media elements read for one snapshot, at most. */
const MOST = 40;
const ADDRESS_CHARS = 400;

export interface PageMedia {
  kind: 'video' | 'audio';
  url: string;
}

interface DescribedNode {
  nodeName: string;
  attributes?: string[];
}

const attribute = (node: DescribedNode, name: string): string | undefined => {
  const values = node.attributes ?? [];
  for (let index = 0; index + 1 < values.length; index += 2)
    if (values[index]?.toLowerCase() === name) return values[index + 1];
  return undefined;
};

/**
 * The addresses a page's video and audio elements, and their `<source>` children, name. Main reads
 * the page's elements the way it reads their boxes, through DevTools' DOM, and runs nothing on the
 * page: running a script there is the user's debugging switch alone.
 */
export async function pageMedia(
  devtools: Pick<PageDebugger, 'send'>,
  pageUrl: string,
): Promise<PageMedia[]> {
  try {
    const { root } = await devtools.send<{ root: { nodeId: number } }>('DOM.getDocument', {
      depth: 0,
    });
    const { nodeIds = [] } = await devtools.send<{ nodeIds?: number[] }>('DOM.querySelectorAll', {
      nodeId: root.nodeId,
      selector: 'video, audio, video > source, audio > source',
    });
    const out: PageMedia[] = [];
    const seen = new Set<string>();
    for (const nodeId of nodeIds.slice(0, MOST)) {
      const { node } = await devtools.send<{ node: DescribedNode }>('DOM.describeNode', {
        nodeId,
      });
      const name = node.nodeName.toLowerCase();
      const type = attribute(node, 'type') ?? '';
      const kind =
        name === 'audio' || (name === 'source' && type.startsWith('audio/')) ? 'audio' : 'video';
      const src = attribute(node, 'src');
      if (!src) continue;
      let url: string;
      try {
        url = new URL(src, pageUrl).toString();
      } catch {
        continue;
      }
      if (seen.has(url)) continue;
      seen.add(url);
      out.push({ kind, url });
    }
    return out;
  } catch {
    // A page whose elements cannot be read this way still has its snapshot.
    return [];
  }
}

/**
 * The media lines that follow the
 * snapshot. A stream (`blob:`) has no address to download; its parts are among the page's
 * requests (browser.network).
 */
export function mediaSection(items: readonly PageMedia[]): string {
  const lines = items
    .filter((item) => !item.url.startsWith('data:'))
    .map((item) => {
      const url =
        item.url.length > ADDRESS_CHARS ? `${item.url.slice(0, ADDRESS_CHARS)}…` : item.url;
      const stream = item.url.startsWith('blob:')
        ? ' (a stream: its parts are in browser.network)'
        : '';
      return `- ${item.kind} ${url}${stream}`;
    });
  return lines.length === 0 ? '' : `\nmedia:\n${lines.join('\n')}`;
}
