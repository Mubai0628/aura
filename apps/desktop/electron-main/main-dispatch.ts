import { CORE_NOT_READY_MESSAGE } from './core-unavailable.js';
import type {
  BeginWorkspaceLifecycleResult,
  HostPreparationBeginResult,
  HostPreparationMutationInput,
  PrepareEditorLifecycleTransition,
} from '@aura/contracts/generated';
import { BrowserWindow, dialog, type IpcMainInvokeEvent } from 'electron';
import { selectWorkspaceDirectory } from './workspace-directory.js';
import { createAuraId } from './ids.js';
import type {
  P03CoreCommand,
  P03CoreParams,
  P03MainCommand,
  P03MainParams,
  P03MainResult,
} from '../shared/p03-validation.js';
import type {
  P04CoreCommand,
  P04CoreParams,
  P04MainCommand,
  P04MainParams,
  P04MainResult,
} from '../shared/p04-validation.js';
import type {
  P05MainParams,
  P05MainResult,
  P05NonSecretMainCommand,
  P05SecretMainCommand,
} from '../shared/p05-validation.js';
import type {
  P07CoreParams,
  P07MainCommand,
  P07MainParams,
  P07MainResult,
} from '../shared/p07-validation.js';
import { CoreSupervisor } from './core-supervisor.js';

const WORKSPACE_LIFECYCLE_PREPARE_EVENT_CHANNEL = 'aura:v1:workspace-lifecycle-prepare';

/**
 * The P03, P04, P05 and P07 commands Main handles itself or forwards to Core, and the delivery of a
 * workspace lifecycle preparation to the Renderer.
 */
export interface MainDispatchState {
  /** Main's current Core supervisor, read where the code read it before. */
  supervisor: () => CoreSupervisor | undefined;
  /** The active Aura window, read where the code read it before. */
  mainWindow: () => BrowserWindow | undefined;
  appendMainLog: (event: string, fields: Record<string, unknown>) => Promise<void>;
  rendererInstances: Map<number, string>;
  pendingLifecycleArms: Map<string, Promise<BeginWorkspaceLifecycleResult>>;
}

