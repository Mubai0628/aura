/** The part of a Markdown tree this pass reads and writes. */
interface MarkdownNode {
  type: string;
  value?: string;
  url?: string;
  children?: MarkdownNode[];
  position?: { start: { offset?: number }; end: { offset?: number } };
}

/** The source the tree was parsed from, as the processor hands it to its passes. */
interface SourceFile {
  value?: unknown;
}

/** A path: in `source[from..to]`; `share` when it begins with `\\` (`\\server\share`). */
interface Path {
  from: number;
  to: number;
  share: boolean;
}

/** CommonMark's escapable characters: the ASCII punctuation. */
const ESCAPABLE = /[!-/:-@[-`{-~]/u;
/** Where a path stops: a space or a line break, a quote or a pipe, or Chinese punctuation. */
const PATH_END =
  /[\s<>"|`\u3000-\u303f\uff01-\uff0f\uff1a-\uff20\uff3b-\uff40\uff5b-\uff65\u2018\u2019\u201c\u201d]/u;
/** A drive (`C:\`) or a share (`\\server\`), not inside a longer word or after a backslash. */
const PATH_START = /(?<![\p{L}\p{N}_\\])(?:[a-z]:\\|\\\\(?=[\p{L}\p{N}_.-]+\\))/giu;

/**
 * Windows paths in the model's answer keep their backslashes. Markdown reads a backslash before
 * ASCII punctuation as an escape and drops it, so `C:\Work\app\.cache` showed as
 * `C:\Work\app.cache`, and a link to it lost a folder. Inside a path, from its drive or share
 * to the first space, quote or Chinese punctuation, every backslash shows; a doubled one is still
 * one, as the model escaped it. Outside paths, the model's escapes work as before.
 *
 * Only text and addresses whose source this pass can read back exactly are changed: one with a
 * character reference (`&amp;`), or any the parser changed otherwise, is left as it was.
 */
export function remarkWindowsPaths() {
  return (tree: MarkdownNode, file?: SourceFile) => {
    const source = typeof file?.value === 'string' ? file.value : undefined;
    if (source?.includes('\\')) visit(tree, source);
  };
}

function visit(node: MarkdownNode, source: string): void {
  if (node.type === 'text') {
    const written = writtenText(node, source);
    const restored = written === undefined ? undefined : restore(written, node.value);
    if (restored !== undefined) node.value = restored;
  } else if (node.type === 'link' || node.type === 'image') {
    const written = writtenText(node, source);
    const address = written === undefined ? undefined : writtenAddress(written);
    const restored = address === undefined ? undefined : restore(address, node.url);
    if (restored !== undefined) node.url = restored;
  }
  for (const child of node.children ?? []) visit(child, source);
}

function writtenText(node: MarkdownNode, source: string): string | undefined {
  const from = node.position?.start.offset;
  const to = node.position?.end.offset;
  return from === undefined || to === undefined ? undefined : source.slice(from, to);
}

/** `written` with every backslash of its paths, when Markdown read it as `parsed`. */
function restore(written: string, parsed: string | undefined): string | undefined {
  if (!written.includes('\\') || written.includes('&')) return undefined;
  const found = paths(written);
  if (!found.length || read(written, []) !== parsed) return undefined;
  const restored = read(written, found);
  return restored === parsed ? undefined : restored;
}

/** The paths in `written`. A path goes on across a space when the word after it goes on with a
 *  backslash (`C:\Program Files\…`). */
function paths(written: string): Path[] {
  const found: Path[] = [];
  for (const match of written.matchAll(PATH_START)) {
    const from = match.index;
    if (found.length && from < found.at(-1)!.to) continue;
    let to = from + match[0].length;
    for (;;) {
      while (to < written.length && !PATH_END.test(written[to]!)) to += 1;
      if (written[to] !== ' ') break;
      let next = to + 1;
      while (next < written.length && !PATH_END.test(written[next]!)) next += 1;
      if (next === to + 1 || !written.slice(to + 1, next).includes('\\')) break;
      to = next;
    }
    found.push({ from, to, share: match[0].startsWith('\\') });
  }
  return found;
}

/**
 * `written` as Markdown reads it: a backslash before ASCII punctuation escapes it, and a line
 * break drops the spaces and tabs around it. Inside `kept`, a backslash before anything but
 * another backslash shows, and a share's own two backslashes show.
 */
function read(written: string, kept: Path[]): string {
  let shown = '';
  let path = 0;
  for (let at = 0; at < written.length; at += 1) {
    while (path < kept.length && kept[path]!.to <= at) path += 1;
    const inside = path < kept.length && kept[path]!.from <= at;
    const char = written[at]!;
    const next = written[at + 1];
    if (inside && kept[path]!.share && at === kept[path]!.from) {
      shown += '\\\\';
      at += 1;
    } else if (char === '\\' && next !== undefined && ESCAPABLE.test(next)) {
      if (inside && next !== '\\') {
        shown += char;
      } else {
        shown += next;
        at += 1;
      }
    } else if (char === '\n' || char === '\r') {
      shown = shown.replace(/[ \t]+$/u, '') + char;
      while (written[at + 1] === ' ' || written[at + 1] === '\t') at += 1;
    } else {
      shown += char;
    }
  }
  return shown;
}

/** The address a link or picture was written with: `[words](address "title")`, as written. */
function writtenAddress(written: string): string | undefined {
  let at = written.startsWith('!') ? 1 : 0;
  if (written[at] !== '[') return undefined;
  for (let depth = 0; at < written.length; at += 1) {
    const char = written[at];
    if (char === '\\') at += 1;
    else if (char === '[') depth += 1;
    else if (char === ']' && --depth === 0) break;
  }
  if (written[at + 1] !== '(') return undefined;
  const rest = written.slice(at + 2).replace(/^\s+/u, '');
  if (rest.startsWith('<')) {
    const end = rest.indexOf('>');
    return end > 1 ? rest.slice(1, end) : undefined;
  }
  let end = 0;
  for (let depth = 0; end < rest.length; end += 1) {
    const char = rest[end]!;
    if (char === '\\') end += 1;
    else if (/\s/u.test(char)) break;
    else if (char === '(') depth += 1;
    else if (char === ')' && depth-- === 0) break;
  }
  return end ? rest.slice(0, end) : undefined;
}
