import { afterEach, describe, expect, it, vi } from 'vitest';

import { CAPTURE_LIMIT_MS, pagePicture, type PicturedContents } from './page-picture.js';

function contents(
  capture: PicturedContents['capturePage'],
  destroyed = false,
): PicturedContents & { capturePage: ReturnType<typeof vi.fn> } {
  return { isDestroyed: () => destroyed, capturePage: vi.fn(capture) };
}

const image = (empty = false) => ({
  isEmpty: () => empty,
  toJPEG: (quality: number) => Buffer.from(`jpeg-${quality}`),
});

afterEach(() => {
  vi.useRealTimers();
});

describe('a picture of a browser page', () => {
  it('is a JPEG data URL of the page, taken without showing a hidden page', async () => {
    const page = contents(async () => image());
    expect(await pagePicture(page)).toBe(
      `data:image/jpeg;base64,${Buffer.from('jpeg-80').toString('base64')}`,
    );
    expect(page.capturePage).toHaveBeenCalledWith(undefined, { stayHidden: true });
  });

  it('is absent for a closed, blank or failing page', async () => {
    expect(await pagePicture(undefined)).toBeUndefined();
    expect(await pagePicture(contents(async () => image(), true))).toBeUndefined();
    expect(await pagePicture(contents(async () => image(true)))).toBeUndefined();
    expect(
      await pagePicture(
        contents(async () => {
          throw new Error('render process gone');
        }),
      ),
    ).toBeUndefined();
  });

  it('is absent when the page does not paint in time', async () => {
    vi.useFakeTimers();
    const waiting = pagePicture(contents(() => new Promise(() => undefined)));
    await vi.advanceTimersByTimeAsync(CAPTURE_LIMIT_MS);
    expect(await waiting).toBeUndefined();
  });
});
