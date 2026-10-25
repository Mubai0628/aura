import type { CSSProperties, ReactNode, TransitionEvent } from 'react';
import { useEffect, useRef, useState } from 'react';

import { AppIcon } from './app-icons';
import { CollapsiblePaneResizer } from './CollapsiblePaneResizer';
import { MIN_LEFT_PANEL_WIDTH } from './ui-preferences';

interface WorkspaceShellSplitPaneProps {
  children: ReactNode;
  left: ReactNode;
  leftPanelOpen: boolean;
  leftPanelWidth: number;
  onCloseLeftPanel: () => void;
  onLeftPanelWidthChange: (width: number) => void;
  onLeftPanelWidthCommit: (width: number) => void;
  onOpenLeftPanel: () => void;
}

type WorkspaceShellStyle = CSSProperties & {
  '--left-divider-width': string;
  '--left-panel-width': string;
};

export function WorkspaceShellSplitPane({
  children,
  left,
  leftPanelOpen,
  leftPanelWidth,
  onCloseLeftPanel,
  onLeftPanelWidthChange,
  onLeftPanelWidthCommit,
  onOpenLeftPanel,
}: WorkspaceShellSplitPaneProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [containerWidth, setContainerWidth] = useState(0);
  const [draftWidth, setDraftWidth] = useState<number>();
  const [dragging, setDragging] = useState(false);
  const [temporarilyCollapsed, setTemporarilyCollapsed] = useState(false);
  const [motionActive, setMotionActive] = useState(false);
  const maximum = Math.max(
    MIN_LEFT_PANEL_WIDTH,
    Math.floor((containerWidth || leftPanelWidth * 2.5) * 0.4),
  );
  const requestedWidth = dragging ? (draftWidth ?? leftPanelWidth) : leftPanelWidth;
  const effectiveWidth = Math.min(maximum, Math.max(MIN_LEFT_PANEL_WIDTH, requestedWidth));

  useEffect(() => {
    const container = containerRef.current;
    if (container === null) return;
    const update = () => setContainerWidth(container.clientWidth);
    update();
    if (typeof ResizeObserver === 'undefined') {
      window.addEventListener('resize', update);
      return () => window.removeEventListener('resize', update);
    }
    const observer = new ResizeObserver(update);
    observer.observe(container);
    return () => observer.disconnect();
  }, []);

  const renderedOpen = leftPanelOpen && !(dragging && temporarilyCollapsed);
  const style: WorkspaceShellStyle = {
    '--left-divider-width': leftPanelOpen ? '8px' : '0px',
    '--left-panel-width': renderedOpen ? `${effectiveWidth}px` : '0px',
  };
  const handleTransitionEnd = (event: TransitionEvent<HTMLDivElement>) => {
    if (event.currentTarget === event.target && event.propertyName === 'grid-template-columns') {
      setMotionActive(false);
    }
  };

  return (
    <div
      className="workspace-shell"
      data-left-sidebar-open={leftPanelOpen ? 'true' : 'false'}
      data-pane-motion={motionActive ? 'true' : 'false'}
      data-resizing={dragging && !motionActive ? 'true' : 'false'}
      data-temporarily-collapsed={temporarilyCollapsed ? 'true' : 'false'}
      onTransitionEnd={handleTransitionEnd}
      ref={containerRef}
      style={style}
    >
      <div
        aria-hidden={!renderedOpen ? 'true' : undefined}
        className="left-pane-slot"
        inert={!renderedOpen}
      >
        {left}
      </div>
      <CollapsiblePaneResizer
        className="left-panel-resizer"
        controls="app-sidebar"
        direction="left"
        disabled={!leftPanelOpen}
        label="调整左侧工作区宽度"
        maximum={maximum}
        minimum={MIN_LEFT_PANEL_WIDTH}
        onCollapse={onCloseLeftPanel}
        onDragStateChange={(nextDragging) => {
          setDragging(nextDragging);
          setDraftWidth(nextDragging ? leftPanelWidth : undefined);
          if (nextDragging) setMotionActive(false);
        }}
        onResize={setDraftWidth}
        onResizeCommit={(width) => {
          onLeftPanelWidthChange(width);
          onLeftPanelWidthCommit(width);
        }}
        onTemporaryCollapseChange={(collapsed) => {
          setTemporarilyCollapsed(collapsed);
          setMotionActive(true);
        }}
        width={effectiveWidth}
      />
      {children}
      {leftPanelOpen ? null : (
        <button
          aria-controls="app-sidebar"
          aria-expanded="false"
          aria-label="展开左侧边栏"
          className="icon-button left-sidebar-reopen"
          onClick={onOpenLeftPanel}
          type="button"
        >
          <AppIcon name="sidebar-left" />
        </button>
      )}
    </div>
  );
}
