import type { ProjectionCompatibilityKey } from '@aura/contracts/generated';
import { AppIcon } from './app-icons';
import { isModelProjectionKind, isToolProjectionKind } from '../../shared/projection-validation';
import { projectionCompatibilityRouteKey } from '../../shared/projection-compatibility-validation';
import { useProjectionCompatibility } from './projection-compatibility-store';

type Surface =
  | 'workspace'
  | 'models'
  | 'tools'
  | 'environment'
  | 'browser'
  | 'security'
  | 'terminal'
  | 'memory'
  | 'commands'
  | 'plugins'
  | 'hooks'
  | 'mcp'
  | 'skills'
  | 'archive';
const title = (kind: ProjectionCompatibilityKey['projection_kind']): string => {
  if (kind === 'core_health') return 'Core 状态';
  if (kind === 'workspace_session') return '工作区与会话';
  if (kind === 'workspace_files') return '工作区文件';
  if (kind === 'pending_approval') return '待批准操作';
  if (kind === 'session_permission') return '会话权限';
  if (kind === 'security_runtime') return '安全状态';
  if (kind === 'terminal_state') return '终端状态';
  return isModelProjectionKind(kind) ? '模型配置' : '工具数据';
};

export function ProjectionCompatibilityNotice({
  surface,
  sessionId,
  terminalId,
}: {
  surface: Surface;
  sessionId?: string;
  terminalId?: string;
}) {
  const state = useProjectionCompatibility();
  const paused =
    state.snapshot?.paused_projections.filter((key) => {
      if (key.projection_kind === 'terminal_state')
        return (
          surface === 'terminal' &&
          key.projection_scope.type === 'terminal' &&
          key.projection_scope.session_id === sessionId &&
          key.projection_scope.terminal_id === terminalId
        );
      if (key.projection_kind === 'session_permission')
        return (
          (surface === 'security' || surface === 'workspace') &&
          key.projection_scope.type === 'session' &&
          key.projection_scope.session_id === sessionId
        );
      if (key.projection_kind === 'security_runtime' || key.projection_kind === 'pending_approval')
        return surface === 'security' || surface === 'workspace';
      if (key.projection_kind === 'core_health') return true;
      if (key.projection_scope.type === 'global') {
        if (surface === 'archive') return key.projection_kind === 'workspace_session';
        if (surface === 'workspace')
          return (
            key.projection_kind === 'workspace_session' || key.projection_kind === 'workspace_files'
          );
        return surface === 'models'
          ? isModelProjectionKind(key.projection_kind)
          : surface === 'tools' && isToolProjectionKind(key.projection_kind);
      }
      if (!state.activeRoutes.has(projectionCompatibilityRouteKey(key))) return false;
      if (surface === 'workspace')
        return (
          key.projection_scope.type !== 'session' || key.projection_scope.session_id === sessionId
        );
      return surface === 'models'
        ? isModelProjectionKind(key.projection_kind)
        : surface === 'tools' && isToolProjectionKind(key.projection_kind);
    }) ?? [];
  if (paused.length === 0) return null;
  const titles = [...new Set(paused.map((key) => title(key.projection_kind)))].join('、');
  return (
    <div
      className="runtime-notice projection-compatibility-notice"
      role="status"
      aria-live="polite"
    >
      <AppIcon name="warning" size={16} />
      <span>
        <strong>{titles}数据格式不兼容，已停止刷新。</strong>
        保留上一次有效数据；收到兼容数据后自动恢复。
      </span>
    </div>
  );
}
