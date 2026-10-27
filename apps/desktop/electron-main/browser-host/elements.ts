/**
 * The elements a model's refs name — where they are on the page, and whether one is a field
 * Aura never types into (passwords, payment cards, one-time codes).
 */
import type { PageDebugger } from './cdp.js';
import type { Box } from './snapshot.js';

export interface Point {
  x: number;
  y: number;
}

type Quads = { quads?: number[][] };
type Described = { node?: { nodeName?: string; attributes?: string[] } };

const stale = (devtools: PageDebugger, ref: string) =>
  `元素引用 ${ref} 已失效（元素已不在页面上）；请重新 ${devtools.snapshotTool}。`;

/** The DOM node a ref names, or what is wrong. */
export function nodeOf(devtools: PageDebugger, ref: string): number | string {
  return (
    devtools.refs.node(ref) ??
    `没有元素引用 ${ref}；请先 ${devtools.snapshotTool}，再用它给出的 ref。`
  );
}

function boxOf(quad: readonly number[]): Box {
  const xs = [quad[0] ?? 0, quad[2] ?? 0, quad[4] ?? 0, quad[6] ?? 0];
  const ys = [quad[1] ?? 0, quad[3] ?? 0, quad[5] ?? 0, quad[7] ?? 0];
  const x = Math.min(...xs);
  const y = Math.min(...ys);
  return { x, y, width: Math.max(...xs) - x, height: Math.max(...ys) - y };
}

/** Where the element is now, in page coordinates, scrolled into view first; or what is wrong. */
export async function elementBox(devtools: PageDebugger, ref: string): Promise<Box | string> {
  const node = nodeOf(devtools, ref);
  if (typeof node === 'string') return node;
  try {
    await devtools.send('DOM.scrollIntoViewIfNeeded', { backendNodeId: node });
    const { quads = [] } = await devtools.send<Quads>('DOM.getContentQuads', {
      backendNodeId: node,
    });
    const quad = quads[0];
    if (quad === undefined) return `元素 ${ref} 现在看不见（没有显示区域），没有执行。`;
    return boxOf(quad);
  } catch {
    return stale(devtools, ref);
  }
}

export async function elementPoint(devtools: PageDebugger, ref: string): Promise<Point | string> {
  const box = await elementBox(devtools, ref);
  return typeof box === 'string' ? box : { x: box.x + box.width / 2, y: box.y + box.height / 2 };
}

/** A box for each node, for the snapshot's canvases and images; nodes without one are left out. */
export async function boxesOf(
  devtools: PageDebugger,
  nodes: readonly number[],
): Promise<Map<number, Box>> {
  const boxes = new Map<number, Box>();
  for (const node of nodes.slice(0, 12)) {
    try {
      const { quads = [] } = await devtools.send<Quads>('DOM.getContentQuads', {
        backendNodeId: node,
      });
      if (quads[0] !== undefined) boxes.set(node, boxOf(quads[0]));
    } catch {
      /* off the page or gone: the snapshot simply gives no position */
    }
  }
  return boxes;
}

const SECRET_AUTOCOMPLETE =
  /(?:^|\s)(?:current-password|new-password|one-time-code|cc-[a-z-]+)(?:\s|$)/iu;
const SECRET_NAME = /passw|pwd|cvv|cvc|card.?num|card.?no|otp|验证码|密码|口令|卡号/iu;

/** Whether a field with these attributes holds a secret the user must type themselves. */
export function sensitiveField(attributes: Readonly<Record<string, string>>): boolean {
  if ((attributes.type ?? '').toLowerCase() === 'password') return true;
  if (SECRET_AUTOCOMPLETE.test(attributes.autocomplete ?? '')) return true;
  return ['name', 'id', 'aria-label', 'placeholder'].some((key) =>
    SECRET_NAME.test(attributes[key] ?? ''),
  );
}

function attributeMap(list: readonly string[] | undefined): Record<string, string> {
  const map: Record<string, string> = {};
  for (let index = 0; index + 1 < (list?.length ?? 0); index += 2)
    map[list![index]!.toLowerCase()] = list![index + 1]!;
  return map;
}

/** The attributes of the element a ref names, or of the focused element when there is no ref. */
export async function fieldAttributes(
  devtools: PageDebugger,
  ref: string | null | undefined,
): Promise<Record<string, string> | string> {
  if (ref) {
    const node = nodeOf(devtools, ref);
    if (typeof node === 'string') return node;
    try {
      const { node: described } = await devtools.send<Described>('DOM.describeNode', {
        backendNodeId: node,
      });
      return attributeMap(described?.attributes);
    } catch {
      return stale(devtools, ref);
    }
  }
  const focused = await devtools.send<{ result?: { value?: Record<string, string> | null } }>(
    'Runtime.evaluate',
    {
      expression:
        '(() => { const e = document.activeElement; if (!e || e === document.body) return null; const a = {}; for (const n of e.getAttributeNames()) a[n.toLowerCase()] = e.getAttribute(n) || ""; return a; })()',
      returnByValue: true,
    },
  );
  return focused.result?.value ?? '没有获得焦点的输入框：用 ref 指明要输入的字段。';
}
