// @vitest-environment node
import { afterEach, expect, test, vi } from 'vitest';
import { mediaPageRun } from './decoder-page.js';

afterEach(() => vi.unstubAllGlobals());

test.each([false, true])(
  'an 8K source retains only output-sized RGBA surfaces (sheet=%s)',
  async (sheet) => {
    const surfaces: Array<{ width: number; height: number }> = [];
    const labels: Array<{ text: string; x: number; y: number; maxWidth: number }> = [];
    class Video extends EventTarget {
      videoWidth = 7680;
      videoHeight = 4320;
      duration = 3;
      readyState = 4;
      currentTime = 0;
      onFrame: VideoFrameRequestCallback | undefined;
      pause() {}
      load() {
        queueMicrotask(() => this.dispatchEvent(new Event('loadedmetadata')));
      }
      play() {
        queueMicrotask(() =>
          this.onFrame?.(0, { mediaTime: this.currentTime } as VideoFrameCallbackMetadata),
        );
        return Promise.resolve();
      }
      requestVideoFrameCallback(callback: VideoFrameRequestCallback) {
        this.onFrame = callback;
        return 1;
      }
      cancelVideoFrameCallback() {}
      removeAttribute() {}
      remove() {}
      captureStream() {
        return { getVideoTracks: () => [{}], getAudioTracks: () => [], getTracks: () => [] };
      }
    }
    const video = new Video();
    let current = 0;
    Object.defineProperty(video, 'currentTime', {
      get: () => current,
      set: (time: number) => {
        current = time;
        queueMicrotask(() => video.dispatchEvent(new Event('seeked')));
      },
    });
    vi.stubGlobal('document', {
      body: { append() {} },
      createElement(name: string) {
        if (name === 'video') return video;
        const canvas = {
          width: 0,
          height: 0,
          getContext: () => ({
            drawImage() {},
            fillRect() {},
            fillText(text: string, x: number, y: number, maxWidth: number) {
              labels.push({ text, x, y, maxWidth });
            },
          }),
          toDataURL: () =>
            `data:image/jpeg;base64,${Buffer.from([255, 216, 255, 224, 255, 217]).toString('base64')}`,
        };
        surfaces.push(canvas);
        return canvas;
      },
    });
    const result = await mediaPageRun({
      url: 'aura://media-decoder/test/source',
      size: 100,
      container: 'webm',
      timeout: 1000,
      action: { kind: 'frames', times: null, count: 12, sheet, max_side: 64 },
    });
    expect(result.ok).toBe(true);
    expect(surfaces.length).toBeGreaterThanOrEqual(12);
    expect(surfaces.every((canvas) => Math.max(canvas.width, canvas.height) <= 64)).toBe(true);
    expect(Math.max(...surfaces.map((canvas) => canvas.width * canvas.height))).toBeLessThanOrEqual(
      64 * 64,
    );
    if (sheet) {
      expect(labels).toHaveLength(12);
      expect(
        labels.every(
          (label) => label.maxWidth <= 14 && (label.x % 16) + label.maxWidth <= 16 && label.y >= 7,
        ),
      ).toBe(true);
    }
  },
);
