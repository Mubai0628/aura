import type { CSSProperties, ReactNode, TransitionEvent } from 'react';
import { useCallback, useLayoutEffect, useRef, useState } from 'react';

import { AppIcon } from './app-icons';
import type { RightPanel } from './app-shell-types';
import { CollapsiblePaneResizer } from './CollapsiblePaneResizer';
import { MIN_RIGHT_PANEL_WIDTH } from './ui-preferences';
import { MIN_CONVERSATION_PANE_WIDTH } from './workspace-layout-state';

const DIVIDER_WIDTH = 8;

interface WorkspaceSplitPaneProps {
  children: ReactNode;
  onCloseRightSidebar: () => void;
  onCombinedSidebarWidthRatioChange?: (ratio: number) => void;
  onRequestOpenRightSidebar: () => void;
  onResponsiveCandidateWidthChange: (candidateConversationWidth: number) => void;
  onRightPanelWidthChange: (width: number) => void;
  onRightPanelWidthCommit: (width: number) => void;
  right: ReactNode;
  rightPanel: RightPanel;
  /** The right pane covers the conversation (the review page, filling the main area). */
  rightExpanded?: boolean;
  rightPanelWidth: number;
  rightSidebarVisible: boolean;
}

type SplitPaneStyle = CSSProperties & {
  '--right-divider-width': string;
  '--right-panel-width': string;
};

