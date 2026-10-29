import type { CoreLifecycleState } from '@aura/contracts/generated';
import { useEffect, useRef, useState } from 'react';

import { AppEmptyState } from './AppEmptyState';
import { AppSidebar } from './AppSidebar';
import { NewSessionView } from './NewSessionView';
import { userMessage } from './agent-session/user-message';
import {
  createLocalConversation,
  discardEmptyConversations,
  getComposerDraft,
  removeComposerDraft,
  updateComposerDraft,
  useComposerDrafts,
} from './agent-session/composer-drafts';
import { flushPendingTitles } from './agent-session/session-titles';
import { setReadinessConnection } from './runtime-readiness-store';
import type {
  AppScreen,
  CoreStatusPresentation,
  SettingsSection,
  WorkspaceNavigationItem,
} from './app-shell-types';
import { useCoreHealth } from './core-health-store';
import { useStartupSplash, type StartupPhase } from './startup-splash';
import { NotificationViewport } from './NotificationCenter';
import { UpdateNotifications } from './app-updates/UpdateNotifications';
import { useNotificationCenter } from './notification-center-store';
import { useMainErrorNotice } from './useMainErrorNotice';
import { useSecurityControl } from './security-control-store';
import { PermissionModeFeedback } from './PermissionModeFeedback';
import { SettingsView } from './SettingsView';
import type { WorkspaceLayoutPreferences } from './ui-preferences';
import { readWorkspaceLayoutPreferences, writeWorkspaceLayoutPreferences } from './ui-preferences';
import { WorkspaceView } from './WorkspaceView';
import { SessionGrantDialog } from './security/SessionGrantDialog';
import { WorkspaceShellSplitPane } from './WorkspaceShellSplitPane';
import { useWorkspaceSessions } from './workspace-session-store';
import { answerWorkspaceLifecycle } from './session-lifecycle/workspace-lifecycle';
import { busyPresentation } from './core-busy';
import { readOpenSession, rememberOpenSession } from './open-session';
import { frontOf, useDesktopNotifications } from './notification-settings/useDesktopNotifications';

const CORE_PRESENTATION: Record<CoreLifecycleState, CoreStatusPresentation> = {
  app_crash_escalated: { label: 'Launcher 正在接管恢复', tone: 'danger' },
  incompatible: { label: '组件版本不兼容', tone: 'danger' },
  not_started: { label: 'Core 尚未启动', tone: 'muted' },
  ready: { label: 'Aura Core 已就绪', tone: 'success' },
  recovery_required: { label: 'Aura 已进入诊断模式', tone: 'danger' },
  restarting: { label: 'Core 正在恢复', tone: 'warning' },
  starting: { label: '正在启动 Aura Core', tone: 'warning' },
  unavailable: { label: '无法启动 Aura Core', tone: 'danger' },
};

/** The background store check found damage (Core's status says what to do). */
const STORE_CHECK_FAILED = 'core.store_check_failed';

