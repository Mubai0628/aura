/** The part of a Markdown tree this pass reads and writes. */
interface MarkdownNode {
  type: string;
  value?: string;
  children?: MarkdownNode[];
}

const LINE_END = /\r\n|\r|\n/u;

/**
 * A person's own words keep every line they typed. A person who presses Enter once means a new
 * line, where Markdown joins the two lines with a space; each line break inside a paragraph becomes
 * one that shows. A blank line still starts a paragraph, and code keeps its own lines.
 */
export function remarkTypedLineBreaks() {
  return (tree: MarkdownNode) => split(tree);
}

function split(node: MarkdownNode): void {
  const children = node.children;
  if (!children) return;
  for (let index = 0; index < children.length; index += 1) {
    const child = children[index]!;
    if (child.type !== 'text' || !LINE_END.test(child.value ?? '')) {
      split(child);
      continue;
    }
    const replacement: MarkdownNode[] = [];
    child.value!.split(LINE_END).forEach((line, at) => {
      if (at > 0) replacement.push({ type: 'break' });
      if (line) replacement.push({ type: 'text', value: line });
    });
    children.splice(index, 1, ...replacement);
    index += replacement.length - 1;
  }
}
