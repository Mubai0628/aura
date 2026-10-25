/**
 * GPT models name the files they
 * wrote as Markdown links to local paths — `[game.html](game.html)`, `[game.html](C:\…\game.html)`,
 * `[game.html](file:///C:/…/game.html)`. The renderer's link guard rightly refuses such an address
 * as a link and used to print "game.html [blocked]". A link (or picture) to a local path becomes
 * the path as inline code instead, which the conversation already turns into a file link when the
 * file is in its workspace; its own words stay in front of it when they are not the file's name.
 */

interface MarkdownNode {
  type: string;
  url?: string;
  alt?: string | null;
  value?: string;
  children?: MarkdownNode[];
}

/** The local path a link's address names, with `/` between parts; `undefined` for any other. */
export function localTarget(address: string): string | undefined {
  let value = address.trim();
  if (!value || /^[#?]/u.test(value) || value.startsWith('//')) return undefined;
  try {
    value = decodeURI(value);
  } catch {
    // An address that is not valid percent-encoding is taken as written.
  }
  const file = /^file:(?:\/\/(?:localhost)?)?(\/.*)$/iu.exec(value);
  if (file) {
    value = /^\/[a-z]:[\\/]/iu.test(file[1]!) ? file[1]!.slice(1) : file[1]!;
  } else if (/^[a-z][a-z0-9+.-]*:/iu.test(value) && !/^[a-z]:[\\/]/iu.test(value)) {
    return undefined;
  }
  value = value.replaceAll('\\', '/');
  return value || undefined;
}

function plainText(node: MarkdownNode): string {
  if (typeof node.value === 'string') return node.value;
  return (node.children ?? []).map(plainText).join('');
}

/** Whether a link's words are just the file it names, so the path alone says everything. */
function namesTheFile(words: string, target: string): boolean {
  const spelled = words.replaceAll('\\', '/');
  const name = target
    .replace(/(?:#L\d+|:\d+(?::\d+)?)$/u, '')
    .split('/')
    .at(-1);
  return !spelled || spelled === target || spelled === name;
}

function rewrite(node: MarkdownNode): void {
  const children = node.children;
  if (!children) return;
  for (let index = 0; index < children.length; index += 1) {
    const child = children[index]!;
    const linked = child.type === 'link' || child.type === 'image';
    const target = linked && child.url ? localTarget(child.url) : undefined;
    if (target === undefined) {
      rewrite(child);
      continue;
    }
    const words = (child.type === 'image' ? (child.alt ?? '') : plainText(child)).trim();
    const code: MarkdownNode = { type: 'inlineCode', value: target };
    const replacement = namesTheFile(words, target)
      ? [code]
      : [{ type: 'text', value: `${words}（` }, code, { type: 'text', value: '）' }];
    children.splice(index, 1, ...replacement);
    index += replacement.length - 1;
  }
}

/** The remark plugin: run on the parsed Markdown, before the link guard sees any address. */
export function remarkLocalFileLinks() {
  return (tree: MarkdownNode) => rewrite(tree);
}
