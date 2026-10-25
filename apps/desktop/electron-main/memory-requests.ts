import { stat } from 'node:fs/promises';
import { CORE_NOT_READY_MESSAGE } from './core-unavailable.js';
import { dialog, shell, type IpcMainInvokeEvent, type BrowserWindow } from 'electron';
import type { MemoryVaultRequest } from '@aura/contracts/generated';
import type {
  MemoryMainCommand,
  MemoryMainParams,
  MemoryCoreCommand,
  MemoryCoreParams,
} from '../shared/generated/routes.js';
import type { CoreSupervisor } from './core-supervisor.js';
import { ordinaryPath } from './folder-dialog.js';
import { writeVaultZip } from './memory-zip.js';

/**
 * Opens the plain text log of a Vault's finished maintenance runs in the user's editor. Core
 * names the file; the renderer never supplies a path, and only a regular `.log` file reaches the
 * shell, which would run a program put in its place.
 */
async function openMaintenanceLog(
  core: CoreSupervisor,
  params: MemoryMainParams[MemoryMainCommand],
): Promise<{ completed: boolean; message: string }> {
  const input = params as MemoryVaultRequest;
  const status = await core.domains.memory('memory.vaults.get', {
    workspace_id: input.workspace_id ?? null,
  });
  const named = status.vaults.find((v) => v.scope === input.scope)?.maintenance_log;
  if (!named) return { completed: false, message: '还没有整理日志；第一次整理完成后才会写入。' };
  const log = ordinaryPath(named);
  const file = await stat(log).then(
    (info) => info.isFile(),
    () => false,
  );
  if (!file || !log.toLowerCase().endsWith('.log'))
    return { completed: false, message: '整理日志已不存在，请刷新后再试。' };
  const failure = await shell.openPath(log);
  return failure
    ? { completed: false, message: '无法打开整理日志，请检查文件是否仍存在。' }
    : { completed: true, message: '已打开整理日志' };
}

export async function dispatchMemoryCommand(
  event: IpcMainInvokeEvent,
  command: MemoryMainCommand,
  params: MemoryMainParams[MemoryMainCommand],
  core: CoreSupervisor | undefined,
  owner: (event: IpcMainInvokeEvent, purpose: string) => BrowserWindow,
): Promise<unknown> {
  if (core === undefined) throw new Error(CORE_NOT_READY_MESSAGE);
  if (command === 'memory.maintenance.open_log') return openMaintenanceLog(core, params);
  if (command === 'memory.vault.open_directory' || command === 'memory.vault.export') {
    const input = params as MemoryVaultRequest;
    const status = await core.domains.memory('memory.vaults.get', {
      workspace_id: input.workspace_id ?? null,
    });
    const vault = status.vaults.find((v) => v.scope === input.scope);
    if (!vault || !vault.exists)
      return { completed: false, message: 'Vault 尚未创建；第一次写入记忆时才会创建目录。' };
    if (command === 'memory.vault.open_directory') {
      const failure = await shell.openPath(ordinaryPath(vault.directory));
      if (failure) throw new Error('无法打开 Vault 目录，请检查目录是否仍存在与文件权限。');
      return { completed: true, message: '已打开 Vault 目录' };
    }
    const selected = await dialog.showSaveDialog(owner(event, 'Memory Vault export'), {
      title: '导出记忆 Vault',
      defaultPath: `aura-${input.scope}-memories.zip`,
      filters: [{ name: 'ZIP', extensions: ['zip'] }],
      properties: ['showOverwriteConfirmation', 'dontAddToRecent'],
    });
    if (selected.canceled || !selected.filePath) return { completed: false, message: '已取消导出' };
    const handle = await core.domains.memory('memory.export.prepare', input);
    try {
      await writeVaultZip(core.domains, handle, selected.filePath);
      return { completed: true, message: 'Vault 已完整导出为 ZIP' };
    } finally {
      try {
        await core.domains.memory('memory.export.close', { export_id: handle.export_id });
      } catch {
        /* Snapshot expires automatically; an export already saved must not be reported as undone. */
      }
    }
  }
  return core.domains.memory(
    command as MemoryCoreCommand,
    params as MemoryCoreParams[MemoryCoreCommand],
  );
}
