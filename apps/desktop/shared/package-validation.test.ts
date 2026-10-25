import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import type { IpcMainInvokeEvent } from 'electron';
import { describe, expect, it, vi } from 'vitest';
import { dispatchDomainRoute } from '../electron-main/domain-route.js';
import { isPackageCommand, isPackageParams, parsePackageResult } from './package-validation.js';
const fixture = JSON.parse(
  readFileSync(
    resolve(import.meta.dirname, '../../../fixtures/contracts/package-routes.v1.json'),
    'utf8',
  ),
) as Record<string, unknown>;
describe('source-owned tool-package boundary', () => {
  it('accepts every Rust-generated route value and rejects widened ones', () => {
    expect(isPackageParams('packages.list', {})).toBe(true);
    expect(isPackageParams('packages.import_directory', {})).toBe(true);
    expect(isPackageParams('packages.import_directory', { path: 'C:/anything' })).toBe(false);
    expect(isPackageParams('packages.version.get', fixture.version_ref)).toBe(true);
    expect(isPackageParams('packages.conformance.prepare', fixture.prepare)).toBe(true);
    expect(isPackageParams('packages.conformance.start', fixture.start)).toBe(true);
    expect(isPackageParams('packages.conformance.cancel', fixture.cancel)).toBe(true);
    expect(isPackageParams('packages.install', fixture.install)).toBe(true);
    expect(isPackageParams('packages.set_enabled', fixture.enable)).toBe(true);
    expect(isPackageParams('packages.uninstall', fixture.change)).toBe(true);
    expect(
      isPackageParams('packages.install', { ...(fixture.install as object), trusted: true }),
    ).toBe(false);
    expect(parsePackageResult('packages.list', fixture.list)).toEqual(fixture.list);
    expect(parsePackageResult('packages.version.get', fixture.version_view)).toEqual(
      fixture.version_view,
    );
    expect(parsePackageResult('packages.import_directory', fixture.import_imported)).toEqual(
      fixture.import_imported,
    );
    expect(parsePackageResult('packages.import_archive', fixture.import_cancelled)).toEqual(
      fixture.import_cancelled,
    );
    expect(parsePackageResult('packages.conformance.prepare', fixture.preparation)).toEqual(
      fixture.preparation,
    );
    expect(() => parsePackageResult('packages.list', { packages: [{}] })).toThrow(
      'Invalid package result contract',
    );
    expect(isPackageCommand('packages.install')).toBe(true);
    expect(isPackageCommand('tools.draft.apply')).toBe(false);
  });
  it('routes only valid package requests to the package handler with the event', async () => {
    const handlers = {
      agent: vi.fn(),
      workspace: vi.fn(),
      security: vi.fn(),
      model: vi.fn(),
      tool: vi.fn(),
      git: vi.fn(),
      package: vi.fn().mockResolvedValue({ type: 'cancelled' }),
      memory: vi.fn(),
    };
    const event = {} as IpcMainInvokeEvent;
    await expect(
      dispatchDomainRoute(event, 'packages.import_directory', {}, handlers),
    ).resolves.toEqual({ type: 'cancelled' });
    expect(handlers.package).toHaveBeenCalledWith(event, 'packages.import_directory', {});
    await expect(
      dispatchDomainRoute(event, 'packages.import_directory', { path: 'C:/x' }, handlers),
    ).rejects.toThrow('Invalid domain route parameters');
  });
});
