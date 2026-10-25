import { SessionApprovalTray } from './security/SessionApprovalTray';
import type { useSecurityControl } from './security-control-store';
import { RuntimeReadinessCard } from './RuntimeReadinessCard';
import { ProjectionCompatibilityNotice } from './ProjectionCompatibilityNotice';
import type { PermissionMode, PermissionModeTransition } from '@aura/contracts/generated';
import type { CSSProperties } from 'react';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import { AppIcon } from './app-icons';
import type { SessionNavigationItem } from './app-shell-types';
import { Conversation } from './agent-session/Conversation';
import { ActivityNoteStack } from './ActivityNoteStack';
import { keepSidebarRatio } from './activity/sidebar-ratio';
import { useAgentSession } from './agent-session/useAgentSession';
import { useLatestInspection } from './agent-session/useLatestInspection';
import { usePromptInspection } from './agent-session/usePromptInspection';
import { InboxPanel } from './agent-session/InboxPanel';
import { ContextRemediation } from './agent-session/ContextRemediation';
import { QueryForm } from './agent-session/query/QueryForm';
import { SensitiveInputPanel } from './terminal/SensitiveInputPanel';
import { stopFailureMessage, stopStillEnding, userMessage } from './agent-session/user-message';
import { rememberFirstTitle } from './agent-session/session-titles';
import type { SessionModelBinding } from '@aura/contracts/generated';
import { ApprovalPanel } from './agent-session/ApprovalPanel';
import { BrowserAssist } from './browser/BrowserAssist';
import './agent-session/agent-session.css';
import { useModelProjectionError, useScopedModelProjection } from './model-projection-store';
import type { NotificationInput } from './notification-center-store';
import { ResizableComposer } from './ResizableComposer';
import { RightWorkspace } from './RightWorkspace';
import { GoalDock } from './goal/GoalDock';
import { TodoDock } from './todo/TodoDock';
import { AgentToolActivityCard } from './AgentToolActivityCard';
import { useUiSettings } from './ui-settings-store';
import {
  DEFAULT_RIGHT_SIDEBAR_RESPONSIVE_STATE,
  manuallyOpenRightSidebar,
  respondToWindowWidth,
} from './workspace-layout-state';
import { WorkspaceSplitPane } from './WorkspaceSplitPane';
import { useToolShortcuts } from './right-workspace/useToolShortcuts';
import { toolTabs, useToolTabs, type ToolKind } from './right-workspace/tool-tabs';
import { FileLinkContext } from './files/file-links';
import { carriedByName } from './files/carried-menu';
import { carriedZips, zipEntryFinder } from './files/zip-entries';
import { ConversationAttachments } from './composer-attachments/ConversationAttachments';
import { useComposerDockClearance } from './agent-session/useComposerDockClearance';

interface WorkspaceViewProps {
  workspaceId: string;
  /** The workspace root on disk: absolute paths inside it open as workspace files. */
  workspaceRoot: string;
  demoMode: boolean;
  onOpenModelSettings: () => void;
  /** `/tools` and `/memory`. */
  onOpenSettings?: ((section: 'tools' | 'memory') => void) | undefined;
  /** Opens the session grant dialog. It is owned by App so that re-creating
   * this view can never mount a second modal layer. */
  onOpenSecuritySettings: () => void;
  security: ReturnType<typeof useSecurityControl>;
  onSetPermissionMode: (mode: PermissionMode) => Promise<void>;
  permissionMode: PermissionMode | undefined;
  permissionTransition?: PermissionModeTransition | undefined;
  rightPanelOpen: boolean;
  rightPanelWidth: number;
  runtimeNotice:
    | {
        message: string;
        title: string;
        tone: 'danger' | 'muted' | 'success' | 'warning';
      }
    | undefined;
  selectedSession: SessionNavigationItem;
  setRightPanelOpen: (open: boolean) => void;
  setRightPanelWidth: (width: number) => void;
  writeRightPanelWidth: (width: number) => void;
  /** App's notifications, the one list on screen. */
  onNotify: (notification: NotificationInput) => void;
  /**
   * Opens a conversation of this workspace — a branch just made, or the one a branch came
   * from. With `mustExist`, nothing is opened and `false` is returned when it no longer exists.
   */
  onOpenSession?: ((sessionId: string, mustExist: boolean) => boolean) | undefined;
}