export function createMainDispatch(deps: MainDispatchState) {
  const { appendMainLog, rendererInstances, pendingLifecycleArms } = deps;
  const dispatchP05Main = async <Command extends P05NonSecretMainCommand>(
    command: Command,
    params: P05MainParams[Command],
  ): Promise<P05MainResult[Command]> => {
    const activeSupervisor = deps.supervisor();
    if (activeSupervisor === undefined) throw new Error(CORE_NOT_READY_MESSAGE);
    return activeSupervisor.requestP05(command, params);
  };

  const dispatchP05SecretMain = async <Command extends P05SecretMainCommand>(
    command: Command,
    params: P05MainParams[Command],
    secret: Buffer,
  ): Promise<P05MainResult[Command]> => {
    const activeSupervisor = deps.supervisor();
    if (activeSupervisor === undefined) {
      secret.fill(0);
      throw new Error(CORE_NOT_READY_MESSAGE);
    }
    return activeSupervisor.requestP05WithSecretInput(command, params, secret);
  };

  const dispatchP07Main = async <Command extends P07MainCommand>(
    event: IpcMainInvokeEvent,
    command: Command,
    params: P07MainParams[Command],
  ): Promise<P07MainResult[Command]> => {
    const activeSupervisor = deps.supervisor();
    if (activeSupervisor === undefined) throw new Error(CORE_NOT_READY_MESSAGE);

    return (await activeSupervisor.requestP07(
      command,
      params as unknown as P07CoreParams[Command],
    )) as P07MainResult[Command];
  };

  const requireActiveAuraWindow = (event: IpcMainInvokeEvent, operation: string): BrowserWindow => {
    const owner = BrowserWindow.fromWebContents(event.sender);
    if (owner === null || owner !== deps.mainWindow() || owner.isDestroyed()) {
      throw new Error(`${operation} requires the active Aura window.`);
    }
    return owner;
  };

  const dispatchP04Main = async <Command extends P04MainCommand>(
    event: IpcMainInvokeEvent,
    command: Command,
    params: P04MainParams[Command],
  ): Promise<P04MainResult[Command]> => {
    const activeSupervisor = deps.supervisor();
    if (activeSupervisor === undefined) throw new Error(CORE_NOT_READY_MESSAGE);
    if (
      command === 'security.host_preparation.prepare' ||
      command === 'security.host_preparation.repair' ||
      command === 'security.host_preparation.remove'
    ) {
      const owner = BrowserWindow.fromWebContents(event.sender);
      if (owner === null || owner !== deps.mainWindow() || owner.isDestroyed()) {
        throw new Error('Host Preparation requires the active Aura window.');
      }
      const begun = (await activeSupervisor.requestP04(
        command,
        params as HostPreparationMutationInput,
      )) as HostPreparationBeginResult;
      if (begun.approval === null || begun.approval === undefined) {
        return begun.status as P04MainResult[Command];
      }
      if (
        begun.approval.allowed_decisions.length !== 2 ||
        begun.approval.allowed_decisions[0] !== 'allow_once' ||
        begun.approval.allowed_decisions[1] !== 'deny'
      ) {
        throw new Error('Core returned an invalid Host Preparation ApprovalView.');
      }
      const operation = begun.status.active_operation;
      if (operation === null || operation === undefined || operation.state !== 'prepared') {
        throw new Error('Core returned a Host Preparation approval without a prepared operation.');
      }
      const details = [
        begun.approval.actual_operation_summary,
        `来源：${begun.approval.source_summary}`,
        `计划到期：${operation.expires_at}`,
        operation.add_all_application_packages_ace
          ? '• 添加 ALL APPLICATION PACKAGES 精确兼容 ACE'
          : undefined,
        operation.add_all_restricted_application_packages_ace
          ? '• 添加 ALL RESTRICTED APPLICATION PACKAGES 精确兼容 ACE'
          : undefined,
        operation.update_null_device_security ? '• 更新或恢复 NUL 设备兼容安全描述符' : undefined,
      ]
        .filter((line): line is string => line !== undefined)
        .join('\n');
      let decision: 'allow_once' | 'deny' = 'deny';
      if (!owner.isDestroyed()) {
        const response = await dialog.showMessageBox(owner, {
          buttons: ['继续并请求管理员权限', '拒绝'],
          cancelId: 1,
          defaultId: 0,
          detail: details,
          message: begun.approval.title,
          noLink: true,
          title: 'Aura Host Preparation',
          type: 'warning',
        });
        decision = response.response === 0 ? 'allow_once' : 'deny';
      }
      return (await activeSupervisor.requestP04('security.host_preparation.resolve', {
        approval_id: begun.approval.id,
        decision,
      })) as P04MainResult[Command];
    }
    if (command === 'security.external_grant.attach_read_only') {
      const owner = BrowserWindow.fromWebContents(event.sender);
      if (owner === null || owner !== deps.mainWindow() || owner.isDestroyed()) {
        throw new Error('External file selection requires the active Aura window.');
      }
      const selection = await dialog.showOpenDialog(owner, {
        properties: ['openFile', 'dontAddToRecent'],
        title: '选择要附加到当前会话的只读文件',
      });
      if (selection.canceled || selection.filePaths.length === 0) {
        return null as P04MainResult[Command];
      }
      const selectedPath = selection.filePaths[0];
      if (selectedPath === undefined) return null as P04MainResult[Command];
      const rendererRequest = params as P04MainParams['security.external_grant.attach_read_only'];
      return (await activeSupervisor.requestP04(
        'security.external_grant.attach_read_only_selected',
        {
          selected_path: selectedPath,
          session_id: rendererRequest.session_id,
          workspace_id: rendererRequest.workspace_id,
        },
      )) as P04MainResult[Command];
    }
    return (await activeSupervisor.requestP04<P04CoreCommand>(
      command as P04CoreCommand,
      params as P04CoreParams[P04CoreCommand],
    )) as P04MainResult[Command];
  };

  const dispatchP03Main = async <Command extends P03MainCommand>(
    event: IpcMainInvokeEvent,
    command: Command,
    params: P03MainParams[Command],
  ): Promise<P03MainResult[Command]> => {
    const activeSupervisor = deps.supervisor();
    if (activeSupervisor === undefined) throw new Error(CORE_NOT_READY_MESSAGE);
    const rendererInstanceId = rendererInstances.get(event.sender.id);
    if (rendererInstanceId === undefined) {
      throw new Error('Current Renderer instance is not registered.');
    }

    if (command === 'workspace.select_directory') {
      return (await selectWorkspaceDirectory(
        requireActiveAuraWindow(event, 'Workspace directory selection'),
      )) as P03MainResult[Command];
    }

    if (command === 'workspace.archive') {
      const request = params as P03MainParams['workspace.archive'];
      return (await beginAndDeliverWorkspaceLifecycle(event, activeSupervisor, rendererInstanceId, {
        expected_workspace_revision: request.expected_workspace_revision,
        operation: { operation_kind: 'archive' },
        renderer_instance_id: rendererInstanceId,
        workspace_id: request.workspace_id,
      })) as P03MainResult[Command];
    }

    // The user confirmed the removal in the Renderer's dialog; this is where that
    // confirmation is issued. The Workspace must already be archived (Core refuses otherwise).
    if (command === 'workspace.delete') {
      const request = params as P03MainParams['workspace.delete'];
      const projection = await activeSupervisor.refreshWorkspaceSessionSnapshot();
      const workspace = projection.payload.workspaces.find(
        (candidate) => candidate.workspace_id === request.workspace_id,
      );
      if (workspace === undefined) throw new Error('workspace.not_found: 这个工作区已不存在。');
      return (await beginAndDeliverWorkspaceLifecycle(event, activeSupervisor, rendererInstanceId, {
        expected_workspace_revision: workspace.revision,
        operation: {
          operation_kind: 'permanent_delete',
          delete_confirmation_id: createAuraId('dcn'),
        },
        renderer_instance_id: rendererInstanceId,
        workspace_id: request.workspace_id,
      })) as P03MainResult[Command];
    }

    if (command === 'workspace.relocate') {
      const request = params as P03MainParams['workspace.relocate'];
      return (await beginAndDeliverWorkspaceLifecycle(event, activeSupervisor, rendererInstanceId, {
        expected_workspace_revision: request.expected_workspace_revision,
        operation: {
          confirmed_same_logical_project:
            request.confirmation === 'user_confirmed_same_logical_project',
          operation_kind: 'relocate',
          selected_target_root: request.selected_root,
        },
        renderer_instance_id: rendererInstanceId,
        workspace_id: request.workspace_id,
      })) as P03MainResult[Command];
    }

    if (command === 'workspace.lifecycle.ack') {
      const request = params as P03MainParams['workspace.lifecycle.ack'];
      await pendingLifecycleArms.get(request.operation_id);
      return (await activeSupervisor.requestP03('workspace.lifecycle.ack', {
        ...request,
        renderer_instance_id: rendererInstanceId,
      })) as P03MainResult[Command];
    }

    if (command === 'workspace.lifecycle.issue_discard_confirmation') {
      const request = params as P03MainParams['workspace.lifecycle.issue_discard_confirmation'];
      return (await activeSupervisor.requestP03('workspace.lifecycle.issue_discard_confirmation', {
        ...request,
        renderer_instance_id: rendererInstanceId,
      })) as P03MainResult[Command];
    }

    if (command === 'workspace.lifecycle.resolve') {
      const request = params as P03MainParams['workspace.lifecycle.resolve'];
      return (await activeSupervisor.requestP03('workspace.lifecycle.resolve', {
        ...request,
        renderer_instance_id: rendererInstanceId,
      })) as P03MainResult[Command];
    }

    return (await activeSupervisor.requestP03<P03CoreCommand>(
      command as P03CoreCommand,
      params as P03CoreParams[P03CoreCommand],
    )) as P03MainResult[Command];
  };

  const beginAndDeliverWorkspaceLifecycle = async (
    event: IpcMainInvokeEvent,
    activeSupervisor: CoreSupervisor,
    rendererInstanceId: string,
    input: P03CoreParams['workspace.lifecycle.begin'],
  ): Promise<BeginWorkspaceLifecycleResult> => {
    const begin = await activeSupervisor.requestP03('workspace.lifecycle.begin', input);
    const prepare = await activeSupervisor.requestP03('workspace.lifecycle.prepare', {
      operation_id: begin.operation_id,
    });
    if (
      event.sender.isDestroyed() ||
      rendererInstances.get(event.sender.id) !== rendererInstanceId
    ) {
      await notifyRendererUnavailable(rendererInstanceId);
      throw new Error('Renderer changed before lifecycle preparation could be delivered.');
    }

    try {
      event.sender.send(
        WORKSPACE_LIFECYCLE_PREPARE_EVENT_CHANNEL,
        structuredClone(prepare satisfies PrepareEditorLifecycleTransition),
      );
    } catch (error) {
      await notifyRendererUnavailable(rendererInstanceId);
      throw error;
    }

    const arm = activeSupervisor.requestP03('workspace.lifecycle.prepare_delivered', {
      expected_lifecycle_revision: begin.lifecycle_revision,
      operation_id: begin.operation_id,
      renderer_instance_id: rendererInstanceId,
    });
    pendingLifecycleArms.set(begin.operation_id, arm);
    try {
      return await arm;
    } catch (error) {
      await notifyRendererUnavailable(rendererInstanceId);
      throw error;
    } finally {
      if (pendingLifecycleArms.get(begin.operation_id) === arm) {
        pendingLifecycleArms.delete(begin.operation_id);
      }
    }
  };

  const notifyRendererUnavailable = async (rendererInstanceId: string): Promise<void> => {
    const activeSupervisor = deps.supervisor();
    if (activeSupervisor === undefined) return;
    try {
      await activeSupervisor.requestP03('workspace.lifecycle.renderer_unavailable', {
        renderer_instance_id: rendererInstanceId,
      });
    } catch (error) {
      await appendMainLog('renderer_lifecycle_cleanup_failed', {
        message: error instanceof Error ? error.message : String(error),
        rendererInstanceId,
      }).catch(() => undefined);
    }
  };
  return {
    dispatchP05Main,
    dispatchP05SecretMain,
    dispatchP07Main,
    requireActiveAuraWindow,
    dispatchP04Main,
    dispatchP03Main,
    notifyRendererUnavailable,
  };
}
