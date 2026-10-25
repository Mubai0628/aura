import type { AgentPromptInspection, PromptInspectorProjection } from '@aura/contracts/generated';
import { useId, useMemo, useRef, useState, useSyncExternalStore } from 'react';

import { AppIcon, type IconName } from './app-icons';
import { ConfirmationDialog } from './ConfirmationDialog';
import { Inspector } from './agent-session/Inspector';
import { useDismissablePopover } from './useDismissablePopover';
import { ProcessPanel } from './terminal/ProcessPanel';
import { TerminalHost } from './terminal/TerminalHost';
import { closeTerminalTab } from './terminal/close-terminal-tab';
import { subscribeTerminals, terminalSnapshot } from './terminal/store';
import { ReviewPanel } from './review/ReviewPanel';
import { FilesPanel } from './files/FilesPanel';
import { BrowserPanel } from './browser/BrowserPanel';
import { browserTabs } from './browser/browser-tabs';
import {
  availableTools,
  tabLabels,
  MENU_TOOLS,
  TOOL_NAMES,
  toolTabs,
  useToolTabs,
  type ToolKind,
  type ToolTab,
} from './right-workspace/tool-tabs';
import { ToolTabStrip, type StripTab } from './right-workspace/ToolTabStrip';
import { TOOL_SHORTCUTS } from './right-workspace/useToolShortcuts';
import './right-workspace/right-workspace-tabs.css';

interface RightWorkspaceProps {
  visible?: boolean | undefined;
  workspaceId?: string | undefined;
  /** The workspace root on disk, to read absolute paths inside it. */
  workspaceRoot?: string | undefined;
  /** The file a message carried under a name, as a 文件 tab entry. */
  attachmentNamed?: ((name: string) => string | undefined) | undefined;
  runtimeInstanceId?: string | undefined;
  completionKey?: string | undefined;
  sessionId?: string | undefined;
  inspection?: AgentPromptInspection | undefined;
  inspectionError?: string | undefined;
  onToggleSidebar: () => void;
  promptInspector: PromptInspectorProjection | undefined;
  /** The review page or the browser fills the main area. */
  expanded?: boolean;
  onToggleExpanded?: () => void;
  /** Says what went wrong with a file action. */
  onNotify?: ((problem: string) => void) | undefined;
}

const ICONS: Record<ToolKind, IconName> = {
  review: 'review',
  terminal: 'terminal',
  process: 'activity',
  browser: 'browser',
  files: 'folder',
  chat: 'chat',
  'prompt-inspector': 'activity',
};

function Placeholder({ kind }: { kind: 'chat' }) {
  const copy = {
    chat: ['侧边对话尚未开始', '侧边对话功能尚不可用。'],
  }[kind];
  return (
    <div className="right-panel-empty">
      <AppIcon name={ICONS[kind]} size={28} />
      <h3>{copy[0]}</h3>
      <p>{copy[1]}</p>
      <span className="mock-inline-note">计划功能 · 尚未接入</span>
    </div>
  );
}

function ToolChoice({ kind, onPick, role }: { kind: ToolKind; onPick: () => void; role?: string }) {
  const shortcut = TOOL_SHORTCUTS[kind];
  return (
    <button onClick={onPick} role={role} type="button">
      <AppIcon name={ICONS[kind]} />
      <span>{TOOL_NAMES[kind]}</span>
      {shortcut === undefined ? <span /> : <kbd>{shortcut}</kbd>}
    </button>
  );
}

/**
 * The right sidebar holds the conversation's tool tabs. "+" offers every tool
 * that may open more than once and the single ones not yet open; a terminal you opened ends with
 * its tab, the model's terminal keeps running when its tab closes.
 */
