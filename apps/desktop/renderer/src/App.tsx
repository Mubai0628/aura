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
import { moveKeptDrafts } from './agent-session/kept-draft';
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
import { usePluginNotices } from './plugin-ui/plugin-ui';
import { PluginTheme } from './plugin-theme/PluginTheme';
import { useMainErrorNotice } from './useMainErrorNotice';
import { useSecurityControl } from './security-control-store';
import { PermissionModeFeedback } from './PermissionModeFeedback';
import { SettingsView } from './SettingsView';
import type { WorkspaceLayoutPreferences } from './ui-preferences';
import { readWorkspaceLayoutPreferences, writeWorkspaceLayoutPreferences } from './ui-preferences';
import { WorkspaceView } from './WorkspaceView';
import { SessionGrantDialog } from './security/SessionGrantDialog';
import { SubagentGrantDialog } from './agent-team/SubagentGrantDialog';
import { useSubagentGrants } from './agent-team/subagent-grants';
import { WorkspaceShellSplitPane } from './WorkspaceShellSplitPane';
import { useWorkspaceSessions } from './workspace-session-store';
import { answerWorkspaceLifecycle } from './session-lifecycle/workspace-lifecycle';
import { busyPresentation } from './core-busy';
import { readOpenSession, rememberOpenSession } from './open-session';
import { frontOf, useDesktopNotifications } from './notification-settings/useDesktopNotifications';
import { listedWorkspaces, worktreeParents } from './worktree-links';

const CORE_PRESENTATION: Record<CoreLifecycleState, CoreStatusPresentation> = {
  app_crash_escalated: { label: '启动器正在接管恢复', tone: 'danger' },
  incompatible: { label: '组件版本不兼容', tone: 'danger' },
  not_started: { label: 'Core 尚未启动', tone: 'muted' },
  ready: { label: 'Aura Core 已就绪', tone: 'success' },
  recovery_required: { label: 'Aura 已进入诊断模式', tone: 'danger' },
  restarting: { label: 'Core 正在恢复', tone: 'warning' },
  starting: { label: '正在启动 Aura Core', tone: 'warning' },
  unavailable: { label: '无法启动 Aura Core', tone: 'danger' },
};

/** What a Core that is not ready means for the user, when its own status says nothing readable. */
const CORE_NOT_READY: Partial<Record<CoreLifecycleState, string>> = {
  not_started:
    'Aura Core 还没有启动，暂时不能发送消息或读取会话；它启动就绪后会自动恢复，不需要操作。',
  starting: 'Aura Core 正在启动，暂时不能发送消息或读取会话；就绪后会自动恢复，不需要操作。',
  restarting:
    'Aura Core 正在重新启动，暂时不能发送消息或读取会话；就绪后会自动恢复，没有完成的操作不会重放。',
  unavailable:
    'Aura Core 没能启动，暂时不能发送消息或读取会话。请重启 Aura；仍然如此，请查看 Core 日志。',
  incompatible: 'Aura 的组件版本不一致，Core 没有启动。请重新安装或更新 Aura。',
  app_crash_escalated:
    'Aura Core 反复意外退出，Aura 将关闭并交给启动器恢复；启动器会显示恢复窗口，可以重新启动、进入安全模式或回到上一版本。',
  recovery_required:
    'Aura Core 发现需要恢复的数据，已进入诊断模式，暂时不能发送消息或读取会话；请到设置中查看恢复选项。',
};

/** The start page and the notice when Core gave no workspace list; the store asks again. */
const WORKSPACE_LIST_UNREAD =
  '工作区和会话列表还没从 Aura Core 读到（Core 可能还在启动或正忙），列表暂时为空；Aura 会自动再读，长时间仍为空请重启 Aura。';

