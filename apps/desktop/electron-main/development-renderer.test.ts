import type { Session } from 'electron';
import { describe, expect, it, vi } from 'vitest';
import {
  DEVELOPMENT_RENDERER_HEADER,
  DEVELOPMENT_RENDERER_URL,
} from '../shared/development-renderer.js';
import { developmentRenderer, guardDevelopmentRenderer } from './development-renderer.js';

const instance = 'a'.repeat(32);
const environment = {
  AURA_RENDERER_URL: DEVELOPMENT_RENDERER_URL,
  AURA_DEV_RENDERER_INSTANCE: instance,
};

describe('development renderer', () => {
  it('uses the bundled renderer in a packaged app and requires an instance in development', () => {
    expect(developmentRenderer(environment, true)).toBeUndefined();
    expect(() =>
      developmentRenderer({ AURA_RENDERER_URL: DEVELOPMENT_RENDERER_URL }, false),
    ).toThrow();
    expect(developmentRenderer(environment, false)).toEqual({
      url: DEVELOPMENT_RENDERER_URL,
      instance,
    });
  });

  it('checks response ownership before Electron loads a document or an asset at that origin', () => {
    type Listener = (
      details: { responseHeaders?: Record<string, string[]> },
      answer: (result: { cancel: boolean }) => void,
    ) => void;
    const install = vi.fn<(filter: { urls: string[] }, listener: Listener) => void>();
    guardDevelopmentRenderer(
      { webRequest: { onHeadersReceived: install } } as unknown as Session,
      developmentRenderer(environment, false),
    );
    const [filter, listener] = install.mock.calls[0]!;
    expect(filter.urls).toEqual([`${DEVELOPMENT_RENDERER_URL}/*`]);
    const answer = vi.fn();
    listener({}, answer);
    expect(answer).toHaveBeenLastCalledWith({ cancel: true });
    listener({ responseHeaders: { [DEVELOPMENT_RENDERER_HEADER]: ['b'.repeat(32)] } }, answer);
    expect(answer).toHaveBeenLastCalledWith({ cancel: true });
    listener(
      { responseHeaders: { [DEVELOPMENT_RENDERER_HEADER.toUpperCase()]: [instance] } },
      answer,
    );
    expect(answer).toHaveBeenLastCalledWith({ cancel: false });
  });
});
