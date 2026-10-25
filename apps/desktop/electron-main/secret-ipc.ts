import type { IpcMainInvokeEvent } from 'electron';
import type { CoreSupervisor } from './core-supervisor.js';
import { registerToolSecretIpc } from './tool-secret-ipc.js';
import { registerTerminalSecretIpc } from './terminal-secret-ipc.js';
import { registerMcpSecretIpc } from './mcp-secret-ipc.js';
export function registerSecretInputIpc(
  getSupervisor: () => CoreSupervisor | undefined,
  trust: (event: IpcMainInvokeEvent) => void,
) {
  registerToolSecretIpc(getSupervisor, trust);
  registerTerminalSecretIpc(getSupervisor, trust);
  registerMcpSecretIpc(getSupervisor, trust);
}
