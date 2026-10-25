import type { CSSProperties, ReactNode, TransitionEvent } from 'react';
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';

import { AppIcon } from './app-icons';
import type { RightPanel } from './app-shell-types';
import { CollapsiblePaneResizer } from './CollapsiblePaneResizer';
import { MIN_RIGHT_PANEL_WIDTH } from './ui-preferences';

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
  const preferredWidthRef = useRef(rightPanelWidth);
  const responsiveCallbackRef = useRef(onResponsiveCandidateWidthChange);
  const [containerWidth, setContainerWidth] = useState(0);
  const [draftWidth, setDraftWidth] = useState<number>();
  const [dragging, setDragging] = useState(false);
  const [temporarilyCollapsed, setTemporarilyCollapsed] = useState(false);
  const [motionActive, setMotionActive] = useState(false);

  useEffect(() => {
    preferredWidthRef.current = rightPanelWidth;
  }, [rightPanelWidth]);

  useEffect(() => {
    responsiveCallbackRef.current = onResponsiveCandidateWidthChange;
  }, [onResponsiveCandidateWidthChange]);

  const maximum = Math.max(
    MIN_RIGHT_PANEL_WIDTH,
    Math.floor((containerWidth || rightPanelWidth * 2) * 0.5),
  );
  const requestedWidth = dragging ? (draftWidth ?? rightPanelWidth) : rightPanelWidth;
  const effectiveWidth = Math.min(maximum, Math.max(MIN_RIGHT_PANEL_WIDTH, requestedWidth));
  const renderedOpen = rightSidebarVisible && !(dragging && temporarilyCollapsed);

  const reportCombinedSidebarRatio = useCallback(() => {
    const viewportWidth = Math.max(1, document.documentElement.clientWidth, window.innerWidth);
    const mainRect = containerRef.current?.getBoundingClientRect();
    const leftSidebarWidth = Math.max(0, mainRect?.left ?? 0);
    const visibleRightWidth = renderedOpen ? effectiveWidth + 8 : 0;
    onCombinedSidebarWidthRatioChange?.(
      Math.min(1, (leftSidebarWidth + visibleRightWidth) / viewportWidth),
    );
  }, [effectiveWidth, onCombinedSidebarWidthRatioChange, renderedOpen]);

  useLayoutEffect(() => {
    reportCombinedSidebarRatio();
  }, [reportCombinedSidebarRatio]);

  useEffect(() => {
    const container = containerRef.current;
    if (container === null) return;
    const updateBounds = () => {
      setContainerWidth(container.clientWidth);
      reportCombinedSidebarRatio();
    };
    updateBounds();
    if (typeof ResizeObserver === 'undefined') {
      window.addEventListener('resize', updateBounds);
      return () => window.removeEventListener('resize', updateBounds);
    }
    const observer = new ResizeObserver(updateBounds);
    observer.observe(container);
    return () => observer.disconnect();
  }, [reportCombinedSidebarRatio]);

  useEffect(() => {
    let animationFrame: number | undefined;
    const reportWindowResponsiveWidth = () => {
      const width = containerRef.current?.clientWidth ?? 0;
      if (width <= 0) return;
      const maximumWidth = Math.max(MIN_RIGHT_PANEL_WIDTH, Math.floor(width * 0.5));
      const candidateRightWidth = Math.min(
        maximumWidth,
        Math.max(MIN_RIGHT_PANEL_WIDTH, preferredWidthRef.current),
      );
      const candidateConversationWidth = width - candidateRightWidth - 8;
      if (containerRef.current !== null) {
        containerRef.current.dataset.responsiveCandidateWidth = String(candidateConversationWidth);
      }
      responsiveCallbackRef.current(candidateConversationWidth);
    };
    const scheduleReport = () => {
      if (animationFrame !== undefined) window.cancelAnimationFrame(animationFrame);
      animationFrame = window.requestAnimationFrame(reportWindowResponsiveWidth);
    };
    reportWindowResponsiveWidth();
    window.addEventListener('resize', scheduleReport);
    return () => {
      if (animationFrame !== undefined) window.cancelAnimationFrame(animationFrame);
      window.removeEventListener('resize', scheduleReport);
    };
  }, []);

  const style: SplitPaneStyle = {
    '--right-divider-width': rightSidebarVisible ? '8px' : '0px',
    '--right-panel-width': renderedOpen ? `${effectiveWidth}px` : '0px',
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
        disabled={!rightSidebarVisible}
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
          type="button"
        >
          <AppIcon name="sidebar-right" />
        </button>
      )}
    </main>
  );
}
