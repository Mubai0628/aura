import { describe, expect, it, vi } from 'vitest';
import { opensInBrowser, signInMcpServer } from './mcp-sign-in.js';
import type { CoreSupervisor } from './core-supervisor.js';

const supervisor = (authorization_url: string) =>
  ({
    requestAgent: vi.fn(async () => ({
      authorization_url,
      authorization_server: 'auth.example.com',
    })),
  }) as unknown as CoreSupervisor;

describe('MCP sign-in in Main', () => {
  it('opens only https or this computer', () => {
    expect(opensInBrowser('https://auth.example.com/authorize?x=1')).toBe(true);
    expect(opensInBrowser('http://127.0.0.1:7000/auth/authorize')).toBe(true);
    expect(opensInBrowser('http://localhost:7000/auth/authorize')).toBe(true);
    expect(opensInBrowser('http://auth.example.com/authorize')).toBe(false);
    expect(opensInBrowser('file:///C:/Windows/system32/calc.exe')).toBe(false);
    expect(opensInBrowser('javascript:alert(1)')).toBe(false);
    expect(opensInBrowser('not an address')).toBe(false);
  });

  it("opens Core's address in the browser and returns it as a link to copy", async () => {
    const open = vi.fn(async () => undefined);
    const address = 'https://auth.example.com/authorize?state=s';
    const begun = await signInMcpServer({ id: 'web' }, supervisor(address), open);
    expect(open).toHaveBeenCalledWith(address);
    expect(begun.authorization_url).toBe(address);
  });

  it('refuses an address it would not open, and a browser that fails still leaves the link', async () => {
    const open = vi.fn(async () => undefined);
    await expect(
      signInMcpServer({ id: 'web' }, supervisor('http://auth.example.com/authorize'), open),
    ).rejects.toThrow(/sign_in_refused/u);
    expect(open).not.toHaveBeenCalled();
    const failing = vi.fn(async () => {
      throw new Error('no browser');
    });
    const begun = await signInMcpServer(
      { id: 'web' },
      supervisor('https://auth.example.com/authorize'),
      failing,
    );
    expect(begun.authorization_url).toBe('https://auth.example.com/authorize');
  });
});
