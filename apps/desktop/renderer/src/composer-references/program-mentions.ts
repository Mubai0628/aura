import { quoted } from './program-tokens';

/** A sent message's words, or a program chosen with "@": its name and its words as written. */
export type ProgramPiece = string | { name: string; text: string };

/** The words cut at each `@"name"`, by the composer's own rule for a program block. */
export function programPieces(value: string): ProgramPiece[] {
  const out: ProgramPiece[] = [];
  let at = 0;
  for (const mention of quoted(value)) {
    if (mention.start > at) out.push(value.slice(at, mention.start));
    out.push({ name: mention.name, text: value.slice(mention.start, mention.end) });
    at = mention.end;
  }
  if (at < value.length) out.push(value.slice(at));
  return out;
}

interface HtmlNode {
  type: string;
  tagName?: string;
  value?: string;
  properties?: Record<string, unknown>;
  children?: HtmlNode[];
}

/** Code and links keep their words. */
const KEPT = new Set(['a', 'code', 'pre']);

function mark(node: HtmlNode): void {
  const children = node.children;
  if (!children) return;
  for (let index = 0; index < children.length; index += 1) {
    const child = children[index]!;
    if (child.type === 'element') {
      if (!KEPT.has(child.tagName ?? '')) mark(child);
      continue;
    }
    if (child.type !== 'text' || !child.value) continue;
    const pieces = programPieces(child.value);
    if (pieces.every((piece) => typeof piece === 'string')) continue;
    const replacement = pieces.map((piece): HtmlNode =>
      typeof piece === 'string'
        ? { type: 'text', value: piece }
        : {
            type: 'element',
            tagName: 'span',
            properties: { dataProgram: piece.name },
            children: [{ type: 'text', value: piece.text }],
          },
    );
    children.splice(index, 1, ...replacement);
    index += replacement.length - 1;
  }
}

/**
 * The rehype pass for a person's own words, after the sanitizer: each `@"name"` outside code and
 * links becomes a `span` naming the program, which the message draws as a block once the program
 * is known. The pass adds these marks and nothing else.
 */
export function rehypeProgramMentions() {
  return (tree: HtmlNode) => mark(tree);
}