export function App() {
  const drafts = useComposerDrafts();
  const health = useCoreHealth();
  const workspaceSessions = useWorkspaceSessions();
  const projection = health.projection;
  const coreState = projection?.payload.state ?? 'not_started';
  useEffect(() => {
    setReadinessConnection(
      `${projection?.runtime_instance_id ?? 'disconnected'}:${coreState === 'ready'}`,
    );
    if (coreState === 'ready') void flushPendingTitles();
  }, [projection?.runtime_instance_id, coreState]);
  // Archiving or removing a Workspace asks this window for unsaved editor buffers.
  useEffect(() => answerWorkspaceLifecycle(), []);
  // The startup screen loops while Core starts and steps aside once it is ready or cannot start.
  const startupPhase: StartupPhase =
    health.error !== undefined
      ? 'failed'
      : health.loading || ['not_started', 'starting', 'restarting'].includes(coreState)
        ? 'waiting'
        : coreState === 'ready'
          ? 'ready'
          : 'failed';
  useStartupSplash(startupPhase);
  const busy = coreState === 'ready' ? projection?.payload.busy : undefined;
  const waiting = coreState === 'ready' ? projection?.payload.waiting : undefined;
  const corePresentation = health.loading
    ? { label: '正在连接 Aura Core', tone: 'warning' as const }
    : waiting
      ? { label: 'Aura Core 正在处理', tone: 'warning' as const, waitingSince: waiting.since }
      : projection?.payload.status_code === STORE_CHECK_FAILED
        ? {
            label: '数据库检查发现问题',
            tone: 'danger' as const,
            detail: projection.payload.status_message,
          }
        : busy
          ? busyPresentation(busy)
          : CORE_PRESENTATION[coreState];
  const actualWorkspaces = workspaceSessions.projection?.payload.workspaces ?? [];
  const actualSessions = workspaceSessions.projection?.payload.sessions ?? [];
  // The frozen design keeps the main list for active work: the main list only
  // *offers* archive, and archived Workspaces are handled in 设置 → 归档.
  const visibleWorkspaces = actualWorkspaces.filter((workspace) => workspace.state !== 'archived');

  const projectedNavigation: WorkspaceNavigationItem[] = visibleWorkspaces.map((workspace) => ({
    id: workspace.workspace_id,
    name: workspace.display_name,
    root: workspace.selected_root,
    sessions: [
      ...actualSessions
        .filter((session) => session.workspace_id === workspace.workspace_id)
        // An archived conversation is listed in 设置 → 归档, not here.
        .filter((session) => session.state !== 'archived')
        .filter(
          (session) =>
            !drafts.some(
              (draft) => draft.workspaceId && draft.serverSessionId === session.session_id,
            ),
        )
        .map((session) => ({
          id: session.session_id,
          state: session.state,
          title: session.title,
        })),
      ...drafts
        .filter((draft) => draft.workspaceId === workspace.workspace_id)
        .map((draft) => ({ id: draft.id, state: 'draft', title: '新会话' })),
    ],
  }));

  const navigation = projectedNavigation;
  const [screen, setScreen] = useState<AppScreen>('workspace');
  const [settingsSection, setSettingsSection] = useState<SettingsSection>('models');
  const [workspaceLayout, setWorkspaceLayout] = useState(readWorkspaceLayoutPreferences);
  const workspaceLayoutRef = useRef(workspaceLayout);
  // The conversation that was open when Aura last closed, however it closed.
  const [restored] = useState(readOpenSession);
  const [selectedWorkspaceId, setSelectedWorkspaceId] = useState(restored?.workspaceId ?? '');
  const [selectedSessionId, setSelectedSessionId] = useState(restored?.sessionId ?? '');
  const [grantsOpen, setGrantsOpen] = useState(false);
  const {
    dismiss: dismissNotification,
    items: notifications,
    publish: publishNotification,
    withdraw: withdrawNotifications,
  } = useNotificationCenter();
  const mainErrorNotice = useMainErrorNotice();

  const selectedWorkspace =
    navigation.find((workspace) => workspace.id === selectedWorkspaceId) ?? navigation[0];
  const selectedSession =
    selectedWorkspace?.sessions.find((session) => session.id === selectedSessionId) ??
    selectedWorkspace?.sessions[0];
  const activeWorkspace = visibleWorkspaces.find(
    (workspace) => workspace.workspace_id === selectedWorkspace?.id,
  );
  const activeSession = actualSessions.find(
    (session) =>
      session.workspace_id === activeWorkspace?.workspace_id &&
      session.session_id === selectedSession?.id,
  );
  const security = useSecurityControl(activeWorkspace?.workspace_id, activeSession?.session_id);
  // A workspace without
  // conversations opens its new-conversation page, as its + does.
  const emptyWorkspaceId =
    screen === 'workspace' &&
    !workspaceSessions.loading &&
    workspaceSessions.error === undefined &&
    selectedWorkspace !== undefined &&
    selectedWorkspace.sessions.length === 0
      ? selectedWorkspace.id
      : undefined;
  // Its draft is kept once the user writes in it (`updateComposerDraft` adds it then).
  const blankDraft = emptyWorkspaceId
    ? {
        id: `draft_${emptyWorkspaceId}`,
        text: '',
        needsReview: false,
        workspaceId: emptyWorkspaceId,
      }
    : undefined;
  // What is written in it is stored by the draft's id alone, so the page keeps
  // its workspace here; the conversation is created when the first message is sent.
  const shownDraft = selectedSession
    ? getComposerDraft(selectedSession.id)
    : blankDraft
      ? { ...blankDraft, ...getComposerDraft(blankDraft.id), workspaceId: blankDraft.workspaceId }
      : undefined;
  // Kept the moment a conversation opens; one not yet sent is not kept.
  const openWorkspaceId = selectedWorkspace?.id;
  const openSessionId = selectedSession?.state === 'draft' ? undefined : selectedSession?.id;
  useEffect(() => {
    if (openWorkspaceId && openSessionId) {
      rememberOpenSession({ workspaceId: openWorkspaceId, sessionId: openSessionId });
    }
  }, [openWorkspaceId, openSessionId]);

  useEffect(() => {
    if (security.error === undefined) {
      withdrawNotifications('security:');
      return;
    }
    // A read that kept failing is not an operation the user started; it says what is
    // missing, what that blocks and that Aura keeps trying (the notice goes once it reads).
    publishNotification(
      security.readFailed
        ? {
            dedupeKey: `security:read:${security.error}`,
            message: userMessage(
              security.error,
              '多次重试后仍没能从 Aura Core 读到这个会话的权限与沙箱状态。读到之前，输入框里的权限暂不能修改；Aura 会在后台继续重读，读到后这条提示自动消失。',
            ),
            title: '权限状态暂时读不到',
            tone: 'danger',
          }
        : {
            dedupeKey: `security:${security.error}`,
            message: userMessage(security.error, '权限操作未完成，请检查当前状态后重试。'),
            title: '权限与沙箱操作未完成',
            tone: 'danger',
          },
    );
  }, [publishNotification, security.error, security.readFailed, withdrawNotifications]);
  const runtimeNotice =
    coreState !== 'ready'
      ? {
          message: userMessage(
            health.error ?? projection?.payload.status_message,
            'Aura Core 当前无法提供完整能力。',
          ),
          title: corePresentation.label,
          tone: corePresentation.tone,
        }
      : workspaceSessions.error === undefined
        ? undefined
        : {
            message: userMessage(
              workspaceSessions.error,
              '工作区与会话列表暂时读不到，请检查 Aura Core 状态后重试。',
            ),
            title: '工作区列表暂不可用',
            tone: 'danger' as const,
          };

  const selectWorkspace = (workspaceId: string) => {
    discardEmptyConversations();
    const workspace = navigation.find((item) => item.id === workspaceId);
    setSelectedWorkspaceId(workspaceId);
    setSelectedSessionId(workspace?.sessions[0]?.id ?? '');
    setScreen('workspace');
  };

  const selectSession = (workspaceId: string, sessionId: string) => {
    discardEmptyConversations();
    setSelectedWorkspaceId(workspaceId);
    setSelectedSessionId(sessionId);
    setScreen('workspace');
  };

  /** A branch just made opens at once, before the sidebar lists it; the conversation a
   * branch came from opens only while it still exists. */
  const openSession = (workspaceId: string, sessionId: string, mustExist: boolean) => {
    const exists = navigation.some((workspace) =>
      workspace.sessions.some((session) => session.id === sessionId),
    );
    if (mustExist && !exists) return false;
    selectSession(workspaceId, sessionId);
    return true;
  };

  const openSettings = (section: SettingsSection) => {
    discardEmptyConversations();
    setSettingsSection(section);
    setScreen('settings');
  };

  useDesktopNotifications(
    frontOf({ screen, section: settingsSection, session: activeSession?.session_id ?? null }),
    (target) => {
      if (target.kind === 'updates') return openSettings('about');
      const session = actualSessions.find((each) => each.session_id === target.sessionId);
      if (session !== undefined) selectSession(session.workspace_id, session.session_id);
    },
  );

  const updateWorkspaceLayout = (
    changes: Partial<WorkspaceLayoutPreferences>,
    persist: boolean,
  ) => {
    const next = { ...workspaceLayoutRef.current, ...changes };
    workspaceLayoutRef.current = next;
    setWorkspaceLayout(next);
    if (persist) writeWorkspaceLayoutPreferences(next);
  };

  return (
    <div className="app-shell">
      {/* Owned here, not inside the workspace view: re-creating that view must
          never mount a second modal layer, and exactly one dialog can exist. */}
      <SessionGrantDialog
        open={grantsOpen && activeSession !== undefined}
        security={security}
        sessionId={activeSession?.session_id ?? ''}
        onClose={() => setGrantsOpen(false)}
      />
      {screen === 'workspace' ? (
        <WorkspaceShellSplitPane
          left={
            <AppSidebar
              activeScreen={screen}
              coreStatus={corePresentation}
              demoMode={false}
              onNotify={publishNotification}
              onOpenSettings={() => openSettings(settingsSection)}
              onNewSession={(workspaceId) => {
                discardEmptyConversations();
                setSelectedWorkspaceId(workspaceId);
                setSelectedSessionId(createLocalConversation(workspaceId));
                setScreen('workspace');
              }}
              onSelectSession={selectSession}
              onSelectWorkspace={selectWorkspace}
              selectedSessionId={selectedSession?.id ?? ''}
              selectedWorkspaceId={selectedWorkspace?.id ?? ''}
              workspaces={navigation}
            />
          }
          leftPanelOpen={workspaceLayout.leftPanelOpen}
          leftPanelWidth={workspaceLayout.leftPanelWidth}
          onCloseLeftPanel={() => updateWorkspaceLayout({ leftPanelOpen: false }, true)}
          onLeftPanelWidthChange={(leftPanelWidth) =>
            updateWorkspaceLayout({ leftPanelWidth }, false)
          }
          onLeftPanelWidthCommit={(leftPanelWidth) =>
            updateWorkspaceLayout({ leftPanelWidth }, true)
          }
          onOpenLeftPanel={() => updateWorkspaceLayout({ leftPanelOpen: true }, true)}
        >
          {shownDraft?.workspaceId ? (
            <NewSessionView
              key={shownDraft.id}
              draft={shownDraft}
              onOpenSettings={openSettings}
              onAccepted={(localId, sessionId) => {
                removeComposerDraft(localId);
                setSelectedSessionId((current) =>
                  current === localId || current === '' ? sessionId : current,
                );
              }}
              onInspect={(sessionId) => {
                const draft = getComposerDraft(shownDraft.id) ?? shownDraft;
                updateComposerDraft(sessionId, { text: draft.text, needsReview: true });
                removeComposerDraft(draft.id);
                setSelectedSessionId(sessionId);
              }}
            />
          ) : selectedSession && activeSession ? (
            <WorkspaceView
              workspaceId={activeSession.workspace_id}
              workspaceRoot={selectedWorkspace?.root ?? ''}
              onNotify={publishNotification}
              key={selectedSession.id}
              demoMode={false}
              onOpenModelSettings={() => openSettings('models')}
              onOpenSettings={openSettings}
              security={security}
              onSetPermissionMode={security.setMode}
              onOpenSecuritySettings={() => setGrantsOpen(true)}
              permissionMode={security.permission?.mode}
              permissionTransition={security.permission?.transition}
              rightPanelOpen={workspaceLayout.rightPanelOpen}
              rightPanelWidth={workspaceLayout.rightPanelWidth}
              runtimeNotice={runtimeNotice}
              selectedSession={selectedSession}
              setRightPanelOpen={(rightPanelOpen) =>
                updateWorkspaceLayout({ rightPanelOpen }, true)
              }
              setRightPanelWidth={(rightPanelWidth) =>
                updateWorkspaceLayout({ rightPanelWidth }, false)
              }
              writeRightPanelWidth={(rightPanelWidth) =>
                updateWorkspaceLayout({ rightPanelWidth }, true)
              }
              onOpenSession={(sessionId, mustExist) =>
                openSession(activeSession.workspace_id, sessionId, mustExist)
              }
              workspaces={navigation}
            />
          ) : (
            <AppEmptyState
              loading={workspaceSessions.loading}
              starting={
                startupPhase !== 'waiting'
                  ? undefined
                  : CORE_PRESENTATION[coreState === 'restarting' ? 'restarting' : 'starting'].label
              }
              workspaceName={selectedWorkspace?.name}
              error={workspaceSessions.error?.message}
              runtimeNotice={runtimeNotice}
              onCreated={selectWorkspace}
              onNewSession={
                selectedWorkspace
                  ? () => {
                      discardEmptyConversations();
                      setSelectedWorkspaceId(selectedWorkspace.id);
                      setSelectedSessionId(createLocalConversation(selectedWorkspace.id));
                      setScreen('workspace');
                    }
                  : undefined
              }
            />
          )}
        </WorkspaceShellSplitPane>
      ) : (
        <SettingsView
          coreReady={!health.loading && coreState === 'ready'}
          coreStatus={corePresentation}
          onChangeSection={setSettingsSection}
          onClose={() => setScreen('workspace')}
          onNotify={publishNotification}
          onWithdrawNotice={withdrawNotifications}
          section={settingsSection}
          security={security}
          workspaceId={
            selectedWorkspace?.id as import('@aura/contracts/generated').WorkspaceId | undefined
          }
        />
      )}
      <UpdateNotifications onNotify={publishNotification} onWithdraw={withdrawNotifications} />
      <NotificationViewport
        items={mainErrorNotice.item ? [...notifications, mainErrorNotice.item] : notifications}
        onDismiss={(id) => {
          dismissNotification(id);
          mainErrorNotice.dismiss(id);
        }}
      />
      <PermissionModeFeedback security={security} />
    </div>
  );
}