/** Core's state could not be read; the store reads it again. */
const HEALTH_UNREAD =
  '这次没能及时从 Aura Core 读到状态（Core 可能正忙），暂时不能确认 Core 是否就绪；Aura 会自动再读，读到后这条提示自动消失。';

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
  // What a Core that is not ready means, in Core's own Chinese words when it gave some.
  // A state read that failed says nothing about Core: it is not a Core that has not started.
  const healthUnread = !health.loading && health.error !== undefined && projection === undefined;
  const notReadyMessage = healthUnread
    ? HEALTH_UNREAD
    : userMessage(
        projection?.payload.status_message,
        CORE_NOT_READY[coreState] ?? 'Aura Core 当前无法提供完整能力。',
      );
  const corePresentation: CoreStatusPresentation = health.loading
    ? { label: '正在连接 Aura Core', tone: 'warning' as const }
    : healthUnread
      ? { label: 'Aura Core 状态暂时读不到', tone: 'warning' as const, detail: HEALTH_UNREAD }
      : waiting
        ? { label: 'Aura Core 正在处理', tone: 'warning' as const }
        : projection?.payload.status_code === STORE_CHECK_FAILED
          ? {
              label: '数据库检查发现问题',
              tone: 'danger' as const,
              detail: projection.payload.status_message,
            }
          : busy
            ? busyPresentation(busy)
            : coreState === 'ready'
              ? CORE_PRESENTATION.ready
              : { ...CORE_PRESENTATION[coreState], detail: notReadyMessage };
  const actualWorkspaces = workspaceSessions.projection?.payload.workspaces ?? [];
  const actualSessions = workspaceSessions.projection?.payload.sessions ?? [];
  // The frozen design keeps the main list for active work: the main list only
  // *offers* archive, and archived Workspaces are handled in 设置 → 归档.
  // A worktree's workspace is archived with its worktree; the user's conversation in it stays.
  const worktrees = worktreeParents(workspaceSessions.projection?.payload);
  const visibleWorkspaces = listedWorkspaces(
    actualWorkspaces.filter(
      (workspace) => workspace.state !== 'archived' || worktrees.has(workspace.workspace_id),
    ),
    actualSessions,
    worktrees,
  );

  const projectedNavigation: WorkspaceNavigationItem[] = visibleWorkspaces.map((workspace) => ({
    id: workspace.workspace_id,
    name: workspace.display_name,
    root: workspace.selected_root,
    offline: workspace.state === 'offline',
    sessions: [
      ...actualSessions
        .filter((session) => session.workspace_id === workspace.workspace_id)
        // An archived conversation is listed in 设置 → 归档, not here; a sub-agent's is shown in
        // the conversation that started it.
        .filter((session) => session.state !== 'archived' && session.state !== 'subagent')
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
  const subagentGrants = useSubagentGrants();
  const {
    dismiss: dismissNotification,
    items: notifications,
    publish: publishNotification,
    withdraw: withdrawNotifications,
  } = useNotificationCenter();
  usePluginNotices(publishNotification);
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

  // Damage the background store check found stays in a notice, with what to do, until it is
  // closed or the check passes again.
  const storeDamage =
    projection?.payload.status_code === STORE_CHECK_FAILED
      ? userMessage(
          projection.payload.status_message,
          '数据库检查发现了问题，建议先备份 Aura 的数据文件夹。',
        )
      : undefined;
  useEffect(() => {
    if (storeDamage === undefined) {
      withdrawNotifications('core:store-check:');
      return;
    }
    publishNotification({
      dedupeKey: `core:store-check:${storeDamage}`,
      message: storeDamage,
      title: '数据库检查发现问题',
      tone: 'danger',
    });
  }, [publishNotification, storeDamage, withdrawNotifications]);

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
          message: notReadyMessage,
          title: corePresentation.label,
          tone: corePresentation.tone,
        }
      : workspaceSessions.error === undefined
        ? undefined
        : {
            message: WORKSPACE_LIST_UNREAD,
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
    <div className="app-shell" data-core-state={health.loading ? undefined : coreState}>
      <PluginTheme />
      {/* Owned here, not inside the workspace view: re-creating that view must
          never mount a second modal layer, and exactly one dialog can exist. */}
      <SessionGrantDialog
        open={grantsOpen && activeSession !== undefined}
        security={security}
        sessionId={activeSession?.session_id ?? ''}
        onClose={() => setGrantsOpen(false)}
      />
      {/* A sub-agent's grants, opened from its record or its card; mounted only while shown. */}
      {subagentGrants ? (
        <SubagentGrantDialog key={subagentGrants.sessionId} target={subagentGrants} />
      ) : null}
      {screen === 'workspace' ? (
        <WorkspaceShellSplitPane
          left={
            <AppSidebar
              activeScreen={screen}
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
              conversations={
                navigation.find((workspace) => workspace.id === shownDraft.workspaceId)?.sessions
              }
              onOpenSettings={openSettings}
              onAccepted={(localId, sessionId) => {
                removeComposerDraft(localId);
                setSelectedSessionId((current) =>
                  current === localId || current === '' ? sessionId : current,
                );
              }}
              onInspect={(sessionId) => {
                const draft = getComposerDraft(shownDraft.id) ?? shownDraft;
                void moveKeptDrafts(draft.id, sessionId).then(() => {
                  updateComposerDraft(sessionId, {
                    text: draft.text,
                    ...(draft.quotes ? { quotes: draft.quotes } : {}),
                    needsReview: true,
                  });
                  removeComposerDraft(draft.id);
                  setSelectedSessionId((current) =>
                    current === draft.id || current === '' ? sessionId : current,
                  );
                });
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
              error={workspaceSessions.error ? WORKSPACE_LIST_UNREAD : undefined}
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
