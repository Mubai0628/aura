/**
 * GFM finds an address in
 * the text by reading on to the next space, so Chinese punctuation and words written right after
 * it became part of it, and an address written right after Chinese text was not found at all.
 * Here an address ends where Chinese punctuation, or Chinese text after its host, begins; one
 * written after Chinese text is found too; an email address stays text; `www.` opens as https.
 */

interface MarkdownNode {
  type: string;
  value?: string;
  url?: string;
  title?: string | null;
  children?: MarkdownNode[];
}

/** Chinese and full-width punctuation, and the typographic quotes, dashes and ellipsis. */
const CJK_PUNCTUATION = new RegExp(
  `[${[
    [0x2010, 0x2027],
    [0x2030, 0x205e],
    [0x3000, 0x303f],
    [0xfe10, 0xfe19],
    [0xfe30, 0xfe6f],
    [0xff00, 0xff65],
  ]
    .map(([from, to]) => `${String.fromCodePoint(from!)}-${String.fromCodePoint(to!)}`)
    .join('')}]`,
  'u',
);
/** An address in plain text: not inside a word, an email or a path written in ASCII. */
const IN_TEXT = /(?<![A-Za-z0-9_@./:\\-])(?:https?:\/\/|www\.)[^\s<>"'`]+/giu;

/** The address an address-like piece of text really is, or nothing. */
export function trimAddress(raw: string): string {
  const stop = raw.search(CJK_PUNCTUATION);
  let text = stop < 0 ? raw : raw.slice(0, stop);
  // A host written right before Chinese text ends at its ASCII top-level domain; a host written
  // in Chinese (an internationalized domain) is left alone.
  const scheme = /^https?:\/\//iu.exec(text)?.[0].length ?? 0;
  const afterHost = text.slice(scheme).search(/[/?#]/u);
  const host = text.slice(scheme, afterHost < 0 ? undefined : scheme + afterHost);
  const ascii = /^[A-Za-z0-9.-]*\.[A-Za-z]{2,}(?::\d+)?(?=\P{ASCII})/u.exec(host);
  if (ascii) text = text.slice(0, scheme + ascii[0].length);
  // Sentence punctuation after an address is not part of it, nor an unmatched closing bracket.
  for (;;) {
    const before = text;
    text = text.replace(/[.,:;!?'"*_~]+$/u, '');
    if (text.endsWith(')') && count(text, '(') < count(text, ')')) text = text.slice(0, -1);
    if (text === before) return text;
  }
}

const count = (text: string, char: string) => text.split(char).length - 1;

/** The web address a piece of text names when that is all it is, with `www.` opened as https. */
export function textAddress(text: string): string | undefined {
  if (!/^(?:https?:\/\/|www\.)/iu.test(text) || trimAddress(text) !== text) return undefined;
  const url = /^www\./iu.test(text) ? `https://${text}` : text;
  try {
    const parsed = new URL(url);
    if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return undefined;
    // `www.` with no top-level domain after it is no address.
    if (/^www\./iu.test(text) && !/\.[a-z]{2,}$/iu.test(parsed.hostname)) return undefined;
    return url;
  } catch {
    return undefined;
  }
}

/** Whether a link's words already are its address, give or take the scheme and a last slash. */
export function sameAddress(label: string, url: string): boolean {
  let decoded = url;
  try {
    decoded = decodeURI(url);
  } catch {
    // An address that is not valid percent-encoding is compared as written.
  }
  const bare = (value: string) => value.replace(/^https?:\/\//iu, '').replace(/\/$/u, '');
  return label === url || bare(label) === bare(decoded);
}

const text = (value: string): MarkdownNode => ({ type: 'text', value });
const link = (url: string, words: string): MarkdownNode => ({
  type: 'link',
  url,
  title: null,
  children: [text(words)],
});

/** A link GFM found in the text: tidied, or turned back into text. */
function tidied(node: MarkdownNode): MarkdownNode[] | undefined {
  const only = node.children?.length === 1 ? node.children[0] : undefined;
  const words = only?.type === 'text' ? only.value : undefined;
  const url = node.url ?? '';
  if (words === undefined || !/^(?:https?|mailto):/iu.test(url)) return undefined;
  const found =
    url === words ||
    url === `http://${words}` ||
    url === `mailto:${words}` ||
    url === `https://${words}`;
  if (!found) return undefined;
  if (/^mailto:/iu.test(url)) return [text(words)];
  const kept = trimAddress(words);
  const address = textAddress(kept);
  if (!address) return [text(words)];
  if (kept === words && address === url) return undefined;
  const rest = words.slice(kept.length);
  return rest ? [link(address, kept), ...readAgain(rest)] : [link(address, kept)];
}

/**
 * What GFM read into an address after it really ends. GFM reads an address on to the next space,
 * so with no space in Chinese text it also takes in the code spans and addresses that follow
 * (an address, a Chinese comma, then code holding another address): code spans are code again, and the
 * addresses in the rest are found.
 */
function readAgain(rest: string): MarkdownNode[] {
  const nodes: MarkdownNode[] = [];
  let from = 0;
  for (const span of rest.matchAll(/`([^`\n]+)`/gu)) {
    if (span.index > from) nodes.push(...plain(rest.slice(from, span.index)));
    nodes.push({ type: 'inlineCode', value: span[1]! });
    from = span.index + span[0].length;
  }
  if (from < rest.length) nodes.push(...plain(rest.slice(from)));
  return nodes;
}

const plain = (value: string): MarkdownNode[] => linked(value) ?? [text(value)];

/** Plain text with the addresses GFM did not find (after Chinese text) made links. */
function linked(value: string): MarkdownNode[] | undefined {
  const parts: MarkdownNode[] = [];
  let from = 0;
  for (const match of value.matchAll(IN_TEXT)) {
    const kept = trimAddress(match[0]);
    const address = textAddress(kept);
    if (!address) continue;
    if (match.index > from) parts.push(text(value.slice(from, match.index)));
    parts.push(link(address, kept));
    from = match.index + kept.length;
  }
  if (!parts.length) return undefined;
  if (from < value.length) parts.push(text(value.slice(from)));
  return parts;
}

function walk(node: MarkdownNode): void {
  const children = node.children;
  if (!children) return;
  for (let index = 0; index < children.length; index += 1) {
    const child = children[index]!;
    const replaced =
      child.type === 'link'
        ? tidied(child)
        : child.type === 'text' && typeof child.value === 'string'
          ? linked(child.value)
          : undefined;
    if (replaced) {
      children.splice(index, 1, ...replaced);
      index += replaced.length - 1;
    } else if (child.type !== 'link' && child.type !== 'inlineCode' && child.type !== 'code') {
      walk(child);
    }
  }
}

/** The remark plugin: run after GFM has found its links. */
export function remarkAddresses() {
  return (tree: MarkdownNode) => walk(tree);
}
