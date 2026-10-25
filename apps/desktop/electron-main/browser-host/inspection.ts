/**
 * What DevTools' Elements and Performance panels would tell the model, read through the DevTools
 * protocol. No script runs in the page and nothing on it changes.
 */
import type { PageDebugger } from './cdp.js';
import { nodeOf } from './elements.js';

/** The longest HTML given back. */
export const HTML_CHARS = 8_000;

/** The computed properties that decide an element's layout and look. */
const STYLES = [
  'display',
  'position',
  'top',
  'left',
  'right',
  'bottom',
  'width',
  'height',
  'margin',
  'padding',
  'border',
  'box-sizing',
  'flex-direction',
  'justify-content',
  'align-items',
  'gap',
  'grid-template-columns',
  'overflow',
  'z-index',
  'opacity',
  'visibility',
  'transform',
  'color',
  'background-color',
  'font-family',
  'font-size',
  'font-weight',
  'line-height',
  'text-align',
  'cursor',
  'pointer-events',
];

export interface Inspected {
  html: string;
  html_truncated: boolean;
  box_model: { x: number; y: number; width: number; height: number } | null;
  styles: string[];
}

/** One element, by its snapshot ref: its HTML, its border box and its computed style. */
export async function inspectElement(
  devtools: PageDebugger,
  element: string,
): Promise<Inspected | string> {
  const node = nodeOf(devtools, element);
  if (typeof node === 'string') return node;
  const { outerHTML } = await devtools.send<{ outerHTML: string }>('DOM.getOuterHTML', {
    backendNodeId: node,
  });
  let box: Inspected['box_model'] = null;
  try {
    const { model } = await devtools.send<{ model: { border: number[] } }>('DOM.getBoxModel', {
      backendNodeId: node,
    });
    const xs = [0, 2, 4, 6].map((index) => model.border[index] ?? 0);
    const ys = [1, 3, 5, 7].map((index) => model.border[index] ?? 0);
    const x = Math.min(...xs);
    const y = Math.min(...ys);
    box = {
      x: Math.round(x),
      y: Math.round(y),
      width: Math.round(Math.max(...xs) - x),
      height: Math.round(Math.max(...ys) - y),
    };
  } catch {
    // Not drawn: display: none, or no longer in the page.
  }
  // Computed style needs the element's id in this DevTools session's own node tree.
  await devtools.send('DOM.getDocument', { depth: 0 });
  const { nodeIds } = await devtools.send<{ nodeIds: number[] }>(
    'DOM.pushNodesByBackendIdsToFrontend',
    { backendNodeIds: [node] },
  );
  await devtools.send('CSS.enable', {});
  const { computedStyle } = await devtools.send<{
    computedStyle: { name: string; value: string }[];
  }>('CSS.getComputedStyleForNode', { nodeId: nodeIds[0] });
  const values = new Map(computedStyle.map(({ name, value }) => [name, value]));
  return {
    html: outerHTML.slice(0, HTML_CHARS),
    html_truncated: outerHTML.length > HTML_CHARS,
    box_model: box,
    styles: STYLES.filter((name) => values.has(name)).map(
      (name) => `${name}: ${values.get(name)!}`,
    ),
  };
}

/** Chromium's counters, in the order a reader looks for them, with units. */
const COUNTERS: readonly [string, 'bytes' | 'seconds' | 'count'][] = [
  ['JSHeapUsedSize', 'bytes'],
  ['JSHeapTotalSize', 'bytes'],
  ['Nodes', 'count'],
  ['Documents', 'count'],
  ['Frames', 'count'],
  ['JSEventListeners', 'count'],
  ['LayoutCount', 'count'],
  ['LayoutDuration', 'seconds'],
  ['RecalcStyleCount', 'count'],
  ['RecalcStyleDuration', 'seconds'],
  ['ScriptDuration', 'seconds'],
  ['TaskDuration', 'seconds'],
];

function measured(value: number, unit: 'bytes' | 'seconds' | 'count'): string {
  if (unit === 'bytes') return `${(value / 1024 / 1024).toFixed(1)} MB`;
  if (unit === 'seconds') return `${value.toFixed(3)} s`;
  return String(Math.round(value));
}

/** The page's performance counters, one a line. */
export async function performanceCounters(devtools: PageDebugger): Promise<string[]> {
  await devtools.send('Performance.enable', {});
  const { metrics } = await devtools.send<{ metrics: { name: string; value: number }[] }>(
    'Performance.getMetrics',
    {},
  );
  const values = new Map(metrics.map(({ name, value }) => [name, value]));
  return COUNTERS.filter(([name]) => values.has(name)).map(
    ([name, unit]) => `${name}: ${measured(values.get(name)!, unit)}`,
  );
}
