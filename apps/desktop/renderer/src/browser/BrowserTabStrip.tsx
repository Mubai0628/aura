import { useEffect, useRef } from 'react';

import { MODEL_PAGE_ID, type BrowserPageState } from '../../../shared/browser-api';
import { AppIcon } from '../app-icons';
import { useScrollingTabRow } from '../ui/useScrollingTabRow';
import { useBrowserPage } from './browser-store';
import { browserTabs } from './browser-tabs';

/** What a page's tab says: its title, else its host; a blank page is a new tab. */
function pageTitle(state: BrowserPageState | undefined): string {
  if (state === undefined || state.url === 'about:blank' || state.url === '') return '新标签页';
  if (state.title !== '') return state.title;
  try {
    return new URL(state.url).host || state.url;
  } catch {
    return state.url;
  }
}

function PageTab({
  sessionId,
  pageId,
  selected,
}: {
  sessionId: string;
  pageId: string;
  selected: boolean;
}) {
  const state = useBrowserPage(sessionId, pageId);
  const model = pageId === MODEL_PAGE_ID;
  const title = pageTitle(state);
  return (
    <div
      className={`browser-tab${selected ? ' is-active' : ''}`}
      data-agent={state?.agentActive ? 'active' : undefined}
    >
      <button
        aria-label={model ? `模型的页面：${title}` : title}
        aria-selected={selected}
        className="browser-tab-main"
        onClick={() => browserTabs.show(sessionId, pageId)}
        role="tab"
        title={state?.url === 'about:blank' ? undefined : state?.url}
        type="button"
      >
        {state?.loading ? (
          <span aria-hidden="true" className="browser-tab-spinner" />
        ) : (
          <AppIcon name={model ? 'model' : 'browser'} size={13} />
        )}
        <span className="browser-tab-title">{title}</span>
      </button>
      <button
        aria-label={`关闭 ${title}`}
        className="browser-tab-close"
        onClick={() => browserTabs.close(sessionId, pageId)}
        type="button"
      >
        <AppIcon name="close" size={12} />
      </button>
    </div>
  );
}

/**
 * The browser's own row of pages — the model's first — with a new page, and
 * the button that lets the browser fill the main area. Pages narrow to 96 px while keeping
 * their titles, then the row scrolls sideways; its edges fade and the page shown stays in view.
 */
export function BrowserTabStrip({
  sessionId,
  pages,
  shown,
  expanded,
  onNewPage,
  onToggleExpanded,
}: {
  sessionId: string;
  pages: readonly string[];
  shown: string | undefined;
  expanded: boolean;
  onNewPage: () => void;
  onToggleExpanded?: (() => void) | undefined;
}) {
  const frame = useRef<HTMLDivElement>(null);
  const row = useRef<HTMLDivElement>(null);
  const overflow = useScrollingTabRow(row, frame, pages.join(' '));
  useEffect(() => {
    row.current
      ?.querySelector('.browser-tab.is-active')
      ?.scrollIntoView?.({ block: 'nearest', inline: 'nearest' });
  }, [shown, pages.length]);
  return (
    <div
      className="browser-tabs"
      data-overflow-left={overflow.left || undefined}
      data-overflow-right={overflow.right || undefined}
      ref={frame}
    >
      <div aria-label="浏览器页面" className="browser-tab-row" ref={row} role="tablist">
        {pages.map((page) => (
          <PageTab key={page} pageId={page} selected={page === shown} sessionId={sessionId} />
        ))}
      </div>
      <button
        aria-label="新建页面"
        className="icon-button browser-new-page"
        onClick={onNewPage}
        title="新建页面"
        type="button"
      >
        <AppIcon name="plus" size={15} />
      </button>
      {onToggleExpanded === undefined ? null : (
        <button
          aria-label={expanded ? '还原浏览器' : '放大浏览器'}
          aria-pressed={expanded}
          className="icon-button browser-expand"
          onClick={onToggleExpanded}
          title={expanded ? '还原浏览器' : '放大浏览器'}
          type="button"
        >
          <AppIcon name={expanded ? 'shrink' : 'expand'} size={15} />
        </button>
      )}
    </div>
  );
}
