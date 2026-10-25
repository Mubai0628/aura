// @vitest-environment node
import { describe, expect, it } from 'vitest';

import { execute } from './actions.js';
import { command, setup } from './executor-fakes.js';
import { parsedRules } from './site-rules.js';

// What DevTools' Elements and Performance panels show, read through DevTools; nothing runs in the page.

const POLICY = { enabled: true, allowed: [], blocked: parsedRules([]) };
const open = { kind: 'open', url: 'http://127.0.0.1:9/game' } as const;

describe('element inspection and performance counters', () => {
  it("are refused while the user's switch is off", async () => {
    const { deps, pages, tools } = setup();
    pages.setPolicy(POLICY);
    await execute(deps, command(open));
    for (const action of [
      { kind: 'inspect', element: 'e3' } as const,
      { kind: 'performance' } as const,
    ]) {
      const result = await execute(deps, command(action, '127.0.0.1'));
      expect(result.outcome).toMatchObject({
        outcome: 'refused',
        message: expect.stringContaining('允许模型调试网页'),
      });
    }
    expect(tools.methods()).not.toContain('DOM.getOuterHTML');
    expect(tools.methods()).not.toContain('Performance.getMetrics');
  });

  it("read an element's HTML, border box and computed style by its ref, with no page script", async () => {
    const { deps, pages, tools, devtools } = setup();
    pages.setPolicy({ ...POLICY, debugging: true });
    await execute(deps, command(open));
    const ref = devtools.refs.refFor(77);
    tools.answers.set('DOM.getOuterHTML', () => ({ outerHTML: '<canvas id="board"></canvas>' }));
    tools.answers.set('DOM.getBoxModel', () => ({
      model: { border: [10.4, 20, 650.6, 20, 650.6, 500.2, 10.4, 500.2] },
    }));
    tools.answers.set('DOM.pushNodesByBackendIdsToFrontend', () => ({ nodeIds: [5] }));
    tools.answers.set('CSS.getComputedStyleForNode', () => ({
      computedStyle: [
        { name: 'display', value: 'block' },
        { name: 'z-index', value: '2' },
        { name: 'animation-name', value: 'none' },
      ],
    }));
    const result = await execute(deps, command({ kind: 'inspect', element: ref }, '127.0.0.1'));
    expect(result.outcome).toMatchObject({
      outcome: 'completed',
      result: {
        url: 'http://127.0.0.1:9/game',
        html: '<canvas id="board"></canvas>',
        html_truncated: false,
        box_model: { x: 10, y: 20, width: 640, height: 480 },
        styles: ['display: block', 'z-index: 2'],
      },
    });
    expect(tools.sent.find(([method]) => method === 'DOM.getOuterHTML')?.[1]).toEqual({
      backendNodeId: 77,
    });
    expect(tools.methods()).not.toContain('Runtime.evaluate');
    expect(tools.methods()).not.toContain('Runtime.callFunctionOn');
    // A ref from no snapshot names nothing.
    const unknown = await execute(deps, command({ kind: 'inspect', element: 'e999' }, '127.0.0.1'));
    expect(unknown.outcome).toMatchObject({ outcome: 'refused' });
  });

  it("read the page's counters with their units", async () => {
    const { deps, pages, tools } = setup();
    pages.setPolicy({ ...POLICY, debugging: true });
    await execute(deps, command(open));
    tools.answers.set('Performance.getMetrics', () => ({
      metrics: [
        { name: 'Nodes', value: 1520 },
        { name: 'JSHeapUsedSize', value: 13_002_342 },
        { name: 'ScriptDuration', value: 0.8412 },
        { name: 'Timestamp', value: 123 },
      ],
    }));
    const result = await execute(deps, command({ kind: 'performance' }, '127.0.0.1'));
    expect(result.outcome).toMatchObject({
      outcome: 'completed',
      result: {
        metrics: ['JSHeapUsedSize: 12.4 MB', 'Nodes: 1520', 'ScriptDuration: 0.841 s'],
      },
    });
  });
});
