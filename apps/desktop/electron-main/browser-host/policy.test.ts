// @vitest-environment node
import { describe, expect, it } from 'vitest';

import { addressFromInput, hostClass } from './address.js';
import { navigationVerdict, subrequestVerdict, type BrowserPolicy } from './policy.js';
import { parsedRules } from './site-rules.js';

const policy = (over: Partial<BrowserPolicy> = {}): BrowserPolicy => ({
  enabled: true,
  allowed: parsedRules(['docs.rs', 'localhost:5173']),
  blocked: parsedRules(['*.tracker.example', 'blocked.example']),
  ...over,
});
const user = { kind: 'user', readOnly: false, approvedHosts: [] } as const;
const model = { kind: 'model', readOnly: false, approvedHosts: [] } as const;
const readOnly = { kind: 'model', readOnly: true, approvedHosts: ['approved.example'] } as const;

describe('the address bar', () => {
  it('opens what was typed: a full address, a host, localhost over http', () => {
    expect(addressFromInput(' https://Example.com/a?b ')).toBe('https://example.com/a?b');
    expect(addressFromInput('example.com/docs')).toBe('https://example.com/docs');
    expect(addressFromInput('localhost:5173')).toBe('http://localhost:5173/');
    expect(addressFromInput('127.0.0.1:8000/game.html')).toBe('http://127.0.0.1:8000/game.html');
    expect(addressFromInput('192.168.1.20')).toBe('http://192.168.1.20/');
    expect(addressFromInput('[::1]:3000')).toBe('http://[::1]:3000/');
    expect(addressFromInput('about:blank')).toBe('about:blank');
  });

  it('refuses what is not a web address', () => {
    for (const text of [
      '',
      'two words',
      'javascript:alert(1)',
      'file:///C:/x',
      'chrome://gpu',
      'foo',
    ])
      expect(addressFromInput(text), text).toBeUndefined();
  });
});

describe('host classes', () => {
  it('tell loopback, private and public hosts apart', () => {
    for (const host of ['localhost', 'app.localhost', '127.0.0.1', '127.9.9.9', '[::1]', '0.0.0.0'])
      expect(hostClass(host), host).toBe('loopback');
    for (const host of [
      '10.0.0.1',
      '172.16.0.1',
      '172.31.255.255',
      '192.168.1.1',
      '169.254.1.1',
      '100.64.0.1',
      '[fd00::1]',
      '[fe80::1]',
      '[::ffff:192.168.1.1]',
    ])
      expect(hostClass(host), host).toBe('private');
    for (const host of ['example.com', '8.8.8.8', '172.32.0.1', '[2606:4700::1111]'])
      expect(hostClass(host), host).toBe('public');
  });
});