export function WorkspaceView({
  workspaceId,
  workspaceRoot,
  demoMode,
  onOpenModelSettings,
  onOpenSettings,
  onOpenSecuritySettings,
  security,
  onSetPermissionMode,
  permissionMode,
  permissionTransition,
  rightPanelOpen,
  rightPanelWidth,
  runtimeNotice,
  selectedSession,
  setRightPanelOpen,
  setRightPanelWidth,
  writeRightPanelWidth,
  onNotify: publishNotification,
  onOpenSession,
}: WorkspaceViewProps) {
  const conversationViewportRef = useRef<HTMLDivElement>(null);
  const conversationCanvasRef = useRef<HTMLDivElement>(null);
  const conversationColumnRef = useRef<HTMLDivElement>(null);
  const dockRef = useRef<HTMLDivElement>(null);
  const latestResponsiveCandidate = useRef<number | undefined>(undefined);
  const dockClearance = useComposerDockClearance(dockRef);
  const agent = useAgentSession(selectedSession.id);
  /** The conversation's latest turn: when it ended, and what the 近期活动 card reads again on. */
  const lastTurn = agent.projection?.turns.at(-1);
  const [toolCompletion, setToolCompletion] = useState('');
  const latestInspection = useLatestInspection(agent.projection);
  const active = agent.projection?.active;
  const [rightSidebarResponsive, setRightSidebarResponsive] = useState(
    DEFAULT_RIGHT_SIDEBAR_RESPONSIVE_STATE,
  );
  const [combinedSidebarWidthRatio, setCombinedSidebarWidthRatio] = useState(0);
  const selectorRoute = useMemo(
    () =>
      ({
        projection_kind: 'session_model_selector',
        projection_scope: { session_id: selectedSession.id, type: 'session' },
      }) as const,
    [selectedSession.id],
  );
  const selectorChange = useScopedModelProjection(selectorRoute, demoMode);
  const debugInformation = useUiSettings().settings.show_debug_information;
  const selectorError = useModelProjectionError(selectorRoute, demoMode);
  const [bindingReceipt, setBindingReceipt] = useState<{
    binding: SessionModelBinding;
    runtimeId: string;
  }>();
  const confirmedBinding =
    bindingReceipt?.runtimeId === selectorChange?.runtime_instance_id
      ? bindingReceipt?.binding
      : undefined;
  const rawSelector = selectorChange?.payload;
  const modelSelector = useMemo(
    () =>
      rawSelector &&
      confirmedBinding &&
      confirmedBinding.session_id === selectedSession.id &&
      BigInt(confirmedBinding.binding_revision) > BigInt(rawSelector.binding_revision)
        ? {
            ...rawSelector,
            binding_revision: confirmedBinding.binding_revision,
            selected_model_profile_id: confirmedBinding.model_profile_id,
          }
        : rawSelector,
    [rawSelector, confirmedBinding, selectedSession.id],
  );
  // A file action that did not happen (copy a path, open a file, show it) says so.
  const notifyFileProblem = useCallback(
    (problem: string) =>
      publishNotification({
        dedupeKey: `file-action:${problem}`,
        title: '文件操作未完成',
        message: problem,
        tone: 'warning',
      }),
    [publishNotification],
  );
  const stop = async () => {
    if (!active) return;
    try {
      const result = await window.aura.v1.cancelAgentRun({
        session_id: selectedSession.id,
        agent_run_id: active.run.agent_run_id,
      });
      if (!result.ok)
        publishNotification({
          dedupeKey: `agent-stop:${active.run.agent_run_id}`,
          title: stopStillEnding(result.error) ? '正在停止' : '停止未确认',
          message: stopFailureMessage(result.error),
          tone: stopStillEnding(result.error) ? 'warning' : 'danger',
        });
    } catch {
      publishNotification({
        dedupeKey: `agent-stop:${active.run.agent_run_id}`,
        title: '停止未确认',
        message:
          '与服务的连接中断了，没能确认这一轮是否停下。连接恢复后请查看任务状态，仍在运行就再点一次“停止”。',
        tone: 'danger',
      });
    } finally {
      agent.refresh();
    }
  };
  const selectModel = useCallback(
    async (modelProfileId: string) => {
      if (modelSelector === undefined) return;
      const result = await window.aura.v1.setSessionModelBinding({
        expected_revision: modelSelector.binding_revision,
        model_profile_id: modelProfileId,
        session_id: modelSelector.session_id,
      });
      if (!result.ok) {
        throw new Error(
          userMessage(result.error.message, '模型切换未完成，请重新打开会话后重试。'),
        );
      }
      if (result.value.value && selectorChange)
        setBindingReceipt({
          binding: result.value.value,
          runtimeId: selectorChange.runtime_instance_id,
        });
    },
    [modelSelector, selectorChange],
  );
  const requestOpenRightSidebar = useCallback(() => {
    setRightPanelOpen(true);
    setRightSidebarResponsive(manuallyOpenRightSidebar(latestResponsiveCandidate.current));
  }, [setRightPanelOpen]);
  const closeRightSidebar = useCallback(() => {
    setRightPanelOpen(false);
    setRightSidebarResponsive(DEFAULT_RIGHT_SIDEBAR_RESPONSIVE_STATE);
  }, [setRightPanelOpen]);
  // A link or a shortcut shows the tool's latest tab in this conversation, or opens one.
  const sessionId = selectedSession.id;
  /** The header's place for the 近期活动 entry, once it is on the page. */
  const [activitySlot, setActivitySlot] = useState<HTMLDivElement | null>(null);
  const openRightPanel = useCallback(
    (kind: ToolKind) => {
      toolTabs.show(sessionId, kind);
      requestOpenRightSidebar();
    },
    [requestOpenRightSidebar, sessionId],
  );
  const openTabs = useToolTabs(sessionId);
  const rightPanel = openTabs.tabs.find((tab) => tab.id === openTabs.active)?.kind ?? 'empty';
  const rightSidebarVisible =
    rightPanelOpen &&
    (!rightSidebarResponsive.autoHidden || rightSidebarResponsive.narrowManualOverride);
  const {
    inspection,
    error: inspectionError,
    openLatest,
  } = usePromptInspection(
    sessionId,
    agent.projection?.runtime_instance_id,
    latestInspection,
    rightSidebarVisible && rightPanel === 'prompt-inspector',
    () => openRightPanel('prompt-inspector'),
  );
  const promptInspector = inspection?.projection;
  useToolShortcuts(openRightPanel);
  // A link in the conversation ("查看终端") shows its tab in the sidebar, opening the sidebar.
  useEffect(
    () =>
      toolTabs.onReveal((session) => {
        if (session === sessionId) requestOpenRightSidebar();
      }),
    [requestOpenRightSidebar, sessionId],
  );
  // The review page can fill the main area while it is the sidebar's open tab; so can the browser.
  const [expandedPanel, setExpandedPanel] = useState<string>();
  // A file outside the workspace a message carried, which the model names by its name.
  const carried = useMemo(
    // A turn read in part may not list its messages yet.
    () =>
      carriedByName(
        (agent.projection?.turns ?? []).flatMap(
          (turn) => (turn.user_messages as typeof turn.user_messages | undefined) ?? [],
        ),
      ),
    [agent.projection?.turns],
  );
  const attachmentNamed = useCallback((name: string) => carried.get(name), [carried]);
  // A file inside a zip a message carried from outside the workspace.
  const zips = useMemo(
    () =>
      carriedZips(
        (agent.projection?.turns ?? []).flatMap(
          (turn) => (turn.user_messages as typeof turn.user_messages | undefined) ?? [],
        ),
      ),
    [agent.projection?.turns],
  );
  const zipEntryNamed = useMemo(() => zipEntryFinder(zips, sessionId), [zips, sessionId]);
  const fileLinks = useMemo(
    () => ({
      session: sessionId,
      workspace: workspaceId ?? '',
      root: workspaceRoot,
      notify: notifyFileProblem,
      attachmentNamed,
      zipEntryNamed,
    }),
    [sessionId, workspaceId, workspaceRoot, notifyFileProblem, attachmentNamed, zipEntryNamed],
  );
  const panelExpanded =
    expandedPanel === rightPanel && (rightPanel === 'review' || rightPanel === 'browser');
  const handleResponsiveCandidateWidth = useCallback(
    (candidateConversationWidth: number) => {
      latestResponsiveCandidate.current = candidateConversationWidth;
      setRightSidebarResponsive((current) =>
        respondToWindowWidth(current, candidateConversationWidth, rightPanelOpen),
      );
    },
    [rightPanelOpen],
  );

  const conversationCanvasStyle =
    dockClearance === undefined
      ? undefined
      : ({ '--conversation-dock-clearance': `${dockClearance}px` } as CSSProperties);

  return (
    <WorkspaceSplitPane
      onCloseRightSidebar={closeRightSidebar}
      onCombinedSidebarWidthRatioChange={(ratio) =>
        setCombinedSidebarWidthRatio((kept) => keepSidebarRatio(kept, ratio))
      }
      onRequestOpenRightSidebar={requestOpenRightSidebar}
      onResponsiveCandidateWidthChange={handleResponsiveCandidateWidth}
      onRightPanelWidthChange={setRightPanelWidth}
      onRightPanelWidthCommit={writeRightPanelWidth}
      right={
        <RightWorkspace
          visible={rightSidebarVisible}
          workspaceId={workspaceId}
          workspaceRoot={workspaceRoot}
          runtimeInstanceId={agent.projection?.runtime_instance_id}
          completionKey={toolCompletion}
          sessionId={selectedSession.id}
          onToggleSidebar={closeRightSidebar}
          expanded={panelExpanded}
          onToggleExpanded={() =>
            setExpandedPanel((value) => (value === rightPanel ? undefined : rightPanel))
          }
          promptInspector={promptInspector}
          inspection={inspection}
          inspectionError={inspectionError}
          onNotify={notifyFileProblem}
          attachmentNamed={attachmentNamed}
        />
      }
      rightPanel={rightPanel}
      rightExpanded={panelExpanded && rightSidebarVisible}
      rightPanelWidth={rightPanelWidth}
      rightSidebarVisible={rightSidebarVisible}
    >
      <ConversationAttachments key={selectedSession.id} className="conversation-workspace">
        <header className="conversation-header">
          <h2>{selectedSession.title || '新会话'}</h2>
          <div className="conversation-header-actions">
            {/* A narrow window's 近期活动 entry sits here, beside the title. */}
            <div className="activity-entry-slot" ref={setActivitySlot} />
            <button
              aria-label="打开文件工具区"
              className="icon-button compact-tools-button"
              onClick={() => openRightPanel('files')}
              type="button"
            >
              <AppIcon name="folder" />
            </button>
          </div>
        </header>

        <div
          className="conversation-canvas"
          ref={conversationCanvasRef}
          style={conversationCanvasStyle}
        >
          <div className="conversation-scroll" ref={conversationViewportRef}>
            {runtimeNotice === undefined ? null : (
              <div className={`runtime-notice notice-${runtimeNotice.tone}`} role="alert">
                <AppIcon name="activity" />
                <span>
                  <strong>{runtimeNotice.title}</strong>
                  {runtimeNotice.message}
                </span>
              </div>
            )}
            <ProjectionCompatibilityNotice surface="workspace" sessionId={selectedSession.id} />
            {/* For debugging only, while 设置 → 环境 → 显示调试信息 is on. */}
            {debugInformation ? <RuntimeReadinessCard /> : null}
            {demoMode || !debugInformation ? null : (
              <AgentToolActivityCard
                key={selectedSession.id}
                sessionId={selectedSession.id}
                currentMode={permissionMode}
                runtimeId={agent.projection?.runtime_instance_id}
              />
            )}

            <div className="conversation-column" ref={conversationColumnRef}>
              <FileLinkContext.Provider value={fileLinks}>
                <Conversation
                  onToolCompleted={setToolCompletion}
                  projection={agent.projection}
                  loading={agent.loading}
                  error={agent.error}
                  viewportRef={conversationViewportRef}
                  onChanged={agent.refresh}
                  loadOlder={agent.loadOlder}
                  loadTurnOlder={agent.loadTurnOlder}
                  title={selectedSession.title}
                  onBranched={
                    onOpenSession && !demoMode
                      ? (made) => void onOpenSession(made.session_id, false)
                      : undefined
                  }
                  onOpenSession={onOpenSession ? (id) => onOpenSession(id, true) : undefined}
                />
              </FileLinkContext.Provider>
            </div>
          </div>

          {demoMode ? null : (
            <ActivityNoteStack
              combinedSidebarWidthRatio={combinedSidebarWidthRatio}
              conversationColumnRef={conversationColumnRef}
              viewportRef={conversationCanvasRef}
              entrySlot={activitySlot}
              goal={
                <GoalDock
                  goal={
                    lastTurn?.goal ??
                    (agent.projection?.mode_episode?.mode === 'goal'
                      ? (agent.projection.mode_episode.inherited_goal ?? undefined)
                      : undefined)
                  }
                  objective={
                    lastTurn?.goal
                      ? lastTurn.user_messages.find(
                          (message) => message.event_id === lastTurn.turn.root_user_event_id,
                        )?.text
                      : undefined
                  }
                  paused={agent.projection?.mode_episode?.paused === true}
                  running={lastTurn?.turn.state.state === 'running'}
                />
              }
              todo={
                <TodoDock
                  sessionId={selectedSession.id}
                  todo={agent.projection?.turns.at(-1)?.todo}
                />
              }
              taskList={agent.projection?.turns.at(-1)?.todo}
              summary={
                workspaceId
                  ? {
                      workspaceId,
                      sessionId,
                      running: agent.projection?.active != null,
                      lastEndedAt: lastTurn?.turn.updated_at,
                      refreshKey: `${lastTurn?.turn.turn_task_id ?? ''}:${lastTurn?.turn.state.state ?? ''}:${lastTurn?.turn.revision ?? ''}`,
                      onOpenReview: () => openRightPanel('review'),
                    }
                  : undefined
              }
              episode={
                agent.projection?.mode_episode ?? {
                  session_id: selectedSession.id,
                  revision: '0',
                  mode: 'normal',
                  paused: false,
                  goal_budget: null,
                  inherited_goal: null,
                }
              }
              onControl={async (action) => {
                const episode = agent.projection?.mode_episode;
                if (!episode) throw new Error('当前模式尚未就绪。');
                const result = await window.aura.v1.controlAgentMode({
                  session_id: selectedSession.id,
                  expected_revision: episode.revision,
                  action,
                });
                agent.refresh();
                if (!result.ok) {
                  throw new Error(userMessage(result.error.message, '模式操作未完成。'));
                }
              }}
            />
          )}

          <div className="conversation-dock" ref={dockRef}>
            <div className="conversation-dock-inner">
              {active?.run.state.state === 'awaiting_context_remediation' ? (
                <ContextRemediation
                  key={`${active.run.agent_run_id}:${active.run.revision}`}
                  sessionId={selectedSession.id}
                  runId={active.run.agent_run_id}
                  onChanged={agent.refresh}
                  onOpenModelSettings={onOpenModelSettings}
                  onStop={() => void stop()}
                />
              ) : null}
              {agent.projection ? (
                <InboxPanel
                  inbox={agent.projection.inbox}
                  activeTurn={active?.turn.turn_task_id}
                  onChanged={agent.refresh}
                />
              ) : null}
              {active?.pending_query?.state.state === 'pending' ? (
                <QueryForm
                  key={`${active.pending_query.query_id}:${active.pending_query.revision}`}
                  query={active.pending_query}
                  sessionId={selectedSession.id}
                  deadlineAt={active.pending_query_deadline_at}
                  onChanged={agent.refresh}
                />
              ) : null}
              {!demoMode ? (
                <SensitiveInputPanel
                  key={`sensitive:${selectedSession.id}`}
                  sessionId={selectedSession.id}
                />
              ) : null}
              {active?.pending_approval_ids.map((id) => (
                <ApprovalPanel
                  key={id}
                  sessionId={selectedSession.id}
                  approvalId={id}
                  onChanged={agent.refresh}
                />
              ))}
              {!demoMode ? (
                <SessionApprovalTray
                  // Each keyed sibling states its own identity: two elements in
                  // this list used to carry the bare session id, which is a
                  // duplicate key the moment both render — they always do — and
                  // React then reuses the wrong child's state across updates.
                  key={`approval-tray:${selectedSession.id}`}
                  security={security}
                  sessionId={selectedSession.id}
                />
              ) : null}
              {!demoMode ? (
                <BrowserAssist
                  key={`browser-assist:${selectedSession.id}`}
                  sessionId={selectedSession.id}
                />
              ) : null}
              <ResizableComposer
                key={`composer:${selectedSession.id}`}
                sessionId={selectedSession.id}
                activeTurn={active?.turn.turn_task_id}
                preparation={{
                  prepare: async () => selectedSession.id,
                  accepted: (id, text) => {
                    if (!selectedSession.title) rememberFirstTitle(id, text);
                  },
                }}
                queuePending={(agent.projection?.inbox.messages.length ?? 0) > 0}
                onChanged={agent.refresh}
                onStop={stop}
                demoMode={demoMode}
                modelSelector={modelSelector}
                modelSelectorError={selectorError}
                onOpenModelSettings={onOpenModelSettings}
                onOpenPromptInspector={openLatest}
                onOpenSecuritySettings={onOpenSecuritySettings}
                onSetPermissionMode={onSetPermissionMode}
                onSelectModel={selectModel}
                permissionMode={permissionMode}
                permissionTransition={permissionTransition}
                contextUsageSource={latestInspection.source}
                contextUsage={latestInspection.usage}
                compactionRunning={latestInspection.compactionRunning}
                promptInspector={latestInspection.view?.projection}
                workspaceId={workspaceId}
                activeRunId={active?.run.agent_run_id}
                onOpenSettings={onOpenSettings}
                onOpenPanel={openRightPanel}
                fileScope={workspaceId ? fileLinks : undefined}
              />
            </div>
          </div>
        </div>
      </ConversationAttachments>
    </WorkspaceSplitPane>
  );
}