export function RightWorkspace({
  visible = true,
  workspaceId,
  workspaceRoot,
  attachmentNamed,
  runtimeInstanceId,
  completionKey,
  sessionId,
  inspection,
  inspectionError,
  onToggleSidebar,
  expanded = false,
  onToggleExpanded,
  onNotify,
}: RightWorkspaceProps) {
  const menuRef = useRef<HTMLDivElement>(null);
  const [menuOpen, setMenuOpen] = useDismissablePopover(menuRef);
  const [closeError, setCloseError] = useState('');
  // Tabs whose closing would end terminals the user opened: closed once the user agrees.
  const [ending, setEnding] = useState<{ tabs: ToolTab[]; terminals: number }>();
  const endingTitle = useId();
  const state = useToolTabs(sessionId);
  const records = useSyncExternalStore(subscribeTerminals, terminalSnapshot);
  const active = state.tabs.find((tab) => tab.id === state.active);
  const offered = availableTools(state);
  const files = useMemo(
    () =>
      workspaceId && sessionId
        ? {
            session: sessionId,
            workspace: workspaceId,
            root: workspaceRoot ?? '',
            notify: (problem: string) => onNotify?.(problem),
            attachmentNamed,
          }
        : undefined,
    [sessionId, workspaceId, workspaceRoot, onNotify, attachmentNamed],
  );

  const open = (kind: ToolKind) => {
    if (sessionId) toolTabs.open(sessionId, kind);
    setMenuOpen(false);
  };
  const close = async (tab: ToolTab) => {
    if (!sessionId) return;
    setCloseError('');
    if (tab.kind === 'terminal') {
      const failure = await closeTerminalTab(sessionId, tab);
      if (failure) setCloseError(failure);
    } else {
      // The browser's pages end with its tab.
      if (tab.kind === 'browser') browserTabs.closeAll(sessionId);
      toolTabs.close(sessionId, tab.id);
    }
  };
  const recordOf = (tab: ToolTab) =>
    tab.terminalId ? records.find((item) => item.view.terminal_id === tab.terminalId) : undefined;
  const labelled: StripTab[] = tabLabels(state).map(({ tab, label }) => {
    const record = recordOf(tab);
    const model = record?.view.owner === 'agent';
    return {
      tab,
      label: record?.view.owner === 'agent' ? record?.view.title || '模型终端' : label,
      icon: model ? 'model' : ICONS[tab.kind],
      title: record ? (record.view.shell === 'cmd' ? 'CMD' : 'PowerShell 7') : tab.file,
      givesWay: model,
    };
  });
  const closeAll = async (tabs: ToolTab[]) => {
    for (const tab of tabs) await close(tab);
  };
  // A terminal the user opened ends with its tab: closing several asks first.
  const closeMany = (tabs: ToolTab[]) => {
    const terminals = tabs.filter((tab) => {
      if (tab.kind !== 'terminal') return false;
      const record = recordOf(tab);
      return record ? record.view.owner !== 'agent' && !record.view.end : true;
    }).length;
    if (terminals) setEnding({ tabs, terminals });
    else void closeAll(tabs);
  };

  return (
    <aside
      aria-label="右侧工作区"
      className="right-workspace"
      data-panel={active?.kind ?? 'empty'}
      id="right-workspace"
    >
      <div className="right-workspace-toolbar">
        <ToolTabStrip
          tabs={labelled}
          active={state.active}
          onActivate={(tab) => sessionId && toolTabs.activate(sessionId, tab.id)}
          onClose={(tab) => void close(tab)}
          onCloseMany={closeMany}
        />
        <div className="right-workspace-actions">
          <div className="tool-menu-anchor" ref={menuRef}>
            <button
              aria-expanded={menuOpen}
              aria-haspopup="menu"
              aria-label="打开工具"
              className="icon-button tool-add-button"
              onClick={() => setMenuOpen((value) => !value)}
              type="button"
            >
              <AppIcon name="plus" />
            </button>
            {menuOpen ? (
              <div aria-label="选择工具" className="tool-picker" role="menu">
                {offered.map((kind) => (
                  <ToolChoice key={kind} kind={kind} onPick={() => open(kind)} role="menuitem" />
                ))}
              </div>
            ) : null}
          </div>
          <button
            aria-label="关闭右侧边栏"
            className="icon-button right-sidebar-toggle"
            onClick={onToggleSidebar}
            type="button"
          >
            <AppIcon name="sidebar-right" />
          </button>
        </div>
      </div>
      {closeError ? (
        <p className="right-workspace-error" role="alert">
          {closeError}
        </p>
      ) : null}
      {ending ? (
        <ConfirmationDialog
          busy={false}
          confirmLabel="关闭并结束终端"
          description={`这些标签里有 ${ending.terminals} 个你打开的终端，关闭后终端会结束，里面正在运行的命令也会停止。`}
          onCancel={() => setEnding(undefined)}
          onConfirm={() => {
            const tabs = ending.tabs;
            setEnding(undefined);
            void closeAll(tabs);
          }}
          title="关闭标签并结束终端？"
          titleId={endingTitle}
        />
      ) : null}

      <TerminalHost
        sessionId={sessionId}
        tabs={state.tabs}
        activeTab={state.active}
        visible={visible && active?.kind === 'terminal'}
      />
      {/* Every 文件 tab stays mounted, hidden while another is shown: it keeps its place. */}
      {files
        ? state.tabs
            .filter((tab) => tab.kind === 'files')
            .map((tab) => (
              <FilesPanel
                key={tab.id}
                scope={files}
                tab={tab}
                visible={visible && tab.id === state.active}
                refreshKey={completionKey}
              />
            ))
        : null}
      {active === undefined ? (
        <div className="right-shortcuts">
          <p>打开工具</p>
          {MENU_TOOLS.map((kind) => (
            <ToolChoice key={kind} kind={kind} onPick={() => open(kind)} />
          ))}
        </div>
      ) : active.kind === 'terminal' ? null : active.kind === 'files' ? (
        files ? null : (
          <p className="right-panel-empty">选择会话后查看其工作区的文件。</p>
        )
      ) : active.kind === 'process' ? (
        <ProcessPanel
          sessionId={sessionId}
          onViewTerminal={(id) => sessionId && toolTabs.showTerminal(sessionId, id)}
        />
      ) : active.kind === 'review' ? (
        !visible ? null : workspaceId && sessionId ? (
          <ReviewPanel
            key={`${workspaceId}:${sessionId}:${runtimeInstanceId ?? ''}`}
            workspaceId={workspaceId}
            sessionId={sessionId}
            completionKey={completionKey}
            expanded={expanded}
            onToggleExpanded={onToggleExpanded}
            onClose={() => toolTabs.close(sessionId, active.id)}
          />
        ) : (
          <p className="right-panel-empty">选择会话后查看其工作区的改动。</p>
        )
      ) : active.kind === 'prompt-inspector' ? (
        <Inspector view={inspection} error={inspectionError} />
      ) : active.kind === 'browser' ? (
        sessionId ? (
          <BrowserPanel
            expanded={expanded}
            key={sessionId}
            onToggleExpanded={onToggleExpanded}
            sessionId={sessionId}
            visible={visible}
          />
        ) : (
          <p className="right-panel-empty">选择会话后使用浏览器。</p>
        )
      ) : (
        <Placeholder kind={active.kind} />
      )}
    </aside>
  );
}
