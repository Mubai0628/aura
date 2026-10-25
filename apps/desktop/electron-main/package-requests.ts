import { CORE_NOT_READY_MESSAGE } from './core-unavailable.js';
import { dialog, type IpcMainInvokeEvent } from 'electron';
import type { CoreSupervisor } from './core-supervisor.js';
import { parsePackageResult } from '../shared/package-validation.js';
import type {
  PackageCoreCommand,
  PackageCoreParams,
  PackageCoreResult,
  PackageMainCommand,
  PackageMainParams,
} from '../shared/generated/routes.js';
import type { ConfirmedRequest } from './secret-requests.js';
import type { BrowserWindow } from 'electron';

export function requestPackageCommand<C extends PackageCoreCommand>(
  command: C,
  params: PackageCoreParams[C],
  confirm: ConfirmedRequest,
): Promise<PackageCoreResult[C]> {
  return confirm({
    command,
    // The Core and Main result types are identical for every package route.
    accept: (value) => parsePackageResult(command, value) as PackageCoreResult[C],
    send: (transport) => transport.request(command, params, 1, 120_000),
  });
}

/** Imports read only what the user picked in Main's own dialog; the renderer names no path. */
export async function dispatchPackageCommand(
  event: IpcMainInvokeEvent,
  command: PackageMainCommand,
  params: PackageMainParams[PackageMainCommand],
  core: CoreSupervisor | undefined,
  owner: (event: IpcMainInvokeEvent, purpose: string) => BrowserWindow,
): Promise<unknown> {
  if (core === undefined) throw new Error(CORE_NOT_READY_MESSAGE);
  if (command === 'packages.import_directory' || command === 'packages.import_archive') {
    const archive = command === 'packages.import_archive';
    const selection = await dialog.showOpenDialog(owner(event, 'Tool package import'), {
      title: archive ? '导入工具包 ZIP' : '导入工具包目录',
      properties: archive ? ['openFile', 'dontAddToRecent'] : ['openDirectory', 'dontAddToRecent'],
      ...(archive ? { filters: [{ extensions: ['zip'], name: 'ZIP' }] } : {}),
    });
    const path = selection.filePaths[0];
    if (selection.canceled || path === undefined) return { type: 'cancelled' };
    return core.domains.package(command, { path });
  }
  return core.domains.package(
    command,
    params as PackageCoreParams[Exclude<
      PackageCoreCommand,
      'packages.import_directory' | 'packages.import_archive'
    >],
  );
}
