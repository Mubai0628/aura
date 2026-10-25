// @vitest-environment node
import { describe, expect, it } from 'vitest';

import { HIDDEN, NetworkLog, redactUrl } from './network-log.js';

describe("a page's network log", () => {
  it('hides cookie, authorization and token values in headers and addresses', () => {
    const log = new NetworkLog();
    log.message('Network.requestWillBeSent', {
      requestId: '1',
      type: 'Fetch',
      request: {
        method: 'POST',
        url: 'https://api.example/v1/score?user=ada&access_token=abc123&page=2#id_token=xyz',
        headers: {
          Cookie: 'sid=s3cret',
          Authorization: 'Bearer abc',
          'X-CSRF-Token': 't0k',
          'Content-Type': 'application/json',
        },
      },
    });
    log.message('Network.responseReceived', {
      requestId: '1',
      response: {
        status: 401,
        mimeType: 'application/json',
        headers: { 'Set-Cookie': 'sid=new', 'Content-Length': '12' },
      },
    });
    log.message('Network.loadingFinished', { requestId: '1' });
    const [entry] = log.entries(false, 50).entries;
    expect(entry).toEqual({
      method: 'POST',
      url: `https://api.example/v1/score?user=ada&access_token=${HIDDEN}&page=2#${HIDDEN}`,
      kind: 'fetch',
      status: 401,
      mime_type: 'application/json',
      failure: null,
      request_headers: [
        `Cookie: ${HIDDEN}`,
        `Authorization: ${HIDDEN}`,
        `X-CSRF-Token: ${HIDDEN}`,
        'Content-Type: application/json',
      ],
      response_headers: [`Set-Cookie: ${HIDDEN}`, 'Content-Length: 12'],
    });
    expect(JSON.stringify(log.entries(false, 50))).not.toMatch(/s3cret|abc123|Bearer|t0k|xyz/u);
    expect(redactUrl('https://ada:pw@example.com/a?code=42')).toBe(
      `https://${HIDDEN}@example.com/a?code=${HIDDEN}`,
    );
  });

  it('keeps each hop of a redirect, failures, and only the failed ones when asked', () => {
    const log = new NetworkLog();
    log.message('Network.requestWillBeSent', {
      requestId: '7',
      type: 'Document',
      request: { method: 'GET', url: 'http://127.0.0.1:9/', headers: {} },
    });
    log.message('Network.requestWillBeSent', {
      requestId: '7',
      type: 'Document',
      redirectResponse: { status: 302, headers: { Location: '/game' } },
      request: { method: 'GET', url: 'http://127.0.0.1:9/game', headers: {} },
    });
    log.message('Network.responseReceived', {
      requestId: '7',
      response: { status: 200, mimeType: 'text/html', headers: {} },
    });
    log.message('Network.requestWillBeSent', {
      requestId: '8',
      type: 'Script',
      request: { method: 'GET', url: 'http://127.0.0.1:9/board.js', headers: {} },
    });
    log.message('Network.loadingFailed', {
      requestId: '8',
      errorText: 'net::ERR_CONNECTION_REFUSED',
    });
    log.message('Network.requestWillBeSent', {
      requestId: '9',
      type: 'Image',
      request: { method: 'GET', url: 'https://tracker.example/p.gif', headers: {} },
    });
    log.message('Network.loadingFailed', {
      requestId: '9',
      errorText: 'net::ERR_BLOCKED_BY_CLIENT',
    });
    const all = log.entries(false, 50).entries;
    expect(all.map((entry) => [entry.url, entry.status, entry.failure])).toEqual([
      ['http://127.0.0.1:9/', 302, null],
      ['http://127.0.0.1:9/game', 200, null],
      ['http://127.0.0.1:9/board.js', null, 'net::ERR_CONNECTION_REFUSED'],
      ['https://tracker.example/p.gif', null, 'net::ERR_BLOCKED_BY_CLIENT（浏览器设置不允许）'],
    ]);
    const failed = log.entries(true, 1);
    expect(failed.entries.map((entry) => entry.url)).toEqual(['https://tracker.example/p.gif']);
    expect(failed.truncated).toBe(true);
    expect(log.message('Page.loadEventFired', {})).toBe(false);
  });
});
