/**
 * A page as the model reads it — Chromium's accessibility tree, compacted to one line per
 * meaningful node, with a stable ref (e1, e2…) on every element the model can act on.
 *
 * A ref names a DOM node for as long as that node exists; the same node keeps its ref from one
 * snapshot to the next, so the model can act on what it read without reading again.
 */
export interface AxValue {
  value?: unknown;
}
export interface AxNode {
  nodeId: string;
  ignored?: boolean;
  role?: AxValue;
  name?: AxValue;
  value?: AxValue;
  properties?: { name: string; value: AxValue }[];
  childIds?: string[];
  parentId?: string;
  backendDOMNodeId?: number;
}
export interface Box {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** The model's names for DOM nodes on one page; cleared when the page navigates. */
export class RefTable {
  #byNode = new Map<number, string>();
  #byRef = new Map<string, number>();
  #next = 1;

  refFor(backendNodeId: number): string {
    let ref = this.#byNode.get(backendNodeId);
    if (ref === undefined) {
      ref = `e${this.#next++}`;
      this.#byNode.set(backendNodeId, ref);
      this.#byRef.set(ref, backendNodeId);
    }
    return ref;
  }

  node(ref: string): number | undefined {
    return this.#byRef.get(ref);
  }

  /** Forgets every element; numbering goes on, so an old ref never names a new element. */
  clear(): void {
    this.#byNode.clear();
    this.#byRef.clear();
  }
}

const ACTIONABLE = new Set([
  'button',
  'link',
  'textbox',
  'searchbox',
  'checkbox',
  'radio',
  'combobox',
  'listbox',
  'option',
  'menuitem',
  'menuitemcheckbox',
  'menuitemradio',
  'tab',
  'switch',
  'slider',
  'spinbutton',
  'treeitem',
  'gridcell',
  'cell',
  'row',
  'Canvas',
  'img',
  'image',
  'video',
  'Video',
  'Audio',
  'audio',
]);
/** Wrappers that say nothing themselves: their children are shown in their place. */
const TRANSPARENT = new Set([
  'generic',
  'none',
  'presentation',
  'RootWebArea',
  'WebArea',
  'LayoutTable',
  'LayoutTableRow',
  'LayoutTableCell',
  'Section',
  'Div',
  'paragraph',
  // A label's text is the field's name already; a select's popup holds its options.
  'LabelText',
  'MenuListPopup',
]);
const SKIPPED = new Set(['InlineTextBox', 'LineBreak']);
const SHOWN_PROPERTIES = [
  'checked',
  'pressed',
  'selected',
  'expanded',
  'disabled',
  'required',
  'level',
  'focused',
];
/** Boxes are given for what has no text to go by. */
export const BOXED = new Set(['Canvas', 'img', 'image', 'video', 'Video']);

const text = (value: AxValue | undefined): string =>
  value?.value === undefined || value.value === null
    ? ''
    : String(value.value).replace(/\s+/gu, ' ').trim();
const quoted = (value: string) =>
  JSON.stringify(value.length > 200 ? `${value.slice(0, 200)}…` : value);

function properties(node: AxNode, role: string): string[] {
  const shown: string[] = [];
  for (const property of node.properties ?? []) {
    if (!SHOWN_PROPERTIES.includes(property.name)) continue;
    const value = property.value.value;
    if (value === false || value === undefined || value === null || value === 'false') continue;
    if (property.name === 'level' && !role.startsWith('heading')) continue;
    shown.push(
      value === true || value === 'true' ? property.name : `${property.name}=${String(value)}`,
    );
  }
  return shown;
}

function secret(node: AxNode): boolean {
  return (node.properties ?? []).some(
    (property) =>
      property.name === 'autocomplete' &&
      /password|cc-|one-time-code/u.test(String(property.value.value)),
  );
}

/**
 * The snapshot of `nodes` (from `Accessibility.getFullAXTree`), at most `maxChars` long.
 * `boxes` gives a position for canvases and images (page coordinates), when known.
 */
export function renderSnapshot(
  nodes: readonly AxNode[],
  refs: RefTable,
  maxChars: number,
  boxes: ReadonlyMap<number, Box> = new Map(),
): { text: string; truncated: boolean } {
  const byId = new Map(nodes.map((node) => [node.nodeId, node]));
  const root = nodes.find((node) => node.parentId === undefined) ?? nodes[0];
  const lines: string[] = [];
  let length = 0;
  let truncated = false;
  const emit = (depth: number, line: string): boolean => {
    const full = `${'  '.repeat(depth)}- ${line}`;
    if (length + full.length + 1 > maxChars) {
      truncated = true;
      return false;
    }
    lines.push(full);
    length += full.length + 1;
    return true;
  };
  const visit = (node: AxNode | undefined, depth: number, parentName: string): boolean => {
    if (node === undefined) return true;
    const role = text(node.role);
    const name = text(node.name);
    const children = node.childIds ?? [];
    if (SKIPPED.has(role)) return true;
    if (node.ignored || TRANSPARENT.has(role)) {
      for (const child of children) if (!visit(byId.get(child), depth, parentName)) return false;
      return true;
    }
    if (role === 'StaticText') {
      if (name === '' || name === parentName) return true;
      return emit(depth, `text: ${quoted(name)}`);
    }
    const parts = [role];
    if (name !== '') parts.push(quoted(name));
    const value = text(node.value);
    if (value !== '' && value !== name)
      parts.push(`value=${quoted(secret(node) ? '••••' : value)}`);
    parts.push(...properties(node, role).map((property) => `[${property}]`));
    const id = node.backendDOMNodeId;
    if (id !== undefined && ACTIONABLE.has(role)) parts.push(`[ref=${refs.refFor(id)}]`);
    const box = id === undefined ? undefined : boxes.get(id);
    if (box !== undefined && BOXED.has(role))
      parts.push(
        `(at ${Math.round(box.x)},${Math.round(box.y)} size ${Math.round(box.width)}x${Math.round(box.height)})`,
      );
    if (!emit(depth, parts.join(' '))) return false;
    for (const child of children) if (!visit(byId.get(child), depth + 1, name)) return false;
    return true;
  };
  visit(root, 0, '');
  return { text: lines.join('\n'), truncated };
}