describe('navigation', () => {
  it('is refused while the browser is off', () => {
    expect(navigationVerdict(policy({ enabled: false }), user, 'https://docs.rs/')).toEqual({
      allow: false,
      reason: 'disabled',
    });
  });

  it('never reaches a blocked site, in any page or mode, redirects included', () => {
    for (const page of [user, model, readOnly])
      for (const url of ['https://blocked.example/', 'http://a.b.tracker.example:8080/x'])
        expect(navigationVerdict(policy(), page, url)).toMatchObject({
          allow: false,
          reason: 'blocked',
        });
  });

  it('lets a read-only session reach allowed and approved hosts only', () => {
    expect(navigationVerdict(policy(), readOnly, 'https://docs.rs/serde').allow).toBe(true);
    expect(navigationVerdict(policy(), readOnly, 'http://localhost:5173/').allow).toBe(true);
    expect(navigationVerdict(policy(), readOnly, 'https://approved.example/x').allow).toBe(true);
    expect(navigationVerdict(policy(), readOnly, 'http://localhost:5174/')).toMatchObject({
      allow: false,
      reason: 'read-only',
      site: 'localhost',
    });
    expect(navigationVerdict(policy(), readOnly, 'https://other.example/')).toMatchObject({
      allow: false,
      reason: 'read-only',
    });
  });

  it('lets other sessions and the user reach any site that is not blocked', () => {
    expect(navigationVerdict(policy(), model, 'https://other.example/').allow).toBe(true);
    expect(navigationVerdict(policy(), user, 'https://other.example/').allow).toBe(true);
  });

  it('opens a workspace file in a model page, and nothing outside the workspace', () => {
    const workspace = { ...model, fileRoot: 'file:///C:/Work/Game/' };
    expect(navigationVerdict(policy(), workspace, 'file:///C:/Work/Game/index.html').allow).toBe(
      true,
    );
    expect(navigationVerdict(policy(), workspace, 'file:///c:/work/game/levels/1.html').allow).toBe(
      true,
    );
    for (const url of [
      'file:///C:/Work/Other/x.html',
      'file:///C:/Work/Game/../secret.txt',
      'file:///C:/Work/GameX/a.html',
    ])
      expect(navigationVerdict(policy(), workspace, url)).toMatchObject({
        allow: false,
        reason: 'scheme',
      });
    expect(navigationVerdict(policy(), { ...user }, 'file:///C:/Work/Game/index.html').allow).toBe(
      false,
    );
    expect(
      subrequestVerdict(
        policy(),
        'file:///C:/Work/Game/index.html',
        'file:///C:/Work/Game/game.js',
        'file:///C:/Work/Game/',
      ).allow,
    ).toBe(true);
    expect(
      subrequestVerdict(
        policy(),
        'file:///C:/Work/Game/index.html',
        'file:///C:/Users/me/.ssh/id_rsa',
        'file:///C:/Work/Game/',
      ).allow,
    ).toBe(false);
    expect(
      subrequestVerdict(
        policy(),
        'https://news.example/',
        'file:///C:/Work/Game/game.js',
        'file:///C:/Work/Game/',
      ).allow,
    ).toBe(false);
  });

  // A workspace page the user opened in a page of
  // their own, and its files; still nothing outside that workspace.
  it('opens the workspace of a page the user opened, and nothing outside it', () => {
    const opened = { ...user, fileRoot: 'file:///C:/Work/Game/' };
    expect(navigationVerdict(policy(), opened, 'file:///C:/Work/Game/levels/2.html').allow).toBe(
      true,
    );
    for (const url of ['file:///C:/Work/Other/x.html', 'file:///C:/Users/me/.ssh/id_rsa'])
      expect(navigationVerdict(policy(), opened, url)).toMatchObject({
        allow: false,
        reason: 'scheme',
      });
  });

  it('opens only web pages and about:blank', () => {
    expect(navigationVerdict(policy(), user, 'about:blank').allow).toBe(true);
    for (const url of ['file:///C:/x.html', 'chrome://settings', 'javascript:alert(1)'])
      expect(navigationVerdict(policy(), user, url)).toMatchObject({
        allow: false,
        reason: 'scheme',
      });
  });
});

describe('requests a page makes', () => {
  it('never reach a blocked site', () => {
    expect(
      subrequestVerdict(policy(), 'https://news.example/', 'https://px.tracker.example/p.gif'),
    ).toMatchObject({
      allow: false,
      reason: 'blocked',
    });
  });

  it('cannot reach this computer or the local network from a public page', () => {
    expect(
      subrequestVerdict(policy(), 'https://news.example/', 'http://127.0.0.1:8080/admin'),
    ).toMatchObject({
      allow: false,
      reason: 'private-from-public',
    });
    expect(subrequestVerdict(policy(), 'https://news.example/', 'ws://192.168.1.2/')).toMatchObject(
      {
        allow: false,
        reason: 'private-from-public',
      },
    );
  });

  it('may reach local services from a local page and public ones from anywhere', () => {
    expect(
      subrequestVerdict(policy(), 'http://localhost:5173/', 'http://127.0.0.1:8080/api').allow,
    ).toBe(true);
    expect(
      subrequestVerdict(policy(), 'http://localhost:5173/', 'https://cdn.example/lib.js').allow,
    ).toBe(true);
    expect(
      subrequestVerdict(policy(), 'https://news.example/', 'https://cdn.example/lib.js').allow,
    ).toBe(true);
    expect(
      subrequestVerdict(policy(), 'https://news.example/', 'data:image/png;base64,AA').allow,
    ).toBe(true);
  });

  it('stop with the browser', () => {
    expect(
      subrequestVerdict(policy({ enabled: false }), 'https://a.example/', 'https://a.example/x'),
    ).toMatchObject({
      allow: false,
      reason: 'disabled',
    });
  });
});
