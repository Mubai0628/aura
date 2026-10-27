import { describe, expect, it } from 'vitest';

import { AttentionBadge, RED_DOT_SIZE, redDotPixels } from './attention-badge.js';

describe('the red dot on the taskbar button', () => {
  it('shows while a notified conversation has not been in front, and goes once each has', () => {
    const applied: number[] = [];
    let front: string | undefined;
    const badge = new AttentionBadge({
      apply: (count) => applied.push(count),
      inFront: (sessionId) => sessionId === front,
    });
    badge.add('ses_a');
    badge.add('ses_b');
    badge.add('ses_a');
    expect(applied).toEqual([1, 2]);
    badge.look();
    expect(applied, 'nothing in front yet').toEqual([1, 2]);
    front = 'ses_a';
    badge.look();
    front = 'ses_b';
    badge.look();
    expect(applied).toEqual([1, 2, 1, 0]);
    expect(badge.count).toBe(0);
  });

  it('is a red disc with a white rim and clear corners', () => {
    const pixels = redDotPixels();
    const at = (x: number, y: number) => [
      ...pixels.subarray((y * RED_DOT_SIZE + x) * 4, (y * RED_DOT_SIZE + x) * 4 + 4),
    ];
    expect(pixels.length).toBe(RED_DOT_SIZE * RED_DOT_SIZE * 4);
    expect(at(8, 8)).toEqual([0x4d, 0x48, 0xe5, 0xff]);
    expect(at(0, 0)).toEqual([0, 0, 0, 0]);
    expect(at(14, 8), 'the rim').toEqual([0xff, 0xff, 0xff, 0xff]);
  });
});
