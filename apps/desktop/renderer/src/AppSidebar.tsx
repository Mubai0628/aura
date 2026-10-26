import { useEffect, useState } from 'react';

import { AppIcon } from './app-icons';
import { WorkspaceIndexBadge } from './files/workspace-indexing';
import { readExpandedWorkspaces, rememberExpandedWorkspaces } from './sidebar-expansion';
import { WorkspaceActions } from './WorkspaceActions';
import { SessionList } from './SessionList';
import { SidebarItemMenu } from './SidebarItemMenu';
import type { AppScreen, CoreStatusPresentation, WorkspaceNavigationItem } from './app-shell-types';
import type { NotificationInput } from './notification-center-store';
import { useSessionActivity } from './session-activity';

interface AppSidebarProps {
  activeScreen: AppScreen;
  coreStatus: CoreStatusPresentation;
  demoMode: boolean;
  onNotify: (notice: NotificationInput) => void;
  onOpenSettings: () => void;
  onSelectWorkspace: (workspaceId: string) => void;
  onSelectSession: (workspaceId: string, sessionId: string) => void;
  onNewSession: (workspaceId: string) => void;
  selectedSessionId: string;
  selectedWorkspaceId: string;
  workspaces: WorkspaceNavigationItem[];
}

export function AppSidebar({
  activeScreen,
  coreStatus,
  demoMode,
  onNotify,
  onOpenSettings,
  onSelectWorkspace,
  onSelectSession,
  onNewSession,
  selectedSessionId,
  selectedWorkspaceId,
  workspaces,
}: AppSidebarProps) {
  const activity = useSessionActivity();
  // What the user left open or closed wins over the selection, including a closed selected one.
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(
    () => new Set(readExpandedWorkspaces() ?? [selectedWorkspaceId].filter(Boolean)),
  );
  const [previousSelection, setPreviousSelection] = useState(selectedWorkspaceId);
  if (previousSelection !== selectedWorkspaceId) {
    setPreviousSelection(selectedWorkspaceId);
    if (selectedWorkspaceId) setExpanded((current) => new Set([...current, selectedWorkspaceId]));
  }
  // Not while the list is still loading: an empty list would forget every open workspace.
  useEffect(() => {
    if (workspaces.length === 0) return;
    rememberExpandedWorkspaces(
      workspaces.map((workspace) => workspace.id).filter((id) => expanded.has(id)),
    );
  }, [expanded, workspaces]);

  const toggleWorkspace = (workspaceId: string) => {
    setExpanded((current) => {
      const next = new Set(current);
      if (next.has(workspaceId)) {
        next.delete(workspaceId);
      } else {
        next.add(workspaceId);
      }
      return next;
    });
  };

  return (
    <aside aria-label="工作区导航" className="app-sidebar" id="app-sidebar">
      <div className="sidebar-heading">
        <span>工作区</span>
        {demoMode ? (
          <span className="demo-chip">演示</span>
        ) : (
          <WorkspaceActions onCreated={onSelectWorkspace} onNotify={onNotify} />
        )}
      </div>

      <nav aria-label="工作区和会话" className="workspace-navigation">
        {workspaces.map((workspace) => {
          const isExpanded = expanded.has(workspace.id);
          const isActiveWorkspace = selectedWorkspaceId === workspace.id;

          return (
            <div className="workspace-tree" key={workspace.id}>
              <div className={isActiveWorkspace ? 'workspace-row is-active' : 'workspace-row'}>
                <button
                  aria-expanded={isExpanded}
                  aria-label={`${isExpanded ? '收起' : '展开'} ${workspace.name}`}
                  className="tree-toggle"
                  onClick={() => toggleWorkspace(workspace.id)}
                  type="button"
                >
                  <AppIcon name={isExpanded ? 'chevron-down' : 'chevron-right'} size={14} />
                </button>
                <button
                  className="workspace-select"
                  onClick={() => onSelectWorkspace(workspace.id)}
                  title={workspace.root}
                  type="button"
                >
                  <AppIcon name="workspace" />
                  <span>{workspace.name}</span>
                  {demoMode ? null : <WorkspaceIndexBadge workspaceId={workspace.id} />}
                </button>
                <SidebarItemMenu workspace={workspace} />
                <button
                  aria-label={`在 ${workspace.name} 新建会话`}
                  title="新建会话"
                  className="icon-button workspace-new-session"
                  type="button"
                  onClick={() => onNewSession(workspace.id)}
                >
                  <AppIcon name="plus" size={16} />
                </button>
              </div>

              {isExpanded ? (
                <SessionList
                  activeScreen={activeScreen}
                  activity={activity}
                  onSelectSession={onSelectSession}
                  selectedSessionId={selectedSessionId}
                  workspace={workspace}
                />
              ) : null}
            </div>
          );
        })}
      </nav>

      <div className="sidebar-footer">
        <button
          aria-current={activeScreen === 'settings' ? 'page' : undefined}
          className={activeScreen === 'settings' ? 'settings-link is-active' : 'settings-link'}
          onClick={onOpenSettings}
          type="button"
        >
          <AppIcon name="settings" />
          <span>设置</span>
        </button>
        <p className={`core-footer-status state-${coreStatus.tone}`} title={coreStatus.detail}>
          <span aria-hidden="true" />
          <span>{coreStatus.label}</span>
        </p>
        <p className="local-only-note">
          <AppIcon name="shield" size={14} />
          <span>本地运行 · 无需登录</span>
        </p>
      </div>
    </aside>
  );
}
