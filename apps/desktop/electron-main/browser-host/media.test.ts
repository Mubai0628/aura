// @vitest-environment node
import { expect, test, vi } from 'vitest';

import { mediaSection, pageMedia } from './media.js';

// The model wrote eight scripts to take a video site's code apart before the
// page, open in Aura's browser, showed the video's address. The snapshot now lists the page's
// video and audio with their addresses.

test('the snapshot lists the video and audio a page plays, each with its address', () => {
  expect(
    mediaSection([
      { kind: 'video', url: 'https://vd9.bdstatic.com/mda-x/clip.mp4' },
      { kind: 'video', url: 'blob:https://haokan.baidu.com/1234' },
      { kind: 'audio', url: 'https://example.com/a.mp3' },
      { kind: 'video', url: 'data:video/mp4;base64,AAAA' },
    ]),
  ).toBe(
    [
      '',
      'media:',
      '- video https://vd9.bdstatic.com/mda-x/clip.mp4',
      '- video blob:https://haokan.baidu.com/1234 (a stream: its parts are in browser.network)',
      '- audio https://example.com/a.mp3',
    ].join('\n'),
  );
  expect(mediaSection([])).toBe('');
});

test("the elements are read through DevTools' DOM: nothing runs on the page", async () => {
  const nodes: Record<number, { nodeName: string; attributes: string[] }> = {
    11: { nodeName: 'VIDEO', attributes: ['class', 'player', 'src', '/mda-x/clip.mp4'] },
    12: { nodeName: 'SOURCE', attributes: ['src', 'https://cdn.example.com/a.webm'] },
    13: { nodeName: 'SOURCE', attributes: ['src', 'track.mp3', 'type', 'audio/mpeg'] },
    14: { nodeName: 'VIDEO', attributes: ['class', 'empty'] },
    15: { nodeName: 'VIDEO', attributes: ['src', '/mda-x/clip.mp4'] },
  };
  const send = vi.fn(async (method: string, params?: { nodeId?: number }) => {
    if (method === 'DOM.getDocument') return { root: { nodeId: 1 } };
    if (method === 'DOM.querySelectorAll') return { nodeIds: [11, 12, 13, 14, 15] };
    if (method === 'DOM.describeNode') return { node: nodes[params?.nodeId ?? 0] };
    throw new Error(`unexpected ${method}`);
  });
  expect(await pageMedia({ send } as never, 'https://haokan.baidu.com/v?vid=1')).toEqual([
    { kind: 'video', url: 'https://haokan.baidu.com/mda-x/clip.mp4' },
    { kind: 'video', url: 'https://cdn.example.com/a.webm' },
    { kind: 'audio', url: 'https://haokan.baidu.com/track.mp3' },
  ]);
  expect(send.mock.calls.map(([method]) => method)).not.toContain('Runtime.evaluate');
  const failing = vi.fn(async () => {
    throw new Error('Execution context was destroyed');
  });
  expect(await pageMedia({ send: failing } as never, 'https://example.com/')).toEqual([]);
});
