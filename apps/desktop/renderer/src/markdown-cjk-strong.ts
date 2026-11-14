/**
 * CommonMark lets `**` close only where it is not preceded by punctuation, unless whitespace or
 * punctuation follows it. So `**慢速工具：**已请求`, a label that ends in Chinese punctuation with
 * the sentence written right after it, as models often write, kept its asterisks as text. Here a
 * `**` that Markdown left as text and that touches Chinese text pairs with the next one in the same
 * paragraph as it would with a space beside it: an opening `**` is not followed by whitespace, a
 * closing one is not preceded by it, and what lies between them becomes bold.
 */

interface MarkdownNode {
  type: string;
  value?: string;
  children?: MarkdownNode[];
}

/** Chinese, Japanese and Korean text and punctuation, full-width forms and typographic quotes. */
const CJK =
  /[\u2010-\u2027\u2030-\u205e\u2e80-\u2fff\u3000-\u9fff\uac00-\ud7af\uf900-\ufaff\ufe10-\ufe19\ufe30-\ufe6f\uff00-\uffef]/u;

/** One `**` left as text: the child it is in and where in that child's text it starts. */
interface Mark {
  child: number;
  at: number;
  opens: boolean;
  closes: boolean;
  cjk: boolean;
}

export function remarkCjkStrong() {
  return (tree: MarkdownNode) => {
    visit(tree);
  };
}

function visit(node: MarkdownNode): void {
  if (!node.children) return;
  for (const child of node.children) visit(child);
  pair(node);
}

/** The `**` runs of exactly two asterisks in `parent`'s text children, in order. */
function marks(parent: MarkdownNode): Mark[] {
  const children = parent.children ?? [];
  const found: Mark[] = [];
  children.forEach((child, index) => {
    if (child.type !== 'text' || !child.value) return;
    const value = child.value;
    for (const match of value.matchAll(/(?<!\*)\*\*(?!\*)/gu)) {
      const at = match.index;
      const before = at > 0 ? value[at - 1] : edge(children[index - 1], 'end');
      const after = at + 2 < value.length ? value[at + 2] : edge(children[index + 1], 'start');
      found.push({
        child: index,
        at,
        opens: after !== undefined && !/\s/u.test(after),
        closes: before !== undefined && !/\s/u.test(before),
        cjk: (before !== undefined && CJK.test(before)) || (after !== undefined && CJK.test(after)),
      });
    }
  });
  return found;
}

/** The character a neighbouring node shows at its `side`; a node without text counts as a word. */
function edge(node: MarkdownNode | undefined, side: 'start' | 'end'): string | undefined {
  if (!node) return undefined;
  if (node.type === 'text' && node.value) {
    return side === 'start' ? node.value[0] : node.value[node.value.length - 1];
  }
  return 'x';
}

function pair(parent: MarkdownNode): void {
  const found = marks(parent);
  if (found.length < 2) return;
  const pairs: Array<[Mark, Mark]> = [];
  let open: Mark | undefined;
  for (const mark of found) {
    const empty = open !== undefined && open.child === mark.child && open.at + 2 === mark.at;
    if (open && mark.closes && !empty && (open.cjk || mark.cjk)) {
      pairs.push([open, mark]);
      open = undefined;
    } else if (mark.opens) {
      open = mark;
    }
  }
  // From the last pair back, so the places of the earlier ones stay where they were.
  for (const [start, end] of pairs.reverse()) wrap(parent, start, end);
}

function wrap(parent: MarkdownNode, start: Mark, end: Mark): void {
  const children = parent.children ?? [];
  const first = children[start.child]?.value ?? '';
  const last = children[end.child]?.value ?? '';
  const text = (value: string): MarkdownNode[] => (value ? [{ type: 'text', value }] : []);
  const inner =
    start.child === end.child
      ? text(first.slice(start.at + 2, end.at))
      : [
          ...text(first.slice(start.at + 2)),
          ...children.slice(start.child + 1, end.child),
          ...text(last.slice(0, end.at)),
        ];
  children.splice(
    start.child,
    end.child - start.child + 1,
    ...text(first.slice(0, start.at)),
    { type: 'strong', children: inner },
    ...text(last.slice(end.at + 2)),
  );
}
