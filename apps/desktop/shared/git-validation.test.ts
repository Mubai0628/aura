import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import type { IpcMainInvokeEvent } from 'electron';
import { describe, expect, it, vi } from 'vitest';
import { dispatchDomainRoute } from '../electron-main/domain-route.js';
import { isGitParams, parseGitResult } from './git-validation.js';
const fixture = JSON.parse(
  readFileSync(
    resolve(import.meta.dirname, '../../../fixtures/contracts/git-reads.v1.json'),
    'utf8',
  ),
) as { requests: unknown[]; results: unknown[]; runtime: unknown[] };
describe('source-owned native Git boundary', () => {
  it('accepts the raw comparison explanation per file and rejects non-text notes', () => {
    for (const index of [0, 1]) {
      const value = structuredClone(fixture.results[index]) as {
        result: { entries?: Record<string, unknown>[]; files?: Record<string, unknown>[] };
      };
      const file = (value.result.entries ?? value.result.files)![0]!;
      file.comparison_note = '未运行外部程序，按原始内容比较';
      expect(parseGitResult('git.workspace.read', value)).toEqual(value);
      file.comparison_note = 42;
      expect(() => parseGitResult('git.workspace.read', value)).toThrow();
    }
  });
  it('accepts every Rust fixture and rejects missing or forged fields', () => {
    fixture.requests.forEach((request, index) => {
      expect(isGitParams('git.workspace.read', request)).toBe(true);
      expect(parseGitResult('git.workspace.read', fixture.results[index])).toEqual(
        fixture.results[index],
      );
    });
    fixture.runtime.forEach((value) =>
      expect(parseGitResult('git.runtime.get', value)).toEqual(value),
    );
    for (const bad of [
      { ...(fixture.requests[0] as object), actor: 'model' },
      { ...(fixture.requests[0] as object), session_id: 'wsp_01ARZ3NDEKTSV4RRFFQ69G5FAV' },
      { ...(fixture.requests[0] as object), query: { kind: 'status', input: { limit: 0 } } },
      { ...(fixture.requests[0] as object), query: { kind: 'diff', input: { max_bytes: 65537 } } },
    ])
      expect(isGitParams('git.workspace.read', bad)).toBe(false);
    expect(() =>
      parseGitResult('git.workspace.read', { kind: 'diff', result: { patch: 'unversioned' } }),
    ).toThrow();
  });
  it('routes only validated Main Git calls and rejects arbitrary install URLs', async () => {
    const handlers = {
      agent: vi.fn(),
      workspace: vi.fn(),
      security: vi.fn(),
      model: vi.fn(),
      tool: vi.fn(),
      git: vi.fn().mockResolvedValue(fixture.results[0]),
      package: vi.fn(),
      memory: vi.fn(),
    };
    const event = {} as IpcMainInvokeEvent;
    await expect(
      dispatchDomainRoute(event, 'git.workspace.read', fixture.requests[0], handlers),
    ).resolves.toEqual(fixture.results[0]);
    await expect(
      dispatchDomainRoute(
        event,
        'git.installation.open',
        { url: 'https://example.invalid' },
        handlers,
      ),
    ).rejects.toThrow();
    expect(handlers.git).toHaveBeenCalledTimes(1);
    expect(handlers.agent).not.toHaveBeenCalled();
  });
});
