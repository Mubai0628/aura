// @vitest-environment node
import { describe, expect, it } from 'vitest';

import { parseUserCommand } from './commands.js';

const page = { sessionId: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV', pageId: 'tab-3' };

describe('commands from the renderer', () => {
  it('are accepted only in their exact shape', () => {
    expect(parseUserCommand({ kind: 'open', ...page })).toEqual({ kind: 'open', ...page });
    expect(parseUserCommand({ kind: 'navigate', ...page, input: 'example.com' })).toEqual({
      kind: 'navigate',
      ...page,
      input: 'example.com',
    });
    expect(
      parseUserCommand({
        kind: 'show',
        ...page,
        bounds: { x: 10, y: 20, width: 400, height: 300 },
      }),
    ).toEqual({ kind: 'show', ...page, bounds: { x: 10, y: 20, width: 400, height: 300 } });
    for (const kind of [
      'devtools',
      'back',
      'forward',
      'reload',
      'stop',
      'hide',
      'close',
      'take-control',
      'release-control',
      'dismiss-notice',
    ])
      expect(parseUserCommand({ kind, ...page })).toEqual({ kind, ...page });
    expect(parseUserCommand({ kind: 'clear-data' })).toEqual({ kind: 'clear-data' });
    // A workspace web page, by its workspace and path.
    const file = { workspaceId: 'wsp_01ARZ3NDEKTSV4RRFFQ69G5FAV', path: 'game/index.html' };
    expect(parseUserCommand({ kind: 'open-file', ...page, ...file })).toEqual({
      kind: 'open-file',
      ...page,
      ...file,
    });
  });

  it('refuse anything else', () => {
    const bad: unknown[] = [
      undefined,
      'open',
      { kind: 'evaluate', ...page, script: '1' },
      { kind: 'clear-data', ...page },
      { kind: 'open', ...page, extra: true },
      { kind: 'open', sessionId: '', pageId: 'tab-3' },
      { kind: 'open', sessionId: 'ses x', pageId: 'tab-3' },
      { kind: 'open', ...page, pageId: '../model' },
      { kind: 'navigate', ...page, input: 7 },
      { kind: 'navigate', ...page, input: 'x'.repeat(4097) },
      { kind: 'open-file', ...page, workspaceId: 'wsp x', path: 'a.html' },
      { kind: 'open-file', ...page, workspaceId: 'wsp_1', path: '' },
      { kind: 'open-file', ...page, workspaceId: 'wsp_1', path: 'a.html', url: 'file:///C:/x' },
      { kind: 'show', ...page, bounds: { x: -1, y: 0, width: 10, height: 10 } },
      { kind: 'show', ...page, bounds: { x: 0, y: 0, width: Number.NaN, height: 10 } },
      { kind: 'show', ...page, bounds: { x: 0, y: 0, width: 0, height: 10 } },
      { kind: 'show', ...page, bounds: { x: 0, y: 0, width: 10, height: 10, z: 1 } },
      { kind: 'show', ...page, bounds: { x: 0, y: 0, width: 100000, height: 10 } },
    ];
    for (const value of bad) expect(parseUserCommand(value), JSON.stringify(value)).toBeUndefined();
  });
});