export function WorkspaceSplitPane({
  children,
  onCloseRightSidebar,
  onCombinedSidebarWidthRatioChange,
  onRequestOpenRightSidebar,
  onResponsiveCandidateWidthChange,
  onRightPanelWidthChange,
  onRightPanelWidthCommit,
  right,
  rightPanel,
  rightExpanded = false,
  rightPanelWidth,
  rightSidebarVisible,
}: WorkspaceSplitPaneProps) {
  const containerRef = useRef<HTMLElement>(null);
  const rightRef = useRef<HTMLDivElement>(null);
  const reopenRef = useRef<HTMLButtonElement>(null);
  const wasExpanded = useRef(false);
  const [containerWidth, setContainerWidth] = useState(0);
  const [draftWidth, setDraftWidth] = useState<number>();
  const [dragging, setDragging] = useState(false);
  const [temporarilyCollapsed, setTemporarilyCollapsed] = useState(false);
  const [motionActive, setMotionActive] = useState(false);

  const reserve = MIN_CONVERSATION_PANE_WIDTH + DIVIDER_WIDTH;
  const available = containerWidth || rightPanelWidth * 2 + reserve;
  const maximum = Math.max(
    MIN_RIGHT_PANEL_WIDTH,
    Math.min(Math.floor(available / 2), available - reserve),
  );
  const requestedWidth = dragging ? (draftWidth ?? rightPanelWidth) : rightPanelWidth;
  const effectiveWidth = Math.min(maximum, Math.max(MIN_RIGHT_PANEL_WIDTH, requestedWidth));
  const renderedOpen = rightSidebarVisible && !(dragging && temporarilyCollapsed);
  const split = renderedOpen && !rightExpanded;

  const reportCombinedSidebarRatio = useCallback(() => {
    const viewportWidth = Math.max(1, document.documentElement.clientWidth, window.innerWidth);
    const mainRect = containerRef.current?.getBoundingClientRect();
    const leftSidebarWidth = Math.max(0, mainRect?.left ?? 0);
    const visibleRightWidth = split ? effectiveWidth + DIVIDER_WIDTH : 0;
    onCombinedSidebarWidthRatioChange?.(
      Math.min(1, (leftSidebarWidth + visibleRightWidth) / viewportWidth),
    );
  }, [effectiveWidth, onCombinedSidebarWidthRatioChange, split]);

  useLayoutEffect(() => {
    reportCombinedSidebarRatio();
  }, [containerWidth, reportCombinedSidebarRatio]);
  useLayoutEffect(() => {
    const right = rightRef.current;
    if (rightExpanded && !wasExpanded.current && !right?.contains(document.activeElement)) {
      right?.querySelector<HTMLButtonElement>('.right-sidebar-toggle')?.focus();
    } else if (!rightExpanded && wasExpanded.current && !rightSidebarVisible) {
      reopenRef.current?.focus();
    }
    wasExpanded.current = rightExpanded;
  }, [rightExpanded, rightSidebarVisible]);

  useLayoutEffect(() => {
    const container = containerRef.current;
    if (container === null) return;
    const updateBounds = () => setContainerWidth(container.clientWidth);
    updateBounds();
    if (typeof ResizeObserver === 'undefined') {
      window.addEventListener('resize', updateBounds);
      return () => window.removeEventListener('resize', updateBounds);
    }
    const observer = new ResizeObserver(updateBounds);
    observer.observe(container);
    return () => observer.disconnect();
  }, []);

  const candidateWidth = containerWidth - MIN_RIGHT_PANEL_WIDTH - DIVIDER_WIDTH;
  useLayoutEffect(() => {
    if (containerWidth > 0) onResponsiveCandidateWidthChange(candidateWidth);
  }, [candidateWidth, containerWidth, onResponsiveCandidateWidthChange]);

  const style: SplitPaneStyle = {
    '--right-divider-width': rightSidebarVisible && !rightExpanded ? `${DIVIDER_WIDTH}px` : '0px',
    '--right-panel-width': split ? `${effectiveWidth}px` : '0px',
  };
  const handleTransitionEnd = (event: TransitionEvent<HTMLElement>) => {
    if (event.currentTarget === event.target && event.propertyName === 'grid-template-columns') {
      setMotionActive(false);
    }
  };

  return (
    <main
      className="workspace-main"
      data-pane-motion={motionActive ? 'true' : 'false'}
      data-panel={rightPanel}
      data-right-expanded={rightExpanded ? 'true' : undefined}
      data-responsive-candidate-width={containerWidth > 0 ? candidateWidth : undefined}
      data-resizing={dragging && !motionActive ? 'true' : 'false'}
      data-sidebar-open={rightSidebarVisible ? 'true' : 'false'}
      data-temporarily-collapsed={temporarilyCollapsed ? 'true' : 'false'}
      onTransitionEnd={handleTransitionEnd}
      ref={containerRef}
      style={style}
    >
      {children}
      <CollapsiblePaneResizer
        className="right-panel-resizer"
        controls="right-workspace"
        direction="right"
        disabled={!rightSidebarVisible || rightExpanded}
        label="调整右侧工作区宽度"
        maximum={maximum}
        minimum={MIN_RIGHT_PANEL_WIDTH}
        onCollapse={onCloseRightSidebar}
        onDragStateChange={(nextDragging) => {
          setDragging(nextDragging);
          setDraftWidth(nextDragging ? rightPanelWidth : undefined);
          if (nextDragging) setMotionActive(false);
        }}
        onResize={setDraftWidth}
        onResizeCommit={(width) => {
          onRightPanelWidthChange(width);
          onRightPanelWidthCommit(width);
        }}
        onTemporaryCollapseChange={(collapsed) => {
          setTemporarilyCollapsed(collapsed);
          setMotionActive(true);
        }}
        width={effectiveWidth}
      />
      <div
        aria-hidden={!renderedOpen ? 'true' : undefined}
        className="right-pane-slot"
        inert={!renderedOpen}
        ref={rightRef}
      >
        {right}
      </div>
      {rightSidebarVisible ? null : (
        <button
          aria-controls="right-workspace"
          aria-expanded="false"
          aria-label="展开右侧边栏"
          className="icon-button right-sidebar-reopen"
          onClick={onRequestOpenRightSidebar}
          ref={reopenRef}
          type="button"
        >
          <AppIcon name="sidebar-right" />
        </button>
      )}
    </main>
  );
}
